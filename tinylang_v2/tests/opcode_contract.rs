use tinylang_v2::*;

#[test]
fn declared_program_load_schema_is_enforced_before_dispatch() {
    let runtime = create_runtime();
    let valid = values(&[u32_value(1), bytes_value(b"TLBC")]);
    let (status, written, response) = call(runtime, OPCODE_PROGRAM_LOAD, &valid, 256);
    assert_eq!(status, TINY_STATUS_OK);
    assert_eq!(written, 48);
    assert_eq!(read_u32(&response, 16), TINY_STATUS_INVALID_ARTIFACT);
    assert_eq!(&response[32..36], &[2, 0, 0, 0]);
    assert_eq!(read_u32(&response, 36), TINY_STATUS_INVALID_ARTIFACT);
    assert_eq!(&response[40..48], &u64::MAX.to_le_bytes());

    let missing_artifact = values(&[u32_value(1)]);
    assert_eq!(
        call(runtime, OPCODE_PROGRAM_LOAD, &missing_artifact, 256).0,
        TINY_STATUS_INVALID_FRAME
    );

    let wrong_format = values(&[u32_value(3), bytes_value(b"TLBC")]);
    assert_eq!(
        call(runtime, OPCODE_PROGRAM_LOAD, &wrong_format, 256).0,
        TINY_STATUS_INVALID_FRAME
    );
    assert_eq!(tinylang_v2_runtime_destroy(runtime), TINY_STATUS_OK);
}

#[test]
fn vm_create_schema_rejects_zero_limits_and_unknown_flags() {
    let runtime = create_runtime();
    let valid = values(&[
        handle_value(0x0000_0001_0000_0001),
        u64_value(1024),
        u32_value(32),
        u32_value(16),
        u32_value(1),
    ]);
    let (status, written, response) = call(runtime, OPCODE_VM_CREATE, &valid, 256);
    assert_eq!(status, TINY_STATUS_OK);
    assert_eq!(written, 48);
    assert_eq!(read_u32(&response, 16), TINY_STATUS_INVALID_HANDLE);
    assert_eq!(&response[32..36], &[1, 0, 0, 0]);
    assert_eq!(read_u32(&response, 36), TINY_STATUS_INVALID_HANDLE);
    assert_eq!(&response[40..48], &u64::MAX.to_le_bytes());

    let zero_heap = values(&[
        handle_value(0x0000_0001_0000_0001),
        u64_value(0),
        u32_value(32),
        u32_value(16),
        u32_value(0),
    ]);
    assert_eq!(
        call(runtime, OPCODE_VM_CREATE, &zero_heap, 256).0,
        TINY_STATUS_INVALID_FRAME
    );

    let unknown_flags = values(&[
        handle_value(0x0000_0001_0000_0001),
        u64_value(1024),
        u32_value(32),
        u32_value(16),
        u32_value(2),
    ]);
    assert_eq!(
        call(runtime, OPCODE_VM_CREATE, &unknown_flags, 256).0,
        TINY_STATUS_INVALID_FRAME
    );
    assert_eq!(tinylang_v2_runtime_destroy(runtime), TINY_STATUS_OK);
}

#[test]
fn vm_invoke_executes_a_compiler_produced_function_and_returns_canonical_i64() {
    let runtime = create_runtime();
    let (program, vm) = create_vm(runtime, "fn difference(a, b) { return a - b }");
    let arguments = vec![i64_value(50), i64_value(8)];
    let (status, written, response) = invoke(runtime, vm, 0, 100, 24, &arguments, 56);

    assert_eq!(status, TINY_STATUS_OK);
    assert_eq!(written, 56);
    assert_eq!(read_u32(&response, 16), TINY_STATUS_OK);
    assert_eq!(read_u16(&response, 32), 9);
    assert_eq!(read_i64(&response, 48), 42);
    release_handle(runtime, vm);
    release_handle(runtime, program);
    assert_eq!(tinylang_v2_runtime_destroy(runtime), TINY_STATUS_OK);
}

#[test]
fn vm_invoke_rejects_unknown_exports_and_invalid_arguments() {
    let runtime = create_runtime();
    let (program, vm) = create_vm(runtime, "fn difference(a, b) { return a - b }");
    let arguments = vec![i64_value(50), i64_value(8)];
    let (status, written, response) = invoke(runtime, vm, 2, 100, 24, &arguments, 56);
    assert_eq!(status, TINY_STATUS_OK);
    assert_eq!(written, 48);
    assert_eq!(read_u32(&response, 16), TINY_STATUS_INVALID_ARGUMENT);

    let wrong_count = vec![i64_value(50)];
    let (status, _, response) = invoke(runtime, vm, 0, 100, 24, &wrong_count, 56);
    assert_eq!(status, TINY_STATUS_OK);
    assert_eq!(read_u32(&response, 16), TINY_STATUS_INVALID_ARGUMENT);

    let wrong_type = vec![utf8_value(b"not a scalar")];
    let (status, _, response) = invoke(runtime, vm, 0, 100, 24, &wrong_type, 56);
    assert_eq!(status, TINY_STATUS_OK);
    assert_eq!(read_u32(&response, 16), TINY_STATUS_INVALID_ARGUMENT);

    let (status, written, response) = invoke(runtime, vm, 0, 100, 24, &arguments, 56);
    assert_eq!(status, TINY_STATUS_OK);
    assert_eq!(written, 56);
    assert_eq!(read_i64(&response, 48), 42);
    release_handle(runtime, vm);
    release_handle(runtime, program);
    assert_eq!(tinylang_v2_runtime_destroy(runtime), TINY_STATUS_OK);
}

#[test]
fn vm_invoke_round_trips_supported_scalar_value_records_without_coercion() {
    let runtime = create_runtime();
    let (program, vm) = create_vm(runtime, "fn identity(value) { return value }");
    let values = [
        null_value(),
        bool_value(true),
        i8_value(-7),
        i16_value(-300),
        i32_value(-70_000),
        i64_value(-42),
        u8_value(250),
        u16_value(60_000),
        u32_value(4_000_000_000),
        u64_value(u64::MAX),
        f32_value(1.25),
        f64_value(-2.5),
    ];

    for value in values {
        let (status, written, response) =
            invoke(runtime, vm, 0, 100, 24, std::slice::from_ref(&value), 56);
        assert_eq!(status, TINY_STATUS_OK);
        assert_eq!(read_u32(&response, 16), TINY_STATUS_OK);
        assert_eq!(written as usize, HEADER_BYTES + value.len());
        assert_eq!(&response[HEADER_BYTES..written as usize], value);
    }

    release_handle(runtime, vm);
    release_handle(runtime, program);
    assert_eq!(tinylang_v2_runtime_destroy(runtime), TINY_STATUS_OK);
}

#[test]
fn vm_invoke_bounds_results_and_reports_unsupported_heap_results() {
    let runtime = create_runtime();
    let (program, vm) = create_vm(
        runtime,
        "fn identity(value) { return value }\nfn returns_text() { return \"text\" }",
    );
    let arguments = vec![i64_value(42)];
    let (status, written, response) = invoke(runtime, vm, 0, 100, 16, &arguments, 48);
    assert_eq!(status, TINY_STATUS_OK);
    assert_eq!(written, 48);
    assert_eq!(read_u32(&response, 16), TINY_STATUS_VM_TRAP);

    let no_arguments = Vec::new();
    let (status, written, response) = invoke(runtime, vm, 1, 100, 24, &no_arguments, 56);
    assert_eq!(status, TINY_STATUS_OK);
    assert_eq!(written, 48);
    assert_eq!(read_u32(&response, 16), TINY_STATUS_INVALID_ARGUMENT);

    let (status, written, response) = invoke(runtime, vm, 0, 100, 24, &arguments, 56);
    assert_eq!(status, TINY_STATUS_OK);
    assert_eq!(written, 56);
    assert_eq!(read_i64(&response, 48), 42);
    release_handle(runtime, vm);
    release_handle(runtime, program);
    assert_eq!(tinylang_v2_runtime_destroy(runtime), TINY_STATUS_OK);
}

#[test]
fn vm_invoke_rejects_handles_from_other_runtimes_wrong_kinds_and_stale_vms() {
    let runtime = create_runtime();
    let other_runtime = create_runtime();
    let (program, vm) = create_vm(runtime, "fn identity(value) { return value }");
    let arguments = vec![i64_value(42)];

    let (status, _, response) = invoke(other_runtime, vm, 0, 100, 24, &arguments, 56);
    assert_eq!(status, TINY_STATUS_OK);
    assert_eq!(read_u32(&response, 16), TINY_STATUS_INVALID_HANDLE);

    let (status, _, response) = invoke(runtime, program, 0, 100, 24, &arguments, 56);
    assert_eq!(status, TINY_STATUS_OK);
    assert_eq!(read_u32(&response, 16), TINY_STATUS_INVALID_HANDLE);

    release_handle(runtime, vm);
    let (status, _, response) = invoke(runtime, vm, 0, 100, 24, &arguments, 56);
    assert_eq!(status, TINY_STATUS_OK);
    assert_eq!(read_u32(&response, 16), TINY_STATUS_STALE_HANDLE);

    release_handle(runtime, program);
    assert_eq!(tinylang_v2_runtime_destroy(runtime), TINY_STATUS_OK);
    assert_eq!(tinylang_v2_runtime_destroy(other_runtime), TINY_STATUS_OK);
}

#[test]
fn vm_invoke_preflights_response_capacity_before_execution() {
    let runtime = create_runtime();
    let (program, vm) = create_vm(runtime, "fn difference(a, b) { return a - b }");
    let arguments = vec![i64_value(50), i64_value(8)];
    let (status, written, _) = invoke(runtime, vm, 0, 100, 24, &arguments, 55);
    assert_eq!(status, TINY_STATUS_BUFFER_TOO_SMALL);
    assert_eq!(written, 56);

    let (status, written, response) = invoke(runtime, vm, 0, 100, 24, &arguments, 56);
    assert_eq!(status, TINY_STATUS_OK);
    assert_eq!(written, 56);
    assert_eq!(read_i64(&response, 48), 42);
    release_handle(runtime, vm);
    release_handle(runtime, program);
    assert_eq!(tinylang_v2_runtime_destroy(runtime), TINY_STATUS_OK);
}

#[test]
fn vm_invoke_fuel_trap_does_not_poison_the_persistent_vm() {
    let runtime = create_runtime();
    let (program, vm) = create_vm(runtime, "fn difference(a, b) { return a - b }");
    let arguments = vec![i64_value(50), i64_value(8)];
    let (status, _, response) = invoke(runtime, vm, 0, 1, 24, &arguments, 56);
    assert_eq!(status, TINY_STATUS_OK);
    assert_eq!(read_u32(&response, 16), TINY_STATUS_VM_TRAP);

    let (status, _, response) = invoke(runtime, vm, 0, 100, 24, &arguments, 56);
    assert_eq!(status, TINY_STATUS_OK);
    assert_eq!(read_u32(&response, 16), TINY_STATUS_OK);
    assert_eq!(read_i64(&response, 48), 42);
    release_handle(runtime, vm);
    release_handle(runtime, program);
    assert_eq!(tinylang_v2_runtime_destroy(runtime), TINY_STATUS_OK);
}

#[test]
fn handle_release_preflights_worst_case_response_before_registry_access() {
    let runtime = create_runtime();
    let payload = values(&[handle_value(0x0000_0001_0000_0001)]);

    let (status, written, _) = call(runtime, OPCODE_HANDLE_RELEASE, &payload, 32);
    assert_eq!(status, TINY_STATUS_BUFFER_TOO_SMALL);
    assert_eq!(written, 48);

    let (status, written, response) = call(runtime, OPCODE_HANDLE_RELEASE, &payload, 48);
    assert_eq!(status, TINY_STATUS_OK);
    assert_eq!(written, 48);
    assert_eq!(read_u32(&response, 16), TINY_STATUS_INVALID_HANDLE);
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

fn u64_value(value: u64) -> Vec<u8> {
    record(5, 0, &value.to_le_bytes())
}

fn bytes_value(value: &[u8]) -> Vec<u8> {
    record(12, 0, value)
}

fn handle_value(value: u64) -> Vec<u8> {
    record(14, value, &[])
}

fn i64_value(value: i64) -> Vec<u8> {
    record(9, 0, &value.to_le_bytes())
}

fn i8_value(value: i8) -> Vec<u8> {
    record(6, 0, &value.to_le_bytes())
}

fn i16_value(value: i16) -> Vec<u8> {
    record(7, 0, &value.to_le_bytes())
}

fn i32_value(value: i32) -> Vec<u8> {
    record(8, 0, &value.to_le_bytes())
}

fn bool_value(value: bool) -> Vec<u8> {
    record(1, 0, &[u8::from(value)])
}

fn u8_value(value: u8) -> Vec<u8> {
    record(2, 0, &value.to_le_bytes())
}

fn u16_value(value: u16) -> Vec<u8> {
    record(3, 0, &value.to_le_bytes())
}

fn null_value() -> Vec<u8> {
    record(0, 0, &[])
}

fn f32_value(value: f32) -> Vec<u8> {
    record(10, 0, &value.to_bits().to_le_bytes())
}

fn f64_value(value: f64) -> Vec<u8> {
    record(11, 0, &value.to_bits().to_le_bytes())
}

fn utf8_value(value: &[u8]) -> Vec<u8> {
    record(13, 0, value)
}

fn sequence_value(value_records: &[u8], count: u64) -> Vec<u8> {
    record(17, count, value_records)
}

fn create_vm(runtime: TinyRuntimeHandle, source: &str) -> (TinyHandle, TinyHandle) {
    let compiled = tinyone::compile_source(source).unwrap();
    let artifact = compiled.to_binary_artifact().unwrap();
    let payload = values(&[u32_value(2), bytes_value(&artifact)]);
    let (status, written, response) = call(runtime, OPCODE_PROGRAM_LOAD, &payload, 48);
    assert_eq!(status, TINY_STATUS_OK);
    assert_eq!(written, 48);
    assert_eq!(read_u32(&response, 16), TINY_STATUS_OK);
    let program = read_u64(&response, 40);

    let payload = values(&[
        handle_value(program),
        u64_value(1024),
        u32_value(16),
        u32_value(4),
        u32_value(0),
    ]);
    let (status, written, response) = call(runtime, OPCODE_VM_CREATE, &payload, 48);
    assert_eq!(status, TINY_STATUS_OK);
    assert_eq!(written, 48);
    assert_eq!(read_u32(&response, 16), TINY_STATUS_OK);
    (program, read_u64(&response, 40))
}

fn invoke(
    runtime: TinyRuntimeHandle,
    vm: TinyHandle,
    export_id: u32,
    fuel: u64,
    max_result_bytes: u32,
    arguments: &[Vec<u8>],
    capacity: usize,
) -> (TinyStatus, u64, Vec<u8>) {
    let payload = values(&[
        handle_value(vm),
        u32_value(export_id),
        u64_value(fuel),
        u32_value(max_result_bytes),
        sequence_value(&values(arguments), arguments.len() as u64),
    ]);
    call(runtime, OPCODE_VM_INVOKE, &payload, capacity)
}

fn release_handle(runtime: TinyRuntimeHandle, handle: TinyHandle) {
    let payload = values(&[handle_value(handle)]);
    let (status, written, response) = call(runtime, OPCODE_HANDLE_RELEASE, &payload, 48);
    assert_eq!(status, TINY_STATUS_OK);
    assert_eq!(written, 32);
    assert_eq!(read_u32(&response, 16), TINY_STATUS_OK);
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn read_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(bytes[offset..offset + 2].try_into().unwrap())
}

fn read_u64(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap())
}

fn read_i64(bytes: &[u8], offset: usize) -> i64 {
    i64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap())
}
