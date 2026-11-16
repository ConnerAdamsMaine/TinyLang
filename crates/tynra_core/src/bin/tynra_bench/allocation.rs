//! Retained-object scaling with fresh execution heaps and untimed output checks.

#[path = "../../../tests/allocation/retained.rs"]
mod retained;

use retained::{COUNTS, OBJECTS};

use super::{Benchmark, JitProgram, bench, leak_name, run_compiled_jit, run_verified_mode, run_verified_program};

pub(super) fn benchmarks() -> Vec<Benchmark> {
    let mut benchmarks = Vec::new();
    for object in OBJECTS {
        for count in COUNTS {
            let (verified, expected) = object.prepare(count);
            let mut reference = Vec::new();
            drop(run_verified_program(&verified, "vm", &mut reference, Vec::new()).expect("retained-object VM"));
            assert_eq!(reference, expected.as_bytes(), "{object:?}/{count}");
            let mut compiled = JitProgram::compile_verified(&verified).expect("retained-object lowering");
            let mut output = Vec::with_capacity(reference.len());
            drop(compiled.run(&mut output, Vec::new()).expect("retained-object JIT"));
            assert_eq!(output, reference, "{object:?}/{count}");
            // These rows reuse code, never execution heaps. Every timed run
            // includes construction, retained reads and shutdown cleanup.
            let iterations = match count {
                0..=16 => 1000,
                17..=256 => 200,
                _ => 5,
            };
            let label = object.label();
            let vm_input = verified.clone();
            benchmarks.push(bench(leak_name(format!("runtime.vm_retained_{label}_{count}")), iterations, move || {
                run_verified_mode(&vm_input, "vm", Vec::new());
            }));
            benchmarks.push(bench(leak_name(format!("runtime.jit_retained_{label}_{count}")), iterations, move || {
                run_compiled_jit(&mut compiled, Vec::new());
            }));
            if count == 256 {
                benchmarks.push(bench(
                    leak_name(format!("jit.cold_run_retained_{label}_{count}")),
                    iterations,
                    move || {
                        let mut compiled =
                            JitProgram::compile_verified(&verified).expect("fresh retained-object lowering");
                        run_compiled_jit(&mut compiled, Vec::new());
                    },
                ));
            }
        }
    }
    benchmarks
}
