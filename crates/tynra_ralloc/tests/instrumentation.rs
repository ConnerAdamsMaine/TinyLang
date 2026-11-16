#![cfg(feature = "testing-hooks")]

use ralloc::RallocBuffer;
use ralloc::testing::{instrumentation_snapshot, reset_instrumentation};

#[test]
fn allocation_gauges_survive_resets_with_live_storage() {
    let mut buffer = RallocBuffer::try_new_aligned(33, 64).expect("aligned allocation");
    reset_instrumentation();
    let start = instrumentation_snapshot();
    assert!(start.live_payload_bytes >= 33);
    assert_eq!(start.peak_payload_bytes, start.live_payload_bytes);
    buffer.try_resize(4096).expect("grow allocation");
    let grown = instrumentation_snapshot();
    assert_eq!(grown.reallocations, 1);
    assert!(grown.live_payload_bytes >= 4096);
    assert!(grown.peak_payload_bytes >= grown.live_payload_bytes);
    assert_eq!(grown.request_sizes[12], 1);
    assert_eq!(grown.request_alignments[6], 1);
    drop(buffer);
    assert_eq!(instrumentation_snapshot().live_payload_bytes, 0);
}
