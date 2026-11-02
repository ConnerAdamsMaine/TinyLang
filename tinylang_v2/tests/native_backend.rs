use tinylang_v2::engine::native::NativeFunction;
use tinylang_v2::engine::{EngineError, ScalarFunction};

#[cfg(all(target_arch = "x86_64", target_os = "windows"))]
mod windows_x64 {
    use super::*;
    use std::ffi::c_void;
    use tinylang::{
        Function, Instr, Op, Program, RuntimeValue, VM, VerifiedProgram, VmExecutionLimits,
        compile_source_unoptimized_verified,
    };
    use tinylang_v2::engine::{BinaryOp, Instruction, TrapKind};

    #[derive(Debug, PartialEq, Eq)]
    enum Outcome {
        Value(i64),
        Trap(TrapKind),
    }

    fn scalar(program: &VerifiedProgram, index: usize) -> ScalarFunction {
        ScalarFunction::lower(program, index).unwrap()
    }

    fn fixture(arity: usize, operations: &[(Op, i64)]) -> VerifiedProgram {
        let slots = operations
            .iter()
            .filter(|(op, _)| matches!(op, Op::Load | Op::Store))
            .map(|(_, slot)| usize::try_from(*slot).unwrap() + 1)
            .fold(arity, usize::max);
        VerifiedProgram::verify(
            Program::new(vec![Instr::new(Op::Halt, 0, 0)], 0).with_functions(vec![Function::new(
                "fixture",
                arity,
                operations
                    .iter()
                    .map(|&(op, arg)| Instr::new(op, arg, 0))
                    .collect(),
                slots,
            )]),
        )
        .unwrap()
    }

    fn native_outcome(result: Result<i64, EngineError>) -> Outcome {
        match result {
            Ok(value) => Outcome::Value(value),
            Err(EngineError::Trap { kind, .. }) => Outcome::Trap(kind),
            other => panic!("unexpected native result: {other:?}"),
        }
    }

    fn vm_outcome(program: &VerifiedProgram, index: usize, args: &[i64], fuel: u64) -> Outcome {
        let mut vm =
            VM::from_verified_with_limits(program, Vec::new(), VmExecutionLimits::default())
                .unwrap();
        let result = vm.invoke_function_with_fuel(
            index,
            args.iter().copied().map(RuntimeValue::I64).collect(),
            &mut Vec::new(),
            fuel,
        );
        match result {
            Ok(RuntimeValue::I64(value)) => Outcome::Value(value),
            Err(error) => {
                let message = error.to_string();
                let kind = if message.to_ascii_lowercase().contains("fuel") {
                    TrapKind::FuelExhausted
                } else if message.contains("Division by zero") {
                    TrapKind::DivisionByZero
                } else if message.to_ascii_lowercase().contains("overflow") {
                    TrapKind::Overflow
                } else {
                    panic!("unexpected VM error: {message}");
                };
                Outcome::Trap(kind)
            }
            other => panic!("unexpected VM value: {other:?}"),
        }
    }

    fn assert_value(program: &VerifiedProgram, index: usize, args: &[i64], expected: i64) {
        let function = scalar(program, index);
        let native = NativeFunction::compile(&function).unwrap();
        assert!(!native.code_bytes().is_empty());
        assert_eq!(native.invoke(args, 1_000).unwrap(), expected);
        assert_eq!(function.interpret(args, 1_000).unwrap(), expected);
        assert_eq!(
            vm_outcome(program, index, args, 1_000),
            Outcome::Value(expected)
        );
    }

    // Reversing argument slots, leaving stale local state or sign-extending imm64
    // incorrectly must change these literal results.
    #[test]
    fn compiler_arguments_locals_and_negative_constants_execute_in_order() {
        let program = compile_source_unoptimized_verified(
            "fn ordered(a, b) { let c = a * 100 + b return c - 9 }",
        )
        .unwrap();
        assert_value(&program, 0, &[7, -3], 688);
        assert_value(&program, 0, &[-3, 7], -302);
        for value in [i64::MIN, -4_294_967_297, -1, 0, i64::MAX] {
            let program = fixture(0, &[(Op::PushInt, value), (Op::Return, 0)]);
            assert_value(&program, 0, &[], value);
        }
    }

    // Signed setcc selection and equality flags must choose the matching branch.
    // Comparisons remain Bool internally; only explicit I64 returns are eligible.
    #[test]
    fn all_comparisons_drive_signed_branches_returning_i64() {
        let operators = [
            ("<", [1, 0, 0, 1, 0]),
            ("<=", [1, 1, 0, 1, 0]),
            (">", [0, 0, 1, 0, 1]),
            (">=", [0, 1, 1, 0, 1]),
            ("==", [0, 1, 0, 0, 0]),
            ("!=", [1, 0, 1, 1, 1]),
        ];
        let arguments = [
            (-7, 2),
            (3, 3),
            (9, -2),
            (i64::MIN, i64::MAX),
            (i64::MAX, i64::MIN),
        ];
        for (operator, expected) in operators {
            let program = compile_source_unoptimized_verified(&format!(
                "fn compare(a, b) {{ if a {operator} b {{ return 1 }} else {{ return 0 }} }}"
            ))
            .unwrap();
            for ((lhs, rhs), want) in arguments.into_iter().zip(expected) {
                assert_value(&program, 0, &[lhs, rhs], want);
            }
        }
    }

    // Missing OF tests or unsafe idiv must fail with located traps, not wrap
    // values or kill this integration-test process.
    #[test]
    fn checked_arithmetic_and_division_guards_match_vm_traps() {
        let cases = [
            ("+", BinaryOp::Add, i64::MAX, 1, TrapKind::Overflow),
            ("+", BinaryOp::Add, i64::MIN, -1, TrapKind::Overflow),
            ("-", BinaryOp::Subtract, i64::MIN, 1, TrapKind::Overflow),
            ("-", BinaryOp::Subtract, i64::MAX, -1, TrapKind::Overflow),
            ("*", BinaryOp::Multiply, i64::MAX, 2, TrapKind::Overflow),
            ("*", BinaryOp::Multiply, i64::MIN, -1, TrapKind::Overflow),
            ("/", BinaryOp::Divide, 5, 0, TrapKind::DivisionByZero),
            ("/", BinaryOp::Divide, i64::MIN, -1, TrapKind::Overflow),
        ];
        for (operator, binary, lhs, rhs, kind) in cases {
            let program = compile_source_unoptimized_verified(&format!(
                "fn arithmetic(a, b) {{ return a {operator} b }}"
            ))
            .unwrap();
            let function = scalar(&program, 0);
            let pc = function
                .instructions()
                .iter()
                .position(|op| *op == Instruction::Binary(binary))
                .unwrap();
            let native = NativeFunction::compile(&function).unwrap();
            assert!(matches!(
                native.invoke(&[lhs, rhs], 100),
                Err(EngineError::Trap { kind: actual, instruction })
                    if actual == kind && instruction == pc
            ));
            assert_eq!(
                vm_outcome(&program, 0, &[lhs, rhs], 100),
                Outcome::Trap(kind)
            );
        }

        let program = compile_source_unoptimized_verified("fn negate(a) { return -a }").unwrap();
        let function = scalar(&program, 0);
        let pc = function
            .instructions()
            .iter()
            .position(|op| *op == Instruction::Negate)
            .unwrap();
        let native = NativeFunction::compile(&function).unwrap();
        assert!(matches!(
            native.invoke(&[i64::MIN], 100),
            Err(EngineError::Trap { kind: TrapKind::Overflow, instruction }) if instruction == pc
        ));
        assert_eq!(
            vm_outcome(&program, 0, &[i64::MIN], 100),
            Outcome::Trap(TrapKind::Overflow)
        );
        assert_eq!(native.invoke(&[-123], 100).unwrap(), 123);
        assert_eq!(native.invoke(&[i64::MAX], 100).unwrap(), -i64::MAX);

        // Exact quotients also exercise all divisor/dividend sign combinations.
        let program =
            compile_source_unoptimized_verified("fn divide(a, b) { return a / b }").unwrap();
        for (lhs, rhs, want) in [
            (21, 3, 7),
            (-21, 3, -7),
            (21, -3, -7),
            (-21, -3, 7),
            (i64::MIN, 1, i64::MIN),
        ] {
            assert_value(&program, 0, &[lhs, rhs], want);
        }
    }

    // Fuel must be charged before every original PC, including return and
    // operations that would otherwise trap; arity is checked before execution.
    #[test]
    fn exact_fuel_and_arity_are_checked_before_work() {
        let program = fixture(
            0,
            &[
                (Op::PushInt, 19),
                (Op::PushInt, 23),
                (Op::Add, 0),
                (Op::Return, 0),
            ],
        );
        let function = scalar(&program, 0);
        let native = NativeFunction::compile(&function).unwrap();
        for fuel in 0..4 {
            assert!(matches!(
                native.invoke(&[], fuel),
                Err(EngineError::Trap { kind: TrapKind::FuelExhausted, instruction })
                    if instruction == fuel as usize
            ));
            assert_eq!(
                vm_outcome(&program, 0, &[], fuel),
                Outcome::Trap(TrapKind::FuelExhausted)
            );
        }
        assert_eq!(native.invoke(&[], 4).unwrap(), 42);
        assert_eq!(native.invoke(&[], u64::MAX).unwrap(), 42);
        assert_eq!(vm_outcome(&program, 0, &[], 4), Outcome::Value(42));
        assert!(matches!(
            native.invoke(&[1], 0),
            Err(EngineError::InvalidArguments {
                expected: 0,
                actual: 1
            })
        ));

        let program = fixture(
            2,
            &[(Op::Load, 0), (Op::Load, 1), (Op::Div, 0), (Op::Return, 0)],
        );
        let native = NativeFunction::compile(&scalar(&program, 0)).unwrap();
        assert!(matches!(
            native.invoke(&[1], 100),
            Err(EngineError::InvalidArguments {
                expected: 2,
                actual: 1
            })
        ));
        assert!(matches!(
            native.invoke(&[1, 2, 3], 100),
            Err(EngineError::InvalidArguments {
                expected: 2,
                actual: 3
            })
        ));
        assert!(matches!(
            native.invoke(&[1, 0], 2),
            Err(EngineError::Trap {
                kind: TrapKind::FuelExhausted,
                instruction: 2
            })
        ));
        assert!(matches!(
            native.invoke(&[1, 0], 3),
            Err(EngineError::Trap {
                kind: TrapKind::DivisionByZero,
                instruction: 2
            })
        ));
    }

    // An idiv-only implementation truncates toward zero and disagrees with the
    // authoritative VM's floor division when signs differ and remainder != 0.
    #[test]
    fn inexact_signed_division_matches_authoritative_vm_floor_semantics() {
        let program =
            compile_source_unoptimized_verified("fn divide(a, b) { return a / b }").unwrap();
        let native = NativeFunction::compile(&scalar(&program, 0)).unwrap();
        for (lhs, rhs, want) in [
            (85, 2, 42),
            (-85, 2, -43),
            (85, -2, -43),
            (-85, -2, 42),
            (-1, 2, -1),
            (1, i64::MIN, -1),
            (i64::MAX, i64::MIN, -1),
            (i64::MIN, i64::MAX, -2),
            (i64::MIN, 3, -3_074_457_345_618_258_603),
            (i64::MIN, -3, 3_074_457_345_618_258_602),
            (0, -1, 0),
        ] {
            assert_eq!(
                vm_outcome(&program, 0, &[lhs, rhs], 100),
                Outcome::Value(want)
            );
            assert_eq!(native.invoke(&[lhs, rhs], 100).unwrap(), want);
        }
    }

    // Branch patching, stack-height joins, backward edges and per-path fuel
    // errors must agree with the independent bytecode VM.
    #[test]
    fn compiler_branches_and_loops_have_per_pc_fuel() {
        let program = compile_source_unoptimized_verified(
            "fn branch(a) { if a { return 11 } else { return 22 } }
             fn sum(n) { let total = 0 while n > 0 { total = total + n n = n - 1 } return total }",
        )
        .unwrap();
        assert_value(&program, 0, &[0], 22);
        assert_value(&program, 0, &[-7], 11);
        assert_value(&program, 1, &[4], 10);
        assert_value(&program, 1, &[0], 0);
        for (index, args) in [(0, vec![0]), (0, vec![3]), (1, vec![4]), (1, vec![0])] {
            let function = scalar(&program, index);
            let native = NativeFunction::compile(&function).unwrap();
            for fuel in 0..100 {
                let actual = native.invoke(&args, fuel);
                match (&actual, function.interpret(&args, fuel)) {
                    (Ok(lhs), Ok(rhs)) => assert_eq!(*lhs, rhs),
                    (
                        Err(EngineError::Trap { kind, instruction }),
                        Err(EngineError::Trap {
                            kind: rhs_kind,
                            instruction: rhs_pc,
                        }),
                    ) => {
                        assert_eq!((*kind, *instruction), (rhs_kind, rhs_pc));
                    }
                    other => panic!("scalar/native mismatch: {other:?}"),
                }
                assert_eq!(
                    native_outcome(actual),
                    vm_outcome(&program, index, &args, fuel)
                );
            }
        }

        // The bytecode verifier requires a final RETURN, even when unreachable.
        let program = fixture(0, &[(Op::Jump, 0), (Op::PushInt, 0), (Op::Return, 0)]);
        let native = NativeFunction::compile(&scalar(&program, 0)).unwrap();
        assert!(matches!(
            native.invoke(&[], 10_000),
            Err(EngineError::Trap {
                kind: TrapKind::FuelExhausted,
                instruction: 0
            })
        ));
    }

    // Pop must not move the wrong slot, unreachable PCs must not execute and
    // each invocation must allocate a fresh zero-initialized local frame.
    #[test]
    fn pop_unreachable_code_and_repeated_trap_recovery() {
        let program = fixture(
            0,
            &[
                (Op::Jump, 3),
                (Op::PushInt, 99),
                (Op::Return, 0),
                (Op::PushInt, -7),
                (Op::Pop, 0),
                (Op::PushInt, 42),
                (Op::Return, 0),
            ],
        );
        let function = scalar(&program, 0);
        assert_eq!(function.stack_heights()[1], None);
        let native = NativeFunction::compile(&function).unwrap();
        assert_eq!(native.invoke(&[], 5).unwrap(), 42);
        assert_eq!(vm_outcome(&program, 0, &[], 5), Outcome::Value(42));

        let program = fixture(
            1,
            &[
                (Op::Load, 0),
                (Op::JumpIfZero, 6),
                (Op::PushInt, 123),
                (Op::Store, 4095),
                (Op::Load, 4095),
                (Op::Return, 0),
                (Op::Load, 4095),
                (Op::Return, 0),
            ],
        );
        let native = NativeFunction::compile(&scalar(&program, 0)).unwrap();
        assert_eq!(native.invoke(&[1], 100).unwrap(), 123);
        assert_eq!(native.invoke(&[0], 100).unwrap(), 0);
        assert_eq!(native.invoke(&[1], 100).unwrap(), 123);
        assert_eq!(native.invoke(&[0], 100).unwrap(), 0);

        let program =
            compile_source_unoptimized_verified("fn divide(a, b) { let c = a / b return c + 1 }")
                .unwrap();
        let native = NativeFunction::compile(&scalar(&program, 0)).unwrap();
        for _ in 0..32 {
            assert_eq!(native.invoke(&[20, 4], 100).unwrap(), 6);
            assert!(matches!(
                native.invoke(&[20, 0], 100),
                Err(EngineError::Trap {
                    kind: TrapKind::DivisionByZero,
                    ..
                })
            ));
            assert!(matches!(
                native.invoke(&[i64::MIN, -1], 100),
                Err(EngineError::Trap {
                    kind: TrapKind::Overflow,
                    ..
                })
            ));
            assert!(matches!(
                native.invoke(&[20, 4], 0),
                Err(EngineError::Trap {
                    kind: TrapKind::FuelExhausted,
                    ..
                })
            ));
            assert_eq!(native.invoke(&[-20, 4], 100).unwrap(), -4);
        }
    }

    #[repr(C)]
    struct MemoryBasicInformation {
        base_address: *mut c_void,
        allocation_base: *mut c_void,
        allocation_protect: u32,
        partition_id: u16,
        region_size: usize,
        state: u32,
        protect: u32,
        kind: u32,
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn VirtualQuery(
            address: *const c_void,
            information: *mut MemoryBasicInformation,
            length: usize,
        ) -> usize;
    }

    // Protection is inspected at the actual entry-point mapping, not asserted
    // from source text; executable pages must not remain writable.
    #[test]
    fn published_machine_code_is_read_execute_not_writable() {
        let program = fixture(0, &[(Op::PushInt, 42), (Op::Return, 0)]);
        let native = NativeFunction::compile(&scalar(&program, 0)).unwrap();
        let mut information = std::mem::MaybeUninit::<MemoryBasicInformation>::uninit();
        // SAFETY: VirtualQuery writes this correctly sized Windows x64 struct;
        // the queried address belongs to a live NativeFunction.
        let written = unsafe {
            VirtualQuery(
                native.code_bytes().as_ptr().cast(),
                information.as_mut_ptr(),
                std::mem::size_of::<MemoryBasicInformation>(),
            )
        };
        assert_eq!(written, std::mem::size_of::<MemoryBasicInformation>());
        // SAFETY: VirtualQuery succeeded and initialized the complete struct.
        let information = unsafe { information.assume_init() };
        assert_eq!(information.state, 0x1000); // MEM_COMMIT
        assert_eq!(information.protect, 0x20); // PAGE_EXECUTE_READ
        assert_eq!(information.allocation_protect, 0x04); // Originally PAGE_READWRITE
        assert_eq!(native.invoke(&[], 2).unwrap(), 42);
    }
}

#[cfg(not(all(target_arch = "x86_64", target_os = "windows")))]
#[test]
fn unsupported_target_is_explicitly_unavailable() {
    let function =
        tinylang_v2::engine::compile_scalar_source("fn answer() { return 42 }", "answer").unwrap();
    assert!(matches!(
        NativeFunction::compile(&function),
        Err(EngineError::BackendUnavailable {
            backend: "x86-64 ASM",
            ..
        })
    ));
}
