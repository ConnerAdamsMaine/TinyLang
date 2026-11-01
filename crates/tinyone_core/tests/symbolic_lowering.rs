use tinyone::{
    Instr,
    JitOptions,
    JitProgram,
    Op,
    Program,
    RuntimeValue,
    TinyMemory,
    VerifiedProgram,
    compile_source_verified,
    run_verified_program,
};

/// Compare successful memory as well as output and exact runtime errors.
/// Float bit patterns also preserve signed zero and NaN payloads, whose
/// ordinary PartialEq deliberately cannot match themselves.
fn assert_parity(program: Program) -> String {
    let verified = VerifiedProgram::verify(program).expect("verified symbolic fixture");
    assert_verified_parity(&verified)
}

fn assert_verified_parity(verified: &VerifiedProgram) -> String {
    let mut reference_output = Vec::new();
    let reference = run_verified_program(verified, "vm", &mut reference_output, Vec::new())
        .map(|memory| scalar_memory_signature(&memory))
        .map_err(|error| error.to_string());
    let mut listing = String::new();
    for threshold in [0, 1, 8] {
        let mut compiled = JitProgram::compile_verified_with_options(
            verified,
            JitOptions::new().with_hot_back_edge_threshold(threshold),
        )
        .expect("symbolic lowering");
        for _ in 0..2 {
            let mut output = Vec::new();
            let result = compiled
                .run(&mut output, Vec::new())
                .map(|memory| scalar_memory_signature(&memory))
                .map_err(|error| error.to_string());
            assert_eq!(result, reference, "threshold {threshold}");
            assert_eq!(output, reference_output, "threshold {threshold}");
        }
        listing = compiled.listing();
    }
    listing
}

fn scalar_memory_signature(memory: &TinyMemory) -> Vec<String> {
    memory
        .snapshot()
        .into_iter()
        .map(|value| {
            match value {
                RuntimeValue::Float { kind, bits } => format!("Float({kind:?}, {:016x})", bits.to_bits()),
                value => format!("{value:?}"),
            }
        })
        .collect()
}

#[test]
fn direct_scalar_store_families_preserve_numeric_results_and_errors() {
    let pairs = [
        (Instr::new(Op::PushInt, 7, 0), Instr::new(Op::PushInt, -3, 0)),
        (Instr::new(Op::PushInt, i64::MAX, 0), Instr::new(Op::PushInt, 1, 0)),
        (Instr::new(Op::PushInt, i64::MIN, 0), Instr::new(Op::PushInt, -1, 0)),
        (Instr::new(Op::PushInt, 7, 0), Instr::new(Op::PushInt, 0, 0)),
        (Instr::new(Op::PushFloat, (-0.0f64).to_bits() as i64, 0), Instr::new(Op::PushInt, 1, 0)),
        (Instr::new(Op::PushFloat, 7.5f64.to_bits() as i64, 0), Instr::new(Op::PushInt, -3, 0)),
        (Instr::new(Op::PushFloat, f64::NAN.to_bits() as i64, 0), Instr::new(Op::PushInt, 1, 0)),
        (Instr::new(Op::PushNull, 0, 0), Instr::new(Op::PushInt, 1, 0)),
        (Instr::new(Op::PushBool, 1, 0), Instr::new(Op::PushInt, 1, 0)),
    ];
    for mode in [
        Op::Add,
        Op::Sub,
        Op::Mul,
        Op::Div,
        Op::Lt,
        Op::Lte,
        Op::Gt,
        Op::Gte,
        Op::Eq,
        Op::Ne,
    ] {
        for (lhs, rhs) in pairs {
            let code = vec![
                lhs,
                Instr::new(Op::Store, 0, 0),
                rhs,
                Instr::new(Op::Store, 1, 0),
                Instr::new(Op::PushInt, 99, 0),
                Instr::new(Op::Print, 0, 0),
                Instr::new(Op::Load, 0, 0),
                Instr::new(Op::Load, 1, 0),
                Instr::new(mode, 0, 0),
                Instr::new(Op::Store, 2, 0),
                Instr::new(Op::Load, 2, 0),
                Instr::new(Op::Print, 0, 0),
                Instr::new(Op::Halt, 0, 0),
            ];
            let listing = assert_parity(Program::new(code.clone(), 3));
            assert!(listing.contains(".slots.store "), "{listing}");
            if rhs.op == Op::PushInt {
                let mut immediate = code;
                immediate[7] = rhs;
                let listing = assert_parity(Program::new(immediate, 3));
                assert!(listing.contains(".slot.i.store "), "{listing}");
            }
        }
    }
}

#[test]
fn float_immediate_stores_preserve_numeric_results_errors_and_destination_aliases() {
    let sources = [
        Instr::new(Op::PushInt, -7, 0),
        Instr::new(Op::PushInt, i64::MAX, 0),
        Instr::new(Op::PushInt, i64::MIN, 0),
        Instr::new(Op::PushFloat, (-0.0f64).to_bits() as i64, 0),
        Instr::new(Op::PushFloat, 7.5f64.to_bits() as i64, 0),
        Instr::new(Op::PushFloat, 0x7ff8_0000_0000_0042, 0),
        Instr::new(Op::PushNull, 0, 0),
        Instr::new(Op::PushBool, 1, 0),
    ];
    let literals = [
        (-3.25f64).to_bits(),
        0.0f64.to_bits(),
        (-0.0f64).to_bits(),
        f64::INFINITY.to_bits(),
        f64::NEG_INFINITY.to_bits(),
        f64::MAX.to_bits(),
        1,                     // smallest positive subnormal
        0x7ff8_0000_0000_0084, // quiet NaN payload
        0x7ff0_0000_0000_0042, // signaling NaN payload
    ];
    for mode in [
        Op::Add,
        Op::Sub,
        Op::Mul,
        Op::Div,
        Op::Lt,
        Op::Lte,
        Op::Gt,
        Op::Gte,
        Op::Eq,
        Op::Ne,
    ] {
        for source in sources {
            for bits in literals {
                for destination in [0, 1] {
                    let code = vec![
                        source,
                        Instr::new(Op::Store, 0, 0),
                        Instr::new(Op::PushInt, 99, 0),
                        Instr::new(Op::Print, 0, 0), // invalidate constant facts
                        Instr::new(Op::Load, 0, 0),
                        Instr::new(Op::PushFloat, bits as i64, 0),
                        Instr::new(mode, 0, 0),
                        Instr::new(Op::Store, destination, 0),
                        Instr::new(Op::Load, destination, 0),
                        Instr::new(Op::Print, 0, 0),
                        Instr::new(Op::Halt, 0, 0),
                    ];
                    let listing = assert_parity(Program::new(code, 2));
                    assert!(listing.contains(".slot.f.store "), "{listing}");
                }
            }
        }
    }
}

#[test]
fn float_immediate_stores_preserve_narrow_kinds_and_changing_slot_types() {
    for kind in [
        "i8", "i16", "i32", "i64", "u8", "u16", "u32", "u64", "fp8", "fp16", "fp32", "fp64",
    ] {
        let initializer = match kind {
            "fp8" | "fp16" | "fp32" | "fp64" => format!("{kind}(7)"),
            _ => format!("check_int_range(7, \"{kind}\")"),
        };
        for operation in ["+", "-", "*", "/", "<", "<=", ">", ">=", "==", "!="] {
            let verified = compile_source_verified(&format!(
                "let a = {initializer} let saved = a a = 99 let result = saved {operation} 3.5 print result"
            ))
            .expect("float immediate promotion fixture");
            let listing = assert_verified_parity(&verified);
            assert!(listing.contains(".slot.f.store "), "{listing}");
        }
    }
    for final_value in ["fp32(2)", "true", "null"] {
        let verified = compile_source_verified(&format!(
            "let i = 0 let value = 1 let total = 0 while i < 96 {{
                if i < 32 {{ value = check_int_range(1, \"i8\") }} else {{
                    if i < 64 {{ value = u16(2) }} else {{ value = {final_value} }}
                }}
                total = value + 0.5 i = i + 1
            }} print total"
        ))
        .expect("changing float-store operand fixture");
        let listing = assert_verified_parity(&verified);
        assert!(listing.contains("binary.add.slot.f.store"), "{listing}");
    }
}

#[test]
fn float_immediate_stores_respect_block_entries_and_live_stack_prefixes() {
    for boundary in 0..=3 {
        let mut code = vec![Instr::new(Op::PushInt, 99, 0), Instr::new(Op::Jump, 2, 0)];
        let expression = [
            Instr::new(Op::Load, 0, 0),
            Instr::new(Op::PushFloat, 0.5f64.to_bits() as i64, 0),
            Instr::new(Op::Add, 0, 0),
            Instr::new(Op::Store, 1, 0),
        ];
        for (index, instruction) in expression.into_iter().enumerate() {
            if index == boundary {
                code.push(Instr::new(Op::Jump, (code.len() + 1) as i64, 0));
            }
            code.push(instruction);
        }
        code.extend([
            Instr::new(Op::Print, 0, 0), // the prefix survives the store
            Instr::new(Op::Load, 1, 0),
            Instr::new(Op::Print, 0, 0),
            Instr::new(Op::Halt, 0, 0),
        ]);
        let listing = assert_parity(Program::new(code, 2));
        assert_eq!(listing.contains(".slot.f.store "), boundary == 0, "{listing}");
    }

    // A pending source read must snapshot before an intervening overwrite.
    let code = vec![
        Instr::new(Op::Load, 0, 0),
        Instr::new(Op::PushInt, 99, 0),
        Instr::new(Op::Store, 0, 0),
        Instr::new(Op::PushFloat, 0.5f64.to_bits() as i64, 0),
        Instr::new(Op::Add, 0, 0),
        Instr::new(Op::Store, 1, 0),
        Instr::new(Op::Halt, 0, 0),
    ];
    let listing = assert_parity(Program::new(code, 2));
    assert!(!listing.contains(".slot.f.store "), "stale read fused across write: {listing}");
}

#[test]
fn float_immediate_stores_preserve_the_maximum_slot_index() {
    for (source, destination) in [(0, 65_535), (65_535, 0), (65_535, 65_535)] {
        let code = vec![
            Instr::new(Op::Load, source, 0),
            Instr::new(Op::PushFloat, 0.5f64.to_bits() as i64, 0),
            Instr::new(Op::Add, 0, 0),
            Instr::new(Op::Store, destination, 0),
            Instr::new(Op::Halt, 0, 0),
        ];
        let listing = assert_parity(Program::new(code, 65_536));
        assert!(listing.contains(&format!("binary.add.slot.f.store {source} 0.5 {destination}")), "{listing}");
    }
}

#[test]
fn symbolic_values_survive_nonadjacent_producers_and_local_overwrites() {
    let code = vec![
        Instr::new(Op::PushInt, 7, 0),
        Instr::new(Op::Store, 0, 0),
        Instr::new(Op::Load, 0, 0),
        Instr::new(Op::PushInt, 99, 0),
        Instr::new(Op::Store, 0, 0),
        Instr::new(Op::Print, 0, 0),
        Instr::new(Op::Load, 0, 0),
        Instr::new(Op::PushBool, 1, 0),
        Instr::new(Op::Pop, 0, 0),
        Instr::new(Op::Load, 1, 0),
        Instr::new(Op::Add, 0, 0),
        Instr::new(Op::Store, 2, 0),
        Instr::new(Op::Load, 2, 0),
        Instr::new(Op::Print, 0, 0),
        Instr::new(Op::Halt, 0, 0),
    ];
    let listing = assert_parity(Program::new(code, 3));
    assert!(listing.contains("binary.add.slots.store 0 1 2"), "{listing}");
    assert!(!listing.contains("push.bool"), "unused non-failing temporary is eliminated");
}

#[test]
fn direct_store_i64_guards_fall_back_for_narrow_unsigned_and_float_kinds() {
    for kind in ["u8", "u16", "u32", "fp8", "fp32"] {
        for operation in ["+", "-", "*", "/"] {
            let program = compile_source_verified(&format!(
                "let a = {kind}(7) let b = {kind}(3) let result = a {operation} b print result"
            ))
            .expect("typed scalar fixture");
            let listing = assert_verified_parity(&program);
            assert!(listing.contains(".slots.store "), "{listing}");
        }
    }
}

#[test]
fn discarded_expressions_keep_failure_order_before_writes_and_output() {
    for effect in [Op::Pop, Op::Store, Op::Print] {
        let mut code = vec![
            Instr::new(Op::PushInt, 1, 0),
            Instr::new(Op::PushInt, 0, 0),
            Instr::new(Op::Div, 0, 0),
            Instr::new(Op::PushInt, i64::MAX, 0),
            Instr::new(Op::PushInt, 1, 0),
            Instr::new(Op::Add, 0, 0),
            Instr::new(effect, 0, 0),
            Instr::new(Op::Pop, 0, 0),
            Instr::new(Op::Halt, 0, 0),
        ];
        assert_parity(Program::new(code.clone(), 1));
        // With the earlier expression non-failing, the discarded addition
        // must still report its overflow instead of becoming a dead value.
        code[1].arg = 1;
        assert_parity(Program::new(code, 1));
    }
}

#[test]
fn deep_symbolic_expressions_do_not_recurse_on_the_host_stack() {
    let mut code = Vec::with_capacity(131_075);
    code.push(Instr::new(Op::PushInt, 0, 0));
    for _ in 0..65_536 {
        code.push(Instr::new(Op::Load, 0, 0));
        code.push(Instr::new(Op::Add, 0, 0));
    }
    code.push(Instr::new(Op::Print, 0, 0));
    code.push(Instr::new(Op::Halt, 0, 0));
    assert_parity(Program::new(code, 1));
}

#[test]
fn ordered_passes_propagate_literals_fold_arithmetic_and_remove_dead_stores() {
    let code = vec![
        Instr::new(Op::PushInt, 1, 0),
        Instr::new(Op::Store, 0, 0),
        Instr::new(Op::PushInt, 7, 0),
        Instr::new(Op::Store, 0, 0),
        Instr::new(Op::Load, 0, 0),
        Instr::new(Op::PushInt, -3, 0),
        Instr::new(Op::Div, 0, 0),
        Instr::new(Op::Store, 1, 0),
        Instr::new(Op::PushInt, 40, 0),
        Instr::new(Op::PushInt, 2, 0),
        Instr::new(Op::Add, 0, 0),
        Instr::new(Op::Pop, 0, 0),
        Instr::new(Op::Halt, 0, 0),
    ];
    let listing = assert_parity(Program::new(code, 2));
    assert!(listing.contains("store.i 0 7"), "{listing}");
    assert!(listing.contains("store.i 1 -3"), "floor division must fold exactly: {listing}");
    assert_eq!(listing.matches("store.i ").count(), 2, "{listing}");
    assert!(!listing.contains("push.i "), "folded/discarded temporaries remain: {listing}");
    assert!(!listing.contains("slot.move"), "known literals must propagate: {listing}");
}

#[test]
fn versioned_copy_chains_preserve_snapshots_after_source_and_intermediate_writes() {
    for overwritten in [0, 1] {
        let code = vec![
            Instr::new(Op::PushInt, 7, 0),
            Instr::new(Op::Store, 0, 0),
            Instr::new(Op::PushInt, 99, 0),
            Instr::new(Op::Print, 0, 0), // discard initial constant knowledge
            Instr::new(Op::Load, 0, 0),
            Instr::new(Op::Store, 1, 0),
            Instr::new(Op::Load, 1, 0),
            Instr::new(Op::Store, 2, 0),
            Instr::new(Op::PushInt, 99, 0),
            Instr::new(Op::Store, overwritten, 0),
            Instr::new(Op::Load, 2, 0),
            Instr::new(Op::PushInt, 3, 0),
            Instr::new(Op::Mul, 0, 0),
            Instr::new(Op::Store, 3, 0),
            Instr::new(Op::Load, 3, 0),
            Instr::new(Op::Print, 0, 0),
            Instr::new(Op::Halt, 0, 0),
        ];
        let listing = assert_parity(Program::new(code, 4));
        assert!(listing.contains("slot.move 0 2"), "unchanged copy chain should shorten: {listing}");
        let source = if overwritten == 0 { 2 } else { 0 };
        assert!(listing.contains(&format!("binary.mul.slot.i.store {source} 3 3")), "{listing}");
    }
}

#[test]
fn local_folding_preserves_all_checked_integer_failures_and_comparisons() {
    for mode in [
        Op::Add,
        Op::Sub,
        Op::Mul,
        Op::Div,
        Op::Lt,
        Op::Lte,
        Op::Gt,
        Op::Gte,
        Op::Eq,
        Op::Ne,
    ] {
        for (lhs, rhs) in [(7, -3), (i64::MAX, 1), (i64::MIN, -1), (7, 0)] {
            let code = vec![
                Instr::new(Op::PushInt, 9, 0),
                Instr::new(Op::Store, 0, 0),
                Instr::new(Op::PushInt, lhs, 0),
                Instr::new(Op::PushInt, rhs, 0),
                Instr::new(mode, 0, 0),
                Instr::new(Op::Store, 0, 0),
                Instr::new(Op::Load, 0, 0),
                Instr::new(Op::Print, 0, 0),
                Instr::new(Op::Halt, 0, 0),
            ];
            let listing = assert_parity(Program::new(code, 1));
            let fails = match mode {
                Op::Add => lhs.checked_add(rhs).is_none(),
                Op::Sub => lhs.checked_sub(rhs).is_none(),
                Op::Mul => lhs.checked_mul(rhs).is_none(),
                Op::Div => lhs.checked_div(rhs).is_none(),
                _ => false,
            };
            if fails {
                assert!(listing.contains("store.i 0 9"), "store before failure was erased: {listing}");
            }
        }
    }
}

#[test]
fn folded_branches_and_effect_barriers_keep_canonical_state() {
    let code = vec![
        Instr::new(Op::PushInt, 1, 0),
        Instr::new(Op::PushInt, 1, 0),
        Instr::new(Op::Sub, 0, 0),
        Instr::new(Op::JumpIfZero, 7, 0),
        Instr::new(Op::PushInt, 4, 0),
        Instr::new(Op::Print, 0, 0),
        Instr::new(Op::Jump, 9, 0),
        Instr::new(Op::PushInt, 5, 0),
        Instr::new(Op::Print, 0, 0),
        Instr::new(Op::Halt, 0, 0),
    ];
    let listing = assert_parity(Program::new(code, 0));
    assert!(!listing.contains("jz"), "constant condition was not folded: {listing}");
    assert!(!listing.contains("push.i 0"), "{listing}");

    let code = vec![
        Instr::new(Op::PushInt, 7, 0),
        Instr::new(Op::Store, 0, 0),
        Instr::new(Op::PushInt, 0, 0),
        Instr::new(Op::Print, 0, 0),
        Instr::new(Op::Load, 0, 0),
        Instr::new(Op::PushInt, 3, 0),
        Instr::new(Op::Mul, 0, 0),
        Instr::new(Op::Store, 1, 0),
        Instr::new(Op::Halt, 0, 0),
    ];
    let listing = assert_parity(Program::new(code, 2));
    assert!(listing.contains("binary.mul.slot.i.store 0 3 1"), "effect must invalidate facts: {listing}");
}

#[test]
fn propagated_literal_truthiness_preserves_float_bits_and_non_numeric_kinds() {
    for literal in [
        Instr::new(Op::PushBool, 0, 0),
        Instr::new(Op::PushBool, 1, 0),
        Instr::new(Op::PushNull, 0, 0),
        Instr::new(Op::PushFloat, (-0.0f64).to_bits() as i64, 0),
        Instr::new(Op::PushFloat, 0x7ff8_0000_0000_0042, 0),
        Instr::new(Op::PushFloat, f64::INFINITY.to_bits() as i64, 0),
    ] {
        let code = vec![
            literal,
            Instr::new(Op::Store, 0, 0),
            Instr::new(Op::Load, 0, 0),
            Instr::new(Op::JumpIfZero, 7, 0),
            Instr::new(Op::PushInt, 8, 0),
            Instr::new(Op::Print, 0, 0),
            Instr::new(Op::Jump, 9, 0),
            Instr::new(Op::PushInt, 9, 0),
            Instr::new(Op::Print, 0, 0),
            Instr::new(Op::Halt, 0, 0),
        ];
        let listing = assert_parity(Program::new(code, 1));
        assert!(!listing.contains("jz"), "{listing}");
    }
}

#[test]
fn deep_constant_folding_uses_topological_ids_without_recursion() {
    let mut code = Vec::with_capacity(131_075);
    code.push(Instr::new(Op::PushInt, 0, 0));
    for _ in 0..65_536 {
        code.push(Instr::new(Op::PushInt, 1, 0));
        code.push(Instr::new(Op::Add, 0, 0));
    }
    code.push(Instr::new(Op::Print, 0, 0));
    code.push(Instr::new(Op::Halt, 0, 0));
    let listing = assert_parity(Program::new(code, 0));
    assert!(listing.contains("push.i 65536"), "{listing}");
    assert_eq!(listing.matches("push.i ").count(), 1, "folding left temporary pushes");
}

#[test]
fn computed_store_facts_fold_chains_without_repeating_dynamic_expressions() {
    let verified = compile_source_verified("let a = 1 let b = a + 2 let c = b * 3 let d = c - a let e = d / 2 print e")
        .expect("constant dependency chain");
    let listing = assert_verified_parity(&verified);
    for (slot, value) in [(0, 1), (1, 3), (2, 9), (3, 8), (4, 4)] {
        assert!(listing.contains(&format!("store.i {slot} {value}")), "{listing}");
    }
    assert!(!listing.contains("binary."), "computed constants failed to propagate: {listing}");

    let code = vec![
        Instr::new(Op::PushInt, 7, 0),
        Instr::new(Op::Store, 0, 0),
        Instr::new(Op::PushInt, 0, 0),
        Instr::new(Op::Print, 0, 0),
        Instr::new(Op::Load, 0, 0),
        Instr::new(Op::PushInt, 2, 0),
        Instr::new(Op::Add, 0, 0),
        Instr::new(Op::Store, 1, 0),
        Instr::new(Op::PushInt, 99, 0),
        Instr::new(Op::Store, 0, 0),
        Instr::new(Op::Load, 1, 0),
        Instr::new(Op::PushInt, 3, 0),
        Instr::new(Op::Mul, 0, 0),
        Instr::new(Op::Store, 2, 0),
        Instr::new(Op::Halt, 0, 0),
    ];
    let listing = assert_parity(Program::new(code, 3));
    assert!(listing.contains("binary.mul.slot.i.store 1 3 2"), "must read the saved result: {listing}");
}

#[test]
fn deterministic_local_mutation_sequences_match_every_tier() {
    for seed in 0..128u64 {
        let mut state = seed + 1;
        let mut code = Vec::new();
        for slot in 0..4 {
            code.push(Instr::new(Op::PushInt, slot + 1, 0));
            code.push(Instr::new(Op::Store, slot, 0));
        }
        for _ in 0..48 {
            state = state.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
            let source = ((state >> 20) & 3) as i64;
            let destination = ((state >> 24) & 3) as i64;
            let immediate = ((state >> 32) % 7) as i64 - 3;
            match (state >> 16) % 5 {
                0 => {
                    code.push(Instr::new(Op::Load, source, 0));
                    code.push(Instr::new(Op::Store, destination, 0));
                }
                1 => {
                    code.push(Instr::new(Op::PushInt, immediate, 0));
                    code.push(Instr::new(Op::Store, destination, 0));
                }
                2 | 3 => {
                    let mode = [Op::Add, Op::Sub, Op::Mul, Op::Div][((state >> 28) & 3) as usize];
                    code.push(Instr::new(Op::Load, source, 0));
                    code.push(Instr::new(Op::PushInt, immediate, 0));
                    code.push(Instr::new(mode, 0, 0));
                    code.push(Instr::new(Op::Store, destination, 0));
                }
                _ => {
                    code.push(Instr::new(Op::Load, source, 0));
                    code.push(Instr::new(Op::Print, 0, 0));
                }
            }
        }
        code.push(Instr::new(Op::Halt, 0, 0));
        assert_parity(Program::new(code, 4));
    }
}
