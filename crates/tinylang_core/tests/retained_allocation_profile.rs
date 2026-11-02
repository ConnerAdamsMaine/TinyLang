#![cfg(feature = "testing-hooks")]

#[path = "allocation/retained.rs"]
mod retained;

use ralloc::testing::instrumentation_snapshot;
use retained::{COUNTS, OBJECTS};
use tinylang::testing::{reset_runtime_cost_counters, runtime_cost_counters};
use tinylang::{JitOptions, JitProgram, run_verified_program};

#[test]
fn retained_object_fixtures_preserve_values_and_repeated_vm_parity() {
    for object in OBJECTS {
        for count in [0, 1, COUNTS[0]] {
            let (verified, expected) = object.prepare(count);
            let mut output = Vec::new();
            drop(run_verified_program(&verified, "vm", &mut output, Vec::new()).expect("retained-object VM"));
            assert_eq!(output, expected.as_bytes(), "{object:?}/{count}");
            for threshold in [0, 1, 8] {
                let mut compiled = JitProgram::compile_verified_with_options(
                    &verified,
                    JitOptions::new().with_hot_back_edge_threshold(threshold),
                )
                .expect("retained-object JIT");
                for _ in 0..2 {
                    output.clear();
                    drop(
                        compiled
                            .run(&mut output, Vec::new())
                            .expect("repeated retained-object JIT"),
                    );
                    assert_eq!(output, expected.as_bytes(), "{object:?}/{count}/{threshold}");
                }
            }
        }
    }
}

#[test]
#[ignore = "process-exclusive retained-object allocation counts; run alone with --ignored --nocapture"]
fn report_retained_object_allocation_scaling() {
    for object in OBJECTS {
        for count in [0, 1].into_iter().chain(COUNTS) {
            let (verified, expected) = object.prepare(count);
            let mut reference = Vec::new();
            drop(run_verified_program(&verified, "vm", &mut reference, Vec::new()).expect("retained-object VM"));
            assert_eq!(reference, expected.as_bytes(), "{object:?}/{count}");
            for threshold in [0, 8] {
                let mut compiled = JitProgram::compile_verified_with_options(
                    &verified,
                    JitOptions::new()
                        .with_hot_back_edge_threshold(threshold)
                        .with_execution_profile(true),
                )
                .expect("retained-object profile lowering");
                for phase in ["cold_execution", "warm_execution"] {
                    let mut output = Vec::with_capacity(expected.len());
                    compiled.reset_execution_profile();
                    reset_runtime_cost_counters();
                    let report = compiled
                        .run_report(&mut output, Vec::new())
                        .expect("retained-object profile");
                    assert_eq!(output, reference, "{object:?}/{count}/{threshold}/{phase}");
                    let heap = report.heap_before_shutdown;
                    drop(report);
                    let allocation = instrumentation_snapshot();
                    let runtime = runtime_cost_counters();
                    assert_eq!(allocation.allocations, allocation.deallocations, "native owner cleanup");
                    assert_eq!(allocation.live_payload_bytes, 0, "no retained native payload across runs");
                    assert_eq!(allocation.allocation_failures, 0);
                    assert_eq!(runtime.ralloc_growth_events, allocation.growth_events);
                    assert_eq!(runtime.ralloc_bytes_copied, allocation.bytes_copied);
                    eprintln!("retained_{}_{count}/{threshold}/{phase}: heap={heap:?}", object.label());
                    eprintln!("  allocator={allocation:?}");
                    eprintln!("  runtime={runtime:?}");
                    eprintln!("  execution={:?}", compiled.execution_profile());
                    eprintln!("  jit={:?}", compiled.stats());
                }
            }
        }
    }
}
