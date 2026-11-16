//! Linux code-page ownership: mmap RW, publish RX, never create a WX view.
//!
//! Anonymous `mmap(PROT_READ | PROT_WRITE)` followed by `mprotect` to
//! `PROT_READ | PROT_EXEC` keeps the same stricter RW -> RX invariant the
//! Windows implementation enforces with VirtualAlloc/VirtualProtect.

use std::ptr::NonNull;

use super::super::super::EngineError;

struct Region {
    address: NonNull<u8>,
    length: usize,
}

impl Drop for Region {
    fn drop(&mut self) {
        // SAFETY: This is the original mmap base, uniquely owned here. munmap
        // releases the complete mapping and its reservation.
        unsafe { libc::munmap(self.address.as_ptr().cast(), self.length) };
    }
}

struct WritableMemory(Region);

impl WritableMemory {
    fn allocate(length: usize) -> Result<Self, EngineError> {
        if length == 0 {
            return Err(EngineError::Codegen("empty native code allocation".to_owned()));
        }
        // SAFETY: No requested address, valid nonzero size, private anonymous
        // mapping. The kernel gives a fresh private RW (not executable) region
        // or MAP_FAILED.
        let address = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                length,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                -1,
                0,
            )
        };
        if address == libc::MAP_FAILED {
            return Err(EngineError::Allocation(std::io::Error::last_os_error().to_string()));
        }
        // SAFETY: mmap returned MAP_SUCCESS with `length` accessible bytes.
        let address = NonNull::new(address.cast::<u8>()).ok_or_else(|| {
            EngineError::Codegen("kernel returned a null mapping for a nonzero size".to_owned())
        })?;
        Ok(Self(Region { address, length }))
    }

    fn publish(self, bytes: &[u8]) -> Result<ExecutableMemory, EngineError> {
        if bytes.len() != self.0.length {
            return Err(EngineError::Codegen("native allocation size mismatch".to_owned()));
        }
        // SAFETY: The uniquely owned RW allocation has exactly bytes.len()
        // accessible bytes. The source Vec cannot overlap this new OS region.
        unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), self.0.address.as_ptr(), bytes.len());
        }
        // SAFETY: The complete live allocation changes from RW to RX. No
        // writable slice/pointer escapes, and failure drops/unmaps the region.
        let protected =
            unsafe { libc::mprotect(self.0.address.as_ptr().cast(), self.0.length, libc::PROT_READ | libc::PROT_EXEC) };
        if protected != 0 {
            return Err(EngineError::Codegen(format!("mprotect(PROT_EXEC): {}", std::io::Error::last_os_error())));
        }
        // No cache flush is required: x86-64 keeps instruction and data caches
        // coherent for stores made before the RW -> RX transition.
        Ok(ExecutableMemory(self.0))
    }
}

pub(in super::super) struct ExecutableMemory(Region);

// SAFETY: Publication consumes all writable access. The private region remains
// immutable RX until its final owner drops, independent of the invoking thread.
unsafe impl Send for ExecutableMemory {}
// SAFETY: Concurrent calls only read code; each call owns its separate frame.
unsafe impl Sync for ExecutableMemory {}

impl ExecutableMemory {
    pub(in super::super) fn from_emitted(bytes: &[u8]) -> Result<Self, EngineError> {
        WritableMemory::allocate(bytes.len())?.publish(bytes)
    }

    pub(in super::super) fn as_ptr(&self) -> *const u8 {
        self.0.address.as_ptr()
    }

    pub(in super::super) fn bytes(&self) -> &[u8] {
        // SAFETY: The mapping is readable for its full length, initialized
        // before RX publication, immutable, and lives at least as long as self.
        unsafe { std::slice::from_raw_parts(self.as_ptr(), self.0.length) }
    }
}
