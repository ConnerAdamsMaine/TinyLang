use tinylang::{
    Function, Instr, Op, Program, VerifiedProgram, compile_source_unoptimized_verified,
};
use tinylang_v2::engine::{EngineError, ScalarFunction, TrapKind};

fn verify(code: Vec<Instr>, slots: usize) -> tinylang::Result<VerifiedProgram> {
    VerifiedProgram::verify(
        Program::new(vec![Instr::new(Op::Halt, 0, 0)], 0).with_functions(vec![Function::new(
            "candidate",
            0,
            code,
            slots,
        )]),
    )
}

#[test]
fn unreachable_unsupported_values_cannot_enter_native_ir() {
    let program = verify(
        vec![
            Instr::new(Op::PushInt, 42, 0),
            Instr::new(Op::Return, 0, 0),
            Instr::new(Op::PushBool, 1, 0),
            Instr::new(Op::Return, 0, 0),
        ],
        0,
    )
    .unwrap();
    assert!(matches!(
        ScalarFunction::lower(&program, 0),
        Err(EngineError::UnsupportedInstruction { instruction: 2, .. })
    ));
}

#[test]
fn scalar_limits_reject_oversized_code_without_truncating_execution() {
    let mut code = vec![Instr::new(Op::Jump, 0, 0); 4097];
    code.extend([Instr::new(Op::PushInt, 42, 0), Instr::new(Op::Return, 0, 0)]);
    let program = verify(code, 0).unwrap();
    assert!(matches!(
        ScalarFunction::lower(&program, 0),
        Err(EngineError::InvalidIr { .. })
    ));
}

#[test]
fn zero_stack_loop_remains_bounded_by_fuel() {
    let program = verify(
        vec![
            Instr::new(Op::Jump, 0, 0),
            Instr::new(Op::PushInt, 0, 0),
            Instr::new(Op::Return, 0, 0),
        ],
        0,
    )
    .unwrap();
    let function = ScalarFunction::lower(&program, 0).unwrap();
    assert_eq!(function.max_stack(), 0);
    assert!(matches!(
        function.interpret(&[], 100),
        Err(EngineError::Trap {
            kind: TrapKind::FuelExhausted,
            instruction: 0
        })
    ));
}

#[test]
fn source_type_mismatches_never_become_implicit_native_conversions() {
    for expression in ["true", "null", "1.5", "\"text\"", "[1,2]"] {
        let program =
            compile_source_unoptimized_verified(&format!("fn invalid() {{ return {expression} }}"))
                .unwrap();
        assert!(matches!(
            ScalarFunction::lower(&program, 0),
            Err(EngineError::UnsupportedInstruction { .. })
        ));
    }
}

#[test]
fn deterministic_malformed_bytecode_is_rejected_without_panicking() {
    for target in -2..10 {
        for operation in [Op::Load, Op::Store, Op::Jump, Op::JumpIfZero] {
            let candidate = verify(
                vec![
                    Instr::new(Op::PushInt, 1, 0),
                    Instr::new(operation, target, 0),
                    Instr::new(Op::PushInt, 42, 0),
                    Instr::new(Op::Return, 0, 0),
                ],
                1,
            );
            let Ok(program) = candidate else {
                continue;
            };
            let Ok(function) = ScalarFunction::lower(&program, 0) else {
                continue;
            };
            match function.interpret(&[], 100) {
                Ok(value) => assert!(matches!(value, 1 | 42)),
                Err(EngineError::Trap {
                    kind: TrapKind::FuelExhausted,
                    ..
                }) => {}
                Err(error) => {
                    panic!("validated scalar program failed unexpectedly: {error}")
                }
            }
        }
    }
}

#[test]
fn type_flow_memory_budget_is_enforced_before_state_expansion() {
    let mut code = vec![Instr::new(Op::Jump, 0, 0)];
    code.extend(vec![Instr::new(Op::Load, 4095, 0); 2048]);
    code.push(Instr::new(Op::Return, 0, 0));
    let program = verify(code, 4096).unwrap();
    let error = ScalarFunction::lower(&program, 0).unwrap_err();
    assert!(matches!(error, EngineError::InvalidIr { .. }));
    assert!(error.to_string().contains("type-flow state"));
}
