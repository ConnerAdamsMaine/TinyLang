use super::status::{TINY_STATUS_INVALID_FRAME, TinyStatus};
use super::types::TinyHandle;
use super::wire::{
    OPCODE_HANDLE_RELEASE,
    OPCODE_PROGRAM_LOAD,
    OPCODE_VM_CREATE,
    OPCODE_VM_INVOKE,
    ValueCursor,
    ValueRef,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArtifactFormat {
    Tlbc,
    CompilerBinary,
}
pub const MAX_ARTIFACT_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_RESULT_BYTES: u32 = 16 * 1024 * 1024;
pub const VM_FLAG_ALLOW_JIT: u32 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProgramLoad<'a> {
    pub artifact_format: ArtifactFormat,
    pub artifact:        &'a [u8],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VmCreate {
    pub program:          TinyHandle,
    pub max_heap_bytes:   u64,
    pub max_heap_objects: u32,
    pub max_call_depth:   u32,
    pub vm_flags:         u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VmInvoke<'a> {
    pub vm:               TinyHandle,
    pub export_id:        u32,
    pub fuel:             u64,
    pub max_result_bytes: u32,
    pub argument_count:   u64,
    pub arguments:        &'a [u8],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HandleRelease {
    pub handle: TinyHandle,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VmRequest<'a> {
    ProgramLoad(ProgramLoad<'a>),
    VmCreate(VmCreate),
    VmInvoke(VmInvoke<'a>),
    HandleRelease(HandleRelease),
}

pub fn decode_vm_request<'a>(opcode: u16, payload: &'a [u8]) -> Result<Option<VmRequest<'a>>, TinyStatus> {
    match opcode {
        OPCODE_PROGRAM_LOAD => decode_program_load(payload).map(|value| Some(VmRequest::ProgramLoad(value))),
        OPCODE_VM_CREATE => decode_vm_create(payload).map(|value| Some(VmRequest::VmCreate(value))),
        OPCODE_VM_INVOKE => decode_vm_invoke(payload).map(|value| Some(VmRequest::VmInvoke(value))),
        OPCODE_HANDLE_RELEASE => decode_handle_release(payload).map(|value| Some(VmRequest::HandleRelease(value))),
        _ => Ok(None),
    }
}

fn decode_program_load(payload: &[u8]) -> Result<ProgramLoad<'_>, TinyStatus> {
    let mut cursor = ValueCursor::new(payload);
    let artifact_format = match required(&mut cursor)?.as_u32()? {
        1 => ArtifactFormat::Tlbc,
        2 => ArtifactFormat::CompilerBinary,
        _ => return Err(TINY_STATUS_INVALID_FRAME),
    };
    let artifact = required(&mut cursor)?.as_bytes()?;
    cursor.finish()?;
    if artifact.is_empty() || artifact.len() > MAX_ARTIFACT_BYTES {
        return Err(TINY_STATUS_INVALID_FRAME);
    }
    Ok(ProgramLoad {
        artifact_format,
        artifact,
    })
}

fn decode_vm_create(payload: &[u8]) -> Result<VmCreate, TinyStatus> {
    let mut cursor = ValueCursor::new(payload);
    let program = required(&mut cursor)?.as_handle()?;
    let max_heap_bytes = required(&mut cursor)?.as_u64()?;
    let max_heap_objects = required(&mut cursor)?.as_u32()?;
    let max_call_depth = required(&mut cursor)?.as_u32()?;
    let vm_flags = required(&mut cursor)?.as_u32()?;
    cursor.finish()?;
    if max_heap_bytes == 0 || max_heap_objects == 0 || max_call_depth == 0 || vm_flags & !VM_FLAG_ALLOW_JIT != 0 {
        return Err(TINY_STATUS_INVALID_FRAME);
    }
    Ok(VmCreate {
        program,
        max_heap_bytes,
        max_heap_objects,
        max_call_depth,
        vm_flags,
    })
}

fn decode_vm_invoke(payload: &[u8]) -> Result<VmInvoke<'_>, TinyStatus> {
    let mut cursor = ValueCursor::new(payload);
    let vm = required(&mut cursor)?.as_handle()?;
    let export_id = required(&mut cursor)?.as_u32()?;
    let fuel = required(&mut cursor)?.as_u64()?;
    let max_result_bytes = required(&mut cursor)?.as_u32()?;
    let (argument_count, arguments) = required(&mut cursor)?.as_sequence()?;
    cursor.finish()?;
    if fuel == 0 || max_result_bytes > MAX_RESULT_BYTES {
        return Err(TINY_STATUS_INVALID_FRAME);
    }
    Ok(VmInvoke {
        vm,
        export_id,
        fuel,
        max_result_bytes,
        argument_count,
        arguments,
    })
}

fn decode_handle_release(payload: &[u8]) -> Result<HandleRelease, TinyStatus> {
    let mut cursor = ValueCursor::new(payload);
    let handle = required(&mut cursor)?.as_handle()?;
    cursor.finish()?;
    Ok(HandleRelease { handle })
}

fn required<'a>(cursor: &mut ValueCursor<'a>) -> Result<ValueRef<'a>, TinyStatus> {
    cursor.next_value()?.ok_or(TINY_STATUS_INVALID_FRAME)
}
