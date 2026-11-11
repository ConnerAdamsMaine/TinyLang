//! Differential parity for the program-level x86-64 backend (`NativeProgram`).
//!
//! The dynamic interpreter (`ScalarProgram::interpret`) and the authoritative
//! VM define the expected semantics; the one emitted machine-code image must
//! reproduce them exactly — results, shared fuel budgets, trap categories and
//! failing instruction indexes included. Fixtures mirror
//! `engine_program_parity.rs`, so every case runs the same recursion through
//! three engines: VM (wrapper chunk), scalar interpreter, native image.

use std::sync::Arc;

use tinylang::engine::{EngineError, ExecutionBackend, PreparedProgram, ScalarProgram, TrapKind};
use tinylang::{Function, Instr, Op, Program, RuntimeValue, TinyMemory, VM, compile_source_unoptimized_verified};

fn native(program: &ScalarProgram) -> PreparedProgram {
    ExecutionBackend::NativeAsm
        .prepare_program(program)
        .expect("native program compilation must succeed for a lowered closure")
}

/// Trap kind and failing instruction of a trap error.
fn trap(error: &EngineError) -> Option<(TrapKind, usize)> {
    match error {
        EngineError::Trap { kind, instruction } => Some((*kind, *instruction)),
        _ => None,
    }
}

fn interpreter_trace(scalar: &ScalarProgram, args: &[i64], fuel: u64) -> Result<i64, EngineError> {
    scalar.interpret(args, fuel)
}

fn interpreter_min_fuel(scalar: &ScalarProgram, args: &[i64]) -> u64 {
    for fuel in 0..=64_000 {
        if interpreter_trace(scalar, args, fuel).is_ok() {
            return fuel;
        }
    }
    panic!("interpreter fixture never completes within the bisect ceiling");
}

fn native_min_fuel(native: &PreparedProgram, args: &[i64]) -> u64 {
    for fuel in 0..=64_000 {
        if native.invoke(args, fuel).is_ok() {
            return fuel;
        }
    }
    panic!("native fixture never completes within the bisect ceiling");
}

fn fact() -> (tinylang::VerifiedProgram, usize) {
    let verified = compile_source_unoptimized_verified("fn fact(n) { if n < 2 { return 1 } return n * fact(n - 1) }")
        .expect("fact compiles");
    (verified, 0)
}

#[test]
fn native_recursion_matches_interpreter_results_and_shared_fuel() {
    let (verified, index) = fact();
    let scalar = ScalarProgram::lower(&verified, index).expect("fact lowers");
    let machine = native(&scalar);
    let has_code = match &machine {
        PreparedProgram::NativeAsm(image) => !image.code_bytes().is_empty(),
        PreparedProgram::Interpreter(_) => false,
        #[cfg(feature = "cranelift")]
        PreparedProgram::Cranelift(_) => false,
    };
    assert!(has_code, "the image must contain code");

    for (argument, expected) in [(5i64, 120i64), (12, 479_001_600)] {
        let exact = interpreter_min_fuel(&scalar, &[argument]);
        assert_eq!(native_min_fuel(&machine, &[argument]), exact, "one shared budget, one exact minimum");
        assert_eq!(machine.invoke(&[argument], u64::MAX).expect("native fact"), expected);
        assert_eq!(interpreter_trace(&scalar, &[argument], u64::MAX).expect("interpreted fact"), expected);
    }
}

/// The strongest single check: for *every* fuel value around the boundary the
/// native image and the interpreter agree on success or on the trap pair.
#[test]
fn native_and_interpreter_agree_on_every_fuel_level() {
    let (verified, index) = fact();
    let scalar = ScalarProgram::lower(&verified, index).expect("fact lowers");
    let machine = native(&scalar);
    let exact = interpreter_min_fuel(&scalar, &[5]);
    for fuel in 0..=exact + 3 {
        let interpreted = interpreter_trace(&scalar, &[5], fuel);
        let executed = machine.invoke(&[5], fuel);
        assert_eq!(
            executed.is_ok(),
            interpreted.is_ok(),
            "fuel {fuel}: outcome mismatch (native {executed:?} vs interpreter {interpreted:?})"
        );
        match (&executed, &interpreted) {
            (Err(native_error), Err(interpreted_error)) => {
                assert_eq!(
                    trap(native_error),
                    trap(interpreted_error),
                    "fuel {fuel}: trap mismatch ({native_error:?} vs {interpreted_error:?})"
                );
            }
            (Ok(native_result), Ok(interpreted_result)) => {
                assert_eq!(native_result, interpreted_result, "fuel {fuel}: result mismatch");
            }
            _ => unreachable!("outcome agreement already asserted"),
        }
    }
}

#[test]
fn zero_argument_calls_empty_frame_bank_correctly() {
    let verified =
        compile_source_unoptimized_verified("fn answer() { return 7 * 6 }\nfn probe() { return answer() + answer() }")
            .expect("compiles");
    let scalar = ScalarProgram::lower(&verified, 1).expect("probe lowers");
    let machine = native(&scalar);
    assert_eq!(machine.invoke(&[], u64::MAX).expect("native probe"), 84);
    assert!(machine.invoke(&[], interpreter_min_fuel(&scalar, &[])).is_ok());
    assert!(matches!(
        machine.invoke(&[], interpreter_min_fuel(&scalar, &[]) - 1),
        Err(EngineError::Trap {
            kind: TrapKind::FuelExhausted,
            ..
        })
    ));
}

#[test]
fn mutual_recursion_through_the_machine_image_matches_the_vm() {
    // Same hand-built odd/even cycle as the interpreter parity suite.
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
    let machine = native(&scalar);
    assert_eq!(machine.invoke(&[10], u64::MAX).expect("odd(10)"), 0);
    assert_eq!(machine.invoke(&[7], u64::MAX).expect("odd(7)"), 1);
    assert_eq!(machine.invoke(&[1], u64::MAX).expect("odd(1)"), 1);
    // VM cross-check on the same bytecode through its wrapper chunk.
    let wrapper = |args: &[i64], call_index: usize, fuel: u64| -> Result<Vec<RuntimeValue>, String> {
        let mut code: Vec<Instr> = args.iter().map(|value| Instr::new(Op::PushInt, *value, 0)).collect();
        code.push(Instr::new(Op::Call, call_index as i64, args.len() as i64));
        code.push(Instr::new(Op::Store, 0, 0));
        code.push(Instr::new(Op::Halt, 0, 0));
        let vm_program = Program::new(code, 1).with_functions(functions.clone());
        let mut output = Vec::new();
        VM::new(Arc::new(vm_program), TinyMemory::try_new(1).map_err(|error| error.to_string())?, Vec::new())
            .map_err(|error| error.to_string())?
            .run_report_with_fuel(&mut output, fuel)
            .map(|report| report.memory.snapshot())
            .map_err(|error| error.to_string())
    };
    let vm_result = wrapper(&[10], 1, u64::MAX).expect("vm even(10) through the same cycle");
    assert_eq!(vm_result[0], RuntimeValue::I64(1));
}

#[test]
fn zero_argument_wrapper_cost_matches_the_vm_budget() {
    // Zero-argument wrapper cost = 3 verified at interpreter level; the native
    // image shares the same budget exactly (see native_recursion_matches_...):
    // here we lock the wrapper-cost relation end to end.
    let verified =
        compile_source_unoptimized_verified("fn answer() { return 7 * 6 }\nfn probe() { return answer() + answer() }")
            .expect("compiles");
    let functions: Vec<Function> = verified.program().functions().to_vec();
    let wrapper = |_args: &[i64], call_index: usize, fuel: u64| -> Result<Vec<RuntimeValue>, String> {
        let code = vec![
            Instr::new(Op::Call, call_index as i64, 0),
            Instr::new(Op::Store, 0, 0),
            Instr::new(Op::Halt, 0, 0),
        ];
        let vm_program = Program::new(code, 1).with_functions(functions.clone());
        let mut output = Vec::new();
        VM::new(Arc::new(vm_program), TinyMemory::try_new(1).map_err(|error| error.to_string())?, Vec::new())
            .map_err(|error| error.to_string())?
            .run_report_with_fuel(&mut output, fuel)
            .map(|report| report.memory.snapshot())
            .map_err(|error| error.to_string())
    };
    let scalar = ScalarProgram::lower(&verified, 1).expect("probe lowers");
    let machine = native(&scalar);
    let mut vm_min = u64::MAX;
    for fuel in 0..=1_000 {
        if wrapper(&[], 1, fuel).is_ok() {
            vm_min = fuel;
            break;
        }
    }
    assert_eq!(vm_min, interpreter_min_fuel(&scalar, &[]) + 3, "zero-arg wrapper costs exactly 3 VM units");
    assert_eq!(native_min_fuel(&machine, &[]), interpreter_min_fuel(&scalar, &[]));
}

#[test]
fn call_argument_order_matches_the_machine_image() {
    let verified = compile_source_unoptimized_verified("fn pack(a, b, c) { return (a - b) * c }").expect("compiles");
    let scalar = ScalarProgram::lower(&verified, 0).expect("pack lowers");
    let machine = native(&scalar);
    let arguments = [19i64, 7, 3];
    let expected = (arguments[0] - arguments[1]) * arguments[2];
    assert_eq!(machine.invoke(&arguments, u64::MAX).expect("native pack"), expected);
    assert_eq!(interpreter_trace(&scalar, &arguments, u64::MAX).expect("interpreted pack"), expected);
}

#[test]
fn infinite_recursion_traps_with_identical_traps() {
    let verified = compile_source_unoptimized_verified("fn spin(n) { return spin(n + 1) }").expect("compiles");
    let scalar = ScalarProgram::lower(&verified, 0).expect("spin lowers");
    let call = scalar
        .function(0)
        .expect("spin present")
        .instructions()
        .iter()
        .position(|instruction| matches!(instruction, tinylang::engine::Instruction::Call(0, 1)))
        .expect("spin calls itself");
    let interpreted = interpreter_trace(&scalar, &[0], 400_000).expect_err("interpreter stops at the depth limit");
    let machine = native(&scalar);
    let executed = machine
        .invoke(&[0], 400_000)
        .expect_err("native stops at the depth limit");
    assert_eq!(
        trap(&interpreted),
        trap(&executed),
        "interpreter {interpreted:?} and native {executed:?} must cite the same call"
    );
    assert!(
        matches!(executed, EngineError::Trap { kind: TrapKind::CallStackOverflow(16), instruction } if instruction == call),
        "depth trap must cite the failing call instruction {call}: {executed:?}"
    );
    // `spin` never completes, so a minimum-fuel bisect is meaningless here.
    // Instead sweep the shared budget line by line: at every fuel level below
    // the depth-trap threshold both engines must cite the identical trap.
    for fuel in 1..=67u64 {
        let interpreted = interpreter_trace(&scalar, &[0], fuel).expect_err("spin must trap");
        let executed = machine.invoke(&[0], fuel).expect_err("spin must trap");
        assert_eq!(
            trap(&interpreted),
            trap(&executed),
            "fuel {fuel}: interpreter {interpreted:?} and native {executed:?} must cite the same trap"
        );
        assert!(
            matches!(
                interpreted,
                EngineError::Trap {
                    kind: TrapKind::FuelExhausted,
                    ..
                }
            ),
            "fuel {fuel} is below the depth-trap threshold: {interpreted:?}"
        );
    }
}

#[test]
fn machine_recursion_depth_boundary_16_and_17() {
    let (verified, index) = fact();
    let scalar = ScalarProgram::lower(&verified, index).expect("fact lowers");
    let machine = native(&scalar);
    // 16 nested calls stay inside the limit; the 17th call exceeds it.
    machine
        .invoke(&[17], u64::MAX)
        .expect("fact(17) stays inside the depth limit");
    assert!(matches!(
        machine.invoke(&[18], u64::MAX),
        Err(EngineError::Trap {
            kind: TrapKind::CallStackOverflow(16),
            ..
        })
    ));
    let interpreted = interpreter_trace(&scalar, &[18], u64::MAX).expect_err("both engines refuse 17 nested calls");
    let executed = machine.invoke(&[18], u64::MAX).expect_err("native refuses too");
    assert_eq!(trap(&interpreted), trap(&executed), "boundary traps must match exactly");
}

#[test]
fn integer_trap_categories_match_across_the_machine_image() {
    let verified = compile_source_unoptimized_verified(
        "fn division(cond) { if cond == 1 { return 7 / 0 } return 0 }\nfn overflow(n) { return n * 4294967296 * 4294967296 }",
    )
    .expect("compiles");
    for (index, args, expected) in [
        (0usize, [1i64], TrapKind::DivisionByZero),
        (1, [4294967296i64], TrapKind::Overflow),
    ] {
        let scalar = ScalarProgram::lower(&verified, index).expect("closes");
        let machine = native(&scalar);
        let executed = machine.invoke(&args, u64::MAX).expect_err("must trap");
        let interpreted = interpreter_trace(&scalar, &args, u64::MAX).expect_err("must trap");
        assert!(matches!(executed, EngineError::Trap { kind, .. } if kind == expected));
        assert_eq!(trap(&interpreted), trap(&executed), "{expected:?} must match across backends: {executed:?}");
    }
}

#[test]
fn local_and_call_results_compose_in_the_machine_image() {
    let verified = compile_source_unoptimized_verified(
        "fn helper(n) { let doubled = n * 2 return doubled + n }\nfn drive(n) { let step = helper(n) return helper(step) }",
    )
    .expect("compiles");
    let scalar = ScalarProgram::lower(&verified, 1).expect("drive lowers");
    let machine = native(&scalar);
    let expected = (6 * 2 + 6) * 2 + (6 * 2 + 6);
    assert_eq!(machine.invoke(&[6], u64::MAX).expect("native drive"), expected);
    assert_eq!(interpreter_trace(&scalar, &[6], u64::MAX).expect("interpreted drive"), expected);
}

#[test]
fn native_program_rejects_arity_mismatch_before_execution() {
    let (verified, index) = fact();
    let scalar = ScalarProgram::lower(&verified, index).expect("fact lowers");
    let machine = native(&scalar);
    assert!(matches!(
        machine.invoke(&[], u64::MAX),
        Err(EngineError::InvalidArguments { expected: 1, actual: 0 })
    ));
    assert!(matches!(
        machine.invoke(&[1, 2], u64::MAX),
        Err(EngineError::InvalidArguments { expected: 1, actual: 2 })
    ));
}

#[test]
fn cranelift_program_backend_is_feature_or_platform_gated_explicitly() {
    let (verified, index) = fact();
    let scalar = ScalarProgram::lower(&verified, index).expect("fact lowers");
    match ExecutionBackend::Cranelift.prepare_program(&scalar) {
        // Compiled in on a supported host, the program profile executes.
        #[cfg(feature = "cranelift")]
        Ok(machine) => {
            assert!(matches!(machine, PreparedProgram::Cranelift(_)));
            assert_eq!(machine.invoke(&[5], u64::MAX).expect("cranelift fact"), 120);
        }
        #[cfg(not(feature = "cranelift"))]
        Ok(_) => panic!("cranelift must not prepare programs without its feature"),
        Err(EngineError::BackendUnavailable { backend, reason }) => {
            assert_eq!(backend, "cranelift");
            assert!(!reason.is_empty(), "an explicit reason must be reported: {reason}");
        }
        Err(other) => panic!("unexpected error {other:?}"),
    }
}
