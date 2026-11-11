//! Differential parity between the authoritative VM and the program-level
//! checked scalar profile (`ScalarProgram`).
//!
//! Every fixture runs the same recursion through the VM (reference) and the
//! scalar program interpreter (candidate). The VM wrapper main chunk is
//! `args..., Call(index, arity), Store(0), Halt`, so a shared budget equals
//! `scalar_min_fuel + args + 3`: the pushes, the call, the store and the halt
//! execute in the VM but not in the scalar invocation. Results, trap
//! categories and exact fuel budgets must agree.
//!
//! Note: frontend verification resolves function references at definition
//! position, so fixtures order callees before callers.

use std::sync::Arc;

use tinylang::engine::{EngineError, ScalarProgram, TrapKind};
use tinylang::{Function, Instr, Op, Program, RuntimeValue, TinyMemory, VM, compile_source_unoptimized_verified};

fn wrapper_code(args: &[i64], call_index: usize) -> Vec<Instr> {
    let mut code: Vec<Instr> = args.iter().map(|value| Instr::new(Op::PushInt, *value, 0)).collect();
    code.push(Instr::new(Op::Call, call_index as i64, args.len() as i64));
    code.push(Instr::new(Op::Store, 0, 0));
    code.push(Instr::new(Op::Halt, 0, 0));
    code
}

/// Runs the wrapper through the VM; slot 0 holds the call result on success.
fn vm_run(args: &[i64], functions: &[Function], call_index: usize, fuel: u64) -> Result<Vec<RuntimeValue>, String> {
    let program = Program::new(wrapper_code(args, call_index), 1).with_functions(functions.to_vec());
    let mut output = Vec::new();
    match VM::new(Arc::new(program), TinyMemory::try_new(1).map_err(|error| error.to_string())?, Vec::new())
        .map_err(|error| error.to_string())?
        .run_report_with_fuel(&mut output, fuel)
    {
        Ok(report) => Ok(report.memory.snapshot()),
        Err(error) => Err(error.to_string()),
    }
}

/// Smallest fuel where the VM wrapper completes.
fn vm_min_fuel(args: &[i64], functions: &[Function], call_index: usize) -> u64 {
    for fuel in 0..=64_000 {
        if vm_run(args, functions, call_index, fuel).is_ok() {
            return fuel;
        }
    }
    panic!("VM fixture never completes within the bisect ceiling");
}

/// Smallest fuel where the scalar program interpreter completes.
fn scalar_min_fuel(scalar: &ScalarProgram, args: &[i64]) -> u64 {
    for fuel in 0..=64_000 {
        if scalar.interpret(args, fuel).is_ok() {
            return fuel;
        }
    }
    panic!("scalar fixture never completes within the bisect ceiling");
}

fn fact() -> (tinylang::VerifiedProgram, usize) {
    let verified = compile_source_unoptimized_verified("fn fact(n) { if n < 2 { return 1 } return n * fact(n - 1) }")
        .expect("fact compiles");
    (verified, 0)
}

#[test]
fn recursive_fact_matches_vm_results_and_exact_fuel_budgets() {
    let (verified, index) = fact();
    let functions: Vec<Function> = verified.program().functions().to_vec();
    let scalar = ScalarProgram::lower(&verified, index).expect("fact lowers");

    for (argument, expected) in [(5i64, 120i64), (12, 479_001_600)] {
        let vm_result = vm_run(&[argument], &functions, index, 64_000).expect("vm succeeds");
        let scalar_result = scalar.interpret(&[argument], 64_000).expect("scalar succeeds");
        assert_eq!(vm_result[0], RuntimeValue::I64(expected));
        assert_eq!(scalar_result, expected);
        assert_eq!(
            vm_min_fuel(&[argument], &functions, index),
            scalar_min_fuel(&scalar, &[argument]) + 1 + 3,
            "exact fuel budgets must differ only by the VM wrapper cost"
        );
    }
}

#[test]
fn zero_argument_calls_match_vm_results_and_budgets() {
    let source = "fn answer() { return 7 * 6 }\nfn probe() { return answer() + answer() }";
    let verified = compile_source_unoptimized_verified(source).expect("compiles");
    let functions: Vec<Function> = verified.program().functions().to_vec();
    // Wrapper calls entry index 1 (probe), no wrapper need to alter layout.
    let scalar = ScalarProgram::lower(&verified, 1).expect("probe lowers");
    let vm_result = vm_run(&[], &functions, 1, 64_000).expect("vm succeeds");
    let scalar_result = scalar.interpret(&[], 64_000).expect("scalar succeeds");
    assert_eq!(scalar_result, 84);
    assert_eq!(vm_result[0], RuntimeValue::I64(scalar_result));
    // A zero-argument wrapper costs exactly 3 units beyond the closure.
    assert_eq!(vm_min_fuel(&[], &functions, 1), scalar_min_fuel(&scalar, &[]) + 3);
}

#[test]
fn mutual_recursion_executes_through_the_dynamic_cycle() {
    // The frontend rejects forward references in source, but bytecode (and
    // its artifacts) can contain index-level cycles. Hand-build the classic
    // odd/even pair and run the same bytecode through both engines.
    let odd_code = vec![
        Instr::new(Op::Load, 0, 0),
        Instr::new(Op::PushInt, 0, 0),
        Instr::new(Op::Eq, 0, 0),
        Instr::new(Op::JumpIfZero, 6, 0),
        Instr::new(Op::PushInt, 0, 0),
        Instr::new(Op::Return, 0, 0),
        Instr::new(Op::Load, 0, 0),
        Instr::new(Op::PushInt, 1, 0),
        Instr::new(Op::Sub, 0, 0),
        Instr::new(Op::Call, 1, 1),
        Instr::new(Op::Return, 0, 0),
    ];
    let even_code = vec![
        Instr::new(Op::Load, 0, 0),
        Instr::new(Op::PushInt, 0, 0),
        Instr::new(Op::Eq, 0, 0),
        Instr::new(Op::JumpIfZero, 6, 0),
        Instr::new(Op::PushInt, 1, 0),
        Instr::new(Op::Return, 0, 0),
        Instr::new(Op::Load, 0, 0),
        Instr::new(Op::PushInt, 1, 0),
        Instr::new(Op::Sub, 0, 0),
        Instr::new(Op::Call, 0, 1),
        Instr::new(Op::Return, 0, 0),
    ];
    let program = Program::new(vec![Instr::new(Op::Halt, 0, 0)], 0).with_functions(vec![
        Function::new("odd", 1, odd_code, 1),
        Function::new("even", 1, even_code, 1),
    ]);
    let functions: Vec<Function> = program.functions().to_vec();
    let verified = tinylang::VerifiedProgram::verify(program).expect("bytecode verifies");
    let scalar = ScalarProgram::lower(&verified, 0).expect("cycle lowers");
    assert_eq!(scalar.interpret(&[10], 64_000).expect("odd(10)"), 0);
    assert_eq!(scalar.interpret(&[7], 64_000).expect("odd(7)"), 1);
    let vm_even = vm_run(&[10], &functions, 1, 64_000).expect("vm even(10) through the same cycle");
    assert_eq!(vm_even[0], RuntimeValue::I64(1));
}

#[test]
fn call_argument_order_matches_the_vm_operand_stack() {
    let source = "fn pack(a, b, c) { return (a - b) * c }";
    let verified = compile_source_unoptimized_verified(source).expect("compiles");
    let functions: Vec<Function> = verified.program().functions().to_vec();
    let scalar = ScalarProgram::lower(&verified, 0).expect("pack lowers");
    let arguments = [19i64, 7, 3];
    let vm_result = vm_run(&arguments, &functions, 0, 64_000).expect("vm succeeds");
    let expected = (arguments[0] - arguments[1]) * arguments[2];
    assert_eq!(vm_result[0], RuntimeValue::I64(expected));
    assert_eq!(scalar.interpret(&arguments, 64_000).expect("scalar"), expected);
}

#[test]
fn locals_and_call_results_compose_as_in_the_vm() {
    let source = "fn helper(n) { let doubled = n * 2 return doubled + n }\nfn drive(n) { let step = helper(n) return helper(step) }";
    let verified = compile_source_unoptimized_verified(source).expect("compiles");
    let functions: Vec<Function> = verified.program().functions().to_vec();
    let scalar = ScalarProgram::lower(&verified, 1).expect("drive lowers");
    let vm_result = vm_run(&[6], &functions, 1, 64_000).expect("vm succeeds");
    let expected = (6 * 2 + 6) * 2 + (6 * 2 + 6);
    assert_eq!(vm_result[0], RuntimeValue::I64(expected));
    assert_eq!(scalar.interpret(&[6], 64_000).expect("scalar"), expected);
}

#[test]
fn infinite_recursion_traps_at_the_vm_call_depth_limit() {
    let source = "fn spin(n) { return spin(n + 1) }";
    let verified = compile_source_unoptimized_verified(source).expect("compiles");
    let scalar = ScalarProgram::lower(&verified, 0).expect("spin lowers");
    let call = scalar
        .function(0)
        .expect("spin present")
        .instructions()
        .iter()
        .position(|instruction| matches!(instruction, tinylang::engine::Instruction::Call(0, 1)))
        .expect("spin calls itself");
    let error = scalar
        .interpret(&[0], 400_000)
        .expect_err("must stop at the depth limit");
    assert!(
        matches!(
            error,
            EngineError::Trap {
                kind: TrapKind::CallStackOverflow(16),
                instruction,
            } if instruction == call
        ),
        "trap must cite the failing call instruction {call}: {error}"
    );
    let functions: Vec<Function> = verified.program().functions().to_vec();
    let vm_error = vm_run(&[0], &functions, 0, 400_000).expect_err("vm must stop too");
    assert!(
        vm_error.contains("Call stack overflow after 16"),
        "VM should stop at the same depth limit: {vm_error}"
    );
}

#[test]
fn recursion_depth_limit_matches_vm_boundary_exactly() {
    // The VM ticket's top-level chunk is the engine budget's "wrapper": the
    // scalar entry corresponds to the chunk, so both engines allow 16 nested
    // calls from their own entry level. Both rubber-band at the same limit
    // value with the wrapper frame accounted top-level to top-level.
    let (verified, index) = fact();
    let functions: Vec<Function> = verified.program().functions().to_vec();
    let scalar = ScalarProgram::lower(&verified, index).expect("fact lowers");
    // Scalar: 16 nested calls after the entry are the maximum. fact(17)
    // recurses 16 levels; fact(18) needs 17 and must not execute.
    scalar
        .interpret(&[17], u64::MAX)
        .expect("16 nested calls stay inside the limit");
    let exceeded = scalar
        .interpret(&[18], u64::MAX)
        .expect_err("17 nested calls exceed the limit");
    assert!(matches!(
        exceeded,
        EngineError::Trap {
            kind: TrapKind::CallStackOverflow(16),
            ..
        }
    ));
    // VM: the wrapper consumes one call slot, so fact(16) is its boundary.
    vm_run(&[16], &functions, index, u64::MAX).expect("vm accepts its 16-call boundary");
    let vm_error = vm_run(&[17], &functions, index, u64::MAX).expect_err("vm must stop at one call beyond");
    assert!(vm_error.contains("Call stack overflow after 16"), "VM limit message: {vm_error}");
}

#[test]
fn fuel_exhaustion_boundary_matches_vm_categories() {
    let (verified, index) = fact();
    let functions: Vec<Function> = verified.program().functions().to_vec();
    let scalar = ScalarProgram::lower(&verified, index).expect("lower");
    let exact = scalar_min_fuel(&scalar, &[5]);
    assert!(exact > 0, "fuel boundary must be positive");
    // Scalar: one unit below the exact budget is fuel exhaustion, exactly at
    // it succeeds.
    let scarce = scalar.interpret(&[5], exact - 1).expect_err("one unit short");
    assert!(matches!(
        scarce,
        EngineError::Trap {
            kind: TrapKind::FuelExhausted,
            ..
        }
    ));
    let vm_error = vm_run(&[5], &functions, index, exact - 1).expect_err("vm one unit short");
    assert!(vm_error.contains("fuel"), "{vm_error}");
}

#[test]
fn unsupported_callee_rejects_the_whole_call_closure() {
    let verified = compile_source_unoptimized_verified(
        "fn bad() { return 1.5 }\nfn ok(n) { return bad() + n }\nfn leaf(n) { return n + 1 }",
    )
    .expect("compiles");
    let error = ScalarProgram::lower(&verified, 1).expect_err("ok must not lower");
    assert!(matches!(error, EngineError::UnsupportedInstruction { .. }), "{error}");
    // An entry whose closure excludes the unsupported callee lowers fine.
    assert!(ScalarProgram::lower(&verified, 2).is_ok());
}

#[test]
fn unknown_entry_index_is_rejected() {
    let verified = compile_source_unoptimized_verified("fn answer() { return 1 }").expect("compiles");
    assert!(matches!(ScalarProgram::lower(&verified, 1), Err(EngineError::UnknownFunction(_))));
    assert!(ScalarProgram::lower(&verified, 0).is_ok());
}

#[test]
fn bool_comparison_result_cannot_become_a_call_argument() {
    // `take` consumes its flag through JumpIfZero, which the VM can express
    // with a Bool. The checked profile requires I64 call arguments, so the
    // caller is rejected before any native code can execute.
    let verified = compile_source_unoptimized_verified(
        "fn take(flag) { if flag { return 1 } return 0 }\nfn drive(a, b) { return take(a < b) }",
    )
    .expect("compiles");
    let error = ScalarProgram::lower(&verified, 1).expect_err("Bool call arguments stay outside the profile");
    assert!(matches!(error, EngineError::InvalidIr { .. }), "{error}");
}

#[test]
fn integer_trap_categories_match_between_profiles() {
    let source = "fn division(cond) { if cond == 1 { return 7 / 0 } return 0 }\nfn overflow(n) { return n * 4294967296 * 4294967296 }";
    let verified = compile_source_unoptimized_verified(source).expect("compiles");
    let functions: Vec<Function> = verified.program().functions().to_vec();
    let scalar_division = ScalarProgram::lower(&verified, 0).expect("division lowers");
    let division = scalar_division.interpret(&[1], 1_000).expect_err("divide by zero");
    assert!(matches!(
        division,
        EngineError::Trap {
            kind: TrapKind::DivisionByZero,
            ..
        }
    ));
    let vm_division = vm_run(&[1], &functions, 0, 1_000).expect_err("vm division");
    assert!(vm_division.contains("zero"), "{vm_division}");

    let scalar_overflow = ScalarProgram::lower(&verified, 1).expect("overflow lowers");
    // 4294967296 squared overflows signed i64 at the second multiply; the
    // single function frame never approaches the call-depth limit.
    let overflow = scalar_overflow
        .interpret(&[4294967296], u64::MAX)
        .expect_err("overflow");
    assert!(
        matches!(
            overflow,
            EngineError::Trap {
                kind: TrapKind::Overflow,
                ..
            }
        ),
        "{overflow}"
    );
    let vm_overflow = vm_run(&[4294967296], &functions, 1, u64::MAX).expect_err("vm overflow");
    assert!(vm_overflow.contains("Overflow") || vm_overflow.contains("overflow"), "{vm_overflow}");
}
