//! Fresh-code call startup: compile, execute and drop inside every sample.
//! Source preparation, verification, fingerprinting and golden checks are untimed.

use super::{
    Benchmark,
    JitOptions,
    JitProgram,
    VerifiedProgram,
    bench,
    compile_source_verified,
    leak_name,
    run_compiled_jit,
    run_verified_program,
};

#[derive(Clone, Copy, Debug)]
enum CallShape {
    Static,
    FunctionValue,
    Closure,
    Alternating,
}

const SHAPES: [CallShape; 4] = [
    CallShape::Static,
    CallShape::FunctionValue,
    CallShape::Closure,
    CallShape::Alternating,
];
const CALL_COUNTS: [usize; 4] = [0, 1, 8, 32];
const BASE_ITERATIONS: u64 = 600;

impl CallShape {
    fn label(self) -> &'static str {
        match self {
            Self::Static => "static",
            Self::FunctionValue => "function_values",
            Self::Closure => "closures",
            Self::Alternating => "alternating",
        }
    }

    fn source(self, calls: usize) -> String {
        let (declarations, setup, selection, invocation) = match self {
            Self::Static => ("fn target(value) { return value + 3 }", "", "", "target(i)"),
            Self::FunctionValue => {
                ("fn target(value) { return value + 3 }", "let callback = target", "", "callback(i)")
            }
            Self::Closure => {
                (
                    "fn target(capture, value) { return capture + value }",
                    "let callback = closure_new(\"target\", [3])",
                    "",
                    "callback(i)",
                )
            }
            Self::Alternating => {
                (
                    "fn first(value) { return value + 3 } fn second(value) { return value + 5 }",
                    "let callback = first let selector = 0",
                    "if selector == 0 { callback = first } else { callback = second } selector = 1 - selector",
                    "callback(i)",
                )
            }
        };
        format!(
            "{declarations} {setup} let i = 0 let total = 0 \
             while i < {calls} {{ {selection} total = total + {invocation} i = i + 1 }} print total"
        )
    }

    fn expected(self, calls: usize) -> String {
        let total = calls * calls.saturating_sub(1) / 2 + 3 * calls;
        let alternating_extra = match self {
            Self::Alternating => 2 * (calls / 2),
            _ => 0,
        };
        format!("{}\n", total + alternating_extra)
    }

    fn prepare(self, calls: usize) -> (VerifiedProgram, String) {
        let verified = compile_source_verified(&self.source(calls)).expect("cold-call source compilation");
        let expected = self.expected(calls);
        let mut output = Vec::new();
        drop(run_verified_program(&verified, "vm", &mut output, Vec::new()).expect("cold-call VM reference"));
        assert_eq!(output, expected.as_bytes(), "{self:?}/{calls}: VM golden");
        // Prime immutable metadata only. No compiled/adaptive owner is retained.
        assert!(!verified.fingerprint().is_empty());
        (verified, expected)
    }
}

pub(super) fn benchmarks() -> Vec<Benchmark> {
    let mut benchmarks = Vec::with_capacity(SHAPES.len() * CALL_COUNTS.len() * 2);
    for shape in SHAPES {
        for calls in CALL_COUNTS {
            let (verified, expected) = shape.prepare(calls);
            for (mode, enabled) in [("off", false), ("on", true)] {
                let options = JitOptions::new().with_call_feedback(enabled);
                let mut checked =
                    JitProgram::compile_verified_with_options(&verified, options).expect("cold-call parity lowering");
                let mut output = Vec::new();
                drop(
                    checked
                        .run(&mut output, Vec::new())
                        .expect("cold-call parity execution"),
                );
                assert_eq!(output, expected.as_bytes(), "{shape:?}/{calls}/{mode}: JIT golden");
                assert!(checked.execution_profile().is_none());
                drop(checked);
                let input = verified.clone();
                let name = leak_name(format!("call_cold.{mode}.{}_{calls}", shape.label()));
                benchmarks.push(bench(name, BASE_ITERATIONS, move || {
                    // Harness warmups also take this fresh-owner route. They
                    // cannot promote the code used by a later timed iteration.
                    let mut program =
                        JitProgram::compile_verified_with_options(&input, options).expect("cold-call timed lowering");
                    run_compiled_jit(&mut program, Vec::new());
                }));
            }
        }
    }
    benchmarks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cold_rows_pair_identical_work_and_keep_the_static_control() {
        let mut rows = benchmarks();
        assert_eq!(rows.len(), 32);
        let (pairs, remainder) = rows.as_chunks::<2>();
        assert!(remainder.is_empty());
        for [off, on] in pairs {
            assert_eq!(off.name.strip_prefix("call_cold.off."), on.name.strip_prefix("call_cold.on."),);
            assert_eq!(off.iterations, BASE_ITERATIONS);
            assert_eq!(on.iterations, BASE_ITERATIONS);
        }
        assert_eq!(rows[0].name, "call_cold.off.static_0");
        // Exercise the same closures used by all three harness warmups.
        for row in &mut rows {
            for _ in 0..3 {
                (row.run)();
            }
        }
    }

    #[test]
    fn fresh_call_samples_follow_cold_boundaries() {
        for shape in SHAPES {
            for calls in [0, 1, 7, 8, 9, 31, 32, 33] {
                let (verified, expected) = shape.prepare(calls);
                for threshold in [0, 1, 8] {
                    for enabled in [false, true] {
                        let options = JitOptions::new()
                            .with_hot_back_edge_threshold(threshold)
                            .with_call_feedback(enabled)
                            .with_execution_profile(true);
                        for _ in 0..2 {
                            let mut compiled = JitProgram::compile_verified_with_options(&verified, options)
                                .expect("fresh boundary lowering");
                            assert_eq!(compiled.stats().compiled_chunks, 1);
                            let mut output = Vec::new();
                            drop(compiled.run(&mut output, Vec::new()).expect("fresh boundary run"));
                            assert_eq!(output, expected.as_bytes(), "{shape:?}/{calls}/{threshold}/{enabled}");
                            let expected_chunks = match (shape, calls) {
                                (_, 0) => 1,
                                (CallShape::Alternating, 2..) => 3,
                                _ => 2,
                            };
                            assert_eq!(compiled.stats().compiled_chunks, expected_chunks);
                            let (samples, promotions, hits) = match (shape, options.call_feedback_enabled()) {
                                (_, false) | (CallShape::Static, _) => (0, 0, 0),
                                (CallShape::Alternating, true) => (calls.min(32), 0, 0),
                                (_, true) => (calls.min(8), usize::from(calls >= 8), calls.saturating_sub(8)),
                            };
                            let profile = compiled.execution_profile().expect("boundary profile");
                            assert_eq!(profile.call_feedback_samples, samples as u64);
                            assert_eq!(profile.call_specializations, promotions as u64);
                            assert_eq!(profile.call_guard_hits, hits as u64);
                            assert_eq!(profile.call_guard_misses, 0);
                            assert_eq!(profile.call_deoptimizations, 0);
                        }
                    }
                }
            }
        }
    }
}
