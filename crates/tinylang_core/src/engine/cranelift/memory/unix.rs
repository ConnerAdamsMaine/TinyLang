use std::io;

use memmap2::{Mmap, MmapMut};

use super::Protection;

pub(super) enum Pages {
    Writable(MmapMut),
    Finalized(Mmap),
}

impl Pages {
    pub(super) fn allocate(size: usize) -> io::Result<Self> {
        // Unix anonymous MmapMut starts RW, unlike memmap2's Windows RWX section.
        MmapMut::map_anon(size).map(Self::Writable)
    }

    pub(super) fn as_mut_ptr(&mut self) -> *mut u8 {
        match self {
            Self::Writable(map) => map.as_mut_ptr(),
            Self::Finalized(map) => map.as_ptr().cast_mut(),
        }
    }

    pub(super) fn finalize(self, protection: Protection) -> io::Result<Self> {
        match (self, protection) {
            (Self::Writable(map), Protection::Execute) => {
                // This backend is limited to coherent-cache x86-64 hosts.
                map.make_exec().map(Self::Finalized)
            }
            (Self::Writable(map), Protection::ReadOnly) => map.make_read_only().map(Self::Finalized),
            (pages, _) => Ok(pages),
        }
    }
}
