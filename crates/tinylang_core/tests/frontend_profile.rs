#![cfg(feature = "testing-hooks")]

#[path = "frontend/fixtures.rs"]
mod fixtures;

use std::hint::black_box;
use std::time::{Duration, Instant};

use tinylang::testing::{
    FrontendMeasurement as Measurement,
    FrontendOptimization as Optimization,
    FrontendPhase as Phase,
    FrontendProfile,
    capture_source_compilation,
};
use tinylang::{
    JitOptions,
    JitProgram,
    VerifiedProgram,
    compile_source_unoptimized_verified_with_filename,
    compile_source_verified_with_filename,
    run_verified_program,
};

const FILENAME: &str = "frontend-profile.to";
const MODES: [Optimization; 2] = [Optimization::Enabled, Optimization::Disabled];

fn control(source: &str, optimization: Optimization) -> tinylang::Result<VerifiedProgram> {
    match optimization {
        Optimization::Enabled => compile_source_verified_with_filename(source, FILENAME),
        Optimization::Disabled => compile_source_unoptimized_verified_with_filename(source, FILENAME),
    }
}

fn accounted(profile: &FrontendProfile) -> Duration {
    let sum = Phase::ALL
        .into_iter()
        .filter_map(|phase| profile.measurement(phase).elapsed())
        .sum::<Duration>();
    assert!(sum <= profile.elapsed(), "source-pipeline intervals do not overlap");
    sum
}

fn successful_phases(profile: &FrontendProfile, optimization: Optimization) {
    for phase in Phase::ALL {
        if phase == Phase::Peephole && optimization == Optimization::Disabled {
            assert_eq!(profile.measurement(phase), Measurement::NotRun);
        } else {
            assert!(matches!(profile.measurement(phase), Measurement::Succeeded(_)), "{phase:?}");
        }
    }
    accounted(profile);
}

fn check_output(verified: &VerifiedProgram, expected: &str) {
    for backend in ["vm", "jit"] {
        let mut output = Vec::new();
        drop(run_verified_program(verified, backend, &mut output, Vec::new()).expect("fixture execution"));
        assert_eq!(output, expected.as_bytes(), "{backend}");
    }
}

#[test]
fn source_stage_capture_matches_public_compilation_and_backend_outputs() {
    for fixture in fixtures::fixtures() {
        for optimization in MODES {
            let expected = control(&fixture.source, optimization).expect("public compilation");
            let (observed, profile) =
                capture_source_compilation(&fixture.source, FILENAME, optimization).expect("phase clocks");
            let observed = observed.expect("observed compilation");
            assert_eq!(observed, expected, "{}", fixture.name);
            assert_eq!(observed.fingerprint(), expected.fingerprint(), "{}", fixture.name);
            successful_phases(&profile, optimization);
            check_output(&observed, &fixture.output);
        }
    }
}

#[test]
fn compile_errors_preserve_diagnostics_and_leave_later_phases_unrun() {
    let over_budget = format!("print \"{}\"", "x".repeat(1024 * 1024 + 1));
    for (source, failed) in [
        ("print \"unterminated", Phase::ConstructionAndLexing),
        ("let answer = missing_name", Phase::ParsingEmissionAndAssembly),
        ("import \"missing.to\"", Phase::ParsingEmissionAndAssembly),
        (over_budget.as_str(), Phase::Verification),
    ] {
        for optimization in MODES {
            let expected = control(source, optimization).expect_err("public rejection");
            let (observed, profile) = capture_source_compilation(source, FILENAME, optimization).expect("error clocks");
            assert_eq!(observed.expect_err("observed rejection").to_string(), expected.to_string());
            let mut after_failure = false;
            for phase in Phase::ALL {
                let measured = profile.measurement(phase);
                if phase == failed {
                    assert!(matches!(measured, Measurement::Failed(_)), "{failed:?}");
                    after_failure = true;
                } else if after_failure || (phase == Phase::Peephole && optimization == Optimization::Disabled) {
                    assert_eq!(measured, Measurement::NotRun, "{phase:?}");
                } else {
                    assert!(matches!(measured, Measurement::Succeeded(_)), "{phase:?}");
                }
            }
            accounted(&profile);
        }
    }
}

#[test]
fn returned_source_owners_keep_exact_runtime_errors_after_inputs_drop() {
    for optimization in MODES {
        let source = String::from("fn divide(x) {\n  return 7 / x\n}\nprint divide(0)\n");
        let expected = control(&source, optimization).expect("reference compiler");
        let (observed, profile) = capture_source_compilation(&source, FILENAME, optimization).expect("capture");
        let observed = observed.expect("source compiler");
        successful_phases(&profile, optimization);
        drop(source);
        let mut expected_output = Vec::new();
        let error =
            run_verified_program(&expected, "vm", &mut expected_output, Vec::new()).expect_err("reference error");
        assert!(error.to_string().contains(FILENAME));
        for threshold in [0, 1, 8] {
            let options = JitOptions::new().with_hot_back_edge_threshold(threshold);
            let mut compiled = JitProgram::compile_verified_with_options(&observed, options).expect("JIT");
            for _ in 0..2 {
                let mut output = Vec::new();
                let actual = compiled.run(&mut output, Vec::new()).expect_err("division by zero");
                assert_eq!(actual.to_string(), error.to_string());
                assert_eq!(output, expected_output);
            }
        }
    }
}

fn timed_batch(iterations: u64, mut operation: impl FnMut()) -> Duration {
    let started = Instant::now();
    for _ in 0..iterations {
        operation();
    }
    Instant::now()
        .checked_duration_since(started)
        .expect("monotonic batch clock")
}

fn control_batch(source: &str, optimization: Optimization, iterations: u64) -> Duration {
    match optimization {
        Optimization::Enabled => {
            timed_batch(iterations, || {
                drop(black_box(compile_source_verified_with_filename(source, FILENAME).expect("public compiler")));
            })
        }
        Optimization::Disabled => {
            timed_batch(iterations, || {
                drop(black_box(
                    compile_source_unoptimized_verified_with_filename(source, FILENAME).expect("public compiler"),
                ));
            })
        }
    }
}

#[derive(Default)]
struct ObservedBatch {
    elapsed:  Duration,
    captured: Duration,
    phases:   [Duration; 4],
    calls:    [u64; 4],
}

fn observed_batch(source: &str, optimization: Optimization, iterations: u64) -> ObservedBatch {
    let mut batch = ObservedBatch::default();
    batch.elapsed = timed_batch(iterations, || {
        let (program, profile) = capture_source_compilation(source, FILENAME, optimization).expect("phase clocks");
        drop(black_box(program.expect("observed compiler")));
        batch.captured += profile.elapsed();
        for (index, phase) in Phase::ALL.into_iter().enumerate() {
            match profile.measurement(phase) {
                Measurement::NotRun => {}
                Measurement::Succeeded(elapsed) => {
                    batch.phases[index] += elapsed;
                    batch.calls[index] += 1;
                }
                Measurement::Failed(_) => panic!("successful compilation must not report a failed phase"),
            }
        }
    });
    let accounted = batch.phases.into_iter().sum::<Duration>();
    assert!(accounted <= batch.captured && batch.captured <= batch.elapsed);
    batch
}

/// Explicit hooks-build wall-clock observations, not ordinary-release
/// optimization acceptance. Reporting and fixture generation are untimed.
/// Both batch routes include fresh verified-owner destruction.
#[test]
#[ignore = "source compilation stage attribution; run alone in release mode with --ignored --nocapture"]
fn report_source_compilation_phases() {
    for fixture in fixtures::fixtures() {
        for optimization in MODES {
            let verified = control(&fixture.source, optimization).expect("reference fixture");
            check_output(&verified, &fixture.output);
            let (observed, one) =
                capture_source_compilation(&fixture.source, FILENAME, optimization).expect("warm capture");
            assert_eq!(observed.expect("observed fixture"), verified);
            successful_phases(&one, optimization);
            let mode = match optimization {
                Optimization::Enabled => "optimized",
                Optimization::Disabled => "unoptimized",
            };
            for sample in 1..=7 {
                let (batch, control, order) = if sample % 2 == 1 {
                    let control = control_batch(&fixture.source, optimization, fixture.iterations);
                    (observed_batch(&fixture.source, optimization, fixture.iterations), control, "control_first")
                } else {
                    let batch = observed_batch(&fixture.source, optimization, fixture.iterations);
                    (batch, control_batch(&fixture.source, optimization, fixture.iterations), "observed_first")
                };
                println!(
                    "frontend-stages fixture={} mode={mode} sample={sample} iterations={} order={order} \
                     source_bytes={} bytecode_ops={} functions={} observed_ns={} control_hooks_ns={} captured_ns={}",
                    fixture.name,
                    fixture.iterations,
                    fixture.source.len(),
                    verified.program().code().len(),
                    verified.program().functions().len(),
                    batch.elapsed.as_nanos(),
                    control.as_nanos(),
                    batch.captured.as_nanos(),
                );
                for (index, phase) in Phase::ALL.into_iter().enumerate() {
                    let expected_calls = if one.measurement(phase) == Measurement::NotRun {
                        0
                    } else {
                        fixture.iterations
                    };
                    assert_eq!(batch.calls[index], expected_calls, "stable stage boundaries");
                    println!(
                        "frontend-stage fixture={} mode={mode} sample={sample} phase={} calls={} elapsed_ns={}",
                        fixture.name,
                        phase.name(),
                        batch.calls[index],
                        batch.phases[index].as_nanos(),
                    );
                }
            }
        }
    }
}
