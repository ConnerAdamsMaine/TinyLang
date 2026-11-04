use tinylang::v2::*;

const MAX_VM_HEAP_BYTES: u64 = 4 * 1024 * 1024;
const MAX_VM_HEAP_OBJECTS: u32 = 1_000_000;
const MAX_VM_CALL_DEPTH: u32 = 16;

#[test]
fn runtime_handles_cannot_release_objects() {
    let runtime = create_runtime();
    let program = load_program(runtime);
    let (status, written, response) = call(runtime, OPCODE_HANDLE_RELEASE, &handle_value(runtime), 48);
    assert_eq!(status, TINY_STATUS_OK);
    assert_eq!(read_u32(&response, 16), TINY_STATUS_INVALID_HANDLE);
    assert_eq!(written, 48);

    // Rejection must leave the actual Program reference live.
    release(runtime, program);
    assert_eq!(tinylang_v2_runtime_destroy(runtime), TINY_STATUS_OK);
}

#[test]
fn object_handles_cannot_resolve_runtimes() {
    let runtime = create_runtime();
    let program = load_program(runtime);
    let (status, _, response) = call(runtime, OPCODE_VM_CREATE, &vm_create_payload(program, 1024, 32, 8, 0), 48);
    assert_eq!(status, TINY_STATUS_OK);
    assert_eq!(read_u32(&response, 16), TINY_STATUS_OK);
    let vm = read_u64(&response, HEADER_BYTES + 8);

    for object in [program, vm] {
        let (status, written, response) = call(object, OPCODE_NOP, &[], 32);
        assert_eq!(status, TINY_STATUS_INVALID_HANDLE);
        assert_eq!(written, 0);
        assert_eq!(response, vec![0xa5; 32]);
        assert_eq!(tinylang_v2_runtime_destroy(object), TINY_STATUS_INVALID_HANDLE);
    }

    release(runtime, program);
    release(runtime, vm);
    assert_eq!(tinylang_v2_runtime_destroy(runtime), TINY_STATUS_OK);
}

#[test]
fn stale_handles_cannot_revalidate_after_runtime_and_object_slot_reuse() {
    let runtime = create_runtime();
    let old_program = load_program(runtime);
    release(runtime, old_program);
    let program = load_program(runtime);
    assert_ne!(program, old_program);

    for (opcode, payload) in [
        (OPCODE_HANDLE_RELEASE, handle_value(old_program)),
        (OPCODE_VM_CREATE, vm_create_payload(old_program, 1024, 32, 8, 0)),
    ] {
        let (status, _, response) = call(runtime, opcode, &payload, 48);
        assert_eq!(status, TINY_STATUS_OK);
        assert_eq!(read_u32(&response, 16), TINY_STATUS_STALE_HANDLE);
    }
    release(runtime, program);
    assert_eq!(tinylang_v2_runtime_destroy(runtime), TINY_STATUS_OK);

    let next_runtime = create_runtime();
    assert_ne!(next_runtime, runtime);
    let (status, written, _) = call(runtime, OPCODE_NOP, &[], 32);
    assert_eq!(status, TINY_STATUS_STALE_HANDLE);
    assert_eq!(written, 0);
    assert_eq!(tinylang_v2_runtime_destroy(runtime), TINY_STATUS_STALE_HANDLE);
    let next_program = load_program(next_runtime);
    assert_ne!(next_program, program);
    release(next_runtime, next_program);
    assert_eq!(tinylang_v2_runtime_destroy(next_runtime), TINY_STATUS_OK);
}

#[test]
fn vm_retains_program_after_program_handle_release() {
    let runtime = create_runtime();
    let program = load_program(runtime);
    let (status, written, response) = call(
        runtime,
        OPCODE_VM_CREATE,
        &vm_create_payload(program, MAX_VM_HEAP_BYTES, MAX_VM_HEAP_OBJECTS, MAX_VM_CALL_DEPTH, 1),
        48,
    );
    assert_eq!(status, TINY_STATUS_OK);
    assert_eq!(written, 48);
    assert_eq!(read_u32(&response, 16), TINY_STATUS_OK);
    let vm = read_u64(&response, HEADER_BYTES + 8);
    assert_ne!(vm, 0);

    release(runtime, program);
    assert_eq!(tinylang_v2_runtime_destroy(runtime), TINY_STATUS_BUSY);

    // Handle wire types are identical, but the registry rejects a VM where a
    // Program is required.
    let (_, _, response) = call(runtime, OPCODE_VM_CREATE, &vm_create_payload(vm, 1024, 32, 8, 0), 48);
    assert_eq!(read_u32(&response, 16), TINY_STATUS_INVALID_HANDLE);

    release(runtime, vm);
    assert_eq!(tinylang_v2_runtime_destroy(runtime), TINY_STATUS_OK);
}

#[test]
fn vm_create_rejects_cross_runtime_and_stale_program_handles() {
    let owner = create_runtime();
    let other = create_runtime();
    let program = load_program(owner);
    let payload = vm_create_payload(program, 1024, 32, 8, 0);

    let (status, written, response) = call(other, OPCODE_VM_CREATE, &payload, 48);
    assert_eq!(status, TINY_STATUS_OK);
    assert_eq!(written, 48);
    assert_eq!(read_u32(&response, 16), TINY_STATUS_INVALID_HANDLE);

    release(owner, program);
    let (status, written, response) = call(owner, OPCODE_VM_CREATE, &payload, 48);
    assert_eq!(status, TINY_STATUS_OK);
    assert_eq!(written, 48);
    assert_eq!(read_u32(&response, 16), TINY_STATUS_STALE_HANDLE);

    assert_eq!(tinylang_v2_runtime_destroy(owner), TINY_STATUS_OK);
    assert_eq!(tinylang_v2_runtime_destroy(other), TINY_STATUS_OK);
}

#[test]
fn vm_create_preflights_capacity_and_enforces_runtime_caps_atomically() {
    let runtime = create_runtime();
    let program = load_program(runtime);
    let valid = vm_create_payload(program, 1024, 32, 8, 0);

    let (status, written, _) = call(runtime, OPCODE_VM_CREATE, &valid, 47);
    assert_eq!(status, TINY_STATUS_BUFFER_TOO_SMALL);
    assert_eq!(written, 48);

    for payload in [
        vm_create_payload(program, MAX_VM_HEAP_BYTES + 1, 32, 8, 0),
        vm_create_payload(program, 1024, MAX_VM_HEAP_OBJECTS + 1, 8, 0),
        vm_create_payload(program, 1024, 32, MAX_VM_CALL_DEPTH + 1, 0),
    ] {
        let (status, written, response) = call(runtime, OPCODE_VM_CREATE, &payload, 48);
        assert_eq!(status, TINY_STATUS_OK);
        assert_eq!(written, 48);
        assert_eq!(read_u32(&response, 16), TINY_STATUS_INVALID_ARGUMENT);
    }

    // If capacity failure or rejected limits inserted a hidden VM, runtime
    // destruction would remain BUSY after releasing this sole Program handle.
    release(runtime, program);
    assert_eq!(tinylang_v2_runtime_destroy(runtime), TINY_STATUS_OK);
}

fn load_program(runtime: TinyRuntimeHandle) -> TinyHandle {
    let artifact = include_bytes!("artifacts/minimal_halt.tlbc");
    let payload = values(&[u32_value(1), bytes_value(artifact)]);
    let (status, written, response) = call(runtime, OPCODE_PROGRAM_LOAD, &payload, 48);
    assert_eq!(status, TINY_STATUS_OK);
    assert_eq!(written, 48);
    assert_eq!(read_u32(&response, 16), TINY_STATUS_OK);
    read_u64(&response, HEADER_BYTES + 8)
}

fn release(runtime: TinyRuntimeHandle, handle: TinyHandle) {
    let payload = values(&[handle_value(handle)]);
    let (status, written, response) = call(runtime, OPCODE_HANDLE_RELEASE, &payload, 48);
    assert_eq!(status, TINY_STATUS_OK);
    assert_eq!(written, 32);
    assert_eq!(read_u32(&response, 16), TINY_STATUS_OK);
}

fn vm_create_payload(
    program: TinyHandle,
    max_heap_bytes: u64,
    max_heap_objects: u32,
    max_call_depth: u32,
    flags: u32,
) -> Vec<u8> {
    values(&[
        handle_value(program),
        u64_value(max_heap_bytes),
        u32_value(max_heap_objects),
        u32_value(max_call_depth),
        u32_value(flags),
    ])
}

fn create_runtime() -> TinyRuntimeHandle {
    let mut runtime = 0;
    // SAFETY: `runtime` is valid output storage.
    assert_eq!(unsafe { tinylang_v2_runtime_create(&mut runtime) }, TINY_STATUS_OK);
    runtime
}

fn call(runtime: TinyRuntimeHandle, opcode: u16, payload: &[u8], capacity: usize) -> (TinyStatus, u64, Vec<u8>) {
    let request = request(opcode, payload);
    let mut response = vec![0xa5; capacity];
    let mut written = 0;
    // SAFETY: request, response, and output length use live disjoint storage.
    let status = unsafe {
        tinylang_v2_call(
            runtime,
            TinyBuffer {
                data:   request.as_ptr(),
                length: request.len() as u64,
            },
            TinyMutableBuffer {
                data:     response.as_mut_ptr(),
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

fn u64_value(value: u64) -> Vec<u8> {
    record(5, 0, &value.to_le_bytes())
}

fn bytes_value(value: &[u8]) -> Vec<u8> {
    record(12, 0, value)
}

fn handle_value(value: u64) -> Vec<u8> {
    record(14, value, &[])
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn read_u64(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap())
}
