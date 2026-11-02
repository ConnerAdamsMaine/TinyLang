use tinylang::{
    Function, Instr, Op, Program, VerifiedProgram, compile_source_unoptimized_verified,
};
use tinylang_v2::engine::{BinaryOp, EngineError, ScalarFunction};

#[test]
fn compiler_output_lowers_to_checked_integer_ir() {
    let program =
        compile_source_unoptimized_verified("fn add(a, b) { let c = a + b return c }").unwrap();
    let function = ScalarFunction::lower(&program, 0).unwrap();
    assert_eq!(function.name(), "add");
    assert_eq!(function.parameter_count(), 2);
    assert!(function.slot_count() >= 3);
    assert!(
        function
            .instructions()
            .iter()
            .any(|op| matches!(op, tinylang_v2::engine::Instruction::Binary(BinaryOp::Add)))
    );
    assert_eq!(function.interpret(&[19, 23], 100).unwrap(), 42);
}

#[test]
fn source_entry_point_reports_unknown_functions_and_preserves_parse_errors() {
    assert!(matches!(
        tinylang_v2::engine::compile_scalar_source("fn answer() { return 42 }", "missing"),
        Err(EngineError::UnknownFunction(_))
    ));
    assert!(matches!(
        tinylang_v2::engine::compile_scalar_source("fn broken( {", "broken"),
        Err(EngineError::Compile(_))
    ));
}

#[test]
fn heap_operations_are_rejected_instead_of_reinterpreted_as_integers() {
    let program = compile_source_unoptimized_verified("fn text() { return \"hello\" }").unwrap();
    assert!(matches!(
        ScalarFunction::lower(&program, 0),
        Err(EngineError::UnsupportedInstruction { instruction: 0, .. })
    ));
}

#[test]
fn integer_execution_checks_overflow_division_and_fuel() {
    let program = compile_source_unoptimized_verified(
        "fn add(a, b) { return a + b } fn div(a, b) { return a / b }",
    )
    .unwrap();
    let add = ScalarFunction::lower(&program, 0).unwrap();
    assert!(matches!(
        add.interpret(&[i64::MAX, 1], 100),
        Err(EngineError::Trap {
            kind: tinylang_v2::engine::TrapKind::Overflow,
            ..
        })
    ));
    assert!(matches!(
        add.interpret(&[1, 2], 0),
        Err(EngineError::Trap {
            kind: tinylang_v2::engine::TrapKind::FuelExhausted,
            instruction: 0
        })
    ));
    assert!(matches!(
        add.interpret(&[1], 100),
        Err(EngineError::InvalidArguments {
            expected: 2,
            actual: 1
        })
    ));
    let div = ScalarFunction::lower(&program, 1).unwrap();
    assert!(matches!(
        div.interpret(&[1, 0], 100),
        Err(EngineError::Trap {
            kind: tinylang_v2::engine::TrapKind::DivisionByZero,
            ..
        })
    ));
    assert!(matches!(
        div.interpret(&[i64::MIN, -1], 100),
        Err(EngineError::Trap {
            kind: tinylang_v2::engine::TrapKind::Overflow,
            ..
        })
    ));
}

#[test]
fn lowering_rejects_inconsistent_stack_heights_at_a_join() {
    let code = vec![
        Instr::new(Op::PushInt, 1, 0),
        Instr::new(Op::JumpIfZero, 4, 0),
        Instr::new(Op::PushInt, 9, 0),
        Instr::new(Op::Jump, 4, 0),
        Instr::new(Op::PushInt, 0, 0),
        Instr::new(Op::Return, 0, 0),
    ];
    let program = VerifiedProgram::verify(
        Program::new(vec![Instr::new(Op::Halt, 0, 0)], 0)
            .with_functions(vec![Function::new("join", 0, code, 0)]),
    );
    match program {
        Ok(program) => assert!(matches!(
            ScalarFunction::lower(&program, 0),
            Err(EngineError::InvalidIr { .. })
        )),
        Err(error) => assert!(error.to_string().contains("stack"), "{error}"),
    }
}

#[test]
fn inferred_loop_frames_preserve_vm_semantics() {
    let source = "fn sum(n) {
        let total = 0 let i = 0
        while i < n { total = total + i i = i + 1 }
        return total
    }";
    let program = compile_source_unoptimized_verified(source).unwrap();
    let lowered = ScalarFunction::lower(&program, 0).unwrap();
    let mut vm = tinylang::VM::from_verified_with_limits(
        &program,
        vec![],
        tinylang::VmExecutionLimits::default(),
    )
    .unwrap();
    for (n, expected) in [(0, 0), (1, 0), (10, 45), (100, 4950)] {
        let actual = vm
            .invoke_function_with_fuel(
                0,
                vec![tinylang::RuntimeValue::I64(n)],
                &mut Vec::new(),
                10000,
            )
            .unwrap();
        assert_eq!(actual, tinylang::RuntimeValue::I64(expected));
        assert_eq!(lowered.interpret(&[n], 10000).unwrap(), expected);
    }
}

#[test]
fn integer_division_uses_language_floor_semantics() {
    let program = compile_source_unoptimized_verified("fn div(a,b) { return a/b }").unwrap();
    let scalar = ScalarFunction::lower(&program, 0).unwrap();
    for (args, expected) in [
        ([-7, 3], -3),
        ([7, -3], -3),
        ([-7, -3], 2),
        ([-6, 3], -2),
        ([i64::MIN, 3], -3074457345618258603),
    ] {
        assert_eq!(scalar.interpret(&args, 100).unwrap(), expected);
    }
}

#[test]
fn boolean_values_are_not_implicitly_coerced_to_integer_results_or_operands() {
    for source in [
        "fn bad(a,b) { return a < b }",
        "fn bad(a,b) { return (a < b) + 1 }",
        "fn bad(a,b) { let x = a < b return x * 2 }",
        "fn bad(a,b) { let x=0 if a < b { x = a < b } return x+1 }",
    ] {
        let program = compile_source_unoptimized_verified(source).unwrap();
        assert!(
            matches!(
                ScalarFunction::lower(&program, 0),
                Err(EngineError::InvalidIr { .. })
            ),
            "{source}"
        );
    }
}

#[test]
fn exact_instruction_fuel_matches_the_authoritative_vm() {
    let program = compile_source_unoptimized_verified("fn add(a,b) { return a+b }").unwrap();
    let lowered = ScalarFunction::lower(&program, 0).unwrap();
    for fuel in 0..=5 {
        let mut vm = tinylang::VM::from_verified_with_limits(
            &program,
            vec![],
            tinylang::VmExecutionLimits::default(),
        )
        .unwrap();
        let actual = vm.invoke_function_with_fuel(
            0,
            vec![
                tinylang::RuntimeValue::I64(19),
                tinylang::RuntimeValue::I64(23),
            ],
            &mut Vec::new(),
            fuel,
        );
        if fuel < 4 {
            assert!(
                actual
                    .unwrap_err()
                    .to_string()
                    .contains("Instruction fuel exhausted")
            );
            assert!(matches!(
                lowered.interpret(&[19, 23], fuel),
                Err(EngineError::Trap {
                    kind: tinylang_v2::engine::TrapKind::FuelExhausted,
                    ..
                })
            ));
        } else {
            assert_eq!(actual.unwrap(), tinylang::RuntimeValue::I64(42));
            assert_eq!(lowered.interpret(&[19, 23], fuel).unwrap(), 42);
        }
    }
}
