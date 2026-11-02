use tinylang_v2::*;

const ARTIFACT_HEADER_BYTES: usize = 32;
const SECTION_ENTRY_BYTES: usize = 24;
const METADATA_OFFSET: usize = ARTIFACT_HEADER_BYTES + 2 * SECTION_ENTRY_BYTES;
const CODE_OFFSET: usize = METADATA_OFFSET + 16;
const INSTRUCTION_BYTES: usize = 24;

#[test]
fn compiler_binary_loads_functions_and_rejects_mismatched_or_damaged_containers() {
    let runtime = create_runtime();
    let program = tinylang::compile_source(
        "fn answer() { return 42 } fn text() { return \"hello\" } fn items() { return [1, 2] }",
    )
    .unwrap();
    let binary = program.to_binary_artifact().unwrap();
    let payload = values(&[u32_value(2), bytes_value(&binary)]);
    assert_eq!(
        call(runtime, OPCODE_PROGRAM_LOAD, &payload, 47).0,
        TINY_STATUS_BUFFER_TOO_SMALL
    );
    // A failed capacity probe did not insert a program.
    assert_eq!(tinylang_v2_runtime_destroy(runtime), TINY_STATUS_OK);

    let runtime = create_runtime();
    let (status, written, response) = call(runtime, OPCODE_PROGRAM_LOAD, &payload, 48);
    assert_eq!(status, TINY_STATUS_OK);
    assert_eq!(written, 48);
    assert_eq!(read_u32(&response, 16), TINY_STATUS_OK);
    let handle = read_u64(&response, HEADER_BYTES + 8);
    let release = values(&[handle_value(handle)]);
    assert_eq!(
        read_u32(&call(runtime, OPCODE_HANDLE_RELEASE, &release, 48).2, 16),
        TINY_STATUS_OK
    );

    let mut wrong_version = binary.clone();
    wrong_version[8..10].copy_from_slice(&u16::MAX.to_le_bytes());
    let mut trailing = binary.clone();
    trailing.push(0);
    for invalid in [
        binary[..binary.len() - 1].to_vec(),
        wrong_version,
        trailing,
        include_bytes!("artifacts/minimal_halt.tlbc").to_vec(),
        b"fn answer() { return 42 }".to_vec(),
    ] {
        let payload = values(&[u32_value(2), bytes_value(&invalid)]);
        let (status, written, response) = call(runtime, OPCODE_PROGRAM_LOAD, &payload, 48);
        assert_eq!(status, TINY_STATUS_OK);
        assert_eq!(written, 48);
        assert_eq!(read_u32(&response, 16), TINY_STATUS_INVALID_ARTIFACT);
        assert_eq!(read_u16(&response, HEADER_BYTES), 2);
    }
    // No magic sniffing: native binary bytes cannot masquerade as TLBC.
    assert_invalid_artifact(runtime, &binary);
    assert_eq!(tinylang_v2_runtime_destroy(runtime), TINY_STATUS_OK);
}

#[test]
fn program_load_creates_a_real_releasable_program_handle() {
    let runtime = create_runtime();
    let artifact = include_bytes!("artifacts/minimal_halt.tlbc");
    let payload = program_load_payload(artifact);
    let (status, written, response) = call(runtime, OPCODE_PROGRAM_LOAD, &payload, 48);
    assert_eq!(status, TINY_STATUS_OK);
    assert_eq!(written, 48);
    assert_eq!(read_u32(&response, 16), TINY_STATUS_OK);
    assert_eq!(read_u16(&response, HEADER_BYTES), 14); // Handle value.
    let program = read_u64(&response, HEADER_BYTES + 8);
    assert_ne!(program, 0);

    assert_eq!(tinylang_v2_runtime_destroy(runtime), TINY_STATUS_BUSY);

    let release_payload = values(&[handle_value(program)]);
    let (status, written, response) = call(runtime, OPCODE_HANDLE_RELEASE, &release_payload, 48);
    assert_eq!(status, TINY_STATUS_OK);
    assert_eq!(written, 32);
    assert_eq!(read_u32(&response, 16), TINY_STATUS_OK);
    assert_eq!(tinylang_v2_runtime_destroy(runtime), TINY_STATUS_OK);
}

#[test]
fn program_load_capacity_failure_inserts_no_handle() {
    let runtime = create_runtime();
    let artifact = include_bytes!("artifacts/minimal_halt.tlbc");
    let payload = program_load_payload(artifact);
    let (status, written, _) = call(runtime, OPCODE_PROGRAM_LOAD, &payload, 47);
    assert_eq!(status, TINY_STATUS_BUFFER_TOO_SMALL);
    assert_eq!(written, 48);
    assert_eq!(tinylang_v2_runtime_destroy(runtime), TINY_STATUS_OK);
}

#[test]
fn malformed_and_unverifiable_artifacts_insert_no_handle() {
    let runtime = create_runtime();

    let bad_opcode = include_bytes!("artifacts/invalid_bad_opcode.tlbc");
    assert_invalid_artifact(runtime, bad_opcode);

    let missing_halt = artifact(0, &[(1, 7, 0)]); // PUSH_INT without HALT.
    assert_invalid_artifact(runtime, &missing_halt);

    let mut noncanonical = include_bytes!("artifacts/minimal_halt.tlbc").to_vec();
    noncanonical[34] = 1; // Metadata directory flags.
    assert_invalid_artifact(runtime, &noncanonical);

    assert_eq!(tinylang_v2_runtime_destroy(runtime), TINY_STATUS_OK);
}

#[test]
fn committed_multi_instruction_artifacts_verify_and_release() {
    let runtime = create_runtime();
    for artifact in [
        include_bytes!("artifacts/push_print_halt.tlbc").as_slice(),
        include_bytes!("artifacts/arithmetic_print.tlbc").as_slice(),
    ] {
        let payload = program_load_payload(artifact);
        let (status, written, response) = call(runtime, OPCODE_PROGRAM_LOAD, &payload, 48);
        assert_eq!(status, TINY_STATUS_OK);
        assert_eq!(written, 48);
        assert_eq!(read_u32(&response, 16), TINY_STATUS_OK);
        let handle = read_u64(&response, HEADER_BYTES + 8);
        let release = values(&[handle_value(handle)]);
        assert_eq!(
            call(runtime, OPCODE_HANDLE_RELEASE, &release, 48).0,
            TINY_STATUS_OK
        );
    }
    assert_eq!(tinylang_v2_runtime_destroy(runtime), TINY_STATUS_OK);
}

#[test]
fn single_byte_artifact_mutations_never_escape_as_panics() {
    let base = include_bytes!("artifacts/minimal_halt.tlbc");
    let runtime = create_runtime();
    for index in 0..base.len() {
        for mask in [0x01u8, 0x80, 0xff] {
            let mut artifact = base.to_vec();
            artifact[index] ^= mask;
            let payload = program_load_payload(&artifact);
            let (status, written, response) = call(runtime, OPCODE_PROGRAM_LOAD, &payload, 48);
            assert_ne!(
                status, TINY_STATUS_PANIC,
                "panic status for artifact byte {index} mask {mask:#04x}"
            );
            if status == TINY_STATUS_OK
                && written == 48
                && read_u32(&response, 16) == TINY_STATUS_OK
            {
                let handle = read_u64(&response, HEADER_BYTES + 8);
                let release = values(&[handle_value(handle)]);
                assert_eq!(
                    call(runtime, OPCODE_HANDLE_RELEASE, &release, 48).0,
                    TINY_STATUS_OK
                );
            }
        }
    }
    assert_eq!(tinylang_v2_runtime_destroy(runtime), TINY_STATUS_OK);
}

fn assert_invalid_artifact(runtime: TinyRuntimeHandle, artifact: &[u8]) {
    let payload = program_load_payload(artifact);
    let (status, written, response) = call(runtime, OPCODE_PROGRAM_LOAD, &payload, 48);
    assert_eq!(status, TINY_STATUS_OK);
    assert_eq!(written, 48);
    assert_eq!(read_u32(&response, 16), TINY_STATUS_INVALID_ARTIFACT);
    assert_eq!(&response[HEADER_BYTES..HEADER_BYTES + 4], &[2, 0, 0, 0]);
}

fn artifact(slot_count: u32, instructions: &[(u16, i64, i64)]) -> Vec<u8> {
    let code_length = instructions.len() * INSTRUCTION_BYTES;
    let total_length = CODE_OFFSET + code_length;
    let mut bytes = vec![0u8; total_length];

    bytes[0..4].copy_from_slice(b"TLBC");
    bytes[4..6].copy_from_slice(&2u16.to_le_bytes());
    bytes[6..8].copy_from_slice(&0u16.to_le_bytes());
    bytes[8..10].copy_from_slice(&(ARTIFACT_HEADER_BYTES as u16).to_le_bytes());
    bytes[10..12].copy_from_slice(&(SECTION_ENTRY_BYTES as u16).to_le_bytes());
    bytes[12..16].copy_from_slice(&(total_length as u32).to_le_bytes());
    bytes[16..18].copy_from_slice(&2u16.to_le_bytes());

    write_section(&mut bytes, ARTIFACT_HEADER_BYTES, 1, METADATA_OFFSET, 16, 1);
    write_section(
        &mut bytes,
        ARTIFACT_HEADER_BYTES + SECTION_ENTRY_BYTES,
        2,
        CODE_OFFSET,
        code_length,
        instructions.len(),
    );

    bytes[METADATA_OFFSET..METADATA_OFFSET + 4].copy_from_slice(&0u32.to_le_bytes());
    bytes[METADATA_OFFSET + 4..METADATA_OFFSET + 8].copy_from_slice(&slot_count.to_le_bytes());

    for (index, (opcode, arg, arg2)) in instructions.iter().copied().enumerate() {
        let offset = CODE_OFFSET + index * INSTRUCTION_BYTES;
        bytes[offset..offset + 2].copy_from_slice(&opcode.to_le_bytes());
        bytes[offset + 8..offset + 16].copy_from_slice(&arg.to_le_bytes());
        bytes[offset + 16..offset + 24].copy_from_slice(&arg2.to_le_bytes());
    }
    bytes
}

fn write_section(
    bytes: &mut [u8],
    entry: usize,
    kind: u16,
    offset: usize,
    length: usize,
    count: usize,
) {
    bytes[entry..entry + 2].copy_from_slice(&kind.to_le_bytes());
    bytes[entry + 4..entry + 8].copy_from_slice(&(offset as u32).to_le_bytes());
    bytes[entry + 8..entry + 12].copy_from_slice(&(length as u32).to_le_bytes());
    bytes[entry + 12..entry + 16].copy_from_slice(&(count as u32).to_le_bytes());
}

fn program_load_payload(artifact: &[u8]) -> Vec<u8> {
    values(&[u32_value(1), bytes_value(artifact)])
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

fn call(
    runtime: TinyRuntimeHandle,
    opcode: u16,
    payload: &[u8],
    capacity: usize,
) -> (TinyStatus, u64, Vec<u8>) {
    let request = request(opcode, payload);
    let mut response = vec![0xa5; capacity];
    let mut written = 0;
    // SAFETY: request, response, and output length use live disjoint storage.
    let status = unsafe {
        tinylang_v2_call(
            runtime,
            TinyBuffer {
                data: request.as_ptr(),
                length: request.len() as u64,
            },
            TinyMutableBuffer {
                data: response.as_mut_ptr(),
                capacity: response.len() as u64,
            },
            &mut written,
        )
    };
    (status, written, response)
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

fn values(records: &[Vec<u8>]) -> Vec<u8> {
    records.concat()
}

fn record(tag: u16, auxiliary: u64, payload: &[u8]) -> Vec<u8> {
    let unpadded = VALUE_HEADER_BYTES + payload.len();
    let padded = (unpadded + 7) & !7;
    let mut bytes = vec![0u8; padded];
    bytes[0..2].copy_from_slice(&tag.to_le_bytes());
    bytes[4..8].copy_from_slice(&(payload.len() as u32).to_le_bytes());
    bytes[8..16].copy_from_slice(&auxiliary.to_le_bytes());
    bytes[VALUE_HEADER_BYTES..unpadded].copy_from_slice(payload);
    bytes
}

fn u32_value(value: u32) -> Vec<u8> {
    record(4, 0, &value.to_le_bytes())
}

fn bytes_value(value: &[u8]) -> Vec<u8> {
    record(12, 0, value)
}

fn handle_value(value: u64) -> Vec<u8> {
    record(14, value, &[])
}

fn read_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(bytes[offset..offset + 2].try_into().unwrap())
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn read_u64(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap())
}
