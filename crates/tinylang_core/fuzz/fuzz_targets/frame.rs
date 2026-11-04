#![no_main]

use libfuzzer_sys::fuzz_target;
use tinylang::v2::{
    HEADER_BYTES, OPCODE_HANDLE_RELEASE, OPCODE_PROGRAM_LOAD, TINY_STATUS_OK, TINY_STATUS_PANIC,
    TinyBuffer, TinyMutableBuffer, tinylang_v2_call, tinylang_v2_runtime_create,
    tinylang_v2_runtime_destroy,
};

fuzz_target!(|input: &[u8]| {
    let mut runtime = 0;
    // SAFETY: the runtime handle and all call descriptors use live, disjoint
    // storage for the duration of each invocation.
    if unsafe { tinylang_v2_runtime_create(&mut runtime) } != TINY_STATUS_OK {
        return;
    }
    let mut output = [0u8; 4096];
    let mut written = 0;
    // SAFETY: input is borrowed from libFuzzer and output is local writable
    // storage. Neither overlaps `written`.
    let status = unsafe {
        tinylang_v2_call(
            runtime,
            TinyBuffer {
                data: input.as_ptr(),
                length: input.len() as u64,
            },
            TinyMutableBuffer {
                data: output.as_mut_ptr(),
                capacity: output.len() as u64,
            },
            &mut written,
        )
    };
    assert_ne!(status, TINY_STATUS_PANIC);
    if status == TINY_STATUS_OK
        && written == 48
        && output[0..4] == *b"TLV2"
        && read_u16(&output, 10) == OPCODE_PROGRAM_LOAD
        && read_u32(&output, 16) == TINY_STATUS_OK
    {
        release(runtime, read_u64(&output, HEADER_BYTES + 8));
    }
    let _ = tinylang_v2_runtime_destroy(runtime);
});

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

fn read_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(bytes[offset..offset + 2].try_into().unwrap())
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn read_u64(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap())
}
