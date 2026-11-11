use super::VM;
use crate::execution::runtime::heap::{heap_str, iter_array_values};
use crate::{HeapData, Result, RuntimeValue, TinyLangError};

/// Owned host data for the interpreter's string, buffer and array adapters.
/// This is a Rust API, not a stable ABI or native heap-layout description.
/// Array elements containing heap references must originate from the same VM.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum VmHeapValue {
    String(String),
    Bytes(Vec<u8>),
    Array(Vec<RuntimeValue>),
}

impl VM {
    /// Copy host data into this VM's existing heap under its execution limits.
    /// Nested array heap references are shallow, VM-local references; the
    /// caller must not pass values originating from another VM. Foreign
    /// interfaces must enforce this provenance through their handle tables.
    ///
    /// # Errors
    /// Returns allocation, limit or invalid nested-reference errors.
    pub fn import_heap_value(&mut self, value: VmHeapValue) -> Result<RuntimeValue> {
        let mut heap = self.context.heap();
        let reference = match value {
            VmHeapValue::String(text) => heap.alloc_string(text)?,
            VmHeapValue::Bytes(bytes) => heap.alloc_buffer_with(bytes)?,
            VmHeapValue::Array(values) => {
                for value in &values {
                    if matches!(value, RuntimeValue::Heap(_)) {
                        heap.get(value)?;
                    }
                }
                heap.alloc_array(values)?
            }
        };
        Ok(RuntimeValue::Heap(reference))
    }

    /// Copy a supported heap value into host-owned storage. No borrowed heap
    /// bytes or locks escape. Array snapshots are shallow: heap elements
    /// remain valid only while their objects remain live in this same VM.
    /// The input must originate from this VM.
    ///
    /// # Errors
    /// Returns wrong-type, stale-reference or host-allocation errors.
    pub fn copy_heap_value(&self, value: &RuntimeValue) -> Result<VmHeapValue> {
        let heap = self.context.heap();
        match &heap.get(value)?.data {
            HeapData::String(bytes) => {
                let text = heap_str(bytes)?;
                let mut output = String::new();
                output
                    .try_reserve_exact(text.len())
                    .map_err(|_| TinyLangError::runtime("Failed to allocate host string"))?;
                output.push_str(text);
                Ok(VmHeapValue::String(output))
            }
            HeapData::Buffer(bytes) => {
                let mut output = Vec::new();
                output
                    .try_reserve_exact(bytes.as_slice().len())
                    .map_err(|_| TinyLangError::runtime("Failed to allocate host buffer"))?;
                output.extend_from_slice(bytes.as_slice());
                Ok(VmHeapValue::Bytes(output))
            }
            HeapData::Array(values) => {
                let mut output = Vec::new();
                output
                    .try_reserve_exact(values.len())
                    .map_err(|_| TinyLangError::runtime("Failed to allocate host array"))?;
                output.extend(iter_array_values(values));
                Ok(VmHeapValue::Array(output))
            }
            _ => Err(TinyLangError::runtime("Heap value is not a string, buffer or array")),
        }
    }

    /// Explicitly free one object in this VM. Does not recursively free array
    /// elements. All aliases to this object become stale, including VM globals.
    /// The input must originate from this VM. Foreign handle release must not
    /// call this while the language program may still own the object.
    ///
    /// # Errors
    /// Returns wrong-type, stale-reference or heap-accounting errors.
    pub fn release_heap_value(&mut self, value: &RuntimeValue) -> Result<()> {
        self.context.heap().free(value)
    }
}
