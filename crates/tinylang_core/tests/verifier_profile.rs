#![cfg(feature = "testing-hooks")]

#[path = "verifier/fixtures.rs"]
mod fixtures;
#[path = "allocation/host.rs"]
mod host;

use std::hint::black_box;

use fixtures::{VerifierFixture, check_output};
use host::HostAllocationInterval;
use ralloc::testing::instrumentation_snapshot;
use tinylang::BytecodeVerifier;
use tinylang::testing::{reset_runtime_cost_counters, runtime_cost_counters};

fn cases() -> impl Iterator<Item = (&'static str, usize, VerifierFixture)> {
    [0, 1, 16, 256, 4096]
        .into_iter()
        .flat_map(|count| {
            [
                ("functions", count, VerifierFixture::Functions(count)),
                ("names", count, VerifierFixture::Names(count)),
            ]
        })
        .chain(
            [0, 1, 16, 256]
                .into_iter()
                .map(|count| ("struct_fields", count, VerifierFixture::StructFields(count))),
        )
}

#[test]
fn verifier_metadata_fixtures_preserve_shape_and_output() {
    for (_, count, fixture) in cases() {
        let verified = fixture.prepare();
        let program = verified.program();
        assert_eq!(program.code().len(), 3);
        match fixture {
            VerifierFixture::Functions(_) => {
                assert_eq!(program.functions().len(), count);
                assert!(program.functions().iter().all(|function| function.code().len() == 2));
            }
            VerifierFixture::Names(_) => assert_eq!(program.slot_count(), count),
            VerifierFixture::StructFields(_) => {
                assert_eq!(program.structs().len(), 1);
                assert_eq!(program.structs()[0].fields().len(), count);
            }
        }
        check_output(&verified);
    }
}

/// Run this test alone: both observers are process-wide. Optimized host
/// allocation counts are build-specific observations, not Rust guarantees.
#[test]
#[ignore = "verifier allocation attribution; run alone with --ignored --nocapture --test-threads=1"]
fn report_verifier_allocations() {
    for (name, count, fixture) in cases() {
        let verified = fixture.prepare();
        check_output(&verified);
        let program = verified.program();
        // Construction, execution checks, and the comparison owner are outside
        // observation. No Program clone, token creation or fingerprint is counted.
        let original = program.clone();
        BytecodeVerifier::verify(program).expect("prime verifier fixture");
        for passes in [0, 1, 32] {
            for sample in 1..=3 {
                reset_runtime_cost_counters();
                let interval = HostAllocationInterval::start();
                for _ in 0..passes {
                    // The public API constructs and drops a fresh certificate,
                    // module graph and traversal scratch on every invocation.
                    black_box(BytecodeVerifier::verify(black_box(program))).expect("verify metadata fixture");
                }
                let host = interval.finish();
                assert_eq!(runtime_cost_counters(), Default::default(), "verification must not execute bytecode");
                assert_eq!(instrumentation_snapshot(), Default::default(), "verification must not use Ralloc");
                assert_eq!(program, &original, "verification must not mutate its input");
                assert_eq!(host.allocations, host.deallocations, "all temporary owners must be dropped");
                assert_eq!(host.size_buckets.iter().sum::<usize>(), host.allocations);
                if passes == 0 {
                    assert_eq!(host.allocations, 0);
                    assert_eq!(host.reallocations, 0);
                    assert_eq!(host.allocated_bytes, 0);
                    assert_eq!(host.reallocated_bytes, 0);
                }
                eprintln!(
                    "verifier-allocation fixture={name} count={count} passes={passes} sample={sample} \
                     host_allocations={} host_deallocations={} host_reallocations={} \
                     host_bytes={} host_reallocated_bytes={} buckets_64_256_1024_large={:?}",
                    host.allocations,
                    host.deallocations,
                    host.reallocations,
                    host.allocated_bytes,
                    host.reallocated_bytes,
                    host.size_buckets,
                );
            }
        }
    }
}
