use super::status::{
    TINY_STATUS_INVALID_ARTIFACT, TINY_STATUS_INVALID_FRAME, TINY_STATUS_UNSUPPORTED_VERSION, TINY_STATUS_VM_TRAP,
    TinyStatus,
};

pub const MAGIC: [u8; 4] = *b"TLV2";
pub const SCHEMA_MAJOR: u16 = 2;
pub const SCHEMA_MINOR: u16 = 0;
pub const HEADER_BYTES: usize = 32;
pub const VALUE_HEADER_BYTES: usize = 16;
pub const MAX_FRAME_BYTES: usize = 64 * 1024 * 1024;
pub const FLAG_RESPONSE: u32 = 1;
pub const ABI_VERSION: u32 = ((SCHEMA_MAJOR as u32) << 16) | SCHEMA_MINOR as u32;

pub const OPCODE_NOP: u16 = 0x0000;
pub const OPCODE_ABI_INFO: u16 = 0x0001;
pub const OPCODE_ECHO: u16 = 0x0002;
pub const OPCODE_PROGRAM_LOAD: u16 = 0x1000;
pub const OPCODE_VM_CREATE: u16 = 0x1001;
pub const OPCODE_VM_INVOKE: u16 = 0x1002;
pub const OPCODE_HANDLE_RELEASE: u16 = 0x1003;

pub const VALUE_NULL: u16 = 0;
pub const VALUE_BOOL: u16 = 1;
pub const VALUE_U8: u16 = 2;
pub const VALUE_U16: u16 = 3;
pub const VALUE_U32: u16 = 4;
pub const VALUE_U64: u16 = 5;
pub const VALUE_I8: u16 = 6;
pub const VALUE_I16: u16 = 7;
pub const VALUE_I32: u16 = 8;
pub const VALUE_I64: u16 = 9;
pub const VALUE_F32: u16 = 10;
pub const VALUE_F64: u16 = 11;
pub const VALUE_BYTES: u16 = 12;
pub const VALUE_UTF8: u16 = 13;
pub const VALUE_HANDLE: u16 = 14;
pub const VALUE_ENUM: u16 = 15;
pub const VALUE_SLICE: u16 = 16;
pub const VALUE_SEQUENCE: u16 = 17;

const MAX_NESTING: usize = 32;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Frame<'a> {
    pub opcode: u16,
    pub correlation_id: u64,
    pub payload: &'a [u8],
}

/// A validated, zero-copy view of one canonical value record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ValueRef<'a> {
    tag: u16,
    auxiliary: u64,
    payload: &'a [u8],
}

impl<'a> ValueRef<'a> {
    pub fn as_u32(self) -> Result<u32, TinyStatus> {
        if self.tag == VALUE_U32 {
            Ok(read_u32(self.payload, 0))
        } else {
            Err(TINY_STATUS_INVALID_FRAME)
        }
    }

    pub fn as_u64(self) -> Result<u64, TinyStatus> {
        if self.tag == VALUE_U64 {
            Ok(read_u64(self.payload, 0))
        } else {
            Err(TINY_STATUS_INVALID_FRAME)
        }
    }

    pub fn as_bytes(self) -> Result<&'a [u8], TinyStatus> {
        if self.tag == VALUE_BYTES {
            Ok(self.payload)
        } else {
            Err(TINY_STATUS_INVALID_FRAME)
        }
    }

    pub fn as_utf8(self) -> Result<&'a str, TinyStatus> {
        if self.tag == VALUE_UTF8 {
            core::str::from_utf8(self.payload).map_err(|_| TINY_STATUS_INVALID_FRAME)
        } else {
            Err(TINY_STATUS_INVALID_FRAME)
        }
    }

    pub fn as_handle(self) -> Result<u64, TinyStatus> {
        if self.tag == VALUE_HANDLE {
            Ok(self.auxiliary)
        } else {
            Err(TINY_STATUS_INVALID_FRAME)
        }
    }

    pub fn as_sequence(self) -> Result<(u64, &'a [u8]), TinyStatus> {
        if self.tag == VALUE_SEQUENCE {
            Ok((self.auxiliary, self.payload))
        } else {
            Err(TINY_STATUS_INVALID_FRAME)
        }
    }

    pub(crate) fn tag(self) -> u16 {
        self.tag
    }

    pub(crate) fn payload(self) -> &'a [u8] {
        self.payload
    }
}

/// Iterates canonical top-level value records without allocating.
pub struct ValueCursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> ValueCursor<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    pub fn next_value(&mut self) -> Result<Option<ValueRef<'a>>, TinyStatus> {
        if self.offset == self.bytes.len() {
            return Ok(None);
        }
        let start = self.offset;
        let record_length = validate_value(self.bytes, start, 0)?;
        let payload_start = start + VALUE_HEADER_BYTES;
        let payload_length = read_u32(self.bytes, start + 4) as usize;
        let payload_end = payload_start + payload_length;
        self.offset = start + record_length;
        Ok(Some(ValueRef {
            tag: read_u16(self.bytes, start),
            auxiliary: read_u64(self.bytes, start + 8),
            payload: &self.bytes[payload_start..payload_end],
        }))
    }

    pub fn finish(mut self) -> Result<(), TinyStatus> {
        if self.next_value()?.is_none() {
            Ok(())
        } else {
            Err(TINY_STATUS_INVALID_FRAME)
        }
    }
}

pub fn parse_request(bytes: &[u8]) -> Result<Frame<'_>, TinyStatus> {
    if bytes.len() < HEADER_BYTES || bytes.len() > MAX_FRAME_BYTES {
        return Err(TINY_STATUS_INVALID_FRAME);
    }
    if bytes[0..4] != MAGIC {
        return Err(TINY_STATUS_INVALID_FRAME);
    }
    let major = read_u16(bytes, 4);
    let minor = read_u16(bytes, 6);
    if major != SCHEMA_MAJOR || minor > SCHEMA_MINOR {
        return Err(TINY_STATUS_UNSUPPORTED_VERSION);
    }
    if usize::from(read_u16(bytes, 8)) != HEADER_BYTES {
        return Err(TINY_STATUS_INVALID_FRAME);
    }
    let opcode = read_u16(bytes, 10);
    let flags = read_u32(bytes, 12);
    let status = read_u32(bytes, 16);
    if flags != 0 || status != 0 {
        return Err(TINY_STATUS_INVALID_FRAME);
    }
    let payload_length = read_u32(bytes, 20) as usize;
    let total = HEADER_BYTES
        .checked_add(payload_length)
        .ok_or(TINY_STATUS_INVALID_FRAME)?;
    if total != bytes.len() {
        return Err(TINY_STATUS_INVALID_FRAME);
    }
    Ok(Frame {
        opcode,
        correlation_id: read_u64(bytes, 24),
        payload: &bytes[HEADER_BYTES..],
    })
}

pub fn response_length(payload_length: usize) -> Result<usize, TinyStatus> {
    let total = HEADER_BYTES
        .checked_add(payload_length)
        .ok_or(TINY_STATUS_INVALID_FRAME)?;
    if total > MAX_FRAME_BYTES || payload_length > u32::MAX as usize {
        return Err(TINY_STATUS_INVALID_FRAME);
    }
    Ok(total)
}

pub fn write_response_header(
    output: &mut [u8],
    opcode: u16,
    status: TinyStatus,
    payload_length: usize,
    correlation_id: u64,
) {
    output[..HEADER_BYTES].fill(0);
    output[0..4].copy_from_slice(&MAGIC);
    put_u16(output, 4, SCHEMA_MAJOR);
    put_u16(output, 6, SCHEMA_MINOR);
    put_u16(output, 8, HEADER_BYTES as u16);
    put_u16(output, 10, opcode);
    put_u32(output, 12, FLAG_RESPONSE);
    put_u32(output, 16, status);
    put_u32(output, 20, payload_length as u32);
    put_u64(output, 24, correlation_id);
}

pub fn write_abi_info_value(output: &mut [u8]) {
    output.fill(0);
    put_u16(output, 0, VALUE_U32);
    put_u32(output, 4, 4);
    put_u32(output, VALUE_HEADER_BYTES, ABI_VERSION);
}

pub fn write_handle_value(output: &mut [u8], handle: u64) {
    output.fill(0);
    put_u16(output, 0, VALUE_HANDLE);
    put_u64(output, 8, handle);
}

pub fn write_status_detail(output: &mut [u8], status: TinyStatus) {
    output.fill(0);
    let domain = match status {
        TINY_STATUS_INVALID_ARTIFACT | TINY_STATUS_VM_TRAP => 2,
        _ => 1,
    };
    put_u16(output, 0, domain);
    put_u32(output, 4, status);
    put_u64(output, 8, u64::MAX); // No byte-specific fault offset.
}

pub fn validate_value_sequence(bytes: &[u8]) -> Result<(), TinyStatus> {
    let mut cursor = ValueCursor::new(bytes);
    while cursor.next_value()?.is_some() {}
    Ok(())
}

fn validate_value(bytes: &[u8], start: usize, depth: usize) -> Result<usize, TinyStatus> {
    if depth >= MAX_NESTING || bytes.len().saturating_sub(start) < VALUE_HEADER_BYTES {
        return Err(TINY_STATUS_INVALID_FRAME);
    }
    let tag = read_u16(bytes, start);
    let flags = read_u16(bytes, start + 2);
    let value_length = read_u32(bytes, start + 4) as usize;
    let auxiliary = read_u64(bytes, start + 8);
    if flags != 0 {
        return Err(TINY_STATUS_INVALID_FRAME);
    }
    let payload_start = start.checked_add(VALUE_HEADER_BYTES).ok_or(TINY_STATUS_INVALID_FRAME)?;
    let payload_end = payload_start
        .checked_add(value_length)
        .ok_or(TINY_STATUS_INVALID_FRAME)?;
    let padded_end = align_to_8(payload_end).ok_or(TINY_STATUS_INVALID_FRAME)?;
    if padded_end > bytes.len() || bytes[payload_end..padded_end].iter().any(|byte| *byte != 0) {
        return Err(TINY_STATUS_INVALID_FRAME);
    }
    let payload = &bytes[payload_start..payload_end];
    match tag {
        VALUE_NULL => require_scalar(payload, auxiliary, 0)?,
        VALUE_BOOL => {
            require_scalar(payload, auxiliary, 1)?;
            if payload[0] > 1 {
                return Err(TINY_STATUS_INVALID_FRAME);
            }
        }
        VALUE_U8 | VALUE_I8 => require_scalar(payload, auxiliary, 1)?,
        VALUE_U16 | VALUE_I16 => require_scalar(payload, auxiliary, 2)?,
        VALUE_U32 | VALUE_I32 | VALUE_F32 => require_scalar(payload, auxiliary, 4)?,
        VALUE_U64 | VALUE_I64 | VALUE_F64 => require_scalar(payload, auxiliary, 8)?,
        VALUE_BYTES => require_auxiliary_zero(auxiliary)?,
        VALUE_UTF8 => {
            require_auxiliary_zero(auxiliary)?;
            core::str::from_utf8(payload).map_err(|_| TINY_STATUS_INVALID_FRAME)?;
        }
        VALUE_HANDLE => {
            if !payload.is_empty() || auxiliary == 0 {
                return Err(TINY_STATUS_INVALID_FRAME);
            }
        }
        VALUE_ENUM => validate_enum(payload, depth + 1)?,
        VALUE_SLICE => validate_slice(payload, auxiliary)?,
        VALUE_SEQUENCE => validate_nested_sequence(payload, auxiliary, depth + 1)?,
        _ => return Err(TINY_STATUS_INVALID_FRAME),
    }
    padded_end.checked_sub(start).ok_or(TINY_STATUS_INVALID_FRAME)
}

fn require_scalar(payload: &[u8], auxiliary: u64, width: usize) -> Result<(), TinyStatus> {
    if payload.len() == width && auxiliary == 0 {
        Ok(())
    } else {
        Err(TINY_STATUS_INVALID_FRAME)
    }
}

fn require_auxiliary_zero(auxiliary: u64) -> Result<(), TinyStatus> {
    if auxiliary == 0 {
        Ok(())
    } else {
        Err(TINY_STATUS_INVALID_FRAME)
    }
}

fn validate_enum(payload: &[u8], depth: usize) -> Result<(), TinyStatus> {
    if payload.len() < 8 || payload[4..8].iter().any(|byte| *byte != 0) {
        return Err(TINY_STATUS_INVALID_FRAME);
    }
    let nested = &payload[8..];
    if nested.is_empty() {
        return Ok(());
    }
    let consumed = validate_value(nested, 0, depth)?;
    if consumed == nested.len() {
        Ok(())
    } else {
        Err(TINY_STATUS_INVALID_FRAME)
    }
}

fn validate_nested_sequence(payload: &[u8], count: u64, depth: usize) -> Result<(), TinyStatus> {
    let count = usize::try_from(count).map_err(|_| TINY_STATUS_INVALID_FRAME)?;
    if count > payload.len() / VALUE_HEADER_BYTES {
        return Err(TINY_STATUS_INVALID_FRAME);
    }
    let mut offset = 0usize;
    for _ in 0..count {
        offset = offset
            .checked_add(validate_value(payload, offset, depth)?)
            .ok_or(TINY_STATUS_INVALID_FRAME)?;
    }
    if offset == payload.len() {
        Ok(())
    } else {
        Err(TINY_STATUS_INVALID_FRAME)
    }
}

fn validate_slice(payload: &[u8], auxiliary: u64) -> Result<(), TinyStatus> {
    if payload.len() < 8 || auxiliary > u64::from(u16::MAX) {
        return Err(TINY_STATUS_INVALID_FRAME);
    }
    let element_tag = auxiliary as u16;
    let width = match element_tag {
        VALUE_BOOL | VALUE_U8 | VALUE_I8 => 1usize,
        VALUE_U16 | VALUE_I16 => 2,
        VALUE_U32 | VALUE_I32 | VALUE_F32 => 4,
        VALUE_U64 | VALUE_I64 | VALUE_F64 | VALUE_HANDLE => 8,
        _ => return Err(TINY_STATUS_INVALID_FRAME),
    };
    let count = usize::try_from(read_u64(payload, 0)).map_err(|_| TINY_STATUS_INVALID_FRAME)?;
    let data_length = count.checked_mul(width).ok_or(TINY_STATUS_INVALID_FRAME)?;
    if payload.len() != 8usize.saturating_add(data_length) {
        return Err(TINY_STATUS_INVALID_FRAME);
    }
    if element_tag == VALUE_BOOL && payload[8..].iter().any(|byte| *byte > 1) {
        return Err(TINY_STATUS_INVALID_FRAME);
    }
    Ok(())
}

fn align_to_8(value: usize) -> Option<usize> {
    value.checked_add(7).map(|value| value & !7)
}

fn read_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([bytes[offset], bytes[offset + 1], bytes[offset + 2], bytes[offset + 3]])
}

fn read_u64(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
        bytes[offset + 4],
        bytes[offset + 5],
        bytes[offset + 6],
        bytes[offset + 7],
    ])
}

fn put_u16(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn put_u64(bytes: &mut [u8], offset: usize, value: u64) {
    bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}
