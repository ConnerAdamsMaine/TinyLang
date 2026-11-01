# Instruction budget: experiment and runtime prerequisite

Status: runtime prerequisite integrated, **foreign invocation still unfinished**.
Date: 2026-10-13. The original interpreter offered only unbounded execution;
the v2 schema promised a nonzero instruction budget without implementing it.

## Research and scope

[Wasmtime configuration](https://docs.wasmtime.dev/api/wasmtime/struct.Config.html#method.consume_fuel)
and [store fuel](https://docs.wasmtime.dev/api/wasmtime/struct.Store.html#method.set_fuel)
provide an authoritative example of opt-in deterministic execution accounting.
This supports separating an embedding execution policy from default execution.
The advantage is a predictable guest-work limit; the cost is instrumentation.
Its exact instruction costs and implementation are not imported into TinyLang.
No Wasmtime timing or binary-size measurement was made here.

TinyLang's selected policy charges one unit before each interpreter bytecode,
including call, return, jump and halt. Native builtin work, separately spawned
VMs, JIT execution and wall time are outside this policy. Foreign invocation
must constrain those facilities before it can claim bounded execution.

## Three working accounting candidates

The independent `tinylang_v2/experiments/instruction-budget` crate implements:

- Baseline: unbounded trace execution; deliberately fails the budget contract.
- A: a local countdown checked before each instruction. Hypothesis: minimal
  compatibility and code complexity, precise side-effect ordering.
- B: admit a whole known straight-line block, or its affordable prefix, once
  per block. Hypothesis: fewer checks on long blocks, but requires verified
  block boundaries and accounting on traps/calls in a real interpreter.
- C: atomically decrement a shared counter using compare-and-update. Hypothesis:
  enables sharing a budget across execution participants, at synchronization cost.

Each kernel executes identical effect traces. Tests examine every budget from
0 through 45 over mixed block sizes, empty blocks and the largest u64 budget.
All bounded candidates execute precisely the affordable prefix. The baseline
test confirms its intentionally unbounded behavior rather than certifying it
as safe. These are accounting-kernel prototypes, not three complete VMs. B's
real-interpreter block preparation and trap refund integration remain unverified;
C's multithreaded performance is unmeasured.

## Reproducible comparison

Windows x64, Intel Core i7-8850H, rustc 1.99.0 MSVC. Independent release profile:
opt-level 3, codegen-units 1, LTO off; no dependencies. For each feature
`baseline`, `a`, `b`, `c` (replace both `a` occurrences):

```powershell
cargo test --manifest-path tinylang_v2/experiments/instruction-budget/Cargo.toml --no-default-features --features a --target-dir target/instruction-budget/a
cargo build --release --manifest-path tinylang_v2/experiments/instruction-budget/Cargo.toml --no-default-features --features a --target-dir target/instruction-budget/a
./target/instruction-budget/a/release/instruction-budget.exe
```

One warm-up plus seven measured batches per workload; 102,400,000 effect
instructions per batch. The work includes an opaque checksum update so it
cannot be optimized to an empty loop. Trace allocation is outside timing.
Raw values: `evidence/instruction-budget-{baseline,a,b,c}.csv` and
`evidence/instruction-budget-build.csv`. No foreign-call overhead is measured.

| Measurement | Baseline | A | B | C |
| --- | ---: | ---: | ---: | ---: |
| Budget correctness | Absent | Prefix tests pass | Prefix tests pass | Prefix tests pass |
| Single-instruction blocks, median ns/instruction | 1.223 | 1.189 | 2.006 | 7.973 |
| Min–max ns/instruction | 1.206–1.283 | 1.178–1.336 | 1.996–2.146 | 7.959–8.045 |
| 32-instruction blocks, median ns/instruction | 1.378 | 1.375 | 1.378 | 7.981 |
| Min–max ns/instruction | 1.374–1.388 | 1.369–1.395 | 1.373–1.380 | 7.969–8.079 |
| Sample count per workload | 7 | 7 | 7 | 7 |
| Executable bytes | 158,720 | 158,720 | 158,720 | 158,720 |
| Compile+link wall time, ms (one observation) | 449.792 | 417.764 | 414.324 | 431.720 |
| Allocation count | Unavailable | Unavailable | Unavailable | Unavailable |
| Peak memory | Unavailable | Unavailable | Unavailable | Unavailable |
| Library size | Not applicable | Not applicable | Not applicable | Not applicable |
| Initialization cost | Unavailable | Unavailable | Unavailable | Unavailable |
| Data transfer copies | No payload transfer in kernel | Same | Same | Same |
| ABI compatibility | No external layout changed | Same | Same | Same |
| Complexity | No accounting | Local counter | Block preparation needed in VM | Atomic update |

Throughput is the reciprocal of per-instruction time, not calls/second.
Using `(baseline time - candidate time) / baseline time * 100`, single-block
observed changes are A +2.78%, B -64.02%, C -551.92%; 32-instruction-block
changes are A +0.22%, B 0%, C -479.17%. Executable size change is 0% for all.
Memory improvement is unavailable. These are sequential, uncontrolled-host
measurements; the near-baseline A timings do not prove an execution speedup.

Selected A: precise per-instruction semantics without preparing blocks or
requiring shared synchronization. B did not demonstrate a benefit on these
workloads that justifies additional integration complexity. This selects the
accounting mechanism only, not the complete foreign invocation architecture.

## Integration and verification

`VM::run_report_with_fuel` selects a statically dispatched `Fuel` budget;
existing `run_report` uses the zero-work `Unlimited` implementation. The same
mutable budget travels through direct and indirect nested calls. No mode
boolean is tested inside the dispatch loop. A `ChunkIdentity` enum replaces
the separate chunk-name/index arguments to keep the extended call interface
coherent. All other interpreter instruction behavior stays in the existing VM.

Regression tests cover zero fuel, exact halt allowance, u64::MAX, infinite
jumps, side effects before/after exhaustion, and shared fuel in direct and
indirect calls. The first infinite-loop fixture failed verification because it
lacked the required trailing HALT; the fixture was corrected to an unreachable
HALT, retaining an actual infinite back edge.

Observed verification: 4 budget tests, 59 runtime-parity tests, 42 frozen ABI
tests, and 28 v2 release tests pass. Core library Clippy with warnings denied
passes. The first Clippy run flagged the added eighth `run_chunk` argument;
`ChunkIdentity` resolved this without a lint suppression.

The integrated runtime benchmark executes a verified loop of 900,002 bytecode
instructions with identical output/memory assertions, once warm then seven
times per mode:

```powershell
cargo build --release --locked --manifest-path tinylang_v2/Cargo.toml --target-dir target/abi-v2 --example fuel_benchmark
./target/abi-v2/release/examples/fuel_benchmark.exe
```

Raw results: `evidence/integrated-fuel.csv`. Unbounded median 14.702 ms,
range 12.966–15.484 ms; bounded median 13.752 ms, range 11.809–15.868 ms.
The ranges overlap substantially; no speedup is claimed. Verification/VM
construction precede timing; execution and normal shutdown are timed. This
example links both modes and is 746,496 bytes; it is not an isolated measure
of the bounded mode's binary-size cost. Build observation: 0.77 seconds.

## Still required

The public method remains a consuming, one-shot Rust API. Persistent sessions,
typed foreign arguments/results, artifact exports, per-session heap limits,
host capability restrictions and actual `VM_INVOKE` dispatch remain to be
implemented and tested. Fuel exhaustion currently maps to the existing typed
`TinyOneError::Runtime` category and annotated message; a future dispatcher
must map runtime failures to its documented trap status without parsing text.
