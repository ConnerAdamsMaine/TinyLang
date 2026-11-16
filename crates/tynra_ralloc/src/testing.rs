//! Feature-gated allocator instrumentation for external test harnesses.

/// Process-wide allocator activity. Instrumented timings are diagnostic only;
/// build without `testing-hooks` when measuring normal allocator latency.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct InstrumentationSnapshot {
    /// Successful buffer growth operations.
    pub growth_events:       u64,
    /// Bytes copied while moving live allocations during growth.
    pub bytes_copied:        u64,
    /// Successful arena allocations, including moves during resize.
    pub allocations:         u64,
    /// Successfully freed allocations, including old storage after moves.
    pub deallocations:       u64,
    /// Calls to the realloc path (including in-place resize).
    pub reallocations:       u64,
    /// Valid nonzero allocation requests that no arena could satisfy.
    pub allocation_failures: u64,
    /// Requested bytes across valid nonzero allocation requests.
    pub requested_bytes:     u64,
    /// Usable payload bytes across successful allocations, including padding.
    pub usable_bytes:        u64,
    /// Currently live usable payload bytes; not reset between intervals.
    pub live_payload_bytes:  u64,
    /// Maximum simultaneously live usable payload bytes since reset.
    pub peak_payload_bytes:  u64,
    /// Block headers visited by allocation scans.
    pub allocation_probes:   u64,
    /// Block headers visited by live-pointer validation scans.
    pub lookup_probes:       u64,
    /// Block headers visited when finding a preceding block for coalescing.
    pub coalesce_probes:     u64,
    /// Requests in ceiling-power-of-two size classes: index 0 is 1 byte,
    /// index k is (2^(k-1), 2^k] bytes; index 31 includes all larger sizes.
    pub request_sizes:       [u64; 32],
    /// Requested alignment 2^k at index k (supported alignments end at 4096).
    pub request_alignments:  [u64; 14],
}

/// Returns the current process-wide allocator instrumentation counters.
pub fn instrumentation_snapshot() -> InstrumentationSnapshot {
    let snapshot = crate::instrumentation::snapshot();
    InstrumentationSnapshot {
        growth_events:       snapshot.growth_events,
        bytes_copied:        snapshot.bytes_copied,
        allocations:         snapshot.allocations,
        deallocations:       snapshot.deallocations,
        reallocations:       snapshot.reallocations,
        allocation_failures: snapshot.allocation_failures,
        requested_bytes:     snapshot.requested_bytes,
        usable_bytes:        snapshot.usable_bytes,
        live_payload_bytes:  snapshot.live_payload_bytes,
        peak_payload_bytes:  snapshot.peak_payload_bytes,
        allocation_probes:   snapshot.allocation_probes,
        lookup_probes:       snapshot.lookup_probes,
        coalesce_probes:     snapshot.coalesce_probes,
        request_sizes:       snapshot.request_sizes,
        request_alignments:  snapshot.request_alignments,
    }
}

/// Resets all process-wide allocator instrumentation counters.
///
/// Call this only when the surrounding test harness has exclusive ownership
/// of the process-wide measurement interval.
pub fn reset_instrumentation() {
    crate::instrumentation::reset();
}
