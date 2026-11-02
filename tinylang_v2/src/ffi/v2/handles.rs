use std::alloc::{Layout, alloc_zeroed, dealloc};
use std::sync::Mutex;

mod execution;
mod lifetime;
pub(super) use execution::with_vm;
use lifetime::SlotLifetime;

use tinylang::VerifiedProgram;

use super::session::VmSession;
use super::status::{
    TINY_STATUS_BUSY, TINY_STATUS_INTERNAL, TINY_STATUS_INVALID_ARGUMENT,
    TINY_STATUS_INVALID_HANDLE, TINY_STATUS_OK, TINY_STATUS_OUT_OF_MEMORY, TinyStatus,
};
use super::types::{TinyOwnedBuffer, TinyRuntimeHandle};
use super::wire::MAX_FRAME_BYTES;

const MAX_RUNTIMES: usize = 64;
const MAX_OWNED_BUFFERS: usize = 64;
const MAX_OBJECTS: usize = 256;
const BUFFER_ALIGNMENT: usize = 8;
// Both tables fit below this slot-word bit, preserving all 32 generation bits.
const OBJECT_HANDLE_TAG: u32 = 1 << 31;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
#[allow(dead_code)] // Kinds become live as their VM dispatchers are implemented.
pub(crate) enum ObjectKind {
    Program = 1,
    Vm = 2,
    Heap = 3,
}

#[derive(Clone, Copy)]
struct RuntimeSlot {
    lifetime: SlotLifetime,
}

impl RuntimeSlot {
    const EMPTY: Self = Self {
        lifetime: SlotLifetime::EMPTY,
    };
}

#[derive(Clone, Copy)]
struct BufferSlot {
    address: usize,
    capacity: usize,
    runtime: TinyRuntimeHandle,
    token: u64,
    active: bool,
}

impl BufferSlot {
    const EMPTY: Self = Self {
        address: 0,
        capacity: 0,
        runtime: 0,
        token: 0,
        active: false,
    };
}

struct ObjectSlot {
    runtime: TinyRuntimeHandle,
    lifetime: SlotLifetime,
    kind: ObjectKind,
    payload: Option<ObjectPayload>,
}

// The registry has a fixed 256-slot bound. Inline VM ownership deliberately
// spends static table space rather than adding an infallible Box allocation
// to the status-returning session creation path.
#[allow(clippy::large_enum_variant)]
enum ObjectPayload {
    Program(VerifiedProgram),
    Vm(VmSession),
    VmRunning,
    VmFailed,
}

impl ObjectSlot {
    const EMPTY: Self = Self {
        runtime: 0,
        lifetime: SlotLifetime::EMPTY,
        kind: ObjectKind::Program,
        payload: None,
    };
}

struct Registry {
    runtimes: [RuntimeSlot; MAX_RUNTIMES],
    buffers: [BufferSlot; MAX_OWNED_BUFFERS],
    objects: [ObjectSlot; MAX_OBJECTS],
}

impl Registry {
    const fn new() -> Self {
        Self {
            runtimes: [RuntimeSlot::EMPTY; MAX_RUNTIMES],
            buffers: [BufferSlot::EMPTY; MAX_OWNED_BUFFERS],
            objects: [const { ObjectSlot::EMPTY }; MAX_OBJECTS],
        }
    }

    fn validate_runtime(&self, handle: TinyRuntimeHandle) -> Result<usize, TinyStatus> {
        let (index, generation) = decode_handle(handle)?;
        let slot = &self.runtimes[index];
        slot.lifetime.validate(generation)?;
        Ok(index)
    }
}

static REGISTRY: Mutex<Registry> = Mutex::new(Registry::new());

fn encode_handle(index: usize, generation: u32) -> TinyRuntimeHandle {
    (u64::from(generation) << 32) | (index as u64 + 1)
}

fn encode_object_handle(index: usize, generation: u32) -> u64 {
    encode_handle(index, generation) | u64::from(OBJECT_HANDLE_TAG)
}

fn decode_handle(handle: TinyRuntimeHandle) -> Result<(usize, u32), TinyStatus> {
    let slot_number = (handle & u64::from(u32::MAX)) as u32;
    let generation = (handle >> 32) as u32;
    if slot_number & OBJECT_HANDLE_TAG != 0 || slot_number == 0 || generation == 0 {
        return Err(TINY_STATUS_INVALID_HANDLE);
    }
    let index = (slot_number - 1) as usize;
    if index >= MAX_RUNTIMES {
        return Err(TINY_STATUS_INVALID_HANDLE);
    }
    Ok((index, generation))
}

pub fn create_runtime() -> Result<TinyRuntimeHandle, TinyStatus> {
    let mut registry = REGISTRY.lock().map_err(|_| TINY_STATUS_INTERNAL)?;
    for (index, slot) in registry.runtimes.iter_mut().enumerate() {
        if let Some(generation) = slot.lifetime.activate() {
            return Ok(encode_handle(index, generation));
        }
    }
    Err(TINY_STATUS_OUT_OF_MEMORY)
}

pub fn validate_runtime(handle: TinyRuntimeHandle) -> TinyStatus {
    let Ok(registry) = REGISTRY.lock() else {
        return TINY_STATUS_INTERNAL;
    };
    match registry.validate_runtime(handle) {
        Ok(_) => TINY_STATUS_OK,
        Err(status) => status,
    }
}

pub fn destroy_runtime(handle: TinyRuntimeHandle) -> TinyStatus {
    let Ok(mut registry) = REGISTRY.lock() else {
        return TINY_STATUS_INTERNAL;
    };
    let index = match registry.validate_runtime(handle) {
        Ok(index) => index,
        Err(status) => return status,
    };
    if registry
        .buffers
        .iter()
        .any(|slot| slot.active && slot.runtime == handle)
        || registry
            .objects
            .iter()
            .any(|slot| slot.lifetime.is_live() && slot.runtime == handle)
    {
        return TINY_STATUS_BUSY;
    }
    let slot = &mut registry.runtimes[index];
    slot.lifetime.release();
    TINY_STATUS_OK
}

#[cfg(test)]
pub(crate) fn validate_object(
    runtime: TinyRuntimeHandle,
    handle: u64,
    expected_kind: Option<ObjectKind>,
) -> TinyStatus {
    let Ok(registry) = REGISTRY.lock() else {
        return TINY_STATUS_INTERNAL;
    };
    if let Err(status) = registry.validate_runtime(runtime) {
        return status;
    }
    match find_object(&registry, runtime, handle, expected_kind) {
        Ok(_) => TINY_STATUS_OK,
        Err(status) => status,
    }
}

pub fn release_object(runtime: TinyRuntimeHandle, handle: u64) -> TinyStatus {
    let Ok(mut registry) = REGISTRY.lock() else {
        return TINY_STATUS_INTERNAL;
    };
    if let Err(status) = registry.validate_runtime(runtime) {
        return status;
    }
    let index = match find_object(&registry, runtime, handle, None) {
        Ok(index) => index,
        Err(status) => return status,
    };
    let slot = &mut registry.objects[index];
    if matches!(slot.payload, Some(ObjectPayload::VmRunning)) {
        return TINY_STATUS_BUSY;
    }
    let payload = slot.payload.take();
    slot.lifetime.release();
    slot.runtime = 0;
    // Destruction can reclaim a VM heap; do not serialize unrelated registry
    // operations behind that work.
    drop(registry);
    drop(payload);
    TINY_STATUS_OK
}

pub fn insert_program(
    runtime: TinyRuntimeHandle,
    program: VerifiedProgram,
) -> Result<u64, TinyStatus> {
    let mut registry = REGISTRY.lock().map_err(|_| TINY_STATUS_INTERNAL)?;
    registry.validate_runtime(runtime)?;
    let (index, slot) = registry
        .objects
        .iter_mut()
        .enumerate()
        .find(|(_, slot)| slot.lifetime.is_available())
        .ok_or(TINY_STATUS_OUT_OF_MEMORY)?;
    let generation = slot.lifetime.activate().ok_or(TINY_STATUS_INTERNAL)?;
    slot.runtime = runtime;
    slot.kind = ObjectKind::Program;
    slot.payload = Some(ObjectPayload::Program(program));
    Ok(encode_object_handle(index, generation))
}

pub fn clone_program(
    runtime: TinyRuntimeHandle,
    handle: u64,
) -> Result<VerifiedProgram, TinyStatus> {
    let registry = REGISTRY.lock().map_err(|_| TINY_STATUS_INTERNAL)?;
    registry.validate_runtime(runtime)?;
    let index = find_object(&registry, runtime, handle, Some(ObjectKind::Program))?;
    match registry.objects[index].payload.as_ref() {
        Some(ObjectPayload::Program(program)) => Ok(program.clone()),
        _ => Err(TINY_STATUS_INTERNAL),
    }
}

pub fn insert_vm(runtime: TinyRuntimeHandle, session: VmSession) -> Result<u64, TinyStatus> {
    let mut registry = REGISTRY.lock().map_err(|_| TINY_STATUS_INTERNAL)?;
    registry.validate_runtime(runtime)?;
    let (index, slot) = registry
        .objects
        .iter_mut()
        .enumerate()
        .find(|(_, slot)| slot.lifetime.is_available())
        .ok_or(TINY_STATUS_OUT_OF_MEMORY)?;
    let generation = slot.lifetime.activate().ok_or(TINY_STATUS_INTERNAL)?;
    slot.runtime = runtime;
    slot.kind = ObjectKind::Vm;
    slot.payload = Some(ObjectPayload::Vm(session));
    Ok(encode_object_handle(index, generation))
}

fn find_object(
    registry: &Registry,
    runtime: TinyRuntimeHandle,
    handle: u64,
    expected_kind: Option<ObjectKind>,
) -> Result<usize, TinyStatus> {
    let (index, generation) = decode_object_handle(handle)?;
    let slot = &registry.objects[index];
    slot.lifetime.validate(generation)?;
    if slot.runtime != runtime {
        return Err(TINY_STATUS_INVALID_HANDLE);
    }
    if expected_kind.is_some_and(|expected| slot.kind != expected) {
        return Err(TINY_STATUS_INVALID_HANDLE);
    }
    Ok(index)
}

fn decode_object_handle(handle: u64) -> Result<(usize, u32), TinyStatus> {
    let slot_component = (handle & u64::from(u32::MAX)) as u32;
    if slot_component & OBJECT_HANDLE_TAG == 0 {
        return Err(TINY_STATUS_INVALID_HANDLE);
    }
    let slot_number = slot_component & !OBJECT_HANDLE_TAG;
    let generation = (handle >> 32) as u32;
    if slot_number == 0 || generation == 0 {
        return Err(TINY_STATUS_INVALID_HANDLE);
    }
    let index = (slot_number - 1) as usize;
    if index >= MAX_OBJECTS {
        return Err(TINY_STATUS_INVALID_HANDLE);
    }
    Ok((index, generation))
}

#[cfg(test)]
pub(crate) fn insert_object_for_test(
    runtime: TinyRuntimeHandle,
    kind: ObjectKind,
) -> Result<u64, TinyStatus> {
    let mut registry = REGISTRY.lock().map_err(|_| TINY_STATUS_INTERNAL)?;
    registry.validate_runtime(runtime)?;
    let (index, slot) = registry
        .objects
        .iter_mut()
        .enumerate()
        .find(|(_, slot)| slot.lifetime.is_available())
        .ok_or(TINY_STATUS_OUT_OF_MEMORY)?;
    let generation = slot.lifetime.activate().ok_or(TINY_STATUS_INTERNAL)?;
    slot.runtime = runtime;
    slot.kind = kind;
    slot.payload = None;
    Ok(encode_object_handle(index, generation))
}

pub fn acquire_buffer(
    runtime: TinyRuntimeHandle,
    minimum_capacity: u64,
) -> Result<TinyOwnedBuffer, TinyStatus> {
    let capacity = usize::try_from(minimum_capacity).map_err(|_| TINY_STATUS_INVALID_ARGUMENT)?;
    if capacity == 0 || capacity > MAX_FRAME_BYTES {
        return Err(TINY_STATUS_INVALID_ARGUMENT);
    }
    let mut registry = REGISTRY.lock().map_err(|_| TINY_STATUS_INTERNAL)?;
    registry.validate_runtime(runtime)?;
    let slot = registry
        .buffers
        .iter_mut()
        .find(|slot| !slot.active)
        .ok_or(TINY_STATUS_OUT_OF_MEMORY)?;
    let layout = Layout::from_size_align(capacity, BUFFER_ALIGNMENT)
        .map_err(|_| TINY_STATUS_INVALID_ARGUMENT)?;
    // SAFETY: `layout` has a non-zero size and valid power-of-two alignment.
    let data = unsafe { alloc_zeroed(layout) };
    if data.is_null() {
        return Err(TINY_STATUS_OUT_OF_MEMORY);
    }
    slot.token = slot.token.wrapping_add(1).max(1);
    slot.address = data as usize;
    slot.capacity = capacity;
    slot.runtime = runtime;
    slot.active = true;
    Ok(TinyOwnedBuffer {
        data,
        length: 0,
        capacity: minimum_capacity,
        owner_token: slot.token,
    })
}

pub fn release_buffer(runtime: TinyRuntimeHandle, buffer: TinyOwnedBuffer) -> TinyStatus {
    if buffer.data.is_null()
        || buffer.capacity == 0
        || buffer.length > buffer.capacity
        || buffer.owner_token == 0
    {
        return TINY_STATUS_INVALID_ARGUMENT;
    }
    let Ok(capacity) = usize::try_from(buffer.capacity) else {
        return TINY_STATUS_INVALID_ARGUMENT;
    };
    let Ok(mut registry) = REGISTRY.lock() else {
        return TINY_STATUS_INTERNAL;
    };
    if let Err(status) = registry.validate_runtime(runtime) {
        return status;
    }
    let Some(slot) = registry.buffers.iter_mut().find(|slot| {
        slot.active
            && slot.runtime == runtime
            && slot.token == buffer.owner_token
            && slot.address == buffer.data as usize
            && slot.capacity == capacity
    }) else {
        return TINY_STATUS_INVALID_HANDLE;
    };
    let Ok(layout) = Layout::from_size_align(slot.capacity, BUFFER_ALIGNMENT) else {
        return TINY_STATUS_INTERNAL;
    };
    // SAFETY: the pointer and layout exactly match the successful allocation
    // recorded in this registry slot, which is removed immediately afterward.
    unsafe { dealloc(slot.address as *mut u8, layout) };
    slot.address = 0;
    slot.capacity = 0;
    slot.runtime = 0;
    slot.active = false;
    TINY_STATUS_OK
}

#[cfg(test)]
mod exhaustion_tests {
    use super::*;
    use crate::TINY_STATUS_STALE_HANDLE;

    #[test]
    fn runtime_and_object_tables_skip_exhausted_slots() {
        let runtime = create_runtime().unwrap();
        let object = insert_object_for_test(runtime, ObjectKind::Program).unwrap();
        let (object_index, _) = decode_object_handle(object).unwrap();
        {
            let mut registry = REGISTRY.lock().unwrap();
            registry.objects[object_index].lifetime = SlotLifetime::Live(u32::MAX);
        }
        let last_object = encode_object_handle(object_index, u32::MAX);
        assert_eq!(release_object(runtime, last_object), TINY_STATUS_OK);
        let next_object = insert_object_for_test(runtime, ObjectKind::Program).unwrap();
        assert_ne!(decode_object_handle(next_object).unwrap().0, object_index);
        assert_eq!(release_object(runtime, object), TINY_STATUS_STALE_HANDLE);
        assert_eq!(
            release_object(runtime, last_object),
            TINY_STATUS_STALE_HANDLE
        );
        assert_eq!(release_object(runtime, next_object), TINY_STATUS_OK);

        let (runtime_index, _) = decode_handle(runtime).unwrap();
        {
            let mut registry = REGISTRY.lock().unwrap();
            registry.runtimes[runtime_index].lifetime = SlotLifetime::Live(u32::MAX);
        }
        let last_runtime = encode_handle(runtime_index, u32::MAX);
        assert_eq!(destroy_runtime(last_runtime), TINY_STATUS_OK);
        let next_runtime = create_runtime().unwrap();
        assert_ne!(decode_handle(next_runtime).unwrap().0, runtime_index);
        assert_eq!(validate_runtime(runtime), TINY_STATUS_STALE_HANDLE);
        assert_eq!(validate_runtime(last_runtime), TINY_STATUS_STALE_HANDLE);
        assert_eq!(destroy_runtime(next_runtime), TINY_STATUS_OK);
    }
}
