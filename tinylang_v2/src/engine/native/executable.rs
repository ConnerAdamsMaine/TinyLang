//! Windows code-page ownership: allocate RW, publish RX, never create a WX view.
//!
//! memmap2's Windows anonymous-map implementation initially creates a WX view.
//! Using VirtualAlloc directly here keeps the stricter RW -> RX invariant.

use std::ffi::c_void;
use std::ptr::NonNull;

use super::EngineError;

const MEM_COMMIT: u32 = 0x1000;
const MEM_RESERVE: u32 = 0x2000;
const MEM_RELEASE: u32 = 0x8000;
const PAGE_READWRITE: u32 = 0x04;
const PAGE_EXECUTE_READ: u32 = 0x20;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn VirtualAlloc(address: *mut c_void, size: usize, kind: u32, protect: u32) -> *mut c_void;
    fn VirtualProtect(address: *mut c_void, size: usize, protect: u32, old: *mut u32) -> i32;
    fn VirtualFree(address: *mut c_void, size: usize, kind: u32) -> i32;
    fn GetCurrentProcess() -> *mut c_void;
    fn FlushInstructionCache(process: *mut c_void, address: *const c_void, size: usize) -> i32;
}

struct Region {
    address: NonNull<u8>,
    length: usize,
}

impl Drop for Region {
    fn drop(&mut self) {
        // SAFETY: This is the original VirtualAlloc base, uniquely owned here.
        // MEM_RELEASE requires size zero and releases the complete reservation.
        unsafe { VirtualFree(self.address.as_ptr().cast(), 0, MEM_RELEASE) };
    }
}

struct WritableMemory(Region);

impl WritableMemory {
    fn allocate(length: usize) -> Result<Self, EngineError> {
        if length == 0 {
            return Err(EngineError::Codegen(
                "empty native code allocation".to_owned(),
            ));
        }
        // SAFETY: No requested address, valid nonzero size and documented flags.
        // The OS gives a fresh private RW (not executable) region or null.
        let address = unsafe {
            VirtualAlloc(
                std::ptr::null_mut(),
                length,
                MEM_COMMIT | MEM_RESERVE,
                PAGE_READWRITE,
            )
        };
        let address = NonNull::new(address.cast::<u8>())
            .ok_or_else(|| EngineError::Allocation(std::io::Error::last_os_error().to_string()))?;
        Ok(Self(Region { address, length }))
    }

    fn publish(self, bytes: &[u8]) -> Result<ExecutableMemory, EngineError> {
        if bytes.len() != self.0.length {
            return Err(EngineError::Codegen(
                "native allocation size mismatch".to_owned(),
            ));
        }
        // SAFETY: The uniquely owned RW allocation has exactly bytes.len()
        // accessible bytes. The source Vec cannot overlap this new OS region.
        unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), self.0.address.as_ptr(), bytes.len());
        }
        let mut previous = 0;
        // SAFETY: The complete live allocation changes from RW to RX. No
        // writable slice/pointer escapes, and failure drops/releases the region.
        let protected = unsafe {
            VirtualProtect(
                self.0.address.as_ptr().cast(),
                self.0.length,
                PAGE_EXECUTE_READ,
                &mut previous,
            )
        };
        if protected == 0 {
            return Err(EngineError::Codegen(format!(
                "VirtualProtect(RX): {}",
                std::io::Error::last_os_error()
            )));
        }
        // SAFETY: GetCurrentProcess returns a pseudo-handle needing no close.
        // Cache synchronization covers precisely the newly emitted RX code.
        let flushed = unsafe {
            FlushInstructionCache(
                GetCurrentProcess(),
                self.0.address.as_ptr().cast(),
                self.0.length,
            )
        };
        if flushed == 0 {
            return Err(EngineError::Codegen(format!(
                "FlushInstructionCache: {}",
                std::io::Error::last_os_error()
            )));
        }
        Ok(ExecutableMemory(self.0))
    }
}

pub(super) struct ExecutableMemory(Region);

// SAFETY: Publication consumes all writable access. The private region remains
// immutable RX until its final owner drops, independent of the invoking thread.
unsafe impl Send for ExecutableMemory {}
// SAFETY: Concurrent calls only read code; each call owns its separate frame.
unsafe impl Sync for ExecutableMemory {}

impl ExecutableMemory {
    pub(super) fn from_emitted(bytes: &[u8]) -> Result<Self, EngineError> {
        WritableMemory::allocate(bytes.len())?.publish(bytes)
    }

    pub(super) fn as_ptr(&self) -> *const u8 {
        self.0.address.as_ptr()
    }

    pub(super) fn bytes(&self) -> &[u8] {
        // SAFETY: The mapping is readable for its full length, initialized
        // before RX publication, immutable, and lives at least as long as self.
        unsafe { std::slice::from_raw_parts(self.as_ptr(), self.0.length) }
    }
}
