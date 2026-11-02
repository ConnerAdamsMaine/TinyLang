#![cfg(feature = "testing-hooks")]

use ralloc::testing::instrumentation_snapshot;
use tinylang::testing::{reset_runtime_cost_counters, runtime_cost_counters};
use tinylang::{JitOptions, JitProgram, compile_source_verified, run_verified_program};

/// Each phase owns the process-wide measurement interval. These counters
/// attribute work, not latency; ordinary benchmarks disable instrumentation.
#[test]
#[ignore = "diagnostic allocation/dispatch profile; run alone with --ignored --nocapture"]
fn report_runtime_allocation_distributions() {
    let workloads = [
        ("small", "let answer = 42 print answer"),
        ("integer_loop", "let i = 0 let total = 0 while i < 4096 { total = total + i i = i + 1 } print total"),
        (
            "nested_integer_loop",
            "let i = 0 let total = 0 while i < 4096 { total = total + (i * 3) i = i + 1 } print total",
        ),
        (
            "float_loop",
            "let i = 0 let total = 0.25 while i < 4096 { total = total + 0.5 i = i + 1 } print total",
        ),
        (
            "functions",
            "fn add(a, b) { return a + b } let i = 0 let total = 0 while i < 512 { total = add(total, i) i = i + 1 } print total",
        ),
        (
            "recursion",
            "fn sum(n) { if n == 0 { return 0 } return n + sum(n - 1) } let i = 0 let total = 0 while i < 64 { total = total + sum(10) i = i + 1 } print total",
        ),
        (
            "closures",
            "fn add(a, b) { return a + b } let callback = closure_new(\"add\", [7]) let i = 0 let total = 0 while i < 512 { total = callback(i) i = i + 1 } print total",
        ),
        (
            "arrays",
            "let values = vec_new() let i = 0 while i < 256 { let ignored = push(values, i) i = i + 1 } print len(values) while len(values) > 0 { let ignored = pop(values) }",
        ),
        (
            "maps",
            "let values = map_new() let i = 0 while i < 256 { let ignored = map_set(values, i, i * 3) i = i + 1 } let total = 0 let j = 0 while j < 256 { total = total + map_get(values, j) j = j + 1 } print total",
        ),
        ("struct_enum", include_str!("programs/pass/016_enums.to")),
        (
            "strings_long_lived",
            "let values = vec_new() let i = 0 while i < 256 { let ignored = push(values, \"a persistent string\") i = i + 1 } print len(values)",
        ),
        (
            "cells_long_lived",
            "let values = vec_new() let i = 0 while i < 256 { let cell = alloc(i) let ignored = push(values, cell) i = i + 1 } print len(values)",
        ),
        ("cell_churn", "let i = 0 while i < 4096 { let cell = alloc(i) unsafe free(cell) i = i + 1 } print i"),
        (
            "mixed_types_branches",
            "let i = 0 let total = 0 while i < 1024 { if i < 512 { total = total + 1 } else { total = total + 0.5 } i = i + 1 } print total",
        ),
        (
            "changing_operand_loop",
            "let i = 0 let total = 0 let value = 1 while i < 4096 { if i < 2048 { value = 1 } else { value = 0.5 } total = total + value i = i + 1 } print total",
        ),
    ];

    for (name, source) in workloads {
        let verified = compile_source_verified(source).unwrap_or_else(|error| panic!("{name}: {error}"));
        let mut reference = Vec::new();
        drop(run_verified_program(&verified, "vm", &mut reference, Vec::new()).expect("reference execution"));
        for threshold in [0, 8] {
            let mut compiled = JitProgram::compile_verified_with_options(
                &verified,
                JitOptions::new()
                    .with_hot_back_edge_threshold(threshold)
                    .with_execution_profile(true),
            )
            .expect("profile lowering");
            for phase in ["cold_execution", "warm_execution"] {
                compiled.reset_execution_profile();
                let mut output = Vec::new();
                reset_runtime_cost_counters();
                let report = compiled.run_report(&mut output, Vec::new()).expect("profile execution");
                assert_eq!(output, reference, "{name}/{threshold}/{phase}");
                let heap = report.heap_before_shutdown;
                drop(report);
                let allocation = instrumentation_snapshot();
                let runtime = runtime_cost_counters();
                assert_eq!(allocation.allocations, allocation.deallocations, "{name} must release all Ralloc storage");
                assert_eq!(allocation.live_payload_bytes, 0, "{name} must have a fresh heap on the next run");
                assert_eq!(runtime.ralloc_growth_events, allocation.growth_events);
                assert_eq!(runtime.ralloc_bytes_copied, allocation.bytes_copied);
                eprintln!("{name}/{threshold}/{phase}: heap={heap:?}");
                eprintln!("  allocator={allocation:?}");
                eprintln!("  runtime={runtime:?}");
                eprintln!("  execution={:?}", compiled.execution_profile());
                eprintln!("  jit={:?}", compiled.stats());
            }
        }
    }
}
