//! Measures boundary dispatch separately from compilation and VM execution.
use std::hint::black_box;
use std::time::Instant;

use tinylang::v2::*;

fn main() {
    let mut runtime = 0;
    let started = Instant::now();
    // SAFETY: runtime is live, exclusive output storage.
    assert_eq!(unsafe { tinylang_v2_runtime_create(&mut runtime) }, TINY_STATUS_OK);
    println!("initialization_ns,{}", started.elapsed().as_nanos());
    println!("workload,sample,iterations,ns_per_call,calls_per_second,payload_bytes_per_second");
    measure(runtime, "nop", request(OPCODE_NOP, &[]), 4_000_000, 0);
    let mut scalar = vec![0u8; 24];
    scalar[0..2].copy_from_slice(&5u16.to_le_bytes());
    scalar[4..8].copy_from_slice(&8u32.to_le_bytes());
    scalar[16..24].copy_from_slice(&42u64.to_le_bytes());
    measure(runtime, "echo_u64", request(OPCODE_ECHO, &scalar), 2_000_000, 8);
    for length in [4096usize, 1024 * 1024] {
        let mut value = vec![0x5a; VALUE_HEADER_BYTES + length];
        value[..VALUE_HEADER_BYTES].fill(0);
        value[0..2].copy_from_slice(&12u16.to_le_bytes());
        value[4..8].copy_from_slice(&(length as u32).to_le_bytes());
        let iterations = if length == 4096 { 1_000_000 } else { 4000 };
        measure(runtime, &format!("echo_bytes_{length}"), request(OPCODE_ECHO, &value), iterations, length);
    }
    assert_eq!(tinylang_v2_runtime_destroy(runtime), TINY_STATUS_OK);
}

fn request(opcode: u16, payload: &[u8]) -> Vec<u8> {
    let mut frame = vec![0; HEADER_BYTES + payload.len()];
    frame[..4].copy_from_slice(&MAGIC);
    frame[4..6].copy_from_slice(&SCHEMA_MAJOR.to_le_bytes());
    frame[6..8].copy_from_slice(&SCHEMA_MINOR.to_le_bytes());
    frame[8..10].copy_from_slice(&(HEADER_BYTES as u16).to_le_bytes());
    frame[10..12].copy_from_slice(&opcode.to_le_bytes());
    frame[20..24].copy_from_slice(&(payload.len() as u32).to_le_bytes());
    frame[HEADER_BYTES..].copy_from_slice(payload);
    frame
}

fn measure(runtime: TinyRuntimeHandle, name: &str, request: Vec<u8>, iterations: usize, data_bytes: usize) {
    let mut output = vec![0; request.len()];
    let input = TinyBuffer {
        data: request.as_ptr(),
        length: request.len() as u64,
    };
    let destination = TinyMutableBuffer {
        data: output.as_mut_ptr(),
        capacity: output.len() as u64,
    };
    let mut written = 0;
    for sample in 0..8 {
        let started = Instant::now();
        for _ in 0..iterations {
            // SAFETY: disjoint buffers remain live and exclusively owned throughout the call.
            let status =
                unsafe { tinylang_v2_call(black_box(runtime), black_box(input), black_box(destination), &mut written) };
            assert_eq!(status, TINY_STATUS_OK);
            black_box(&output);
        }
        let elapsed = started.elapsed().as_secs_f64();
        assert_eq!(written as usize, request.len());
        assert_eq!(u32::from_le_bytes(output[16..20].try_into().unwrap()), TINY_STATUS_OK);
        assert_eq!(&output[HEADER_BYTES..], &request[HEADER_BYTES..]);
        if sample != 0 {
            let calls = iterations as f64 / elapsed;
            println!(
                "{name},{sample},{iterations},{:.3},{calls:.3},{:.3}",
                elapsed * 1e9 / iterations as f64,
                calls * data_bytes as f64
            );
        }
    }
}
