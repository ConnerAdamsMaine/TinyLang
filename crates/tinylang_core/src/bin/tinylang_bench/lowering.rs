//! Construction scaling, with verification and output checks outside timing.

#[path = "../../../tests/preparation/fixtures.rs"]
mod fixtures;

use fixtures::LARGE_CASES;

use super::{Benchmark, JitCache, JitProgram, bench, compile_jit_verified, leak_name, run_verified_program};

pub(super) fn benchmarks() -> Vec<Benchmark> {
    LARGE_CASES
        .into_iter()
        .map(|(label, fixture, iterations)| {
            let (verified, expected) = fixture.prepare();
            let mut reference = Vec::new();
            drop(run_verified_program(&verified, "vm", &mut reference, Vec::new()).expect("scaling VM reference"));
            assert_eq!(reference, expected.as_bytes(), "{label}");
            let mut compiled = JitProgram::compile_verified(&verified).expect("scaling parity lowering");
            let mut output = Vec::with_capacity(reference.len());
            drop(compiled.run(&mut output, Vec::new()).expect("scaling JIT reference"));
            assert_eq!(output, reference, "{label}");
            // The checked code above is discarded. Samples each construct and
            // drop a fresh cache; only immutable verified input is shared.
            drop(compiled);
            let name = leak_name(format!("jit.codegen_{label}_cold"));
            bench(name, iterations, move || {
                let mut cache = JitCache::new();
                compile_jit_verified(&verified, &mut cache);
            })
        })
        .collect()
}
