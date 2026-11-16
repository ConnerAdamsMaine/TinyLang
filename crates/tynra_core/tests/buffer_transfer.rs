//! Boundary and concurrency tests for the experimental transfer-buffer API
//! (`tynra_v2_buffer_acquire` / `tynra_v2_buffer_release`), pinning the
//! V2-TOKEN-WRAP ABA/lifetime argument: transfer tokens are globally unique
//! per allocation, so a released or forged descriptor can never match a later
//! or different live allocation.

use std::thread;

use tynra::v2::*;

fn create_runtime() -> TynraRuntimeHandle {
    let mut runtime = 0;
    // SAFETY: `runtime` is valid output storage.
    assert_eq!(unsafe { tynra_v2_runtime_create(&mut runtime) }, TYNRA_STATUS_OK);
    runtime
}

fn acquire(runtime: TynraRuntimeHandle, capacity: u64) -> TynraOwnedBuffer {
    let mut buffer = TynraOwnedBuffer::default();
    // SAFETY: `buffer` is valid output storage.
    assert_eq!(unsafe { tynra_v2_buffer_acquire(runtime, capacity, &mut buffer) }, TYNRA_STATUS_OK);
    buffer
}

#[test]
fn released_descriptor_copies_and_double_releases_are_invalid() {
    let runtime = create_runtime();
    let buffer = acquire(runtime, 4096);
    let copy = TynraOwnedBuffer {
        data: buffer.data,
        length: buffer.length,
        capacity: buffer.capacity,
        owner_token: buffer.owner_token,
    };
    // SAFETY: both descriptors reference the single live allocation returned above.
    assert_eq!(unsafe { tynra_v2_buffer_release(runtime, &mut copy_of(&copy)) }, TYNRA_STATUS_OK);
    // The copy is invalid after the release, and so is the original descriptor.
    let mut stale_copy = copy;
    assert_eq!(unsafe { tynra_v2_buffer_release(runtime, &mut stale_copy) }, TYNRA_STATUS_INVALID_HANDLE);
    let mut stale_original = buffer;
    assert_eq!(unsafe { tynra_v2_buffer_release(runtime, &mut stale_original) }, TYNRA_STATUS_INVALID_HANDLE);
    assert_eq!(tynra_v2_runtime_destroy(runtime), TYNRA_STATUS_OK);
}

#[test]
fn stale_descriptor_fails_even_when_the_address_and_capacity_are_reused() {
    let runtime = create_runtime();
    let first = acquire(runtime, 8192);
    let mut first_descriptor = first;
    // SAFETY: the live allocation returned above.
    assert_eq!(unsafe { tynra_v2_buffer_release(runtime, &mut first_descriptor) }, TYNRA_STATUS_OK);

    // The allocator is likely to hand back the same block for the same size.
    let second = acquire(runtime, 8192);
    let mut stale = TynraOwnedBuffer {
        data: first.data,
        length: 0,
        capacity: first.capacity,
        owner_token: first.owner_token,
    };
    // SAFETY: `stale` was never live; the release lookup fails before any dealloc.
    assert_eq!(unsafe { tynra_v2_buffer_release(runtime, &mut stale) }, TYNRA_STATUS_INVALID_HANDLE);

    // The live second allocation is untouched by the stale attempt and still releases.
    let mut live = second;
    // SAFETY: the live allocation returned above.
    assert_eq!(unsafe { tynra_v2_buffer_release(runtime, &mut live) }, TYNRA_STATUS_OK);
    assert_eq!(tynra_v2_runtime_destroy(runtime), TYNRA_STATUS_OK);
}

#[test]
fn forged_token_cannot_release_a_different_live_buffer() {
    let runtime = create_runtime();
    let first = acquire(runtime, 2048);
    let second = acquire(runtime, 4096);

    // A fabricated descriptor mixing the first allocation's token with the
    // second allocation's address must not match the live second slot.
    let mut forged = TynraOwnedBuffer {
        data: second.data,
        length: 0,
        capacity: second.capacity,
        owner_token: first.owner_token,
    };
    // SAFETY: `forged` matches no registry slot; nothing is deallocated.
    assert_eq!(unsafe { tynra_v2_buffer_release(runtime, &mut forged) }, TYNRA_STATUS_INVALID_HANDLE);

    // Both live allocations survive the forged attempt and release normally.
    let mut live_first = first;
    // SAFETY: the live allocation returned above.
    assert_eq!(unsafe { tynra_v2_buffer_release(runtime, &mut live_first) }, TYNRA_STATUS_OK);
    let mut live_second = second;
    // SAFETY: the live allocation returned above.
    assert_eq!(unsafe { tynra_v2_buffer_release(runtime, &mut live_second) }, TYNRA_STATUS_OK);
    assert_eq!(tynra_v2_runtime_destroy(runtime), TYNRA_STATUS_OK);
}

#[test]
fn release_is_scoped_to_the_issuing_runtime() {
    let first_runtime = create_runtime();
    let second_runtime = create_runtime();
    let buffer = acquire(first_runtime, 1024);

    let mut cross_runtime = TynraOwnedBuffer {
        data: buffer.data,
        length: buffer.length,
        capacity: buffer.capacity,
        owner_token: buffer.owner_token,
    };
    // SAFETY: the descriptor is genuine, but the runtime handle is not the issuer.
    assert_eq!(unsafe { tynra_v2_buffer_release(second_runtime, &mut cross_runtime) }, TYNRA_STATUS_INVALID_HANDLE);

    // SAFETY: the live allocation from `first_runtime`.
    let mut live = buffer;
    assert_eq!(unsafe { tynra_v2_buffer_release(first_runtime, &mut live) }, TYNRA_STATUS_OK);
    assert_eq!(tynra_v2_runtime_destroy(first_runtime), TYNRA_STATUS_OK);
    assert_eq!(tynra_v2_runtime_destroy(second_runtime), TYNRA_STATUS_OK);
}

#[test]
fn concurrent_acquire_release_on_one_runtime_stays_consistent() {
    let runtime = create_runtime();
    let handles: Vec<_> = (0..4)
        .map(|_| {
            thread::spawn(move || {
                for _ in 0..200 {
                    let mut buffer = TynraOwnedBuffer::default();
                    // SAFETY: `buffer` is valid output storage on this thread.
                    let status = unsafe { tynra_v2_buffer_acquire(runtime, 4096, &mut buffer) };
                    if status != TYNRA_STATUS_OK {
                        // The fixed 64-slot table may be exhausted by the other
                        // threads; only OOM is an acceptable failure.
                        assert_eq!(status, TYNRA_STATUS_OUT_OF_MEMORY);
                        continue;
                    }
                    // SAFETY: this thread owns the live allocation returned above.
                    assert_eq!(unsafe { tynra_v2_buffer_release(runtime, &mut buffer) }, TYNRA_STATUS_OK);
                }
            })
        })
        .collect();
    for handle in handles {
        handle.join().expect("worker thread must not panic");
    }
    assert_eq!(tynra_v2_runtime_destroy(runtime), TYNRA_STATUS_OK);
}

#[test]
fn null_and_mismatched_descriptors_are_rejected_before_lookup() {
    let runtime = create_runtime();
    let mut null_descriptor = TynraOwnedBuffer::default();
    // SAFETY: the zeroed descriptor is invalid by contract and rejected early.
    assert_eq!(unsafe { tynra_v2_buffer_release(runtime, &mut null_descriptor) }, TYNRA_STATUS_INVALID_ARGUMENT);

    let buffer = acquire(runtime, 1024);
    let mut oversized_length = TynraOwnedBuffer {
        data: buffer.data,
        length: buffer.capacity + 1,
        capacity: buffer.capacity,
        owner_token: buffer.owner_token,
    };
    // SAFETY: length > capacity is rejected before any registry access.
    assert_eq!(unsafe { tynra_v2_buffer_release(runtime, &mut oversized_length) }, TYNRA_STATUS_INVALID_ARGUMENT);
    let mut live = buffer;
    // SAFETY: the live allocation returned above.
    assert_eq!(unsafe { tynra_v2_buffer_release(runtime, &mut live) }, TYNRA_STATUS_OK);
    assert_eq!(tynra_v2_runtime_destroy(runtime), TYNRA_STATUS_OK);
}

fn copy_of(descriptor: &TynraOwnedBuffer) -> TynraOwnedBuffer {
    TynraOwnedBuffer {
        data: descriptor.data,
        length: descriptor.length,
        capacity: descriptor.capacity,
        owner_token: descriptor.owner_token,
    }
}
