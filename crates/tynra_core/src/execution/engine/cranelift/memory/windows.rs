use std::ffi::c_void;
use std::io;
use std::ptr::NonNull;

use super::Protection;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn VirtualAlloc(address: *mut c_void, size: usize, kind: u32, protection: u32) -> *mut c_void;
    fn VirtualProtect(address: *mut c_void, size: usize, protection: u32, old: *mut u32) -> i32;
    fn VirtualFree(address: *mut c_void, size: usize, kind: u32) -> i32;
    fn GetCurrentProcess() -> *mut c_void;
    fn FlushInstructionCache(process: *mut c_void, address: *const c_void, size: usize) -> i32;
}

pub(super) struct Pages {
    base: NonNull<c_void>,
    size: usize,
}

// SAFETY: A mapping is a uniquely owned OS resource independent of its creating
// thread. Only the JIT module writes it, exclusively before finalization. Moves
// do not relocate pages; destruction cannot overlap owner-borrowing invocation.
unsafe impl Send for Pages {}

impl Pages {
    pub(super) fn allocate(size: usize) -> io::Result<Self> {
        // SAFETY: No existing address is replaced. MEM_COMMIT | MEM_RESERVE and
        // PAGE_READWRITE request owned pages that have never been executable.
        let base = unsafe { VirtualAlloc(std::ptr::null_mut(), size, 0x3000, 0x04) };
        let base = NonNull::new(base).ok_or_else(io::Error::last_os_error)?;
        Ok(Self { base, size })
    }

    pub(super) fn as_mut_ptr(&mut self) -> *mut u8 {
        self.base.as_ptr().cast()
    }

    pub(super) fn finalize(self, protection: Protection) -> io::Result<Self> {
        let protection_flag = match protection {
            Protection::Execute => 0x20,  // PAGE_EXECUTE_READ, never write + execute
            Protection::ReadOnly => 0x02, // PAGE_READONLY
            Protection::Write => return Ok(self),
        };
        let mut old = 0;
        // SAFETY: Base and size describe the live allocation exclusively owned
        // by self. No machine-code call is permitted until finalization succeeds.
        if unsafe { VirtualProtect(self.base.as_ptr(), self.size, protection_flag, &mut old) } == 0 {
            return Err(io::Error::last_os_error());
        }
        if matches!(protection, Protection::Execute) {
            // SAFETY: Valid pseudo-handle and complete live code range.
            if unsafe { FlushInstructionCache(GetCurrentProcess(), self.base.as_ptr(), self.size) } == 0 {
                return Err(io::Error::last_os_error());
            }
        }
        Ok(self)
    }
}

impl Drop for Pages {
    fn drop(&mut self) {
        // SAFETY: This is the original VirtualAlloc base, uniquely owned. No
        // caller can execute code once its module owner begins destruction.
        let released = unsafe { VirtualFree(self.base.as_ptr(), 0, 0x8000) }; // MEM_RELEASE
        debug_assert_ne!(released, 0, "invalid owned VirtualAlloc mapping");
    }
}
