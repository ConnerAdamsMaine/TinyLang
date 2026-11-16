use std::ffi::CString;
use std::io::{Read, Write};
use std::os::raw::c_char;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::process::Command;
use std::time::Duration;

mod sandbox;
mod worker;
pub mod v2;

use serde::{Deserialize, Serialize};
use serde_json::{Value as JsonValue, json};

use crate::language::bytecode::artifact::MAX_ARTIFACT_BYTES;
use crate::{
    JitProgram, ModuleCapabilities, ModulePermissions, Result, RuntimeValue, TynraHeapStats, TynraError, TynraMemory,
    VerifiedProgram, compile_authorized_source_verified, compile_file_verified, compile_source_verified, lex_source,
    module_source_hash, run_verified_program_report,
};
use worker::{WorkDir, WorkerLimits};

/// The C ABI version for the Tynra-prefixed JSON entry points.
pub const TYNRA_ABI_VERSION: u32 = 2;

/// Maximum UTF-8 source text accepted by the C ABI, excluding the trailing NUL.
pub(crate) const MAX_FFI_SOURCE_BYTES: usize = 1024 * 1024;
/// Maximum path text accepted by the C ABI, excluding the trailing NUL.
pub(crate) const MAX_FFI_PATH_BYTES: usize = 32 * 1024;
/// Maximum execution-mode text accepted by the C ABI, excluding the trailing NUL.
const MAX_FFI_MODE_BYTES: usize = 16;
/// Maximum JSON input-queue text accepted by the C ABI, excluding the trailing NUL.
const MAX_FFI_INPUTS_BYTES: usize = 8 * 1024 * 1024;
const SANDBOX_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_SANDBOX_RESPONSE_BYTES: usize = 16 * 1024 * 1024;

/// Version of the single-request worker protocol, carried by every request
/// variant. The worker defaults a missing field (old hosts) and refuses
/// versions above its own, so a host that negotiates a future protocol
/// cannot make an older worker silently misread a request.
const SANDBOX_PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
enum SandboxRequest {
    RunSource {
        #[serde(default)]
        protocol_version: u32,
        source: String,
        mode: String,
        inputs: Vec<String>,
    },
    RunFile {
        #[serde(default)]
        protocol_version: u32,
        path: String,
        mode: String,
        inputs: Vec<String>,
    },
    RunArtifact {
        #[serde(default)]
        protocol_version: u32,
        artifact_json: String,
        mode: String,
        inputs: Vec<String>,
    },
    JitListing {
        #[serde(default)]
        protocol_version: u32,
        artifact_json: String,
    },
}

/// Fails closed on a protocol version the worker does not implement. A
/// request without the field deserializes to `0` (v1 semantics); a version
/// above the worker's own must never be silently downgraded.
fn ensure_supported_protocol(protocol_version: u32) -> Result<()> {
    if protocol_version > SANDBOX_PROTOCOL_VERSION {
        return Err(TynraError::runtime(format!(
            "sandbox request protocol version {protocol_version} is above the worker's supported version {SANDBOX_PROTOCOL_VERSION}"
        )));
    }
    Ok(())
}

#[unsafe(no_mangle)]
/// Return the declared stable `Tynra` C ABI version.
pub extern "C" fn tynra_abi_version() -> u32 {
    TYNRA_ABI_VERSION
}

/// # Safety
///
/// `value` must be null or a pointer returned by a `Tynra` C-ABI function
/// that has not already been freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tynra_free_string(value: *mut c_char) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        if value.is_null() {
            return;
        }
        unsafe {
            drop(CString::from_raw(value));
        }
    }));
}

/// # Safety
///
/// `source` must be non-null and point to a valid NUL-terminated UTF-8 C
/// string for the duration of the call and must be no larger than 1 MiB.
/// A null pointer or oversized string returns a compile error response.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tynra_lex_source_json(source: *const c_char) -> *mut c_char {
    respond(|| {
        let source = read_string_limited(source, "source", MAX_FFI_SOURCE_BYTES)?;
        Ok(json!({"tokens": lex_source(&source)?}))
    })
}

/// # Safety
///
/// `source` must be non-null and point to a valid NUL-terminated UTF-8 C
/// string for the duration of the call and must be no larger than 1 MiB.
/// A null pointer or oversized string returns a compile error response.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tynra_compile_source_json(source: *const c_char) -> *mut c_char {
    respond(|| {
        let source = read_string_limited(source, "source", MAX_FFI_SOURCE_BYTES)?;
        program_payload(compile_source_verified(&source)?)
    })
}

/// # Safety
///
/// `path` must be non-null and point to a valid NUL-terminated UTF-8 C string
/// for the duration of the call and must be no larger than 32 KiB. A null
/// pointer or oversized string returns a compile error response.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tynra_compile_file_json(path: *const c_char) -> *mut c_char {
    respond(|| {
        let path = read_string_limited(path, "path", MAX_FFI_PATH_BYTES)?;
        program_payload(compile_file_verified(Path::new(&path))?)
    })
}

/// # Safety
///
/// `source` and `mode` must be non-null and point to valid NUL-terminated
/// UTF-8 C strings for the duration of the call. `source` is limited to 1 MiB
/// and `mode` to 16 bytes. `inputs_json` is nullable; null means an empty input
/// queue. A non-null `inputs_json` is limited to 8 MiB.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tynra_run_source_json(
    source: *const c_char,
    mode: *const c_char,
    inputs_json: *const c_char,
) -> *mut c_char {
    respond(|| {
        let source = read_string_limited(source, "source", MAX_FFI_SOURCE_BYTES)?;
        let mode = read_string_limited(mode, "mode", MAX_FFI_MODE_BYTES)?;
        let inputs = read_inputs(inputs_json)?;
        run_sandboxed(SandboxRequest::RunSource {
            protocol_version: SANDBOX_PROTOCOL_VERSION,
            source,
            mode,
            inputs,
        })
    })
}

/// # Safety
///
/// `path` and `mode` must be non-null and point to valid NUL-terminated UTF-8
/// C strings for the duration of the call. `path` is limited to 32 KiB and
/// `mode` to 16 bytes. `inputs_json` is nullable; null means an empty input
/// queue. A non-null `inputs_json` is limited to 8 MiB.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tynra_run_file_json(
    path: *const c_char,
    mode: *const c_char,
    inputs_json: *const c_char,
) -> *mut c_char {
    respond(|| {
        let path = read_string_limited(path, "path", MAX_FFI_PATH_BYTES)?;
        let mode = read_string_limited(mode, "mode", MAX_FFI_MODE_BYTES)?;
        let inputs = read_inputs(inputs_json)?;
        run_sandboxed(SandboxRequest::RunFile {
            protocol_version: SANDBOX_PROTOCOL_VERSION,
            path,
            mode,
            inputs,
        })
    })
}

/// # Safety
///
/// `artifact_json` and `mode` must be non-null and point to valid NUL-terminated
/// UTF-8 C strings for the duration of the call. `mode` is limited to 16 bytes.
/// `inputs_json` is nullable; null means an empty input queue. A non-null
/// `inputs_json` is limited to 8 MiB.
/// `artifact_json` must not exceed the documented artifact byte limit.
/// `mode` is limited to 16 bytes and `inputs_json` to 8 MiB.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tynra_run_artifact_json(
    artifact_json: *const c_char,
    mode: *const c_char,
    inputs_json: *const c_char,
) -> *mut c_char {
    respond(|| {
        let artifact_json = read_string_limited(artifact_json, "artifact", MAX_ARTIFACT_BYTES)?;
        let mode = read_string_limited(mode, "mode", MAX_FFI_MODE_BYTES)?;
        let inputs = read_inputs(inputs_json)?;
        run_sandboxed(SandboxRequest::RunArtifact {
            protocol_version: SANDBOX_PROTOCOL_VERSION,
            artifact_json,
            mode,
            inputs,
        })
    })
}

/// # Safety
///
/// `artifact_json` must be non-null and point to a valid NUL-terminated UTF-8
/// C string for the duration of the call. A null pointer returns a compile
/// error response. It must not exceed the documented artifact byte limit.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tynra_jit_listing_json(artifact_json: *const c_char) -> *mut c_char {
    respond(|| {
        let artifact_json = read_string_limited(artifact_json, "artifact", MAX_ARTIFACT_BYTES)?;
        run_sandboxed(SandboxRequest::JitListing {
            protocol_version: SANDBOX_PROTOCOL_VERSION,
            artifact_json,
        })
    })
}

fn run_sandboxed(request: SandboxRequest) -> Result<JsonValue> {
    let request = serde_json::to_vec(&request)
        .map_err(|error| TynraError::runtime(format!("sandbox request serialization failed: {error}")))?;
    let worker = sandbox_worker_path()?;
    let output = sandbox::exchange(
        Command::new(worker).arg("--tynra-sandbox-worker"),
        &request,
        SANDBOX_TIMEOUT,
        MAX_SANDBOX_RESPONSE_BYTES,
    )?;
    let response: JsonValue = serde_json::from_slice(&output)
        .map_err(|error| TynraError::runtime(format!("sandbox response was invalid JSON: {error}")))?;
    if response.get("ok").and_then(JsonValue::as_bool) == Some(true) {
        return response
            .get("value")
            .cloned()
            .ok_or_else(|| TynraError::runtime("sandbox response omitted its value"));
    }
    let message = response
        .get("error")
        .and_then(JsonValue::as_str)
        .unwrap_or("sandbox worker returned an invalid error response");
    match response.get("kind").and_then(JsonValue::as_str) {
        Some("compile") => Err(TynraError::compile(message)),
        _ => Err(TynraError::runtime(message)),
    }
}

fn sandbox_worker_path() -> Result<std::path::PathBuf> {
    if let Some(path) = std::env::var_os("TYNRA_SANDBOX_WORKER") {
        let path = std::path::PathBuf::from(path);
        if path.is_file() {
            return Ok(path);
        }
        return Err(TynraError::runtime(format!(
            "TYNRA_SANDBOX_WORKER does not identify a file: {}",
            path.display()
        )));
    }

    let current = std::env::current_exe()
        .map_err(|error| TynraError::runtime(format!("could not locate sandbox worker: {error}")))?;
    let directories: Vec<_> = current.ancestors().map(std::path::Path::to_path_buf).collect();
    let name = if cfg!(windows) {
        "tynra-sandbox-worker.exe"
    } else {
        "tynra-sandbox-worker"
    };
    for directory in directories {
        let exact = directory.join(name);
        if exact.is_file() {
            return Ok(exact);
        }
        let deps = directory.join("deps");
        if let Ok(entries) = std::fs::read_dir(deps)
            && let Some(path) = entries.flatten().map(|entry| entry.path()).find(|path| {
                path.is_file()
                    && path.file_name().and_then(|value| value.to_str()).is_some_and(|value| {
                        value.starts_with("tynra_sandbox_worker-")
                            && value.ends_with(if cfg!(windows) { ".exe" } else { "" })
                    })
            })
        {
            return Ok(path);
        }
    }
    Err(TynraError::runtime(format!(
        "sandbox worker not found; install {name} beside the host or set TYNRA_SANDBOX_WORKER"
    )))
}

/// Entry point for the dedicated worker process used by the C ABI.
///
/// The worker applies the playground safety contract to itself before reading
/// a request: self-applied platform resource limits, a confined per-run
/// working directory, and the fixed deny-all capability policy for every
/// executed source (see [`execute_sandbox_request`]).
#[doc(hidden)]
pub fn sandbox_worker_main() {
    // Fail closed: if the platform caps cannot be applied, answer with an
    // error response instead of serving a request with host resources
    // unbounded.
    let limits = match WorkerLimits::acquire() {
        Ok(limits) => limits,
        Err(error) => {
            let _ = std::io::stdout().write_all(response_cstring(|| Err(error)).as_bytes());
            return;
        }
    };
    let response = match WorkDir::acquire() {
        Ok(work_dir) => {
            let response = worker_response();
            drop(work_dir);
            response
        }
        Err(error) => response_cstring(|| Err(error)),
    };
    let _ = std::io::stdout().write_all(response.as_bytes());
    // The Windows job object terminates the worker when its last handle
    // closes. The handle must therefore stay open through the response write
    // and until process exit; forgetting the guard here is the deliberate
    // lifetime contract, not an unbounded leak.
    std::mem::forget(limits);
}

fn worker_response() -> CString {
    let mut request_bytes = Vec::new();
    let read_result = std::io::stdin()
        .take((MAX_ARTIFACT_BYTES + MAX_FFI_INPUTS_BYTES + MAX_FFI_SOURCE_BYTES) as u64)
        .read_to_end(&mut request_bytes);
    match read_result {
        Ok(_) => match serde_json::from_slice::<SandboxRequest>(&request_bytes) {
            Ok(request) => response_cstring(|| execute_sandbox_request(request)),
            Err(error) => response_cstring(|| Err(TynraError::runtime(format!("invalid sandbox request: {error}")))),
        },
        Err(error) => response_cstring(|| Err(TynraError::runtime(format!("sandbox request read failed: {error}")))),
    }
}

/// Executes one sandboxed request inside the worker.
///
/// The worker protocol carries no policy field: v1 defines no grant
/// mechanism, and the sandbox must be safe by default. Every executed source
/// therefore runs under the fixed deny-all policy — `RunSource` compiles
/// through the authorized path's deny-all default, `RunFile` pins the root
/// and every imported module to deny-all (a caller-chosen `Config.toml` is
/// not a grant, for root or module code), and `RunArtifact` already executes
/// decoded artifacts without authority because artifact policy is data, not
/// a grant. Any future protocol field that requests a grant must be refused
/// here, not negotiated.
fn execute_sandbox_request(request: SandboxRequest) -> Result<JsonValue> {
    match request {
        SandboxRequest::RunSource {
            protocol_version,
            source,
            mode,
            inputs,
        } => {
            ensure_supported_protocol(protocol_version)?;
            // The authorized compile path's default policy is exactly the
            // worker's fixed deny-all policy; only a verified signed manifest
            // could pin anything else, and the worker protocol never supplies
            // one. The digest binds the compile to the exact bytes received
            // over the request pipe.
            let program = compile_authorized_source_verified(
                source.as_bytes(),
                &module_source_hash(source.as_bytes()),
                "<source>",
            )?;
            let mut stdout = Vec::new();
            let report = run_verified_program_report(&program, &mode, &mut stdout, inputs)?;
            run_payload(stdout, report.memory, report.heap_before_shutdown, report.heap_after_shutdown)
        }
        SandboxRequest::RunFile {
            protocol_version,
            path,
            mode,
            inputs,
        } => {
            ensure_supported_protocol(protocol_version)?;
            let program = compile_file_verified(Path::new(&path))?;
            run_compiled_program(pinned_deny_all(program)?, &mode, inputs)
        }
        SandboxRequest::RunArtifact {
            protocol_version,
            artifact_json,
            mode,
            inputs,
        } => {
            ensure_supported_protocol(protocol_version)?;
            let artifact = serde_json::from_str(&artifact_json)
                .map_err(|error| TynraError::compile(format!("artifact must be valid JSON: {error}")))?;
            let program = VerifiedProgram::from_artifact(artifact)?;
            run_compiled_program(program, &mode, inputs)
        }
        SandboxRequest::JitListing {
            protocol_version,
            artifact_json,
        } => {
            ensure_supported_protocol(protocol_version)?;
            let artifact = serde_json::from_str(&artifact_json)
                .map_err(|error| TynraError::compile(format!("artifact must be valid JSON: {error}")))?;
            let program = VerifiedProgram::from_artifact(artifact)?;
            Ok(json!({"listing": JitProgram::compile_verified(&program)?.listing()}))
        }
    }
}

fn respond(callback: impl FnOnce() -> Result<JsonValue>) -> *mut c_char {
    match catch_unwind(AssertUnwindSafe(|| response_cstring(callback))) {
        Ok(response) => response.into_raw(),
        Err(_) => fallback_response().into_raw(),
    }
}

fn response_cstring(callback: impl FnOnce() -> Result<JsonValue>) -> CString {
    let payload = match catch_unwind(AssertUnwindSafe(callback)) {
        Ok(Ok(value)) => json!({"ok": true, "value": value}),
        Ok(Err(error)) => error_payload(&error),
        Err(_) => {
            json!({
                "ok": false,
                "kind": "panic",
                "error": "Tynra panicked across the FFI boundary"
            })
        }
    };
    match serde_json::to_string(&payload) {
        Ok(text) => cstring_or_fallback(text),
        Err(_) => fallback_response(),
    }
}

fn error_payload(error: &TynraError) -> JsonValue {
    let kind = match error {
        TynraError::Compile(_) => "compile",
        TynraError::Runtime(_) => "runtime",
    };
    json!({"ok": false, "kind": kind, "error": error.to_string()})
}

fn cstring_or_fallback(text: String) -> CString {
    CString::new(text).unwrap_or_else(|_| fallback_response())
}

fn fallback_response() -> CString {
    const FALLBACK: &[u8] = b"{\"ok\":false,\"kind\":\"panic\",\"error\":\"response serialization failed\"}\0";
    // The byte string above is static valid JSON followed by exactly one NUL.
    unsafe { CString::from_vec_with_nul_unchecked(FALLBACK.to_vec()) }
}

fn read_json(value: *const c_char, name: &str) -> Result<JsonValue> {
    let text = read_string_limited(value, name, MAX_FFI_INPUTS_BYTES)?;
    serde_json::from_str(&text).map_err(|error| TynraError::compile(format!("{name} must be valid JSON: {error}")))
}

fn read_string_limited(value: *const c_char, name: &str, max_bytes: usize) -> Result<String> {
    if value.is_null() {
        return Err(TynraError::compile(format!("{name} pointer was null")));
    }
    for len in 0..=max_bytes {
        let byte = unsafe { *value.add(len) };
        if byte == 0 {
            let bytes = unsafe { std::slice::from_raw_parts(value.cast::<u8>(), len) };
            return std::str::from_utf8(bytes)
                .map(ToOwned::to_owned)
                .map_err(|error| TynraError::compile(format!("{name} must be UTF-8: {error}")));
        }
    }
    Err(TynraError::compile(format!("{name} exceeds byte limit {max_bytes}")))
}

fn read_inputs(value: *const c_char) -> Result<Vec<String>> {
    if value.is_null() {
        return Ok(Vec::new());
    }
    let data = read_json(value, "inputs")?;
    serde_json::from_value(data)
        .map_err(|error| TynraError::runtime(format!("inputs must be a JSON string list: {error}")))
}

fn program_payload(program: VerifiedProgram) -> Result<JsonValue> {
    // The frozen v1 response cannot represent declaration decorators. Refuse
    // lossy export; the Rust artifact APIs preserve the complete metadata.
    if program
        .program()
        .functions()
        .iter()
        .any(|function| !function.decorators().is_empty())
    {
        return Err(TynraError::compile(
            "Decorator metadata requires the Rust artifact API; the JSON C compile response cannot represent it",
        ));
    }
    let mut artifact = program.program().to_artifact();
    if let Some(functions) = artifact.get_mut("functions").and_then(JsonValue::as_array_mut) {
        for function in functions {
            if let Some(function) = function.as_object_mut() {
                function.remove("decorators");
            }
        }
    }
    Ok(json!({
        "artifact": artifact,
        "fingerprint": program.fingerprint(),
    }))
}

fn run_compiled_program(program: VerifiedProgram, mode: &str, inputs: Vec<String>) -> Result<JsonValue> {
    let mut stdout = Vec::new();
    let report = run_verified_program_report(&program, mode, &mut stdout, inputs)?;
    run_payload(stdout, report.memory, report.heap_before_shutdown, report.heap_after_shutdown)
}

/// Pins an already-compiled program to the worker's fixed deny-all policy
/// before re-verification. Caller-chosen files carry whatever their nearest
/// `Config.toml` grants; the worker protocol has no grant mechanism, so the
/// root's authority is lowered to deny-all.
fn pinned_deny_all(program: VerifiedProgram) -> Result<VerifiedProgram> {
    let mut program = program.into_program();
    program.root_capabilities = ModuleCapabilities::none();
    program.root_permissions = ModulePermissions::from_capabilities(ModuleCapabilities::none());
    // Imported modules keep the grants their nearest on-disk `Config.toml`
    // or package manifest gave them (`modules.X.permissions` ∩
    // `[permissions].codebase`, or the signed-manifest declaration when one
    // verifies). Those files are caller-controlled data and the worker
    // protocol supplies no manifest, so every module's authority is lowered
    // to deny-all too. A grant surviving this lowering would mean the pinning
    // was skipped or the pinned program mutated afterward — `verify` only
    // reads permissions and cannot repopulate them.
    for module in &mut program.modules {
        module.capabilities = ModuleCapabilities::none();
        module.permissions = ModulePermissions::from_capabilities(ModuleCapabilities::none());
    }
    VerifiedProgram::verify(program)
}

fn run_payload(
    stdout: Vec<u8>,
    memory: TynraMemory,
    heap_before_shutdown: TynraHeapStats,
    heap_after_shutdown: TynraHeapStats,
) -> Result<JsonValue> {
    let stdout =
        String::from_utf8(stdout).map_err(|error| TynraError::runtime(format!("stdout was not UTF-8: {error}")))?;
    Ok(json!({
        "stdout": stdout,
        "memory": memory_to_json(&memory),
        "heap_before_shutdown": heap_stats_to_json(heap_before_shutdown),
        "heap_after_shutdown": heap_stats_to_json(heap_after_shutdown),
    }))
}

fn memory_to_json(memory: &TynraMemory) -> Vec<JsonValue> {
    memory.snapshot().iter().map(value_to_json).collect()
}

fn value_to_json(value: &RuntimeValue) -> JsonValue {
    match value {
        RuntimeValue::I8(v) => json!({"type": "i8",  "value": v}),
        RuntimeValue::I16(v) => json!({"type": "i16", "value": v}),
        RuntimeValue::I32(v) => json!({"type": "i32", "value": v}),
        RuntimeValue::I64(v) => json!({"type": "i64", "value": v}),
        RuntimeValue::U8(v) => json!({"type": "u8",  "value": v}),
        RuntimeValue::U16(v) => json!({"type": "u16", "value": v}),
        RuntimeValue::U32(v) => json!({"type": "u32", "value": v}),
        RuntimeValue::U64(v) => json!({"type": "u64", "value": v}),
        RuntimeValue::Float { kind, bits } => json!({"type": kind.name(), "value": bits}),
        RuntimeValue::Bool(b) => json!({"type": "bool", "value": b}),
        RuntimeValue::Unit => json!({"type": "unit"}),
        RuntimeValue::Null => json!({"type": "null"}),
        RuntimeValue::Function(id) => json!({"type": "function", "id": id}),
        RuntimeValue::Reference(p) => json!({"type": "reference", "address": p.address}),
        RuntimeValue::Phantom => json!({"type": "phantom"}),
        RuntimeValue::Zst(k) => json!({"type": "zst", "marker": k.name()}),
        RuntimeValue::Unsafe => json!({"type": "unsafe"}),
        RuntimeValue::Heap(r) => {
            json!({
                "type": "heap",
                "address": r.address,
                "generation": r.generation,
            })
        }
        RuntimeValue::Pointer(p) => {
            json!({
                "type": "pointer",
                "address": p.address,
                "kind": p.kind.as_str(),
                "index": p.index,
                "field": p.field,
                "generation": p.generation,
                "cast": p.cast.as_str(),
            })
        }
    }
}

fn heap_stats_to_json(stats: TynraHeapStats) -> JsonValue {
    json!({
        "live_objects": stats.live_objects,
        "live_bytes": stats.live_bytes,
        "peak_objects": stats.peak_objects,
        "peak_bytes": stats.peak_bytes,
        "total_allocations": stats.total_allocations,
        "total_frees": stats.total_frees,
        "shutdown_frees": stats.shutdown_frees,
    })
}

#[cfg(test)]
mod protocol_tests {
    use super::*;

    /// A host build older than the protocol field must keep working: a
    /// request without `protocol_version` deserializes with v1 semantics.
    #[test]
    fn legacy_request_without_protocol_version_still_parses() {
        let request = json!({"run_file": {"path": "m.to", "mode": "vm", "inputs": []}});
        let parsed: SandboxRequest = serde_json::from_value(request).expect("legacy request must parse");
        match parsed {
            SandboxRequest::RunFile { protocol_version, path, mode, inputs } => {
                assert_eq!(protocol_version, 0);
                assert_eq!(path, "m.to");
                assert_eq!(mode, "vm");
                assert!(inputs.is_empty());
            }
            _ => panic!("legacy request must deserialize to its own variant"),
        }
    }

    /// Documented contract: any future grant field must be refused by the
    /// worker, never silently ignored. `deny_unknown_fields` is what makes
    /// that contract hold for every field serde does not declare.
    #[test]
    fn unknown_request_field_is_refused() {
        let request = json!({"run_file": {"path": "m.to", "mode": "vm", "inputs": [], "policy": {"filesystem": "all"}}});
        let error = serde_json::from_value::<SandboxRequest>(request)
            .expect_err("an unknown request field must be refused");
        assert!(error.to_string().contains("unknown field"), "{error}");
    }

    #[test]
    fn protocol_version_above_the_worker_is_refused() {
        ensure_supported_protocol(0).expect("legacy default is supported");
        ensure_supported_protocol(SANDBOX_PROTOCOL_VERSION).expect("current version is supported");
        let error = ensure_supported_protocol(SANDBOX_PROTOCOL_VERSION + 1)
            .expect_err("a future protocol version must not be silently downgraded");
        assert!(error.to_string().contains("protocol version"), "{error}");
    }
}
