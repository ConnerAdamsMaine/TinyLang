//! TinyLang v2 binary ABI: framed requests, handles, sessions, and wire values.
#![deny(unsafe_op_in_unsafe_fn)]

mod artifact;
mod handles;
mod schema;
mod session;
mod status;
mod types;
mod values;
mod wire;

use core::ptr;
use std::panic::{AssertUnwindSafe, catch_unwind};

pub use artifact::{ArtifactError, decode as decode_binary_artifact};
pub use status::*;
pub use types::*;
pub use wire::{
    ABI_VERSION, FLAG_RESPONSE, HEADER_BYTES, MAGIC, MAX_FRAME_BYTES, OPCODE_ABI_INFO, OPCODE_ECHO,
    OPCODE_HANDLE_RELEASE, OPCODE_NOP, OPCODE_PROGRAM_LOAD, OPCODE_VM_CREATE, OPCODE_VM_INVOKE, SCHEMA_MAJOR,
    SCHEMA_MINOR, VALUE_HEADER_BYTES, VALUE_UTF8, ValueCursor, ValueRef,
};

const ABI_INFO_PAYLOAD_BYTES: usize = 24;
const STATUS_DETAIL_BYTES: usize = 16;

#[derive(Clone, Copy)]
enum Operation<'a> {
    Nop,
    AbiInfo,
    Echo(&'a [u8]),
    ProgramLoad(schema::ProgramLoad<'a>),
    VmCreate(schema::VmCreate),
    VmInvoke(schema::VmInvoke<'a>),
    Unimplemented,
    HandleRelease(TinyHandle),
}

#[unsafe(no_mangle)]
pub extern "C" fn tinylang_v2_abi_version() -> u32 {
    ABI_VERSION
}

/// Creates a process-local runtime handle.
///
/// # Safety
/// `out_runtime` must be valid for one `u64` write and must not alias memory
/// concurrently accessed by another thread.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tinylang_v2_runtime_create(out_runtime: *mut TinyRuntimeHandle) -> TinyStatus {
    ffi_guard(|| {
        if out_runtime.is_null() {
            return TINY_STATUS_INVALID_ARGUMENT;
        }
        let runtime = match handles::create_runtime() {
            Ok(runtime) => runtime,
            Err(status) => return status,
        };
        // SAFETY: required by this function's C ABI contract; unaligned access
        // avoids imposing a stronger alignment rule than the byte ABI needs.
        unsafe { ptr::write_unaligned(out_runtime, runtime) };
        TINY_STATUS_OK
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn tinylang_v2_runtime_destroy(runtime: TinyRuntimeHandle) -> TinyStatus {
    ffi_guard(|| handles::destroy_runtime(runtime))
}

/// Acquires an optional runtime-owned transfer buffer.
///
/// The allocation is zero-initialized and must be returned using
/// `tinylang_v2_buffer_release` before destroying its runtime.
///
/// # Safety
/// `out_buffer` must be valid for one `TinyOwnedBuffer` write.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tinylang_v2_buffer_acquire(
    runtime: TinyRuntimeHandle,
    minimum_capacity: u64,
    out_buffer: *mut TinyOwnedBuffer,
) -> TinyStatus {
    ffi_guard(|| {
        if out_buffer.is_null() {
            return TINY_STATUS_INVALID_ARGUMENT;
        }
        let buffer = match handles::acquire_buffer(runtime, minimum_capacity) {
            Ok(buffer) => buffer,
            Err(status) => return status,
        };
        // SAFETY: required by the function contract.
        unsafe { ptr::write_unaligned(out_buffer, buffer) };
        TINY_STATUS_OK
    })
}

/// Releases a buffer acquired from this runtime.
///
/// On success the caller's descriptor is cleared. Copies of the old
/// descriptor remain invalid and must not be used.
///
/// # Safety
/// `buffer` must be valid for one read and write of `TinyOwnedBuffer`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tinylang_v2_buffer_release(
    runtime: TinyRuntimeHandle,
    buffer: *mut TinyOwnedBuffer,
) -> TinyStatus {
    ffi_guard(|| {
        if buffer.is_null() {
            return TINY_STATUS_INVALID_ARGUMENT;
        }
        // SAFETY: required by the function contract.
        let descriptor = unsafe { ptr::read_unaligned(buffer) };
        let status = handles::release_buffer(runtime, descriptor);
        if status == TINY_STATUS_OK {
            // SAFETY: required by the function contract.
            unsafe { ptr::write_unaligned(buffer, TinyOwnedBuffer::default()) };
        }
        status
    })
}

/// Validates and dispatches exactly one binary request frame.
///
/// No input pointer is retained. Envelope parsing and response serialization
/// allocate no transfer buffers; an opcode may allocate runtime-owned state
/// after capacity preflight. On success, `out_written` receives the actual
/// response size.
/// On `TINY_STATUS_BUFFER_TOO_SMALL`, it receives the minimum safe capacity
/// for dispatch; side-effecting operations may report a conservative bound
/// larger than their eventual success frame. No response bytes are written.
///
/// # Safety
/// All non-empty buffers must identify live memory for the duration of the
/// call. The request, response, and `out_written` ranges must not overlap.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tinylang_v2_call(
    runtime: TinyRuntimeHandle,
    request: TinyBuffer,
    response: TinyMutableBuffer,
    out_written: *mut u64,
) -> TinyStatus {
    ffi_guard(|| {
        if out_written.is_null() {
            return TINY_STATUS_INVALID_ARGUMENT;
        }
        // SAFETY: required by the function contract.
        unsafe { ptr::write_unaligned(out_written, 0) };

        let request_length = match checked_length(request.length) {
            Ok(length) => length,
            Err(status) => return status,
        };
        let response_capacity = match checked_length(response.capacity) {
            Ok(length) => length,
            Err(status) => return status,
        };
        if (request_length != 0 && request.data.is_null()) || (response_capacity != 0 && response.data.is_null()) {
            return TINY_STATUS_INVALID_ARGUMENT;
        }
        if ranges_overlap(request.data as usize, request_length, response.data as usize, response_capacity)
            || ranges_overlap(request.data as usize, request_length, out_written as usize, size_of::<u64>())
            || ranges_overlap(response.data as usize, response_capacity, out_written as usize, size_of::<u64>())
        {
            return TINY_STATUS_INVALID_ARGUMENT;
        }
        let runtime_status = handles::validate_runtime(runtime);
        if runtime_status != TINY_STATUS_OK {
            return runtime_status;
        }

        // SAFETY: pointer validity is required by the C contract; null is
        // handled separately for the zero-length case above.
        let request_bytes = if request_length == 0 {
            &[]
        } else {
            unsafe { core::slice::from_raw_parts(request.data, request_length) }
        };
        let frame = match wire::parse_request(request_bytes) {
            Ok(frame) => frame,
            Err(status) => return status,
        };

        let operation = match frame.opcode {
            wire::OPCODE_NOP if frame.payload.is_empty() => Operation::Nop,
            wire::OPCODE_ABI_INFO if frame.payload.is_empty() => Operation::AbiInfo,
            wire::OPCODE_ECHO => {
                if let Err(status) = wire::validate_value_sequence(frame.payload) {
                    return status;
                }
                Operation::Echo(frame.payload)
            }
            wire::OPCODE_NOP | wire::OPCODE_ABI_INFO => return TINY_STATUS_INVALID_FRAME,
            _ => match schema::decode_vm_request(frame.opcode, frame.payload) {
                Ok(Some(schema::VmRequest::HandleRelease(request))) => Operation::HandleRelease(request.handle),
                Ok(Some(schema::VmRequest::ProgramLoad(request))) => Operation::ProgramLoad(request),
                Ok(Some(schema::VmRequest::VmCreate(request))) => Operation::VmCreate(request),
                Ok(Some(schema::VmRequest::VmInvoke(request))) => Operation::VmInvoke(request),
                Ok(None) => Operation::Unimplemented,
                Err(status) => return status,
            },
        };
        let preflight_payload_length = match operation {
            Operation::Nop => 0,
            Operation::AbiInfo => ABI_INFO_PAYLOAD_BYTES,
            Operation::Echo(payload) => payload.len(),
            Operation::ProgramLoad(_)
            | Operation::VmCreate(_)
            | Operation::Unimplemented
            | Operation::HandleRelease(_) => STATUS_DETAIL_BYTES,
            Operation::VmInvoke(request) => request.max_result_bytes as usize,
        };
        let preflight_required = match wire::response_length(preflight_payload_length) {
            Ok(required) => required,
            Err(status) => return status,
        };
        // SAFETY: required by the function contract and checked for overlap.
        unsafe { ptr::write_unaligned(out_written, preflight_required as u64) };
        if response_capacity < preflight_required {
            return TINY_STATUS_BUFFER_TOO_SMALL;
        }

        let mut response_handle = None;
        let mut invoke_result = None;
        let (response_status, payload_length) = match operation {
            Operation::ProgramLoad(request) => match artifact::decode_request(request) {
                Ok(program) => match handles::insert_program(runtime, program) {
                    Ok(handle) => {
                        response_handle = Some(handle);
                        (TINY_STATUS_OK, VALUE_HEADER_BYTES)
                    }
                    Err(status) => (status, STATUS_DETAIL_BYTES),
                },
                Err(_) => (TINY_STATUS_INVALID_ARTIFACT, STATUS_DETAIL_BYTES),
            },
            Operation::VmCreate(request) => match handles::clone_program(runtime, request.program) {
                Ok(program) => match session::VmSession::new(program, request) {
                    Ok(session) => match handles::insert_vm(runtime, session) {
                        Ok(handle) => {
                            response_handle = Some(handle);
                            (TINY_STATUS_OK, VALUE_HEADER_BYTES)
                        }
                        Err(status) => (status, STATUS_DETAIL_BYTES),
                    },
                    Err(status) => (status, STATUS_DETAIL_BYTES),
                },
                Err(status) => (status, STATUS_DETAIL_BYTES),
            },
            Operation::VmInvoke(request) => {
                let result =
                    values::decode_arguments(request.arguments, request.argument_count).and_then(|arguments| {
                        handles::with_vm(runtime, request.vm, |session| {
                            let Some(export) = session.exported_function(request.export_id) else {
                                return Err(TINY_STATUS_INVALID_ARGUMENT);
                            };
                            if arguments.len() != export.parameter_count {
                                return Err(TINY_STATUS_INVALID_ARGUMENT);
                            }
                            let mut stdout = std::io::sink();
                            let value = session
                                .execution
                                .invoke_function_with_fuel(export.function_index, arguments, &mut stdout, request.fuel)
                                .map_err(|_| TINY_STATUS_VM_TRAP)?;
                            values::encode_result(&value)
                        })
                    });
                match result {
                    Ok(result) if result.len() <= request.max_result_bytes as usize => {
                        let length = result.len();
                        invoke_result = Some(result);
                        (TINY_STATUS_OK, length)
                    }
                    Ok(_) => (
                        TINY_STATUS_VM_TRAP,
                        usize::from(request.max_result_bytes as usize >= STATUS_DETAIL_BYTES) * STATUS_DETAIL_BYTES,
                    ),
                    Err(status) => (
                        status,
                        usize::from(request.max_result_bytes as usize >= STATUS_DETAIL_BYTES) * STATUS_DETAIL_BYTES,
                    ),
                }
            }
            Operation::HandleRelease(handle) => {
                let status = handles::release_object(runtime, handle);
                (status, usize::from(status != TINY_STATUS_OK) * STATUS_DETAIL_BYTES)
            }
            Operation::Unimplemented => (TINY_STATUS_UNKNOWN_OPCODE, STATUS_DETAIL_BYTES),
            Operation::Nop | Operation::AbiInfo | Operation::Echo(_) => (TINY_STATUS_OK, preflight_payload_length),
        };
        let required = match wire::response_length(payload_length) {
            Ok(required) => required,
            Err(status) => return status,
        };
        // A side-effecting operation may use less than its conservative
        // preflight bound, but it may never require more after dispatch.
        debug_assert!(required <= preflight_required);
        // SAFETY: required by the function contract and checked for overlap.
        unsafe { ptr::write_unaligned(out_written, required as u64) };

        // SAFETY: pointer validity is required by the C contract; the required
        // prefix is within the checked capacity and does not overlap input.
        let output = unsafe { core::slice::from_raw_parts_mut(response.data, required) };
        wire::write_response_header(output, frame.opcode, response_status, payload_length, frame.correlation_id);
        let payload = &mut output[wire::HEADER_BYTES..required];
        match operation {
            Operation::Nop | Operation::HandleRelease(_) if payload.is_empty() => {}
            Operation::AbiInfo => wire::write_abi_info_value(payload),
            Operation::Echo(input) => payload.copy_from_slice(input),
            Operation::ProgramLoad(_) | Operation::VmCreate(_) if response_status == TINY_STATUS_OK => {
                wire::write_handle_value(payload, response_handle.unwrap_or_default());
            }
            Operation::VmInvoke(_) if response_status == TINY_STATUS_OK => {
                let Some(result) = invoke_result else {
                    return TINY_STATUS_INTERNAL;
                };
                payload.copy_from_slice(&result);
            }
            Operation::ProgramLoad(_)
            | Operation::VmCreate(_)
            | Operation::Unimplemented
            | Operation::HandleRelease(_) => {
                wire::write_status_detail(payload, response_status);
            }
            Operation::VmInvoke(_) if !payload.is_empty() => {
                wire::write_status_detail(payload, response_status);
            }
            Operation::VmInvoke(_) => {}
            Operation::Nop => unreachable!("NOP responses are empty"),
        }
        TINY_STATUS_OK
    })
}

fn checked_length(length: u64) -> Result<usize, TinyStatus> {
    let length = usize::try_from(length).map_err(|_| TINY_STATUS_INVALID_ARGUMENT)?;
    if length > wire::MAX_FRAME_BYTES {
        Err(TINY_STATUS_INVALID_ARGUMENT)
    } else {
        Ok(length)
    }
}

fn ranges_overlap(a: usize, a_length: usize, b: usize, b_length: usize) -> bool {
    if a_length == 0 || b_length == 0 {
        return false;
    }
    let Some(a_end) = a.checked_add(a_length) else {
        return true;
    };
    let Some(b_end) = b.checked_add(b_length) else {
        return true;
    };
    a < b_end && b < a_end
}

fn ffi_guard(operation: impl FnOnce() -> TinyStatus) -> TinyStatus {
    catch_unwind(AssertUnwindSafe(operation)).unwrap_or(TINY_STATUS_PANIC)
}

const fn size_of<T>() -> usize {
    core::mem::size_of::<T>()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interoperability::ffi::v2::handles::ObjectKind;

    #[test]
    fn release_capacity_failure_preserves_a_live_object() {
        let runtime = handles::create_runtime().unwrap();
        let object = handles::insert_object_for_test(runtime, ObjectKind::Program).unwrap();
        let request = release_request(object);
        let mut short_output = [0xa5u8; 32];
        let mut written = 0;

        // SAFETY: all descriptors use live, disjoint local storage.
        let status = unsafe {
            tinylang_v2_call(
                runtime,
                TinyBuffer {
                    data: request.as_ptr(),
                    length: request.len() as u64,
                },
                TinyMutableBuffer {
                    data: short_output.as_mut_ptr(),
                    capacity: short_output.len() as u64,
                },
                &mut written,
            )
        };
        assert_eq!(status, TINY_STATUS_BUFFER_TOO_SMALL);
        assert_eq!(written, 48);
        assert_eq!(handles::validate_object(runtime, object, Some(ObjectKind::Program)), TINY_STATUS_OK);

        let mut output = [0u8; 48];
        // SAFETY: all descriptors use live, disjoint local storage.
        let status = unsafe {
            tinylang_v2_call(
                runtime,
                TinyBuffer {
                    data: request.as_ptr(),
                    length: request.len() as u64,
                },
                TinyMutableBuffer {
                    data: output.as_mut_ptr(),
                    capacity: output.len() as u64,
                },
                &mut written,
            )
        };
        assert_eq!(status, TINY_STATUS_OK);
        assert_eq!(written, 32);
        assert_eq!(handles::validate_object(runtime, object, Some(ObjectKind::Program)), TINY_STATUS_STALE_HANDLE);
        assert_eq!(handles::destroy_runtime(runtime), TINY_STATUS_OK);
    }

    #[test]
    fn object_handles_are_runtime_and_kind_scoped() {
        let first = handles::create_runtime().unwrap();
        let second = handles::create_runtime().unwrap();
        let object = handles::insert_object_for_test(first, ObjectKind::Heap).unwrap();
        assert_eq!(handles::validate_object(first, object, Some(ObjectKind::Heap)), TINY_STATUS_OK);
        assert_eq!(handles::validate_object(first, object, Some(ObjectKind::Vm)), TINY_STATUS_INVALID_HANDLE);
        assert_eq!(handles::validate_object(second, object, None), TINY_STATUS_INVALID_HANDLE);
        assert_eq!(handles::destroy_runtime(first), TINY_STATUS_BUSY);
        assert_eq!(handles::release_object(first, object), TINY_STATUS_OK);
        assert_eq!(handles::destroy_runtime(first), TINY_STATUS_OK);
        assert_eq!(handles::destroy_runtime(second), TINY_STATUS_OK);
    }

    fn release_request(handle: TinyHandle) -> Vec<u8> {
        let mut bytes = vec![0u8; HEADER_BYTES + VALUE_HEADER_BYTES];
        bytes[0..4].copy_from_slice(&MAGIC);
        bytes[4..6].copy_from_slice(&SCHEMA_MAJOR.to_le_bytes());
        bytes[6..8].copy_from_slice(&SCHEMA_MINOR.to_le_bytes());
        bytes[8..10].copy_from_slice(&(HEADER_BYTES as u16).to_le_bytes());
        bytes[10..12].copy_from_slice(&OPCODE_HANDLE_RELEASE.to_le_bytes());
        bytes[20..24].copy_from_slice(&(VALUE_HEADER_BYTES as u32).to_le_bytes());
        bytes[HEADER_BYTES..HEADER_BYTES + 2].copy_from_slice(&14u16.to_le_bytes());
        bytes[HEADER_BYTES + 8..HEADER_BYTES + 16].copy_from_slice(&handle.to_le_bytes());
        bytes
    }
}
