//! Length-prefixed binary adapter for hosts without a built-in C ABI loader.
//!
//! Request packet: u32 request bytes, u32 response capacity, request bytes.
//! Response packet: u32 call status, u64 written/required, u32 response bytes,
//! followed by the response bytes. Every integer is little-endian.

use std::io::{self, Read, Write};

use tinylang_v2::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut runtime = 0;
    // SAFETY: `runtime` is valid output storage.
    let create_status = unsafe { tinylang_v2_runtime_create(&mut runtime) };
    if create_status != TINY_STATUS_OK {
        return Err(format!("runtime creation failed with status {create_status}").into());
    }

    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut input = stdin.lock();
    let mut output = stdout.lock();
    while let Some((request_length, response_capacity)) = read_packet_header(&mut input)? {
        if request_length > MAX_FRAME_BYTES || response_capacity > MAX_FRAME_BYTES {
            return Err("bridge packet exceeds the ABI frame limit".into());
        }
        let mut request = vec![0u8; request_length];
        input.read_exact(&mut request)?;
        let mut response = vec![0u8; response_capacity];
        let mut written = 0u64;
        // SAFETY: all descriptors refer to live disjoint vectors and scalar
        // storage for the duration of this call.
        let call_status = unsafe {
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
        let response_length = if call_status == TINY_STATUS_OK {
            usize::try_from(written)
                .ok()
                .filter(|length| *length <= response.len())
                .unwrap_or(0)
        } else {
            0
        };
        let mut header = [0u8; 16];
        header[0..4].copy_from_slice(&call_status.to_le_bytes());
        header[4..12].copy_from_slice(&written.to_le_bytes());
        header[12..16].copy_from_slice(&(response_length as u32).to_le_bytes());
        output.write_all(&header)?;
        output.write_all(&response[..response_length])?;
        output.flush()?;
    }

    let destroy_status = tinylang_v2_runtime_destroy(runtime);
    if destroy_status != TINY_STATUS_OK {
        return Err(format!("runtime destruction failed with status {destroy_status}").into());
    }
    Ok(())
}

fn read_packet_header(input: &mut impl Read) -> io::Result<Option<(usize, usize)>> {
    let mut header = [0u8; 8];
    let count = input.read(&mut header[..1])?;
    if count == 0 {
        return Ok(None);
    }
    input.read_exact(&mut header[1..])?;
    let request_length = u32::from_le_bytes(header[0..4].try_into().expect("fixed field"));
    let response_capacity = u32::from_le_bytes(header[4..8].try_into().expect("fixed field"));
    Ok(Some((request_length as usize, response_capacity as usize)))
}
