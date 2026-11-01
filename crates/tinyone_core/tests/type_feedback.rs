use tinyone::{JitOptions, JitProgram, VerifiedProgram, compile_source_verified, run_verified_program};

fn assert_run_matches_vm(
    verified: &VerifiedProgram,
    compiled: &mut JitProgram,
    inputs: Vec<String>,
) -> Result<(), String> {
    let mut reference_output = Vec::new();
    let reference = run_verified_program(verified, "vm", &mut reference_output, inputs.clone())
        .map(|_| ())
        .map_err(|error| error.to_string());
    let mut output = Vec::new();
    let actual = compiled
        .run(&mut output, inputs)
        .map(|_| ())
        .map_err(|error| error.to_string());
    assert_eq!(actual, reference);
    assert_eq!(output, reference_output);
    actual
}

fn profiled(verified: &VerifiedProgram, threshold: u16) -> JitProgram {
    let options = JitOptions::new()
        .with_hot_back_edge_threshold(threshold)
        .with_execution_profile(true);
    JitProgram::compile_verified_with_options(verified, options).expect("feedback lowering")
}

#[test]
fn recursive_frame_resumes_observe_current_chunk_feedback() {
    let source = "fn probe(depth, value) { \
        let i = 0 let result = value \
        while i < 24 { \
          result = result * 1 \
          if depth > 0 { let ignored = probe(depth - 1, value) } \
          i = i + 1 \
        } return result \
      } print probe(1, 7) print probe(1, 1.5)";
    let verified = compile_source_verified(source).expect("recursive feedback fixture");
    let mut reference = Vec::new();
    run_verified_program(&verified, "vm", &mut reference, Vec::new()).expect("reference execution");
    assert_eq!(reference, b"7\n1.5\n");

    for profiling in [false, true] {
        for threshold in [0, 1, 8] {
            let options = JitOptions::new()
                .with_hot_back_edge_threshold(threshold)
                .with_execution_profile(profiling);
            let mut compiled = JitProgram::compile_verified_with_options(&verified, options).expect("main lowering");
            assert_eq!(compiled.stats().compiled_chunks, 1);
            for _ in 0..2 {
                assert_run_matches_vm(&verified, &mut compiled, Vec::new()).expect("recursive frame resumption");
                assert_eq!(compiled.stats().compiled_chunks, 2);
                assert_eq!(compiled.stats().peak_call_depth, 2);
            }
            assert_eq!(compiled.execution_profile().is_some(), profiling);
            if threshold == 0 {
                assert_eq!(compiled.feedback_bytes(), 0);
            } else {
                assert!(compiled.feedback_bytes() > 0);
                let listing = compiled.listing();
                assert!(!listing.contains("slot.mul.i.hot"));
                assert!(!listing.contains("sample.slot.mul.i"));
                if let Some(profile) = compiled.execution_profile() {
                    assert!(profile.specialization_hits > 0);
                    assert!(profile.specialization_misses >= 4);
                    assert!(profile.specialization_demotions > 0);
                }
            }
        }
    }
}

#[test]
fn stack_scalar_sites_record_misses_and_demote_after_a_runtime_type_change() {
    for operator in ["+", "-", "*", "/", "<", "<=", ">", ">=", "==", "!="] {
        let source = format!(
            "fn identity(value) {{ return value }} \
             fn probe(value) {{ let round = 0 let answer = 0 while round < 24 {{ \
               answer = identity(value) {operator} value round = round + 1 \
             }} return answer }} print probe(4) print probe(fp64(4.0))"
        );
        let verified = compile_source_verified(&source).expect("scalar feedback source");
        for threshold in [0, 1, 8] {
            let mut compiled = profiled(&verified, threshold);
            assert_run_matches_vm(&verified, &mut compiled, Vec::new()).expect("scalar execution");
            let profile = compiled.execution_profile().expect("profile");
            if threshold == 0 {
                assert_eq!(compiled.feedback_bytes(), 0);
                assert_eq!(profile.feedback_samples, 0);
                assert_eq!(profile.specialization_hits, 0);
                assert_eq!(profile.specialization_misses, 0);
            } else {
                assert!(profile.specialization_promotions > 0, "{operator}");
                assert!(profile.specialization_hits > 0, "{operator}");
                assert_eq!(profile.specialization_misses, 4, "{operator}");
                assert_eq!(profile.specialization_demotions, 1, "{operator}");
                let misses = profile.specialization_misses;
                assert_run_matches_vm(&verified, &mut compiled, Vec::new()).expect("repeated scalar execution");
                assert_eq!(compiled.execution_profile().expect("profile").specialization_misses, misses);
            }
        }
    }
}

#[test]
fn encoded_slot_fallbacks_use_fresh_values_and_clones_do_not_share_adaptive_state() {
    let verified = compile_source_verified(
        "let selection = read_int() let value = 7 \
         if selection == 1 { value = fp64(7.0) } \
         if selection == 2 { value = u8(7) } \
         if selection == 3 { value = true } \
         let round = 0 while round < 32 { value = value * 1 value = value / 1 round = round + 1 } print value",
    )
    .expect("slot feedback source");
    let mut original = profiled(&verified, 1);
    assert_run_matches_vm(&verified, &mut original, vec!["0".to_owned()]).expect("I64 warm-up");
    assert!(original.listing().contains("slot.mul.i.hot"));
    assert!(original.listing().contains("slot.div.i.hot"));
    for selection in ["1", "2", "3"] {
        let mut cloned = original.clone();
        assert_eq!(cloned.feedback_bytes(), original.feedback_bytes());
        cloned.reset_execution_profile();
        let result = assert_run_matches_vm(&verified, &mut cloned, vec![selection.to_owned()]);
        if selection == "3" {
            assert!(result.is_err(), "Bool arithmetic must fail after the guard miss");
        } else {
            result.expect("numeric fallback execution");
        }
        assert!(cloned.execution_profile().expect("profile").specialization_misses > 0);
        assert_eq!(original.execution_profile().expect("profile").specialization_misses, 0);
        assert!(original.listing().contains("slot.mul.i.hot"));
        assert!(original.listing().contains("slot.div.i.hot"));
        assert_run_matches_vm(&verified, &mut cloned, vec!["0".to_owned()]).expect("fresh execution after fallback");
    }
}

#[test]
fn typed_branch_and_slot_push_sites_fall_back_without_losing_stack_or_mutation_order() {
    for expression in ["value * 2", "value / 2"] {
        let source = format!(
            "fn probe(value) {{ let round = 0 let total = 0 while round < 24 {{ \
               if value {{ if value < 16 {{ total = total + ({expression}) }} }} \
               round = round + 1 \
             }} return total }} print probe(8) print probe(fp64(8.0)) print probe(0)"
        );
        let verified = compile_source_verified(&source).expect("branch feedback source");
        for threshold in [0, 1, 8] {
            let mut compiled = profiled(&verified, threshold);
            assert_run_matches_vm(&verified, &mut compiled, Vec::new()).expect("branch execution");
            if threshold != 0 {
                let profile = compiled.execution_profile().expect("profile");
                assert!(profile.specialization_misses >= 12, "{expression}: {profile:?}");
                assert!(profile.opcodes.contains_key("generic.slot.jz"));
                assert!(profile.opcodes.contains_key("generic.slot.cmp.i.jz"));
                let push = if expression.contains('*') {
                    "push.slot.mul.i.hot"
                } else {
                    "push.slot.div.i.hot"
                };
                assert!(profile.opcodes.contains_key(push), "{expression}: {profile:?}");
            }
        }
    }
}
