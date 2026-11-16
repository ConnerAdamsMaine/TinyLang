//! Compare allocation placement against an unconditional scan of live metadata.

extern crate std;

use core::ptr::NonNull;
use std::vec::Vec;

use super::{AlignedStorage, test_arena};
use crate::arena::{Arena, ArenaError};
use crate::block::{ALIGNMENT, HEADER_SIZE, request_to_block_size};
use crate::region;

struct LiveAllocation {
    payload: NonNull<u8>,
    usable:  usize,
    marker:  u8,
}

/// Deliberately ignores any allocation-search state. Alignment/splitting use
/// the unchanged placement calculation, while the scan always begins at base.
fn full_scan_placement(arena: &Arena, size: usize, alignment: usize) -> Result<usize, ArenaError> {
    if alignment == 0 || !alignment.is_power_of_two() {
        return Err(ArenaError::OutOfMemory);
    }
    let needed = request_to_block_size(size).ok_or(ArenaError::OutOfMemory)?;
    let mut current = arena.base.as_ptr().addr();
    while current < arena.end() {
        // SAFETY: The test owns this arena and calls the oracle only after an
        // integrity check; each step follows a valid header inside its span.
        let block = unsafe { &*arena.header_at(current) };
        if !block.is_allocated()
            && let Some((header, _)) =
                arena.aligned_allocation_slot(current, block.size(), needed, alignment.max(ALIGNMENT))
        {
            return Ok(header.addr() + HEADER_SIZE);
        }
        current += block.size();
    }
    Err(ArenaError::OutOfMemory)
}

fn assert_live_allocations(arena: &Arena, live: &[LiveAllocation]) {
    let snapshot = arena.integrity_snapshot().expect("valid arena block walk");
    assert_eq!(snapshot.live_block_count(), live.len());
    for allocation in live {
        let payload = allocation.payload.as_ptr();
        assert_eq!(arena.allocation_size(payload), Ok(allocation.usable));
        // SAFETY: Each owner remains live and was initialized across its full
        // usable extent. This borrowed byte slice is consumed before any
        // allocation, free or write; it never spans allocator metadata.
        let bytes = unsafe { core::slice::from_raw_parts(payload, allocation.usable) };
        assert!(bytes.iter().all(|byte| *byte == allocation.marker), "live payload was overwritten");
    }
}

#[test]
fn mixed_requests_keep_full_scan_first_fit_placement() {
    const SIZES: [usize; 13] = [0, 1, 15, 16, 17, 31, 32, 48, 64, 96, 128, 240, usize::MAX];
    const ALIGNMENTS: [usize; 11] = [0, 1, 3, 8, 16, 32, 64, 128, 256, 512, 4096];
    let _guard = region::TEST_LOCK.lock();
    for seed in [1u32, 0x5678, 0xdead_beef, 0xfeed_cafe] {
        let mut storage = AlignedStorage([0; 512]);
        let mut arena = test_arena(&mut storage);
        let mut live = Vec::<LiveAllocation>::new();
        let mut state = seed;
        let mut requests = 0;
        let mut successful = 0;
        for step in 0..256 {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let choice = usize::try_from(state >> 8).expect("24-bit choice fits a supported host");
            assert_live_allocations(&arena, &live);
            if step % 3 == 0 && !live.is_empty() {
                let removed = live.swap_remove(choice % live.len());
                arena
                    .deallocate(removed.payload.as_ptr())
                    .expect("release selected live owner");
            } else {
                let size = SIZES[step % SIZES.len()];
                let alignment = ALIGNMENTS[choice % ALIGNMENTS.len()];
                let expected = full_scan_placement(&arena, size, alignment);
                let actual = arena.allocate_aligned(size, alignment);
                assert_eq!(actual.map(|payload| payload.addr()), expected, "{seed:#x}/{step}/{size}/{alignment}");
                requests += 1;
                if let Ok(payload) = actual {
                    let usable = arena
                        .allocation_size(payload)
                        .unwrap_or_else(|error| panic!("{seed:#x}/{step}/{size}/{alignment}: {error:?}"));
                    assert!(usable >= size.max(1));
                    assert_eq!(payload.addr() % alignment.max(ALIGNMENT), 0);
                    let marker = state.to_le_bytes()[0];
                    // SAFETY: This is a fresh, disjoint allocation with the
                    // validated usable byte extent; every u8 pattern is valid.
                    unsafe { payload.write_bytes(marker, usable) };
                    live.push(LiveAllocation {
                        payload: NonNull::new(payload).expect("successful allocation is non-null"),
                        usable,
                        marker,
                    });
                    successful += 1;
                }
            }
            assert_live_allocations(&arena, &live);
        }
        while let Some(allocation) = live.pop() {
            arena
                .deallocate(allocation.payload.as_ptr())
                .expect("release final live owner");
            assert_live_allocations(&arena, &live);
        }
        assert_eq!(arena.integrity_snapshot().expect("fully coalesced arena").block_count(), 1);
        assert!(successful > 0 && successful < requests, "both success and failure routes exercised");
        std::eprintln!("first-fit-oracle seed={seed:#x} steps=256 requests={requests} successful={successful}");
    }
}

#[test]
fn over_aligned_exact_fit_initializes_header_inside_reused_payload() {
    let _guard = region::TEST_LOCK.lock();
    let mut storage = AlignedStorage([0; 512]);
    let mut arena = test_arena(&mut storage);
    let first = arena.allocate(4 * ALIGNMENT).expect("future aligned hole");
    let barrier = arena.allocate(ALIGNMENT).expect("prevent forward coalescing");
    // SAFETY: This is the first owner's entire initialized payload, before
    // freeing it. The later aligned header will occupy part of these bytes.
    unsafe { first.write_bytes(0x5a, 4 * ALIGNMENT) };
    arena.deallocate(first).expect("make a bounded free block");

    let aligned = arena
        .allocate_aligned(ALIGNMENT, 2 * ALIGNMENT)
        .expect("alignment padding leaves exactly one minimum-sized block");
    assert_eq!(aligned.addr(), arena.base.as_ptr().addr() + 4 * ALIGNMENT);
    assert_eq!(arena.allocation_size(aligned), Ok(ALIGNMENT));
    let snapshot = arena
        .integrity_snapshot()
        .expect("the interior header must be initialized");
    assert_eq!(snapshot.live_block_count(), 2);
    arena.deallocate(aligned).expect("aligned owner remains freeable");
    arena.deallocate(barrier).expect("release final owner");
    assert_eq!(
        arena
            .integrity_snapshot()
            .expect("whole region recovered")
            .block_count(),
        1
    );
    assert_eq!(arena.stats().current_payload_bytes(), 0);
}

#[test]
fn backward_coalescing_revisits_an_earlier_undersized_hole() {
    let _guard = region::TEST_LOCK.lock();
    let mut storage = AlignedStorage([0; 512]);
    let mut arena = test_arena(&mut storage);
    let first = arena.allocate(64).expect("prefix owner");
    let hole = arena.allocate(16).expect("future undersized hole");
    let adjacent = arena.allocate(64).expect("owner after the hole");
    let last = arena.allocate(64).expect("last original owner");
    arena.deallocate(hole).expect("expose the undersized hole");
    let beyond = arena.allocate(64).expect("skip the undersized hole");
    assert!(beyond.addr() > last.addr());

    arena
        .deallocate(adjacent)
        .expect("grow the hole by backward coalescing");
    arena
        .integrity_snapshot()
        .expect("search boundary must remain a block boundary");
    let expected = full_scan_placement(&arena, 80, ALIGNMENT);
    let merged = arena.allocate(80).expect("reuse the newly enlarged early hole");
    assert_eq!(expected, Ok(merged.addr()));
    assert_eq!(merged, hole);
    for payload in [beyond, first, last, merged] {
        arena.deallocate(payload).expect("release remaining owner");
        arena.integrity_snapshot().expect("valid coalescing after every free");
    }
    assert_eq!(arena.integrity_snapshot().expect("coalesced region").block_count(), 1);
}

#[test]
fn request_equal_to_an_earlier_free_block_restarts_first_fit() {
    let _guard = region::TEST_LOCK.lock();
    let mut storage = AlignedStorage([0; 512]);
    let mut arena = test_arena(&mut storage);
    let hole = arena.allocate(ALIGNMENT).expect("future minimum-sized hole");
    let barrier = arena.allocate(ALIGNMENT).expect("keep the hole separate");
    arena.deallocate(hole).expect("expose the early hole");
    let large = arena.allocate(4 * ALIGNMENT).expect("skip the small hole");
    let expected = full_scan_placement(&arena, ALIGNMENT, ALIGNMENT);
    let exact = arena.allocate(ALIGNMENT).expect("exact earlier fit");
    assert_eq!(expected, Ok(exact.addr()));
    assert_eq!(exact, hole);
    for payload in [barrier, large, exact] {
        arena.deallocate(payload).expect("release live owner");
        arena.integrity_snapshot().expect("valid arena after free");
    }
    assert_eq!(
        arena
            .integrity_snapshot()
            .expect("fully recovered region")
            .block_count(),
        1
    );
}

#[test]
fn alignment_rejection_keeps_earlier_holes_available_to_later_requests() {
    let _guard = region::TEST_LOCK.lock();
    let mut storage = AlignedStorage([0; 512]);
    let mut arena = test_arena(&mut storage);
    let hole = arena.allocate(ALIGNMENT).expect("future alignment-rejected hole");
    let barrier = arena.allocate(ALIGNMENT).expect("keep the hole separate");
    arena.deallocate(hole).expect("expose the early hole");
    let aligned = arena
        .allocate_aligned(ALIGNMENT, 4 * ALIGNMENT)
        .expect("aligned allocation beyond the early hole");
    assert!(aligned.addr() > barrier.addr());
    let expected = full_scan_placement(&arena, ALIGNMENT, ALIGNMENT);
    let ordinary = arena.allocate(ALIGNMENT).expect("ordinary request fits the early hole");
    assert_eq!(expected, Ok(ordinary.addr()));
    assert_eq!(ordinary, hole);
    for payload in [aligned, ordinary, barrier] {
        arena.deallocate(payload).expect("release live owner");
        arena.integrity_snapshot().expect("valid arena after free");
    }
    assert_eq!(
        arena
            .integrity_snapshot()
            .expect("fully recovered region")
            .block_count(),
        1
    );
}
