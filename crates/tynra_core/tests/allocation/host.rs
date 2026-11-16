//! Process-isolated host-allocation diagnostics, never linked into production.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::Ordering::Relaxed;
use std::sync::atomic::{AtomicBool, AtomicUsize};

struct CountedHostAllocator;

// This allocator belongs only to this diagnostic integration-test binary.
// Production executables and the latency harness retain their allocator.
#[global_allocator]
static HOST_ALLOCATOR: CountedHostAllocator = CountedHostAllocator;

static COUNTING: AtomicBool = AtomicBool::new(false);
static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);
static DEALLOCATIONS: AtomicUsize = AtomicUsize::new(0);
static REALLOCATIONS: AtomicUsize = AtomicUsize::new(0);
static ALLOCATED_BYTES: AtomicUsize = AtomicUsize::new(0);
static REALLOCATED_BYTES: AtomicUsize = AtomicUsize::new(0);
static SIZE_BUCKETS: [AtomicUsize; 4] = [const { AtomicUsize::new(0) }; 4];

fn record_allocation(size: usize) {
    ALLOCATIONS.fetch_add(1, Relaxed);
    ALLOCATED_BYTES.fetch_add(size, Relaxed);
    let bucket = match size {
        0..=64 => 0,
        65..=256 => 1,
        257..=1024 => 2,
        _ => 3,
    };
    SIZE_BUCKETS[bucket].fetch_add(1, Relaxed);
}

// SAFETY: Every allocation operation forwards the original pointer/layout to
// System without changing ownership or alignment. Instrumentation uses only
// non-allocating atomics and bounded indexing; it cannot recurse or unwind.
unsafe impl GlobalAlloc for CountedHostAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: The caller supplies the nonzero valid GlobalAlloc layout.
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() && COUNTING.load(Relaxed) {
            record_allocation(layout.size());
        }
        pointer
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        // SAFETY: Forward the valid layout and preserve System's zeroing.
        let pointer = unsafe { System.alloc_zeroed(layout) };
        if !pointer.is_null() && COUNTING.load(Relaxed) {
            record_allocation(layout.size());
        }
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY: All pointers from this allocator originate in System; the
        // caller supplies a live allocation and its original matching layout.
        unsafe { System.dealloc(pointer, layout) };
        if COUNTING.load(Relaxed) {
            DEALLOCATIONS.fetch_add(1, Relaxed);
        }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: The caller supplies a live System allocation, its matching
        // layout, and a valid nonzero new size. Forwarding preserves the old
        // allocation on failure and transfers ownership only on success.
        let resized = unsafe { System.realloc(pointer, layout, new_size) };
        if !resized.is_null() && COUNTING.load(Relaxed) {
            REALLOCATIONS.fetch_add(1, Relaxed);
            REALLOCATED_BYTES.fetch_add(new_size, Relaxed);
        }
        resized
    }
}

#[derive(Debug)]
pub(super) struct HostAllocationCounts {
    pub(super) allocations: usize,
    pub(super) deallocations: usize,
    pub(super) reallocations: usize,
    pub(super) allocated_bytes: usize,
    pub(super) reallocated_bytes: usize,
    pub(super) size_buckets: [usize; 4],
}

pub(super) struct HostAllocationInterval;

impl HostAllocationInterval {
    pub(super) fn start() -> Self {
        assert!(!COUNTING.load(Relaxed), "measurement intervals cannot overlap");
        for counter in [
            &ALLOCATIONS,
            &DEALLOCATIONS,
            &REALLOCATIONS,
            &ALLOCATED_BYTES,
            &REALLOCATED_BYTES,
        ] {
            counter.store(0, Relaxed);
        }
        for counter in &SIZE_BUCKETS {
            counter.store(0, Relaxed);
        }
        COUNTING.store(true, Relaxed);
        Self
    }

    pub(super) fn finish(self) -> HostAllocationCounts {
        drop(self);
        HostAllocationCounts {
            allocations: ALLOCATIONS.load(Relaxed),
            deallocations: DEALLOCATIONS.load(Relaxed),
            reallocations: REALLOCATIONS.load(Relaxed),
            allocated_bytes: ALLOCATED_BYTES.load(Relaxed),
            reallocated_bytes: REALLOCATED_BYTES.load(Relaxed),
            size_buckets: std::array::from_fn(|index| SIZE_BUCKETS[index].load(Relaxed)),
        }
    }
}

impl Drop for HostAllocationInterval {
    fn drop(&mut self) {
        COUNTING.store(false, Relaxed);
    }
}
