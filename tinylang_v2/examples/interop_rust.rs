use std::path::{Path, PathBuf};
use std::sync::Arc;

use tinylang_v2::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let supplied: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    let artifacts = if supplied.is_empty() {
        let base = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("artifacts");
        vec![
            base.join("minimal_halt.tlbc"),
            base.join("push_print_halt.tlbc"),
            base.join("arithmetic_print.tlbc"),
        ]
    } else {
        supplied
    };
    for path in artifacts {
        run_case(&path)?;
    }
    Ok(())
}

fn run_case(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let artifact = std::fs::read(path)?;
    let verified = decode_binary_artifact(&artifact)
        .map_err(|error| format!("{} failed v2 decoding: {error:?}", path.display()))?;
    let mut stdout = Vec::new();
    tinyone::run_program(
        Arc::new(verified.program().clone()),
        "vm",
        &mut stdout,
        Vec::new(),
    )?;

    let runtime = create_runtime();
    let load_payload = [u32_value(1), bytes_value(&artifact)].concat();
    let load = call(runtime, OPCODE_PROGRAM_LOAD, &load_payload, 48);
    expect_response(&load, TINY_STATUS_OK, 48, "PROGRAM_LOAD")?;
    let program = read_u64(&load.2, HEADER_BYTES + 8);

    let vm_payload = [
        handle_value(program),
        u64_value(1024),
        u32_value(32),
        u32_value(8),
        u32_value(0),
    ]
    .concat();
    let create = call(runtime, OPCODE_VM_CREATE, &vm_payload, 48);
    expect_response(&create, TINY_STATUS_OK, 48, "VM_CREATE")?;
    let vm = read_u64(&create.2, HEADER_BYTES + 8);

    release(runtime, program)?;
    release(runtime, vm)?;
    if tinylang_v2_runtime_destroy(runtime) != TINY_STATUS_OK {
        return Err("runtime destruction failed".into());
    }

    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("artifact");
    let output = String::from_utf8(stdout)?;
    let expected_output = match name {
        "minimal_halt.tlbc" => Some(""),
        "push_print_halt.tlbc" | "arithmetic_print.tlbc" => Some("42\n"),
        _ => None,
    };
    if expected_output.is_some_and(|expected| output != expected) {
        return Err(format!("{name} produced unexpected output {output:?}").into());
    }
    println!(
        "RS-TL {name}: decode=OK execute=OK load=OK vm_create=OK output={:?}",
        output
    );
    Ok(())
}

fn create_runtime() -> TinyRuntimeHandle {
    let mut runtime = 0;
    // SAFETY: `runtime` is valid output storage.
    let status = unsafe { tinylang_v2_runtime_create(&mut runtime) };
    assert_eq!(status, TINY_STATUS_OK);
    runtime
}

fn release(
    runtime: TinyRuntimeHandle,
    handle: TinyHandle,
) -> Result<(), Box<dyn std::error::Error>> {
    let response = call(runtime, OPCODE_HANDLE_RELEASE, &handle_value(handle), 48);
    expect_response(&response, TINY_STATUS_OK, 32, "HANDLE_RELEASE")
}

fn expect_response(
    result: &(TinyStatus, u64, Vec<u8>),
    response_status: TinyStatus,
    written: u64,
    operation: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    if result.0 != TINY_STATUS_OK
        || result.1 != written
        || read_u32(&result.2, 16) != response_status
    {
        return Err(format!("{operation} failed: call={} written={}", result.0, result.1).into());
    }
    Ok(())
}

fn call(
    runtime: TinyRuntimeHandle,
    opcode: u16,
    payload: &[u8],
    capacity: usize,
) -> (TinyStatus, u64, Vec<u8>) {
    let mut request = vec![0u8; HEADER_BYTES + payload.len()];
    request[0..4].copy_from_slice(&MAGIC);
    request[4..6].copy_from_slice(&SCHEMA_MAJOR.to_le_bytes());
    request[6..8].copy_from_slice(&SCHEMA_MINOR.to_le_bytes());
    request[8..10].copy_from_slice(&(HEADER_BYTES as u16).to_le_bytes());
    request[10..12].copy_from_slice(&opcode.to_le_bytes());
    request[20..24].copy_from_slice(&(payload.len() as u32).to_le_bytes());
    request[HEADER_BYTES..].copy_from_slice(payload);
    let mut response = vec![0u8; capacity];
    let mut written = 0;
    // SAFETY: request, response, and output length are live and disjoint.
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
    response.truncate(written.min(capacity as u64) as usize);
    (status, written, response)
}

fn record(tag: u16, auxiliary: u64, payload: &[u8]) -> Vec<u8> {
    let unpadded = VALUE_HEADER_BYTES + payload.len();
    let mut bytes = vec![0u8; (unpadded + 7) & !7];
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
    u32::from_le_bytes(
        bytes[offset..offset + 4]
            .try_into()
            .expect("bounded response"),
    )
}

fn read_u64(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(
        bytes[offset..offset + 8]
            .try_into()
            .expect("bounded response"),
    )
}
