use core::ptr;

use tinylang_v2::*;

fn request(opcode: u16, correlation_id: u64, payload: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0u8; HEADER_BYTES + payload.len()];
    bytes[0..4].copy_from_slice(&MAGIC);
    bytes[4..6].copy_from_slice(&SCHEMA_MAJOR.to_le_bytes());
    bytes[6..8].copy_from_slice(&SCHEMA_MINOR.to_le_bytes());
    bytes[8..10].copy_from_slice(&(HEADER_BYTES as u16).to_le_bytes());
    bytes[10..12].copy_from_slice(&opcode.to_le_bytes());
    bytes[20..24].copy_from_slice(&(payload.len() as u32).to_le_bytes());
    bytes[24..32].copy_from_slice(&correlation_id.to_le_bytes());
    bytes[HEADER_BYTES..].copy_from_slice(payload);
    bytes
}

fn u32_value(value: u32) -> Vec<u8> {
    let mut bytes = vec![0u8; 24];
    bytes[0..2].copy_from_slice(&4u16.to_le_bytes());
    bytes[4..8].copy_from_slice(&4u32.to_le_bytes());
    bytes[16..20].copy_from_slice(&value.to_le_bytes());
    bytes
}

fn create_runtime() -> TinyRuntimeHandle {
    let mut runtime = 0;
    // SAFETY: `runtime` is valid output storage.
    let status = unsafe { tinylang_v2_runtime_create(&mut runtime) };
    assert_eq!(status, TINY_STATUS_OK);
    assert_ne!(runtime, 0);
    runtime
}

#[test]
fn reports_version_and_round_trips_canonical_values() {
    assert_eq!(tinylang_v2_abi_version(), 0x0002_0000);
    let runtime = create_runtime();
    let payload = u32_value(0x1234_5678);
    let input = request(OPCODE_ECHO, 99, &payload);
    let mut output = vec![0xa5; input.len()];
    let mut written = 0;
    // SAFETY: all descriptors point to live, disjoint storage for this call.
    let status = unsafe {
        tinylang_v2_call(
            runtime,
            TinyBuffer {
                data: input.as_ptr(),
                length: input.len() as u64,
            },
            TinyMutableBuffer {
                data: output.as_mut_ptr(),
                capacity: output.len() as u64,
            },
            &mut written,
        )
    };
    assert_eq!(status, TINY_STATUS_OK);
    assert_eq!(written as usize, output.len());
    assert_eq!(&output[0..4], b"TLV2");
    assert_eq!(
        u32::from_le_bytes(output[12..16].try_into().unwrap()),
        FLAG_RESPONSE
    );
    assert_eq!(u64::from_le_bytes(output[24..32].try_into().unwrap()), 99);
    assert_eq!(&output[HEADER_BYTES..], payload.as_slice());
    assert_eq!(tinylang_v2_runtime_destroy(runtime), TINY_STATUS_OK);
}

#[test]
fn capacity_probe_does_not_write_response() {
    let runtime = create_runtime();
    let input = request(OPCODE_ABI_INFO, 7, &[]);
    let mut sentinel = [0x5a; 8];
    let mut written = 0;
    // SAFETY: all descriptors point to live, disjoint storage.
    let status = unsafe {
        tinylang_v2_call(
            runtime,
            TinyBuffer {
                data: input.as_ptr(),
                length: input.len() as u64,
            },
            TinyMutableBuffer {
                data: sentinel.as_mut_ptr(),
                capacity: sentinel.len() as u64,
            },
            &mut written,
        )
    };
    assert_eq!(status, TINY_STATUS_BUFFER_TOO_SMALL);
    assert_eq!(written, 56);
    assert_eq!(sentinel, [0x5a; 8]);
    assert_eq!(tinylang_v2_runtime_destroy(runtime), TINY_STATUS_OK);
}

#[test]
fn rejects_overlapping_buffers_before_parsing() {
    let runtime = create_runtime();
    let mut input = request(OPCODE_NOP, 0, &[]);
    let mut written = 0;
    // SAFETY: memory is live; deliberate overlap is rejected before slices are formed.
    let status = unsafe {
        tinylang_v2_call(
            runtime,
            TinyBuffer {
                data: input.as_ptr(),
                length: input.len() as u64,
            },
            TinyMutableBuffer {
                data: input.as_mut_ptr(),
                capacity: input.len() as u64,
            },
            &mut written,
        )
    };
    assert_eq!(status, TINY_STATUS_INVALID_ARGUMENT);
    assert_eq!(tinylang_v2_runtime_destroy(runtime), TINY_STATUS_OK);
}

#[test]
fn owned_buffers_require_explicit_release() {
    let runtime = create_runtime();
    let mut buffer = TinyOwnedBuffer::default();
    // SAFETY: `buffer` is valid output storage.
    assert_eq!(
        unsafe { tinylang_v2_buffer_acquire(runtime, 1024, &mut buffer) },
        TINY_STATUS_OK
    );
    assert!(!buffer.data.is_null());
    assert_eq!(buffer.capacity, 1024);
    assert_eq!(tinylang_v2_runtime_destroy(runtime), TINY_STATUS_BUSY);
    // SAFETY: this is the original live descriptor returned above.
    assert_eq!(
        unsafe { tinylang_v2_buffer_release(runtime, &mut buffer) },
        TINY_STATUS_OK
    );
    assert!(buffer.data.is_null());
    assert_eq!(tinylang_v2_runtime_destroy(runtime), TINY_STATUS_OK);
    assert_eq!(
        tinylang_v2_runtime_destroy(runtime),
        TINY_STATUS_STALE_HANDLE
    );
}

#[test]
fn zero_length_buffers_may_be_null() {
    let runtime = create_runtime();
    let mut written = 123;
    // SAFETY: null with zero length/capacity is allowed; frame validation fails cleanly.
    let status = unsafe {
        tinylang_v2_call(
            runtime,
            TinyBuffer {
                data: ptr::null(),
                length: 0,
            },
            TinyMutableBuffer {
                data: ptr::null_mut(),
                capacity: 0,
            },
            &mut written,
        )
    };
    assert_eq!(status, TINY_STATUS_INVALID_FRAME);
    assert_eq!(written, 0);
    assert_eq!(tinylang_v2_runtime_destroy(runtime), TINY_STATUS_OK);
}
