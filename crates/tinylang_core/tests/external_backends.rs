#[cfg(feature = "cranelift")]
use tinylang::engine::EngineError;
use tinylang::engine::llvm::emit_llvm_ir;
use tinylang::engine::{BinaryOp, Instruction, ScalarFunction, TrapKind};
use tinylang::{RuntimeValue, VM, VmExecutionLimits, compile_source_unoptimized_verified};

#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    Value(i64),
    Trap(TrapKind),
}

fn scalar(source: &str) -> ScalarFunction {
    tinylang::engine::compile_scalar_source(source, "test").unwrap()
}

fn vm_outcome(source: &str, args: &[i64], fuel: u64) -> Outcome {
    let verified = compile_source_unoptimized_verified(source).unwrap();
    let index = verified
        .program()
        .functions()
        .iter()
        .position(|function| function.name() == "test")
        .unwrap();
    // This checkout's persistent verified constructor explicitly takes limits.
    let mut vm = VM::from_verified_with_limits(&verified, Vec::new(), VmExecutionLimits::default()).unwrap();
    match vm.invoke_function_with_fuel(
        index,
        args.iter().copied().map(RuntimeValue::I64).collect(),
        &mut Vec::new(),
        fuel,
    ) {
        Ok(RuntimeValue::I64(value)) => Outcome::Value(value),
        Ok(value) => panic!("unexpected scalar VM value: {value:?}"),
        Err(error) => {
            let message = error.to_string();
            if message.contains("Runtime.Memory_Overflow") || message.contains("Division overflow") {
                Outcome::Trap(TrapKind::Overflow)
            } else if message.contains("Division by zero") {
                Outcome::Trap(TrapKind::DivisionByZero)
            } else if message.contains("fuel") {
                Outcome::Trap(TrapKind::FuelExhausted)
            } else {
                panic!("unexpected VM failure: {message}");
            }
        }
    }
}

#[cfg(feature = "cranelift")]
fn engine_outcome(result: Result<i64, EngineError>) -> Outcome {
    match result {
        Ok(value) => Outcome::Value(value),
        Err(EngineError::Trap { kind, .. }) => Outcome::Trap(kind),
        Err(error) => panic!("unexpected backend failure: {error}"),
    }
}

const ARITHMETIC: &[(&str, &[i64], i64)] = &[
    ("fn test(a, b) { return a + b }", &[19, 23], 42),
    ("fn test(a, b) { return a - b }", &[-19, 23], -42),
    ("fn test(a, b) { return a * b }", &[-6, 7], -42),
    ("fn test(a, b) { return a / b }", &[-85, 2], -43),
    ("fn test(a, b) { return a / b }", &[85, -2], -43),
    ("fn test(a, b) { return a / b }", &[-85, -2], 42),
    ("fn test(a, b) { return a / b }", &[85, 2], 42),
    ("fn test(a, b) { return a / b }", &[i64::MIN, 1], i64::MIN),
    ("fn test(a, b) { return a / b }", &[i64::MIN, 3], -3074457345618258603),
    ("fn test(a, b) { return a / b }", &[i64::MAX, i64::MIN], -1),
    ("fn test(a, b) { return a / b }", &[i64::MIN, i64::MAX], -2),
    ("fn test(a) { return -a }", &[-42], 42),
    ("fn test(a, b) { if a < b { return 1 } else { return 0 } }", &[i64::MIN, i64::MAX], 1),
    ("fn test(a, b) { if a <= b { return 1 } else { return 0 } }", &[7, 7], 1),
    ("fn test(a, b) { if a > b { return 1 } else { return 0 } }", &[i64::MIN, i64::MAX], 0),
    ("fn test(a, b) { if a >= b { return 1 } else { return 0 } }", &[7, 7], 1),
    ("fn test(a, b) { if a == b { return 1 } else { return 0 } }", &[7, 7], 1),
    ("fn test(a, b) { if a != b { return 1 } else { return 0 } }", &[7, 7], 0),
    ("fn test() { return 42 }", &[], 42),
];

const LOOP: &str = "fn test(n) { let sum = 0 while n > 0 { sum = sum + n n = n - 1 } return sum }";

const TRAPS: &[(&str, &[i64], TrapKind, Instruction)] = &[
    (
        "fn test(a, b) { return a + b }",
        &[i64::MAX, 1],
        TrapKind::Overflow,
        Instruction::Binary(BinaryOp::Add),
    ),
    (
        "fn test(a, b) { return a - b }",
        &[i64::MIN, 1],
        TrapKind::Overflow,
        Instruction::Binary(BinaryOp::Subtract),
    ),
    (
        "fn test(a, b) { return a * b }",
        &[i64::MAX, 2],
        TrapKind::Overflow,
        Instruction::Binary(BinaryOp::Multiply),
    ),
    (
        "fn test(a, b) { return a / b }",
        &[1, 0],
        TrapKind::DivisionByZero,
        Instruction::Binary(BinaryOp::Divide),
    ),
    (
        "fn test(a, b) { return a / b }",
        &[i64::MIN, -1],
        TrapKind::Overflow,
        Instruction::Binary(BinaryOp::Divide),
    ),
    ("fn test(a) { return -a }", &[i64::MIN], TrapKind::Overflow, Instruction::Negate),
];

#[cfg(feature = "cranelift")]
mod jit {
    use tinylang::engine::cranelift::CraneliftFunction;

    use super::*;

    #[test]
    fn cranelift_executes_real_source_and_matches_authoritative_vm() {
        for &(source, args, expected) in ARITHMETIC {
            let jit = CraneliftFunction::compile(&scalar(source)).unwrap();
            assert_eq!(jit.invoke(args, u64::MAX).unwrap(), expected, "{source}");
            assert_eq!(engine_outcome(jit.invoke(args, 100)), vm_outcome(source, args, 100));
        }
        let jit = CraneliftFunction::compile(&scalar(LOOP)).unwrap();
        for (n, expected) in [(0, 0), (1, 1), (9, 45), (-1, 0)] {
            assert_eq!(jit.invoke(&[n], 1000).unwrap(), expected);
            assert_eq!(engine_outcome(jit.invoke(&[n], 1000)), vm_outcome(LOOP, &[n], 1000));
        }
    }

    #[test]
    fn cranelift_traps_are_checked_and_located_before_work() {
        for &(source, args, kind, operation) in TRAPS {
            let function = scalar(source);
            let pc = function.instructions().iter().position(|op| *op == operation).unwrap();
            let jit = CraneliftFunction::compile(&function).unwrap();
            assert!(matches!(jit.invoke(args, 100),
                Err(EngineError::Trap { kind: actual, instruction }) if actual == kind && instruction == pc));
            assert!(matches!(jit.invoke(args, pc as u64),
                Err(EngineError::Trap { kind: TrapKind::FuelExhausted, instruction }) if instruction == pc));
            assert_eq!(engine_outcome(jit.invoke(args, 100)), vm_outcome(source, args, 100));
        }
    }

    #[test]
    fn cranelift_fuel_tracks_each_source_instruction_including_back_edges() {
        let jit = CraneliftFunction::compile(&scalar(LOOP)).unwrap();
        for fuel in 0..150 {
            assert_eq!(engine_outcome(jit.invoke(&[3], fuel)), vm_outcome(LOOP, &[3], fuel), "fuel={fuel}");
        }
        let forever = scalar("fn test() { while 1 { } return 0 }");
        let jit = CraneliftFunction::compile(&forever).unwrap();
        assert!(matches!(
            jit.invoke(&[], 23),
            Err(EngineError::Trap {
                kind: TrapKind::FuelExhausted,
                ..
            })
        ));
    }

    #[test]
    fn cranelift_arity_and_module_lifetime_are_owned() {
        for _ in 0..32 {
            let jit = CraneliftFunction::compile(&scalar("fn test(a) { return a + 1 }")).unwrap();
            assert!(matches!(
                jit.invoke(&[], 100),
                Err(EngineError::InvalidArguments {
                    expected: 1,
                    actual:   0,
                })
            ));
            assert!(matches!(
                jit.invoke(&[1, 2], 100),
                Err(EngineError::InvalidArguments {
                    expected: 1,
                    actual:   2,
                })
            ));
            for n in -8..8 {
                assert_eq!(jit.invoke(&[n], 100).unwrap(), n + 1);
            }
        }
    }
}

#[test]
fn llvm_emits_all_scalar_operations_and_reachable_control_flow() {
    for &(source, _, _) in ARITHMETIC {
        assert!(!emit_llvm_ir(&scalar(source)).unwrap().is_empty());
    }
    assert!(!emit_llvm_ir(&scalar(LOOP)).unwrap().is_empty());
    // The return after the first return is unreachable and must not create an invalid block.
    assert!(
        !emit_llvm_ir(&scalar("fn test() { return 42 return 7 }"))
            .unwrap()
            .is_empty()
    );
}

mod llvm_execution {
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    fn clang() -> Option<PathBuf> {
        if let Some(path) = std::env::var_os("TINYLANG_LLVM_CLANG") {
            return Some(path.into());
        }
        for path in [
            "clang",
            "clang.exe",
            "C:\\Program Files\\LLVM\\bin\\clang.exe",
            "C:\\Program Files\\Microsoft Visual Studio\\18\\Community\\VC\\Tools\\Llvm\\x64\\bin\\clang.exe",
            "C:\\Program Files\\Microsoft Visual Studio\\2022\\Community\\VC\\Tools\\Llvm\\x64\\bin\\clang.exe",
        ] {
            if Command::new(path)
                .arg("--version")
                .output()
                .is_ok_and(|out| out.status.success())
            {
                return Some(path.into());
            }
        }
        None
    }

    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "tinylang-llvm-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed),
            ));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }

    fn compile(clang: &Path, function: &ScalarFunction, scratch: &Scratch) -> PathBuf {
        let ir = scratch.0.join("function.ll");
        let driver = scratch.0.join("driver.c");
        let binary = scratch.0.join(if cfg!(windows) { "run.exe" } else { "run" });
        std::fs::write(&ir, emit_llvm_ir(function).unwrap()).unwrap();
        let c = format!(
            r#"
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <inttypes.h>
extern uint32_t tinylang_scalar(int64_t*, int64_t*, int64_t*, uint64_t*, uint64_t*);
int main(int argc, char **argv) {{
    int64_t slots[{slots}] = {{0}}, stack[{stack}] = {{0}}, result = 0;
    uint64_t fuel = strtoull(argv[1], 0, 10), pc = UINT64_MAX;
    if (argc != {argc}) return 99;
    for (int i = 2; i < argc; ++i) slots[i - 2] = strtoll(argv[i], 0, 10);
    uint32_t status = tinylang_scalar(slots, stack, &result, &fuel, &pc);
    printf("%" PRIu32 " %" PRId64 " %" PRIu64 " %" PRIu64, status, result, fuel, pc);
    return 0;
}}
"#,
            slots = function.slot_count().max(1),
            stack = function.max_stack().max(1),
            argc = function.parameter_count() + 2
        );
        std::fs::write(&driver, c).unwrap();
        let output = Command::new(clang)
            .arg("-O2")
            .arg(&ir)
            .arg(&driver)
            .arg("-o")
            .arg(&binary)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "LLVM validation/compilation failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        binary
    }

    fn run(binary: &Path, args: &[i64], fuel: u64) -> (Outcome, u64, u64) {
        let output = Command::new(binary)
            .arg(fuel.to_string())
            .args(args.iter().map(ToString::to_string))
            .output()
            .unwrap();
        assert!(output.status.success(), "LLVM executable failed: {output:?}");
        let output = String::from_utf8(output.stdout).unwrap();
        let parts: Vec<_> = output.split_whitespace().collect();
        let outcome = match parts[0] {
            "0" => Outcome::Value(parts[1].parse().unwrap()),
            "1" => Outcome::Trap(TrapKind::Overflow),
            "2" => Outcome::Trap(TrapKind::DivisionByZero),
            "3" => Outcome::Trap(TrapKind::FuelExhausted),
            status => panic!("unexpected generated ABI status {status}"),
        };
        (outcome, parts[2].parse().unwrap(), parts[3].parse().unwrap())
    }

    #[test]
    #[ignore = "requires external clang; run with --ignored"]
    fn llvm_compiles_and_executes_against_vm() {
        let clang = clang()
            .expect("LLVM execution validation requires external clang; set TINYLANG_LLVM_CLANG or put clang on PATH");
        for &(source, args, expected) in ARITHMETIC {
            let scratch = Scratch::new();
            let function = scalar(source);
            let binary = compile(&clang, &function, &scratch);
            assert_eq!(run(&binary, args, 100).0, Outcome::Value(expected));
            assert_eq!(run(&binary, args, 100).0, vm_outcome(source, args, 100));
            for fuel in 0..function.instructions().len() as u64 {
                assert_eq!(run(&binary, args, fuel).0, vm_outcome(source, args, fuel));
            }
            assert_eq!(run(&binary, args, u64::MAX).0, Outcome::Value(expected));
        }
        for &(source, args, kind, operation) in TRAPS {
            let scratch = Scratch::new();
            let function = scalar(source);
            let pc = function.instructions().iter().position(|op| *op == operation).unwrap();
            let binary = compile(&clang, &function, &scratch);
            let (actual, remaining, instruction) = run(&binary, args, 100);
            assert_eq!(actual, Outcome::Trap(kind));
            assert_eq!(instruction, pc as u64);
            assert_eq!(remaining, 100 - pc as u64 - 1);
            assert_eq!(run(&binary, args, pc as u64), (Outcome::Trap(TrapKind::FuelExhausted), 0, pc as u64));
            assert_eq!(actual, vm_outcome(source, args, 100));
        }
        let scratch = Scratch::new();
        let binary = compile(&clang, &scalar(LOOP), &scratch);
        for fuel in 0..100 {
            assert_eq!(run(&binary, &[3], fuel).0, vm_outcome(LOOP, &[3], fuel));
        }
    }
}
