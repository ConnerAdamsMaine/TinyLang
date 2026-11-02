#![cfg(feature = "testing-hooks")]

#[path = "allocation/host.rs"]
mod host;

use host::HostAllocationInterval;
use ralloc::testing::instrumentation_snapshot;
use tinylang::testing::{reset_runtime_cost_counters, runtime_cost_counters};
use tinylang::{JitOptions, JitProgram, compile_source_verified, run_verified_program};

enum Callee {
    Direct,
    Function,
    Closure(usize),
}

impl Callee {
    fn source(&self, calls: usize) -> String {
        let (parameters, setup, target) = match self {
            Self::Direct => ("value".to_owned(), String::new(), "target"),
            Self::Function => ("value".to_owned(), "let callback = target".to_owned(), "callback"),
            Self::Closure(count) => {
                let parameters = (0..*count)
                    .map(|index| format!("capture_{index}, "))
                    .collect::<String>();
                let captures = (0..*count)
                    .map(|index| index.to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                (
                    format!("{parameters}value"),
                    format!("let callback = closure_new(\"target\", [{captures}])"),
                    "callback",
                )
            }
        };
        format!(
            "fn target({parameters}) {{ return value }} {setup} let i = 0 let total = 0 \
             while i < {calls} {{ total = total + {target}(i) i = i + 1 }} print total"
        )
    }
}

/// Run alone: host and Ralloc counters aggregate the entire process. Counts
/// describe this build, not a language guarantee or instrumented latency.
#[test]
#[ignore = "call/capture allocation attribution; run alone with --ignored --nocapture"]
fn report_call_allocation_profiles() {
    let workloads = [
        ("direct", Callee::Direct),
        ("function_value", Callee::Function),
        ("closure_0", Callee::Closure(0)),
        ("closure_1", Callee::Closure(1)),
        ("closure_8", Callee::Closure(8)),
        ("closure_32", Callee::Closure(32)),
    ];
    for (name, callee) in workloads {
        for calls in [0, 1, 512] {
            let verified = compile_source_verified(&callee.source(calls)).expect("call fixture compilation");
            let mut reference = Vec::new();
            drop(run_verified_program(&verified, "vm", &mut reference, Vec::new()).expect("VM reference"));
            for (call_feedback, threshold) in [(false, 0), (false, 8), (true, 0), (true, 8)] {
                let mut compiled = JitProgram::compile_verified_with_options(
                    &verified,
                    JitOptions::new()
                        .with_hot_back_edge_threshold(threshold)
                        .with_call_feedback(call_feedback),
                )
                .expect("call fixture lowering");
                for phase in ["cold", "warm"] {
                    // Compilation, the output buffer and all reporting are
                    // outside the interval; lazy callee lowering is inside.
                    let mut output = Vec::with_capacity(reference.len());
                    reset_runtime_cost_counters();
                    let interval = HostAllocationInterval::start();
                    drop(compiled.run_report(&mut output, Vec::new()).expect("call execution"));
                    let host = interval.finish();
                    let runtime = runtime_cost_counters();
                    let ralloc = instrumentation_snapshot();
                    assert_eq!(output, reference, "{name}/{calls}/{threshold}/{phase}");
                    assert_eq!(ralloc.allocations, ralloc.deallocations, "Ralloc cleanup");
                    assert_eq!(ralloc.live_payload_bytes, 0, "fresh heap after execution");
                    // Warm code and cleared operand buffers are retained by
                    // JitProgram, so host allocation/deallocation need not balance.
                    eprintln!(
                        "{name}/{calls}/{threshold}/{phase}: call_feedback={call_feedback} \
                         host_allocations={} host_deallocations={} \
                         host_reallocations={} host_bytes={} host_reallocated_bytes={} \
                         buckets_64_256_1024_large={:?}",
                        host.allocations,
                        host.deallocations,
                        host.reallocations,
                        host.allocated_bytes,
                        host.reallocated_bytes,
                        host.size_buckets,
                    );
                    eprintln!("  runtime={runtime:?}");
                    eprintln!("  ralloc={ralloc:?}");
                }
            }
        }
    }
}

#[test]
#[ignore = "optional-profile allocation attribution; run alone with --ignored --nocapture"]
fn report_optional_profile_allocation() {
    let verified = compile_source_verified("print 42").expect("profile fixture");
    // Both compilation intervals must start with the same memoized metadata;
    // the token's first fingerprint allocation is not a profiling cost.
    assert!(!verified.fingerprint().is_empty());
    let interval = HostAllocationInterval::start();
    let plain = JitProgram::compile_verified(&verified).expect("ordinary lowering");
    let ordinary_counts = interval.finish();
    let interval = HostAllocationInterval::start();
    let profiled = JitProgram::compile_verified_with_options(&verified, JitOptions::new().with_execution_profile(true))
        .expect("profiled lowering");
    let profiled_counts = interval.finish();
    let profile_bytes = size_of::<tinylang::JitExecutionProfile>();
    eprintln!(
        "optional profile: header_bytes={} program_bytes={} plain={ordinary_counts:?} profiled={profiled_counts:?}",
        profile_bytes,
        size_of::<JitProgram>(),
    );
    assert!(plain.execution_profile().is_none());
    assert!(profiled.execution_profile().is_some());
    assert_eq!(profiled_counts.allocations, ordinary_counts.allocations + 1);
    assert_eq!(profiled_counts.allocated_bytes, ordinary_counts.allocated_bytes + profile_bytes);
    assert_eq!(profiled_counts.deallocations, ordinary_counts.deallocations);
    assert_eq!(profiled_counts.reallocations, ordinary_counts.reallocations);
    assert_eq!(profiled.retained_bytes(), plain.retained_bytes() + profile_bytes);
}
