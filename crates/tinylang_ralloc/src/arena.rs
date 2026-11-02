//! Arena-local allocation state and block mutation.

use core::ptr::NonNull;

use crate::backend::MemoryRegion;
use crate::block::{ALIGNMENT, BlockHeader, HEADER_SIZE, MIN_BLOCK_SIZE, align_up, request_to_block_size};
use crate::region::{self, ArenaSlot};

/// Arena allocation failure or pointer validation error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ArenaError {
    /// The backend-provided region is not usable as an arena span.
    InvalidRegion,
    /// Region registration failed.
    RegionRegistrationFailed,
    /// No free block can satisfy the request.
    OutOfMemory,
    /// The pointer is not a live allocation in this arena.
    InvalidPointer,
}

/// Single arena backed by one contiguous memory span.
pub(crate) struct Arena {
    base:          NonNull<u8>,
    len:           usize,
    stats:         ArenaStats,
    search_prefix: SearchPrefix,
}

/// A known block boundary and an upper bound on every free block before it.
/// Deallocation can shorten the prefix without recomputing the size bound;
/// overestimating it only causes a future request to rescan from the base.
#[derive(Clone, Copy, Default, Eq, PartialEq)]
struct SearchPrefix {
    end_offset:       usize,
    free_block_bound: usize,
}

impl SearchPrefix {
    fn for_request(self, needed: usize) -> Self {
        // A strictly larger request cannot fit any earlier free block,
        // regardless of alignment. Equal or smaller requests must revisit it.
        if needed > self.free_block_bound {
            self
        } else {
            Self::default()
        }
    }
}

// SAFETY: The arena exclusively manages its backend region's metadata.
// Moving this owner does not move its backing bytes or grant concurrent
// access. Production arenas live behind their respective ARENAS spin locks;
// allocated payloads are disjoint from all live metadata and other payloads.
unsafe impl Send for Arena {}

/// Per-arena allocation counters for internal diagnostics.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct ArenaStats {
    current_allocations:   usize,
    current_payload_bytes: usize,
    peak_payload_bytes:    usize,
}

/// Test-only result of walking an arena's block metadata.
#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ArenaIntegritySnapshot {
    block_count:      usize,
    live_block_count: usize,
}

#[cfg(test)]
#[allow(dead_code)]
impl ArenaIntegritySnapshot {
    /// Returns the number of blocks reached by the arena metadata walk.
    pub(crate) const fn block_count(self) -> usize {
        self.block_count
    }

    /// Returns the number of allocated blocks reached by the arena metadata walk.
    pub(crate) const fn live_block_count(self) -> usize {
        self.live_block_count
    }
}

#[allow(dead_code)]
impl ArenaStats {
    /// Returns the number of live allocations in this arena.
    pub(crate) const fn current_allocations(self) -> usize {
        self.current_allocations
    }

    /// Returns the total usable payload bytes currently live in this arena.
    pub(crate) const fn current_payload_bytes(self) -> usize {
        self.current_payload_bytes
    }

    /// Returns the peak live usable payload bytes observed in this arena.
    pub(crate) const fn peak_payload_bytes(self) -> usize {
        self.peak_payload_bytes
    }

    fn record_allocate(&mut self, payload_bytes: usize) {
        self.current_allocations = self.current_allocations.saturating_add(1);
        self.current_payload_bytes = self.current_payload_bytes.saturating_add(payload_bytes);
        self.peak_payload_bytes = self.peak_payload_bytes.max(self.current_payload_bytes);
    }

    fn record_deallocate(&mut self, payload_bytes: usize) {
        self.current_allocations = self.current_allocations.saturating_sub(1);
        self.current_payload_bytes = self.current_payload_bytes.saturating_sub(payload_bytes);
    }
}

impl Arena {
    /// Initializes an arena over a backend-provided region.
    pub(crate) fn from_region(region: MemoryRegion, slot: ArenaSlot, generation: usize) -> Result<Self, ArenaError> {
        let start = region.as_ptr().addr();
        let len = region.len();

        if start & (ALIGNMENT - 1) != 0 || len < MIN_BLOCK_SIZE || len & (ALIGNMENT - 1) != 0 {
            return Err(ArenaError::InvalidRegion);
        }

        region::register(region::RegionRecord::new(start, len, slot, generation).ok_or(ArenaError::InvalidRegion)?)
            .map_err(|_| ArenaError::RegionRegistrationFailed)?;

        let header = region.as_ptr().cast::<BlockHeader>();
        // SAFETY: `start` is aligned, `len >= MIN_BLOCK_SIZE`, and this arena
        // owns the newly registered span, so writing the initial free header is
        // the first metadata mutation inside the region.
        unsafe { header.write(BlockHeader::new_free(len)) };

        Ok(Self {
            base: region.as_non_null(),
            len,
            stats: ArenaStats::default(),
            search_prefix: SearchPrefix::default(),
        })
    }

    /// Allocates `size` payload bytes from this arena.
    #[allow(dead_code)]
    pub(crate) fn allocate(&mut self, size: usize) -> Result<*mut u8, ArenaError> {
        self.allocate_aligned(size, ALIGNMENT)
    }

    /// Allocates `size` payload bytes with a requested payload alignment.
    pub(crate) fn allocate_aligned(&mut self, size: usize, align: usize) -> Result<*mut u8, ArenaError> {
        if align == 0 || !align.is_power_of_two() {
            return Err(ArenaError::OutOfMemory);
        }
        let align = align.max(ALIGNMENT);
        let needed = request_to_block_size(size).ok_or(ArenaError::OutOfMemory)?;
        let mut prefix = self.search_prefix.for_request(needed);
        let mut current = self.base.as_ptr().addr() + prefix.end_offset;
        let end = self.end();

        while current < end {
            #[cfg(any(test, feature = "testing-hooks"))]
            crate::instrumentation::record_allocation_probe();
            let header = self.header_at(current);
            // SAFETY: `current` walks block starts created by this arena inside
            // `start..end`, so reading the block header is within the span.
            let block = unsafe { &mut *header };
            let block_size = block.size();

            if !block.is_allocated() {
                let Some((allocation_header, available_size)) =
                    self.aligned_allocation_slot(current, block_size, needed, align)
                else {
                    prefix.free_block_bound = prefix.free_block_bound.max(block_size);
                    current = current.checked_add(block_size).ok_or(ArenaError::InvalidRegion)?;
                    continue;
                };

                if allocation_header.addr() != current {
                    let leading_size = allocation_header
                        .addr()
                        .checked_sub(current)
                        .ok_or(ArenaError::InvalidRegion)?;
                    prefix.free_block_bound = prefix.free_block_bound.max(leading_size);
                    // SAFETY: `leading_size` is an aligned usable block inside
                    // the original free block and ends at `allocation_header`.
                    unsafe { header.write(BlockHeader::new_free(leading_size)) };
                }

                Self::allocate_from_block(allocation_header, available_size, needed);
                // SAFETY: The selected header now describes a live block.
                let allocated = unsafe { &*allocation_header };
                // Derive the payload from the backing region, not a reference
                // whose borrow only covers the header (and is read-only).
                let payload = self.base.as_ptr().with_addr(allocation_header.addr() + HEADER_SIZE);
                // Stop the proven prefix at this header, not beyond it. A
                // repeated allocate/free of the same slot can keep the hint
                // stationary; retained growth revisits this allocated header.
                prefix.end_offset = allocation_header.addr() - self.base.as_ptr().addr();
                if prefix != self.search_prefix {
                    self.search_prefix = prefix;
                }
                self.stats.record_allocate(allocated.size().saturating_sub(HEADER_SIZE));
                #[cfg(any(test, feature = "testing-hooks"))]
                crate::instrumentation::record_allocation(allocated.size().saturating_sub(HEADER_SIZE));
                return Ok(payload);
            }

            current = current.checked_add(block_size).ok_or(ArenaError::InvalidRegion)?;
        }

        prefix.end_offset = self.len;
        self.search_prefix = prefix;
        Err(ArenaError::OutOfMemory)
    }

    /// Frees a payload pointer previously returned by this arena.
    pub(crate) fn deallocate(&mut self, payload: *mut u8) -> Result<(), ArenaError> {
        if payload.is_null() {
            return Ok(());
        }

        let payload_addr = payload.addr();
        if payload_addr < self.base.as_ptr().addr() || payload_addr >= self.end() {
            return Err(ArenaError::InvalidPointer);
        }

        let header = self.find_live_header(payload)?;
        // SAFETY: `find_live_header` only returns known block starts inside
        // this arena for exact live payload pointers.
        let block = unsafe { &mut *header };
        let payload_bytes = block.size().saturating_sub(HEADER_SIZE);

        block.mark_free();
        self.stats.record_deallocate(payload_bytes);
        #[cfg(any(test, feature = "testing-hooks"))]
        crate::instrumentation::record_deallocation(payload_bytes);
        let free_header = self.coalesce(header);
        // Coalescing may remove the old boundary or grow a previously smaller
        // hole before it. Revisit the entire resulting free block next time.
        let free_offset = free_header.addr() - self.base.as_ptr().addr();
        if free_offset < self.search_prefix.end_offset {
            self.search_prefix.end_offset = free_offset;
        }

        Ok(())
    }

    /// Returns usable payload bytes for a live allocation.
    pub(crate) fn allocation_size(&self, payload: *mut u8) -> Result<usize, ArenaError> {
        if payload.is_null() {
            return Err(ArenaError::InvalidPointer);
        }

        let payload_addr = payload.addr();
        if payload_addr < self.base.as_ptr().addr() || payload_addr >= self.end() {
            return Err(ArenaError::InvalidPointer);
        }

        let header = self.find_live_header(payload)?;
        // SAFETY: `find_live_header` only returns known block starts inside
        // this arena for exact live payload pointers.
        let block = unsafe { &*header };

        block.size().checked_sub(HEADER_SIZE).ok_or(ArenaError::InvalidPointer)
    }

    /// Returns a snapshot of internal arena counters.
    #[allow(dead_code)]
    pub(crate) const fn stats(&self) -> ArenaStats {
        self.stats
    }

    /// Walks block metadata and reports a test-only arena integrity snapshot.
    #[cfg(test)]
    pub(crate) fn integrity_snapshot(&self) -> Result<ArenaIntegritySnapshot, ArenaError> {
        if self.search_prefix.end_offset > self.len {
            return Err(ArenaError::InvalidRegion);
        }
        let prefix_end = self.base.as_ptr().addr() + self.search_prefix.end_offset;
        let mut current = self.base.as_ptr().addr();
        let end = self.end();
        let mut block_count = 0usize;
        let mut live_block_count = 0usize;

        while current < end {
            let header = self.header_at(current);
            // SAFETY: `current` starts at the arena base and advances only by
            // block sizes accepted by `is_valid_block_step`.
            let block = unsafe { &*header };
            let block_size = block.size();
            if !is_valid_block_step(current, block_size, end) {
                return Err(ArenaError::InvalidRegion);
            }
            if current < prefix_end
                && (current + block_size > prefix_end
                    || (!block.is_allocated() && block_size > self.search_prefix.free_block_bound))
            {
                return Err(ArenaError::InvalidRegion);
            }

            block_count = block_count.checked_add(1).ok_or(ArenaError::InvalidRegion)?;
            if block.is_allocated() {
                live_block_count = live_block_count.checked_add(1).ok_or(ArenaError::InvalidRegion)?;
            }

            current = current.checked_add(block_size).ok_or(ArenaError::InvalidRegion)?;
        }

        if current != end {
            return Err(ArenaError::InvalidRegion);
        }

        Ok(ArenaIntegritySnapshot {
            block_count,
            live_block_count,
        })
    }

    fn find_live_header(&self, payload: *mut u8) -> Result<*mut BlockHeader, ArenaError> {
        let payload_addr = payload.addr();
        let mut current = self.base.as_ptr().addr();
        let end = self.end();

        while current < end {
            #[cfg(any(test, feature = "testing-hooks"))]
            crate::instrumentation::record_lookup_probe();
            let header = self.header_at(current);
            // SAFETY: `current` is either the arena start or the next address
            // reached from a previously validated block size.
            let block = unsafe { &*header };
            let block_size = block.size();
            if !is_valid_block_step(current, block_size, end) {
                return Err(ArenaError::InvalidRegion);
            }

            if block.is_allocated() && current.checked_add(HEADER_SIZE) == Some(payload_addr) {
                return Ok(header);
            }

            current = current.checked_add(block_size).ok_or(ArenaError::InvalidRegion)?;
        }

        Err(ArenaError::InvalidPointer)
    }

    fn allocate_from_block(header: *mut BlockHeader, block_size: usize, needed: usize) {
        if let Some((head_size, tail_size)) = BlockHeader::split_sizes(block_size, needed) {
            // SAFETY: `head_size + tail_size == block_size`, and both sizes are
            // aligned usable blocks within the original free block.
            unsafe {
                header.write(BlockHeader::new_free(head_size));
                (*header).mark_allocated();
                header
                    .cast::<u8>()
                    .add(head_size)
                    .cast::<BlockHeader>()
                    .write(BlockHeader::new_free(tail_size));
            }
        } else {
            // SAFETY: The selected aligned slot has `block_size` available
            // bytes within the original free block. Its header may overlap
            // former payload, so initialize it before marking it allocated.
            unsafe {
                header.write(BlockHeader::new_free(block_size));
                (*header).mark_allocated();
            }
        }
    }

    fn aligned_allocation_slot(
        &self,
        block_start: usize,
        block_size: usize,
        needed: usize,
        align: usize,
    ) -> Option<(*mut BlockHeader, usize)> {
        if block_size < needed {
            return None;
        }

        let block_end = block_start.checked_add(block_size)?;
        let mut header_addr = align_up(block_start.checked_add(HEADER_SIZE)?, align)?.checked_sub(HEADER_SIZE)?;

        if header_addr < block_start {
            return None;
        }

        let mut leading_size = header_addr.checked_sub(block_start)?;
        if leading_size > 0 && leading_size < MIN_BLOCK_SIZE {
            header_addr = header_addr.checked_add(align)?;
            leading_size = header_addr.checked_sub(block_start)?;
        }

        if leading_size > 0 && leading_size & (ALIGNMENT - 1) != 0 {
            return None;
        }
        if leading_size > 0 && leading_size < MIN_BLOCK_SIZE {
            return None;
        }

        let allocation_end = header_addr.checked_add(needed)?;
        if allocation_end > block_end {
            return None;
        }

        let available_size = block_end.checked_sub(header_addr)?;
        Some((self.header_at(header_addr), available_size))
    }

    fn coalesce(&mut self, header: *mut BlockHeader) -> *mut BlockHeader {
        let header = self.coalesce_with_next(header);

        if let Some(previous) = self.previous_block(header) {
            // SAFETY: `previous` is a valid block start found by scanning this
            // arena, and `header` is the next block being freed.
            let previous_block = unsafe { &mut *previous };
            // SAFETY: `header` is a valid block pointer in this arena.
            let current_block = unsafe { &mut *header };

            if !previous_block.is_allocated()
                && let Some(size) = BlockHeader::coalesced_size(previous_block.size(), current_block.size())
            {
                // SAFETY: The two free blocks are adjacent by construction
                // from `previous_block`; replacing the left header expands
                // it over the right block.
                unsafe { previous.write(BlockHeader::new_free(size)) };
                return self.coalesce_with_next(previous);
            }
        }

        header
    }

    fn coalesce_with_next(&self, header: *mut BlockHeader) -> *mut BlockHeader {
        // SAFETY: `header` points to a block in this arena.
        let block = unsafe { &mut *header };
        let next_addr = header.addr() + block.size();

        if next_addr >= self.end() {
            return header;
        }

        let next = self.header_at(next_addr);
        // SAFETY: `next_addr` is inside the arena and follows the current
        // block size, so it is the next block header.
        let next_block = unsafe { &mut *next };
        if next_block.is_allocated() {
            return header;
        }

        if let Some(size) = BlockHeader::coalesced_size(block.size(), next_block.size()) {
            // SAFETY: The current and next blocks are adjacent free blocks.
            unsafe { header.write(BlockHeader::new_free(size)) };
        }

        header
    }

    fn previous_block(&self, header: *mut BlockHeader) -> Option<*mut BlockHeader> {
        let mut current = self.base.as_ptr().addr();
        let target = header.addr();
        let mut previous = None;

        while current < target {
            #[cfg(any(test, feature = "testing-hooks"))]
            crate::instrumentation::record_coalesce_probe();
            let current_header = self.header_at(current);
            previous = Some(current_header);
            // SAFETY: `current` walks known block starts before `target`.
            let size = unsafe { (*current_header).size() };
            current = current.checked_add(size)?;
        }

        (current == target).then_some(previous?)
    }

    fn header_at(&self, address: usize) -> *mut BlockHeader {
        self.base.as_ptr().with_addr(address).cast::<BlockHeader>()
    }

    fn end(&self) -> usize {
        self.base.as_ptr().addr() + self.len
    }
}

fn is_valid_block_step(current: usize, block_size: usize, end: usize) -> bool {
    block_size >= MIN_BLOCK_SIZE
        && block_size & (ALIGNMENT - 1) == 0
        && current.checked_add(block_size).is_some_and(|next| next <= end)
}

impl Drop for Arena {
    fn drop(&mut self) {
        let _ = region::remove(self.base.as_ptr().addr(), self.len);
    }
}

#[cfg(test)]
mod tests {
    mod first_fit;

    use super::Arena;
    use crate::backend::{FixedRegionBackend, MemoryBackend, RegionRequest};
    use crate::block::BlockHeader;
    use crate::region::{self, ArenaSlot};

    #[repr(align(64))]
    struct AlignedStorage([u8; 512]);

    fn test_arena(storage: &mut AlignedStorage) -> Arena {
        region::clear_for_tests();
        let mut backend = FixedRegionBackend::new(storage.0.as_mut_ptr(), storage.0.len());
        let region = backend
            .allocate_region(RegionRequest::new(storage.0.len(), 64).unwrap())
            .expect("test region should fit");

        Arena::from_region(region, ArenaSlot::new(0), 0).expect("arena should initialize")
    }

    #[test]
    fn arena_allocates_and_reuses_freed_blocks() {
        let _guard = region::TEST_LOCK.lock();
        let mut storage = AlignedStorage([0; 512]);
        let mut arena = test_arena(&mut storage);

        let first = arena.allocate(24).expect("allocation should succeed");
        arena.deallocate(first).expect("free should succeed");
        let second = arena.allocate(24).expect("allocation should reuse space");

        assert_eq!(first, second);
    }

    #[test]
    fn arena_coalesces_adjacent_free_blocks() {
        let _guard = region::TEST_LOCK.lock();
        let mut storage = AlignedStorage([0; 512]);
        let mut arena = test_arena(&mut storage);

        let first = arena.allocate(96).expect("first allocation should succeed");
        let second = arena.allocate(96).expect("second allocation should succeed");

        arena.deallocate(first).expect("first free should succeed");
        arena.deallocate(second).expect("second free should succeed");

        let combined = arena
            .allocate(208)
            .expect("coalesced free blocks should satisfy larger request");
        assert_eq!(combined, first);
    }

    #[test]
    fn arena_rejects_in_region_non_payload_pointers() {
        let _guard = region::TEST_LOCK.lock();
        let mut storage = AlignedStorage([0; 512]);
        let mut arena = test_arena(&mut storage);

        let allocation = arena.allocate(64).expect("allocation should succeed");
        let non_payload = allocation.wrapping_sub(1);

        assert_eq!(arena.deallocate(non_payload), Err(super::ArenaError::InvalidPointer));
        assert_eq!(arena.allocation_size(non_payload), Err(super::ArenaError::InvalidPointer));

        arena
            .deallocate(allocation)
            .expect("original allocation should remain live");
    }

    #[test]
    fn arena_stats_track_current_and_peak_payload_bytes() {
        let _guard = region::TEST_LOCK.lock();
        let mut storage = AlignedStorage([0; 512]);
        let mut arena = test_arena(&mut storage);

        assert_eq!(arena.stats().current_allocations(), 0);
        assert_eq!(arena.stats().current_payload_bytes(), 0);
        assert_eq!(arena.stats().peak_payload_bytes(), 0);

        let first = arena.allocate(24).expect("first allocation should succeed");
        let first_size = arena
            .allocation_size(first)
            .expect("first allocation should have a size");
        assert_eq!(arena.stats().current_allocations(), 1);
        assert_eq!(arena.stats().current_payload_bytes(), first_size);
        assert_eq!(arena.stats().peak_payload_bytes(), first_size);

        let second = arena.allocate(64).expect("second allocation should succeed");
        let second_size = arena
            .allocation_size(second)
            .expect("second allocation should have a size");
        assert_eq!(arena.stats().current_allocations(), 2);
        assert_eq!(arena.stats().current_payload_bytes(), first_size + second_size);
        assert_eq!(arena.stats().peak_payload_bytes(), first_size + second_size);

        arena.deallocate(first).expect("first free should succeed");
        assert_eq!(arena.stats().current_allocations(), 1);
        assert_eq!(arena.stats().current_payload_bytes(), second_size);
        assert_eq!(arena.stats().peak_payload_bytes(), first_size + second_size);

        arena.deallocate(second).expect("second free should succeed");
        assert_eq!(arena.stats().current_allocations(), 0);
        assert_eq!(arena.stats().current_payload_bytes(), 0);
        assert_eq!(arena.stats().peak_payload_bytes(), first_size + second_size);
    }

    #[test]
    fn arena_allocate_aligned_returns_requested_payload_alignment() {
        let _guard = region::TEST_LOCK.lock();
        let mut storage = AlignedStorage([0; 512]);
        let mut arena = test_arena(&mut storage);

        let payload = arena
            .allocate_aligned(24, 64)
            .expect("aligned allocation should succeed");

        assert_eq!(payload.addr() % 64, 0);
        assert!(arena.allocation_size(payload).is_ok());

        arena.deallocate(payload).expect("aligned allocation should free");
    }

    #[test]
    fn first_fit_keeps_small_early_holes_after_larger_requests() {
        let _guard = region::TEST_LOCK.lock();
        let mut storage = AlignedStorage([0; 512]);
        let mut arena = test_arena(&mut storage);
        let first = arena.allocate(64).expect("first block");
        let second = arena.allocate(64).expect("second block");
        let third = arena.allocate(64).expect("third block");
        arena.deallocate(first).expect("early hole");

        let large = arena.allocate(128).expect("request beyond the small hole");
        assert!(large.addr() > third.addr());
        let small = arena.allocate(32).expect("small request must still see the early hole");
        assert_eq!(small, first);
        assert_eq!(arena.integrity_snapshot().expect("valid block walk").live_block_count(), 4);
        for payload in [second, large, small, third] {
            arena.deallocate(payload).expect("release live block");
        }
        assert_eq!(arena.integrity_snapshot().expect("coalesced arena").block_count(), 1);
        assert_eq!(arena.stats().current_allocations(), 0);
    }

    #[test]
    fn first_fit_preserves_overaligned_leading_space() {
        let _guard = region::TEST_LOCK.lock();
        for alignment in [64, 128, 256] {
            let mut storage = AlignedStorage([0; 512]);
            let mut arena = test_arena(&mut storage);
            let aligned = arena.allocate_aligned(32, alignment).expect("overaligned block");
            assert_eq!(aligned.addr() % alignment, 0);
            let leading = arena.allocate(16).expect("leading alignment space stays available");
            assert!(leading.addr() < aligned.addr());
            assert_eq!(leading.addr(), arena.base.as_ptr().addr() + super::HEADER_SIZE);

            arena.deallocate(aligned).expect("free aligned block");
            arena.deallocate(leading).expect("coalesce leading block");
            assert_eq!(arena.integrity_snapshot().expect("valid coalescing").block_count(), 1);
            let whole = arena
                .allocate(512 - super::HEADER_SIZE)
                .expect("all alignment padding reclaimed");
            assert_eq!(whole, leading);
            arena.deallocate(whole).expect("release whole region");
        }
    }

    #[test]
    fn first_fit_recovers_after_full_region_and_rejected_requests() {
        let _guard = region::TEST_LOCK.lock();
        let mut storage = AlignedStorage([0; 512]);
        let mut arena = test_arena(&mut storage);
        let whole = arena.allocate(512 - super::HEADER_SIZE).expect("whole region");
        assert_eq!(arena.allocate(1), Err(super::ArenaError::OutOfMemory));
        assert_eq!(arena.allocate(usize::MAX), Err(super::ArenaError::OutOfMemory));
        assert_eq!(arena.allocate_aligned(1, 3), Err(super::ArenaError::OutOfMemory));
        assert_eq!(arena.deallocate(whole.wrapping_add(1)), Err(super::ArenaError::InvalidPointer));
        assert_eq!(arena.allocate(1), Err(super::ArenaError::OutOfMemory));

        arena.deallocate(whole).expect("return full allocation");
        let recovered = arena
            .allocate(512 - super::HEADER_SIZE)
            .expect("recover the full region");
        assert_eq!(recovered, whole);
        arena.deallocate(recovered).expect("release recovered region");
        assert_eq!(arena.integrity_snapshot().expect("valid metadata").block_count(), 1);
        std::eprintln!(
            "allocation-scan-layout arena_bytes={} optional_arena_bytes={} block_header_bytes={}",
            size_of::<Arena>(),
            size_of::<Option<Arena>>(),
            super::HEADER_SIZE,
        );
    }

    #[test]
    fn first_fit_reuse_keeps_search_prefix_stationary() {
        let _guard = region::TEST_LOCK.lock();
        for retained_bytes in [0, 64] {
            let mut storage = AlignedStorage([0; 512]);
            let mut arena = test_arena(&mut storage);
            let retained = if retained_bytes == 0 {
                None
            } else {
                Some(arena.allocate(retained_bytes).expect("retained prefix"))
            };
            let reusable = arena.allocate(64).expect("first reusable allocation");
            arena.deallocate(reusable).expect("seed the reusable hole");
            let prefix = arena.search_prefix;

            for _ in 0..16 {
                let allocation = arena.allocate(64).expect("reuse the same first-fit slot");
                assert_eq!(allocation, reusable);
                assert_eq!(arena.search_prefix.end_offset, prefix.end_offset);
                assert_eq!(arena.search_prefix.free_block_bound, prefix.free_block_bound);
                arena.deallocate(allocation).expect("release the reusable slot");
                assert_eq!(arena.search_prefix.end_offset, prefix.end_offset);
                assert_eq!(arena.search_prefix.free_block_bound, prefix.free_block_bound);
                arena.integrity_snapshot().expect("valid prefix proof after reuse");
            }
            if let Some(retained) = retained {
                arena.deallocate(retained).expect("release the retained prefix");
            }
            assert_eq!(arena.integrity_snapshot().expect("coalesced region").block_count(), 1);
            assert_eq!(arena.stats().current_payload_bytes(), 0);
        }
    }

    #[test]
    fn first_fit_rewinds_through_split_holes_and_backward_coalescing() {
        let _guard = region::TEST_LOCK.lock();
        let mut storage = AlignedStorage([0; 512]);
        let mut arena = test_arena(&mut storage);
        let first = arena.allocate(64).expect("first block");
        let second = arena.allocate(64).expect("second block");
        let third = arena.allocate(64).expect("third block");
        let fourth = arena.allocate(64).expect("fourth block");
        arena.deallocate(second).expect("middle hole");
        arena.deallocate(fourth).expect("tail hole");
        let split = arena.allocate(32).expect("split the earliest hole");
        assert_eq!(split, second);

        arena.deallocate(first).expect("rewind before the split hole");
        arena.deallocate(split).expect("merge backward and forward");
        let merged = arena.allocate(144).expect("merged prefix before the live third block");
        assert_eq!(merged, first);
        assert_eq!(arena.allocation_size(third), Ok(64));
        arena.deallocate(third).expect("release intervening live block");
        arena.deallocate(merged).expect("release merged prefix");
        assert_eq!(arena.integrity_snapshot().expect("coalesced region").block_count(), 1);
        assert_eq!(arena.stats().current_payload_bytes(), 0);
    }

    #[test]
    fn integrity_snapshot_rejects_invalid_block_step() {
        let _guard = region::TEST_LOCK.lock();
        let mut storage = AlignedStorage([0; 512]);
        let arena = test_arena(&mut storage);

        let first_header = arena.base.as_ptr().cast::<BlockHeader>();
        // SAFETY: This test intentionally corrupts allocator metadata inside
        // its private arena span to verify the integrity hook catches it.
        unsafe { first_header.write(BlockHeader::new_free(super::ALIGNMENT)) };

        assert_eq!(arena.integrity_snapshot(), Err(super::ArenaError::InvalidRegion));
    }
}
