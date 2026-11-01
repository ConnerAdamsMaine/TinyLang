use tinyone::{JitOptions, JitProgram, VerifiedProgram, compile_source_verified, run_verified_program};

fn run_matches_vm(verified: &VerifiedProgram, compiled: &mut JitProgram, input: i64) -> Result<(), String> {
    let inputs = vec![input.to_string()];
    let mut reference_output = Vec::new();
    let reference = run_verified_program(verified, "vm", &mut reference_output, inputs.clone())
        .map(|memory| memory.snapshot())
        .map_err(|error| error.to_string());
    let mut output = Vec::new();
    let actual = compiled
        .run(&mut output, inputs)
        .map(|memory| memory.snapshot())
        .map_err(|error| error.to_string());
    assert_eq!(output, reference_output);
    assert_eq!(actual, reference);
    actual.map(|_| ())
}

fn profiled(verified: &VerifiedProgram, threshold: u16) -> JitProgram {
    JitProgram::compile_verified_with_options(
        verified,
        JitOptions::new()
            .with_hot_back_edge_threshold(threshold)
            .with_call_feedback(true)
            .with_execution_profile(true),
    )
    .expect("call-feedback lowering")
}

#[test]
fn non_loop_sites_specialize_then_fall_back_to_a_different_frame_layout() {
    let verified = compile_source_verified(
        "fn first() { return 7 } fn second() { let a = 11 let b = a * 2 return a + b }
         fn invoke(callback) { return callback() }
         let choice = read_int() let callback = first
         if choice == 1 { callback = second } print invoke(callback)",
    )
    .expect("changing call source");
    for threshold in [0, 1, 8] {
        let mut compiled = profiled(&verified, threshold);
        assert_eq!(compiled.stats().compiled_chunks, 1);
        for _ in 0..10 {
            run_matches_vm(&verified, &mut compiled, 0).expect("stable call");
        }
        assert_eq!(compiled.stats().compiled_chunks, 3, "the uncalled function remains lazy");
        assert_eq!(compiled.feedback_bytes(), 0, "inline call feedback needs no side table");
        let profile = compiled.execution_profile().expect("profile");
        if threshold == 0 {
            assert_eq!(profile.call_feedback_samples, 0);
            assert_eq!(profile.call_specializations, 0);
        } else {
            assert_eq!(profile.call_feedback_samples, 8);
            assert_eq!(profile.call_specializations, 1);
            assert_eq!(profile.call_guard_hits, 2);
        }
        for _ in 0..4 {
            run_matches_vm(&verified, &mut compiled, 1).expect("new callee layout");
        }
        assert_eq!(compiled.stats().compiled_chunks, 4);
        for _ in 0..40 {
            run_matches_vm(&verified, &mut compiled, 0).expect("generic remains correct");
        }
        let profile = compiled.execution_profile().expect("profile");
        if threshold != 0 {
            assert_eq!(profile.call_guard_misses, 4);
            assert_eq!(profile.call_deoptimizations, 1);
            assert_eq!(profile.call_feedback_samples, 8, "demoted calls must not rearm");
            assert_eq!(profile.call_guard_hits, 2);
            assert!(compiled.listing().contains("call.value args="));
            assert!(!compiled.listing().contains("call.value.hot"));
        }
    }
}

#[test]
fn cloned_guards_preserve_the_consecutive_recovery_boundary() {
    let verified = compile_source_verified(
        "fn first() { return 7 } fn second() { return 9 }
         let callback = first if read_int() == 1 { callback = second } print callback()",
    )
    .expect("guard recovery source");
    for threshold in [1, 8] {
        let mut compiled = profiled(&verified, threshold);
        for _ in 0..8 {
            run_matches_vm(&verified, &mut compiled, 0).expect("stable promotion");
        }
        for _ in 0..3 {
            run_matches_vm(&verified, &mut compiled, 1).expect("retained guard miss");
        }
        let mut interrupted = compiled.clone();
        interrupted.reset_execution_profile();
        compiled.reset_execution_profile();
        for _ in 0..31 {
            run_matches_vm(&verified, &mut interrupted, 0).expect("incomplete recovery");
        }
        run_matches_vm(&verified, &mut interrupted, 1).expect("fourth miss");
        let profile = interrupted.execution_profile().expect("clone profile");
        assert_eq!(profile.call_guard_hits, 31);
        assert_eq!(profile.call_guard_misses, 1);
        assert_eq!(profile.call_deoptimizations, 1);
        assert_eq!(profile.call_feedback_samples, 0);
        assert!(!interrupted.listing().contains("call.value.hot"));

        for _ in 0..32 {
            run_matches_vm(&verified, &mut compiled, 0).expect("complete recovery");
        }
        run_matches_vm(&verified, &mut compiled, 1).expect("new miss budget");
        let profile = compiled.execution_profile().expect("original profile");
        assert_eq!(profile.call_guard_hits, 32);
        assert_eq!(profile.call_guard_misses, 1);
        assert_eq!(profile.call_deoptimizations, 0);
        assert_eq!(profile.call_feedback_samples, 0);
        assert!(compiled.listing().contains("call.value.hot"));
        for _ in 0..3 {
            run_matches_vm(&verified, &mut compiled, 1).expect("exhaust new miss budget");
        }
        assert_eq!(compiled.execution_profile().expect("profile").call_deoptimizations, 1);
        assert!(!compiled.listing().contains("call.value.hot"));
    }
}

#[test]
fn cloned_guards_use_fresh_captures_and_do_not_require_detailed_profiling() {
    let verified = compile_source_verified(
        r#"fn add(base, value) { return base + value }
           fn invoke(callback) { return callback(1) }
           let base = read_int() let callback = closure_new("add", [base]) print invoke(callback)"#,
    )
    .expect("capture source");
    for profiling in [false, true] {
        let mut compiled = JitProgram::compile_verified_with_options(
            &verified,
            JitOptions::new()
                .with_call_feedback(true)
                .with_execution_profile(profiling),
        )
        .expect("capture lowering");
        for input in 0..40 {
            run_matches_vm(&verified, &mut compiled, input).expect("fresh captured value");
        }
        assert!(compiled.listing().contains("call.value.hot"));
        assert_eq!(compiled.execution_profile().is_some(), profiling);
        let mut cloned = compiled.clone();
        cloned.reset_execution_profile();
        run_matches_vm(&verified, &mut cloned, 12345).expect("clone with fresh capture");
        run_matches_vm(&verified, &mut compiled, 98765).expect("original with fresh capture");
        if let Some(profile) = cloned.execution_profile() {
            assert_eq!(profile.call_guard_hits, 1);
            assert_eq!(profile.call_feedback_samples, 0);
        }
    }
}

#[test]
fn guarded_closures_preserve_arity_kind_and_generation_failures() {
    let verified = compile_source_verified(
        r#"fn pair(a, b) { return a + b } fn invoke(callback) { return callback(1) }
           let choice = read_int() let captures = [7]
           if choice == 1 { captures = [] }
           if choice == 2 { captures = [7, 8] }
           let callback = closure_new("pair", captures)
           if choice == 3 { let ignored = unsafe free(callback) }
           if choice == 4 { callback = [3] }
           print invoke(callback)"#,
    )
    .expect("guard failure source");
    let mut compiled = profiled(&verified, 8);
    for _ in 0..8 {
        run_matches_vm(&verified, &mut compiled, 0).expect("warm closure identity");
    }
    assert_eq!(compiled.execution_profile().expect("profile").call_specializations, 1);
    for input in 1..=4 {
        assert!(run_matches_vm(&verified, &mut compiled, input).is_err());
    }
    let profile = compiled.execution_profile().expect("profile");
    assert_eq!(profile.call_guard_misses, 4);
    assert_eq!(profile.call_deoptimizations, 1);
    run_matches_vm(&verified, &mut compiled, 0).expect("fresh successful execution after failures");
    assert!(compiled.listing().contains("call.value args="));
    assert!(!compiled.listing().contains("call.value.hot"));
}

#[test]
fn guarded_recursive_calls_preserve_depth_failure_and_recovery() {
    let verified = compile_source_verified(
        "fn dive(depth) { if depth == 0 { return 7 } let callback = dive return callback(depth - 1) }
         let callback = dive print callback(read_int())",
    )
    .expect("recursive dynamic call source");
    for threshold in [0, 1, 8] {
        let mut compiled = profiled(&verified, threshold);
        for _ in 0..8 {
            run_matches_vm(&verified, &mut compiled, 2).expect("warm shallow recursion");
        }
        assert_eq!(
            compiled.execution_profile().expect("profile").call_specializations,
            if threshold == 0 { 0 } else { 2 },
        );
        compiled.reset_execution_profile();
        let error = run_matches_vm(&verified, &mut compiled, 16).expect_err("logical depth limit");
        assert!(error.contains("Call stack overflow after 16 nested call(s)"), "{error}");
        let profile = compiled.execution_profile().expect("profile");
        assert_eq!(profile.call_guard_hits > 0, threshold != 0);
        assert_eq!(profile.call_guard_misses, 0);
        run_matches_vm(&verified, &mut compiled, 0).expect("fresh run after depth failure");
    }
}

#[test]
fn unstable_identities_stop_sampling_without_back_edge_rearming() {
    let verified = compile_source_verified(
        "fn one() { return 1 } fn two() { return 2 } fn three() { return 3 }
         let i = 0 let selector = 0 let total = 0 while i < 96 {
           let callback = one if selector == 1 { callback = two } if selector == 2 { callback = three }
           total = total + callback() selector = selector + 1 if selector == 3 { selector = 0 } i = i + 1
         } print total",
    )
    .expect("unstable source");
    let mut compiled = profiled(&verified, 8);
    for _ in 0..3 {
        run_matches_vm(&verified, &mut compiled, 0).expect("unstable calls");
        let profile = compiled.execution_profile().expect("profile");
        assert_eq!(profile.call_feedback_samples, 32);
        assert_eq!(profile.call_specializations, 0);
        assert_eq!(profile.call_guard_hits, 0);
        assert_eq!(profile.call_guard_misses, 0);
    }
    assert!(compiled.listing().contains("call.value args="));
    assert!(!compiled.listing().contains("sample.call.value"));
}

#[test]
fn ordinary_calls_do_not_sample_and_scalar_quickening_stays_independent() {
    let verified = compile_source_verified(
        "fn target(value) { return value * 2 }
         fn invoke(callback, value) { return callback(value) }
         let callback = invoke let i = 0 let total = 0
         while i < 96 { total = total + callback(target, i) i = i + 1 } print total",
    )
    .expect("independent feedback source");
    for options in [
        JitOptions::new(),
        JitOptions::new()
            .with_call_feedback(false)
            .with_hot_back_edge_threshold(1),
        JitOptions::new()
            .with_call_feedback(true)
            .with_hot_back_edge_threshold(0),
    ] {
        assert!(!options.call_feedback_enabled());
        let mut compiled = JitProgram::compile_verified_with_options(&verified, options.with_execution_profile(true))
            .expect("ordinary call lowering");
        assert_eq!(compiled.stats().compiled_chunks, 1);
        assert!(!compiled.listing().contains("call.value.unseen"));
        for _ in 0..3 {
            run_matches_vm(&verified, &mut compiled, 0).expect("ordinary calls");
        }
        assert_eq!(compiled.stats().compiled_chunks, 3);
        let profile = compiled.execution_profile().expect("profile");
        assert_eq!(profile.call_feedback_samples, 0);
        assert_eq!(profile.call_specializations, 0);
        assert_eq!(profile.call_guard_hits, 0);
        assert_eq!(profile.call_guard_misses, 0);
        assert_eq!(profile.specialization_promotions > 0, options.quickening_enabled());
        let listing = compiled.listing();
        assert!(listing.contains("call.value args="));
        assert!(!listing.contains("call.value.unseen"));
        assert!(!listing.contains("sample.call.value"));
        assert!(!listing.contains("call.value.hot"));
    }
}
