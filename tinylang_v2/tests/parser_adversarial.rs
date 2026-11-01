use tinylang_v2::*;

#[test]
fn single_byte_mutations_never_escape_as_panics() {
    let base = include_bytes!("vectors/valid_echo_values.bin");
    let runtime = create_runtime();
    for index in 0..base.len() {
        for mask in [0x01u8, 0x80, 0xff] {
            let mut input = base.to_vec();
            input[index] ^= mask;
            let status = call(runtime, &input);
            assert_ne!(
                status, TINY_STATUS_PANIC,
                "panic status for byte {index} mask {mask:#04x}"
            );
        }
    }
    assert_eq!(tinylang_v2_runtime_destroy(runtime), TINY_STATUS_OK);
}

#[test]
fn excessive_value_nesting_is_rejected() {
    let mut nested = null_value();
    for _ in 0..33 {
        nested = sequence_value(&nested, 1);
    }
    let input = request(OPCODE_ECHO, &nested);
    let runtime = create_runtime();
    assert_eq!(call(runtime, &input), TINY_STATUS_INVALID_FRAME);
    assert_eq!(tinylang_v2_runtime_destroy(runtime), TINY_STATUS_OK);
}

#[test]
fn impossible_slice_count_is_rejected_without_multiplication_overflow() {
    let mut slice = vec![0u8; 24];
    slice[0..2].copy_from_slice(&16u16.to_le_bytes());
    slice[4..8].copy_from_slice(&8u32.to_le_bytes());
    slice[8..16].copy_from_slice(&5u64.to_le_bytes()); // U64 element tag.
    slice[16..24].copy_from_slice(&u64::MAX.to_le_bytes());
    let input = request(OPCODE_ECHO, &slice);
    let runtime = create_runtime();
    assert_eq!(call(runtime, &input), TINY_STATUS_INVALID_FRAME);
    assert_eq!(tinylang_v2_runtime_destroy(runtime), TINY_STATUS_OK);
}

#[test]
fn malformed_utf8_is_rejected() {
    let mut value = vec![0u8; 24];
    value[0..2].copy_from_slice(&13u16.to_le_bytes());
    value[4..8].copy_from_slice(&2u32.to_le_bytes());
    value[16..18].copy_from_slice(&[0xc0, 0x80]);
    let input = request(OPCODE_ECHO, &value);
    let runtime = create_runtime();
    assert_eq!(call(runtime, &input), TINY_STATUS_INVALID_FRAME);
    assert_eq!(tinylang_v2_runtime_destroy(runtime), TINY_STATUS_OK);
}

fn create_runtime() -> TinyRuntimeHandle {
    let mut runtime = 0;
    // SAFETY: `runtime` is valid output storage.
    assert_eq!(
        unsafe { tinylang_v2_runtime_create(&mut runtime) },
        TINY_STATUS_OK
    );
    runtime
}

fn call(runtime: TinyRuntimeHandle, input: &[u8]) -> TinyStatus {
    let mut output = [0u8; 512];
    let mut written = 0;
    // SAFETY: all descriptors reference live, disjoint storage for this call.
    unsafe {
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
    }
}

fn request(opcode: u16, payload: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0u8; HEADER_BYTES + payload.len()];
    bytes[0..4].copy_from_slice(&MAGIC);
    bytes[4..6].copy_from_slice(&SCHEMA_MAJOR.to_le_bytes());
    bytes[6..8].copy_from_slice(&SCHEMA_MINOR.to_le_bytes());
    bytes[8..10].copy_from_slice(&(HEADER_BYTES as u16).to_le_bytes());
    bytes[10..12].copy_from_slice(&opcode.to_le_bytes());
    bytes[20..24].copy_from_slice(&(payload.len() as u32).to_le_bytes());
    bytes[HEADER_BYTES..].copy_from_slice(payload);
    bytes
}

fn null_value() -> Vec<u8> {
    vec![0u8; VALUE_HEADER_BYTES]
}

fn sequence_value(nested: &[u8], count: u64) -> Vec<u8> {
    let mut bytes = vec![0u8; VALUE_HEADER_BYTES + nested.len()];
    bytes[0..2].copy_from_slice(&17u16.to_le_bytes());
    bytes[4..8].copy_from_slice(&(nested.len() as u32).to_le_bytes());
    bytes[8..16].copy_from_slice(&count.to_le_bytes());
    bytes[VALUE_HEADER_BYTES..].copy_from_slice(nested);
    bytes
}
