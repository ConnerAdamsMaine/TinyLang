use tinylang::{RuntimeValue as Value, VM, VmExecutionLimits, compile_source_unoptimized_verified};
use tinylang_v2::engine::{ExecutionBackend, ScalarFunction};

fn compare(source: &str, args: &[i64], expected: i64) {
    let program = compile_source_unoptimized_verified(source).unwrap();
    let function = ScalarFunction::lower(&program, 0).unwrap();
    let mut vm =
        VM::from_verified_with_limits(&program, vec![], VmExecutionLimits::default()).unwrap();
    assert_eq!(
        vm.invoke_function_with_fuel(
            0,
            args.iter().copied().map(Value::I64).collect(),
            &mut Vec::new(),
            10000,
        )
        .unwrap(),
        Value::I64(expected),
    );
    for backend in [ExecutionBackend::Interpreter, ExecutionBackend::NativeAsm] {
        let executable = backend.prepare(&function).unwrap();
        assert_eq!(
            executable.invoke(args, 10000).unwrap(),
            expected,
            "{backend:?}"
        );
    }
    #[cfg(feature = "cranelift")]
    {
        let executable = ExecutionBackend::Cranelift.prepare(&function).unwrap();
        assert_eq!(
            executable.invoke(args, 10000).unwrap(),
            expected,
            "Cranelift"
        );
    }
}

#[test]
fn public_execution_selection_runs_actual_compiler_output() {
    compare("fn add(a,b) { return a+b }", &[19, 23], 42);
    compare(
        "fn choose(a,b) { if a < b { return a } else { return b } }",
        &[-9, 4],
        -9,
    );
    compare(
        "fn sum(n) { let t=0 let i=0 while i<n { t=t+i i=i+1 } return t }",
        &[10],
        45,
    );
    compare(
        "fn identity<T>(value: T) -> T { return value }",
        &[-42],
        -42,
    );
    compare(
        "fn predicate(a,b) { let flag=a<b if flag { return 1 } else { return 0 } }",
        &[-9, 4],
        1,
    );
    compare(
        "fn overwritten(a,b) { let x=0 if a<b { x=a<b } x=42 return x }",
        &[-9, 4],
        42,
    );
}

#[test]
fn generated_integer_functions_agree_across_backends() {
    // Deterministic small programs and bounded inputs avoid UB or unbounded work.
    for seed in 0i64..64 {
        let multiplier = seed % 7 + 1;
        let constant = seed % 11 - 5;
        let source = format!(
            "fn calc(a,b) {{ let x=a*{multiplier}+({constant})
                if x < b {{ return b-x }} else {{ return x-b }} }}"
        );
        let a = seed - 32;
        let b = seed % 13 - 6;
        let x = a * multiplier + constant;
        compare(&source, &[a, b], (x - b).abs());
    }
}

#[test]
fn verified_compiler_artifacts_retain_native_execution_semantics() {
    let compiled = compile_source_unoptimized_verified(
        "fn difference(a,b) { if a>b { return a-b } else { return b-a } }",
    )
    .unwrap();
    let bytes = compiled.program().to_binary_artifact().unwrap();
    let loaded = tinylang::VerifiedProgram::from_binary_artifact(&bytes).unwrap();
    let scalar = ScalarFunction::lower(&loaded, 0).unwrap();
    for backend in [ExecutionBackend::Interpreter, ExecutionBackend::NativeAsm] {
        let executable = backend.prepare(&scalar).unwrap();
        for (args, expected) in [([19, 23], 4), ([-9, -4], 5), ([0, 0], 0)] {
            assert_eq!(executable.invoke(&args, 100).unwrap(), expected);
        }
    }
}

#[test]
fn executable_reports_fuel_and_arithmetic_traps_without_poisoning_reuse() {
    use tinylang_v2::engine::{EngineError, TrapKind};
    let program = compile_source_unoptimized_verified("fn add(a,b) { return a+b }").unwrap();
    let function = ScalarFunction::lower(&program, 0).unwrap();
    for backend in [ExecutionBackend::Interpreter, ExecutionBackend::NativeAsm] {
        let executable = backend.prepare(&function).unwrap();
        assert!(matches!(
            executable.invoke(&[1, 2], 3),
            Err(EngineError::Trap {
                kind: TrapKind::FuelExhausted,
                instruction: 3
            })
        ));
        assert!(matches!(
            executable.invoke(&[i64::MAX, 1], 100),
            Err(EngineError::Trap {
                kind: TrapKind::Overflow,
                instruction: 2
            })
        ));
        assert_eq!(executable.invoke(&[19, 23], 4).unwrap(), 42);
    }
}

#[test]
#[cfg(not(feature = "cranelift"))]
fn disabled_backend_never_silently_uses_an_interpreter() {
    let program = compile_source_unoptimized_verified("fn answer() { return 42 }").unwrap();
    let function = ScalarFunction::lower(&program, 0).unwrap();
    assert!(matches!(
        ExecutionBackend::Cranelift.prepare(&function),
        Err(tinylang_v2::engine::EngineError::BackendUnavailable {
            backend: "cranelift",
            ..
        })
    ));
}
