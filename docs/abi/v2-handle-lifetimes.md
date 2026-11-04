# Runtime and object handle lifetime experiment

Implemented and measured 2026-10-11; integrated and verified 2026-10-13.

Problem: the imported runtime/object registry used
`generation.wrapping_add(1).max(1)` on release. After the final u32 generation,
the next allocation can reproduce an old handle. This violates the intended
stale-reference contract. No allocator, VM, or wire representation change is
needed to address this issue.

The [research notebook](v2-research.md) motivates opaque boundaries and explicit
ownership. The counter-exhaustion diagnosis and candidate algorithms here are
local engineering analysis, not claims from external benchmarks.

## Candidates and equivalent checks

The isolated executable crate is `crates/tinylang_core/experiments/handle-lifetimes`.
It compares only identity allocation, lookup, and release for 64 slots. It
does not simulate VM execution, locking, object payloads, or foreign calls.

- Baseline reproduces the existing slot-index/u32-generation wrapping policy.
- A preserves the handle encoding and direct lookup, but permanently retires a
  slot when its generation is exhausted. Hypothesis: smallest compatibility
  cost and no allocation requirement.
- B assigns monotonic u64 identities and scans a fixed table. Hypothesis:
  substantially longer identity lifetime, simple compact storage, at the cost
  of linear lookups instead of decoding a slot index.
- C stores monotonic u64 identities in a hash set. Hypothesis: avoid linear
  lookup at high occupancy, with allocation/hashing overhead and extra code.

Each is separately compiled and runs the same capacity, release, stale-key,
and invalid-key test. Counter-exhaustion tests seed the counter at its final
value, equivalent to prior reuse without billions of iterations. The baseline
test deliberately confirms resurrection; its passing test means the defect
was reproduced, not that it satisfies the safety requirement. A/B/C reject old
identities permanently. B/C fail allocation after u64 exhaustion; A loses one
slot after that slot's u32 exhaustion.

## Reproduction

Same Windows x64/MSVC host and toolchain as [baseline](v2-baseline-performance.md).
No dependencies; release opt-level 3, codegen-units 1, LTO off, unwind panics.
For each of `baseline`, `a`, `b`, and `c`, run:

```powershell
cargo test --manifest-path crates/tinylang_core/experiments/handle-lifetimes/Cargo.toml --no-default-features --features a --target-dir target/handle-lifetimes/a
cargo build --release --manifest-path crates/tinylang_core/experiments/handle-lifetimes/Cargo.toml --no-default-features --features a --target-dir target/handle-lifetimes/a
./target/handle-lifetimes/a/release/handle-lifetimes.exe
```

One warm-up and seven timed batches, ten million cycles per batch. A cycle
inserts, checks live, removes, and checks stale. Results include assertions,
black-box and loop overhead. Zero and 63 preoccupied slots test opposite ends
of the fixed-table search. Raw samples are `evidence/handle-lifetimes-*.csv`;
the first two rows contain initialization and inline-size observations.

| Measurement | Baseline | A | B | C |
| --- | ---: | ---: | ---: | ---: |
| Safety at exhaustion | Fails (reproduced) | Pass | Pass | Pass |
| Empty-table cycle median ns | 7.053 | 5.202 | 21.555 | 36.299 |
| Empty-table min–max ns | 5.897–8.775 | 5.042–6.036 | 21.298–21.902 | 35.486–37.050 |
| 63-occupied cycle median ns | 27.933 | 19.044 | 70.588 | 48.316 |
| 63-occupied min–max ns | 27.280–29.717 | 18.119–19.322 | 68.627–73.244 | 46.074–49.243 |
| Samples per occupancy | 7 | 7 | 7 | 7 |
| Executable bytes | 162,816 | 162,816 | 165,376 | 166,400 |
| Inline registry bytes | 512 | 512 | 520 | 56 plus unmeasured heap storage |
| Allocation count | Unavailable | Unavailable | Unavailable | Unavailable |
| Peak process memory | Unavailable | Unavailable | Unavailable | Unavailable |
| Data copying | No payload interchange in experiment | Same | Same | Same |
| Compile+link wall time, ms | Unavailable: anomalous stall | 478.567 | 552.207 | 597.493 |
| Library size | Not applicable: isolated executable | Same | Same | Same |
| Initialization latency | Below reliable single-sample resolution | Same | Single sample only | Single sample only |
| Wire representation | Current u64 | Unchanged encoding | Opaque u64, changed interpretation | Opaque u64, changed interpretation |
| Lookup implementation | Decode index | Decode index | Linear scan | Hash lookup |
| Limitation | Old identities can recur | Retired slots reduce capacity | Global identity exhaustion | Global identity exhaustion and heap use |

Compile+link values are single rebuild observations, not statistical
comparisons. The raw baseline wall time of 6,900,617.570 ms conflicts with
Cargo's approximately 56-minute duration and normal subsecond builds; a
session/environment timing disruption is suspected, not established. The
anomalous observation remains in the evidence; it is not a valid baseline for
calculating compile-time improvement. Initial short-batch timing runs were
superseded by the longer raw samples above.

Using `(baseline - candidate) / baseline * 100`, observed empty-table time
changes are A +26.24%, B -205.61%, C -414.66%; high-occupancy changes are
A +31.82%, B -152.70%, C -72.97%. These describe this microbenchmark, not ABI
call speedups. Binary-size changes are A 0%, B +1.57%, C +2.20%.
Memory improvement is unavailable: inline struct size is not peak memory.
Throughput is the reciprocal of the measured cycle time; it is not foreign
call throughput. Scheduling/frequency were uncontrolled and candidate order
was not randomized, so small differences should not be treated as causal.

## Selection and integration

Selected A for correctness, unchanged identity interpretation, direct lookup,
and bounded storage. Measurements provide no reason to pay B/C's additional
lookup/representation costs for this fix. Production runtime/object slots now
use explicit Vacant/Live/Retired states in `handles/lifetime.rs`. A regression
test seeds the actual registry tables at the final generation, releases them,
checks stale rejection, and verifies later allocation skips those slots.

Post-integration release tests: 28 passed. Clippy with warnings denied passes.
Golden-vector tests continue to pass. Full boundary benchmark was rerun with
seven warmed-up samples; raw results are `evidence/retired-slots-boundary.csv`.
Medians: NOP 28.729 ns, u64 echo 60.403 ns, 4 KiB echo 97.498 ns, 1 MiB echo
32,238.500 ns. Release DLL is 1,482,240 bytes and benchmark executable 301,568
bytes, each 512 bytes larger than the earlier integrated baseline. These
different-day boundary timings do not establish a speed improvement.

This experiment does not resolve owned-buffer token wrap/reuse, cross-domain
handle collisions, or registry concurrency/reentrancy questions. Those remain
audit items; no general foreign-handle safety completion claim is made.

## Runtime/object domain separation, 2026-10-29

The later executable audit reproduced a separate defect: a Runtime and Program
could have the same numeric value, so `HANDLE_RELEASE(runtime_value)` could
release the Program. The reference registry now reserves an unused bit in the
slot component for object handles and rejects the opposite domain before table
lookup. All 32 generation bits and exhaustion retirement remain unchanged.

This supersedes only the cross-domain collision item above. It changes generated
object numbers, not the opaque u64 wire contract, ABI symbols or version. Old
untagged object numbers are rejected as objects; clients must obtain fresh live
handles when replacing the library. See the [compatibility notes](v2-binary-spec.md#5-handles-and-provenance).

Public ABI regressions cover both substitution directions, unchanged ownership
after rejection, and stale references after runtime/object slot reuse. The
existing final-generation test continues to exercise both actual registry
tables. Owned-buffer identity, general heap provenance and foreign invocation
remain separate open gates; this is not a full ownership or release-readiness
claim.
