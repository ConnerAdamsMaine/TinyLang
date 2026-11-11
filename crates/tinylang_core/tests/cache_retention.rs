use std::collections::HashMap;

use tinylang::{
    DEFAULT_COMPILED_CACHE_MAX_BYTES, DEFAULT_COMPILED_CACHE_MAX_ENTRIES, Instr, JitCache, JitOptions, JitProgram, Op,
    Program, VerifiedProgram, compile_source_verified,
};

fn constant_program(value: i64, capacity: usize) -> VerifiedProgram {
    let mut code = Vec::with_capacity(capacity);
    code.extend([
        Instr::new(Op::PushInt, value, 0),
        Instr::new(Op::Print, 0, 0),
        Instr::new(Op::Halt, 0, 0),
    ]);
    VerifiedProgram::verify(Program::new(code, 0)).expect("verified cache fixture")
}

fn run(cache: &mut JitCache, program: &VerifiedProgram, inputs: Vec<String>) -> Vec<u8> {
    let mut output = Vec::new();
    cache
        .run_verified_program(program, &mut output, inputs)
        .expect("execution");
    let stats = cache.compiled_cache_stats();
    assert!(stats.programs <= cache.compiled_cache_max_entries());
    assert!(stats.retained_bytes <= cache.compiled_cache_byte_limit());
    assert_eq!(stats.transient_bytes, 0);
    output
}

#[test]
fn default_compiled_cache_does_not_retain_every_distinct_program() {
    let mut cache = JitCache::new();
    assert_eq!(cache.compiled_cache_max_entries(), DEFAULT_COMPILED_CACHE_MAX_ENTRIES);
    assert_eq!(cache.compiled_cache_byte_limit(), DEFAULT_COMPILED_CACHE_MAX_BYTES);
    for value in 0..257 {
        let verified = VerifiedProgram::verify(Program::new(
            vec![
                Instr::new(Op::PushInt, value, 0),
                Instr::new(Op::Print, 0, 0),
                Instr::new(Op::Halt, 0, 0),
            ],
            0,
        ))
        .expect("verified cache fixture");
        let mut output = Vec::new();
        cache
            .run_verified_program(&verified, &mut output, Vec::new())
            .expect("execution");
        assert_eq!(output, format!("{value}\n").as_bytes());
        assert!(cache.compiled_cache_stats().retained_bytes <= DEFAULT_COMPILED_CACHE_MAX_BYTES);
    }
    assert_eq!(cache.len(), DEFAULT_COMPILED_CACHE_MAX_ENTRIES);
    assert_eq!(cache.compiled_cache_stats().evictions, 129);
    assert_eq!(cache.compiled_cache_stats().compilations, 257);
    assert_eq!(cache.source_cache_len(), 0);
}

#[test]
fn entry_and_byte_limits_both_evict_the_least_recently_used_program() {
    let first = constant_program(1, 3);
    let second = constant_program(2, 3);
    let third = constant_program(3, 3);
    let mut measured = JitCache::new();
    measured.compile_verified(&first).expect("lowering");
    let entry_bytes = measured.compiled_cache_stats().retained_bytes;

    for (entries, bytes) in [(2, usize::MAX), (128, entry_bytes * 2)] {
        let mut cache = JitCache::new().with_compiled_cache_limits(entries, bytes);
        for program in [&first, &second, &first, &third] {
            cache.compile_verified(program).expect("lowering");
            assert!(cache.compiled_cache_stats().retained_bytes <= bytes);
        }
        assert_eq!(cache.len(), 2);
        assert_eq!(cache.compiled_cache_stats().evictions, 1);
        assert_eq!(cache.compiled_cache_stats().compilations, 3);
        cache.compile_verified(&first).expect("retained first");
        cache.compile_verified(&third).expect("retained third");
        assert_eq!(cache.compiled_cache_stats().compilations, 3);
        cache.compile_verified(&second).expect("evicted second recompiles");
        assert_eq!(cache.compiled_cache_stats().compilations, 4);
        assert_eq!(cache.compiled_cache_stats().evictions, 2);
    }
}

#[test]
fn unused_bytecode_capacity_is_charged_and_oversized_runs_preserve_hot_entries() {
    let compact = constant_program(7, 3);
    let roomy = constant_program(7, 8192);
    assert_eq!(compact.fingerprint(), roomy.fingerprint());
    let compact_bytes = JitProgram::compile_verified(&compact)
        .expect("compact")
        .retained_bytes();
    let roomy_bytes = JitProgram::compile_verified(&roomy).expect("roomy").retained_bytes();
    assert!(roomy_bytes - compact_bytes >= (8192 - 3) * size_of::<Instr>());

    let small = constant_program(1, 3);
    let mut cache = JitCache::new().with_compiled_cache_limits(8, roomy_bytes / 2);
    assert_eq!(run(&mut cache, &small, Vec::new()), b"1\n");
    let retained = cache.compiled_cache_stats().retained_bytes;
    for _ in 0..2 {
        assert_eq!(run(&mut cache, &roomy, Vec::new()), b"7\n");
        assert_eq!(cache.compiled_cache_stats().retained_bytes, retained);
        assert_eq!(cache.len(), 1);
    }
    assert_eq!(cache.compiled_cache_stats().bypasses, 2);
    assert_eq!(cache.compiled_cache_stats().evictions, 0);
    assert_eq!(run(&mut cache, &small, Vec::new()), b"1\n");
    assert_eq!(cache.compiled_cache_stats().hits, 1);
}

#[test]
fn source_and_compiled_retention_are_independently_disabled() {
    for (entries, bytes) in [(0, 4096), (4, 0), (0, 0)] {
        let mut cache = JitCache::new().with_compiled_cache_limits(entries, bytes);
        for _ in 0..3 {
            let mut output = Vec::new();
            cache
                .run_source("print 42", &mut output, Vec::new())
                .expect("uncached JIT run");
            assert_eq!(output, b"42\n");
        }
        assert!(cache.is_empty());
        assert_eq!(cache.compiled_cache_max_entries(), 0);
        assert_eq!(cache.compiled_cache_byte_limit(), 0);
        assert_eq!(cache.compiled_cache_stats().retained_bytes, 0);
        assert_eq!(cache.compiled_cache_stats().transient_bytes, 0);
        assert_eq!(cache.compiled_cache_stats().table_capacity, 0);
        assert_eq!(cache.compiled_cache_stats().compilations, 3);
        assert_eq!(cache.compiled_cache_stats().bypasses, 3);
        assert_eq!(cache.source_cache_len(), 1);
        assert_eq!(cache.stats().source_hits, 2);
    }
    let mut cache = JitCache::new().with_source_cache_limits(0, 0);
    for _ in 0..3 {
        cache
            .run_source("print 42", &mut Vec::new(), Vec::new())
            .expect("source bypass");
    }
    assert_eq!(cache.source_cache_len(), 0);
    assert_eq!(cache.compiled_cache_stats().programs, 1);
    assert_eq!(cache.compiled_cache_stats().compilations, 1);
    assert_eq!(cache.compiled_cache_stats().hits, 2);
}

#[test]
fn borrowed_bypass_results_are_separate_non_reusable_and_not_cloned() {
    let program = constant_program(42, 3);
    let mut cache = JitCache::new().with_compiled_cache_limits(8, 1);
    for _ in 0..2 {
        let result = cache.compile_verified(&program).expect("borrowed bypass result");
        assert_eq!(result.fingerprint(), program.fingerprint());
        let result_bytes = result.retained_bytes();
        assert_eq!(cache.compiled_cache_stats().transient_bytes, result_bytes);
        assert!(cache.is_empty());
        assert_eq!(cache.compiled_cache_stats().retained_bytes, 0);
    }
    assert_eq!(cache.compiled_cache_stats().compilations, 2);
    assert_eq!(cache.compiled_cache_stats().hits, 0);
    assert_eq!(cache.clone().compiled_cache_stats().transient_bytes, 0);
    assert_eq!(run(&mut cache, &program, Vec::new()), b"42\n");
    cache.compile_verified(&program).expect("new bypass result");
    assert!(cache.run_source("let =", &mut Vec::new(), Vec::new()).is_err());
    assert_eq!(cache.compiled_cache_stats().transient_bytes, 0);
    cache.compile_verified(&program).expect("new bypass result");
    let invalid = Program::new(vec![Instr::new(Op::Pop, 0, 0)], 0);
    assert!(cache.compile(&invalid).is_err());
    assert_eq!(cache.compiled_cache_stats().transient_bytes, 0);
}

#[test]
fn lazy_compilation_growth_is_accounted_on_success_and_error() {
    for tail in ["return value", "return value / 0"] {
        let source = format!(
            "fn lazy(value) {{ let i = 0 while i < 64 {{ value = value * 1 i = i + 1 }} {tail} }} \
             if read_int() {{ print lazy(9) }} else {{ print 1 }}"
        );
        let program = compile_source_verified(&source).expect("lazy fixture");
        let options = JitOptions::new().with_hot_back_edge_threshold(1);
        let mut cache = JitCache::with_options(options);
        assert_eq!(run(&mut cache, &program, vec!["0".to_owned()]), b"1\n");
        assert_eq!(cache.stats().compiled_chunks, 1);
        let bytes = cache.compiled_cache_stats().retained_bytes;
        cache = cache.with_compiled_cache_limits(8, bytes);

        let mut reference = Vec::new();
        let expected = tinylang::run_verified_program(&program, "vm", &mut reference, vec!["1".to_owned()]);
        let mut output = Vec::new();
        let actual = cache.run_verified_program(&program, &mut output, vec!["1".to_owned()]);
        assert_eq!(output, reference);
        assert_eq!(actual.as_ref().err().map(ToString::to_string), expected.as_ref().err().map(ToString::to_string));
        assert_eq!(actual.is_ok(), tail == "return value");
        assert!(cache.is_empty(), "lazy code/feedback/scratch must not escape the byte limit");
        assert_eq!(cache.compiled_cache_stats().retained_bytes, 0);
        assert_eq!(cache.compiled_cache_stats().table_capacity, 0);
        assert_eq!(cache.compiled_cache_stats().evictions, 1);
        assert_eq!(cache.compiled_cache_stats().bypasses, 1);
        assert_eq!(run(&mut cache, &program, vec!["0".to_owned()]), b"1\n");
    }
}

#[test]
fn feedback_only_growth_can_evict_a_previously_warm_entry() {
    let program = compile_source_verified(
        "let end = read_int() let i = 0 let value = 7 while i < end { value = value * 1 i = i + 1 } print value",
    )
    .expect("feedback fixture");
    let mut cache = JitCache::new();
    assert_eq!(run(&mut cache, &program, vec!["1".to_owned()]), b"7\n");
    assert_eq!(cache.compile_verified(&program).expect("cold entry").feedback_bytes(), 0);
    let bytes = cache.compiled_cache_stats().retained_bytes;
    cache = cache.with_compiled_cache_limits(8, bytes);
    assert_eq!(run(&mut cache, &program, vec!["128".to_owned()]), b"7\n");
    assert!(cache.is_empty());
    assert_eq!(cache.compiled_cache_stats().bypasses, 1);
}

#[test]
fn growing_entry_evicts_older_entries_when_it_still_fits_alone() {
    let program = compile_source_verified(
        "let end = read_int() let i = 0 let value = 7 while i < end { value = value * 1 i = i + 1 } print value",
    )
    .expect("feedback fixture");
    let mut cache = JitCache::new();
    assert_eq!(run(&mut cache, &program, vec!["1".to_owned()]), b"7\n");
    assert_eq!(run(&mut cache, &constant_program(2, 3), Vec::new()), b"2\n");
    let bytes = cache.compiled_cache_stats().retained_bytes;
    cache = cache.with_compiled_cache_limits(8, bytes);
    assert_eq!(run(&mut cache, &program, vec!["128".to_owned()]), b"7\n");
    assert_eq!(cache.len(), 1);
    assert_eq!(cache.compiled_cache_stats().evictions, 1);
    assert_eq!(cache.compiled_cache_stats().bypasses, 0);
    assert!(
        cache
            .compile_verified(&program)
            .expect("newly hot entry retained")
            .feedback_bytes()
            > 0
    );
    assert_eq!(cache.compiled_cache_stats().compilations, 2);
}

#[test]
fn reducing_limits_and_clearing_release_compiled_storage_but_preserve_sources() {
    let mut cache = JitCache::new();
    for value in 0..8 {
        cache
            .run_source(&format!("print {value}"), &mut Vec::new(), Vec::new())
            .expect("source fixture");
    }
    let capacity = cache.compiled_cache_stats().table_capacity;
    cache = cache.with_compiled_cache_limits(2, usize::MAX);
    assert_eq!(cache.len(), 2);
    assert!(cache.compiled_cache_stats().table_capacity < capacity);
    assert_eq!(cache.compiled_cache_stats().evictions, 6);
    assert_eq!(cache.source_cache_len(), 8);
    cache.clear_compiled_cache();
    assert_eq!(cache.compiled_cache_stats().retained_bytes, 0);
    assert_eq!(cache.compiled_cache_stats().table_capacity, 0);
    assert_eq!(cache.compiled_cache_stats().evictions, 8);
    assert_eq!(cache.source_cache_len(), 8);
    cache
        .run_source("print 7", &mut Vec::new(), Vec::new())
        .expect("source survives clear");
    assert_eq!(cache.stats().source_hits, 1);
    cache = cache.with_compiled_cache_limits(0, 0);
    assert_eq!(cache.compiled_cache_stats().retained_bytes, 0);
    assert_eq!(cache.compiled_cache_stats().table_capacity, 0);
}

#[test]
fn all_runner_families_preserve_inputs_and_environment_when_retention_is_disabled() {
    let program = compile_source_verified("print read_int() print sys_argc() print sys_env_get(\"CACHE_TEST\")")
        .expect("environment fixture");
    let mut cache = JitCache::new().with_compiled_cache_limits(0, 0);
    let mut output = Vec::new();
    cache
        .run_verified_program_with_env(
            &program,
            &mut output,
            vec!["7".to_owned()],
            vec!["argument".to_owned()],
            HashMap::from([("CACHE_TEST".to_owned(), "first".to_owned())]),
        )
        .expect("verified environment run");
    assert_eq!(output, b"7\n1\nfirst\n");
    output.clear();
    cache
        .run_program_with_env(
            program.program(),
            &mut output,
            vec!["9".to_owned()],
            Vec::new(),
            HashMap::from([("CACHE_TEST".to_owned(), "second".to_owned())]),
        )
        .expect("raw environment run");
    assert_eq!(output, b"9\n0\nsecond\n");
    let report_program = compile_source_verified("let a = [1, 2] print len(a)").expect("heap fixture");
    let report = cache
        .run_verified_program_report(&report_program, &mut Vec::new(), Vec::new())
        .expect("verified report");
    assert_eq!(report.heap_after_shutdown.live_objects, 0);
    let report = cache
        .run_program_report(report_program.program(), &mut Vec::new(), Vec::new())
        .expect("raw report");
    assert_eq!(report.heap_after_shutdown.live_objects, 0);
    let report = cache
        .run_source_report("let a = [1, 2] print len(a)", &mut Vec::new(), Vec::new())
        .expect("source report");
    assert_eq!(report.heap_after_shutdown.live_objects, 0);
    assert_eq!(cache.compiled_cache_stats().bypasses, 5);
    assert_eq!(cache.compiled_cache_stats().retained_bytes, 0);
}

#[test]
fn panicking_output_drops_the_mutated_entry_and_propagates_the_original_panic() {
    struct PanickingOutput;
    impl std::io::Write for PanickingOutput {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            std::panic::panic_any("cache output panic");
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let program = compile_source_verified("fn lazy() { return 42 } print lazy()").expect("panic fixture");
    let mut cache = JitCache::new();
    cache.compile_verified(&program).expect("cold cached entry");
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        cache.run_verified_program(&program, &mut PanickingOutput, Vec::new())
    }));
    let panic = outcome.expect_err("host output panic must propagate");
    assert_eq!(panic.downcast_ref::<&str>(), Some(&"cache output panic"));
    assert!(cache.is_empty(), "partly mutated compiled state must not be reused after unwind");
    assert_eq!(cache.compiled_cache_stats().retained_bytes, 0);
    assert_eq!(cache.compiled_cache_stats().table_capacity, 0);
    assert_eq!(run(&mut cache, &program, Vec::new()), b"42\n");
}
