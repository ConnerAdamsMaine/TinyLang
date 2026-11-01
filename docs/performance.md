---
title: Performance
---

# TinyOne performance workflow

`tinylang_bench` is the performance regression and optimization-targeting
harness. It measures isolated subsystems instead of treating source-to-output
latency as one opaque number.

## Current whole-system checkpoint

The [current acceptance audit](optimization-acceptance.md) reconciles all 40
plan sections, container decisions, available evidence and remaining gaps.

The [matched original/current comparison](optimization-worklog.md#reproducible-originalcurrent-whole-system-comparison)
uses byte-identical original harnesses and separately frozen source/build
trees on Windows and Arch WSL. Its 256 fixed captures yield 124 usable pairs:
14 workload rows improve in both orders on both platforms, 11 regress,
three are mixed and four lack complete quality-qualified pairs. All four
noisy captures and all 47 eligible greater-than-5% primary/best-wall
regressions remain in the report. No measurement was retried.

Tiny cold lowering is about 3x the original cost; 4-KiB buffer creation is
about 1.8x on Windows and 2.8x on Linux. The allocator comparison includes
the current safe-buffer initialization guarantee absent from the original
implementation; it does not justify removing that guarantee or assign the
entire gap to zeroing. Vector/map program rows also regress. This is not a
Vec-versus-Box experiment, a uniform system speedup or full-plan acceptance.
Fixed metadata tables remain boxed. Private JitOps still exposes slices,
accounts for retained capacity and reclaims spare extents of at least 4KiB.
The subsequent [initialized-copy trial](optimization-worklog.md#initialized-copy-constructor-trial-not-promoted)
preserves initialization guarantees and improves isolated copy timings, but
does not pass its fixed screen: one Linux 4-KiB confirmation is too noisy,
and Linux allocation workloads lack consistent benefit. All 32 captures are
retained; no retries or conditional guardrail runs follow. The candidate APIs
remain confined to evidence snapshots; production storage is unchanged.
See the worklog for exact intervals and follow-up
attribution requirements before proposing further changes.

## Measurement model

- Build and run the optimized binary. Debug timings are not representative.
- Correctness checks run before timings and compare VM/JIT output on every
  runtime workload. `--skip-correctness` is for exploratory diagnostics only:
  it records that fact and cannot save a decision-grade baseline.
- Runtime rows reuse a `VerifiedProgram`; compiler and verifier cost is kept in
  the compiler, verifier, cache, and API rows.
- The table reports best and mean wall time plus coefficient of variation.
- Windows reports per-thread CPU time and scheduled cycles using
  `GetThreadTimes` and `QueryThreadCycleTime`. The Windows CPU-time clock is
  coarse; rows shorter than its update quantum may show `-` rather than a
  misleading zero or overcount.
- The short Windows 64-byte/4-KiB allocation, 4-KiB zero-fill, and
  64-to-4096 resize rows use 32 times their original base iterations to reduce
  CPU-accounting quantization. Linux counts are unchanged. This does not
  guarantee precise CPU measurements on every machine; inspect batch duration
  and use the same additional `--sample-scale` for both compared binaries.
- Linux reports high-resolution per-thread CPU time using
  `CLOCK_THREAD_CPUTIME_ID`. On x86/x86_64 it also reports fenced TSC cycles;
  other Linux architectures show no built-in cycle value.
- JSON baselines store wall time, CPU time, and cycle metrics. Linux TSC cycles
  include descheduling, so use `perf stat` when retired instructions, hardware
  cycles, branches, or branch misses are needed.

Use at least seven repeats for optimization decisions and rerun any row with a
coefficient of variation above 10 percent. The harness records a
decision-eligibility verdict and refuses to save a baseline when pre-timing
correctness was skipped, `--quick` was used, there are fewer than seven
repeats, or any row exceeds its CV limit:

```text
cargo build --release --manifest-path crates/tinyone_core/Cargo.toml --bin tinylang_bench
./target/release/tinylang_bench --repeats 7
```

Hot-loop decision rows use a stricter 5 percent coefficient-of-variation
limit. The harness marks rows above their applicable limit with `!`. A normal
`--baseline` comparison remains useful for diagnostics, but prints that it
cannot support an optimization claim if either side is not decision-grade.

For a bounded Windows scheduling diagnostic, both frozen binaries may be run
with the same explicit benchmark-process affinity. Record the actual logical
processor mask, process priority and unchanged power policy outside the
version-3 JSON, and use a distinct `--machine-label` for that mask. Do not pair
captures with different affinities or weaken the existing quality gates.
Affinity can reduce variance without improving absolute throughput; it is a
measurement condition, not a runtime optimization. A same-binary restricted /
unrestricted check can expose scheduling-sensitive variability without proving
its exact cause. Windows scheduled-cycle counts are not retired-instruction
counts and must not be converted to elapsed time; see the
[QueryThreadCycleTime contract](https://learn.microsoft.com/en-us/windows/win32/api/realtimeapiset/nf-realtimeapiset-querythreadcycletime).

Save machine-local baselines under the platform-specific
`target/perf/` tree with:

```text
./target/release/tinylang_bench --repeats 7 \
  --machine-label workstation --power-policy performance \
  --save-baseline-auto
```

Automatic baseline files use schema version 3. Each file contains the result
rows plus the timestamp, package and Rust versions, build profile, OS,
architecture, CPU model, Git commit and dirty state, filesystem context,
measurement options, and optional machine/power-policy labels. Baseline
comparison continues to accept the original result-array format, but legacy
arrays have no quality metadata and therefore cannot pass a decision gate.

Validate the Priority 3 performance criterion against a clean saved baseline
with an all-row, normalized capture (increase `--sample-scale` if a row is
noisy, and keep it identical for each side of a pair):

```text
./target/release/tinylang_bench --priority-3-only --repeats 7 \
  --sample-scale 16 \
  --baseline target/perf/<platform>/baseline-<commit>-<time>.json \
  --priority-3-gate
```

The gate requires decision-grade current and baseline runs and requires the
`runtime.jit_vec_push_pop_256`, `runtime.jit_map_set_get_256`, and
`runtime.jit_heap_churn` rows to improve by at least 10 percent in CPU time or
cycles. It deliberately fails rather than treating a near miss as a pass.

Priority 5 uses a separate guardrail gate. It selects the planned 64-byte and
4-KiB Ralloc allocation rows, 64-to-4096 resize, memory reset, and memory
snapshot; it rejects a regression above 5% in paired mean CPU time or cycles,
falling back to mean wall time only when neither primary metric is available.
The baseline must have matching platform, machine-label, power-policy, repeat,
and sample-scale metadata, and must record `priority_5_only: true`. Each row
must also have the same actual iteration count; old shorter-batch baselines
cannot pass the gate against the longer-batch harness:

```text
./target/release/tinylang_bench --priority-5-only --repeats 7 \
  --sample-scale 4 --machine-label workstation --power-policy performance \
  --save-baseline target/perf/windows-native/priority-5-guardrails-baseline-final.json
```

```text
./target/release/tinylang_bench --priority-5-only --priority-5-gate \
  --repeats 7 --sample-scale 4 --machine-label workstation --power-policy performance \
  --baseline target/perf/windows-native/priority-5-guardrails-baseline-final.json \
  --save-baseline target/perf/windows-native/priority-5-guardrails-current-final.json
```

The accepted Windows-native and Arch WSL pairs both used `--sample-scale 4`.
All ten saved rows have CV below 5%, and all 22 pre-timing parity checks passed
in every capture. Windows' largest adverse paired mean primary metric is
+0.59% (64-byte allocation cycles); every Linux primary metric improved, with
snapshot cycles down 4.74%. Earlier runs that exceeded 5% or failed the
decision-quality save were rejected and remain diagnostic only.

When Windows and WSL use the same mounted checkout, keep their Cargo artifacts
separate even though they share the source tree. From WSL at the repository
root, set:

```text
export CARGO_TARGET_DIR="$PWD/target/linux"
```

Then use `$CARGO_TARGET_DIR/release/tinylang_bench` as the benchmark binary.
This prevents one operating system from replacing the other's binaries and
Cargo fingerprints.

Comparisons built from different source roots also need fresh, isolated Cargo
artifact directories for each platform and variant. Account for any separate
build-directory override as well as the target directory; Cargo documents both
in its [build-cache layout](https://doc.rust-lang.org/cargo/reference/build-cache.html).
In this workflow a shared target directory reused the control executable for
the candidate, invalidating an otherwise successful allocation diagnostic.
Preserve such results as excluded evidence rather than a before/after pair.

Build with `--message-format=json`; check the compared package's manifest path,
features and `fresh: false`, and resolve the reported `executable` within the
intended isolated artifact directory. Cargo's
[artifact messages](https://doc.rust-lang.org/cargo/reference/external-tools.html#artifact-messages)
provide these fields. Do not assume integration-test executables always live
under `release/deps`. Record source hashes, freeze each executable and record
its hash before running it. A successful build or a source snapshot alone does
not establish the intended source/executable pairing.

## Workload map

| Prefix / pair | Isolated cost |
| --- | --- |
| `allocator.*` | Ralloc allocation, zero-fill, growth, arena-boundary scans, fragmentation, and multithread contention |
| `memory.*` | Ralloc-backed VM slots, reset, load/store, and snapshots |
| `frontend.*`, `compiler.*`, `optimizer.*`, `verifier.*` | Source pipeline stages |
| `frontend.lex_line_heavy_4096`, `compile.line_heavy_4096` | Source-copy and line-index attribution: 4,096 comment lines followed by `print 42`; not general large-program throughput |
| `compile.declarations_256` | Full compilation of 256 function declarations, local-name tables and two direct calls; compare with ordinary compilation and uncached module graphs |
| `optimizer.declarations_256`, `optimizer.constant_chain_128` | Unchanged multi-function bytecode versus repeated constant-folding passes; excludes source compilation but includes the shared-input API's required program clone |
| `compile.constant_chain_128` | Full verified compilation of the existing 128-term integer-chain fixture; pairs with optimizer-only chain cost and reuses the existing golden checks |
| `compiler.file_modules_*` | Multi-file compile versus the size-aware disk-cache policy |
| `compiler.module_graph_{small,medium,large}_*` | Disk-cache bypass/hit behavior over 2, 16, and 64 imported modules, plus a medium incremental row |
| `compiler.cache_phase.*` | Metadata/input I/O, metadata-prefilter `stat`, metadata decode, hashing, canonicalization, artifact loading, verification, and fingerprint comparison |
| `program.*` | Fingerprints and JSON/binary artifact conversion |
| `jit.codegen_*`, `jit.cache_hit_*` | Lowering and verified/unverified cache lookup |
| `jit.codegen_float_loop_cold` | Fresh cache construction and lowering of the same verified float-loop fixture used in runtime timing; no source compilation or loop execution in the interval |
| `jit.codegen_dead_stores_{1024,65536}_cold`, `jit.codegen_copy_stores_32768_cold`, `jit.codegen_expression_chain_65536_cold`, `jit.codegen_stored_expression_chain_65536_cold` | Fresh cache construction, lowering and drop for raw verified action-heavy/deep-expression inputs, including a late source-origin gap after a long expression chain; bypasses frontend folding, with input verification and VM/JIT output checks outside timing |
| `jit.codegen_sparse_feedback_cold`, `jit.cold_run_sparse_feedback` | Fresh main lowering versus fresh lowering plus first execution of the padded feedback fixture; verification is outside both intervals |
| `jit.execution_context_setup`, `jit.operand_stack_*`, `jit.chunk_dispatch`, `jit.calls`, `jit.back_edge_promotion` | JIT execution setup, transient stack allocation/reuse, dispatch, calls, and adaptive promotion |
| `runtime.vm_*`, `runtime.jit_*` | Execution after verification |
| `runtime.*_hot_loop_4096*` | Dispatch and quickening over 4,096 controlled iterations |
| `runtime.*_float_loop_4096`, `runtime.*_mixed_loop_4096` | Stable floating-point arithmetic and one I64-to-F64 operand change over 4,096 iterations; generic numeric execution remains the fallback |
| `runtime.*_closure_calls_512` | 512 dynamic calls through a closure with one capture, including fresh closure construction and cleanup per execution |
| `runtime.*_function_value_calls_512` | 512 calls through a function value with two explicit arguments, alongside the corresponding closure fixture |
| `call_cold.{off,on}.*` | Fresh standalone lowering, first execution and complete drop for static, function-value, closure and alternating-call fixtures at 0/1/8/32 calls; source compilation, verification and fingerprint initialization are outside timing |
| `runtime.*_slot_{compare,mul,div}_immediate_4096` | Isolated slot/immediate guard, multiply, and floor-divide paths |
| `runtime.*_sparse_feedback_loop_4096` | A scalar loop between two non-executed 2,048-print padding regions; attributes feedback eligibility versus lowered code size, not general large-program throughput |
| `runtime.*_vec_push_pop_{16,256,4096}` | Size-scaled vector behavior |
| `runtime.*_vec_{push_in_capacity,capacity_growth,pop,clear}_*` | Individual vector mutation phases |
| `runtime.*_map_set_get_{16,256,4096}` | Size-scaled map behavior |
| `runtime.*_map_{hit,miss,update,insert_in_capacity,delete,capacity_growth,pointer_key_validation}_*` | Map lookup, mutation, and growth phases; the legacy `pointer_key_validation` fixture uses heap-cell identity keys, not raw pointers |
| `runtime.*_map_string_lookup_4096` | Eight string keys and 4,096 content-key lookups, including fresh map construction and cleanup; a non-integer-key guardrail, not an isolated hash microbenchmark |
| `runtime.*_heap_{allocation,lookup,load,store,free,slot_reuse}_*` | Individual heap lifecycle phases |
| `runtime.*_retained_{cells,strings,structs,enums}_{16,256,4096}`, `jit.cold_run_retained_*_256` | Construction, retained readback and shutdown of shared retained-object fixtures; warm VM/JIT reuse verified code with fresh heaps, while cold JIT includes lowering and drop. Golden output and VM/JIT parity run outside timing. |
| `api.*` | End-to-end public API cost, including compilation where applicable |

Build with `--features testing-hooks` to use
`tinyone::testing::{reset_runtime_cost_counters, runtime_cost_counters}`. The
process-wide diagnostic counters report heap-lock acquisitions, encoded-value
encodes/decodes, successful Ralloc buffer growth events, and bytes copied by
allocator moves. Reset them only around an exclusive measurement interval;
they deliberately aggregate work from spawned runtime threads.

For combined runtime attribution, run the process-isolated diagnostic:

```text
cargo test -p tinylang --release --features testing-hooks --test allocation_profile -- --ignored --nocapture --test-threads=1
```

It covers 15 sources at thresholds 0/8, with separate first/repeated executions
(60 cases). The nested integer, stable-float and changing-operand loops match
the arithmetic shapes in the runtime timing rows. Each interval reports
opcode/stack/adaptation activity, value encode/decode counts, heap-lock counts,
heap statistics and the complete Ralloc snapshot. The single reset covers both
counter systems, and their growth/copy totals must agree. Source compilation
is outside the interval; execution, heap shutdown and returned-memory cleanup
are inside. Every case compares output with the portable VM and checks complete
Ralloc cleanup. These instrumented work counts are not latency measurements.

The retained-object timing rows share their sources and golden checksums with
a separate scaling diagnostic:

```text
cargo test -p tinylang --release --features testing-hooks --test retained_allocation_profile -- --ignored --nocapture --test-threads=1
```

It covers cells, strings, standalone structs and enums at 0, 1, 16, 256 and
4,096 retained objects, thresholds 0/8 and first/repeated execution (80 cases).
The root array keeps every object live through value/length readback. Compilation
and VM reference execution are outside the counter interval; construction,
readback and complete native cleanup are inside. Allocation sizes, alignment,
growth/copy work and header visits are reported separately from latency. Run
this process-wide diagnostic alone, never concurrently with timing captures.

For real native-exhaustion and recovery checks, run:

```text
cargo test -p tinylang --features testing-hooks --test allocation_failure -- --nocapture --test-threads=1
```

This single-test harness reserves fixed-pool storage until no 512-KiB request
fits, then checks failed byte/VM-allocation growth, fallible frame cloning,
and main-frame, callee-frame, callee-buffer and array-growth failures. Portable
VM and JIT thresholds 0/1/8 must agree on output and full errors, release every
new native allocation, and recover after pressure is removed. The same JIT
instances are reused. This is artificial allocator-pressure correctness
coverage, not a representative memory baseline, host-OOM test or latency run.
See the [ownership and exhaustion audit](optimization-worklog.md#ralloc-ownership-on-unwind-and-native-exhaustion).

For feedback storage versus code size, run:

```text
cargo test -p tinylang --lib report_sparse_feedback_storage -- --ignored --nocapture
```

This reports six prefix/suffix padding shapes at thresholds 0/1/8 (18 cases),
including retained-byte charges, lowered operation count and specialization
activity. The padding does not execute. Pair the corresponding runtime and
cold benchmark rows with normal loops, calls and collections: a padded-case
improvement alone does not justify changing every site's feedback lookup.
The dense-span prototype was removed after consistent mixed-loop regressions;
a distinct opcode-local-ID prototype was also removed after ordinary Linux
hot/mixed/map slowdowns and a Windows cold-code guardrail miss. Its Linux
sparse-target gains do not justify the broader tradeoff; all Windows sparse
pairs were too variable to use. See the
[feedback-ID trial](optimization-worklog.md#opcode-local-feedback-ids-rejected-by-ordinary-workload-guardrails).
The current implementation still budgets a full-chunk boxed table.

For complete-verifier host allocation attribution, run:

```text
cargo test -p tinylang --release --features testing-hooks --test verifier_profile report_verifier_allocations -- --exact --ignored --nocapture --test-threads=1
```

The test-only observer counts the public verifier, including module tables,
traversal scratch and certificate construction/drop. Fourteen raw metadata
shapes, 0/1/32 calls and three samples produce 126 records per OS. Setup,
VM/JIT output checks, Program cloning and reporting are outside observation.
Every interval checks balanced host owners, unchanged input and zero
runtime/Ralloc counters. These are build-specific allocation requests, not
label-only attribution, bytes copied, timing or peak memory.

Ordinary `verifier.functions_256`, `verifier.names_4096` and
`verifier.struct_fields_256` rows share those raw fixtures;
`verifier.declarations_256` reuses the real declaration-heavy source.
Compilation and fixture checks are outside timing. The
[verifier metadata baseline](optimization-worklog.md#verifier-metadata-allocation-and-timing-baseline)
records 252 matching counter observations and 28 eligible ordinary captures,
including the original Windows log-parser failure and read-only recovery.
A subsequent
[lazy-label trial](optimization-worklog.md#lazy-verifier-diagnostic-labels-rejected-whole-system-trial)
adds 252 matched allocation observations and 104 completed ordinary captures.
Its usable verifier pairs improve, but two eligible Windows warm-API pairs
regress (+6.29% / +1.26% scheduled cycles). The candidate was removed, three
semantic tests remain, and the 48 Linux guardrail captures were not run after
the Windows rejection. Quality exclusions and the coarse Windows CPU-time
clock's limitations remain explicit; no verifier optimization or Vec/Box
advantage is established.

The historical `compiler.emit_bytecode` row includes lexing, parsing/emission
and mandatory verification through `compile_source_unoptimized`. Its name and
interval are preserved; it is not emission-only timing.

For explicit source-pipeline wall-clock attribution, use the testing-only
diagnostics below, one process at a time after release builds have finished:

```text
cargo test -p tinylang --release --features testing-hooks --lib compiler::profile::tests::report_empty_frontend_clock_floor -- --ignored --exact --nocapture --test-threads=1
cargo test -p tinylang --release --features testing-hooks --test frontend_profile report_source_compilation_phases -- --ignored --exact --nocapture --test-threads=1
```

The explicit capture entry point calls the existing constructor, compiler,
peephole optimizer and verifier; the ordinary compiler has no observer calls.
Its four fixed records distinguish construction/source ownership/lexing;
fused parsing, semantic/name work, emission and assembly; optional peephole
optimization; and verification/certificate/verified-owner construction.
`NotRun`, `Succeeded` and `Failed` are distinct states, including after errors.
There is no AST or independently timed parsing/name-resolution/emission pass.

Eight source shapes and both optimization modes produce seven alternating
control/observed batches each: 112 batch rows and 448 phase rows per platform.
Both batch routes construct and drop fresh verified owners. Named phases
exclude final returned-owner destruction, lazy fingerprints and reporting;
source generation and output checks are outside timing. The control is the
corresponding verified public API in the same hooks executable, not the
legacy `compiler.emit_bytecode` interval. Differences include observation,
aggregation and wrapper code generation; they are not production speedups.

Report the seven empty-recorder floor samples separately without subtraction.
Keep every raw sample and flag high variation. The
[fresh source-stage checkpoint](optimization-worklog.md#explicit-source-compilation-stage-attribution)
records exact frozen builds, sequential runs, all rows and sample CVs. It
identifies source-shape-specific costs without accepting an optimization.
Module/file resolution, disk caching, certificate-only costs and finer
attribution within the fused compiler pass remain outside this diagnostic.

For host argument/capture allocation attribution, run the isolated diagnostic:

```text
cargo test -p tinylang --release --features testing-hooks --test call_profile -- --ignored --nocapture --test-threads=1
```

Its counting `System` wrapper belongs only to that test binary, never to the
latency harness or production runtime. It covers direct calls, function values
and closures with 0/1/8/32 captures, at 0/1/512 calls and thresholds 0/8, with
call feedback both off/on and separate first/repeated executions (144 cases).
A separate compile-only interval checks the optional detailed-profile allocation.
Source compilation, output-buffer allocation
and reporting are outside each interval; lazy callee lowering and run cleanup
are inside. Allocation-size buckets are <=64, <=256, <=1024 and larger bytes;
reallocation bytes count requested new extents, not bytes actually copied.
These are build-specific diagnostic counts, not timing, RSS or allocator
guarantees. Retained code/cleared operand buffers mean host allocation and
deallocation counts need not balance. Ralloc cleanup is checked separately.

For host allocation during map removal, run:

```text
cargo test -p tinylang --release --features testing-hooks --test map_profile report_map_removal_allocation_profiles -- --exact --ignored --nocapture --test-threads=1
```

This reuses the same test-only counter implementation. It covers integer,
string-content, heap-reference and raw-pointer keys; front/back deletion and
deletion followed by reinsertion; sizes 0/1/256; thresholds 0/8; and separate
first/repeated executions (144 cases). Every case checks exact expected output
and VM parity, repeated missing-key deletion, complete Ralloc cleanup and
agreement between runtime and allocator growth/copy counters. Construction,
mutation and cleanup are inside the interval; compilation, output reservation
and reporting are outside. Requested host bytes are cumulative allocation
traffic, not peak memory or retained capacity. Normal release benchmark rows
remain necessary to establish latency improvements.

For host allocation during fixed-cardinality map queries, run separately:

```text
cargo test -p tinylang --release --features testing-hooks --test map_profile report_map_query_allocation_profiles -- --exact --ignored --nocapture --test-threads=1
```

This keeps a one-entry map and measures get, has, missing has, missing delete
and existing-key update with integer, 8-byte string, 256-byte string and field
pointer keys. Counts 0/1/4,096, thresholds 0/8 and first/repeated executions
produce 240 cases. The same output, cleanup and interval rules apply. The
zero/one-query controls separate recurring key-copy traffic from setup and
growth. Field-pointer totals also include other metadata copies in the runtime.
The [two-platform control baseline](optimization-worklog.md#constant-cardinality-map-query-allocation-baseline)
finds one allocation per string query and two per field-pointer query in the
repeated threshold-8 cases. These are build-specific allocation observations,
not latency gains or a change to the Vec/Box ownership policy.

The subsequent [borrowed-query trial](optimization-worklog.md#borrowed-map-queries-rejected-by-ordinary-mutation-workloads)
removes one profiled text-key allocation per query, but is rejected after
repeatable ordinary map-mutation regressions despite faster string reads.
Production ownership is restored; the new semantic boundary tests remain.
All 480 restored query-counter records match the original controls exactly.
Allocation savings alone do not establish runtime acceptance.

A separate [get/has-only trial](optimization-worklog.md#read-only-map-borrowing-rejected-by-mutation-screen)
preserves mutation ownership and function bodies, but is also rejected:
Windows updates regress by 2.20% and 1.51% in the two eligible orders.
Its read-only allocation savings leave mutation counters unchanged. Production
is restored exactly; the 24-capture screen stops the 112 conditional captures.

For host allocation during JIT preparation, run:

```text
cargo test -p tinylang --release --features testing-hooks --test jit_preparation_profile -- --ignored --nocapture --test-threads=1
```

Eleven fixtures cover source print/straight-line/float-loop/functions, raw expression
depths 4/1,024/65,536, the stored-expression chain with a late origin gap,
and the three store-heavy inputs shared with the timing
harness. Three samples each construct and drop fresh standalone code or a fresh
cache (66 records). Immutable input memoization is primed outside the interval;
compiled code is not reused. Every interval checks balanced host allocations,
zero Ralloc execution payloads, and lazy function installation. Requested
reallocation extents are not copied bytes, peak memory or RSS. The five raw
timing fixtures also check VM/JIT parity during benchmark registration, in
addition to the 49 printed pre-timing checks.

For opt-in wall-clock attribution within preparation, build first and then run
the two diagnostics separately, with no other measurement or build running:

```text
cargo test -p tinylang --release --features testing-hooks --locked --offline --lib --test jit_preparation_phases --no-run
cargo test -p tinylang --release --features testing-hooks --locked --offline --lib report_empty_phase_clock_floor -- --ignored --nocapture --test-threads=1
cargo test -p tinylang --release --features testing-hooks --locked --offline --test jit_preparation_phases -- --ignored --nocapture --test-threads=1
```

`testing::capture_jit_preparation` records 17 disjoint phases: chunk setup,
branch-map filling, symbolic construction, unreachable encoding, stack and
definition finalization, each ordered optimizer pass, encoding, target
remapping, and origin/chunk installation. Slot-version propagation is counted
only when it runs. Captures are current-thread-only; nested capture is an
error. The record is a fixed array, boxed only during an explicit capture.
No per-phase allocation is needed, inactive captures do not read clocks, and
the module and observation calls are absent without `testing-hooks`.

The phase test shares all eleven allocation fixtures. It primes immutable
input metadata, constructs and drops fresh main code through standalone/cache
routes, and alternates active/inactive observation order for seven samples
(154 batch records and 2,618 phase records). Source compilation, verification
and output-parity checks are outside these intervals. Lazy functions are
covered by separate correctness tests, not by these latency records. The
capture total includes caller, accounting, destruction and observation work
outside the named spans, but excludes capture allocation/report extraction.

These are **hooks-build diagnostics**, not ordinary-release per-pass costs.
Clock reads distort tiny phases; even same-binary active/inactive differences
can be noisy or negative. Report raw variation and the empty-span clock floor;
do not subtract that floor to manufacture precise pass costs. The comparison
does not measure hooks-versus-ordinary code-generation overhead. See the
[scoped preparation attribution results](optimization-worklog.md#scoped-preparation-attribution-with-observer-limits).

`compiler.cache_phase.binary_decode_verify` retains the public artifact
loader's mandatory verification. Compare it with the separate
`compiler.cache_phase.verification` row to attribute verifier cost without
introducing an unverified artifact-loading API solely for benchmarking.

File fixtures default to the platform temporary directory. Set
`TINYONE_BENCH_FIXTURE_ROOT` to measure a specific filesystem. Benchmark JSON
records both the process working-directory filesystem and the fixture
filesystem, which distinguishes WSL `/mnt/c` runs from WSL native `/tmp` runs.

The line-heavy and 256-declaration sources are built outside the timed interval.
Both have VM/JIT correctness cases. A separate 128-term constant chain and
float, changing-type, closure-call and function-value fixtures, polymorphic and
unstable calls, five explicit call-feedback opt-in cases, and padded-feedback
and string-map VM/JIT pairs bring the current pre-timing suite to 49 checks.
Pair the line-heavy attribution rows with ordinary function-heavy
compilation before making a broad frontend claim; comments produce almost no
token or bytecode work. Declaration-heavy source adds actual token, symbol,
function-buffer, optimization and verification work; module-graph rows also
include filesystem and import-resolution costs.

The `call_feedback.off.*` and `call_feedback.on.*` rows isolate the experimental
dynamic-call option in one executable. Each pair shares source, verified
metadata, iteration count and default scalar quickening; detailed profiles are
disabled on both paths. The rows cover static calls as a guardrail, function
values, closures, polymorphic sites and unstable sites. Use separate filtered
invocations in both orders with identical measurement parameters, and retain
failed evidence gates. This is attribution, not proof that the complete
feature improves over the pre-feedback executable. Default JIT rows keep call
feedback off until its latency acceptance is established.

The separate cold-call rows construct and drop a fresh program on every
invocation, including the three harness warmups. Both option choices retain
normal scalar quickening and disable detailed profiles. Registration checks
16 VM and 32 JIT golden outputs in addition to the 49 printed prechecks.
The [short-call startup baseline](optimization-worklog.md#fresh-short-call-feedback-startup-baseline)
records 128 fixed ordinary captures, 58 eligible pairs and eight noisy
captures without retries. Thirty-two-call closures are slower with feedback
in both orders on both platforms; mixed and zero-call control results remain
explicit. This is cold-lifecycle option attribution, not isolated handler
cost or whole-feature acceptance. Call feedback stays off by default, and
the Vec/Box ownership policy is unchanged.

The subsequent [settled-guard trial](optimization-worklog.md#settled-initial-call-guards-rejected-by-cold-call-screen)
skips initial no-miss opcode rewrites, but fails its repeatable-benefit screen:
the reverse Linux closure target is 6.00% slower despite first-order gains.
All 48 captures are eligible and retained. Production is restored; three
semantic regression tests remain. No container change or default-enable
decision follows from this trial.

The optimizer rows reuse compiled input, so `optimize_program` clones the
shared program before consuming it. This intentional API copy is included on
both sides of a comparison; internal optimizer copies are a separate cost.
The straight-line and loop-passthrough fixtures have no foldable constants.
Use `optimizer.constant_chain_128` as the shrinking-path guardrail, and pair
optimizer-only results with `compile.constant_chain_128` and
`compile.full_pipeline` for source-specific and ordinary end-to-end attribution.
The [continuation-folding trial](optimization-worklog.md#constant-chain-continuation-trial-rejected-production-restored)
improved chain timings but regressed guardrails; both production candidates
were removed. Its 132 raw captures, including quality exclusions, are retained.
The additional benchmark and semantic tests do not establish an accepted
optimizer speedup or change the Vec/Box ownership policy.

## Implemented optimizations

The first optimization pass produced three material improvements on the Windows
baseline machine:

1. Repeated `JitCache::run_source` now caches the exact source's
   `VerifiedProgram`. Together with the integer runtime fast paths, the warm
   source API row fell from about 52.8 us / 137,000 cycles to 21-28 us /
   54,000-71,000 cycles across clean runs, a 47-60 percent reduction. Digest
   buckets still require exact source equality, and first use is still fully
   compiled and verified. Retention is bounded by a 128-entry/8-MiB
   deterministic LRU policy, with hit, miss, compilation, eviction, bypass,
   and retained-byte statistics available through `JitCacheStats`.
   The later [clock-rollover correctness fix](optimization-worklog.md#source-cache-clock-rollover-correctness-fix-limited-overhead-evidence)
   preserves that LRU policy at counter overflow. Its fixed normal-path
   comparison has Windows confirmation gaps and establishes no added speedup.
   Its raw artifacts became unavailable before final integrity verification;
   the linked worklog records that interruption. A [fresh correctness checkpoint](optimization-worklog.md#fresh-current-source-correctness-checkpoint-after-evidence-loss)
   verifies the preserved current implementation and bounded storage policy
   on Windows and Linux. It does not restore the missing timing evidence or
   establish Vec speed superiority over boxed slices.
2. Quickened I64 arithmetic now has checked I64 fast paths with generic
   fallbacks, and Ralloc-backed VM slots update I64 immediates in place. Fused
   slot/immediate multiply, floor-divide, compare/jump, zero-jump, and
   slot-to-slot move operations remove
   additional dispatches, decoding, and operand-stack moves while preserving
   branch-target safety and generic numeric behavior. The
   quickened 4,096-iteration JIT loop fell from about 930 us / 2.41 million
   cycles to 559 us / 1.43 million cycles, roughly 40 percent. The quickened
   tier now uses about 27 percent fewer cycles than the no-quickening tier.
   The later branch-safe slot superinstruction pass reduced the same Windows
   row from 598 us / 1.55 million scheduled cycles to 355 us / 0.92 million
   (about 41 percent). On Arch WSL it moved from 461 us / 1.19 million TSC
   cycles to 276 us / 0.71 million (about 40 percent). Identical Linux `perf`
   workloads fell from 5.734 billion to 3.454 billion retired instructions
   (40 percent) and from 2.511 billion to 1.810 billion hardware cycles
   (28 percent). Packed verifier-bounded operands keep `JitOp` at 24 bytes, so
   the new guard does not inflate every cold lowered operation.
3. The completed collection/heap pass removes several shared fixed costs.
   Indexed map operations use one heap-lock window, canonical hits decode only
   the selected value, and pointer-key validation reads encoded bases under
   that lock. Direct JIT builtins move operands without generic name dispatch;
   I64 map set/get, `len(slot) > 0`, map set with a slot/immediate product, and
   `total + map_get(slot, slot)` have branch-safe lowered forms with generic
   fallbacks. Vector pop avoids a temporary host byte vector, collection growth
   is amortized fourfold, maps reserve their small host index, and cell churn
   reuses one fixed payload. The VM heap no longer shadow-tracks each payload
   through TinyAllocator's diagnostics, while its standalone diagnostics stay
   intact. Two decision-grade Windows captures reduced CPU/cycles by 16-18% /
   19% for vector, 23-24% / 23-24% for map, and 41% / 40-42% for heap churn.
   Two Arch WSL captures reduced them by 13-17% / 13-17%, 22-23% / 22-23%, and
   35-37% / 35-37%, respectively. All four runs executed 22
   pre-timing parity checks and stayed below 10% CV. The earlier 9.05%,
   skipped-correctness Linux vector artifact and high-CV heap artifacts remain
   diagnostic only. Pointer generations, byte budgets, and allocation totals
   remain exact.
4. The disk-cache pass combines hit and incremental probing into one metadata,
   input, resolution, and artifact-validation flow. An incremental rebuild
   reuses the changed source bytes and digest from that probe, and repeated
   aliases reuse canonical resolutions within a compile session. Cache format
   v4 adds a path/size/modification-time prefilter that rejects known-stale
   roots, newly present inputs, and multi-input edits before digest/artifact
   work; matching metadata still falls through to all content digests,
   canonical resolution checks, binary verification, and fingerprint matching.
   A same-size edit with a spoofed stored identity is parity-tested to ensure
   metadata never becomes cache trust. Tiny graphs (<=2 imports and <=4 KiB)
   bypass everywhere. Windows-native medium graphs (3-16 imports and <=64 KiB)
   now also bypass because the two prior paired captures showed only 8-11%
   cache-hit improvement, below the 20% threshold; 64-import graphs remain
   cacheable and the phase suite measures `input_metadata_prefilter` there.
   A bounded 64-entry process LRU remembers compiled bypass decisions without
   retaining or trusting output. WSL source trees on mounted Windows drives
   bypass at every graph size. The former Windows small hit was about 19%
   slower than uncached compilation; bypass was within about 1% across two full
   release runs. Windows large hits were 25% faster in both runs. On WSL's
   native `/tmp` filesystem, medium hits were 44-46% faster and two clean
   11-repeat large-graph confirmations measured 59-60%, while the small bypass
   was effectively neutral. On WSL `/mnt/c`, pre-policy medium and large hits
   were about 24 and 19% slower than recompilation, which is why that filesystem
   bypasses. The v4 decision capture ran all 22 pre-timing parity checks for
   each invocation: Windows medium bypass averaged 3.68 ms versus 3.70 ms
   uncached (CV 0.5% / 1.6%), the 65-source-path prefilter averaged 1.80 ms
   (8.2% CV), and native Arch WSL `/tmp` medium cache hit averaged 357.09 us
   versus 571.36 us uncached (37.5% wall-time reduction; CV 6.3% / 7.6%). The
   WSL `/mnt/c` medium bypass assertion also completed. Any threshold expansion
   still needs a paired capture on every affected filesystem. These captures
   predate source-origin restoration in cache format v9; they are historical
   policy evidence, not performance acceptance of the current diagnostic
   pipeline. The current source-origin comparison records higher cold-lowering
   costs on both platforms and split disk-hit results: faster than the
   pre-origin control on Windows, slower on native WSL. A separate unchanged-
   owner cache-accounting reduction improves verified compile-only lookup,
   not cold lowering. See the
   [source-origin timing checkpoint](optimization-worklog.md#source-origin-pipeline-latency-costs-and-unchanged-owner-cache-hits)
   for paired results, rejected captures and cold guardrails. Subsequent
   [preparation allocation attribution](optimization-worklog.md#origin-preparation-allocations-and-paired-action-records)
   identifies added host buffers. A combined action/origin record improved two
   small cold-lowering workloads by 6–11%, but subsequent
   [action-heavy scaling](optimization-worklog.md#action-storage-scaling-paired-records-withdrawn)
   found consistent regressions; that layout was withdrawn. Both sets of
   evidence remain, including conflicting unrestricted Linux hot-loop results
   and separate same-CPU confirmations. Fewer allocations do not establish
   faster lowering. A subsequent [compact-target-only trial](optimization-worklog.md#compact-action-targets-smaller-records-rejected)
   kept origins separate and reduced action records from 16 to 12 bytes, but
   was also removed: two eligible Linux copy-store pairs regress 18.65%/8.35%
   despite lower scratch allocation requests. A subsequent
   [consecutive-expression-origin trial](optimization-worklog.md#consecutive-expression-origins-scratch-savings-rejected)
   eliminates about 1 MiB of deep expression-origin backing, but two Linux
   orders regress the target chain by 4.79%/5.28% and a late-gap case by
   12.54%/11.57%. That prototype is also removed; the late-gap benchmark and
   corrected layout diagnostic remain. A subsequent
   [single-block map/remap bypass](optimization-worklog.md#single-block-map-and-remap-bypass-rejected)
   saves one temporary allocation and improves deep-expression preparation
   on both platforms, but is removed after two eligible Windows warm-call
   regressions of 5.79%/4.35%. Sampled source attribution is retained with its
   header/call-chain limitations, not presented as exact phase timing.
   Optional [scoped preparation attribution](optimization-worklog.md#scoped-preparation-attribution-with-observer-limits)
   now separates symbolic construction, ordered passes and encoding in the
   hooks build, with clock-floor and variation limits preserved; 132 new
   inactive-observer allocation records match the prior control exactly.
   A following [collection-prefix screen](optimization-worklog.md#collection-prefix-screening-not-retained)
   leaves allocations unchanged, but eight eligible target pairs do not show
   a repeatable cross-platform benefit. That guard is removed; only its
   matcher boundary test remains.
   Whole-pipeline performance acceptance remains open.
5. Allocator coverage now separates 4-KiB zero fill from allocation and adds
   near-arena-capacity, fragmented-arena-cycle, and persistent four-thread
   contention rows. Existing 64-byte, 4-KiB, resize, memory reset, and snapshot
   workload bodies remain unchanged; the short Windows allocator batches are
   longer as described above. Runtime profiles did not identify snapshots as a
   production hot path, so snapshot decoding was deliberately left alone.

The arithmetic implementation preserves overflow errors and falls back to the generic
numeric path for floats, unsigned values, and narrower signed integers.

A guarded encoded-I64 slot-pair prototype was evaluated and removed: it cut
the warm integer-loop's full Value decodes from 8,193 to 1, but two eligible
Linux quickened-loop comparisons were 6.66%/4.16% slower in mean CPU time.
Both Windows target pairs were excluded for variability. See the
[slot-pair trial](optimization-worklog.md#encoded-i64-slot-pair-execution-rejected-despite-fewer-decodes)
for mixed-loop gains, excluded cold captures, and the restored control.
Reduced codec counts alone are not performance acceptance.

For targeted JIT attribution, compile with
`JitOptions::with_execution_profile(true)` and read
`JitProgram::execution_profile()`. The opt-in profile reports lowered-opcode
dispatches and operand-stack pushes/pops per opcode, plus the observed maximum
stack depth. It is intentionally disabled for normal execution measurements.
Completed JIT runs retain at most eight cleared operand vectors; recursive runs
borrow distinct vectors while active and return them afterward, so reuse never
aliases an active caller's stack.

An Arch Linux WSL2 confirmation run from the shared `/mnt/c` worktree produced
the same CPU-side result. Across 11 repeats, the quickened hot loop used about
425.5 us / 1.10 million TSC cycles per iteration versus 516.8 us / 1.34 million
for the non-quickened tier, an 18 percent reduction. Identically configured
`perf stat` captures reported 5.48 billion retired instructions and 2.04
billion hardware cycles for quickened execution versus 7.40 billion
instructions and 2.47 billion cycles without quickening: 26 percent fewer
instructions and 17 percent fewer hardware cycles. The warm source API row was
17.2 us / 44,600 TSC cycles. These runtime rows are CPU-focused; filesystem
rows from `/mnt/c` also include WSL's mounted-filesystem behavior.

## Current optimization order

The first cycle-aware baseline on Windows identified these priorities. The
absolute values are machine-specific; the paired ratios are the useful signal.

1. **Keep verified capabilities on hot paths.** A JIT cache hit through a raw
   `Program` took about 33,100 cycles, while `compile_verified` took about 291
   cycles. Re-verification and re-fingerprinting dominate compatibility cache
   lookup. Optimize callers and internal flows to retain `VerifiedProgram`;
   do not weaken verification at untrusted boundaries.
2. **Preserve the measured disk-cache policy.** Tiny graphs bypass everywhere;
   Windows-native medium graphs (3-16 imports, <=64 KiB) bypass because their
   recorded hit rate missed the threshold; large graphs retain verified binary
   cache hits; and changed modules use the single-pass incremental probe.
   Revisit thresholds only with paired Windows, mounted-WSL, and native-
   filesystem measurements; mounted WSL drive trees currently bypass all sizes.
   Never replace content validation with metadata-only trust.
3. **Measure the new dispatch/stack workstream selectively.** Branch-safe
   slot moves, direct zero jumps, in-place slot multiply/divide, and bounded
   operand-stack reuse are now available alongside the earlier compare/jump
   fusions. Use the opt-in opcode/stack profile and the
   `jit.operand_stack_reuse_32` row to retain only changes that improve the
   isolated instruction/cycle evidence without a cold-code regression.
4. **Keep collection paths measured.** Priority 3 now meets its paired CPU
   work target on Windows and WSL. Retain the encoded-slot, stale-pointer, byte
   budget, and lock-acquisition tests as guardrails before pursuing bulk
   `map_keys`/`map_values` work or any larger heap architecture change.
5. **Use `memory.*` and `allocator.*` as guardrails.** These microbenchmarks
   catch regressions in slot initialization, reset, copying, arena-boundary and
   fragmented scans, contention, and reallocations that would otherwise be
   multiplied by every VM run.

Treat an optimization as credible when best wall time and the platform's
primary CPU-work metric improve across two clean runs, correctness still
passes, and variance remains below 10 percent. Prefer scheduled cycles on
Windows, thread CPU time on Linux, and `perf stat` when native instruction
counts matter. Save machine-local baselines rather than committing them,
because scheduler, CPU, power policy, and toolchain differences make absolute
numbers non-portable.
