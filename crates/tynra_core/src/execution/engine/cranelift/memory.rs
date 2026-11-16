//! RAII mappings avoid Cranelift 0.125's leaking default Windows allocator.

use std::io;

use cranelift_jit::{BranchProtection, JITMemoryProvider};
use cranelift_module::{ModuleError, ModuleResult};
#[cfg(not(windows))]
mod unix;
#[cfg(windows)]
mod windows;

#[cfg(not(windows))]
use unix::Pages;
#[cfg(windows)]
use windows::Pages;

#[derive(Clone, Copy)]
pub(super) enum Protection {
    Execute,
    ReadOnly,
    Write,
}

enum Mapping {
    Pending(Pages, Protection),
    Ready(Pages),
}

#[derive(Default)]
pub(super) struct MappedMemory {
    mappings: Vec<Mapping>,
}

impl MappedMemory {
    fn allocate(&mut self, size: usize, align: u64, protection: Protection) -> io::Result<*mut u8> {
        let align = usize::try_from(align).map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
        if !align.is_power_of_two() || size == 0 {
            return Err(io::Error::from(io::ErrorKind::InvalidInput));
        }
        let size = size
            .checked_add(align - 1)
            .ok_or_else(|| io::Error::from(io::ErrorKind::OutOfMemory))?;
        self.mappings.try_reserve(1).map_err(io::Error::other)?;
        let mut map = Pages::allocate(size)?;
        let base = map.as_mut_ptr();
        let offset = (align - base as usize % align) % align;
        // SAFETY: Added align-1 bytes above; offset is strictly less than align.
        let aligned = unsafe { base.add(offset) };
        self.mappings.push(Mapping::Pending(map, protection));
        Ok(aligned)
    }
}

impl JITMemoryProvider for MappedMemory {
    fn allocate_readexec(&mut self, size: usize, align: u64) -> io::Result<*mut u8> {
        self.allocate(size, align, Protection::Execute)
    }

    fn allocate_readwrite(&mut self, size: usize, align: u64) -> io::Result<*mut u8> {
        self.allocate(size, align, Protection::Write)
    }

    fn allocate_readonly(&mut self, size: usize, align: u64) -> io::Result<*mut u8> {
        self.allocate(size, align, Protection::ReadOnly)
    }

    unsafe fn free_memory(&mut self) {
        // The trait caller guarantees no code is executing and pointers are dead.
        self.mappings.clear();
    }

    fn finalize(&mut self, branch_protection: BranchProtection) -> ModuleResult<()> {
        let failure = |err| ModuleError::Allocation {
            message: "JIT mapping finalization",
            err,
        };
        if branch_protection != BranchProtection::None {
            return Err(failure(io::Error::new(
                io::ErrorKind::Unsupported,
                "branch protection is unsupported on this x86-64 provider",
            )));
        }
        let mut finalized = Vec::new();
        finalized
            .try_reserve_exact(self.mappings.len())
            .map_err(|error| failure(io::Error::other(error)))?;
        for mapping in self.mappings.drain(..) {
            let mapping = match mapping {
                Mapping::Pending(map, protection) => Mapping::Ready(map.finalize(protection).map_err(failure)?),
                Mapping::Ready(map) => Mapping::Ready(map),
            };
            finalized.push(mapping);
        }
        self.mappings = finalized;
        Ok(())
    }
}
