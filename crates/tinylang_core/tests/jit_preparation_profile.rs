#![cfg(feature = "testing-hooks")]

#[path = "preparation/fixtures.rs"]
mod fixtures;
#[path = "allocation/host.rs"]
mod host;
#[path = "preparation/sources.rs"]
mod sources;

use std::hint::black_box;

use fixtures::{LARGE_CASES, PreparationFixture};
use host::HostAllocationInterval;
use ralloc::testing::instrumentation_snapshot;
use sources::SOURCE_CASES;
use tinylang::testing::reset_runtime_cost_counters;
use tinylang::{JitCache, JitProgram, JitStats, VerifiedProgram, compile_source_verified, run_verified_program};

enum Fixture {
    Source { source: &'static str, output: &'static str },
    Raw(PreparationFixture),
}

impl Fixture {
    fn prepare(self) -> (VerifiedProgram, String) {
        match self {
            Self::Source { source, output } => {
                (compile_source_verified(source).expect("source fixture"), output.to_owned())
            }
            Self::Raw(fixture) => fixture.prepare(),
        }
    }
}

struct PreparedStorage {
    stats: JitStats,
    retained_bytes: usize,
}

fn standalone(verified: &VerifiedProgram) -> PreparedStorage {
    let compiled = JitProgram::compile_verified(verified).expect("standalone lowering");
    black_box(&compiled);
    PreparedStorage {
        stats: compiled.stats(),
        retained_bytes: compiled.retained_bytes(),
    }
}

fn cached(verified: &VerifiedProgram) -> PreparedStorage {
    let mut cache = JitCache::new();
    let compiled = cache.compile_verified(verified).expect("fresh-cache lowering");
    black_box(compiled);
    let stats = compiled.stats();
    PreparedStorage {
        stats,
        retained_bytes: cache.compiled_cache_stats().retained_bytes,
    }
}

fn profile_preparation(
    name: &str,
    verified: &VerifiedProgram,
    route: &str,
    prepare: impl Fn(&VerifiedProgram) -> PreparedStorage,
) {
    // Prime immutable fingerprint/storage memoization and process-local
    // allocator setup, not compiled-code reuse. Every sample owns fresh code.
    black_box(prepare(verified));
    for sample in 1..=3 {
        reset_runtime_cost_counters();
        let interval = HostAllocationInterval::start();
        let storage = prepare(verified);
        let host = interval.finish();
        let ralloc = instrumentation_snapshot();
        assert_eq!(host.allocations, host.deallocations, "preparation owners are dropped inside the interval");
        assert_eq!(ralloc.allocations, 0, "lowering must not allocate execution payloads");
        assert_eq!(ralloc.deallocations, 0);
        assert_eq!(ralloc.live_payload_bytes, 0);
        assert_eq!(storage.stats.compiled_chunks, 1, "function lowering remains lazy");
        eprintln!(
            "jit-preparation fixture={name} route={route} sample={sample} \
             bytecode_ops={} functions={} compiled_ops={} retained_bytes={} \
             host_allocations={} host_deallocations={} host_reallocations={} \
             host_bytes={} host_reallocated_bytes={} buckets_64_256_1024_large={:?}",
            verified.program().code().len(),
            verified.program().functions().len(),
            storage.stats.compiled_ops,
            storage.retained_bytes,
            host.allocations,
            host.deallocations,
            host.reallocations,
            host.allocated_bytes,
            host.reallocated_bytes,
            host.size_buckets,
        );
    }
}

/// Run alone: counters cover one process, and optimized allocator calls are
/// build-specific observations, not language guarantees or latency results.
#[test]
#[ignore = "JIT preparation allocation attribution; run alone with --ignored --nocapture"]
fn report_jit_preparation_allocations() {
    let raw_fixtures = [
        ("deep_4", Fixture::Raw(PreparationFixture::ExpressionChain(4))),
        ("deep_1024", Fixture::Raw(PreparationFixture::ExpressionChain(1024))),
    ];
    let source_fixtures = SOURCE_CASES.into_iter().map(|case| {
        (
            case.name,
            Fixture::Source {
                source: case.source,
                output: case.output,
            },
        )
    });
    for (name, fixture) in source_fixtures.chain(raw_fixtures).chain(
        LARGE_CASES
            .into_iter()
            .map(|(name, fixture, _)| (name, Fixture::Raw(fixture))),
    ) {
        let (verified, expected) = fixture.prepare();
        let mut reference = Vec::new();
        drop(run_verified_program(&verified, "vm", &mut reference, Vec::new()).expect("VM reference"));
        assert_eq!(reference, expected.as_bytes(), "{name}");
        let mut compiled = JitProgram::compile_verified(&verified).expect("parity lowering");
        let mut output = Vec::with_capacity(reference.len());
        drop(compiled.run(&mut output, Vec::new()).expect("JIT reference"));
        assert_eq!(output, reference, "{name}");
        drop(compiled);
        profile_preparation(name, &verified, "standalone", standalone);
        profile_preparation(name, &verified, "cache", cached);
    }
}
