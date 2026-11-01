#![allow(dead_code)]
use core::sync::atomic::{AtomicU64, Ordering};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct InstrumentationSnapshot {
    pub(crate) growth_events:       u64,
    pub(crate) bytes_copied:        u64,
    pub(crate) allocations:         u64,
    pub(crate) deallocations:       u64,
    pub(crate) reallocations:       u64,
    pub(crate) allocation_failures: u64,
    pub(crate) requested_bytes:     u64,
    pub(crate) usable_bytes:        u64,
    pub(crate) live_payload_bytes:  u64,
    pub(crate) peak_payload_bytes:  u64,
    pub(crate) allocation_probes:   u64,
    pub(crate) lookup_probes:       u64,
    pub(crate) coalesce_probes:     u64,
    pub(crate) request_sizes:       [u64; 32],
    pub(crate) request_alignments:  [u64; 14],
}

static GROWTH_EVENTS: AtomicU64 = AtomicU64::new(0);
static BYTES_COPIED: AtomicU64 = AtomicU64::new(0);
static ALLOCATIONS: AtomicU64 = AtomicU64::new(0);
static DEALLOCATIONS: AtomicU64 = AtomicU64::new(0);
static REALLOCATIONS: AtomicU64 = AtomicU64::new(0);
static ALLOCATION_FAILURES: AtomicU64 = AtomicU64::new(0);
static REQUESTED_BYTES: AtomicU64 = AtomicU64::new(0);
static USABLE_BYTES: AtomicU64 = AtomicU64::new(0);
static LIVE_PAYLOAD_BYTES: AtomicU64 = AtomicU64::new(0);
static PEAK_PAYLOAD_BYTES: AtomicU64 = AtomicU64::new(0);
static ALLOCATION_PROBES: AtomicU64 = AtomicU64::new(0);
static LOOKUP_PROBES: AtomicU64 = AtomicU64::new(0);
static COALESCE_PROBES: AtomicU64 = AtomicU64::new(0);
static REQUEST_SIZES: [AtomicU64; 32] = [const { AtomicU64::new(0) }; 32];
static REQUEST_ALIGNMENTS: [AtomicU64; 14] = [const { AtomicU64::new(0) }; 14];

pub(crate) fn record_request(size: usize, align: usize) {
    let size_class = (usize::BITS - size.saturating_sub(1).leading_zeros()) as usize;
    REQUEST_SIZES[size_class.min(31)].fetch_add(1, Ordering::Relaxed);
    let align_class = align.trailing_zeros() as usize;
    REQUEST_ALIGNMENTS[align_class.min(13)].fetch_add(1, Ordering::Relaxed);
    REQUESTED_BYTES.fetch_add(size as u64, Ordering::Relaxed);
}

pub(crate) fn record_allocation(usable_bytes: usize) {
    ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
    USABLE_BYTES.fetch_add(usable_bytes as u64, Ordering::Relaxed);
    let live = LIVE_PAYLOAD_BYTES.fetch_add(usable_bytes as u64, Ordering::Relaxed) + usable_bytes as u64;
    PEAK_PAYLOAD_BYTES.fetch_max(live, Ordering::Relaxed);
}

pub(crate) fn record_deallocation(usable_bytes: usize) {
    DEALLOCATIONS.fetch_add(1, Ordering::Relaxed);
    LIVE_PAYLOAD_BYTES.fetch_sub(usable_bytes as u64, Ordering::Relaxed);
}

pub(crate) fn record_reallocation() {
    REALLOCATIONS.fetch_add(1, Ordering::Relaxed);
}

pub(crate) fn record_allocation_failure() {
    ALLOCATION_FAILURES.fetch_add(1, Ordering::Relaxed);
}

pub(crate) fn record_allocation_probe() {
    ALLOCATION_PROBES.fetch_add(1, Ordering::Relaxed);
}

pub(crate) fn record_lookup_probe() {
    LOOKUP_PROBES.fetch_add(1, Ordering::Relaxed);
}

pub(crate) fn record_coalesce_probe() {
    COALESCE_PROBES.fetch_add(1, Ordering::Relaxed);
}

#[inline]
pub(crate) fn record_growth() {
    GROWTH_EVENTS.fetch_add(1, Ordering::Relaxed);
}

#[inline]
pub(crate) fn record_bytes_copied(bytes: usize) {
    BYTES_COPIED.fetch_add(bytes as u64, Ordering::Relaxed);
}

pub(crate) fn snapshot() -> InstrumentationSnapshot {
    InstrumentationSnapshot {
        growth_events:       GROWTH_EVENTS.load(Ordering::Relaxed),
        bytes_copied:        BYTES_COPIED.load(Ordering::Relaxed),
        allocations:         ALLOCATIONS.load(Ordering::Relaxed),
        deallocations:       DEALLOCATIONS.load(Ordering::Relaxed),
        reallocations:       REALLOCATIONS.load(Ordering::Relaxed),
        allocation_failures: ALLOCATION_FAILURES.load(Ordering::Relaxed),
        requested_bytes:     REQUESTED_BYTES.load(Ordering::Relaxed),
        usable_bytes:        USABLE_BYTES.load(Ordering::Relaxed),
        live_payload_bytes:  LIVE_PAYLOAD_BYTES.load(Ordering::Relaxed),
        peak_payload_bytes:  PEAK_PAYLOAD_BYTES.load(Ordering::Relaxed),
        allocation_probes:   ALLOCATION_PROBES.load(Ordering::Relaxed),
        lookup_probes:       LOOKUP_PROBES.load(Ordering::Relaxed),
        coalesce_probes:     COALESCE_PROBES.load(Ordering::Relaxed),
        request_sizes:       core::array::from_fn(|index| REQUEST_SIZES[index].load(Ordering::Relaxed)),
        request_alignments:  core::array::from_fn(|index| REQUEST_ALIGNMENTS[index].load(Ordering::Relaxed)),
    }
}

pub(crate) fn reset() {
    GROWTH_EVENTS.store(0, Ordering::Relaxed);
    BYTES_COPIED.store(0, Ordering::Relaxed);
    for counter in [
        &ALLOCATIONS,
        &DEALLOCATIONS,
        &REALLOCATIONS,
        &ALLOCATION_FAILURES,
        &REQUESTED_BYTES,
        &USABLE_BYTES,
        &ALLOCATION_PROBES,
        &LOOKUP_PROBES,
        &COALESCE_PROBES,
    ]
    .into_iter()
    .chain(REQUEST_SIZES.iter())
    .chain(REQUEST_ALIGNMENTS.iter())
    {
        counter.store(0, Ordering::Relaxed);
    }
    // Live allocations can predate a measurement interval. Keep their gauge
    // intact so later frees remain balanced; restart the peak at that gauge.
    PEAK_PAYLOAD_BYTES.store(LIVE_PAYLOAD_BYTES.load(Ordering::Relaxed), Ordering::Relaxed);
}
