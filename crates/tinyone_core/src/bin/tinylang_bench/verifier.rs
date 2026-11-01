//! Complete verification of prebuilt metadata, including certificate cleanup.

#[path = "../../../tests/verifier/fixtures.rs"]
mod fixtures;

use fixtures::{VerifierFixture, check_output};

use super::{Benchmark, BytecodeVerifier, bench, black_box};

pub(super) fn benchmarks() -> Vec<Benchmark> {
    [
        ("verifier.functions_256", VerifierFixture::Functions(256), 500),
        ("verifier.names_4096", VerifierFixture::Names(4096), 500),
        ("verifier.struct_fields_256", VerifierFixture::StructFields(256), 5_000),
    ]
    .into_iter()
    .map(|(name, fixture, iterations)| {
        let verified = fixture.prepare();
        check_output(&verified);
        bench(name, iterations, move || {
            black_box(BytecodeVerifier::verify(black_box(verified.program()))).expect("verify metadata fixture");
        })
    })
    .collect()
}
