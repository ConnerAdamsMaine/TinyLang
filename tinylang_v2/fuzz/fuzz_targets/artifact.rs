#![no_main]

use libfuzzer_sys::fuzz_target;
use tinylang_v2::{
    HEADER_BYTES, OPCODE_HANDLE_RELEASE, OPCODE_PROGRAM_LOAD, TINY_STATUS_OK, TINY_STATUS_PANIC,
    TinyBuffer, TinyMutableBuffer, tinylang_v2_call, tinylang_v2_runtime_create,
    tinylang_v2_runtime_destroy,
};

fuzz_target!(|artifact: &[u8]| {
    let Some(request) = program_load_request(artifact) else {
        return;
    };
    let mut runtime = 0;
    // SAFETY: `runtime` is valid output storage.
    if unsafe { tinylang_v2_runtime_create(&mut runtime) } != TINY_STATUS_OK {
        return;
    }
    let mut output = [0u8; 48];
    let mut written = 0;
    // SAFETY: all descriptors reference live, disjoint storage.
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
    assert_ne!(status, TINY_STATUS_PANIC);
    if status == TINY_STATUS_OK && written == 48 && read_u32(&output, 16) == TINY_STATUS_OK {
        release(runtime, read_u64(&output, HEADER_BYTES + 8));
    }
    let _ = tinylang_v2_runtime_destroy(runtime);
});

fn program_load_request(artifact: &[u8]) -> Option<Vec<u8>> {
    let bytes_record_unpadded = 16usize.checked_add(artifact.len())?;
    let bytes_record_length = bytes_record_unpadded.checked_add(7)? & !7;
    let payload_length = 24usize.checked_add(bytes_record_length)?;
    let total_length = HEADER_BYTES.checked_add(payload_length)?;
    let mut request = vec![0u8; total_length];
    request[0..4].copy_from_slice(b"TLV2");
    request[4..6].copy_from_slice(&2u16.to_le_bytes());
    request[8..10].copy_from_slice(&(HEADER_BYTES as u16).to_le_bytes());
    request[10..12].copy_from_slice(&OPCODE_PROGRAM_LOAD.to_le_bytes());
    request[20..24].copy_from_slice(&(payload_length as u32).to_le_bytes());
    request[32..34].copy_from_slice(&4u16.to_le_bytes());
    request[36..40].copy_from_slice(&4u32.to_le_bytes());
    request[48..52].copy_from_slice(&1u32.to_le_bytes());
    let bytes_offset = HEADER_BYTES + 24;
    request[bytes_offset..bytes_offset + 2].copy_from_slice(&12u16.to_le_bytes());
    request[bytes_offset + 4..bytes_offset + 8]
        .copy_from_slice(&(artifact.len() as u32).to_le_bytes());
    request[bytes_offset + 16..bytes_offset + 16 + artifact.len()].copy_from_slice(artifact);
    Some(request)
}

fn release(runtime: u64, handle: u64) {
    let mut request = [0u8; 48];
    request[0..4].copy_from_slice(b"TLV2");
    request[4..6].copy_from_slice(&2u16.to_le_bytes());
    request[8..10].copy_from_slice(&32u16.to_le_bytes());
    request[10..12].copy_from_slice(&OPCODE_HANDLE_RELEASE.to_le_bytes());
    request[20..24].copy_from_slice(&16u32.to_le_bytes());
    request[32..34].copy_from_slice(&14u16.to_le_bytes());
    request[40..48].copy_from_slice(&handle.to_le_bytes());
    let mut output = [0u8; 48];
    let mut written = 0;
    // SAFETY: all descriptors reference live, disjoint local storage.
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
    assert_ne!(status, TINY_STATUS_PANIC);
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn read_u64(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap())
}
