use std::hint::black_box;
use std::time::Instant;

use tinylang_v2::engine::{ExecutionBackend, ScalarFunction};
use tinyone::{RuntimeValue, VM, VmExecutionLimits, compile_source_unoptimized_verified};

const SOURCE: &str = "fn sum(n) { let t=0 let i=0 while i<n { t=t+i i=i+1 } return t }";
const SAMPLES: usize = 101;
const BATCH: usize = 256;

fn measure(
    metric: &str,
    batch: usize,
    mut operation: impl FnMut() -> Result<(), Box<dyn std::error::Error>>,
) -> Result<(), Box<dyn std::error::Error>> {
    for _ in 0..8 {
        operation()?;
    }
    let mut samples = Vec::with_capacity(SAMPLES);
    for _ in 0..SAMPLES {
        let start = Instant::now();
        for _ in 0..batch {
            operation()?;
        }
        samples.push(start.elapsed().as_nanos() / batch as u128);
    }
    samples.sort_unstable();
    println!(
        "{metric},{},{},{},{SAMPLES},{batch}",
        samples[SAMPLES / 2],
        samples[95],
        samples[99]
    );
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let program = compile_source_unoptimized_verified(SOURCE)?;
    let scalar = ScalarFunction::lower(&program, 0)?;
    let mut vm = VM::from_verified_with_limits(&program, vec![], VmExecutionLimits::default())?;
    println!("metric,median_ns,p95_ns,p99_ns,samples,batch");
    measure("lex", BATCH, || {
        black_box(tinyone::lex_source(black_box(SOURCE))?);
        Ok(())
    })?;
    measure("frontend_and_lower", 1, || {
        let program = compile_source_unoptimized_verified(black_box(SOURCE))?;
        black_box(ScalarFunction::lower(&program, 0)?);
        Ok(())
    })?;
    measure("scalar_lower", BATCH, || {
        black_box(ScalarFunction::lower(black_box(&program), 0)?);
        Ok(())
    })?;
    measure("vm_warm_sum100", BATCH, || {
        let value = vm.invoke_function_with_fuel(
            0,
            vec![RuntimeValue::I64(black_box(100))],
            &mut Vec::new(),
            10000,
        )?;
        if value != RuntimeValue::I64(4950) {
            return Err("reference VM produced an incorrect result".into());
        }
        black_box(value);
        Ok(())
    })?;
    let backends = [
        ("scalar_interpreter", ExecutionBackend::Interpreter),
        ("asm", ExecutionBackend::NativeAsm),
        #[cfg(feature = "cranelift")]
        ("cranelift", ExecutionBackend::Cranelift),
    ];
    for (name, backend) in backends {
        measure(&format!("{name}_prepare"), 1, || {
            black_box(backend.prepare(black_box(&scalar))?);
            Ok(())
        })?;
        let executable = backend.prepare(&scalar)?;
        measure(&format!("{name}_warm_sum100"), BATCH, || {
            let result = executable.invoke(black_box(&[100]), 10000)?;
            if result != 4950 {
                return Err("backend produced an incorrect result".into());
            }
            black_box(result);
            Ok(())
        })?;
    }
    Ok(())
}
