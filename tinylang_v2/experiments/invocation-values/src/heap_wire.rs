use crate::invocation::Failure;
use tinylang::{RuntimeValue, VmHeapValue};

// Fixed test signature: string, bytes, array<i64>. Both codecs reject other
// shapes. The general ABI still needs type metadata and recursive conversion.
pub fn encode(values: &[VmHeapValue]) -> Result<Vec<u8>, Failure> {
    let [
        VmHeapValue::String(text),
        VmHeapValue::Bytes(bytes),
        VmHeapValue::Array(array),
    ] = values
    else {
        return Err(Failure::Arguments);
    };
    let mut integers = Vec::new();
    for value in array {
        let RuntimeValue::I64(value) = value else {
            return Err(Failure::Arguments);
        };
        #[cfg(feature = "a")]
        record(&mut integers, 9, 0, &value.to_le_bytes());
        #[cfg(feature = "b")]
        integers.extend_from_slice(&value.to_le_bytes());
    }
    let mut output = Vec::new();
    #[cfg(feature = "a")]
    {
        record(&mut output, 13, 0, text.as_bytes());
        record(&mut output, 12, 0, bytes);
        record(&mut output, 17, array.len() as u64, &integers);
    }
    #[cfg(feature = "b")]
    for payload in [text.as_bytes(), bytes, &integers] {
        output.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        output.extend_from_slice(payload);
    }
    Ok(output)
}

#[cfg(feature = "a")]
fn record(output: &mut Vec<u8>, tag: u16, count: u64, bytes: &[u8]) {
    output.extend_from_slice(&tag.to_le_bytes());
    output.extend_from_slice(&0u16.to_le_bytes());
    output.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    output.extend_from_slice(&count.to_le_bytes());
    output.extend_from_slice(bytes);
    output.resize(output.len().next_multiple_of(8), 0);
}

fn take<'a>(input: &mut &'a [u8], length: usize) -> Result<&'a [u8], Failure> {
    let (value, rest) = input.split_at_checked(length).ok_or(Failure::Arguments)?;
    *input = rest;
    Ok(value)
}

#[cfg(feature = "a")]
fn read_record<'a>(input: &mut &'a [u8], expected: u16) -> Result<(u64, &'a [u8]), Failure> {
    let header = take(input, 16)?;
    let tag = u16::from_le_bytes(header[..2].try_into().unwrap());
    let length = u32::from_le_bytes(header[4..8].try_into().unwrap()) as usize;
    let count = u64::from_le_bytes(header[8..16].try_into().unwrap());
    if tag != expected || header[2..4] != [0, 0] {
        return Err(Failure::Arguments);
    }
    let payload = take(input, length)?;
    if take(input, (8 - length % 8) % 8)?
        .iter()
        .any(|byte| *byte != 0)
    {
        return Err(Failure::Arguments);
    }
    Ok((count, payload))
}

#[cfg(feature = "b")]
fn read_packed<'a>(input: &mut &'a [u8]) -> Result<&'a [u8], Failure> {
    let length = u32::from_le_bytes(take(input, 4)?.try_into().unwrap()) as usize;
    take(input, length)
}

pub fn decode(mut input: &[u8]) -> Result<Vec<VmHeapValue>, Failure> {
    if input.len() > 1024 * 1024 {
        return Err(Failure::Arguments);
    }
    #[cfg(feature = "a")]
    let (text, bytes, mut integers, count) = {
        let (text_count, text) = read_record(&mut input, 13)?;
        let (bytes_count, bytes) = read_record(&mut input, 12)?;
        let (count, integers) = read_record(&mut input, 17)?;
        if text_count != 0 || bytes_count != 0 {
            return Err(Failure::Arguments);
        }
        (text, bytes, integers, count)
    };
    #[cfg(feature = "b")]
    let (text, bytes, integers) = (
        read_packed(&mut input)?,
        read_packed(&mut input)?,
        read_packed(&mut input)?,
    );
    if !input.is_empty() {
        return Err(Failure::Arguments);
    }
    let text = std::str::from_utf8(text)
        .map_err(|_| Failure::Arguments)?
        .to_owned();
    let mut array = Vec::new();
    #[cfg(feature = "a")]
    {
        while !integers.is_empty() {
            let (auxiliary, value) = read_record(&mut integers, 9)?;
            if auxiliary != 0 || value.len() != 8 {
                return Err(Failure::Arguments);
            }
            array.push(RuntimeValue::I64(i64::from_le_bytes(
                value.try_into().unwrap(),
            )));
        }
        if array.len() as u64 != count {
            return Err(Failure::Arguments);
        }
    }
    #[cfg(feature = "b")]
    {
        let (values, remainder) = integers.as_chunks::<8>();
        if !remainder.is_empty() {
            return Err(Failure::Arguments);
        }
        array.extend(
            values
                .iter()
                .map(|bytes| RuntimeValue::I64(i64::from_le_bytes(*bytes))),
        );
    }
    Ok(vec![
        VmHeapValue::String(text),
        VmHeapValue::Bytes(bytes.to_vec()),
        VmHeapValue::Array(array),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_every_truncation_and_trailing_bytes() {
        let values = crate::heap_invocation::fixture();
        let bytes = encode(&values).unwrap();
        assert_eq!(decode(&bytes).unwrap(), values);
        for end in 0..bytes.len() {
            assert!(decode(&bytes[..end]).is_err());
        }
        let mut trailing = bytes;
        trailing.push(0);
        assert!(decode(&trailing).is_err());
    }
    #[test]
    fn rejects_invalid_utf8_and_unbounded_lengths() {
        let mut bytes = encode(&crate::heap_invocation::fixture()).unwrap();
        let header = if cfg!(feature = "a") { 16 } else { 4 };
        bytes[header] = 0xff;
        assert!(decode(&bytes).is_err());
        bytes[header - 4..header].fill(0xff);
        assert!(decode(&bytes).is_err());
    }
}
