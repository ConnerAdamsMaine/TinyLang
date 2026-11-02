use tinylang::{
    Function,
    Instr,
    JitCache,
    JitOptions,
    JitProgram,
    Op,
    Program,
    VerifiedProgram,
    compile_source_verified,
    run_verified_program,
};

#[test]
fn reused_function_frames_reset_unwritten_slots_and_do_not_survive_execution() {
    let program = Program::new(
        vec![
            Instr::new(Op::PushInt, 1, 0),
            Instr::new(Op::Call, 0, 1),
            Instr::new(Op::Print, 0, 0),
            Instr::new(Op::PushInt, 0, 0),
            Instr::new(Op::Call, 0, 1),
            Instr::new(Op::Print, 0, 0),
            Instr::new(Op::Halt, 0, 0),
        ],
        0,
    )
    .with_functions(vec![Function::new(
        "choose",
        1,
        vec![
            Instr::new(Op::Load, 0, 0),
            Instr::new(Op::JumpIfZero, 4, 0),
            Instr::new(Op::PushInt, 99, 0),
            Instr::new(Op::Store, 1, 0),
            Instr::new(Op::Load, 1, 0),
            Instr::new(Op::Return, 0, 0),
        ],
        2,
    )]);
    let verified = VerifiedProgram::verify(program).expect("valid frame reset program");
    let mut reference = Vec::new();
    run_verified_program(&verified, "vm", &mut reference, Vec::new()).expect("VM reference");
    assert_eq!(reference, b"99\n0\n");
    for threshold in [0, 1, 8] {
        let mut compiled = JitProgram::compile_verified_with_options(
            &verified,
            JitOptions::new().with_hot_back_edge_threshold(threshold),
        )
        .expect("compile frame reset test");
        for run in 1..=3 {
            let mut output = Vec::new();
            compiled.run(&mut output, Vec::new()).expect("JIT frame reuse");
            assert_eq!(output, reference);
            let stats = compiled.stats();
            assert_eq!(stats.frame_memory_allocations, run, "one fresh frame per execution");
            assert_eq!(stats.frame_memory_reuses, run, "one same-run reuse");
            assert_eq!(stats.peak_call_depth, 1);
        }
    }
}

#[test]
fn cached_artifact_authority_matches_vm_in_both_insertion_orders() {
    let sources = [
        r#"print sys_env_has("TINYLANG_CACHE_TEST")"#,
        r#"fn check() { return sys_env_has("TINYLANG_CACHE_TEST") } print check()"#,
        "let cell = alloc(7) unsafe free(cell) print 42",
        "fn release() { let cell = alloc(7) unsafe free(cell) return 42 } print release()",
    ];

    for source in sources {
        let trusted = compile_source_verified(source).expect("compile trusted source");
        let untrusted_copies = [
            VerifiedProgram::from_artifact(trusted.program().to_artifact()).expect("decode JSON"),
            VerifiedProgram::from_binary_artifact(&trusted.program().to_binary_artifact().expect("encode binary"))
                .expect("decode binary"),
        ];

        for untrusted in untrusted_copies {
            assert_eq!(trusted.fingerprint(), untrusted.fingerprint(), "artifact content is unchanged");
            let mut reference_output = Vec::new();
            let expected_error = run_verified_program(&untrusted, "vm", &mut reference_output, Vec::new())
                .expect_err("untrusted policy grants no authority")
                .to_string();
            let mut trusted_output = Vec::new();
            run_verified_program(&trusted, "vm", &mut trusted_output, Vec::new()).expect("trusted VM run");

            for threshold in [0, 1, 8] {
                for trusted_first in [true, false] {
                    let mut cache = JitCache::with_options(JitOptions::new().with_hot_back_edge_threshold(threshold));
                    let order = if trusted_first {
                        [&trusted, &untrusted, &trusted, &untrusted]
                    } else {
                        [&untrusted, &trusted, &untrusted, &trusted]
                    };
                    for program in order {
                        let mut output = Vec::new();
                        let result = cache.run_verified_program(program, &mut output, Vec::new());
                        if std::ptr::eq(program, &trusted) {
                            result.expect("a restricted cache entry must not remove trusted authority");
                            assert_eq!(output, trusted_output);
                        } else {
                            let error = result.expect_err("a trusted cache entry must not grant artifact authority");
                            assert_eq!(error.to_string(), expected_error);
                            assert_eq!(output, reference_output);
                        }
                    }
                    assert_eq!(cache.len(), 2, "distinct execution authority needs distinct cache entries");
                }
            }
        }
    }
}

#[test]
#[cfg(target_pointer_width = "64")]
fn unreachable_wide_arities_are_preserved_by_jit_installation() {
    for arity in [i64::from(u32::MAX) + 1, i64::MAX] {
        let verified = VerifiedProgram::verify(Program::new(
            vec![
                Instr::new(Op::Jump, 3, 0),
                Instr::new(Op::MakeArray, arity, 0),
                Instr::new(Op::CallValue, arity, 0),
                Instr::new(Op::Halt, 0, 0),
            ],
            0,
        ))
        .expect("unreachable arities are statically valid on a 64-bit host");
        let mut reference = Vec::new();
        run_verified_program(&verified, "vm", &mut reference, Vec::new()).expect("VM reference");
        assert!(reference.is_empty());
        for threshold in [0, 1, 8] {
            let mut compiled = JitProgram::compile_verified_with_options(
                &verified,
                JitOptions::new().with_hot_back_edge_threshold(threshold),
            )
            .expect("lowering must not narrow verifier-valid operands");
            let listing = compiled.listing();
            assert!(listing.contains(&format!("array {arity}\n")), "{listing}");
            assert!(listing.contains(&format!("call.value args={arity}\n")), "{listing}");
            for _ in 0..2 {
                let mut output = Vec::new();
                compiled
                    .run(&mut output, Vec::new())
                    .expect("wide unreachable operands must not execute");
                assert_eq!(output, reference);
            }
        }
    }
}

#[test]
fn maximum_slots_and_branch_targets_beyond_u16_match_all_tiers() {
    const LAST_SLOT: usize = 65_535;
    for target in [65_535usize, 65_536, 262_140] {
        for comparison in [7, 8] {
            let mut code = vec![Instr::new(Op::Halt, 0, 0); target + 3];
            let target_operand = i64::try_from(target).expect("bounded test target");
            code[..9].copy_from_slice(&[
                Instr::new(Op::PushInt, 7, 0),
                Instr::new(Op::Store, LAST_SLOT as i64, 0),
                Instr::new(Op::Load, LAST_SLOT as i64, 0),
                Instr::new(Op::PushInt, comparison, 0),
                Instr::new(Op::Eq, 0, 0),
                Instr::new(Op::JumpIfZero, target_operand, 0),
                Instr::new(Op::PushInt, 999, 0),
                Instr::new(Op::Print, 0, 0),
                Instr::new(Op::Jump, target_operand, 0),
            ]);
            code[target] = Instr::new(Op::Load, LAST_SLOT as i64, 0);
            code[target + 1] = Instr::new(Op::Print, 0, 0);
            let program = VerifiedProgram::verify(Program::new(code, LAST_SLOT + 1)).expect("valid large chunk");
            let mut reference_output = Vec::new();
            run_verified_program(&program, "vm", &mut reference_output, Vec::new()).expect("VM boundary run");
            assert_eq!(reference_output, if comparison == 7 { &b"999\n7\n"[..] } else { &b"7\n"[..] });

            for threshold in [0, 1, 8] {
                let mut cache = JitCache::with_options(JitOptions::new().with_hot_back_edge_threshold(threshold));
                for _ in 0..2 {
                    let mut output = Vec::new();
                    let memory = cache
                        .run_verified_program(&program, &mut output, Vec::new())
                        .expect("JIT preserves every verifier-valid index");
                    assert_eq!(output, reference_output);
                    assert_eq!(memory.load(LAST_SLOT).expect("maximum slot"), tinylang::RuntimeValue::I64(7));
                }
            }
        }
    }
}

#[test]
fn nonempty_operand_stacks_merge_without_losing_values_in_any_tier() {
    for condition in [0, 1] {
        let program = VerifiedProgram::verify(Program::new(
            vec![
                Instr::new(Op::PushInt, 42, 0),
                Instr::new(Op::PushInt, condition, 0),
                Instr::new(Op::JumpIfZero, 6, 0),
                Instr::new(Op::PushInt, 3, 0),
                Instr::new(Op::Jump, 7, 0),
                Instr::new(Op::Halt, 0, 0),
                Instr::new(Op::PushInt, 5, 0),
                Instr::new(Op::Add, 0, 0),
                Instr::new(Op::Print, 0, 0),
                Instr::new(Op::Halt, 0, 0),
            ],
            0,
        ))
        .expect("both predecessors enter ADD with two values");
        let expected = if condition == 0 { b"47\n" } else { b"45\n" };
        let mut output = Vec::new();
        run_verified_program(&program, "vm", &mut output, Vec::new()).expect("VM stack merge");
        assert_eq!(output, expected);
        for threshold in [0, 1, 8] {
            let mut cache = JitCache::with_options(JitOptions::new().with_hot_back_edge_threshold(threshold));
            for _ in 0..2 {
                output.clear();
                cache
                    .run_verified_program(&program, &mut output, Vec::new())
                    .expect("JIT stack merge");
                assert_eq!(output, expected);
            }
        }
    }
}
