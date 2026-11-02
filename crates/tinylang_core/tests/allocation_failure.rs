#![cfg(feature = "testing-hooks")]

//! Real fixed-pool pressure across native owners, frames and execution.
//! All scenarios share one test so process-wide pressure/counter intervals
//! cannot overlap. Host allocation failure is not simulated by this harness.

use std::fmt::Write;

use ralloc::testing::{InstrumentationSnapshot, instrumentation_snapshot, reset_instrumentation};
use ralloc::{RallocBuffer, RallocError, VmAllocator};
use tinylang::{JitOptions, JitProgram, RuntimeValue, TinyMemory, compile_source_verified, run_verified_program};

struct ExhaustionFixture {
    name:             &'static str,
    source:           String,
    failed_output:    &'static str,
    recovered_output: &'static str,
    error:            &'static str,
}

fn exhaustion_fixtures() -> [ExhaustionFixture; 4] {
    let mut declarations = String::new();
    // 8,192 fixed 64-byte slots require 512 KiB, irrespective of how many
    // dead stores symbolic lowering removes from executable operations.
    for index in 0..8192 {
        writeln!(declarations, "let local_{index} = {index}").expect("source string");
    }
    [
        ExhaustionFixture {
            name: "main_frame",
            source: format!("{declarations}print 17"),
            failed_output: "",
            recovered_output: "17\n",
            error: "Failed to allocate VM memory",
        },
        ExhaustionFixture {
            name: "callee_frame",
            source: format!("fn large() {{ {declarations} return 17 }} print 3 print large()"),
            failed_output: "3\n",
            recovered_output: "3\n17\n",
            error: "Failed to allocate VM memory",
        },
        ExhaustionFixture {
            name: "callee_buffer",
            source: "fn large() { let bytes = buffer(1048576) return len(bytes) } print 5 print large()".into(),
            failed_output: "5\n",
            recovered_output: "5\n1048576\n",
            error: "Failed to allocate buffer",
        },
        ExhaustionFixture {
            name: "array_growth",
            source: "let values = [] let i = 0 while i < 4096 { let ignored = push(values, i) i = i + 1 } print len(values) let ignored = push(values, 4096) print len(values)".into(),
            failed_output: "4096\n",
            recovered_output: "4096\n4097\n",
            error: "failed to grow container",
        },
    ]
}

fn hold_pool_below(request: usize) -> Vec<RallocBuffer> {
    let mut held = Vec::new();
    // Even a broken exhaustion path cannot turn this test into an unbounded
    // host-Vec growth loop. Successful live requests must fit the fixed pool.
    for _ in 0..=VmAllocator::capacity() / request {
        match RallocBuffer::try_new(request) {
            Ok(buffer) => held.push(buffer),
            Err(RallocError::OutOfMemory) => {
                assert!(!held.is_empty(), "pressure must reserve real native storage");
                return held;
            }
            Err(error) => panic!("unexpected pressure failure: {error:?}"),
        }
    }
    panic!("live native requests exceeded the fixed pool");
}

fn assert_interval_released(before: InstrumentationSnapshot) {
    let after = instrumentation_snapshot();
    assert_eq!(after.live_payload_bytes, before.live_payload_bytes, "execution leaked native payload");
    assert_eq!(
        after.allocations - before.allocations,
        after.deallocations - before.deallocations,
        "execution did not release every new native owner",
    );
}

fn assert_native_failure(
    run: impl FnOnce(&mut Vec<u8>) -> tinylang::Result<TinyMemory>,
    fixture: &ExhaustionFixture,
) -> String {
    let before = instrumentation_snapshot();
    let mut output = Vec::new();
    let error = run(&mut output).expect_err("native pressure must be a recoverable runtime failure");
    assert_eq!(output, fixture.failed_output.as_bytes(), "{} mutation/output order", fixture.name);
    let error = error.to_string();
    assert!(error.contains(fixture.error), "{}: {error}", fixture.name);
    assert!(instrumentation_snapshot().allocation_failures > before.allocation_failures);
    assert_interval_released(before);
    error
}

fn assert_recovered_execution(
    run: impl FnOnce(&mut Vec<u8>) -> tinylang::Result<TinyMemory>,
    fixture: &ExhaustionFixture,
) {
    let before = instrumentation_snapshot();
    let mut output = Vec::new();
    let memory = run(&mut output).expect("execution must recover once native pressure is released");
    assert_eq!(output, fixture.recovered_output.as_bytes(), "{} recovery", fixture.name);
    drop(memory);
    assert_interval_released(before);
}

#[test]
fn native_exhaustion_preserves_owners_and_repeated_execution_cleanup() {
    reset_instrumentation();
    assert_eq!(instrumentation_snapshot().live_payload_bytes, 0);
    let mut cases = exhaustion_fixtures().map(|fixture| {
        let verified = compile_source_verified(&fixture.source).expect("exhaustion fixture");
        let compiled = [0, 1, 8].map(|threshold| {
            JitProgram::compile_verified_with_options(
                &verified,
                JitOptions::new().with_hot_back_edge_threshold(threshold),
            )
            .expect("preparing a failure/recovery pair")
        });
        (fixture, verified, compiled)
    });
    assert_eq!(instrumentation_snapshot().live_payload_bytes, 0, "preparation owns no execution payload");

    let mut bytes = RallocBuffer::try_new_aligned(64, 64).expect("original byte owner");
    bytes.as_mut_slice().fill(0xa7);
    let original_pointer = bytes.as_ptr();
    let vm = VmAllocator::global();
    let mut allocation = vm.allocate(64).expect("original VM allocation");
    allocation.as_mut_slice().fill(0x3d);
    let original_vm_pointer = allocation.as_ptr();
    let mut memory = TinyMemory::try_new(16_384).expect("original 1-MiB frame");
    memory.store(0, RuntimeValue::I64(91)).expect("first slot");
    memory.store(16_383, RuntimeValue::I64(37)).expect("last slot");

    let pressure = hold_pool_below(512 * 1024);
    println!(
        "native-pressure held_blocks={} live_payload_bytes={}",
        pressure.len(),
        instrumentation_snapshot().live_payload_bytes
    );
    let before_failed_growth = instrumentation_snapshot();
    assert_eq!(bytes.try_resize(1024 * 1024), Err(RallocError::OutOfMemory));
    assert_eq!(bytes.as_ptr(), original_pointer);
    assert_eq!(bytes.len(), 64);
    assert_eq!(bytes.as_slice(), &[0xa7; 64]);
    allocation = match vm.reallocate(allocation, 1024 * 1024) {
        Err((original, error)) => {
            assert_eq!(error, RallocError::OutOfMemory);
            original
        }
        Ok(unexpected) => {
            vm.deallocate(unexpected);
            panic!("native resize unexpectedly escaped pool pressure");
        }
    };
    assert_eq!(allocation.as_ptr(), original_vm_pointer);
    assert_eq!(allocation.len(), 64);
    assert_eq!(allocation.as_slice(), &[0x3d; 64]);
    let error = memory.try_clone().expect_err("frame clone must report exhaustion");
    assert!(error.to_string().contains("Failed to allocate VM memory"));
    assert!(matches!(memory.load(0), Ok(RuntimeValue::I64(91))));
    assert!(matches!(memory.load(16_383), Ok(RuntimeValue::I64(37))));
    assert_interval_released(before_failed_growth);

    for (fixture, verified, compiled) in &mut cases {
        let reference =
            assert_native_failure(|output| run_verified_program(verified, "vm", output, Vec::new()), fixture);
        let repeated =
            assert_native_failure(|output| run_verified_program(verified, "vm", output, Vec::new()), fixture);
        assert_eq!(repeated, reference, "repeated portable failure");
        for program in compiled {
            for _ in 0..2 {
                let error = assert_native_failure(|output| program.run(output, Vec::new()), fixture);
                assert_eq!(error, reference, "VM/JIT failure location and message");
            }
        }
        println!("native-exhaustion fixture={} failures=8 cleanup=balanced", fixture.name);
    }

    drop(pressure);
    for (fixture, verified, compiled) in &mut cases {
        for _ in 0..2 {
            assert_recovered_execution(|output| run_verified_program(verified, "vm", output, Vec::new()), fixture);
        }
        // Reuse the same compiled instances that failed above: lazy chunks,
        // feedback and cleared frame pools must not retain poisoned state.
        for program in compiled {
            for _ in 0..2 {
                assert_recovered_execution(|output| program.run(output, Vec::new()), fixture);
            }
        }
        println!("native-recovery fixture={} executions=8 cleanup=balanced", fixture.name);
    }
    drop(memory);
    vm.deallocate(allocation);
    drop(bytes);
    let final_counts = instrumentation_snapshot();
    assert_eq!(final_counts.live_payload_bytes, 0);
    assert_eq!(final_counts.allocations, final_counts.deallocations);
}
