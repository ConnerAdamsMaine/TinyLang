# Invocation argument comparison: scalar phase

## Reference runtime integration status

The v2 dispatcher now implements `VM_INVOKE` for null, boolean, signed and
unsigned integer, and f32/f64 scalar records. Values retain their wire tags;
there is no numeric coercion. Export IDs are function-table indices in the
loaded artifact. Unqualified root functions are host-callable, while
qualified module functions require an explicit module export. Function arity,
wire shape, handle ownership, fuel, and result capacity are checked at the
appropriate boundary.

The reference implementation remains partial: heap values, handles,
aggregates, and unit results are not marshalled. Unsupported input/result
values return `INVALID_ARGUMENT`; when an unsupported result is discovered,
the function has already executed and its language side effects remain.
Capacity preflight and low-fuel recovery are covered by
`crates/tinylang_core/tests/opcode_contract.rs`. This integration does not make
performance claims and does not implement a native JIT or AOT backend.

The historical three-way invocation comparison remains incomplete. It lives in
`crates/tinylang_core/experiments/invocation-values` and executes the actual compiler's
`difference(a, b)` function after an untrusted binary-artifact round trip.
The reference runtime's current scalar integration uses canonical tagged
records; this experiment is not its serializer implementation.

The comparison follows the research notebook's alternatives: canonical tagged
records (A), signature-driven packed values (B), and an explicit argument stack
(C, inspired by Lua's state-owned stack). A preserves the existing interchange
shape. B tests avoiding per-value tags given a prepared signature. C tests
separate typed push/call operations and transfer of an argument vector.

The synthetic baseline passes Rust values directly to the same VM method.
These benchmark paths are isolated prototypes, not measurements of the current
v2 dispatcher. All four paths execute the same function, fuel budget and
arguments; baseline omits interchange work. Timing includes host encoding,
decoding, execution and a result assertion. It excludes compilation, program
loading, initialization, registry access, foreign entry and result
serialization.

## Correctness and limitations

All configurations compile and pass tests of argument order, signed 64-bit
extremes, invalid counts and successful invocation after rejection. A also
rejects changes to type, flags, length and auxiliary header bytes; B rejects
truncated/extra packed data. All configurations pass Clippy with warnings
denied. Release benchmark assertions check every result.

Only two i64 arguments and an i64 result are exercised. A's narrow parser is
an isolated prototype, not a replacement for the canonical value parser.
B's signature is fixed by the fixture, not discovered from an export schema.
C calls Rust functions with inlining disabled for push/call; this does not
measure repeated C ABI crossings. Its vector storage is transferred to the
existing consuming argument interface rather than retained for reuse.

The production integration subsequently selected the existing canonical tagged
records for supported scalar values, including floating point and result
encoding. This experiment did not compare general value marshalling: strings,
bytes, aggregates, opaque handles, nested ownership and allocation limits
remain outside the production invocation boundary.

## Measurements

Same Windows x64 i7-8850H / rustc 1.99.0 MSVC environment as prior reports.
Release opt-level 3, one codegen unit, LTO disabled. Each run warms 100,000
calls, then records seven samples of 100,000 calls. Builds share warmed
dependencies. Build times below are single final-crate observations.

| Measurement | Direct Rust baseline | A tagged | B packed | C stack |
| --- | ---: | ---: | ---: | ---: |
| Median ns/call | 433.774 | 507.403 | 484.282 | 507.914 |
| Min–max ns/call | 425.711–442.549 | 490.904–538.432 | 479.847–515.234 | 448.193–535.048 |
| Throughput, million calls/s (reciprocal median) | 2.305 | 1.971 | 2.065 | 1.969 |
| Time improvement from baseline | — | -16.97% | -11.64% | -17.09% |
| Executable bytes | 1449984 | 1452032 | 1450496 | 1452032 |
| Size change | — | +0.141% | +0.035% | +0.141% |
| Final-crate build ms | 718.075 | 718.329 | 731.407 | 705.586 |
| Encoded argument payload bytes (layout, not measured copy traffic) | N/A | 48 | 16 | N/A |
| Allocation count / peak memory / copied bytes | Unavailable | Unavailable | Unavailable | Unavailable |
| Library size / initialization cost | Unavailable | Unavailable | Unavailable | Unavailable |
| ABI compatibility | Rust only | Existing I64 record shape | Needs signature/format extension | Needs additional state/API operations |

Percentages use `(baseline time - candidate time) / baseline time` and
`(candidate size - baseline size) / baseline size`. None improves on direct
Rust calls, which intentionally omit interchange. Candidate ranges overlap;
the lowest median does not establish statistical superiority. Code sizes
include compiler/loader support shared by all configurations, not a separately
linked ABI library. Allocation and memory improvements cannot be calculated.

Raw data: [baseline](evidence/invocation-values-baseline.csv),
[A](evidence/invocation-values-a.csv), [B](evidence/invocation-values-b.csv),
[C](evidence/invocation-values-c.csv), [builds](evidence/invocation-values-build.csv).
An initial run used an incorrect I64 tag in both A's encoder and local test;
it was discarded after checking the authoritative wire constants. The final
run above uses tag 9 and was rerun after Clippy-driven fixed-array decoding.

## Reproduction

Warm dependencies with a release build, then substitute each of `baseline`,
`a`, `b`, `c` for FEATURE. Exactly one feature must be enabled. Time the final
`cargo rustc` process externally. The otherwise unused cfg forces a distinct
final-crate build from the warm-up invocation.

```text
cargo test --locked --manifest-path crates/tinylang_core/experiments/invocation-values/Cargo.toml --target-dir target/session-execution --no-default-features --features FEATURE
cargo clippy --locked --manifest-path crates/tinylang_core/experiments/invocation-values/Cargo.toml --target-dir target/session-execution --all-targets --no-default-features --features FEATURE -- -D warnings
cargo rustc --release --locked --manifest-path crates/tinylang_core/experiments/invocation-values/Cargo.toml --target-dir target/session-execution --no-default-features --features FEATURE -- --cfg invocation_measurement3
target/session-execution/release/invocation-values.exe
```
