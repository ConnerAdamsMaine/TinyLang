#![cfg(feature = "testing-hooks")]

#[path = "preparation/fixtures.rs"]
mod fixtures;
#[path = "preparation/sources.rs"]
mod sources;

use std::hint::black_box;
use std::time::{Duration, Instant};

use fixtures::{LARGE_CASES, PreparationFixture};
use sources::SOURCE_CASES;
use tinylang::testing::{PreparationPhase as Phase, PreparationProfile, capture_jit_preparation};
use tinylang::{
    Instr, JitCache, JitOptions, JitProgram, Op, Program, VerifiedProgram, compile_source_verified,
    run_verified_program,
};

fn accounted(profile: &PreparationProfile) -> Duration {
    let elapsed: Duration = Phase::ALL
        .into_iter()
        .map(|phase| profile.measurement(phase).elapsed())
        .sum();
    assert!(elapsed <= profile.elapsed(), "phase intervals must not overlap");
    elapsed
}

fn assert_empty(profile: &PreparationProfile) {
    assert!(
        Phase::ALL
            .into_iter()
            .all(|phase| profile.measurement(phase).calls() == 0)
    );
    assert_eq!(accounted(profile), Duration::ZERO);
}

fn check_parity(verified: &VerifiedProgram, expected: &str) {
    let mut reference = Vec::new();
    drop(run_verified_program(verified, "vm", &mut reference, Vec::new()).expect("VM reference"));
    assert_eq!(reference, expected.as_bytes());
    let mut compiled = JitProgram::compile_verified(verified).expect("parity compilation");
    let mut output = Vec::new();
    drop(compiled.run(&mut output, Vec::new()).expect("JIT reference"));
    assert_eq!(output, reference);
}

#[test]
fn cold_phase_counts_cover_actual_passes_without_changing_installed_code() {
    for (name, fixture, _) in LARGE_CASES {
        let (verified, expected) = fixture.prepare();
        check_parity(&verified, &expected);
        let control = JitProgram::compile_verified(&verified).expect("unobserved lowering");
        let (compiled, profile) = capture_jit_preparation(|| JitProgram::compile_verified(&verified)).expect("capture");
        let mut compiled = compiled.expect("observed lowering");
        assert_eq!(compiled.stats().compiled_ops, control.stats().compiled_ops, "{name}");
        assert_eq!(compiled.retained_bytes(), control.retained_bytes(), "{name}");
        for phase in Phase::ALL {
            let expected_calls = match phase {
                Phase::UnreachableEncoding => 0,
                Phase::SlotVersionPropagation => u64::from(matches!(fixture, PreparationFixture::CopyStores(_))),
                _ => 1,
            };
            assert_eq!(profile.measurement(phase).calls(), expected_calls, "{name}/{phase:?}");
        }
        accounted(&profile);
        let mut output = Vec::new();
        let (result, warm) = capture_jit_preparation(|| compiled.run(&mut output, Vec::new())).expect("run capture");
        drop(result.expect("observed execution"));
        assert_eq!(output, expected.as_bytes());
        assert_empty(&warm);
    }
}

#[test]
fn unreachable_blocks_are_encoded_without_claiming_optimizer_work() {
    let verified = VerifiedProgram::verify(Program::new(
        vec![
            Instr::new(Op::Jump, 3, 0),
            Instr::new(Op::PushInt, 7, 0),
            Instr::new(Op::Pop, 0, 0),
            Instr::new(Op::PushInt, 42, 0),
            Instr::new(Op::Print, 0, 0),
            Instr::new(Op::Halt, 0, 0),
        ],
        0,
    ))
    .expect("unreachable-block fixture");
    check_parity(&verified, "42\n");
    let (compiled, profile) = capture_jit_preparation(|| JitProgram::compile_verified(&verified)).expect("capture");
    drop(compiled.expect("lowering"));
    assert_eq!(profile.measurement(Phase::ChunkSetup).calls(), 1);
    assert_eq!(profile.measurement(Phase::BranchMap).calls(), 3);
    assert_eq!(profile.measurement(Phase::UnreachableEncoding).calls(), 1);
    assert_eq!(profile.measurement(Phase::DefinitionFinalization).calls(), 2);
    assert_eq!(profile.measurement(Phase::Encoding).calls(), 2);
    accounted(&profile);
}

#[test]
fn lazy_chunks_are_attributed_only_when_first_called() {
    let verified = compile_source_verified("fn twice(x) { return x * 2 } fn unused() { return 99 } print twice(21)")
        .expect("lazy fixture");
    check_parity(&verified, "42\n");
    let (compiled, main) = capture_jit_preparation(|| JitProgram::compile_verified(&verified)).expect("main capture");
    let mut compiled = compiled.expect("main lowering");
    assert_eq!(compiled.stats().compiled_chunks, 1);
    assert_eq!(main.measurement(Phase::ChunkSetup).calls(), 1);
    for expected_lowerings in [1, 0] {
        let mut output = Vec::new();
        let (result, profile) = capture_jit_preparation(|| compiled.run(&mut output, Vec::new())).expect("run capture");
        drop(result.expect("execution"));
        assert_eq!(output, b"42\n");
        assert_eq!(compiled.stats().compiled_chunks, 2, "the unused function stays lazy");
        assert_eq!(profile.measurement(Phase::ChunkSetup).calls(), expected_lowerings);
        assert_eq!(profile.measurement(Phase::OriginInstallation).calls(), expected_lowerings);
        if expected_lowerings == 0 {
            assert_empty(&profile);
        } else {
            accounted(&profile);
        }
    }
}

#[test]
fn observed_lazy_failures_preserve_errors_and_do_not_recompile_warm_chunks() {
    let verified = compile_source_verified("fn divide(x) { return 7 / x } print divide(0)").expect("error fixture");
    let mut reference_output = Vec::new();
    let reference = run_verified_program(&verified, "vm", &mut reference_output, Vec::new()).expect_err("VM failure");
    for threshold in [0, 1, 8] {
        let options = JitOptions::new().with_hot_back_edge_threshold(threshold);
        let mut compiled = JitProgram::compile_verified_with_options(&verified, options).expect("main lowering");
        for expected_lowerings in [1, 0] {
            let mut output = Vec::new();
            let (result, profile) =
                capture_jit_preparation(|| compiled.run(&mut output, Vec::new())).expect("failure capture");
            assert_eq!(result.expect_err("JIT failure").to_string(), reference.to_string());
            assert_eq!(output, reference_output);
            assert_eq!(profile.measurement(Phase::ChunkSetup).calls(), expected_lowerings);
            accounted(&profile);
        }
    }
}

fn standalone(verified: &VerifiedProgram) {
    let compiled = JitProgram::compile_verified(verified).expect("standalone lowering");
    black_box(&compiled);
}

fn cached(verified: &VerifiedProgram) {
    let mut cache = JitCache::new();
    black_box(cache.compile_verified(verified).expect("fresh-cache lowering"));
}

fn batch(verified: &VerifiedProgram, iterations: u64, prepare: &impl Fn(&VerifiedProgram)) {
    for _ in 0..iterations {
        prepare(verified);
    }
}

fn inactive_batch(verified: &VerifiedProgram, iterations: u64, prepare: &impl Fn(&VerifiedProgram)) -> Duration {
    let started = Instant::now();
    batch(verified, iterations, prepare);
    Instant::now()
        .checked_duration_since(started)
        .expect("monotonic inactive clock")
}

fn active_batch(
    verified: &VerifiedProgram,
    iterations: u64,
    prepare: &impl Fn(&VerifiedProgram),
) -> PreparationProfile {
    capture_jit_preparation(|| batch(verified, iterations, prepare))
        .expect("phase capture")
        .1
}

fn profile_route(
    name: &str,
    verified: &VerifiedProgram,
    route: &str,
    iterations: u64,
    prepare: impl Fn(&VerifiedProgram),
) {
    // Prime immutable memoization, TLS initialization and allocator setup.
    // Every measured preparation still constructs and drops fresh code.
    prepare(verified);
    let one = active_batch(verified, 1, &prepare);
    for sample in 1..=7 {
        let (profile, inactive, order) = if sample % 2 == 1 {
            let inactive = inactive_batch(verified, iterations, &prepare);
            (active_batch(verified, iterations, &prepare), inactive, "inactive_first")
        } else {
            let profile = active_batch(verified, iterations, &prepare);
            (profile, inactive_batch(verified, iterations, &prepare), "active_first")
        };
        let accounted = accounted(&profile);
        assert_eq!(profile.measurement(Phase::ChunkSetup).calls(), iterations, "main only");
        eprintln!(
            "jit-phases fixture={name} route={route} sample={sample} iterations={iterations} order={order} \
             bytecode_ops={} functions={} elapsed_ns={} inactive_hooks_ns={} accounted_ns={}",
            verified.program().code().len(),
            verified.program().functions().len(),
            profile.elapsed().as_nanos(),
            inactive.as_nanos(),
            accounted.as_nanos(),
        );
        for phase in Phase::ALL {
            let measurement = profile.measurement(phase);
            assert_eq!(measurement.calls(), one.measurement(phase).calls() * iterations, "stable phase boundaries");
            eprintln!(
                "jit-phase fixture={name} route={route} sample={sample} phase={} calls={} elapsed_ns={}",
                phase.name(),
                measurement.calls(),
                measurement.elapsed().as_nanos(),
            );
        }
    }
}

fn profile_fixture(name: &str, verified: &VerifiedProgram, expected: &str, iterations: u64) {
    check_parity(verified, expected);
    profile_route(name, verified, "standalone", iterations, standalone);
    profile_route(name, verified, "cache", iterations, cached);
}

/// Diagnostic wall-time decomposition of the hooks build, not an ordinary-
/// release speed comparison. Clock checks and changed code generation perturb
/// short phases; report the inactive-hooks interval and clock floor separately.
#[test]
#[ignore = "JIT preparation phase attribution; run alone in release mode with --ignored --nocapture"]
fn report_jit_preparation_phases() {
    for case in SOURCE_CASES {
        let verified = compile_source_verified(case.source).expect("source fixture");
        profile_fixture(case.name, &verified, case.output, 5_000);
    }
    for (name, fixture, iterations) in [
        ("deep_4", PreparationFixture::ExpressionChain(4), 5_000),
        ("deep_1024", PreparationFixture::ExpressionChain(1_024), 128),
    ]
    .into_iter()
    .chain(LARGE_CASES)
    {
        let (verified, expected) = fixture.prepare();
        profile_fixture(name, &verified, &expected, iterations);
    }
}
