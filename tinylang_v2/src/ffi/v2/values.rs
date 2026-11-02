use tinylang::{RuntimeValue, TypeKind};

use super::status::{
    TINY_STATUS_INTERNAL, TINY_STATUS_INVALID_ARGUMENT, TINY_STATUS_INVALID_FRAME,
    TINY_STATUS_OUT_OF_MEMORY, TinyStatus,
};
use super::wire::{
    VALUE_BOOL, VALUE_F32, VALUE_F64, VALUE_HEADER_BYTES, VALUE_I8, VALUE_I16, VALUE_I32,
    VALUE_I64, VALUE_NULL, VALUE_U8, VALUE_U16, VALUE_U32, VALUE_U64, ValueCursor, ValueRef,
};

pub(super) fn decode_arguments(
    bytes: &[u8],
    argument_count: u64,
) -> Result<Vec<RuntimeValue>, TinyStatus> {
    let expected_count = usize::try_from(argument_count).map_err(|_| TINY_STATUS_INVALID_FRAME)?;
    let mut values = Vec::new();
    values
        .try_reserve_exact(expected_count)
        .map_err(|_| TINY_STATUS_OUT_OF_MEMORY)?;
    let mut cursor = ValueCursor::new(bytes);
    while let Some(value) = cursor.next_value()? {
        values.push(decode_scalar(value)?);
    }
    if values.len() != expected_count {
        return Err(TINY_STATUS_INVALID_FRAME);
    }
    Ok(values)
}

pub(super) fn encode_result(value: &RuntimeValue) -> Result<Vec<u8>, TinyStatus> {
    let (tag, payload) = match value {
        RuntimeValue::Null => (VALUE_NULL, [0; 8]),
        RuntimeValue::Bool(value) => (VALUE_BOOL, [u8::from(*value), 0, 0, 0, 0, 0, 0, 0]),
        RuntimeValue::I8(value) => (VALUE_I8, widen(value.to_le_bytes())),
        RuntimeValue::I16(value) => (VALUE_I16, widen(value.to_le_bytes())),
        RuntimeValue::I32(value) => (VALUE_I32, widen(value.to_le_bytes())),
        RuntimeValue::I64(value) => (VALUE_I64, value.to_le_bytes()),
        RuntimeValue::U8(value) => (VALUE_U8, widen(value.to_le_bytes())),
        RuntimeValue::U16(value) => (VALUE_U16, widen(value.to_le_bytes())),
        RuntimeValue::U32(value) => (VALUE_U32, widen(value.to_le_bytes())),
        RuntimeValue::U64(value) => (VALUE_U64, value.to_le_bytes()),
        RuntimeValue::Float {
            kind: TypeKind::Fp32,
            bits,
        } => (VALUE_F32, widen((*bits as f32).to_bits().to_le_bytes())),
        RuntimeValue::Float {
            kind: TypeKind::Fp64,
            bits,
        } => (VALUE_F64, bits.to_bits().to_le_bytes()),
        _ => return Err(TINY_STATUS_INVALID_ARGUMENT),
    };
    let payload_length = match tag {
        VALUE_NULL => 0,
        VALUE_BOOL | VALUE_I8 | VALUE_U8 => 1,
        VALUE_I16 | VALUE_U16 => 2,
        VALUE_I32 | VALUE_U32 | VALUE_F32 => 4,
        VALUE_I64 | VALUE_U64 | VALUE_F64 => 8,
        _ => return Err(TINY_STATUS_INTERNAL),
    };
    let record_length =
        align_to_8(VALUE_HEADER_BYTES + payload_length).ok_or(TINY_STATUS_INVALID_FRAME)?;
    let mut output = Vec::new();
    output
        .try_reserve_exact(record_length)
        .map_err(|_| TINY_STATUS_OUT_OF_MEMORY)?;
    output.resize(record_length, 0);
    output[0..2].copy_from_slice(&tag.to_le_bytes());
    output[4..8].copy_from_slice(&(payload_length as u32).to_le_bytes());
    output[VALUE_HEADER_BYTES..VALUE_HEADER_BYTES + payload_length]
        .copy_from_slice(&payload[..payload_length]);
    Ok(output)
}

fn decode_scalar(value: ValueRef<'_>) -> Result<RuntimeValue, TinyStatus> {
    let payload = value.payload();
    Ok(match value.tag() {
        VALUE_NULL => RuntimeValue::Null,
        VALUE_BOOL => RuntimeValue::Bool(payload[0] != 0),
        VALUE_I8 => RuntimeValue::I8(i8::from_le_bytes(read_array(payload)?)),
        VALUE_I16 => RuntimeValue::I16(i16::from_le_bytes(read_array(payload)?)),
        VALUE_I32 => RuntimeValue::I32(i32::from_le_bytes(read_array(payload)?)),
        VALUE_I64 => RuntimeValue::I64(i64::from_le_bytes(read_array(payload)?)),
        VALUE_U8 => RuntimeValue::U8(u8::from_le_bytes(read_array(payload)?)),
        VALUE_U16 => RuntimeValue::U16(u16::from_le_bytes(read_array(payload)?)),
        VALUE_U32 => RuntimeValue::U32(u32::from_le_bytes(read_array(payload)?)),
        VALUE_U64 => RuntimeValue::U64(u64::from_le_bytes(read_array(payload)?)),
        VALUE_F32 => RuntimeValue::Float {
            kind: TypeKind::Fp32,
            bits: f64::from(f32::from_bits(u32::from_le_bytes(read_array(payload)?))),
        },
        VALUE_F64 => RuntimeValue::Float {
            kind: TypeKind::Fp64,
            bits: f64::from_bits(u64::from_le_bytes(read_array(payload)?)),
        },
        _ => return Err(TINY_STATUS_INVALID_ARGUMENT),
    })
}

fn read_array<const N: usize>(bytes: &[u8]) -> Result<[u8; N], TinyStatus> {
    bytes.try_into().map_err(|_| TINY_STATUS_INVALID_FRAME)
}

fn widen<const N: usize>(bytes: [u8; N]) -> [u8; 8] {
    let mut widened = [0; 8];
    widened[..N].copy_from_slice(&bytes);
    widened
}

fn align_to_8(value: usize) -> Option<usize> {
    value.checked_add(7).map(|value| value & !7)
}
