# Total-system optimization worklog

This batch follows all 40 sections of the user-provided **TinyOne Total System
Optimization Plan**. This is an in-progress evidence log, not a completion
report. The scope includes the compiler, verifier, interpreted JIT, runtime,
Ralloc, memory, collections, capabilities, caches, and diagnostics. Native code
generation is excluded. The final acceptance audit must cover the entire plan.

The [current acceptance map](optimization-acceptance.md) reconciles all 40
sections with the latest code/evidence. The older requirement ledger below
is preserved as a historical snapshot, not the current status summary.

Evidence availability interruption (2026-09-13 UTC): the workspace's target
directory was recreated at 00:35 UTC during the source-clock final audit.
Previously observed target/perf logs, frozen binaries and source archives are
now unavailable, including earlier checkpoints referenced throughout this log.
Historical results below record observations, not currently re-auditable files.
Source changes remain intact. A [fresh current-source correctness checkpoint](#fresh-current-source-correctness-checkpoint-after-evidence-loss)
now has independently identified evidence outside target/. It does not restore
the historical timing records or close their performance-acceptance gaps.

## Starting state and documentation

- Starting revision: `523242d` (`ci: format project`), clean worktree.
- Host: Windows x86_64 MSVC; Rust 1.98.0, Cargo 1.98.0, stable toolchain.
- Located the applicable router at `C:/Users/conne/.codex/AGENTS.md`.
  No repository-local `AGENT.md` or `AGENTS.md` was found, including hidden and
  ignored source paths. Parent-directory checks found no additional guidance.
- Read `C:/Users/conne/.codex/.agents/rust/README.md` and all four required
  examples: `control-flow.rs`, `state-machine.rs`, `errors.rs`, `modules.rs`.
- Read `docs/INDEX.md`, `architecture.md`, `bytecode.md`, `vm.md`,
  `memory-model.md`, `performance.md`, `CONTRIBUTING.md`, `CI_CD.md`,
  `syntax/modules.md`, and `ffi/rust-api.md` for the first workstream.
- Treated the router's optional [stable Rust documentation](https://doc.rust-lang.org/stable/)
  as required, including the relevant
  [HashMap entry API](https://doc.rust-lang.org/stable/std/collections/hash_map/enum.Entry.html).
- Checked [blake2 0.10.6](https://docs.rs/blake2/0.10.6/blake2/) and
  [hex 0.4.3](https://docs.rs/hex/0.4.3/hex/fn.encode.html) for the existing
  fingerprint design. Read the installed `serde_json` 1.0.150 README's value
  representation/API documentation after its versioned web page failed.
  No dependency replacement is proposed.
- Ralloc documentation is chiefly inline in `crates/tinyone_ralloc/src/`, its
  C header, and the shared memory/architecture/performance pages. Allocator
  architecture/ownership gate has been read, including `lib`, `buffer`, `vm`,
  `ralloc`, `arena`, `block`, `backend`, `region`, `sync`, `page`, instrumentation,
  and the C header. Reconfirmed allocation, reallocation, coalescing and handle
  ownership before adding feature-gated instrumentation. There are no Ralloc
  production external dependencies. The lib comment refers to a missing
  `docs/clippy_doc_lints.md`; existing repository Clippy rules remain applicable.

Locked direct dependencies: `thiserror` 2.0.18, `blake2` 0.10.6, `hex` 0.4.3,
`serde` 1.0.228 (derive), `serde_json` 1.0.150, `toml` 0.8.23,
`ed25519-dalek` 2.2.0 (std, no defaults), `sha2` 0.10.9, and local
`tinyone_ralloc` 0.2.0. Ralloc has no production external dependencies. Relevant
test dependencies are `tempfile` 3.27.0 and `static_assertions` 1.1.0. Libraries
not used by a change still need their API documentation read before that change.

## Baseline evidence

- `cargo test --workspace --features testing-hooks --locked --offline`: passed
  before source changes. Ralloc's mixed-size fragmentation test and one existing
  TinyOne doctest are ignored. Windows does not select the Unix-only Ralloc C
  suite; this does not prove that suite passed.
- `cargo build --release -p tinylang --bin tinylang_bench --locked --offline`:
  passed before source changes.
- Preserved that binary as `target/perf/full-system/baseline-523242d.exe`.
  The 135-row, seven-repeat capture is recorded in
  `target/perf/full-system/baseline-all.log`; only a capture that meets the
  benchmark's quality gate may be called decision-grade.
- Baseline coverage is incomplete: the existing allocation rows combine some
  allocation/deallocation work; full size/lifetime distributions, memory
  ownership accounting, and several requested workload classes need additional
  measurement. Existing measurements must not be claimed to cover those gaps.

## Findings driving the next changes

1. `Program::fingerprint` is an artifact-content identity. It deliberately
   round-trips through generic artifact loading, which removes policy trust.
   `JitCache` currently uses only that fingerprint and can reuse a program with
   different effective runtime authority. Execution identity must also include
   policy trust, while preserving content fingerprint compatibility.
2. The verifier's stack traversal uses a hash map over instruction indexes and
   repeats immutable operand checks already performed by its static pass.
   It also models several operations only by their net stack effect: checking
   `depth + pushes - pops >= 0` does not prove all inputs exist. Correctness
   regressions must precede any optimization relying on stack metadata.
3. Public verified programs can contain chunks longer than 65,536 instructions
   (up to the 262,144 total-op budget). JIT comments incorrectly claim every
   branch index fits in 16 bits. Packed candidates currently use fallible
   conversion; tests must establish the large-target fallback and maximum-slot
   behavior before changing their representation.
4. Some VM/memory documentation still describes `Vec<Value>` frame slices;
   actual `TinyMemory` uses Ralloc-owned 64-byte encoded slots. Runtime errors
   currently carry strings without bytecode source spans. Future diagnostic
   changes must start from this actual state.

## Correctness checkpoint

- Both new verifier regressions failed against the original implementation:
  `target/perf/full-system/verifier-regressions-before.log`.
- The cache authority regression reproduced a trusted-cache entry granting
  authority to an untrusted artifact. The maximum-index test already passed,
  proving the existing wide-target fallback:
  `target/perf/full-system/jit-boundaries-before.log`.
- Execution cache keys now represent trusted/untrusted content as distinct enum
  states. Entry-based insertion also removes repeated hash lookups and one cold
  insertion key clone. Public content fingerprints and artifact formats are
  unchanged.
- The verifier now represents input/output stack effects separately and checks
  that inputs exist before counting outputs.
- `cargo test -p tinylang --lib --test optimization_boundaries --locked --offline`:
  134 unit tests and both integration tests pass after the fixes. The cache test
  covers JSON/binary, root/function bodies, generic/direct builtins, both cache
  insertion orders, repeated executions and thresholds 0/1/8. The index test
  covers slot 65,535 and branch targets 65,535, 65,536 and 262,140 in all tiers.
- Used installed nightly `rustfmt` on changed files to honor the repository's
  nightly-only formatting options; stable formatting cannot honor those options.
- The initial full benchmark sweep overlaps correctness-test builds and is an
  exploratory capture. Optimization decisions require fresh, isolated selected
  baseline/current captures using the preserved original binary.

## Measured verifier and profiling checkpoint

- Isolated verifier baseline: `baseline-verifier-2.json`, nine repeats,
  sample scale 16, all 22 correctness prechecks, accepted by the harness.
  The shorter preceding attempt failed its variance gate and is diagnostic only.
- Two accepted dense-table comparisons (`current-verifier-1.json` and
  `current-verifier-2.json`) reduced best wall time by 61-65% for loop CFGs,
  37-43% for function CFGs and 39-45% for heap/struct verification. Scheduled
  cycles improved in both captures (62-65%, 39-43% and 41-45%). Current CVs
  ranged from 0.8% to 6.3%. The indexed table uses four transient bytes per
  instruction instead of a hash-map entry. No operand checks were removed
  for these captures. This optimization is accepted; later metadata changes
  require their own comparisons.
- Full workspace tests with `testing-hooks` passed after the dense table, and
  again after allocator instrumentation. Clippy with all workspace targets,
  `testing-hooks` and `-D warnings` passed at the dense-table checkpoint.
- The verifier now also retains per-chunk reachable stack peaks and numeric
  permission-set ownership in an immutable certificate. A token clone shares
  program, certificate and memoized fingerprint through one Arc. JIT operand
  capacity uses the verified stack bound instead of instruction-count guesses;
  module ownership is no longer searched by name on JIT chunk entry.
- Static operand checks remain authoritative for unreachable code. Repeated
  immutable operand/arity checks were removed from reachable traversal. Enum
  constructor arity was missing from the static pass; it is now checked there,
  with a regression for an unreachable malformed constructor.
- Certificate/cleanup unit, optimization-boundary, runtime-parity and module
  sandbox tests passed. Fresh release benchmark `certificate.exe` is preserved;
  its comparison is still under evaluation, not an accepted speedup claim.
- The pristine 15-row JIT baseline (`baseline-jit-1.json`, nine repeats, scale
  eight) passed all correctness and variance gates. Function-call execution
  measured 57.75 us best wall time / 145,310 scheduled cycles per workload.
- The pre-existing collection cost profile was explicitly run successfully.
  Full output is `collection-costs-before.log`. It reports codec and lock work,
  not the complete allocation profile requested by the plan.
- Added feature-gated Ralloc allocation/deallocation/reallocation, size and
  alignment histograms, live/peak usable bytes, and allocation/lookup/coalescing
  scan counters. These compile out without `testing-hooks` (or unit-test cfg);
  instrumented runs are for attribution, not latency claims. Live-byte gauges
  remain balanced across counter resets with outstanding allocations.
- The new diagnostic profile covers 13 workload classes in nonadaptive and
  adaptive JIT, cold and warm execution. All 52 executions matched VM output
  and released every Ralloc allocation. It found 513 total allocations for 512
  simple calls (128-byte frames), 705 for repeated shallow recursion, 516 for
  closure calls, and roughly 31,000 allocation-scan steps for 256 retained
  strings/cells. Vector/map growth copied 5,376/10,752 bytes. Cell churn already
  reuses one payload (two total Ralloc allocations including main memory).
  Evidence: `allocation-distributions-before-frames.log`.
- Re-enabled the mixed-size fragmentation test: its ignored marker conversion
  rejected sizes 256/512 on *all* pointer widths. Markers now intentionally use
  the size's low byte, and retained allocations are checked after hole refill.
  This is a test repair, not an allocator-performance change.
- Remaining existing exclusions: allocator-audit double-deallocation test is
  an ignored compile-time ownership note; one VM-hooks doctest is ignored;
  diagnostic profiles are explicit-only; Unix-only C tests do not run on Windows.

## Explicit frames and allocator safety checkpoint

- JIT calls now suspend explicit cursors (chunk, resume PC, operand stack) in
  an execution-local frame vector instead of recursing through the host
  interpreter. The portable VM remains the semantic reference. Depth-boundary
  tests compare exact errors/output at limits 1, 2 and 16 in all three tiers.
- Cleared exact-size Ralloc frames can be reused within a run, independently
  bounded by eight frames and 64 KiB. Idle memory is released before runtime
  heap allocation/growth and before retrying an exhausted frame allocation;
  all idle storage is dropped on either run outcome. A regression verifies
  unwritten slots start at zero on reuse and no frames survive repeated runs.
- All 52 allocation-profile executions pass after this change. For 512 calls,
  allocations fall from 513 to 2; closure workloads fall from 516 to 5, and
  repeated recursion from 705 to 201. Peak Ralloc payload is unchanged in
  these workloads. Evidence: `allocation-distributions-after-frames.log`.
  These are work-count results, not yet an accepted latency claim.
- Full workspace tests and all-target Clippy with `testing-hooks` passed after
  explicit frames. The certificate-only 15-row timing capture was rejected by
  the cold-clone row's CV; it remains diagnostic, not accepted evidence.
- Miri first found a pre-existing pointer-provenance defect in Ralloc's arena
  setup: `.addr()` was converted back into a dereferenced pointer. Arena
  metadata/payload pointers now derive from the retained backing-region pointer
  with `with_addr`, not integers or header-only shared references. The allocator
  strategy and raw C allocation contract are unchanged.
- A new Miri regression then proved safe byte reads could observe uninitialized
  padding left by a freed `RallocBox`. Owned byte allocation and growth now
  initialize their entire new logical byte range before exposing safe slices.
  TinyOne's redundant outer zero-fill was removed. Shrink/regrow, over-alignment,
  empty growth, in-place growth and moving growth have regressions.
- Strict-provenance Miri passes all 26 focused fragmentation, initialization,
  native-buffer and native-box tests (`miri-owned-buffers-after.log`). Full
  workspace tests and all-target Clippy with hooks pass after both safety fixes.
  Broader Miri and fresh timing/guardrail captures remain in progress.
- Consulted the official [Miri instructions](https://github.com/rust-lang/miri),
  [strict provenance](https://doc.rust-lang.org/stable/std/ptr/index.html#strict-provenance),
  and [pointer APIs](https://doc.rust-lang.org/stable/std/primitive.pointer.html#method.with_addr)
  before these fixes. Installed the Miri Linux sysroot in the workspace, with
  authorized dependency downloads. `runtime/limits.rs` also references a missing
  `phase_2_allocator.md`; no such file exists in this checkout.

The full basic-block/local-pass phase remains required. Frame work is isolated
from it using the measured allocation bottleneck; it does not replace it.

## Container-choice audit requested during the batch

The user specifically requested evidence for vectors over boxes. Neither is a
universal default. The audit distinguishes temporary growth from retained,
fixed-length storage and avoids individually boxing already-owning handles.

- Verification certificates already use `Box<[ChunkCertificate]>`.
- Lowering needs a temporary `Vec<JitOp>`, but installed operations, edge
  counters, and reserved lazy chunk positions never change length. They now
  expose fixed extents; quickening mutates elements, not lengths. The initial
  candidate boxed all three, but the measurements below rejected always
  shrinking the operation buffer.
- Runtime operand stacks keep `Vec<Value>`: their initialized length changes,
  values can own strings, and verified stack bounds supply initial capacity.
  A boxed capacity buffer would need explicit live-length/initialization/drop
  handling. Call frames and the bounded idle pool also change live length;
  they remain flat vectors of owning handles, not vectors of boxed frames.
- Observed x86_64 call-frame layout is 88 bytes. The current vector reserves
  0/352/704/1,408 bytes at depths 0/1/8/16, respectively, with 0/1/2/3 capacity
  changes. A maximum-depth boxed `[Option<CallFrame>; 16]` would use 1,408
  backing bytes, even for a shallow call. This is a storage comparison, not a
  claim that Vec is faster than every possible boxed-stack implementation.
- The pre-conversion JIT layout profile reports `JitOp=24`, `JitChunk=80`,
  `JitProgram=256`. Operation/counter storage retains 504 bytes for 312 live
  bytes in the loop fixture and 51,328 for 26,728 in the repeated-store fixture.
  Full results are `storage-vector-layout.log`. The all-boxed candidate measured
  `JitChunk=64`, `JitProgram=248` and removed all recorded spare operation
  capacity (`storage-boxed-layout.log`). These are Rust-owned allocation
  extents/header sizes, not RSS or host-allocator overhead measurements.
- Read the official [Vec guarantees and conversion documentation](https://doc.rust-lang.org/stable/std/vec/struct.Vec.html#method.into_boxed_slice)
  and [Box ownership documentation](https://doc.rust-lang.org/stable/std/boxed/index.html).
  Conversion discards excess capacity and may reallocate; cold lowering must
  therefore be measured, not assumed free.
- The broader Miri unit run was stopped after eight minutes in the exhaustive
  8-KiB allocation-exhaustion case so timing runs can be isolated. No UB had
  been reported, but this is **not** a passing full Miri run. Native exhaustion
  tests and the 26 focused strict-provenance tests passed; broader Miri remains
  on the acceptance ledger.
- The broad vector timing capture (`storage-vec-jit-1.log`) failed the verified
  lookup CV gate and is diagnostic only. Eight focused baseline captures at
  nine repeats / sample scale 64 passed every quality gate, covering 11 rows.
  They are named `storage-vec-<filter>-2.json`.
- Two fully accepted all-boxed comparisons (`storage-boxed-<filter>-1/2.json`)
  exposed a meaningful tiny cold-lowering regression: best time increased
  10.6-25.1%, with cycles up 10.5-24.1%. Call/dispatch execution stayed close,
  and clones improved, but those do not erase the cold-path regression. The
  unconditional operation-buffer shrink is rejected.
- The revised candidate retains boxed edge counters and reserved chunk slots
  (both already exactly sized), while `JitOps` privately owns an operation Vec
  and exposes only `[JitOp]` access. No caller can grow or truncate installed
  code. Installation reclaims spare operation storage only at 4 KiB or more;
  smaller buffers retain their allocation without a shrink. This threshold is
  a conservative cold/memory tradeoff, not a claimed universally optimal size.
  Large reclamation converts through a boxed slice and back without cloning;
  the resulting Vec has no spare capacity. Regression tests cover the tiny
  no-resize path, large reclamation and fixed extent across quickening.
- Revised layout (`storage-frozen-layout.log`): `JitChunk=72` (10% smaller than
  the original vector layout), `JitProgram=248` (down eight bytes). Small and
  loop buffers retain their original extent; repeated-store operation/counter
  storage drops from 51,328 to 26,728 bytes (24,600 bytes reclaimed). The
  constructor's 4-KiB threshold bounds retained spare operation capacity per
  chunk; it does not measure host-allocator internal fragmentation or RSS.
- Full workspace tests pass after the revision, including 143 core unit tests,
  56 runtime-parity tests and all four optimization-boundary tests. Both storage
  diagnostics were explicitly run. All-target Clippy with `testing-hooks`,
  nightly formatting checks for changed Rust files and `git diff --check` pass.
- Strict-provenance Miri also passes the other 41 Ralloc unit tests in 10.59 s
  (`miri-ralloc-unit-bounded.log`). Exactly one test was explicitly filtered:
  exhaustive all-arena 8-KiB exhaustion, which passes natively but was too slow
  in the preceding interpreter run. No full-Miri pass is claimed.
- Interleaved original/revised captures (suffix `-3`) were used to check drift
  after the first revised runs varied noticeably. Both sides passed the gate
  for the four cold-lowering rows, setup, operand reuse, dispatch, calls and
  verified lookup. Paired mean-cycle changes were -1.8%/+2.8%/-0.8%/+0.5% for
  straightline/function/heap/builtin lowering, -0.3% setup, -0.4% operand reuse,
  -2.2% dispatch, -1.7% calls and +0.8% verified lookup. These results support
  the memory/structural choice, not a general execution-speedup claim.
- The second interleaved pass reversed executable order (suffix `-4`). Its
  vector codegen capture and revised call capture failed variance gates;
  neither is accepted evidence. The separately accepted clone/promotion/lookup
  pairs changed mean cycles by +3.4%/-5.8%/+0.3%. Earlier phase-3 clone and
  promotion captures also had a rejected side. Do not cherry-pick rows from
  those rejected captures or claim two clean interleaved confirmations of all
  paths. Further timing acceptance remains part of the full-batch audit.
- Retained decision: fixed boxed tables plus slice-only `JitOps`, bounded live
  vectors, and thresholded large-buffer reclamation. This removes the measured
  unconditional-shrink cold regression and bounds spare retained operations.
  The 24,600-byte repeated-store saving and smaller headers are direct layout
  evidence. This is not acceptance of the still-pending whole-system batch.
- The follow-up clone audit reproduced two bugs with failing regressions:
  derived cloning of `Vec<Vec<Value>>` filled the bounded operand pool with
  empty, zero-capacity vectors while copying stale retained-capacity stats;
  checking out a stack also left the idle-storage gauges unchanged. An explicit
  `JitProgram::clone` now copies compiled/adaptive state and historical counters
  but starts with no scratch pool and zero retained-stack gauges. Checkout and
  recycling update those gauges incrementally, removing the prior capacity
  rescan. Tests cover a full eight-entry source pool, successful reuse in the
  clone, unchanged source storage, cloned execution, checkout/recycle accounting
  and zero-capacity rejection. Both regressions pass after the fix.
- The clone/pool follow-up passes full workspace tests with `testing-hooks`
  (`workspace-stack-pool-clone.log`) and all-target Clippy with warnings denied
  (`clippy-stack-pool-clone.log`). This is a correctness and accounting fix;
  no new measured clone-throughput improvement is claimed.

## Verified basic blocks and symbolic-stack lowering

- The verifier now retains a 12-byte `BasicBlock` per range, including the
  canonical reachable entry depth. `None` means unreachable, not depth zero.
  Starts come from entry, branch targets and instructions after terminators.
  Target validation is still authoritative and retains its existing diagnostic.
  The new `BytecodeOffset(u32)` represents the whole 262,144-op budget; typed
  `SlotId(u16)` represents indexes through 65,535, not a narrowed slot count.
- Block metadata is boxed and shared in the verification certificate. Lowering
  no longer constructs a branch-target `HashSet` or repeats interior-target
  checks in every fusion family. Unreachable instructions remain statically
  checked and receive ordinary encoding, without invented symbolic stack facts.
- A block-local expression arena and symbolic root stack now track literals,
  local reads and binary expressions. Incoming values stay on the physical
  operand stack. Writes and observable/effectful operations materialize older
  roots in order; block exits restore canonical stack state. An explicit
  postorder work vector handles deep expressions without host recursion.
- General `BinarySlotsStore` and `BinarySlotIntStore` families cover arithmetic
  and comparisons. Existing in-place encodings are selected from expressions.
  Six scalar source-pattern matchers were removed. Three collection idioms
  remain explicit effect barriers during the continuing local-IR migration.
  Literal branch folding and elimination of unused non-failing temporaries
  work; potentially failing discarded arithmetic is still evaluated.
- Regression coverage compares output, final scalar memory and exact errors
  against the VM at JIT thresholds 0/1/8, including repeated runs, overflow,
  division, signed zero, NaN, invalid scalar kinds, nonadjacent producers,
  overwritten-slot snapshots, failure ordering and 65,536-level expressions.
  Certificate tests cover non-empty merges, unreachable code after terminators
  and the exact maximum instruction budget. Existing all-tier wide-slot and
  wide-branch tests pass. The old `add.int` listing assertion was updated to
  require the replacement direct operation; hot-range and branch checks remain.
- Initial storage diagnostic (`storage-symbolic-1.log`): `JitOp=24`,
  `JitChunk=72`, `JitProgram=248`, `Expression=16`, `TempId=4`, traversal entry=8.
  The loop fixture drops from 312 to 234 live operation/counter bytes, while
  retained code changes from 504 to 498 bytes and adds 48 shared block bytes.
  Small/function/repeated-store fixtures add 12/36/12 shared block bytes.
  Certificate headers also grow by one boxed-slice field per chunk. These are
  logical Rust-owned extents, not RSS or allocator-internal overhead.
- The 65,536-level diagnostic retains 4 MiB of expression capacity and 2 MiB
  of traversal capacity during lowering (plus a 16-byte root buffer). This
  builder-only memory is released at chunk installation, not cached. The
  extra worst-case transient storage is real and needs continued memory review;
  it is not hidden by the unchanged installed-op size.
- First isolated before/after captures (`blocks-before/blocks-symbolic-*-1`)
  passed their nine-repeat variance gates. They exposed tiny cold-lowering
  regression (0.883 to 1.369 us best), added verifier work, and a hot-loop
  regression (395 to 456 us best). The initial scalar family also lacked the
  separate quickened add's checked I64 path. This candidate was not accepted
  as a finished performance optimization.
- The revised family uses the existing actual-tag-guarded I64 helpers with
  generic fallbacks. It does not claim per-site type feedback. Simple leaves
  no longer allocate a postorder work vector; straight-line verification no
  longer allocates a temporary leader vector. Full workspace tests and Clippy
  pass (`workspace-symbolic-5.log`, `clippy-symbolic-5.log`). Two intervening
  test-fixture mistakes were corrected: the source compiler returns an Arc,
  and source-level casts are `u8/u16/u32`, not `i16/u64` constructors.
- Reverse-order capture 2 passed all gates, but did **not** resolve the nested
  hot-loop regression. The pinned 12-op regression test confirms that this
  fixture still has the old executable sequence and does not use the new scalar
  store family. Losing that family's I64 guard therefore does not explain the
  nested-loop slowdown. Dispatch/code-generation effects are an inference to
  investigate, not a proven cause.
- The following are paired capture-2 measurements of the pre-outline revised
  candidate. Times are best microseconds; cycle changes use paired means.
  These demonstrate both benefits and unresolved costs, not full acceptance.

  | Fixture / phase | Before us | After us | Mean cycles |
  | --- | ---: | ---: | ---: |
  | Straight-line main lowering | 0.884 | 1.382 | +63.3% |
  | Function fixture main lowering | 3.086 | 1.060 | -65.6% |
  | Heap fixture main lowering | 15.755 | 1.711 | -88.7% |
  | Builtin fixture main lowering | 10.906 | 1.529 | -85.4% |
  | Loop verification | 1.013 | 1.503 | +42.3% |
  | Function verification | 5.551 | 6.386 | +12.9% |
  | Heap verification | 4.189 | 4.384 | -1.3% |
  | Quickened nested loop | 421.023 | 477.587 | +16.9% |
  | Nonquickened nested loop | 685.972 | 747.055 | +8.9% |
  | Function execution | 49.983 | 45.261 | -8.8% |
  | Straight-line dispatch | 1.139 | 0.976 | -13.8% |

  Before/after CVs are recorded in each `blocks-*-2.json`; heap-verifier before
  CV was 9.6%, so the small mean-cycle difference there is especially weak.
- A scalar-handler outlining candidate keeps its large Value temporaries and
  secondary operation dispatch outside the main opcode match. Full tests and
  Clippy pass (`workspace-symbolic-outlined.log`, `clippy-symbolic-outlined.log`).
  Its first hot-loop pair failed variance gates on both sides (48.2% and 5.1%
  CV) and is rejected, with no hot-loop JSON saved. A separately captured calls
  pair passed. The reverse-order retry also rejected the outlined side (45.2%
  CV); the baseline side passed. Neither hot-loop pair is accepted evidence.
  The outlining methods/attributes were removed, restoring the measured
  capture-2 implementation instead of retaining an unproven tuning change.
- Retention at this checkpoint is for the required block/symbolic architecture
  and demonstrated direct-operation benefits, as allowed by section 37's
  necessary-architecture exception. Tiny cold lowering, added verifier work,
  transient builder memory and the nested hot-loop regression remain explicit
  acceptance gaps. Do not claim broad no-regression or completion of this phase.
- Final retained-source checks pass: full workspace with `testing-hooks`
  (`workspace-symbolic-retained.log`: 148 core tests passed, three diagnostic
  tests ignored there and run separately), the five symbolic differential tests
  with exact float-bit signatures (`parity-symbolic-retained.log`), all-target
  Clippy (`clippy-symbolic-final.log`), nightly formatting and `git diff --check`.
  The release benchmark was rebuilt (`build-symbolic-retained.log`). Historical
  timing tables refer to their preserved capture binaries; the rebuilt file is
  not byte-identical to capture 2, and no fresh final performance pass is claimed.
- At that checkpoint the ordered local layer, slot versions, propagation,
  folding, safe dead stores and precise cold source mappings were pending.
  The following work implements the local layer; it does not complete the batch.

## Ordered block-local optimization

- `SymbolicLowerer` now builds expressions plus source-ordered definition and
  action records. `LocalOptimizer` runs constant propagation, copy propagation,
  slot-version propagation, constant folding, branch folding, dead temporaries,
  and safe dead stores in that order. `LocalEncoder` performs final fusion.
  The old emitter's branch folding and discarded-value elimination were removed;
  there is no second scalar optimizer layered under the new passes.
- Stored-value references preserve which definition a read observes. Copy
  propagation proposes aliases; the version pass rejects stale source reads
  and restores the saved slot. Computed stores propagate constants through
  dependency chains during folding, but nonconstant expressions are never
  reevaluated at a later read. Checked failures remain runtime operations.
  I64 arithmetic and comparisons fold; floating-point arithmetic does not.
- Slot facts are dense, generation-stamped, eight-byte records. Unknown reads
  do not allocate them, writes grow only through the highest verified slot,
  and the maximum logical table is 512 KiB. Effect/block/pass invalidation
  advances a checked version instead of clearing the whole table. The revised
  copy pass reuses stored-definition facts instead of rebuilding another map.
- Safe dead-store removal initially accepts adjacent non-failing literal
  writes to the same slot, with no intervening read, possible failure or effect.
  All other stores remain. Non-empty merges and pre-write snapshots retain
  canonical physical-stack behavior. Three collection idioms remain opaque
  effect barriers; precise source mappings are still an open requirement.
- Differential tests cover unchanged and stale multi-hop aliases, overwritten
  intermediate copies, constant dependency chains, dynamic stored results,
  arithmetic overflow/division/comparison behavior, literal truthiness with
  signed zero and NaN payloads, dead writes before errors, effect barriers,
  and deep constant and nonconstant trees. Unit checks cover last `u32` IDs,
  version wrap rejection, slot 65,535 and cross-block fact invalidation.
- Two old fixtures now folded away the operation they intended to exercise.
  The fixed-extent test now starts with an unknown local multiply; the profile
  fixture obtains its seed through a runtime I64 cast. Both still require their
  original quickening/profile behavior. A new test's mistaken expectation that
  `i64::MIN - (-1)` overflows was corrected using checked arithmetic; no runtime
  defect was hidden by that correction.
- The initial candidate passes workspace tests with hooks and all-target
  Clippy (`workspace-local-passes-1.log`, `clippy-local-passes-1.log`). All three
  ignored storage diagnostics were run separately (`storage-local-passes-1.log`).
  `Expression=16`, `Action=16`, optional `TempId=4`, builder=160 bytes; installed
  `JitOp=24`, `JitChunk=72`, `JitProgram=248` remain unchanged. The repeated-store
  fixture shrinks operation/counter storage from 26,728 to 104 bytes. Its final
  slot and output remain observable; the reduction removes overwritten writes.
- The 65,536-level nonconstant diagnostic uses 4 MiB of expression capacity,
  4 MiB of action capacity and 2 MiB of traversal capacity, plus small roots
  and effects buffers. This is 4 MiB more transient storage than the streaming
  foundation, before any slot-fact table. It is released at installation, not
  cached. This memory regression is explicit, not a claimed memory win overall.
- The first isolated nine-repeat/scale-64 codegen pair passed all variance
  gates. Best times regressed from 1.62/1.27/1.87/1.74 us to
  2.86/2.19/3.75/3.11 us (straight-line/function-main/heap/builtin). These are
  real costs of building and scanning the new local representation, not an
  accepted cold-start optimization. Both independently captured hot-loop runs
  failed their 5% gate (candidate 5.73%, reference 6.32%); neither contributes
  accepted timing rows. Files use `local-before/after-*-1`.
- The revised candidate adds computed-store propagation and removes the
  redundant copy-pass assignment-table traversal. Workspace tests with hooks
  and all-target Clippy pass (`workspace-local-passes-2.log`,
  `clippy-local-passes-2.log`). Storage diagnostics agree with the sizes above
  (`storage-local-passes-2.log`). The final differential suite has 13 passing
  tests, including 128 deterministic 48-step mutation sequences, each compared
  with the VM at thresholds 0/1/8 over repeated runs
  (`parity-local-passes-final.log`). Final all-target Clippy, nightly formatting
  of every changed/new Rust file, and `git diff --check` also pass.
  The subsequent complete workspace rerun also passes with that final test
  (`workspace-local-passes-final.log`: 150 core unit tests, 13 symbolic tests,
  56 runtime-parity tests, boundary tests and the native Ralloc suites; existing
  opt-in/ignored tests remain separate, with storage diagnostics run above).
- Capture 2's revised four-row codegen invocation passed, but its reference
  invocation failed variance gates (10.70% straight-line and 12.05% builtin).
  That pair is not accepted evidence; no reference row is salvaged from it.
  Independent filtered capture-3 pairs all passed nine-repeat/scale-64 gates:

  | Fixture | Before best us | After best us | Mean cycles | Before/after CV |
  | --- | ---: | ---: | ---: | ---: |
  | Straight-line runtime | 1.024 | 0.881 | -16.5% | 7.1% / 2.6% |
  | Function calls | 44.743 | 45.249 | -1.4% | 3.8% / 2.3% |
  | Straight-line cold lowering | 1.479 | 2.740 | +78.0% | 8.5% / 5.1% |

  A reverse-order runtime-only confirmation (capture 4) also passed: best
  1.041 to 0.817 us, mean cycles -23.5%, CV 3.5%/2.7%. Calls are effectively
  flat; their opposite best/mean directions do not support a speedup claim.
  Every timing invocation ran all 22 pre-timing correctness checks. Production
  candidate 2 is preserved as `local-passes-2.exe`; reference is the preceding
  `symbolic-blocks-retained.exe`. No native-code backend was introduced.
- Retention is justified by the required local architecture, two accepted
  runtime improvements and eliminated redundant stores, not full performance
  acceptance. The +78% tiny cold-lowering cost, extra transient action storage,
  and missing accepted hot-loop comparison remain open. The added IR building
  and pass scans are known extra work; their individual costs have not yet
  been isolated. No claim is made that cold-start or whole-system targets pass.

## Bounded vector storage and definition runs

- Rechecked the vector/box decision as the local optimizer grew. Fixed
  certificates, block tables, chunk slots and edge counters remain boxed
  slices. Runtime stacks and temporary builders retain vectors for changing
  initialized lengths and reuse. Installed `JitOps` still exposes only slice
  access and reclaims spare operation capacity at 4 KiB. The earlier paired
  all-boxed measurements justify avoiding unconditional small-buffer shrinking;
  they do not establish that vectors are universally faster than boxed slices.
- The local action vector formerly repeated a definition record for every
  expression. Consecutive definitions now share one `DefineThrough` record.
  A pending typed ID delays that record until the next action, preserving
  exact read/write/effect order without accessing the action vector on every
  definition. Clearing a block clears this pending state. Action storage is
  private; the encoder borrows a slice and cannot append or truncate it.
- Copy propagation reports whether it proposed any aliases. Only that case
  needs the slot-version pass: no subsequent pass introduces a copy, and no
  other expression consumes revision comparisons. Copy-bearing blocks still
  run version analysis before folding. This removes a provably unnecessary
  pass, not the checks protecting stale aliases.
- All three storage diagnostics pass (`storage-definition-runs-2.log`). In
  the 65,536-level nonconstant fixture, action capacity falls from 4 MiB to
  64 bytes; expression/traversal capacity remains 4 MiB/2 MiB. Combined builder
  buffers are now 6 MiB plus 176 bytes, excluding the separate output buffer,
  source bytecode and allocator overhead. This fixture needs no slot-fact
  allocation. The builder header grows from 160 to 168 bytes. Installed
  `JitOp=24`, `JitChunk=72`, `JitProgram=248`, and the repeated-store fixture's
  104-byte operation/counter storage are unchanged by this follow-up.
- The first definition-run candidate accessed the last action on each
  definition. Its accepted nine-repeat/scale-64 codegen pair showed small
  regressions: best 2.60/1.95/3.52/2.80 us became 2.75/2.04/3.56/2.94 us
  (straight-line/function-main/heap/builtin). Files are
  `runs-before/after-codegen-1`; the preserved candidate is
  `definition-runs-1.exe`. The retained candidate instead defers the marker
  and skips unneeded version analysis (`definition-runs-2.exe`).
- Candidate 2's broad reverse-order codegen comparison failed variance gates
  on both invocations (candidate function-main 28.93%, reference builtin
  15.06%). Neither invocation contributes accepted rows. Independently
  filtered nine-repeat/scale-128 capture-3 pairs produced:

  | Fixture | Before best us | After best us | Mean cycles | Before/after CV |
  | --- | ---: | ---: | ---: | ---: |
  | Heap fixture cold lowering | 3.800 | 3.563 | -6.0% | 4.5% / 3.8% |
  | Straight-line runtime | 0.871 | 0.925 | +6.4% | 7.4% / 9.1% |

  Both pairs passed their quality gates, but the runtime result is a
  regression, not a win. Its relatively high accepted CVs warrant caution,
  not exclusion. Straight-line cold lowering has no accepted pair: its
  reference failed at 13.61% CV while the candidate passed. Files are
  `runs-before/after-{cold-heap,runtime-straightline,cold-straightline}-3`;
  all invocations ran the 22 pre-timing correctness checks. The reference
  executable for this follow-up is `local-passes-2.exe`.
- Final workspace tests with `testing-hooks` pass: 154 core unit tests,
  13 symbolic differential tests, 56 runtime-parity tests and the native
  Ralloc suites (`workspace-definition-runs-final.log`). Existing ignored
  tests remain separate; the three storage diagnostics were run above.
  New regressions cover definition-run boundaries, pending-state clearing,
  version-pass selection and operation-buffer reclamation immediately around
  the threshold, including empty and already-full buffers. All-target Clippy
  with warnings denied also passes (`clippy-definition-runs-final.log`).
- Retention is for the substantial transient-memory reduction and the required
  local architecture. It is not full timing acceptance: tiny cold lowering
  remains more expensive than the earlier streaming implementation, and the
  new runtime comparison above needs follow-up. The per-chunk slack bound
  also does not substitute for the still-pending cache-wide retained-byte
  limit. No new unsafe code, dependency or native backend was introduced.

## Compact executable representation experiment (rejected)

The prototype below was built, tested and measured, then removed from the
active code after the accepted runtime comparisons regressed. Its source and
release binaries are preserved with the experiment artifacts.

- Re-read the full plan before this phase. The previous goal turn made
  progress by adding storage-boundary regressions and recording completed
  definition-run measurements; the full 40-section goal remains active.
- Audited every semantic `JitOp` variant against verifier operand limits.
  Slot, global, string and field indexes fit 16 bits; function, struct and
  enum indexes fit 12 bits. Slot counts and function arities can reach 65,536
  and must not be encoded as `u16`. Branch offsets need more than 16 bits:
  the total instruction budget is 262,144, including valid end offsets.
  Crucially, unreachable `MakeArray` and `CallValue` arities can exceed `u32`
  on 64-bit hosts while remaining verifier-valid. Reachable stack limits do
  not justify narrowing those operands during static installation.
- Consulted the Rust Reference's
  [layout rules](https://doc.rust-lang.org/reference/type-layout.html) and
  [numeric-cast rules](https://doc.rust-lang.org/reference/expressions/operator-expr.html#numeric-cast).
  The prototype uses a private `repr(C)` 16-byte record with typed kind/mode
  fields and three `u32` operand words. A compile-time size assertion guards
  the layout. No transmute, raw-byte parsing, unsafe code or dependency was
  added. Integer/float literals preserve all 64 payload bits.
- The temporary `compact-jit-experiment` build feature selects compact
  storage for the entire interpreted JIT, including lazy chunks, listings,
  clones, profiling and quickening. There is no runtime representation switch
  or duplicate installed op buffer. Construction/dispatch still pass through
  semantic `JitOp` during this measurement: the extra encoding pass, transient
  old/new buffer overlap and decode bridge are real prototype costs.
  This is an experiment, not an accepted/default new tier.
- Arithmetic/comparisons share mode-driven compact kinds. Oversized operands
  and the four-slot/i64 map descriptor use a checked `ExtendedId` into an
  owned boxed side table. Inline narrowing is checked before encoding; a
  failure preserves the complete semantic operation in the side table.
  That worst-case record costs 16 + 24 = 40 bytes, not 16. The existing
  packed comparison's 16-bit target limit is unchanged; wider valid branches
  retain their unfused encoding and full target.
- Seven codec tests cover every current instruction variant, all direct
  builtins/scalar modes, operand positions, last slots, branch offsets up to
  and beyond the verifier budget, `u32`/`usize` extremes, I64 limits, signed
  zero/NaN/infinity bits, exact side-table accounting, independent clones,
  partial/idempotent promotion, and execution past unreachable wide arities.
  An initial visibility error for the existing internal-testing op counter
  was fixed by matching the existing crate-visible storage boundary.
- Both complete workspace builds pass with `testing-hooks`
  (`workspace-compact-control-1.log`, `workspace-compact-1.log`). The control
  has 154 passing core tests; compact has 159 (two vector-only capacity tests
  are not applicable, seven codec tests are added). Both run 13 symbolic
  differential tests, 56 runtime-parity tests, maximum-slot/wide-branch tests
  and native Ralloc suites. All-target Clippy with warnings denied passes in
  both configurations (`clippy-compact-control-1.log`, `clippy-compact-1.log`).
- All three ignored storage diagnostics were run for each build. The map
  fixture was added to expose side-table cost in the installed totals:

  | Fixture | Control live/reserved bytes | Compact live/reserved bytes |
  | --- | ---: | ---: |
  | Small | 104 / 128 | 72 / 72 |
  | Loop | 234 / 498 | 162 / 162 |
  | Function | 286 / 286 | 198 / 198 |
  | Map | 312 / 624 | 240 / 240 |
  | Repeated stores | 104 / 104 | 72 / 72 |

  These are operation/counter allocations, including compact side data, not
  RSS. Compact chunk headers grow from 72 to 80 bytes; `JitProgram` remains
  248 bytes and builder buffers are unchanged. Sources are
  `storage-compact-control-1.log` and `storage-compact-1.log`.
- Release builds are preserved as `compact-control-1.exe` (SHA-256
  `90cd998b54251fe181316fbb1b43ca8351899f22650c02d3a0bf4443a507331e`)
  and `compact-1.exe` (`ecb45316258da71d055bf6ae98dac600c5d4f294d57ac7ee0347e9227e8e06d7`).
  These compare the same semantic fetch interface with the feature off/on,
  using the same release profile. Neither binary is the final retained build.
- Independent nine-repeat capture-1 pairs passed for cold lowering at sample
  scale 64. The runtime pairs did not: both hot-loop invocations failed their
  5% CV gate (control 6.61%, compact 9.69%); calls failed on the control side
  (14.77%), map on the compact side (12.88%) and straight-line execution on
  the compact side (12.37%). No row from those failed invocations is accepted.
- Reverse-order capture 2 used sample scale 64 for hot loops, calls and maps,
  256 for straight-line execution and 128 for heap lowering. Hot-loop, call
  and map pairs passed. Straight-line control failed at 10.11% CV; both heap
  invocations failed (compact 17.35%, control 34.64%). There is no accepted
  straight-line-runtime pair or capture-2 heap pair. The accepted comparisons
  are below; times are best microseconds, cycle deltas use paired means:

  | Fixture / capture | Control us | Compact us | Mean cycles | Control/compact CV |
  | --- | ---: | ---: | ---: | ---: |
  | Cold straight-line lowering / 1 | 2.914 | 2.928 | +2.7% | 5.0% / 5.5% |
  | Cold heap lowering / 1 | 3.648 | 4.049 | +10.8% | 7.0% / 9.5% |
  | Quickened hot loop / 2 | 498.542 | 647.602 | +26.2% | 4.9% / 3.6% |
  | Function-heavy execution / 2 | 45.947 | 62.993 | +32.9% | 4.8% / 6.7% |
  | Map set/get / 2 | 120.959 | 132.182 | +11.5% | 2.9% / 3.7% |

  Files use `compact-{control,compact}-{fixture}-{1,2}.json/log` under
  `target/perf/full-system`. Every invocation ran all 22 correctness checks;
  timing was isolated from compilation and profiling. Different capture
  scales are never combined into a pair.
- Decision: reject this compact-to-semantic decode bridge. The measured
  memory reductions do not justify double-digit runtime regressions across
  three workload classes. Per-dispatch expansion is known additional work,
  but its individual cost was not isolated from other layout/codegen effects.
  These measurements do not prove every possible 16-byte representation
  slower. Any later compact attempt must address direct execution and the
  temporary construction overlap, not repeat this storage wrapper unchanged.
- Removed the feature, compact module, storage-selection/fetch adapters and
  prototype-only slot conversion. The original direct 24-byte semantic
  execution path is restored; no alternate representation remains in Cargo.
  The source is recoverable from `target/perf/full-system/compact-prototype-1.zip`
  (SHA-256 `1c63fb71353a2cc71dc9b1af3179e1f9965dc5af9bd8ece1848fee2f0c0d76d5`),
  containing the experiment's JIT directory and core manifest. Its codec
  tests are preserved there, not left as dead active code.
- Retained the map storage diagnostic and a public-boundary regression for
  `u32::MAX + 1` and `i64::MAX` unreachable arities on 64-bit hosts. It compares
  VM/JIT execution at thresholds 0/1/8 over repeated runs and verifies exact
  operands in the JIT listing. The bytecode documentation now records this
  static-versus-reachable distinction.
- Final retained-source checks pass: full workspace with `testing-hooks`
  (`workspace-compact-rejected-final.log`: 154 core tests, five optimization
  boundary tests, 13 symbolic tests, 56 runtime-parity tests and native Ralloc
  suites), all-target Clippy with warnings denied
  (`clippy-compact-rejected-final.log`), nightly formatting of changed/new Rust
  files, manifest formatting and `git diff --check`. The three ignored storage
  diagnostics pass separately (`storage-compact-rejected-final.log`) and
  confirm the original 72-byte chunk headers and retained code extents.
  The normal release benchmark has been rebuilt without the experimental
  feature (`build-compact-rejected-final.log`); no final-source timing win is
  claimed. A source/manifest search confirms no compact feature, module or
  storage-selection references remain active.

## Feedback-driven scalar specialization (acceptance incomplete)

- Re-read the full 40-section objective after the container-only check. That
  check confirmed an existing decision; it did not advance the remaining
  implementation. This continuation replaces blind scalar promotion with
  executable per-site feedback rather than another container comparison.
- `jit/feedback.rs` owns payload-free representation observations and an
  exhaustive state machine. Arity, signed/unsigned widths, float kind, Bool,
  Null and the other runtime storage tags remain distinct. Metadata cannot
  retain a value, heap identity, pointer field, or closure capture.
- A hot back edge arms sampling, not I64 specialization. Eight consecutive
  actual-I64 observations install the existing checked fast handler. The
  sampler retains at most two shapes and 32 observations. Unsupported stable
  shapes, a third shape, or an exhausted window select generic execution.
  Four misses without 32 consecutive hits trigger resampling; each site gets
  at most two promotions before further instability makes it terminal generic.
  These are conservative initial thresholds, not measured universal optima.
- All formerly blindly quickened stack arithmetic/comparisons, slot/immediate
  multiply/divide, compare/branch and slot truthiness paths participate.
  Pure branch-counter elision requires no type assumption. Permanent scalar
  promotion and `promote_range` have been removed. A saturated maximum-threshold
  edge cannot repeatedly activate its range.
- Every type miss records its actual tags and uses shared generic semantics.
  TinyMemory's private in-place multiply/divide methods now return whether
  their encoded I64 path applied, leaving non-I64 slots untouched for the
  caller. Misses decode once; successful in-place paths still operate on the
  encoded slot. No unsafe code, external dependencies, Ralloc layout changes,
  or native-code backend were added. Existing allocator/representation
  documentation gates apply; no new external-crate API was introduced.
- Feedback tables are lazy boxed slices, fixed after allocation, with one
  64-KiB backing-byte budget shared by all chunks in a program. A whole-chunk
  dense table that cannot fit is skipped; the program still executes generically.
  Clone copies independent feedback and its accounting, never runtime scratch
  values. `feedback_bytes()` exposes table backing bytes. Detailed sample,
  hit/miss and transition counts remain opt-in in `JitExecutionProfile`.
  Encoded arithmetic errors before a returned guard result are not counted as
  hits; sampling attempts and actual type misses are recorded before execution.
- Current x86_64 layout (`storage-type-feedback-1.log`): feedback site 11 bytes,
  `JitOp` unchanged at 24 bytes, `JitChunk` 72 -> 88, `JitProgram` 248 -> 296.
  Cold fixtures allocate no tables. The loop/map fixtures retain 99/132 bytes
  after warming; small/function/repeated-store fixtures retain zero. Existing
  installed code extents and symbolic-builder storage remain unchanged. These
  are Rust allocation/header extents, not RSS or allocator fragmentation.
- Initial parity failures were old assertions demanding unsupported or unseen
  sites become I64-hot. Stable unsigned/float tests now require generic forms.
  Always-failing sites collect observations across repeated failing runs before
  testing their hot error paths. New fixtures were corrected to use verifier-
  valid terminal RETURN padding and numeric `read_int()` selectors; successful
  differential cases explicitly require success, not merely matching failures.
- New differential coverage exercises all ten stack scalar modes across
  thresholds 0/1/8; I64-to-float/unsigned changes; Bool miss/error cleanup;
  compare/truthiness and multiply/divide push fallbacks; independent cloned
  adaptive state; repeated fresh executions; and a shared lazy-function budget.
  State tests pin sample limits, third-shape rejection, sparse-miss tolerance,
  bounded promotions, tag distinctions and saturating hit history.
- Full workspace tests with `testing-hooks` pass (`workspace-type-feedback-1.log`):
  core 160 plus three ignored diagnostics, runtime parity 56, symbolic 13,
  optimization boundaries 5, new feedback integration 3, Ralloc core 42, and
  the other suites. All three diagnostics were separately run. All-target
  Clippy with warnings denied and changed/new Rust formatting pass. A final
  saturation regression and final verification are recorded below. Earlier
  fixture/assertion and Clippy failures were corrected, not ignored.
- Normal release control is `type-feedback-control-1.exe`, SHA-256
  `4bc6c7e1c4bd51671d8469c33f225e4e249821365f13fd52a7a4bd3646774e03`;
  candidate is `type-feedback-1.exe`, SHA-256
  `4dec67b57a0249d6cbc1dd807998cc807bf2a0c93c28b3db9756872b8b82a78e`.
  Every timing invocation ran all 22 correctness checks, nine repeats and
  sample scale 64, isolated from Cargo, tests, profiling and compression.
  Captures are `type-feedback-<control|candidate>-<filter>-<capture>.json/.log`.
- Accepted capture-1 pairs: tiny cold lowering best 2.302 -> 2.254 us,
  mean cycles -6.6%, CV 8.6/4.8%; calls best 41.701 -> 43.200 us,
  mean cycles +2.8%, CV 5.7/2.8%. These are one pair each, not independently
  confirmed broad performance wins. The call regression remains tracked.
- No accepted hot-loop pair: capture-1 control CV 10.16% failed its 5% gate
  (candidate 2.2% passed); reversed-order capture-2 candidate/control CV
  34.77/44.99% both failed. Slot-multiply capture-1 candidate/control CV
  16.26/27.34% both failed its 10% gate. No rows from rejected invocations are
  accepted and no hot-loop or in-place arithmetic speedup is claimed.
- This is the requested feedback architecture, retained provisionally while
  acceptance is incomplete. I64 is the only specialized representation;
  no dedicated F64/mixed handler is claimed. Stable-float/mixed workload timing,
  sparse large-chunk behavior, broader memory evidence, and quiet accepted hot
  pairs remain necessary. No full Miri or whole-system acceptance is claimed.
- Final verification after the saturation regression passes:
  `workspace-type-feedback-final.log` (core 161 plus three explicitly handled
  ignored diagnostics; all workspace suites pass) and
  `clippy-type-feedback-final.log` (all targets, `testing-hooks`, warnings
  denied). Nightly rustfmt checks all changed/new Rust files, and
  `git diff --check` passes. Architecture, bytecode, VM and Rust API docs now
  describe observed, reversible specialization and bounded feedback storage.

## Bounded compiled retention and capacity accounting

- Re-read the complete 40-section objective. The preceding container review
  revalidated existing choices but did not advance the remaining implementation.
  The preserved failing `cache_retention` regression demonstrated 257 compiled
  entries retained after 257 distinct programs, despite the source cache limits
  (`cache-retention-before.log`).
- `jit/compiled_cache.rs` now owns an independent deterministic LRU with default
  limits of 128 compiled entries / 16 MiB of charged entry storage. A typed
  disabled/bounded policy interprets either zero limit as disabled. Public
  defaults are initial retention-policy choices, not a measured optimum.
  The public
  configuration, effective-limit getters, clear and lifetime cache statistics
  are additive; source-cache limits and exact-source collision checks remain
  independent. Limits are enforced after successful and failing runs, including
  lazy code, feedback and operand-pool growth. A single oversized entry bypasses
  retention without flushing unrelated hot entries. Programs still execute with
  either or both caches disabled.
- Preserve `compile`/`compile_verified -> &JitProgram`: a non-cacheable
  compile-only result occupies one separately reported, non-reusable temporary
  slot until the next mutable operation. That slot is explicitly outside the
  reusable-entry byte limit and omitted by cloning. It is boxed so the uncommon
  result's large header does not inflate every cache instance. Run APIs bypass
  directly and do not keep this slot. This is a compatibility accommodation,
  not a claim of a hard peak-process-memory limit.
- Immutable bytecode retention in `bytecode/retention.rs` counts vector and
  string **capacity**, nested function/struct/enum/module/import/export metadata,
  detailed permission allowlists and boxed verifier/block tables. The deep scan
  is memoized with the shared verified owner. Installed chunk storage is tracked
  incrementally; default post-run accounting does not scan all chunks/bytecode.
  Optional opcode profiling charges its string capacities and entry payloads.
  `JitProgram::retained_bytes()` includes owner headers, reserved lazy slots,
  installed code/counters, feedback and idle operand capacity. Cloning recounts
  actual copied extents and starts with no scratch storage.
- Byte charges conservatively include shared verified data in full per entry;
  they are not exclusive freed bytes or RSS. Allocator/Arc overhead, unused
  hash-table buckets and opaque profiling-tree node slack are excluded. Table
  capacity is separately visible and released on disabling/clearing; reducing
  limits also requests shrinking. Source-cache bytes still mean source text.
  No Ralloc heap/frame payload survives in a compiled program, so none is
  charged to this owner. Execution temporaries and returned memory are outside
  the cache retention budget; no optimized-region storage currently exists.
- A new host-output-panic regression failed after lazy growth because unwinding
  skipped normal accounting (`cache-retention-unwind-before.log`). Cached runs
  now discard the affected compiled entry before resuming the original panic;
  there is no panic-to-language-error conversion or reuse of partially mutated
  state. Normal runtime errors use `Result` and still update retention.
- The first bounded implementation retained allocating string keys. Its clean
  nine-repeat/scale-64 pair increased compile-only verified lookup mean time
  from 99.45 to 137.20 ns (mean cycles 257.27 to 355.17). The borrowed API needs
  separate membership/return lookup around possible eviction. Candidate 2 keeps
  that safe ownership boundary but replaces the key allocation with a copied
  fixed 32-byte hexadecimal fingerprint plus typed authority state. Every
  fingerprint byte is preserved; neither digest computation nor artifact
  compatibility changes. Runtime hits still perform one table lookup.
- Preserved baseline binary `bounded-cache-control-1.exe` has SHA256
  `4dec67b57a0249d6cbc1dd807998cc807bf2a0c93c28b3db9756872b8b82a78e`.
  Pre-change core/Ralloc sources are `bounded-cache-control-source-1.zip`, SHA256
  `2e964f1733550d5da3cc09688aa7a137f46dd7708fb26f8b043c3e19ff1929c2`.
  Candidate 1 is `bounded-cache-1.exe`, SHA256
  `86c882178f784727ece8db84c741c4465357012a1ec98b92d1523ae3d2e2d3b0`.
  Candidate 2 is `bounded-cache-2.exe`, SHA256
  `1e3b97a7c696ca38af48d223fdfd32c816c05375fde4e3817ce209af2a61af06`.
  Its core/Ralloc source snapshot is `bounded-cache-source-2.zip`, SHA256
  `3d357e0a90bc92ec67040146f1cf1111c2548a1410f255c0f71d459e6bab28aa`.
- Candidate 2 was compared with the preserved original in two nine-repeat /
  scale-64 passes, first candidate/control (suffix `-2`), then control/candidate
  (`-3`). All 18 invocations across the three comparison passes ran the 22
  pre-timing correctness checks; timing was isolated from Cargo, tests and
  profiling. Files are `bounded-cache-{control,candidate}-<row>-<pass>`.
  Accepted candidate-2 pairs are:

  | Row / pass | Before mean | After mean | Mean cycles change | Before / after CV |
  | --- | ---: | ---: | ---: | ---: |
  | Verified compile-only lookup / 2 | 107.15 ns | 72.29 ns | -32.75% | 5.99% / 4.47% |
  | Verified compile-only lookup / 3 | 105.36 ns | 69.35 ns | -34.20% | 3.01% / 2.48% |
  | Warm source API / 2 | 16.67 us | 15.64 us | -6.07% | 2.14% / 3.58% |
  | Warm source API / 3 | 16.16 us | 17.30 us | +6.59% | 6.06% / 7.16% |
  | Straight-line cold lowering / 3 | 2.729 us | 2.436 us | -10.45% | 8.75% / 6.27% |

  Lookup best times also improve in both pairs (99.23/99.02 ns to 68.84/67.98
  ns). This supports the allocation-free key change, not a broad execution
  speedup. Warm results conflict and include an accepted regression; do not
  discard it. Cold lowering has only one accepted pair. Its candidate captures
  at passes 1 and 2 failed at 10.52% and 16.93% CV, respectively; neither
  rejected invocation contributes accepted timing evidence. The candidate-1
  warm-source pair was +1.41% mean cycles with 0.61%/1.14% CV. Bounds and the
  ownership/accounting correction are retained, with cold/warm acceptance open.
- Focused coverage now includes entry/byte LRU, aggregate versus individual
  growth, capacity-heavy identical-content programs, source/compiled disable,
  borrowed bypass lifetime, limit reduction/clear, all runner families,
  successful/failing lazy growth, feedback-only growth, panic propagation,
  exact clone accounting, ownership release and access-clock rollover.
  Full workspace tests with `testing-hooks` pass: 168 core unit tests,
  11 cache-retention integrations, 56 runtime-parity tests and the native
  Ralloc suites (`workspace-bounded-cache-final.log`). All-target Clippy with
  warnings denied passes (`clippy-bounded-cache-final.log`). Nightly formatting
  checks for all changed/new Rust files and `git diff --check` also pass.
- All four ignored storage diagnostics were separately run and passed
  (`storage-bounded-cache-2.log`). JitOp/JitChunk remain 24/88 bytes;
  JitProgram grows by 8 bytes to 304 for incremental installed-storage tracking.
  The churn fixture retains 128 entries and 159,488 charged bytes at both
  128 and 257 distinct programs, with 129 evictions by the end. Table capacity
  and excluded overhead are not interpreted as RSS or allocator-free counts.
  Weak-owner tests prove eviction and temporary-result cleanup release their
  verified owners when no external owner remains. No new unsafe code,
  dependency, allocator implementation or native backend was introduced.
- Consulted the current official Rust docs for HashMap entry/capacity/shrinking,
  Vec capacities, OnceLock memoization, fixed-array slice conversion and
  catch/resume-unwind behavior. Existing dependency versions are unchanged.
  The full batch's broader profiling, timing and safety acceptance remains open;
  this phase does not claim another Miri run or completion of the 40 sections.

## Parser token ownership

This compiler workstream applies the same ownership-based container rule:
build with a vector, then expose only the operations the consuming stage needs.
This addresses section 6's repeated token allocation without changing bytecode,
name resolution, module policy or verification.

- `Compiler::eat` previously cloned every accepted token's `String` and kept
  the original token behind a manually incremented vector index. `primary`
  additionally cloned the current token even for branches that discarded it.
- The compiler now owns `std::vec::IntoIter<Token>`. `as_slice()` supplies
  borrowed current/lookahead tokens, and `next()` transfers the accepted token.
  The redundant cursor is gone; the parser cannot append or resize the stream.
  Literal branches use the consumed token, and other primary forms inspect only
  its kind. Wrong-kind errors do not advance the stream; the EOF sentinel and
  original token spans retain their existing behavior.
- This is not a blanket conversion from Vec to Box. The lexer still needs a
  growable builder, and the owning iterator takes that allocation without
  shrinking or boxing each token. The backing allocation is retained until
  iterator drop. Consumed strings no longer also remain in the token stream,
  although names/literals may be retained by their subsequent compiler owner.
- Re-read the repository documentation router, Rust rules and all four required
  Rust examples; compiler/architecture, performance, contribution/CI and
  expression, statement, module, type and v2 documentation apply. Consulted
  the official [`IntoIter` documentation](https://doc.rust-lang.org/std/vec/struct.IntoIter.html)
  and installed Rust 1.98.0 sources for `Vec::into_iter`, `IntoIter::as_slice`,
  `next` and `Drop`: ownership of the original allocation transfers, remaining
  items drop normally, and no shrink is introduced. No dependency or unsafe
  implementation was added; public APIs and runtime allocation paths are unchanged.
- The new ownership regression failed on the old parser because the returned
  text had a different allocation; the three diagnostic/lookahead tests passed
  (`token-stream-before.log`). All four now pass, together with the explicitly
  run storage diagnostic (`token-stream-after.log`). Coverage includes retry
  after a kind mismatch, qualified-call versus field lookahead after advancing,
  exhausted lookahead, repeated EOF, and exact oversized-literal diagnostics.
- On this x86_64 machine, the iterator header is 32 bytes, equal to the old
  24-byte vector plus 8-byte cursor. A 1,537-token diagnostic retains the same
  98,304-byte token allocation. All 1,536 non-EOF token strings transfer with
  identical allocation addresses; the unconsumed suffix's text capacity falls
  from 5,376 bytes to zero. This is an ownership/capacity diagnostic, not an RSS,
  allocator-fragmentation or whole-compilation peak-memory measurement.
- `cargo test --workspace --features testing-hooks` passes
  (`workspace-token-stream-1.log`): 172 core tests plus five ignored diagnostics,
  49/49 language fixtures, all 56 runtime-parity tests, all cache/lowering suites
  and the Ralloc suites. All five core diagnostics were separately run and pass
  (`storage-token-stream-all-1.log`). All-target workspace Clippy with hooks and
  `-D warnings`, nightly formatting for changed/new Rust, and `git diff --check`
  pass. This safe compiler-only change does not claim a new Miri run.
- Release control `token-stream-control-1.exe` has SHA-256
  `1e3b97a7c696ca38af48d223fdfd32c816c05375fde4e3817ce209af2a61af06`;
  its production sources are the preceding `bounded-cache-source-2.zip`.
  Candidate `token-stream-1.exe` has SHA-256
  `3251751a57c69dec428292bf1d1377d159c394089c469207b4d10f2ba0f8943d`;
  `token-stream-source-1.zip` has SHA-256
  `ed7cae8766261e96e641305a1feca001e59400919e249e969a1d6156ca434251`.
  These machine-local artifacts are under `target/perf/full-system`.
- Final verification after removing an unused clone in the diagnostic-only
  test passes again (`workspace-token-stream-final.log`,
  `storage-token-stream-final.log`, `clippy-token-stream-final.log`, and
  `build-token-stream-final.log`). All changed/new Rust formatting and diff
  checks pass. Final source snapshot `token-stream-source-2.zip` has SHA-256
  `9324c79157b0a667beea9b904a582a9f6ec44ab8e553b3c74628650b234e752e`;
  final release `token-stream-2.exe` and `target/release/tinylang_bench.exe`
  have SHA-256
  `0dd1c43189f007ac8a8b642b3737d820cb5d67e1e34bb9f9a6bc1a360d5d89d0`.
  Comparing the saved parser source confirms only that `cfg(test)` cleanup
  differs; production source is unchanged. The rebuilt binary nevertheless has
  a different hash. All timings below refer specifically to candidate 1, not
  to an unperformed timing run of the final binary; confirmation must use the
  final recorded build.
- All ten timing invocations used the unchanged harness, 22 pre-timing
  correctness checks, nine repeats and sample scale 64, isolated from builds,
  tests, diagnostics and compression. Capture 1 ran controls before candidates;
  capture 2 ran candidate then control for each compilation row. Files are
  `token-stream-<control|candidate>-<exact-row>-<capture>.json/.log`.
- First-pair `compiler.emit_bytecode` is decision-grade: best 41.872 ->
  34.030 us, mean 45.446 -> 35.616 us, mean scheduled cycles -21.64%,
  CV 6.96/3.85%. This row includes lexing and authoritative verification;
  it is not an isolated parser-only timer.
- The reversed-order `compile.full_pipeline` capture-2 pair is decision-grade:
  best 44.924 -> 36.633 us, mean 47.308 -> 37.566 us, mean scheduled cycles
  -20.57%, CV 3.95/2.64%. The optimizer and verifier are unchanged.
- Candidate full-pipeline and unchanged-lexer capture 1 failed their quality
  gates at 19.27% and 54.34% CV; candidate emission capture 2 also failed at
  12.42%. Those invocations are excluded entirely, with no timing claim taken
  from them. There is one independent accepted pair per compilation row, not
  two confirmations of either row. Retain the simpler ownership path
  provisionally; full timing acceptance remains open. Do not repeatedly run
  the same noisy captures without a useful change in measurement conditions.
- At this checkpoint, other frontend work remained open,
  including duplicate source/source-map construction, shared-table cloning,
  symbol handling and larger/isolated compiler workloads. No broad frontend or
  whole-system performance completion is claimed.

## Shared source ownership and on-demand diagnostics

The next frontend workstream addresses sections 5–6 and 26/28–29 together:
the compiler constructed one source map, then the lexer independently owned
another map and another source string. That meant three owned source copies
inside compiler construction and two eagerly built newline indexes. Standalone
lexing owned two source copies and one index. Neither stage needs a newline
index unless it reports an error.

- `SourceMap` is now the non-cloneable source/filename owner. `Lexer` borrows
  that owner, and the compiler retains it after lexing instead of recreating
  source data. A regression proves an incoming owned source allocation survives
  construction and compilation at the same address. Public source APIs still
  accept `&str`; their one owned source copy is not misrepresented as zero-copy.
  Internal incoming String capacity is preserved rather than forcibly shrunk.
- Removed the line-index vector and its construction pass. Diagnostic lookup
  counts preceding newline bytes and locates the current line on demand.
  Compilation stops at its first error, so there is no diagnostic index or
  memoization on the successful path. A repeated private line lookup is now
  linear in preceding source bytes instead of logarithmic in indexed lines;
  this tradeoff does not change public diagnostic text.
- Span calculations use byte slices until actual line boundaries are known.
  The reference test compares the old eager-index result at every byte offset
  of empty, ASCII, LF, CRLF, lone-CR, tabbed and multi-byte UTF-8 sources,
  including offsets inside scalars, beyond EOF and `usize::MAX`, with empty,
  backward, extended and maximal span ends. Exact lexer errors also cover a
  multi-byte unknown escape and CRLF. Token grammar, bytecode, verification,
  module authority and runtime error/source mappings are unchanged.
- The pre-change storage regression fails at 72 versus 48 expected header
  bytes; the diagnostic reference passes (`source-owner-before.log`). The
  4,096-line source-map fixture reports 106,496 text-capacity bytes, nine
  filename-capacity bytes and 65,536 line-index bytes. Afterward the text and
  filename capacities are unchanged, the header is 48 bytes and the line index
  is absent (`source-owner-after.log`). These are Rust-owned extents, not RSS
  or a measured whole-compilation peak. Source/token storage uses the host
  allocator; no Ralloc allocation path changed.
- Added `frontend.lex_line_heavy_4096` and `compile.line_heavy_4096` attribution
  rows. Their fixture is 4,096 comment lines plus `print 42`, generated outside
  timing and checked through both VM and JIT. The pre-timing suite now has 24
  checks; its original 22 cases remain. This is intentionally low-token source
  attribution, not a general large-program throughput claim. Ordinary
  `compile.full_pipeline` remains the function-heavy comparison.
- The repository router and Rust rules/examples were rechecked, with existing
  compiler, syntax, architecture, performance and public API documentation
  applying. Consulted current official Rust byte-slice/UTF-8 and OnceLock
  documentation for the borrowed scanner and once-built benchmark fixture.
  All used APIs are available on the installed Rust 1.98.0 toolchain. No crate,
  feature, unsafe implementation or native backend was introduced.
- Workspace tests with `testing-hooks` pass (`workspace-source-owner-1.log`):
  176 core tests plus six ignored diagnostics, all 49 language fixtures, all
  56 runtime-parity cases, cache/lowering suites and Ralloc suites. All six
  diagnostics pass when explicitly run (`storage-source-owner-1.log`). Focused
  source/parser tests, all-target workspace Clippy with warnings denied,
  changed/new Rust formatting and diff checks pass. No new Miri run is claimed.
- Control `source-owner-control-1.exe` has SHA-256
  `9503b11da24701ec2987e07081c516913059649347ac5064ce9d3c6a723c6fc1`;
  `source-owner-control-source-1.zip` has SHA-256
  `e84b61232eb9b73dfc3cac394945604fdf023db884d962fb2bc01b87f3d765bf`.
  Candidate `source-owner-1.exe` has SHA-256
  `ca4a2bfa9d98f772571de49ba659235395872200533092d3caf9139d7ca05c67`;
  `source-owner-source-1.zip` has SHA-256
  `56000993d8ad3e782da79733eee78f927664e528a3ae994083e1e0d22af98c40`.
  Both snapshots include the same expanded benchmark harness. Artifacts remain
  machine-local under `target/perf/full-system`.
- Completed twelve isolated invocations at nine repeats / sample scale 64;
  every invocation passed all 24 correctness prechecks. Capture 1 ran the
  control rows before the candidate rows; capture 2 reversed executable order
  within each row. Candidate `compile.line_heavy_4096` capture 1 failed the
  10% quality gate at 11.46% CV and is excluded entirely. The other eleven
  invocations passed their quality gates. Files are named
  `source-owner-{control,candidate}-<row>-{1,2}.json`/`.log`; the rejected
  invocation has a log only.

| Capture | Workload | Best us, before → after | Mean us, before → after | Mean-cycle change | CV %, before / after |
| --- | --- | --- | --- | --- | --- |
| 1 | `frontend.lex_line_heavy_4096` | 121.90 → 66.00 | 125.68 → 69.63 | -44.68% | 3.71 / 4.93 |
| 2 | `frontend.lex_line_heavy_4096` | 131.93 → 58.77 | 134.08 → 59.38 | -55.75% | 2.24 / 1.43 |
| 2 | `compile.line_heavy_4096` | 192.12 → 60.75 | 197.59 → 61.82 | -68.71% | 4.91 / 1.47 |
| 1 | `compile.full_pipeline` | 35.73 → 33.93 | 35.94 → 35.66 | -0.90% | 0.57 / 5.84 |
| 2 | `compile.full_pipeline` | 36.36 → 33.68 | 38.19 → 33.88 | -11.24% | 8.11 / 0.38 |

- Two independent clean pairs confirm the line-heavy lexer reduction. The
  ordinary function-heavy compilation guardrail has two clean pairs without
  an observed regression, but its varying gain does not establish a general
  compilation speedup. Line-heavy compilation has only one clean pair; its
  timing confirmation remains open. Retain the shared source owner and
  error-only diagnostic scan for their measured attribution benefit, removed
  allocations and simpler ownership. This does not accept the whole frontend.
- The final release binary matches the timed candidate SHA-256 above. No Rust
  source changed after that build. Wider frontend work, independent parser
  token-ownership confirmation, runtime diagnostics after fused operations and
  the full optimization acceptance ledger remain open.

## Compiler unit contribution and owned program assembly

The next frontend workstream addresses sections 5–6 and 28–29. Every imported
unit called `Compiler::compile`, deep-cloned the entire accumulated program,
and discarded that result. The independent 64-import benchmark therefore made
64 throwaway snapshots containing 1 + 2 + ... + 64 = 2,080 function entries,
in addition to final root assembly. This count follows the inspected fixture
and call path; it is not an allocator counter. Function completion also cloned
its finished code and local-name buffers before restoring the outer compiler.

- Split unit contribution from final assembly. Imported units compile and
  finalize their declarations/module metadata without creating a `Program` or
  allocating a discarded `HALT` chunk. The final root or incremental-fragment
  compiler is consumed; `Rc::try_unwrap` requires sole ownership before moving
  the shared tables into its result. There is no clone fallback or draining of
  another live compiler. Completed function buffers move out before the outer
  symbols/code/globals are restored.
- The first pointer-transfer regression failed on the old assembly path
  (`compiler-finish-before.log`). The refined regression checks retained
  string-payload and already-finished function-buffer addresses across final
  assembly, exact final buffer capacities, generic/module metadata and
  authoritative verification. Other tests cover refusing shared-owner
  finalization without draining it, allocation-free imported main code and
  restoration of enclosing buffers after a function-body error.
- A move-only prototype exposed a retained-capacity regression. With 1/32/256
  two-local function declarations, inclusive verified storage increased from
  949/14,394/111,766 to 1,333/15,930/124,054 bytes. In the 256-function case the
  increase was exactly 12,288 bytes of spare local-name headers
  (`compiler-finish-after.log`). That prototype was refined before timing.
- Completed code, local-name and declaration-field buffers now compact through
  boxed slices, then retain the existing `Program` representation. Final shared
  tables likewise discard builder slack without cloning their elements. This
  restores 949/14,394/111,766 retained bytes in both optimized and unoptimized
  diagnostic variants (`compiler-finish-compacted-after.log`). Reallocation
  during shrinking remains possible and is included in the timing comparison;
  this is not a universal no-reallocation claim. These figures include Rust
  capacities and verification metadata, not compiler peak memory, RSS or
  allocator-internal overhead. Host compiler storage never reaches Ralloc.
- Added `compile.declarations_256`: a once-built source with 256 function bodies,
  local-name tables and two direct calls. Both VM/JIT correctness cases execute
  it, bringing the pre-timing suite from 24 to 26 checks. Ordinary
  `compile.full_pipeline` and `compiler.module_graph_large_uncached` are the
  companion rows; the latter includes native filesystem/resolver cost.
- A nested/diamond/repeated-alias regression verifies three modules, five
  functions, one struct and erased generic metadata are assembled only once.
  A one-module incremental rebuild retains its imported dependencies and
  produces a reusable cache hit. Old and new verified programs remain
  independent and execute the expected results in both VM and JIT
  (`compiler-finish-nested-modules.log`). Existing authority, export, artifact,
  source diagnostic and cache-invalidity tests remain in the workspace suite.
- Rechecked the repository router, Rust rules/examples and compiler-related
  architecture, bytecode, syntax, module, API, performance, contribution, CI and
  v2 documentation. Consulted official Rust
  [Rc ownership extraction](https://doc.rust-lang.org/std/rc/struct.Rc.html#method.try_unwrap),
  [RefCell extraction](https://doc.rust-lang.org/std/cell/struct.RefCell.html#method.into_inner),
  [owned field transfer](https://doc.rust-lang.org/std/mem/fn.take.html) and
  [Vec/boxed-slice conversion](https://doc.rust-lang.org/std/vec/struct.Vec.html#method.into_boxed_slice)
  documentation. All used APIs are supported by installed Rust 1.98.0. No crate,
  feature, unsafe code, artifact format, capability rule or Ralloc path changed.
- Final workspace tests with `testing-hooks` pass
  (`workspace-compiler-finish-2.log`): 180 core tests, 57 runtime-parity cases,
  all 49 language fixtures and the remaining workspace/Ralloc suites. All seven
  core diagnostics pass explicitly (`storage-compiler-finish-1.log`). All-target
  workspace Clippy with warnings denied, changed/new Rust formatting, diff
  checks and the release build pass. No new Miri result is claimed.
- Control `compiler-finish-control-1.exe` has SHA-256
  `67982f03218474e3db2486225a284070e6940eb0a39e4a75290710745c86ab77`;
  `compiler-finish-control-source-1.zip` has SHA-256
  `c49034d863869f50ed05f62c39fb4205500c65d43b17142719743797c0fcc20c`.
  Candidate `compiler-finish-1.exe` has SHA-256
  `405ac4620a1160322a393c893bf357f87f3346549cf45e179c6d8e1d0927527a`;
  `compiler-finish-source-1.zip` has SHA-256
  `4581aaaf678619452f8460fd0b7d645bf078a387cc8debf580e40e969b8f4793`.
  Both binaries use the same expanded benchmark harness. Artifacts remain
  machine-local under `target/perf/full-system`.
- All twelve timing invocations passed all 26 correctness checks and the
  quality gate. Each used nine repeats, with matching sample scales within
  each pair: 16 for declarations, 4 for the module graph and 64 for ordinary
  compilation. Capture 1 ran the control rows before the candidate rows;
  capture 2 reversed executable order within each row. No invocation from this
  comparison was rejected. Files are
  `compiler-finish-{control,candidate}-<row>-{1,2}.json`/`.log`.

| Capture | Workload | Best us, before → after | Mean us, before → after | Mean-cycle change | CV %, before / after |
| --- | --- | --- | --- | --- | --- |
| 1 | `compile.declarations_256` | 1,932.58 → 1,708.00 | 2,044.07 → 1,772.06 | -13.12% | 6.05 / 5.97 |
| 2 | `compile.declarations_256` | 1,913.39 → 1,731.37 | 1,952.30 → 1,754.29 | -10.15% | 3.35 / 3.07 |
| 1 | `compiler.module_graph_large_uncached` | 13,822.88 → 11,208.03 | 14,073.43 → 11,361.22 | -19.32% | 1.41 / 1.60 |
| 2 | `compiler.module_graph_large_uncached` | 13,881.04 → 11,188.87 | 14,086.73 → 11,424.37 | -18.89% | 1.32 / 1.90 |
| 1 | `compile.full_pipeline` | 34.08 → 24.32 | 35.00 → 24.75 | -29.30% | 2.89 / 4.03 |
| 2 | `compile.full_pipeline` | 33.82 → 24.40 | 34.26 → 24.58 | -28.27% | 2.09 / 1.34 |

- Retain the owned assembly and compacted handoff: all three workloads improve
  best wall time and mean scheduled cycles in two independent clean pairs,
  while the measured declaration fixtures retain their original storage size.
  This includes the cost of shrinking finished buffers; it does not overturn
  the separately measured JIT operation-buffer policy. The final release binary
  matches the timed candidate hash, with no later Rust source changes.
- Wider frontend, bytecode-quality, runtime-subsystem, profiling and full-batch
  acceptance work remains open. The next observed compiler cost was the peephole
  optimizer's initial full-code clone before its first separately allocated
  output pass; its ownership and unchanged-chunk follow-up is recorded below.

## Owned peephole buffers and unchanged-chunk shortcut

The next sections 6–7 and 28–29 workstream removes the optimizer's redundant
instruction copies. Its API already owns `Program`, but each chunk was copied
before the first pass, and every pass allocated a second instruction vector
and a complete offset map, including the final pass that changed nothing.

- The optimizer now takes each instruction vector by value. A pass borrows a
  mutable slice and returns an optional shorter initialized prefix; only the
  owner truncates the vector. Writes stay strictly behind unread instructions
  after the first fold. No unsafe code or separate instruction-output buffer
  is needed. Completed code compacts once through a boxed slice, preserving
  the prior output's lack of spare capacity without cloning elements.
- A scan for a successful constant candidate precedes branch-target storage.
  No candidate means no scratch allocation. Candidates protected by interior
  jump targets can require a target set but cannot allocate a remapping table
  or mutate code unless a safe fold exists. Changed passes retain the former
  forward folding order and total branch mapping, including the end offset.
  Negative and oversized jump operands are not used as unchecked indexes or
  repaired into valid targets. Verification remains a separate safety gate.
- The compact unchanged-buffer regression failed on the old main allocation
  (`peephole-owned-before-1.log`) and passes after ownership transfer. Other
  regressions cover in-place prefix compaction, no final spare capacity after
  large input reservations or folding, invalid jumps remaining invalid, and
  21,120 exact-instruction comparisons against an independent copying model.
  That matrix covers all ten binary fold operations, signed limits, zero,
  both branch kinds, every insertion/target boundary, repeated passes and
  noncanonical unused operands. The arithmetic folder itself is unchanged.
- Eight source fixtures compare unoptimized VM behavior with optimized VM
  and JIT thresholds 0/1/8. Output and exact error strings preserve successful
  integer/comparison/function results, floor division, addition/multiplication
  overflow, minimum-integer division/negation, divide-by-zero and float errors.
- The existing straight-line and control-flow optimizer rows contain no
  foldable constants. Added `optimizer.declarations_256` for unchanged
  multi-function input and `optimizer.constant_chain_128` for the shrinking
  path. The latter adds VM/JIT checks, bringing pre-timing correctness to 28.
  All optimizer rows include the existing shared-Arc API's deliberate program
  clone, but not source compilation; `compile.full_pipeline` is the end-to-end
  companion. Repeated fixed-point passes are still used for deep constant
  chains; this change does not claim linear-time chain folding.
- Rechecked the full objective, repository/Rust router and concrete examples,
  architecture, bytecode, contribution, CI, public Rust API and performance
  guidance. Consulted the official Rust
  [Vec truncation](https://doc.rust-lang.org/std/vec/struct.Vec.html#method.truncate),
  [boxed-slice conversion](https://doc.rust-lang.org/std/vec/struct.Vec.html#method.into_boxed_slice)
  and [HashSet](https://doc.rust-lang.org/std/collections/struct.HashSet.html)
  documentation. Only long-stable standard-library APIs are used on installed
  Rust 1.98.0. No crate, feature, bytecode opcode, artifact format, runtime,
  capability rule or Ralloc implementation changed.
- Focused tests, all workspace tests with hooks, all-target Clippy with warnings
  denied, all seven explicit storage diagnostics, changed/new Rust formatting,
  diff checks and the release build pass (`peephole-owned-after-1.log`,
  `workspace-peephole-owned-1.log`, `clippy-peephole-owned-1.log`,
  `storage-peephole-owned-1.log`, `build-peephole-owned-1.log`). Workspace
  coverage includes 186 core tests, 57 parity cases, all 49 language fixtures
  and the remaining workspace/Ralloc suites. Compiler storage remains
  949/14,394/111,766 bytes for 1/32/256 declarations in both optimization modes.
  These capacity/accounting diagnostics are not RSS, allocator overhead or
  peak compiler-memory measurements. No new Miri result is claimed.
- Control `peephole-owned-control-1.exe` has SHA-256
  `b27440e6e39fffd2c89811da634cdd44b33d62fce5b3cb3dc426e899a02f5546`;
  `peephole-owned-control-source-1.zip` has SHA-256
  `b8b45c75153ec871ac81a3c87881cc8ee7bd3425f61fd35d8a67086daa6a34f1`.
  Candidate `peephole-owned-1.exe` has SHA-256
  `7e109d37d72bcef4cd57e53311620d2f173e1dedd62d72eca095c75edd9f806a`;
  `peephole-owned-source-1.zip` has SHA-256
  `b7aeff2e841a12d603082520df8686410760bcd6439e85dea1296f22041e98f6`.
  Both use the same expanded harness. All artifacts are machine-local under
  `target/perf/full-system`.
- The initial control straight-line capture at sample scale 64 failed the
  10% variance gate (11.83% CV) and is excluded in full; the invocation saved
  no decision-grade JSON. The two unchanged small-chunk rows now use matching
  scale 128 on both sides. All twenty invocations in captures 2 and 3 passed
  all 28 correctness checks and their variance gates, with nine repeats each.
  Declaration and constant-chain rows use scale 16; full compilation uses 32.
  Capture 2 ran control then candidate for each row; capture 3 reversed that
  order. Files are `peephole-owned-{control,candidate}-<row>-{2,3}.json`/`.log`.

| Capture | Workload | Best us, before → after | Mean us, before → after | Mean-cycle change | CV %, before / after |
| --- | --- | --- | --- | --- | --- |
| 2 | `optimizer.straightline` | 0.906 → 0.636 | 0.908 → 0.639 | -29.69% | 0.20 / 0.63 |
| 3 | `optimizer.straightline` | 0.911 → 0.641 | 0.916 → 0.646 | -29.48% | 0.79 / 1.52 |
| 2 | `optimizer.control_flow_passthrough` | 0.804 → 0.438 | 0.809 → 0.439 | -45.70% | 0.81 / 0.28 |
| 3 | `optimizer.control_flow_passthrough` | 0.805 → 0.443 | 0.807 → 0.448 | -44.55% | 0.18 / 1.52 |
| 2 | `optimizer.declarations_256` | 187.92 → 135.95 | 189.10 → 138.61 | -26.62% | 0.64 / 4.08 |
| 3 | `optimizer.declarations_256` | 191.49 → 136.56 | 192.91 → 137.93 | -28.49% | 0.94 / 0.52 |
| 2 | `optimizer.constant_chain_128` | 337.10 → 204.08 | 345.69 → 232.69 | -32.67% | 3.57 / 4.76 |
| 3 | `optimizer.constant_chain_128` | 313.45 → 213.68 | 333.30 → 239.79 | -28.03% | 2.41 / 4.13 |
| 2 | `compile.full_pipeline` | 31.31 → 29.20 | 31.51 → 29.30 | -7.01% | 0.68 / 0.22 |
| 3 | `compile.full_pipeline` | 31.15 → 29.71 | 31.34 → 30.11 | -3.89% | 0.56 / 1.63 |

- Retain the in-place owned buffer and no-candidate shortcut: every measured
  row improves best wall time and mean scheduled cycles in two clean pairs,
  including the shrinking-path and end-to-end companion. This supports these
  workloads, not a universal compiler speedup. There is no measured retained
  capacity increase. The final release binary matches the timed candidate,
  with no later Rust changes; the copying reference is test-only.
- This closes the observed peephole copying/no-op-pass workstream, not bytecode
  quality or the full system plan. Deep-chain fixed-point work, broader
  compiler/verifier coverage, runtime/feedback regressions, collections,
  closures, dynamic-call feedback, allocation profiling and final acceptance
  remain on the full ledger.

## Frame-resume chunk ownership and runtime attribution

- Returned to the open runtime/feedback work after completing the requested
  container recheck. The current 13-workload, threshold-0/8, cold/warm profile
  passes all 52 VM comparisons and releases every Ralloc allocation
  (`runtime-feedback-profile-before-1.log`). The warm integer fixture executes
  16,390 JitOps and records 4,097 specialization hits, but makes only one
  Ralloc allocation. Its instruction fetch and hit recording repeatedly resolve
  the same installed chunk. Those two call sites account for 20,487 lookups
  in that profile, derived from source-level call counts, not hardware counters.
- `jit/execution.rs` now owns `ChunkExecution`, a safe frame-resume borrow of
  one installed chunk, existing statistics, optional profile and the shared
  feedback-byte budget. Fetch and feedback methods use that borrow, removing
  per-op chunk-table resolution. The immutable threshold is selected once;
  remaining feedback-budget arithmetic runs only when an edge first activates.
  The former program-wide recording methods have been removed, not layered
  behind the new interface. Instruction bounds, representation guards,
  hysteresis, saturation and the program-wide 64-KiB feedback bound remain.
- A call, return or error ends the borrow. Lazy installation and recursive
  calls therefore cannot leave a suspended frame holding stale code. Runtime
  declarations are borrowed from the VM's existing shared verification token;
  no tables, permission allowlists or runtime values are cloned for this view.
  No new opcode, decode bridge, unsafe code, heap allocation, allocator method,
  retained field, dependency or public API is introduced.
- Added VM/JIT benchmark pairs for stable floating-point arithmetic, an operand
  changing from I64 to F64 halfway through a 4,096-iteration loop, and 512
  dynamic closure calls with a capture. Compilation is outside these runtime
  timings; closure construction and cleanup occur in each complete execution.
  They are attribution workloads, not an implementation of F64 specialization
  or call-site feedback. The harness now runs 34 pre-timing correctness checks.
- New tests reject missing/uninstalled execution chunks without eager lowering
  and preserve fetch bounds. A recursive shared-chunk fixture covers lazy
  installation, resumed I64-to-F64 demotion, thresholds 0/1/8, optional profiling
  and repeated fresh runs against the VM. Initial fixture assertions were
  corrected to preserve the VM's `3072.0` formatting and the existing generic
  multiply listing name; no language behavior was changed to satisfy them.
- Full workspace tests with hooks pass (`workspace-runtime-chunk-1.log`):
  187 core unit tests, 57 runtime-parity cases, four feedback integration tests,
  the language fixtures and all remaining workspace/Ralloc suites. All-target
  Clippy with warnings denied, all seven explicit core storage diagnostics,
  changed/new Rust formatting and diff checks pass (`clippy-runtime-chunk-1.log`,
  `storage-runtime-chunk-1.log`). All 52 complete before/after profile blocks
  match exactly, including opcode/stack/adaptation counts and allocator probes,
  allocation sizes, reuse and cleanup (`runtime-feedback-profile-after-1.log`).
  This is count/layout evidence, not a latency or RSS claim. No new Miri result
  is claimed.
- Rechecked the objective, Rust/router examples, architecture, VM, memory,
  performance and contribution/CI requirements, plus the relevant standard
  library [Option borrowing API](https://doc.rust-lang.org/stable/std/option/enum.Option.html#method.as_mut)
  and [Rust borrowing documentation](https://doc.rust-lang.org/book/ch04-02-references-and-borrowing.html).
  The prior Ralloc ownership/safety gate remains applicable; no allocator or
  encoded-value operation is changed. Installed Rust remains 1.98.0.
- Control `runtime-chunk-control-1.exe` has SHA-256
  `dac2909cf16a39753c35a01adb4f0c43439d6a41eaa4426efe4a0932163ed8cc`;
  `runtime-chunk-control-source-1.zip` has SHA-256
  `b1a9adcf6d853e8c34a3bef46650dc981ae5afc4bccebb57b015726e9c6517ee`.
  Candidate `runtime-chunk-1.exe` has SHA-256
  `f5e3d2a84f9764a0af0985096c72c578b95c583b041f3edf17f3a645c8392eab`;
  `runtime-chunk-source-1.zip` has SHA-256
  `eb00854bff3a22e3ad6c5dc406160bf66ab8113566cc8b0c99e2498c5856fce0`.
  Both use the expanded harness. All artifacts are machine-local under
  `target/perf/full-system`; the candidate's Rust sources remain fixed during
  measurement. Runtime timing excludes Cargo, tests, profiling and compression.
- Completed 56 timing invocations, each with all 34 correctness checks and
  nine repeats. Forty-one invocations pass their complete quality gate; fifteen
  are rejected in full. Files are
  `runtime-chunk-{control,candidate}-<row>-{1,2,3}.json/.log`.
  Capture 1 runs control then candidate; capture 2 reverses that order.
  Capture 3 repeats candidate first for quickened loops, calls and floats,
  and control first for mixed loops, closures, slot multiplication,
  nonadaptive loops and maps. Matching sample scales are 16 for loops/slot
  arithmetic, 32 for calls/closures, 8 for maps, and 64 for tiny dispatch and
  cold lowering. No samples or individual rows are salvaged from a rejected
  invocation.
- The following rows have two complete, independent clean pairs with reversed
  executable order. Calls, mixed-type loops and encoded multiplication improve
  both best wall time and mean scheduled cycles. Tiny dispatch also improves
  slightly; cold lowering is effectively a neutral guardrail, not a material
  speedup claim.

| Capture | Workload | Best us, before → after | Mean us, before → after | Mean-cycle change | CV %, before / after |
| --- | --- | --- | --- | --- | --- |
| 1 | `jit.calls` | 42.529 → 41.014 | 42.740 → 41.263 | -3.14% | 0.41 / 0.55 |
| 3 | `jit.calls` | 42.313 → 41.141 | 42.931 → 41.758 | -2.71% | 3.62 / 3.08 |
| 2 | `runtime.jit_mixed_loop_4096` | 407.843 → 381.774 | 409.198 → 382.881 | -6.43% | 0.24 / 0.27 |
| 3 | `runtime.jit_mixed_loop_4096` | 577.736 → 520.072 | 578.818 → 521.560 | -9.89% | 0.19 / 0.16 |
| 2 | `runtime.jit_slot_mul_immediate_4096` | 108.558 → 85.396 | 109.774 → 85.918 | -21.74% | 1.02 / 0.70 |
| 3 | `runtime.jit_slot_mul_immediate_4096` | 108.459 → 85.335 | 111.743 → 86.232 | -22.84% | 7.27 / 0.99 |
| 1 | `jit.chunk_dispatch` | 0.774 → 0.764 | 0.777 → 0.768 | -1.25% | 0.29 / 0.55 |
| 2 | `jit.chunk_dispatch` | 0.766 → 0.755 | 0.772 → 0.759 | -1.71% | 0.73 / 0.42 |
| 1 | `jit.codegen_straightline_cold` | 2.235 → 2.225 | 2.244 → 2.241 | -0.10% | 0.21 / 1.43 |
| 2 | `jit.codegen_straightline_cold` | 2.245 → 2.220 | 2.258 → 2.232 | -1.15% | 0.93 / 0.62 |

- These other complete eligible pairs remain visible, but do not establish
  repeated wins. General quickened/nonadaptive loops and floats have only one
  clean pair each. The sole clean closure pair is slightly worse. Map results
  are contradictory: +13.09%, +0.01% and -2.10% mean cycles. That first
  regression is not discarded merely because subsequent pairs differ.

| Capture | Workload | Best us, before → after | Mean us, before → after | Mean-cycle change | CV %, before / after |
| --- | --- | --- | --- | --- | --- |
| 1 | `runtime.jit_hot_loop_4096_quickened` | 447.775 → 420.548 | 451.021 → 423.465 | -6.07% | 0.71 / 0.66 |
| 2 | `runtime.jit_hot_loop_4096_no_quickening` | 724.135 → 703.399 | 726.789 → 705.274 | -2.97% | 0.44 / 0.17 |
| 1 | `runtime.jit_float_loop_4096` | 446.713 → 429.889 | 447.904 → 431.033 | -3.86% | 0.14 / 0.44 |
| 2 | `runtime.jit_closure_calls_512` | 319.469 → 321.879 | 320.552 → 322.980 | +0.76% | 0.27 / 0.17 |
| 1 | `runtime.jit_map_set_get_256` | 99.323 → 113.345 | 100.717 → 113.750 | +13.09% | 1.84 / 0.34 |
| 2 | `runtime.jit_map_set_get_256` | 97.818 → 96.851 | 98.247 → 98.502 | +0.01% | 0.31 / 1.14 |
| 3 | `runtime.jit_map_set_get_256` | 117.946 → 115.299 | 118.107 → 115.633 | -2.10% | 0.12 / 0.26 |

- Rejected CVs by capture (control/candidate where both fail): capture 1
  nonadaptive loop 8.41/6.18%, mixed-loop candidate 14.29%, closures
  10.25/12.03%, slot-multiply candidate 44.81%; capture 2 quickened loop
  15.97/14.11%, call control 10.66%, float candidate 12.58%; capture 3
  quickened-loop candidate 15.81%, float candidate 11.09%, closures
  10.08/12.07%, nonadaptive-loop control 8.96%. General hot-loop limits remain
  5%; other rows use 10%. The accepted slot-multiply control's 7.27% CV in
  capture 3 is retained as reported, not hidden.
- A read-only check reports the Windows Balanced power plan; no power setting
  was changed. Additional processor/load telemetry was unavailable because
  Windows denied the CIM queries. The cause of cross-invocation timing shifts
  is not established. This does not block implementation or correctness checks.
- Rechecked both binary and source-archive hashes after timing. The current
  release executable matches the candidate, and all 114 archived Rust files
  are byte-identical to the worktree. The first text-only comparison was
  affected by PowerShell's default decoding; SHA-256 byte comparisons passed.
  Final changed/new Rust formatting and diff checks pass.
- Retain the narrower frame-resume borrowing change for the confirmed call
  (2.71–3.14%), mixed-loop (6.43–9.89%) and encoded-multiply (21.74–22.84%)
  cycle reductions, unchanged detailed work/allocation profiles and preserved
  correctness. This is not whole-runtime acceptance: general hot-loop/float
  confirmation, closure guardrails and the inconsistent map results remain
  open. No dynamic call-site specialization, allocator improvement, or full
  optimization-batch completion is claimed.

## Phase — Direct call-argument transfer (2026-09-07)

Observed cost and scope:

- The retained Vec/Box policy is unchanged: fixed metadata uses boxed slices;
  live operand/call stacks remain bounded vectors with ordinary ownership.
  This phase removes temporary containers, not those live stacks.
- `pop_args` used `Vec::split_off` for every nonempty argument list. Closure
  calls then decoded a second vector and extended it with those arguments.
  A new isolated, release-mode `call_profile` test measures host allocations
  separately from Ralloc. Its process-wide counting `System` wrapper exists
  only in that test binary; production and timing executables are unaffected.
- Six callable shapes (direct, function value, closures with 0/1/8/32 captures),
  three call counts (0/1/512), thresholds 0/8 and first/repeated execution make
  72 profiles per version. Compilation, output reservation and reporting are
  outside each interval; lazy lowering and execution cleanup are inside.
  Exact VM output and complete Ralloc cleanup pass in every case. Counts are
  build-specific diagnostics, not an allocation guarantee, RSS or timings.

Implementation and invariants:

- `FrameExit::Call` now contains typed target metadata and an argument count.
  The suspended caller owns the arguments until `start_call` drains its suffix
  directly into callee slots. No temporary argument Vec or replacement Box is
  allocated. The untouched prefix and caller allocation/capacity survive.
- Dynamic callable resolution consumes the leading drained operand. Closures
  borrow their current captured storage under generation/type validation and
  decode directly into leading callee slots. A shared borrowed decoder in
  `runtime/heap.rs` also supplies existing owned-snapshot callers via `collect`.
  No capture instance is cached or retained in suspended call metadata.
- The closure heap lock covers capture access and frame initialization only;
  it ends before lazy lowering and before any callee/user code. Ralloc calls
  use the existing safe owned-frame API and normal heap-to-allocator lock order.
  Frame sizes, pooling limits, slot encoding and heap budgets are unchanged.
- Arity still precedes logical-depth checking; callable kind/generation still
  precedes arity. `Drain` drops unconsumed arguments on an early return.
  Unit tests verify prefix/capacity preservation and recovery after a slot
  encoding failure. Four differential tests cover nested argument side effects,
  zero-argument calls, distinct closure instances and fresh captured values,
  recursive closures, a callee freeing its own closure, wrong callable kinds,
  arity/depth error precedence, stale generations and repeated failures at
  thresholds 0/1/8. The portable VM remains unchanged.

Measured allocation evidence (`target/perf/full-system`):

| Warm 512 calls, threshold 8 | Host allocations before → after | Host reallocations before → after | Newly allocated host bytes before → after |
| --- | --- | --- | --- |
| Direct | 518 → 6 | 0 → 0 | 33,667 → 899 |
| Function value | 518 → 6 | 0 → 0 | 33,667 → 899 |
| Closure, 0 captures | 1,032 → 8 | 2 → 2 | 164,751 → 911 |
| Closure, 1 capture | 1,034 → 10 | 514 → 2 | 66,575 → 1,039 |
| Closure, 8 captures | 1,034 → 10 | 514 → 2 | 296,847 → 1,935 |
| Closure, 32 captures | 1,034 → 10 | 514 → 2 | 1,086,351 → 5,007 |

- Raw evidence: `call-allocations-before-1.log` and
  `call-allocations-after-1.log`. All 72 runtime/Ralloc counter records are
  exactly unchanged, including codec work, heap-lock count, allocation sizes,
  probes, Ralloc high-water payload and zero post-run live storage. The
  removed host allocations do not imply improved raw Ralloc allocator latency.
- The 32-capture case also reduces requested reallocation extents from
  2,097,728 to 576 bytes. These are requests, not measured copying. Remaining
  closure allocations/reallocations belong to per-run setup and construction,
  not repeated call transfer. In these scalar fixtures, host allocation counts
  no longer grow between one and 512 calls.
- The independent 52-workload dispatch/adaptation/Ralloc profile is identical
  to the prior source: `runtime-feedback-profile-after-1.log` versus
  `call-transfer-runtime-profile-1.log`, all 52 complete records matched.
- Added paired VM/JIT function-value timing fixtures, bringing pre-timing
  correctness checks from 34 to 36. Control and candidate share that harness;
  direct call, closure and unrelated dispatch/cold/loop/map guardrails are
  timed separately without diagnostic instrumentation.

Documentation and verification:

- Re-read the complete goal, applicable user/repository Rust rules and four
  examples, Router, runtime memory/VM/performance guidance, and relevant Ralloc
  ownership, zero-initialization, heap-vector and value-codec contracts. Earlier
  full subsystem/supplemental gates remain recorded above. No dependencies or
  public API changed. Checked official [GlobalAlloc safety and forwarding](https://doc.rust-lang.org/stable/std/alloc/trait.GlobalAlloc.html),
  [System ownership](https://doc.rust-lang.org/stable/std/alloc/struct.System.html)
  and [Vec drain/split-off behavior](https://doc.rust-lang.org/stable/std/vec/struct.Vec.html#method.drain).
  New unsafe code is confined to the test-only System forwarding wrapper,
  with unchanged pointers/layouts and non-allocating atomic instrumentation.
- `call-transfer-harness-1.log`: 13 benchmark unit tests pass.
  `call-transfer-focused-1.log`: existing core/parity/feedback suites pass.
  `call-transfer-boundaries-1.log`: 189 core tests pass; one new expected-error
  substring incorrectly said "reference" instead of the VM's actual "pointer".
  That fixture was corrected without changing runtime behavior.
- `workspace-call-transfer-1.log`: full hooks workspace passes, including
  189 core tests, 57 parity tests, four new call-transfer tests, four feedback
  tests, language fixtures and Ralloc suites. `clippy-call-transfer-1.log`:
  all targets/hooks with `-D warnings` pass. All changed/new Rust formatting,
  `git diff --check`, both explicit count profiles and seven isolated storage
  diagnostics (`storage-call-transfer-1.log`) pass. No new Miri claim.

Immutable comparison sources/binaries:

- `call-transfer-control-1.exe`:
  `4616e921ced664a5ac35eb045ffec3d03dca18b1b75cc2e6e827e06f146e9a8f`.
- `call-transfer-control-source-1.zip`:
  `4122f4dc73fb8afd16f581e319873f8954abe6b3f6d511577f606355e436c6f7`.
- `call-transfer-candidate-1.exe`:
  `5d75cf133860bfd99063dbb705d0a79645355172316173b394e48af69051e496`.
- `call-transfer-candidate-source-1.zip`:
  `4504be096cadd49e5bf76992baffbfc9b4858e9b4287d0cfe56b68b8d4acb1c7`.

After timing, all 116 archived Rust files were SHA-256 byte-identical to the
worktree, and the current release executable matched the candidate hash.
No Rust changes followed the final candidate build; reporting/documentation
updates do not alter the measured source. Final formatting and diff checks pass.

Timing and acceptance (Windows native, Rust 1.98.0, existing Balanced power
plan; no machine settings changed):

- All 36 invocations passed all 36 pre-timing correctness checks. There are 33
  eligible captures and 15 complete eligible pairs below. Repeats are nine.
  Captures 1/2 use control-first/candidate-first order respectively; larger
  no-call/cold guardrail captures 3/4 use candidate-first/control-first. Sample
  scales match within every pair; the two larger guardrail pairs both use 128.
  Filenames are `call-transfer-{control,candidate}-<workload>-<capture>.json`
  with corresponding logs.
- Three invocations were rejected in full, not cherry-picked: candidate
  `jit.chunk_dispatch` captures 1/2 (11.8% / 14.0% CV), and control
  `jit.codegen_straightline_cold` capture 1 (14.69% CV). The unmatched eligible
  sides are not comparisons. No rejected capture supports a speed claim.
- Both independent pairs support the targeted reductions: function-heavy
  calls 17.0–22.9%, function-value calls 17.9–27.6%, and closures 41.6–42.3%
  in mean scheduled cycles. The first call/function-value pairs are more
  variable than their confirmations; the ranges are individual paired results,
  not confidence intervals or expected universal gains.

| Workload / capture | Scale | Best µs before → after | Mean µs before → after | Mean cycles before → after | Cycle change | CV % before / after |
| --- | --- | --- | --- | --- | --- | --- |
| `jit.calls` / 1 | 32 | 44.813 → 34.254 | 48.338 → 37.328 | 124947.61 → 96390.17 | -22.86% | 4.87 / 9.99 |
| `jit.calls` / 2 | 32 | 41.155 → 34.172 | 41.618 → 34.475 | 107574.71 → 89247.01 | -17.04% | 1.12 / 0.63 |
| `jit.chunk_dispatch` / 3 | 128 | 0.771 → 0.771 | 0.776 → 0.773 | 2010.00 → 2002.27 | -0.38% | 0.96 / 0.19 |
| `jit.chunk_dispatch` / 4 | 128 | 0.770 → 0.769 | 0.837 → 0.783 | 2164.42 → 2025.72 | -6.41% | 7.96 / 2.20 |
| `jit.codegen_straightline_cold` / 2 | 64 | 2.280 → 2.286 | 2.306 → 2.308 | 5968.74 → 5973.98 | +0.09% | 0.92 / 0.74 |
| `jit.codegen_straightline_cold` / 3 | 128 | 2.293 → 2.313 | 2.321 → 2.323 | 6007.06 → 6012.70 | +0.09% | 2.57 / 0.35 |
| `jit.codegen_straightline_cold` / 4 | 128 | 2.306 → 2.306 | 2.360 → 2.360 | 6103.10 → 6104.85 | +0.03% | 4.89 / 3.58 |
| `runtime.jit_closure_calls_512` / 1 | 32 | 323.432 → 186.939 | 325.518 → 187.868 | 842565.04 → 486356.27 | -42.28% | 0.36 / 0.37 |
| `runtime.jit_closure_calls_512` / 2 | 32 | 325.247 → 187.392 | 326.783 → 191.122 | 845677.46 → 494122.31 | -41.57% | 0.35 / 3.38 |
| `runtime.jit_function_value_calls_512` / 1 | 32 | 223.108 → 181.423 | 256.678 → 185.820 | 664120.45 → 480958.53 | -27.58% | 9.52 / 5.65 |
| `runtime.jit_function_value_calls_512` / 2 | 32 | 222.691 → 183.029 | 224.702 → 184.246 | 581195.13 → 476914.29 | -17.94% | 0.89 / 0.56 |
| `runtime.jit_hot_loop_4096_quickened` / 1 | 16 | 421.898 → 425.489 | 430.532 → 427.374 | 1114101.00 → 1106169.14 | -0.71% | 3.12 / 0.30 |
| `runtime.jit_hot_loop_4096_quickened` / 2 | 16 | 421.797 → 421.031 | 425.862 → 424.372 | 1102416.95 → 1098428.17 | -0.36% | 0.79 / 0.73 |
| `runtime.jit_map_set_get_256` / 1 | 8 | 98.562 → 98.595 | 99.191 → 102.658 | 256749.76 → 265667.82 | +3.47% | 0.42 / 6.68 |
| `runtime.jit_map_set_get_256` / 2 | 8 | 99.299 → 98.254 | 100.422 → 98.864 | 259956.22 → 255905.05 | -1.56% | 0.69 / 0.60 |

- No-call dispatch and cold lowering have two complete larger-sample guardrail
  pairs; cold mean cycles change only +0.03–0.09%. Quickened loops change
  -0.36–0.71%. Map results are mixed (+3.47%, -1.56%), so no map improvement
  is claimed. These are guardrails for this transfer change, not resolution of
  all prior whole-runtime measurements. The larger no-call mean reduction in
  capture 4 does not establish an allocation benefit for workloads without calls.
- Retain direct transfer for the observed elimination of per-call containers,
  independently confirmed call/closure timing gains, unchanged operation/Ralloc
  work and preserved correctness/ownership. There is no new production unsafe
  code, pool, dependency, opcode, or retained adaptive/capture metadata.
  This phase does not implement dynamic-callee identity feedback or complete
  the broader function/closure optimization requirements.

## Requested Vec/Box recheck on the current lowering pipeline

- The renewed user request prompted a bounded recheck of the instruction-buffer
  exception; the earlier all-boxed timings predate several lowering changes.
  No blanket Vec or Box performance claim follows from either experiment.
- Fixed certificates, block tables, chunk positions, edge counters and scalar
  feedback tables remain boxed slices. Live operand/call stacks and builders
  retain Vec ownership. The only nested runtime Vec is the private, eight-entry
  cleared operand-stack pool; it does not individually box values or frames.
  Installed `JitOps` exposes slices, not push/truncate/capacity mutation, and
  installation leaves less than 4 KiB of spare operation storage per chunk.
- Re-ran the current workspace suites and all seven explicit storage diagnostics.
  Stack-pool bounds, independent clones, idle-capacity accounting, direct argument
  transfer, fixed installed extents and the reclamation threshold pass.
  Current layout is `CallFrame=88`, `JitOp=24`, `JitChunk=88`, `JitProgram=344`.
  Call-vector backing extents remain 0/352/704/1,408 bytes at depths 0/1/8/16;
  a maximum-depth boxed option array reserves 1,408 bytes at every depth. These
  are backing-storage observations, not a boxed-stack latency comparison.
- Rechecked the official
  [Vec conversion contract](https://doc.rust-lang.org/stable/std/vec/struct.Vec.html#method.into_boxed_slice).
  The temporary comparison changes only `JitOps` to an unconditional boxed
  slice and charges its exact extent. Its Vec-specific capacity unit tests were
  not adapted or claimed as a passing boxed-candidate suite; executable semantics
  were checked by the same 40-case benchmark harness in every invocation.
- Frozen current Vec executable: `vec-box-current-vec-2.exe`, SHA256
  `6f86050c6f3a349400e796b3b28e41f928a9a6f0f0793fee3367851b2d7394ac`;
  source: `vec-box-current-vec-source-2.zip`, SHA256
  `3342a25b41c917c2ae815bac229f5a8516786c9581ba8c0e22ad4595b7db34e4`.
  Boxed executable: `vec-box-current-box-1.exe`, SHA256
  `3584d4bc3a7522fed87de1dc4dbe5626e6c30cbbc82d12af1e7a830ded6a8bb8`;
  source: `vec-box-current-box-source-1.zip`, SHA256
  `9192db6ab0da5c702b5234afab47aa833102c2c08866d86a04a9805866eba89e`.
  All artifacts are under `target/perf/full-system`. Comparing all 182 archived
  files found only `jit/chunk.rs` different. The initial `vec-1.exe` copy was
  made before its build completed, still hashes to the older call-feedback
  control, and was never used in this comparison; its matching-number source
  archive is not a valid executable/source pairing.
- Ran eight uninstrumented invocations with all 40 correctness checks, nine
  repeats, machine label `tinyone-workstation`, and unchanged Balanced power
  policy. No Cargo builds, allocation profiles or compression ran during timing.
  `codegen-1` (scale 64, Vec first) failed the boxed straightline CV gate at
  16.75%; `codegen-2` (scale 128, Box first) failed the Vec gate at 12.48%.
  Both complete comparisons are excluded; other rows from the rejected
  invocations are not accepted selectively.
- Isolated `straightline-3` (scale 512, Vec first) produced one eligible pair:
  mean time 2.333835 -> 2.331838 microseconds, mean cycles 6039.165 -> 6036.139,
  and CV 4.21% / 0.54% (Vec -> Box). That is effectively a tie: Box changes
  mean cycles by -0.05%, not a measured Vec win. The reversed `straightline-4`
  comparison failed the boxed CV gate at 10.27% and is excluded even though its
  Vec partner passed. Larger samples therefore did not establish two clean
  confirmations. No further timing retries were used to force a conclusion.
- Container choices are unchanged. `chunk.rs` was restored byte-for-byte to
  SHA256 `817f3aba3976d92e76a05af6552322da6859882288b3eb91f78cc4ef777e5c75`.
  The current evidence supports encapsulation and bounded storage, not a new
  claim that Vec is faster than Box. Historical cold-regression numbers above
  must not be presented as measurements of today's lowering pipeline.
- Full workspace tests pass (`vec-box-review-tests-1/2.log`: 192 core tests,
  57 parity tests, four call-feedback and four call-transfer tests, plus the
  remaining workspace suites). All seven explicit diagnostics pass in
  `vec-box-review-storage-1.log`. The first Clippy run caught `manual_inspect`
  in the pending call-feedback error path. Replaced `map_err` with
  [`inspect_err`](https://doc.rust-lang.org/stable/std/result/enum.Result.html#method.inspect_err)
  while preserving the original error; the second workspace test run and
  all-target Clippy pass (`vec-box-review-clippy-2.log`). Changed/new Rust
  formatting checks and `git diff --check` pass.

  After all measurements, all 118 Rust files still match the frozen Vec source
  archive byte-for-byte. Restored the normal, non-`testing-hooks` release build
  and re-ran its 40 correctness checks successfully. That later rebuild hashes
  to `87895ba496a017a78ea92ae07dc213e88066bb5f7537ba6b6bbb9c5040630660`;
  it is not substituted for either frozen executable in the timing evidence.

### Pending call-feedback prototype, not performance acceptance

- The working tree now includes bounded inline dynamic-call sampling and guarded
  verified callee-layout metadata. Eight stable entries can promote a site;
  32 unstable samples or four guard misses leave it permanently generic.
  Closures still resolve their current generation, kind and captures, and the
  guard includes the current total argument count. No captured instance or
  growable feedback table is retained. The new four-test differential suite
  covers changed identities/layouts, fresh captures and clones, arity/kind/stale
  failures, disabled feedback, and unstable sites that must stop sampling.
- Inline call state still fits the 24-byte JitOp. However, the five extra optional
  profile counters enlarge the always-present `JitProgram` header from 304 to
  344 bytes, even with profiling disabled. This is not a zero-memory-cost change:
  the 128-entry diagnostic charge rises by exactly 5,120 bytes. Existing retained
  accounting includes the larger header. There is no new feedback allocation.
- Re-ran all 72 cold/warm host-call profiles after the timing experiment
  (`vec-box-review-call-allocations-1.log`). Every host allocation/deallocation/
  reallocation count, requested-byte total, size bucket, runtime codec/lock
  counter and complete Ralloc snapshot matches `call-allocations-after-1.log`
  exactly. All VM-output and Ralloc-cleanup checks pass. Warm 512-call runs
  still use six host allocations for direct/function-value calls and ten for
  one-capture closures, with no repeated temporary argument/capture buffers.
  These instrumented counts do not establish a feedback latency improvement.
- The current Vec/Box comparison includes this same prototype on both sides;
  it does not measure the benefit of call feedback. Expanded harness coverage
  is now 40 checks with polymorphic and unstable dynamic-call fixtures. The
  separate frozen `call-feedback-control-1.exe` and its source archive remain
  the pre-feedback control. Post-promotion module-permission coverage and
  independent uninstrumented call-feedback timing pairs are still required
  before accepting that prototype or section 24.

## Call-feedback authority coverage and call-handoff revisions

- Previous goal turn: progress. It fixed the error-inspection lint, completed
  the requested container recheck, and established unchanged 72-case allocation
  records. Container timing was inconclusive; it did not accept call feedback.
  This turn returns to sections 15, 21, 24 and 29 without narrowing the full plan.
- Re-read the entire objective, including the initially truncated allocator
  section, and rechecked the working tree. No additional repository agent file
  was found. Existing Rust/router/Ralloc gates remain applicable. Re-read the
  module-capability contract and relevant VM/feedback/frame code, plus the
  official [filesystem write contract](https://doc.rust-lang.org/stable/std/fs/fn.write.html)
  and [type-size documentation](https://doc.rust-lang.org/stable/std/mem/fn.size_of.html).
- Added post-promotion module-authority coverage in `tests/module_sandbox.rs`.
  Harmless calls warm the same site before a protected filesystem probe.
  Exported function values and closures, granted and denied modules, thresholds
  0/1/8, and original/cloned programs match the VM's output, memory or exact
  error. The enabled cases assert actual guard hits, not merely successful
  generic fallback. Caller/root authority cannot replace the callee's grant.
  An initial fixture used unsupported standalone `host.probe` syntax; it now
  obtains the function value through an exported factory. No language syntax
  or production permission code was changed.
- Added warmed dynamic-recursion overflow/recovery coverage. Both dynamic sites
  specialize before the 16-call limit is exceeded; the error still matches the
  VM and a subsequent fresh run succeeds. Existing 1/2/16-depth tests remain.
  `call-feedback-authority-2.log` passes all five call-feedback and 15 sandbox
  tests. `call-feedback-workspace-1.log` and `call-feedback-clippy-1.log` pass
  the full workspace and all-target Clippy with testing hooks.
- The unchanged pre-feedback control is `call-feedback-control-1.exe`, SHA256
  `c31264f7ea56bad22c5add1dc5ac29d9fa71da50768892e1c063dff20b598d07`;
  `call-feedback-control-source-1.zip` hashes to
  `0db7be4a384532e671ab0c0f2aeb0b6eaba5422dd5606e2bd0f50e431409fd5a`.
  The expanded harness is byte-identical on control and candidate:
  `201dd189eb7f4079ec8ea3d638428c4bb0bb13300e8f2e66899f89725a7c1341`.
  All artifacts below are in `target/perf/full-system`.

### Initial guarded-call candidate: no consistent acceptance

- `call-feedback-candidate-1.exe` hashes to
  `87895ba496a017a78ea92ae07dc213e88066bb5f7537ba6b6bbb9c5040630660`;
  `call-feedback-candidate-source-1.zip` hashes to
  `93529fcf6f12fbb60d4625ce4baa7b8078fdf1a2383bef081105ecd180f6f3bd`.
- Twenty invocations used all 40 correctness checks, nine repeats, scale 32,
  the `tinyone-workstation` label and unchanged Balanced policy. Capture 1
  runs control first; capture 2 reverses order. Nineteen invocations pass all
  gates, forming nine complete pairs. Candidate unstable capture 2 fails at
  10.60% CV; that entire invocation and incomplete pair are excluded.
- The first four dynamic-call pairs regress by 1.39-3.80% in mean cycles.
  Reversed pairs show larger cross-capture shifts and conflicting directions;
  even static calls change +15.68% and -16.78%. Their individual CV gates
  passing does not establish a stable causal gain. The cause of the shifts
  was not established. No favorable subset is used to accept this candidate.

| Workload / capture | Mean us, control -> candidate | Mean cycles, control -> candidate | Cycle change | CV %, control / candidate |
| --- | ---: | ---: | ---: | ---: |
| `jit.calls` / 1 | 34.436 -> 39.836 | 89150.07 -> 103129.83 | +15.68% | 7.46 / 8.96 |
| `jit.calls` / 2 | 41.602 -> 34.622 | 107696.87 -> 89629.94 | -16.78% | 0.17 / 0.78 |
| `runtime.jit_closure_calls_512` / 1 | 185.089 -> 192.106 | 479117.24 -> 497338.32 | +3.80% | 0.27 / 0.40 |
| `runtime.jit_closure_calls_512` / 2 | 207.060 -> 195.460 | 535999.66 -> 506002.07 | -5.60% | 4.61 / 4.24 |
| `runtime.jit_function_value_calls_512` / 1 | 183.252 -> 185.786 | 474386.68 -> 480984.57 | +1.39% | 0.39 / 0.48 |
| `runtime.jit_function_value_calls_512` / 2 | 180.788 -> 213.637 | 467354.21 -> 553006.67 | +18.33% | 0.70 / 0.29 |
| `runtime.jit_polymorphic_calls_512` / 1 | 250.852 -> 259.186 | 649403.56 -> 670864.70 | +3.30% | 0.23 / 2.79 |
| `runtime.jit_polymorphic_calls_512` / 2 | 313.431 -> 292.003 | 811386.87 -> 755816.95 | -6.85% | 0.20 / 8.78 |
| `runtime.jit_unstable_calls_512` / 1 | 228.229 -> 231.816 | 590864.54 -> 600175.16 | +1.58% | 0.36 / 0.33 |

### Unified call request: structurally simpler, still not general acceptance

- Replaced separate target, argument-count and call-site fields with one
  private `CallRequest` enum. Each request variant owns its applicable arity;
  guarded arity is no longer duplicated. Frame entry recovers the call site
  from the suspended caller's chunk and already-advanced PC. It accepts the
  caller cursor directly and continues draining only its operand suffix.
- Layout diagnostics (`call-feedback-handoff-before-1.log` and
  `call-feedback-handoff-after-1.log`) show the old target alone and new
  complete request are both 24 bytes. Removing the separate fields reduces
  call-side metadata, but `FrameExit` stays 64 bytes. `CallFrame` stays 88,
  and there is no claim of a total frame/exit footprint reduction.
- Full workspace and Clippy pass in `call-feedback-handoff-workspace-1.log`
  and `call-feedback-handoff-clippy-1.log`. All 72 host-allocation records,
  codec/lock counters and full Ralloc snapshots still exactly match the
  accepted argument-transfer profile (`call-feedback-handoff-allocations-1.log`).
  No pool, feedback allocation, capture retention or production unsafe was added.
- `call-feedback-candidate-2.exe` hashes to
  `1d6954b36abdc886789d8a47766f4ba9ea0412860c9c1b1559400b04d973534d`;
  `call-feedback-candidate-source-2.zip` hashes to
  `b66b03d06efa516a16ae10553608a4f486117d5f6c6b5d71de160c2a8eb47d7f`.
  The `call-feedback-v2-*` captures use that candidate and the same control.
  All 20 invocations pass 40 correctness checks and the unchanged nine-repeat,
  scale-32 gates, producing ten complete reversed-order pairs.
- Closures improve 1.45-4.08% and polymorphic calls are close, but function values
  regress 1.17-3.15% and static calls 2.17-3.79%. Unstable results include a
  +27.26% capture and a +0.47% capture; the larger result is not silently removed.
  This does not justify accepting the whole call-feedback change.

| Workload / capture | Mean us, control -> candidate | Mean cycles, control -> candidate | Cycle change | CV %, control / candidate |
| --- | ---: | ---: | ---: | ---: |
| `jit.calls` / 1 | 39.816 -> 41.328 | 103070.37 -> 106979.82 | +3.79% | 6.51 / 0.14 |
| `jit.calls` / 2 | 33.779 -> 34.513 | 87447.28 -> 89345.05 | +2.17% | 0.22 / 0.21 |
| `runtime.jit_closure_calls_512` / 1 | 211.642 -> 202.968 | 547720.67 -> 525353.97 | -4.08% | 0.24 / 5.73 |
| `runtime.jit_closure_calls_512` / 2 | 218.246 -> 215.083 | 564962.81 -> 556780.97 | -1.45% | 0.18 / 0.28 |
| `runtime.jit_function_value_calls_512` / 1 | 184.040 -> 189.829 | 476442.90 -> 491470.55 | +3.15% | 6.09 / 5.36 |
| `runtime.jit_function_value_calls_512` / 2 | 184.446 -> 186.560 | 477446.12 -> 483009.45 | +1.17% | 4.14 / 0.25 |
| `runtime.jit_polymorphic_calls_512` / 1 | 256.534 -> 255.337 | 664076.93 -> 661012.09 | -0.46% | 5.15 / 0.29 |
| `runtime.jit_polymorphic_calls_512` / 2 | 258.215 -> 257.736 | 668480.22 -> 667193.63 | -0.19% | 0.24 / 0.24 |
| `runtime.jit_unstable_calls_512` / 1 | 237.619 -> 302.422 | 615101.59 -> 782786.74 | +27.26% | 3.07 / 0.86 |
| `runtime.jit_unstable_calls_512` / 2 | 233.615 -> 234.687 | 604731.86 -> 607590.78 | +0.47% | 0.55 / 0.89 |

- Source inspection found a remaining unconditional call to the sampling
  handler after static, guarded and permanently generic calls, although that
  handler immediately returns for those states. The next revision makes that
  eligibility check at the caller. Its full workspace and Clippy checks pass
  in `call-feedback-entry-workspace-1.log` and
  `call-feedback-entry-clippy-1.log`; dedicated timing is still required.
  This is a concrete code revision, not another unchanged timing retry.

### Sampling-entry eligibility revision: still not accepted

- Static, guarded and permanently generic calls now skip the sampling handler
  at frame entry. The bounded sampler still runs only for eligible dynamic
  entries. This removes a known redundant call, but does not establish that
  the complete guarded-call feature is cheaper than the pre-feedback control.
- Frozen `call-feedback-candidate-3.exe` SHA256:
  `768efa0bed07f8f274936dfadaacfdef0acdb93eb6d52e7efaa873603d7d4975`;
  `call-feedback-candidate-source-3.zip` SHA256:
  `299978e23f20e368e10116b444da5918c7cc42c6fbb760bcf736507904009f57`.
  The control and byte-identical benchmark harness remain unchanged.
- The `call-feedback-v3-*` captures completed all 20 planned invocations.
  Each passes all 40 correctness checks and uses nine repeats, scale 32,
  `tinyone-workstation` and Balanced policy. Both opposite-order captures
  for each workload were run together. No build, allocation diagnostic or
  archive compression ran during timing.
- Eighteen invocations pass the evidence gates, forming eight complete pairs.
  Polymorphic control capture 1 fails at 10.60% CV; unstable candidate capture 1
  fails at 10.06% CV. Both incomplete pairs are excluded in their entirety;
  their eligible partners are not used as independent comparisons.
- Static calls regress in both pairs (+13.09%, +0.88%). Function-value calls
  change -1.18% and +12.67%; closures change -5.33% and +0.20%. Polymorphic
  and unstable workloads each have only one complete pair. Cross-capture
  level shifts remain unexplained. Neither the favorable subset nor the
  eligibility revision justifies accepting the complete feature.

| Workload / capture | Mean us, control -> candidate | Mean cycles, control -> candidate | Cycle change | CV %, control / candidate |
| --- | ---: | ---: | ---: | ---: |
| `jit.calls` / 1 | 34.094 -> 38.564 | 88274.21 -> 99826.43 | +13.09% | 0.40 / 0.22 |
| `jit.calls` / 2 | 33.862 -> 34.159 | 87649.23 -> 88424.28 | +0.88% | 0.26 / 0.26 |
| `runtime.jit_closure_calls_512` / 1 | 211.040 -> 199.767 | 546300.59 -> 517169.38 | -5.33% | 0.24 / 6.15 |
| `runtime.jit_closure_calls_512` / 2 | 213.845 -> 214.320 | 553549.43 -> 554673.04 | +0.20% | 0.29 / 0.21 |
| `runtime.jit_function_value_calls_512` / 1 | 192.342 -> 190.067 | 497933.00 -> 492081.77 | -1.18% | 7.61 / 4.48 |
| `runtime.jit_function_value_calls_512` / 2 | 182.274 -> 205.380 | 471883.70 -> 531656.82 | +12.67% | 0.39 / 6.48 |
| `runtime.jit_polymorphic_calls_512` / 2 | 311.974 -> 257.255 | 807601.85 -> 665974.22 | -17.54% | 0.26 / 0.23 |
| `runtime.jit_unstable_calls_512` / 2 | 234.331 -> 232.625 | 606572.67 -> 602199.68 | -0.72% | 9.30 / 1.02 |

- Full workspace tests and all-target Clippy pass in the entry logs named above.
  `call-feedback-entry-allocations-1.log` passes all 72 diagnostic cases.
  Every host allocation/deallocation/reallocation count, requested-byte total,
  size bucket, runtime codec/lock counter and complete Ralloc snapshot exactly
  matches `call-allocations-after-1.log`. This preserves the earlier transfer
  benefit, not a new call-feedback allocation or latency win.
- After timing, all 118 Rust files match the 182-file frozen source archive
  byte-for-byte. The installed-operation container and benchmark harness hashes
  also match their recorded identities. No container change followed this run.
- Section 24 remains experimental and is not accepted. The working-tree prototype
  still follows the existing quickening option; describing it as experimental
  is not an independent off switch. Its unresolved static/function-value cost
  and 40-byte program-header increase must be removed or justified by a concrete
  redesign before final retention. Do not repeat unchanged timing captures or
  represent passing correctness checks as performance acceptance.

## Call-feedback isolation and cold-profile ownership

- Previous goal turn: progress. It completed the third call-feedback capture
  set, verified all frozen Rust sources, and recorded non-acceptance. This turn
  addresses that unresolved default-enabled cost and normal header footprint;
  the full 40-section goal remains active.
- Re-read the complete objective, applicable Rust rules/examples and router,
  and rechecked the current source/docs. No additional repository agent file
  was found. Existing VM/Ralloc ownership and authority contracts are unchanged.
  Checked the official [Box ownership/layout documentation](https://doc.rust-lang.org/stable/std/boxed/index.html),
  [optional dereference API](https://doc.rust-lang.org/stable/std/option/enum.Option.html#method.as_deref_mut)
  and [fixed-size slice chunks](https://doc.rust-lang.org/stable/std/primitive.slice.html#method.as_chunks).
  No dependency, toolchain configuration, pool or production unsafe was added.
- Added the independent `JitOptions::with_call_feedback` opt-in, default false.
  Threshold zero still disables all adaptation, and scalar quickening stays
  enabled by its existing setting when call feedback is off. Fresh private
  chunks arm unseen call sites only when requested. Ordinary calls and permanent
  fallbacks now share `CallValue`; the duplicate generic opcode/request was
  replaced by explicit unseen state. No per-call option check or disabled-mode
  arming scan is needed, and back edges cannot rearm generic fallbacks.
- Moved detailed attribution into `Option<Box<JitExecutionProfile>>`.
  `ChunkExecution` borrows an optional profile reference without knowing its
  owner's container. The public reference-returning API is unchanged; cloning
  copies independent counters and resetting retains the box while releasing
  its map entries. Retained-byte accounting now charges the profile header only
  when present, in addition to its recorded map storage.
- Windows layout (`call-feedback-opt-in-layout-1.log`): `JitProgram` decreases
  344 -> 208 bytes, while `JitOp=24` and `JitChunk=88` stay unchanged.
  A detailed profile is 136 bytes and its optional owner is eight bytes.
  This removes 17,408 bytes of normal program headers across 128 entries;
  it is a layout/payload charge, not an RSS or whole-cache allocation claim.
- Expanded the isolated host diagnostic to 144 call cases: both call-feedback
  choices for the earlier 72-case matrix. Every host count, byte total, bucket,
  codec/lock counter and complete Ralloc snapshot matches the corresponding
  accepted argument-transfer record exactly in both modes.
  `call-feedback-opt-in-allocations-2.log` passes all output/cleanup checks.
  Its separate compile-only interval observes 10 -> 11 allocations and
  474 -> 610 requested bytes when detailed profiling is enabled, with equal
  deallocation/reallocation counts. The box therefore costs one allocation
  and 136 requested bytes only for explicit profiling.
- The first optional-profile diagnostic incorrectly included the shared token's
  one-time fingerprint allocation on only its first side. That assertion failed
  and is retained in `call-feedback-opt-in-allocations-1.log`. The corrected
  interval primes the token's memoized fingerprint before both compilations;
  the call matrix itself passed in both runs. No runtime workaround was made.
- Current Windows workspace and all-target Clippy pass in
  `call-feedback-opt-in-workspace-2.log` and
  `call-feedback-opt-in-clippy-3.log`: 193 core tests, six call-feedback,
  four call-transfer, 15 module-sandbox and 57 parity tests, plus remaining
  workspace suites. New regressions cover normal/lazy disabled call sites,
  independent scalar quickening, optional profile charges and independent
  clones. The first focused build missed a benchmark import; the first Clippy
  run requested `as_chunks` in a test. Both were corrected and rechecked.
- The harness now has 45 pre-timing checks, including five explicit enabled
  cases, and ten `call_feedback.off/on.*` rows. Each pair uses identical source,
  scalar quickening and iteration counts without detailed profiling. The
  binary is `call-feedback-opt-in-1.exe`, SHA256
  `6153403fd452a12697be00eb655d319aeccf3a2d31b8d137d64a946df1978cf1`;
  `call-feedback-opt-in-source-1.zip` hashes to
  `cdc3910501e5fafe7bb4ea917010cc8fc9432cabf81662fdeffb50580cf3b4ad`.
  Its harness hash is
  `32c25ac7afc3b75a46417c36464fed52cd7778c87a6cd70410fc0952822a1b3b`.
- All 20 Windows invocations passed 45 correctness checks with nine repeats,
  scale 32, `tinyone-workstation`, and unchanged Balanced policy. Captures are
  off/on then on/off for each workload. No builds, profiles or compression ran
  during timing. Nineteen invocations produce nine complete eligible pairs;
  unstable off capture 2 fails at 11.14% CV, excluding that entire pair.

| Workload / capture | Mean us, off -> on | Mean cycles, off -> on | Cycle change | CV %, off / on |
| --- | ---: | ---: | ---: | ---: |
| `closures_512` / 1 | 217.219 -> 205.145 | 562294.80 -> 530895.70 | -5.58% | 0.23 / 4.86 |
| `closures_512` / 2 | 206.233 -> 197.756 | 533894.94 -> 510738.11 | -4.34% | 5.75 / 4.67 |
| `function_values_512` / 1 | 195.860 -> 211.311 | 507003.52 -> 546989.94 | +7.89% | 6.99 / 0.41 |
| `function_values_512` / 2 | 212.551 -> 212.035 | 550216.28 -> 548865.60 | -0.25% | 0.24 / 0.19 |
| `polymorphic_512` / 1 | 257.712 -> 317.335 | 667046.15 -> 821457.70 | +23.15% | 0.23 / 0.19 |
| `polymorphic_512` / 2 | 264.903 -> 308.079 | 685779.47 -> 797455.39 | +16.28% | 7.37 / 0.29 |
| `static_calls` / 1 | 35.284 -> 34.233 | 91346.80 -> 88616.21 | -2.99% | 0.24 / 0.25 |
| `static_calls` / 2 | 34.472 -> 34.226 | 89241.84 -> 88605.60 | -0.71% | 0.15 / 0.38 |
| `unstable_512` / 1 | 290.926 -> 237.843 | 753097.28 -> 615678.00 | -18.25% | 0.33 / 6.86 |

- These are same-executable option comparisons, not before/after acceptance
  against the older control. Closure pairs improve 4.34-5.58%, function values
  conflict, and polymorphic calls show large regressions. The feature remains
  opt-in and unaccepted; no favorable subset is used to enable it by default.
- All 118 Rust files matched the 182-file frozen archive after timing. A
  subsequent benchmark-unit regression, not part of that frozen executable,
  checks the exact three-run warm-up used by the harness. Static, polymorphic
  and unstable rows then have identical complete listings and execution
  profiles in off/on modes, with zero remaining call samples/guard updates.
  Dispatches are respectively 1,478 / 11,271 / 11,099
  (`call-feedback-opt-in-warm-traces-1.log`). Thus those timing shifts are not
  explained by extra JitOp dispatches or ongoing call-feedback work; their cause
  remains unestablished. The next check uses independent hardware-counter
  attribution, not another unchanged Windows latency retry.
- WSL Arch provides independent hardware-counter attribution. Plain sandboxed
  WSL inspection first returned `Wsl/E_ACCESSDENIED`; approved scoped access
  worked. The project-default stable resolution also reported unavailable
  requested components. No installation or toolchain configuration was changed:
  commands explicitly use the already-installed
  `RUSTUP_TOOLCHAIN=nightly`, rustc 1.99.0-nightly
  (`7608eb7b0 2026-08-05`), Cargo 1.99.0-nightly
  (`7c83d4cc0 2026-07-29`), and perf 7.1.8-1.
- The offline, locked normal Linux release build and full hooks workspace pass
  in `call-feedback-opt-in-linux-build-1.log` and
  `call-feedback-opt-in-linux-workspace-1.log`. Linux Cargo artifacts have a
  separate `target/linux` directory. The frozen ELF
  `call-feedback-opt-in-linux-1` hashes to
  `eeddefb9965ba13e5b2851b339773436f5b799edcfb2b2783ed3ad48ca266855`;
  `call-feedback-opt-in-linux-source-1.zip` hashes to
  `acc07bd2cd9f80a1de090785921a155d9d8a535bd367a6a708db601fa1e97261`.
  This source includes the later warm-trace test; its harness hash is
  `65794d0515e2c0ad34b2e370bffb195884be81b25e7767f7b787209c10ab81ac`.
  All 118 Rust files matched this 182-file archive by byte SHA256 after timing.
- Twenty perf-wrapped Linux invocations use the same five off/on fixtures,
  off/on then on/off order, nine repeats, scale 32, and unchanged Balanced policy.
  Every invocation passes all 45 correctness checks and the harness's metadata/CV
  gate. All 80 event records are finite, positive, supported, and report 100%
  running time. No build, allocation profile or compression overlapped timing.
  Raw `call-feedback-opt-in-linux-{off,on}-{workload}-{1,2}` logs, baseline
  JSON and `.perf.jsonl` records are retained under `target/perf/full-system`.
- The counter prefix is
  `perf stat --no-inherit --json-output -e instructions:u,cycles:u,branches:u,branch-misses:u`.
  These are user-space main-task counters, excluding child tasks but including
  harness startup, correctness, setup and output as well as repeated execution.
  They are not isolated per-language-operation counts. Timing means below were
  captured under perf and are attribution evidence, not new uninstrumented
  latency-acceptance pairs. Counter-change columns compare complete on/off
  main-task totals; the raw files retain their absolute cycle/branch/miss counts.

| Workload / capture | Mean us, off -> on | Retired instructions, off -> on | Instructions | User cycles | Branches | Branch misses |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| `closures_512` / 1 | 171.529 -> 167.421 | 43652263632 -> 43549714764 | -0.23492% | -2.35% | -0.53133% | +19.83% |
| `closures_512` / 2 | 169.824 -> 171.566 | 43652560598 -> 43549101926 | -0.23700% | +0.77% | -0.53529% | +143.27% |
| `function_values_512` / 1 | 161.403 -> 161.710 | 42553152215 -> 42449876376 | -0.24270% | +0.09% | -0.57640% | +33.23% |
| `function_values_512` / 2 | 164.514 -> 161.050 | 42553100981 -> 42449933063 | -0.24245% | -1.97% | -0.57589% | -57.60% |
| `polymorphic_512` / 1 | 227.928 -> 228.991 | 58062081099 -> 58061953664 | -0.00022% | -0.09% | -0.00037% | -2.36% |
| `polymorphic_512` / 2 | 226.325 -> 230.396 | 58062005353 -> 58061989199 | -0.00003% | +1.74% | -0.00004% | +0.57% |
| `static_calls` / 1 | 31.060 -> 31.781 | 49332184270 -> 49332197782 | +0.00003% | +2.55% | +0.00006% | -3.80% |
| `static_calls` / 2 | 30.615 -> 30.297 | 49332163311 -> 49332181101 | +0.00004% | -1.16% | +0.00007% | +40.17% |
| `unstable_512` / 1 | 209.412 -> 221.281 | 55369931100 -> 55369957560 | +0.00005% | +5.12% | +0.00006% | +30.51% |
| `unstable_512` / 2 | 209.122 -> 209.584 | 55369914735 -> 55370005569 | +0.00016% | -0.09% | +0.00027% | +2.42% |

- Stable function-value and closure fixtures consistently retire only
  0.235-0.243% fewer instructions and 0.531-0.576% fewer branches with feedback
  enabled. User-cycle direction changes across the reversed captures in both
  cases; branch-miss direction/magnitude is also not stable across the set.
  This supports a small instruction-work reduction, not a general latency win.
- Static, polymorphic and unstable main-task instruction counts differ by less
  than 0.00022% and branch counts by less than 0.00038%. Together with the exact
  warmed JitOp/profile equality, these runs do not attribute the earlier large
  Windows timing shifts to ongoing feedback work. Hardware-cycle and miss
  variation does not establish a specific OS, frequency or predictor cause.
  Keep the prototype opt-in and section-24 latency acceptance open; do not use
  these Linux results to erase or accept the earlier Windows regressions.
- Final Windows hooks workspace and all-target Clippy pass after the new
  warm-trace regression in `call-feedback-opt-in-workspace-3.log` and
  `call-feedback-opt-in-clippy-4.log`. Both platforms include 193 core tests,
  15 benchmark tests, six call-feedback, four call-transfer, 15 module-sandbox
  and 57 parity tests, plus the remaining TinyOne/Ralloc suites.
  Changed/new Rust formatting and `git diff --check` pass.


## Scalar-loop cost attribution and operand extraction

- Previous goal turn: progress. It completed the call-feedback hardware
  attribution, final checks and evidence ledger. This turn returns to the
  outstanding hot-loop/float/map work; the full 40-section goal stays active.
  Re-read the complete objective, global Rust router/rules and all four
  examples, repository router, architecture, VM, bytecode, memory, performance,
  contribution and CI documents. No additional repository agent file exists.
  The existing Ralloc gate remains applicable; its instrumentation/reset API
  and TinyOne's process-wide codec/lock counter API were rechecked.
- Extended the isolated runtime diagnostic from 52 to 60 cases, adding the
  nested multiply/add loop and changing-operand loop used by the timing harness.
  All 15 sources run against the VM at thresholds 0/8, first/repeated execution.
  One reset now covers both runtime and Ralloc counters. Reporting happens after
  snapshots and report cleanup; compilation is outside the interval.
  Runtime growth/copy counts must agree with the complete Ralloc snapshot.
- `scalar-runtime-profile-1.log` passes all 60 cases. The earlier 52 complete
  heap, Ralloc, opcode/stack/adaptation and JIT-stat blocks match
  `call-transfer-runtime-profile-1.log` exactly after removing the five new,
  uniformly zero call-feedback counters. Codec/lock counts are newly recorded
  here, not retroactively claimed as present in the earlier 52-case logs.
  Selected repeated-execution counts follow; these are diagnostic work counts,
  not timing, RSS or allocator-overhead estimates.

| Workload | Threshold | Dispatches | Stack pushes / pops | Encodes | Decodes | Ralloc allocations / requested bytes |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| `integer_loop` | 0 | 16390 | 1 / 1 | 4099 | 12290 | 1 / 128 |
| `integer_loop` | 8 | 16390 | 1 / 1 | 4099 | 8193 | 1 / 128 |
| `nested_integer_loop` | 0 | 28678 | 12289 / 12289 | 4099 | 12290 | 1 / 128 |
| `nested_integer_loop` | 8 | 28678 | 12289 / 12289 | 4099 | 4097 | 1 / 128 |
| `float_loop` | 0 | 28679 | 12290 / 12290 | 4099 | 8194 | 1 / 128 |
| `float_loop` | 8 | 28679 | 12290 / 12290 | 4099 | 4097 | 1 / 128 |
| `maps` | 0 | 2058 | 2 / 2 | 1029 | 2051 | 5 / 43840 |
| `maps` | 8 | 2058 | 2 / 2 | 1029 | 1537 | 5 / 43840 |
| `changing_operand_loop` | 0 | 28679 | 2049 / 2049 | 8196 | 16386 | 1 / 192 |
| `changing_operand_loop` | 8 | 28679 | 2049 / 2049 | 8196 | 8193 | 1 / 192 |

- These scalar runs have one Ralloc allocation, three heap-lock acquisitions
  and no growth/copies. The nested and float fixtures each still execute roughly
  12,000 physical stack pushes and pops, while their warmed feedback sampling
  has already stopped. In the nested fixture, 4,096 pair extractions accompany
  4,096 `add.int` dispatches. The changing-operand fixture uses the generic
  `binary.slots.store` family without those intermediate operands.
- Read the official [Cargo profile documentation](https://doc.rust-lang.org/cargo/reference/profiles.html)
  and [environment overrides](https://doc.rust-lang.org/cargo/reference/environment-variables.html).
  Built a separate offline/locked Linux release executable with
  `CARGO_PROFILE_RELEASE_DEBUG=line-tables-only`,
  `CARGO_PROFILE_RELEASE_STRIP=none`, and `CARGO_TARGET_DIR=target/linux-profile`
  (absolute mounted-workspace path in the actual command).
  The existing nightly is selected per command; release optimization settings,
  normal build configuration, allocator features and dependencies are unchanged.
- The symbol-enabled ELF `scalar-symbol-1` hashes to
  `19e654a86c65bec7a20f921c3f311ddf69c68e5be1b7164728edfb89f4598785`.
  Its 182-file `scalar-symbol-source-1.zip` hashes to
  `ab39412939cfdaf152b053d48c80fc7921b27d90f744840786f67fd986772693`.
  Harness SHA256 is
  `65794d0515e2c0ad34b2e370bffb195884be81b25e7767f7b787209c10ab81ac`;
  expanded diagnostic SHA256 is
  `afd5f813d096ddcd9c6f344b0c73ce03640cbe95392eebab9dddd2a338a35d72`.
- Direct `perf record` output on the mounted Windows filesystem failed with
  `Bad address`, even in a minimal probe without call chains. The first
  workload had passed all 45 correctness checks but produced no timing baseline;
  that failed trace/log remains diagnostic only. A separately created native
  temporary directory was absent on the next inspection; no specific lifetime
  cause is asserted. A direct native `/tmp` output probe succeeded. Each actual
  trace was therefore recorded to a task-specific native file and immediately
  exported to the workspace in the same command sequence.
- Five captures use
  `perf record --no-inherit --no-buildid-cache -e cycles:u -F 499 --call-graph dwarf,4096`.
  They observe the benchmark main task, not unrelated processes or child tasks.
  No build or other profile overlaps sampling. All five complete normally,
  export successfully, pass 45 correctness checks and their harness CV gates,
  and report zero lost samples. Traces, timing JSON, capture logs and symbol
  reports are `scalar-symbol-{workload}-2.*` under `target/perf/full-system`.
  The reports disable cumulative child costs; their percentages are sampled
  self-cycle attribution, not exact call overhead or before/after latency wins.
  `run_frame` also contains inlined semantic operations; its entire share must
  not be described as pure dispatch overhead.

| Workload | `run_frame` self | Pair extraction self | Other notable self costs |
| --- | ---: | ---: | --- |
| Quickened nested I64 | 62.27% | 11.15% | Slot store 9.86%; checked addition 5.71% |
| Nonadaptive nested I64 | 45.92% | 7.59% | Encoding 8.47%; integer conversion 7.22% |
| Stable float | 58.45% | 11.66% | Encoding 9.60%; generic addition 7.85%; slot store 7.83% |
| Changing operand | 57.10% | below 0.5% display cutoff | Slot store 14.16%; encoding 11.81% |
| Map set/get | 23.22% | below 0.5% display cutoff | Hasher write 11.57%; key hash 11.11%; map set/get 6.54% / 6.21% |

- The symbol-enabled timings are not acceptance pairs. The sampled extraction
  cost motivates a minimal dispatch-owner experiment: an `#[inline]` hint on
  `jit_pop_pair`, leaving its two ordered owning pops unchanged. The
  [Rust inline-attribute contract](https://doc.rust-lang.org/stable/reference/attributes/codegen.html#the-inline-attribute)
  is a compiler hint, not a guarantee, and code-size/guardrail effects need
  measurement. No new opcode, representation, unchecked access, allocator API
  or public behavior is introduced.
- A focused ownership regression passes
  (`scalar-inline-operands-1.log`): differently tagged pointer operands retain
  their original long-string allocations and order; the stack prefix, backing
  address and capacity remain unchanged. Existing one/zero-operand underflow
  ordering is also preserved. These pointers are moved, never dereferenced.
- Normal control builds are verified separately from the symbol-enabled
  profiler. `scalar-inline-control-1.exe` hashes to
  `f49c6eeb38c20e5127739fc3fd1a23dd5966e7dc7d3dc0def519ea7cf30a72dc`;
  `scalar-inline-linux-control-1` hashes to
  `eeddefb9965ba13e5b2851b339773436f5b799edcfb2b2783ed3ad48ca266855`.
  Their `scalar-inline-control-source-1.zip` is the same
  `ab39412939cfdaf152b053d48c80fc7921b27d90f744840786f67fd986772693`
  source snapshot. Candidate measurement follows below.
- Candidate Windows/Linux workspace checks, Windows all-target Clippy, and
  changed/new Rust formatting pass before timing. The core suite has 194
  ordinary tests plus seven ignored storage diagnostics. The second isolated
  `scalar-runtime-profile-2.log` passes and all 60 complete profile blocks,
  including the newly added codec/lock counters, match the control exactly.
- The normal candidates are `scalar-inline-candidate-1.exe`, SHA256
  `6af109ac59ae6cdc0d91f11b9bf442498b4744de01952a36ccf2ab21f7e07a6e`,
  and `scalar-inline-linux-candidate-1`, SHA256
  `8500f5e90d6e568eea0abe8fd942210bde21616cd350347938cc15f5e721fff5`.
  `scalar-inline-candidate-source-1.zip` hashes to
  `2f295a7f871a27e7e198e4e3a6ae6a43ef8a778b3966f0cc2c7c5b4e40342c88`.
  All 118 current Rust files matched that archive after timing. The only Rust
  file changed between control/candidate archives is `jit/vm.rs`: the two-line
  hint/comment and the new test; no other production change is mixed in.
- Linux `.text` grows 1,807,102 -> 1,807,726 bytes (+624), and its ELF file grows
  2,338,320 -> 2,339,056 bytes (+736). Both Windows PE files are 2,420,224 bytes.
  After checking the [Microsoft PE section format](https://learn.microsoft.com/en-us/windows/win32/debug/pe-format),
  read-only byte hashing confirms identical Windows `.text` sections:
  1,906,688 raw bytes including padding, SHA256
  `a8d116f067f5971eef81f19e09fb4fc2f07922e3e61d7944d6297ed8230cd1a8`.
  `.data`, `.pdata` and `.reloc` also match; `.rdata` differs, so the
  complete executables are not claimed byte-identical. Windows workspace tests
  pass, but no Windows instruction-change or latency-win claim is made, and
  identical code was not put through another noisy timing comparison.
- All 28 uninstrumented Linux invocations pass all 45 pre-timing correctness
  checks and save eligible baselines: seven workloads, nine repeats, scale 16,
  control/candidate then candidate/control order, unchanged Balanced policy.
  No Cargo, diagnostics, hashing or compression overlaps timing.
  The `scalar-inline-linux-{control,candidate}-{workload}-{1,2}.json/.log`
  files retain all 14 complete pairs. Linux thread CPU time is primary; the
  built-in TSC counts also include descheduling and are not hardware core cycles.

| Workload / capture | Mean thread CPU us, control -> hint | CPU change | Mean TSC change | CV %, control / hint |
| --- | ---: | ---: | ---: | ---: |
| `calls` / 1 | 30.139 -> 30.132 | -0.02% | -0.03% | 0.65 / 2.54 |
| `calls` / 2 | 29.564 -> 29.530 | -0.11% | +0.01% | 0.56 / 0.52 |
| `cold_lowering` / 1 | 1.155 -> 1.161 | +0.47% | +0.47% | 0.95 / 1.03 |
| `cold_lowering` / 2 | 1.143 -> 1.145 | +0.14% | +0.20% | 0.96 / 1.95 |
| `float` / 1 | 381.173 -> 384.748 | +0.94% | +0.84% | 0.82 / 0.91 |
| `float` / 2 | 396.300 -> 384.124 | -3.07% | -3.08% | 3.70 / 3.45 |
| `hot_generic` / 1 | 571.683 -> 564.318 | -1.29% | -1.33% | 1.41 / 0.97 |
| `hot_generic` / 2 | 558.359 -> 574.306 | +2.86% | +2.86% | 0.22 / 0.21 |
| `hot_quickened` / 1 | 380.103 -> 378.780 | -0.35% | -0.31% | 1.31 / 0.27 |
| `hot_quickened` / 2 | 374.396 -> 373.515 | -0.24% | -0.22% | 0.52 / 2.94 |
| `maps` / 1 | 83.733 -> 84.623 | +1.06% | +1.06% | 1.27 / 1.73 |
| `maps` / 2 | 82.773 -> 83.265 | +0.59% | +0.59% | 0.29 / 0.81 |
| `mixed` / 1 | 334.755 -> 331.973 | -0.83% | -0.84% | 0.65 / 0.31 |
| `mixed` / 2 | 331.906 -> 336.664 | +1.43% | +1.43% | 0.42 / 1.65 |

- Decision: reject and remove the inlining hint. The small quickened-loop mean
  changes (CPU -0.24% to -0.35%, TSC -0.22% to -0.31%) do not establish the
  targeted extraction win; capture-1 best wall time actually rises
  376.165 -> 377.721 us. Float, nonadaptive and changing-type results reverse
  direction; maps and cold lowering are slightly worse in both captures.
  The aggregate evidence does not justify retaining the hint. No speedup is
  attributed to it, and no unchanged retry is used to seek a favorable result.
- Retain the expanded 60-case attribution and ordered-ownership regression.
  The next implementation target is the measured physical stack/dispatch work,
  especially the unfused slot-plus-float-literal store. Reuse the existing local
  scalar-expression and shared numeric-semantic architecture; do not make an
  F64 type assumption from loop hotness or replace growable storage containers
  to address this cost. Exact float bits, type/error behavior, snapshot ordering,
  block entries, code layout, cold lowering and unrelated runtime guardrails
  remain mandatory. Wider hot-loop/map/feedback acceptance stays open.
- After removing the hint, the complete production portion of `jit/vm.rs`
  matches the control source text; only the ownership regression is added to
  that file. Its retained byte SHA256 is
  `c9907beffcd7b690fe7d11c7f84911f8ddf6e05a177cc8c1543666f4e693cd9f`.
  Current Windows and Linux hooks workspaces both pass, including 194 core
  tests, 15 benchmark tests and all TinyOne/Ralloc integration suites
  (`scalar-attribution-retained-workspace-1.log` and
  `scalar-attribution-retained-linux-workspace-1.log`). Windows all-target
  Clippy passes (`scalar-attribution-retained-clippy-1.log`), as do changed/new
  Rust formatting and `git diff --check`.
- Both normal release outputs are rebuilt from the retained source, so direct
  benchmark commands no longer run the discarded candidate. Windows hashes to
  `16e8c454878153ecad41bad03fbab723a54faa7130ce10b7604c52575fb5731f`;
  Linux is byte-identical to the original normal control, SHA256
  `eeddefb9965ba13e5b2851b339773436f5b799edcfb2b2783ed3ad48ca266855`.
  Build logs are `scalar-attribution-retained-build-1.log` and
  `scalar-attribution-retained-linux-build-1.log`. The performance workflow now
  documents the expanded diagnostic and uses the manifest's actual
  `tinylang_bench` binary name. This checkpoint adds evidence and coverage;
  it does not claim a retained runtime speedup or complete the optimization plan.




## Float-immediate semantic stores — initial measurements

This checkpoint left Windows acceptance open. The subsequent scheduling
isolation below supplies the controlled Windows comparison; the original
unrestricted failures and their limitations remain recorded here.

- Continued the full batch from the measured scalar-stack cost. This turn adds
  one parameterized `BinarySlotFloatStore(ScalarOp, SlotId, u64, SlotId)`
  family in the existing final encoder, covering all ten scalar modes.
  It replaces a slot read, Fp64 literal push, arithmetic/comparison and store
  with one operation when local version/effect/block facts permit it.
  It is not a new portable opcode, float constant folder, unchecked encoded
  memory path, adaptive F64 specialization or native backend.
- The immediate retains exact literal bits. Runtime loads the current source,
  constructs the same Fp64 operand as `PushFloat`, calls existing
  `ScalarOp::execute` and stores only after successful arithmetic. Source and
  destination may alias. Numeric promotion, float-kind rounding, comparison
  booleans and exact runtime errors stay in shared semantics. No branch/effect
  may enter the fused expression, and live stack prefixes remain untouched.
  Fixed metadata, bounded stacks/pools and the private slice-only instruction
  Vec policy are unchanged. `JitOp` remains 24 bytes.
- Re-read the complete objective, global `C:/Users/conne/.codex/AGENTS.md`,
  Rust rules and all four examples, and the documentation Router
  `docs/INDEX.md`. No repository/child or ancestor AGENT/AGENTS file was found.
  Relevant required/supplemental reading included architecture, VM, bytecode,
  memory model, performance, CONTRIBUTING, CI_CD, numeric types/expressions,
  numeric builtin documentation, v2 roadmap and community guidance.
  Ralloc 0.2.0's library, owned-buffer and VM-allocation API/safety documentation
  and TinyMemory's load/store/ownership implementation were checked; no
  allocator internals, allocation APIs, unsafe code or dependencies changed.
  Ralloc's referenced `docs/clippy_doc_lints.md` is absent at both repository
  and crate locations. Checked the [stable Rust documentation](https://doc.rust-lang.org/stable/)
  and [f64 bit-conversion contract](https://doc.rust-lang.org/stable/std/primitive.f64.html#method.from_bits).
  Literal-bit preservation is distinct from Rust's non-portable arithmetic NaN
  payload choices; parity tests compare current backends on each tested target.
  Older syntax docs' integer-only arithmetic/comparison wording remains a
  final documentation-cleanup item, not the runtime contract used here.
- Added four symbolic-lowering regressions: 1,440 arithmetic/comparison matrix
  fixtures (eight source values, nine immediate bit patterns, ten modes and
  aliased/distinct destinations); all twelve numeric kinds and changing-kind
  loops; non-empty block entries, interior boundaries and overwritten-source
  snapshots; and index 65,535 as source and/or destination. Existing parity
  infrastructure compares memory signatures, output and exact errors at
  thresholds 0/1/8 with two executions each. A private encoding unit checks
  literal payloads, maximum typed slots, zero stack traffic and no type sampling.
  The new block test failed on the unfused control as intended
  (`float-store-preimplementation-test-1.log`). An initially invalid
  `i8(...)` source fixture was corrected to the documented
  `check_int_range(..., "i8")` API; the failure is preserved in
  `float-store-symbolic-test-1.log`.
- Windows and Linux full hooks workspaces pass: 195 core tests, 15 benchmark
  tests, 17 symbolic-lowering tests and all remaining TinyOne/Ralloc suites
  (`float-store-workspace-1.log`, `float-store-linux-workspace-1.log`).
  Windows all-target hooks Clippy passes (`float-store-clippy-1.log`).
  All 17 symbolic tests also pass in the ordinary Windows release build
  (`float-store-release-symbolic-1.log`). Changed/new Rust formatting and
  whitespace checks pass. This does not claim a new full Miri/sanitizer run.
- Added `jit.codegen_float_loop_cold` before freezing either side. It compiles
  the existing float-loop source outside timing, then measures fresh JitCache
  construction/lowering/destruction from verified input, without loop execution.
  Its benchmark-presence regression passes; the 45 pre-timing correctness
  cases are unchanged. Harness byte SHA256:
  `49ce4b3b533e3a6145441d92ed713d55282d2e16a21fc59efb12ab2ee6ad3d63`.

### Runtime work and allocation attribution

`float-store-runtime-profile-1.log` passes all 60 cases. Compared with
`scalar-runtime-profile-1.log`, every complete heap, Ralloc and runtime
codec/lock record is identical. All 52 cases outside the two sources below
also have identical complete execution/JIT records. Cold/warm and threshold
0/8 all agree on these work reductions:

| Source | Installed ops | Dispatches | Pushes / pops, each | Observed peak stack |
| --- | --- | --- | --- | --- |
| float_loop | 13 → 10 | 28,679 → 16,391 | 12,290 → 2 | 2 → 1 |
| mixed_types_branches | 15 → 12 | 7,174 → 5,638 | 1,537 → 1 | 2 → 1 |

The first float run at threshold 8 samples eight observations instead of
16; the mixed-branch fixture samples 16 instead of 48. Successful integer-guard
counts and promotions are unchanged. Unsupported float binary sampling is no
longer needed for the fused operation. Warm feedback samples remain zero.
Verifier-sized cached operand capacity stays at two values; these are
dispatch/stack reductions, not a claim of fewer value codecs or Ralloc calls.
The 512-iteration `mixed_types_branches` diagnostic is not the separate
4,096-iteration changing-operand benchmark, whose complete profile is unchanged.

### Frozen comparison artifacts

All artifacts below are under `target/perf/full-system`. Both normal release
builds finished before copying; timing uses no testing-hooks or symbol profile.
Each source archive contains 143 Rust/Cargo/configuration files, including
119 Rust files. Raw-byte comparison finds only `jit/encoding.rs`,
`jit/op.rs`, `jit/vm.rs` and `tests/symbolic_lowering.rs` different between
control and candidate. All 143 current files still match the candidate archive
after every capture. No Cargo build, Rust edit, allocation profile, hashing or
compression overlapped timings or hardware-counter capture.

| Artifact | SHA256 |
| --- | --- |
| float-store-control-1.exe | `582cb824486153cd76e68bd83589ca774b71c85916963287bea4ced5eda86838` |
| float-store-candidate-1.exe | `b7a994de963df8db8fa8cffa33b722520331587d50c996b9d08ba9567a3658eb` |
| float-store-linux-control-1 | `08bb71eff3978805d6ecbdcd61dfe7c93b15e7774d8a03100d4d3ab3c4ac00f8` |
| float-store-linux-candidate-1 | `504f112c353bfa3f9d68e9a4f6c76421276a75f219f8fc3ac0efca7221b4c5e5` |
| float-store-control-source-1.zip | `4d299103678b4365e6b697ed991c1d14ba630b70fa89dd12e91f5699fa254f61` |
| float-store-candidate-source-1.zip | `6463a80e535a0c0c626f692b6df650f2c938af58333daa592a8f644c396f7a9b` |

Complete Windows executable size is 2,420,736 → 2,421,760 bytes (+1,024);
Linux ELF size is 2,339,072 → 2,339,200 bytes (+128). These are whole-file
sizes, not isolated code-section or live-memory sizes. Logical JitOp extent
shrinks for affected chunks without increasing the 24-byte enum or adding
retained side tables.

### Linux timing and hardware confirmation

All 28 uninstrumented invocations pass all 45 correctness checks and evidence
gates, producing 14 complete pairs. Capture 1 is control → candidate; capture 2
reverses that order. Both sides use nine repeats, scale 32, machine
`tinyone-wsl-arch`, and unchanged Windows Balanced policy.
Files: `float-store-linux-{control,candidate}-{label}-{1,2}.{json,log}`.
Thread CPU time is primary; TSC includes descheduling.

| Workload / capture | Mean thread CPU µs, control → candidate | CPU change | TSC change | Best wall change | CV %, control / candidate |
| --- | --- | --- | --- | --- | --- |
| calls / 1 | 29.635 → 29.831 | 0.66% | 0.64% | 0.34% | 0.24 / 0.64 |
| calls / 2 | 30.144 → 29.346 | -2.65% | -2.67% | -2.75% | 1.46 / 0.99 |
| float / 1 | 386.414 → 251.163 | -35.00% | -34.99% | -34.43% | 3.71 / 1.48 |
| float / 2 | 387.127 → 218.597 | -43.53% | -43.49% | -42.65% | 1.48 / 0.37 |
| float_cold / 1 | 0.879 → 0.816 | -7.18% | -7.33% | -7.81% | 1.00 / 2.66 |
| float_cold / 2 | 0.898 → 0.832 | -7.37% | -7.36% | -8.20% | 0.95 / 2.87 |
| hot / 1 | 373.275 → 369.879 | -0.91% | -0.96% | -0.39% | 0.76 / 0.30 |
| hot / 2 | 381.789 → 378.813 | -0.78% | -0.77% | -2.16% | 0.48 / 2.28 |
| maps / 1 | 82.741 → 84.176 | 1.73% | 1.74% | 1.52% | 0.30 / 0.79 |
| maps / 2 | 83.426 → 82.315 | -1.33% | -1.31% | -1.81% | 0.14 / 0.40 |
| mixed / 1 | 367.455 → 335.265 | -8.76% | -8.80% | -9.08% | 0.48 / 1.05 |
| mixed / 2 | 338.833 → 332.499 | -1.87% | -1.88% | -2.45% | 0.41 / 0.77 |
| straight_cold / 1 | 1.159 → 1.154 | -0.44% | -0.44% | -0.73% | 0.53 / 0.81 |
| straight_cold / 2 | 1.149 → 1.111 | -3.33% | -3.33% | -3.05% | 0.98 / 0.52 |

The paired float target improves 35.00% / 43.53% in mean thread CPU, while cold
float lowering improves 7.18% / 7.37%. Call/map guardrails change direction
between captures, so no general gain is claimed there. The unchanged
changing-operand profile also prevents crediting its whole timing difference
to this new instruction. Straight-line cold lowering and integer hot loops
show no repeated adverse result.

The initial 28 Linux JSON files honestly record runtime Rust-probe
`"unknown"`: those benchmark invocations did not set `RUSTUP_TOOLCHAIN`,
whereas both frozen builds explicitly selected installed nightly. Their
build commands/logs and source/binary identities are preserved, not silently
replaced. `float-store-linux-toolchain-1.log` separately records rustc
1.99.0-nightly (7608eb7b0 2026-08-05), LLVM 23.1.0 and kernel
6.18.33.2-microsoft-standard-WSL2. Subsequent counter captures explicitly set
that same toolchain for the runtime metadata probe and record it correctly.

Four separate, reversed-order `perf stat --no-inherit` invocations measure
only the benchmark task's user-mode cycles, retired instructions, branches
and branch misses. All four pass the 45 correctness checks and timing gates;
all 16 counter readings ran 100% of their intervals. Installed `perf stat -h`
was checked; no system-wide/other-process profiling or machine setting changed.
Files: `float-store-linux-perf-{control,candidate}-{1,2}.{stat,json,log}`.

| Capture | Hardware cycles, control → candidate | Instructions, control → candidate | Branches, control → candidate |
| --- | --- | --- | --- |
| 1 | 43,003,436,527 → 26,285,523,161 (-38.88%) | 96,643,039,115 → 65,849,881,162 (-31.86%) | 11,723,805,351 → 9,600,185,353 (-18.11%) |
| 2 | 42,792,330,895 → 24,714,496,992 (-42.25%) | 96,643,009,496 → 65,849,865,970 (-31.86%) | 11,723,799,049 → 9,600,180,289 (-18.11%) |

Branch misses are 3,779,757 → 1,837,557 and 2,143,302 → 1,869,380; their
varying magnitude is not a separate optimization claim. Whole-task counters
include setup, correctness checks, warmup and reporting, not just one language
execution. They independently support less CPU work in the float workload.

### Windows limitations and acceptance boundary

Forty Windows invocations also pass every one of the 45 correctness checks.
Captures 1/2 cover the same seven workloads at scale 32. A single longer-sample
follow-up covers float, float cold lowering and calls at scale 128 in captures
3/4. All use nine repeats, rustc 1.98.0 (88d9e12ae 2026-08-18), machine
`tinyone-workstation`, unchanged Balanced policy and reversed orders.

Only 19 runs save eligible JSON; 21 fail CV gates. Exactly five complete
eligible pairs remain, listed below. Orphaned eligible runs are not compared
across captures or sample scales.

| Workload / capture | Scale | Mean scheduled cycles, control → candidate | Cycle change | Best wall change | CV %, control / candidate |
| --- | --- | --- | --- | --- | --- |
| calls / 2 | 32 | 91705.7 → 107758.9 | 17.51% | 22.05% | 7.77 / 0.25 |
| calls / 3 | 128 | 91094.6 → 91775.3 | 0.75% | -0.51% | 4.91 / 6.64 |
| maps / 1 | 32 | 288099.6 → 265271.1 | -7.92% | -0.41% | 5.15 / 7.08 |
| maps / 2 | 32 | 258913.9 → 257423.8 | -0.58% | 0.02% | 6.11 / 5.62 |
| straight_cold / 2 | 32 | 8188.1 → 8081.0 | -1.31% | -0.97% | 0.58 / 0.18 |

There is **no complete Windows float target or float-cold pair**. Candidate
float CVs are 10.44%, 10.04%, 11.17% and 10.50%, all above the unchanged 10%
limit. Other failures include all four integer-hot captures, all four
changing-operand captures, cold-float control 1/3/4 and candidate 2/3/4,
call control 1 and candidate 4, and straight-cold candidate 1.
Their logs remain evidence; the narrow 10.04% miss is not rounded into a pass.
The eligible call capture 2 worsens mean scheduled cycles by 17.51%, whereas
the longer capture 3 changes them by +0.75% with slightly better best wall
time. This does not prove a stable call regression or clear it. Unpaired cold
candidate observations likewise vary; neither a cold win nor a cold regression
is accepted from them. No Windows speedup or cross-platform acceptance is claimed.

The working-tree fusion now has Linux target/cold/hardware evidence, broad
semantic coverage and unchanged allocator work. It remains under evaluation
for Windows latency; the complete optimization batch is still active.
Next resolve that platform's variance/outlier cause with an explicitly changed,
bounded diagnostic (for example per-sample evidence or benchmark-only scheduling
attribution), not another unchanged repetition or a relaxed quality gate.
Do not change machine-wide power/security settings to obtain a result.
The remaining all-system ledger, including exact runtime source mappings,
broader feedback/cache acceptance and final cleanup/report, is not closed by
this one instruction family.

## Windows scheduling isolation — float-store latency acceptance

Continued the complete optimization batch with a changed diagnostic for the
previous Windows variance, without editing Rust, rebuilding either side or
repeating the container comparison. Read the benchmark timer / sample / quality
gate implementation and current performance documentation. The harness retains
aggregate wall, scheduled-cycle and accepted CPU-time samples; it does not
retain individual observations or processor migrations. In particular, CPU-time
samples that fail its elapsed-time plausibility check are omitted, so their
mean can describe a different subset from the wall-time mean. Neither the
coarse CPU-time result nor an inferred clock frequency is used as primary
evidence below.

Consulted the Windows / .NET contracts for
[processor affinity](https://learn.microsoft.com/en-us/dotnet/api/system.diagnostics.process.processoraffinity?view=netframework-4.8.1),
[child affinity inheritance](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-setprocessaffinitymask),
[scheduled thread cycles](https://learn.microsoft.com/en-us/windows/win32/api/realtimeapiset/nf-realtimeapiset-querythreadcycletime),
[PowerShell 5.1 process launch](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.management/start-process?view=powershell-5.1),
[direct executable launch](https://learn.microsoft.com/en-us/dotnet/api/system.diagnostics.processstartinfo.useshellexecute?view=netframework-4.8.1)
and [redirected output](https://learn.microsoft.com/en-us/dotnet/api/system.diagnostics.processstartinfo.redirectstandardoutput?view=netframework-4.8.1).
Affinity restricts the logical processors available to a process; it does not
reserve a physical core, remove sibling-thread contention or fix CPU frequency.
No such isolation is claimed.

### Fixed experiment and provenance

- The existing machine is an Intel Core i7-8850H with 12 logical processors;
  the launching process's allowed mask is 4095 (`0xfff`). The processor name
  was read from the local hardware-description registry. A preceding read-only
  CIM query was denied; no permission or machine setting was changed for it.
- Restrict only the benchmark launcher at child creation to mask 4 (`0x4`,
  zero-based logical processor 2), then restore the launcher mask in `finally`.
  Each child's inherited mask is read back. Every child is `Normal` priority;
  every launcher restoration is confirmed. Windows Balanced remains unchanged
  before and after the experiment. There is no persistent affinity setting.
- The first `Start-Process` attempt failed before execution on duplicate
  `Path` / `PATH` environment entries and left two empty redirected logs. A
  second preflight correctly refused to overwrite those files. The successful
  route uses direct `Process.Start` with hidden/no-window execution and both
  output streams drained asynchronously. It leaves the inherited environment
  untouched. Those launch failures are not benchmark failures.
- Diagnostic script: `target/perf/full-system/windows-affinity-diagnostic-1.ps1`;
  final raw-byte SHA256
  `d6ca7d3bd8c92ee0b2b3c902885f2059745bc2c2044b655e3a4638c2b087e124`.
  Default `targets`, `-Suite guardrails` and `-Suite crosscheck` are bounded
  plans with evidence-overwrite checks. All benchmark paths are explicit and
  contain no spaces. The script is a local evidence runner, not runtime code.
- Both ordinary Windows release binaries are unchanged: control
  `582cb824486153cd76e68bd83589ca774b71c85916963287bea4ced5eda86838`,
  candidate
  `b7a994de963df8db8fa8cffa33b722520331587d50c996b9d08ba9567a3658eb`.
  Pre/post raw-byte hashes match the previous frozen artifacts. After all
  captures finished, all 143 files in the candidate source archive also match
  the current Rust/Cargo/config files byte-for-byte; the normal release binary
  matches the frozen candidate. No Rust edits, builds, allocation profiles,
  hashing, compression or other benchmark workloads overlap the measurements.

### Matched control / candidate results

Seven workloads, nine repeats, scale 32, non-quick, all 45 correctness checks,
rustc 1.98.0 (88d9e12ae 2026-08-18), label
`tinyone-workstation-affinity-0x4`, and Balanced policy. Capture 1 runs control
then candidate; capture 2 reverses that order for each workload.
All **28 runs / 14 complete pairs** pass the unchanged evidence gates. The
largest CV is 4.45% in the mixed-type candidate; both integer-hot pairs meet
their stricter 5% limit. Every target/cold/call CV is below 1%.

Transcripts are `windows-affinity-diagnostic-3.log` and
`windows-affinity-guardrails-1.log`. Individual evidence is
`float-store-windows-affinity4-{control,candidate}-{label}-{1,2}.{json,log,err.log}`
under `target/perf/full-system`. The target labels / exact filters are unchanged
from the preceding float-store comparison. Scheduled cycles are primary;
wall deltas below are an independently observed check, not converted cycles.

| Workload / capture | Mean scheduled cycles, control → candidate | Cycle change | Best wall change | CV %, control / candidate |
| --- | --- | --- | --- | --- |
| calls / 1 | 112254.6 → 107982.2 | -3.81% | -3.56% | 0.47 / 0.33 |
| calls / 2 | 109809.3 → 108902.1 | -0.83% | -1.12% | 0.15 / 0.55 |
| float / 1 | 1384176.7 → 862329.8 | -37.70% | -37.89% | 0.51 / 0.98 |
| float / 2 | 1372663.1 → 859171.5 | -37.41% | -37.42% | 0.43 / 0.26 |
| float_cold / 1 | 6443.4 → 5976.5 | -7.25% | -7.28% | 0.81 / 0.48 |
| float_cold / 2 | 6411.4 → 6002.7 | -6.38% | -6.31% | 0.43 / 0.49 |
| hot / 1 | 1540863.1 → 1442324.0 | -6.40% | -6.33% | 0.32 / 0.15 |
| hot / 2 | 1474297.0 → 1427750.2 | -3.16% | -2.98% | 0.41 / 0.15 |
| maps / 1 | 295841.4 → 296389.6 | 0.19% | 0.03% | 0.42 / 0.60 |
| maps / 2 | 297500.5 → 291608.7 | -1.98% | -2.10% | 0.28 / 0.33 |
| mixed / 1 | 1338306.6 → 1266390.3 | -5.37% | -13.15% | 2.78 / 4.45 |
| mixed / 2 | 1311058.0 → 1288018.3 | -1.76% | -1.60% | 0.20 / 0.15 |
| straight_cold / 1 | 8198.7 → 8143.4 | -0.67% | -0.49% | 0.94 / 0.53 |
| straight_cold / 2 | 8139.7 → 8100.4 | -0.48% | -0.31% | 0.52 / 0.29 |

The float target improves mean scheduled cycles by 37.41–37.70% and cold
lowering by 6.38–7.25%. Calls improve in both orders; the previous +17.51%
unrestricted call outlier is not reproduced under the matched mask. The largest
adverse guardrail is +0.19% for maps in one order, reversing to -1.98% in the
other; this is not evidence of a material map regression. Improvements in
unchanged-profile guardrails are not credited to direct float-stack elimination.

### Same-binary scheduling crosscheck

Eight further runs use **only the frozen candidate executable**, varying the
mask between the original 4095 and 4. Capture 1 is unrestricted then fixed;
capture 2 reverses that order. In the crosscheck file names only, `control`
means unrestricted scheduling and `candidate` means mask 4: these are not
different implementations. Each start record includes the exact executable
and observed mask. Other benchmark options, environment and launch mechanism
match; the unrestricted machine label remains `tinyone-workstation`.

All eight runs pass 45 correctness checks. Seven pass the timing gate; the
first unrestricted float run fails with 10.9975636% CV and remains diagnostic
only, including its full aggregate JSON in stdout. It is not saved as an
eligible baseline. Transcript: `windows-affinity-crosscheck-1.log`; individual
logs are `float-store-windows-affinity-crosscheck-{control,candidate}-{float,calls}-{1,2}.*`.

| Workload / capture | Unrestricted CV % | Fixed-processor CV % | Unrestricted / fixed mean wall time |
| --- | --- | --- | --- |
| float / 1 | 11.00 | 0.34 | 280.41 / 337.91 µs |
| float / 2 | 0.30 | 0.24 | 331.80 / 336.48 µs |
| calls / 1 | 8.99 | 0.35 | 38.01 / 41.25 µs |
| calls / 2 | 9.51 | 0.40 | 37.41 / 41.43 µs |

The first unrestricted float capture reproduces the earlier high variance;
both unrestricted call captures have about 9% CV versus 0.35–0.40% when fixed.
The second unrestricted float capture is itself stable, at a different
throughput from the first. Thus unrestricted placement is not always noisy,
and an individual low-CV run does not establish identical throughput conditions
across processes. Fixed placement is steadier here but can be slower in absolute
time. This is evidence of scheduling-sensitive measurement conditions, not a
performance benefit from affinity, proof of the precise earlier outlier cause,
or a claim that processor migration, frequency, sibling contention or any one
Windows mechanism has been isolated. Do not mix the two masks in optimization
comparisons or retry noisy unrestricted rows until they happen to pass.

### Acceptance and continuation

The float-store family is retained with measured Linux and controlled Windows
target / cold-lowering benefit, passing workload guardrails, the existing
numeric/error/alias/block/max-slot tests, and unchanged 60-case allocator /
codec / lock attribution. Across this follow-up, **36 runs pass all 45 checks**;
35 satisfy timing gates, with the sole rejection intentionally preserved above.
Both runner sessions are terminal; no benchmark process remains. Rust was not
changed, so previous full Windows/Linux workspace, Clippy and release symbolic
results apply to exactly the same sources; no new full test or safety-tool run
is claimed here. Post-documentation `git diff --check` and nightly rustfmt
checks of every changed/new Rust file pass. All 35 saved JSON documents retain
the expected release toolchain, repeat count, scale, correctness flag, machine
label and Balanced policy; the rejected crosscheck remains in its stdout log.

This closes the bounded float-store latency decision under the stated
measurement conditions, not unrestricted Windows timing diagnosis or the full
optimization batch. The one earlier call outlier is not retroactively accepted
or erased. Preserve the complete noisy evidence. Further low-level Windows
cause attribution is a diagnostic opportunity, not justification for another
unchanged retry matrix or machine-wide power/security changes. Resume the
remaining runtime/feedback workload, cache, allocator and fused-source-mapping
ledger rather than repeatedly reopening this accepted instruction family.

## Sparse scalar-feedback span — measured and rejected

The previous Windows scheduling work made progress by closing the float-store
acceptance question. This workstream resumed the outstanding large-code
feedback audit, not another Vec/Box comparison. The full 40-section objective,
Router, Rust rules and four examples were read; applicable architecture, VM,
memory, contributor, CI, performance and supplemental gates were retained.
Ralloc 0.2.0 ownership/API documentation was checked, but this prototype changed
only Rust-owned feedback storage: no Ralloc API, unsafe code, value layout,
native backend, capability boundary or external dependency changed. The
[standard-library boxed-slice conversion contract](https://doc.rust-lang.org/stable/std/vec/struct.Vec.html#method.into_boxed_slice)
was consulted before implementation.

### Observed cost and bounded prototype

A feedback entry occupies 11 bytes. The current lazy table covers every lowered
operation in a chunk, including non-adaptive instructions. Consequently a chunk
with more than 5,957 operations cannot fit even an otherwise unused 64-KiB
program budget. This is a table-layout problem, not evidence that changing its
owner from Box to Vec would help.

The retained ignored diagnostic `report_sparse_feedback_storage` measures six
non-executed prefix/suffix padding shapes at thresholds 0/1/8. Each source calls
an identity function, executes a 32-iteration scalar loop, and prints 7. Total
operation counts below include the lazily installed identity chunk.

| Non-executed print padding: prefix / suffix | Total lowered ops | Control feedback bytes | Prototype feedback bytes |
| --- | --- | --- | --- |
| 0 / 0 | 17 | 143 | 22 |
| 1,024 / 0 or 0 / 1,024 | 2,065 | 22,671 | 22 |
| 8,192 / 0 or 0 / 8,192 | 16,401 | 0 (budget denied) | 22 |
| 8,192 / 8,192 | 32,785 | 0 (budget denied) | 22 |

The prototype allocated one fixed boxed span from the first to last potentially
sampleable operation. Its origin was immutable after installation; lookup used
`index - feedback_start`. It did not introduce growth, a sparse map, pages or a
larger budget. Interior non-adaptive holes still occupied entries; distant
potential sites could still exceed the budget. It conservatively included
unreachable potential scalar sites rather than treating only the current hot
range as the complete set of future sites.

All adaptive diagnostic cases installed two sites, sampled 16 times and promoted
twice; threshold 1/8 recorded 47/33 hits. Disabled adaptation allocated no table.
The cost was an eight-byte origin in every reserved chunk slot:
`JitChunk` and `Option<JitChunk>` grew 88 → 96 bytes, while `JitOp=24`,
`JitProgram=208` and the 11-byte entry stayed unchanged. Both cold chunk slots
in each fixture therefore cost 16 additional bytes even with adaptation off.

For the 1,024-print cases, charged retained bytes after profiled execution fell
129,108 → 106,475. In the previously ineligible large cases, retained charges
instead rose by 298 bytes: the added headers, newly installed feedback and new
detailed-profile entries are included. These are capacity charges, not RSS,
allocator overhead or an assertion that enabling specialization allocates less.
Logs: `feedback-span-control-storage-2.log`,
`feedback-span-candidate-storage-1.log` and
`feedback-span-candidate-layout-1.log`. The first storage build had a missing
borrow in the diagnostic; its compile-error log remains preserved.

The prototype passed the new exact-budget/span boundary unit test, including an
operation offset above 65,535. That new test first failed against the control
as expected; `feedback-span-preimplementation-test-1.log` preserves the failure.
Two additional integration tests covered large
prefix/suffix code, thresholds 0/1/8, I64/float/u8/Bool fallback and error parity,
independent clones, lazy callees sharing the same byte budget and fresh repeated
executions. These tests and the changed potential-site budget fixture are
preserved in the candidate archive; they were removed/restored with the rejected
implementation rather than weakened to pretend the current table is compact.

Windows and Linux full hooks workspaces passed for the prototype: respectively
511 and 512 tests across 39 test binaries, with 14 explicit ignores each.
Windows all-target hooks Clippy passed. All 60 runtime allocation profiles match
the accepted float-store baseline exactly in **every** reported heap, Ralloc,
runtime-codec/lock, execution-profile and JIT-stat field. This does not include
the changed host metadata charges. Relevant logs are
`feedback-span-{workspace-tests,linux-workspace-tests,clippy,runtime-allocation}-1.log`.

### Frozen identities and timing conditions

Control and candidate snapshots each contain 143 Rust/Cargo/configuration files
(119 Rust files). Raw-byte comparison found exactly three differing files:
`jit/chunk.rs`, the budget test in `jit/program.rs`, and
`tests/type_feedback.rs`. Only `chunk.rs` changed production execution.
The identical benchmark harness SHA-256 is
`d902d7d2c6f711792511771f765ce9f689f17b7f600b4da096ff57f4095cd302`.

Frozen artifacts under `target/perf/full-system`:

- Windows control `feedback-span-control-1.exe`:
  `65a92540f12e8072f422b5c6165ecd8731e0214267882249f128ddc706196579`.
- Windows candidate `feedback-span-candidate-1.exe`:
  `bd69a41545b3d9fbb95665c6f260fbc349f03a8d1b8cb8cb8419b10912b9ba1c`.
- Linux control `feedback-span-linux-control-1`:
  `9e8535affd2940b5770a8c2fc1c8c340880cbb3ebc8293f9df331d0f320a299c`.
- Linux candidate `feedback-span-linux-candidate-1`:
  `343977f5dd60109785e194b12d0e9bd3c86fe34174ef7903ce224c70bb08cf1b`.
- Control source `feedback-span-control-source-1.zip`:
  `f12170eb6d685c96c1f92ad8f1cb55f6bc17a1e21c90bf6a68140edadccfbb26`.
- Candidate source `feedback-span-candidate-source-1.zip`:
  `8877f1b8a87ebf0d053097fab04cb7582a179e898178d48fe22d1fcec0d7345f`.

The new harness fixture places a 4,096-iteration scalar loop between two
non-executed 2,048-print regions. Its VM/JIT cases raise pre-timing correctness
from 45 to **47 checks**. The warm runtime pair reuses verified/compiled state;
`jit.codegen_sparse_feedback_cold` measures fresh main lowering, while
`jit.cold_run_sparse_feedback` includes fresh lowering, lazy identity
installation, first feedback activation, execution and cleanup. Verification
and fixture construction are outside those intervals. The cold rows use 20
base iterations, so sample scale 32 supplies 640 iterations per repeat.

The bounded Windows matrix used the unchanged Balanced policy, Normal process
priority, affinity mask 4 (logical processor 2), nine repeats and sample scale
32, with `rustc 1.98.0 (88d9e12ae 2026-08-18)` release binaries. The mask was
inherited at launch, read back, and the launcher's original
4095 mask restored. Capture 1 ran control then candidate; capture 2 reversed
order. Builds, source edits, hashing, compression and other measurements were
absent during timings. Linux binaries were built and tested, but no Linux
latency or hardware-counter result is claimed for this rejected prototype.

Runner: `feedback-span-bench-1.ps1`, SHA-256
`39784b1b17a8f285fca0b0e9967d1e01389aa4545bdccefe3e3ca71470fb72d8`.
Transcript: `feedback-span-windows-events-1.log`; individual stems:
`feedback-span-windows-affinity4-{control,candidate}-{workload}-{1,2}`.

All **32 captures pass all 47 correctness checks**. Twenty-eight pass timing
gates, providing 14 complete eligible pairs. All four sparse cold-lowering
captures fail their 10% CV gate and remain diagnostic only, with aggregate JSON
preserved in stdout and no decision-grade baseline saved. The outer transcript's
`GuardrailsExit=1` inherited the prior PowerShell native exit status; each of
the 20 guardrail child reports explicitly records exit 0 and an eligible gate.
The whole invocation correctly remains nonzero because the target suite had
four rejected captures.

| Workload / capture | Mean wall µs: control → candidate | Mean scheduled cycles: control → candidate | Cycles change | CV %: control / candidate | Gate |
| --- | --- | --- | --- | --- | --- |
| `runtime.jit_sparse_feedback_loop_4096` / 1 | 426.19 → 222.06 | 1,103,231 → 574,832 | -47.90% | 0.47 / 1.12 | pass |
| `jit.codegen_sparse_feedback_cold` / 1 | 364.35 → 381.46 | 942,977 → 987,279 | +4.70% | 12.88 / 15.77 | rejected CV |
| `jit.cold_run_sparse_feedback` / 1 | 822.77 → 587.19 | 2,125,065 → 1,519,778 | -28.48% | 9.74 / 3.26 | pass |
| `runtime.jit_hot_loop_4096_quickened` / 1 | 554.61 → 568.62 | 1,435,673 → 1,471,930 | +2.53% | 0.40 / 0.23 | pass |
| `runtime.jit_mixed_loop_4096` / 1 | 499.92 → 544.62 | 1,293,999 → 1,409,748 | +8.95% | 0.31 / 0.31 | pass |
| `jit.calls` / 1 | 41.78 → 42.20 | 108,165 → 109,231 | +0.99% | 0.28 / 0.87 | pass |
| `runtime.jit_map_set_get_256` / 1 | 114.22 → 113.49 | 295,643 → 293,773 | -0.63% | 0.18 / 0.10 | pass |
| `jit.codegen_straightline_cold` / 1 | 3.28 → 3.19 | 8,492 → 8,250 | -2.85% | 5.51 / 0.81 | pass |
| `runtime.jit_sparse_feedback_loop_4096` / 2 | 433.20 → 222.52 | 1,121,258 → 576,003 | -48.63% | 0.76 / 1.78 | pass |
| `jit.codegen_sparse_feedback_cold` / 2 | 367.25 → 376.58 | 950,703 → 974,750 | +2.53% | 13.15 / 13.97 | rejected CV |
| `jit.cold_run_sparse_feedback` / 2 | 810.36 → 582.67 | 2,095,606 → 1,508,297 | -28.03% | 2.85 / 2.56 | pass |
| `runtime.jit_hot_loop_4096_quickened` / 2 | 549.19 → 570.49 | 1,421,586 → 1,476,685 | +3.88% | 0.16 / 0.35 | pass |
| `runtime.jit_mixed_loop_4096` / 2 | 498.97 → 538.63 | 1,291,561 → 1,394,254 | +7.95% | 0.23 / 0.24 | pass |
| `jit.calls` / 2 | 43.09 → 42.21 | 111,537 → 109,269 | -2.03% | 1.71 / 0.34 | pass |
| `runtime.jit_map_set_get_256` / 2 | 113.67 → 114.07 | 294,213 → 295,270 | +0.36% | 2.59 / 0.28 | pass |
| `jit.codegen_straightline_cold` / 2 | 3.19 → 3.16 | 8,259 → 8,182 | -0.93% | 3.21 / 0.30 | pass |

### Rejection, restoration and remaining work

The padded fixture improves warm cycles 47.90–48.63% and first-run cycles
28.03–28.48%, but the ordinary mixed-type loop regresses **7.95–8.95%** in both
orders with CV below 0.32%. The ordinary hot loop also regresses 2.53–3.88%.
Calls, maps and straight-line cold lowering do not show a consistent comparable
regression. Identical TinyOne work profiles rule out a change in those recorded
dispatch/allocator counts; they do not isolate the machine-code reason for the
slowdown or prove the offset subtraction alone caused it.

**The span prototype is rejected and removed.** Its synthetic large-code win
does not justify the consistent realistic-workload regression and larger cold
headers. Longer cold samples and Linux timing were not pursued after this
decision; the prepared, unused longer-duration runner was removed. Existing
captures, failed gates, prototype binaries and complete source archives remain
available. This is not evidence that Vec is faster than Box: both implementations
used lazy fixed-length boxed feedback tables.

Restoration changed only the three prototype files back to their frozen control
contents; all 143 source/configuration entries then matched the control archive
byte-for-byte. The original shared-budget regression test is restored, and the
new diagnostic plus four benchmark rows and 47-check harness remain. The final
Windows/Linux hooks workspaces pass **508/509 tests**, with 14 ignores each;
Windows all-target hooks Clippy passes. Both restored release binaries pass all
47 correctness checks. Both ignored storage diagnostics pass, and all 18 sparse
storage records exactly match the control, including the restored 88-byte chunk
header. Logs use the `feedback-span-restored-*` and
`feedback-span-linux-restored-*` prefixes.

The rebuilt Linux binary exactly matches the frozen control hash. The Windows
whole-file hash differs after relinking
(`1e20219e7221f979d2f6f993fb0d6f3903e7ac19e9c6194fbadf09ab22a9a103`);
its `.text`, `.data`, `.pdata` and `.reloc` section bytes match, while
`.rdata` differs. Do not call the two Windows files byte-identical.
The unchanged `.text` SHA-256 is
`2eac4ea075255fba2e4e208e883fc13419508f1ab046bfb66d23dcef2d0028f7`.
The restored Windows executable is preserved as `feedback-span-restored-1.exe`.
Compared with the previously accepted float-store source archive, the only
retained Rust changes are the benchmark harness and the ignored storage
diagnostic in `jit/program.rs`; no production feedback change remains.
Final changed/new Rust formatting checks and `git diff --check` pass. All 28
saved baselines have matching release toolchain, repeat/scale, checked
correctness, machine-label and Balanced-policy metadata.

The large-code feedback limitation remains open and is now explicitly documented
in the architecture, VM and performance references. Preserve this attribution
fixture and rejected tradeoff; do not reintroduce the same offset-indexed span or
repeat unchanged timings without addressing its ordinary-workload cost. Continue
the broader runtime, collection/allocation, cache and fused-diagnostic ledger.
Fixed metadata stays boxed; changing-length stacks stay bounded vectors; installed
operations still have slice-only access and less than 4 KiB spare capacity per
chunk. No native backend or full-system completion is claimed.

## Canonical map-key width — measured and rejected

The repeated Vec/Box check confirmed existing storage invariants but did not
advance the remaining optimization work. This workstream returns to the measured
map-hashing cost in the scalar symbol profiles. The full 40-section objective
remains active; collection work is not a substitute for the outstanding runtime,
allocator, cache, diagnostic and final-cleanup requirements.

### Scope, contracts and control

- Re-read the full objective and inspected the current map implementation and
  pending boundary diagnostic. The global Rust rules/examples and repository
  router remain authoritative. Memory, performance, syntax/type and map API
  references, Ralloc's crate/API ownership boundary and `RallocVec` were checked.
  The preceding architecture, VM, bytecode, contributor, CI and Ralloc gates
  remain applicable; no allocator implementation or dependency changes here.
- Rechecked the official [Hash/Eq contract](https://doc.rust-lang.org/stable/std/hash/trait.Hash.html)
  and [HashMap key/default-hasher contract](https://doc.rust-lang.org/stable/std/collections/struct.HashMap.html).
  Keep derived equality/hash and the standard randomized hasher. Hash-input
  bytes and Rust layouts below are observed diagnostics, not portable hashes,
  allocator sizes, RSS, or guarantees about future standard-library versions.
- The existing canonical numeric domain is **I64, U8, U16 and U32**, all of
  which fit losslessly in `i64`. The candidate changed only `MapKey::Integer`
  from `i128` to `i64`, uses typed lossless conversions in canonicalization,
  and removes widening from the two direct-I64 builtin paths. Ralloc-encoded
  entries, insertion order, original key tags, logical budgets, deletion index
  repair, heap identities and stored-pointer validation are unchanged.
- Pre-existing debt: I8/I16/I32/U64 values are not indexed by this map path,
  despite the broader numeric runtime and general integer-key documentation.
  That behavior predates this candidate and is not silently changed or claimed
  repaired here. Any broader numeric-key work must account for U64 values above
  `i64::MAX`, not truncate them into this representation.
- Added an eight-string-key/4,096-lookup VM/JIT benchmark guardrail and exact
  `14336` output checks before freezing either side. The common harness now has
  49 pre-timing checks; its 15 unit tests pass. The old
  `map_pointer_key_validation_256` fixture actually uses heap-cell identities,
  not raw pointers; the performance reference now states that limitation.
- The boundary regression verifies unsigned maxima, signed minimum/maximum,
  cross-width lookup/update/delete, index shifts, reinsertion and preservation
  of the original encoded key types. VM output is checked exactly, then compared
  with two executions at JIT thresholds 0/1/8. The initial control fixture used
  a duplicate global `let` name; its compile failure is retained in
  `map-key-control-boundaries-1.log`, and the corrected fixture passes in `-2`.

Both ordinary release controls were built to completion before freezing. The
first source-archive command lacked the Windows compression assembly and did
not create an archive; the corrected capture loaded it explicitly and completed
before production edits. Each completed source archive contains 143 Rust/Cargo/
configuration files. Raw-byte SHA256 comparison finds exactly two differences:
`runtime/heap.rs` and `runtime/stdlib.rs`. The harness and all other archived
sources are identical. Frozen artifacts are under `target/perf/full-system`:

| Artifact | Control SHA256 | Candidate SHA256 |
| --- | --- | --- |
| `map-key-{control,candidate}-1.exe` | `d6ebbdea959e370bd9ca7f3da3a3fefad69f6594dd30fe3849c2af3cfb308d73` | `84fa429fd8d9eb04c13b829bd31c6a62066ddd2d53909ccacf8be46d5fb63c2a` |
| `map-key-linux-{control,candidate}-1` | `d48356b05d7a9964ea45056e947330a4a4aa9cd64210a3d9ff6d03b63383aeb6` | `b95c2d5618e5788a5f78363cc9cdfe77612f0aba0bf4f364ccac8d7a9e2ab106` |
| `map-key-{control,candidate}-source-1.zip` | `312af08efe0851c2ed1805b41fb757c85d66bbdad6aeeb21b7735a6448cc8a4c` | `5fa9bd529f360159bd484a597de5e2b518b43d0d00e72b80816b68daa696a738` |

Harness SHA256 is `00fb59b55ee77f948290393140c3a070610907271af688a84b14ca25954ef511`.
Both frozen variants pass all 49 standalone release correctness checks on
Windows and Linux.

### Structural and correctness evidence

| Observed x86_64 measure | Control | Candidate |
| --- | ---: | ---: |
| `MapKey` size / alignment | 64 / 16 | 56 / 8 |
| `(MapKey, usize)` tuple size | 80 | 64 |
| `MapData` header | 112 | 112 |
| Integer hash-input bytes / writes | 24 / 2 | 16 / 2 |
| String / heap / pointer hash-input bytes | 12 / 24 / 46 | 12 / 24 / 46 |

The layout tests only report observations; they do not assert fixed hash bytes.
The 60 before/after runtime profiles match **all five complete groups** exactly:
heap, Ralloc, runtime codec/lock counters, execution profile and JIT statistics.
Every case checks VM parity and complete Ralloc cleanup. These are instrumented
work counts, not timings. Logs are `map-key-{control,candidate}-runtime-profile-1.log`
and `map-key-runtime-profile-comparison-1.json`.

Windows and Linux hooks workspace suites pass with 509/510 tests respectively
(39 result groups and 15 opt-in ignored tests each). The candidate's 22 stdlib
tests include the numeric boundary regression; its layout diagnostic also passes
explicitly. All-target Windows workspace Clippy with warnings denied, changed/new
Rust formatting and `git diff --check` pass. Existing wider memory-safety evidence
is retained; this safe key-layout change is not a new Miri or OOM-testing claim.

### Windows latency evidence and rejection

The frozen runner `map-key-bench-1.ps1` has SHA256
`500608cc2771c70b3702480e112f1b08751280798dda7add6bcb7650bcc92c9e`.
It schedules two opposite-order pairs, nine repeats and scale 8, checks all
49 correctness results and exact filter selection per invocation, and preserves
failed variance gates. Windows children use actual affinity 0x4 / Normal priority;
the launcher affinity is restored after each start. Balanced power policy is
unchanged. No builds, Rust edits, profiles, compression or other benchmarks run
during latency capture. A nonzero quality result is propagated, and successful
suites explicitly exit zero rather than inheriting an earlier failed status.

The Windows target suite completed all 32 invocations, each with 49 correctness
checks (1,568 passes). There were 27 eligible captures and five rejected captures,
forming 13 complete eligible pairs. JIT set/get capture 1 failed at 31.88% CV;
all four growth captures failed at 14.39–20.24% CV. Their entire pairs are excluded,
including otherwise eligible partners; neither best samples nor partial rows are
used to rescue them. The runner's exit 1 correctly reports those quality failures.

Paired mean scheduled-cycle changes below are candidate relative to control.
Negative means less work. Pair 1 ran control first; pair 2 ran candidate first.
These are the Windows affinity-0x4 measurements, not Linux or hardware-instruction
claims. Exact before/after means, CVs and wall-time ratios are retained in
`map-key-windows-target-summary-1.json`, alongside the event log and per-run files.

| Workload | Pair 1 cycles | Pair 2 cycles | Evidence |
| --- | ---: | ---: | --- |
| JIT set/get, 256 keys | Excluded | -3.47% | Only one eligible pair |
| VM set/get, 256 keys | -4.91% | -1.18% | Both eligible |
| JIT repeated hits | -6.77% | -3.03% | Both eligible |
| JIT repeated misses | -3.02% | -1.89% | Both eligible |
| JIT updates | -1.95% | -2.25% | Both eligible |
| JIT delete/reinsert in capacity | +0.90% | +2.19% | Both eligible; includes setup/deletion |
| JIT deletion | +1.76% | +5.04% | Both eligible |
| JIT growth, 4,096 keys | Excluded | Excluded | No eligible pair |

Deletion moved from 671,867 -> 683,663 and 669,987 -> 703,737 mean cycles; all
four CVs are below 0.74%. Best wall time also regressed 1.88% and 5.50%, and mean
wall time regressed 1.76% and 5.03%. The deletion regression is therefore not a
failed-variance capture or a single reversed-order outlier. The smaller insertion
regressions are supporting observations, not the sole rejection criterion.

**Rejected and removed:** narrowing the key reduced hash input and tuple layout,
and improved several read/update paths, but consistently regressed a realistic
mutation workload. It is not required for correctness or a later architectural
boundary. The full-goal acceptance rule therefore rejects this isolated change;
smaller headers do not by themselves establish better collection performance.
The exact cause of the mutation slowdown is not established by these timings.

The planned remaining Windows guardrails and Linux timings were not launched
after this target failure. In particular, there is no string-key, unrelated
runtime, cold-lowering, Linux latency or new hardware-counter acceptance claim.
The growth and initial JIT noise need not be retried to justify rejecting the
independent, clean deletion regression. Both Linux builds and correctness suites
were tested as recorded above; that is not a substitute for Linux timing.

Both production edits were reversed using a minimal patch. All 143 archived
Rust/Cargo/configuration sources now match the frozen control byte-for-byte
(`map-key-restored-source-comparison-1.json`), and `runtime/stdlib.rs` has no
working-tree diff. The rejected binaries, source archive, diagnostics and all
failed captures remain available. Only the new boundary test, map-index layout
diagnostic, string-key benchmark coverage and documentation are retained from
this workstream.

Post-restoration Windows/Linux hooks workspaces pass again with 509/510 tests
(39 result groups, 15 ignored each). Windows all-target Clippy and the explicit
layout diagnostic pass; the latter reports the original 64-byte/alignment-16
key, 80-byte tuple and 24-byte integer hash input. Both normal release rebuilds
completed and pass all 49 correctness checks. Changed/new Rust formatting and
`git diff --check` pass. Logs use the `map-key-*-restored-*` prefixes.
The rebuilt Windows copy `map-key-restored-1.exe` hashes to
`1dbded01ef999deb69d2f43c0db49c887bc230972e65dbd1e2ab395c362f10a3`;
its whole-file hash differs after relinking, and it is not substituted for
the frozen timing control. The rebuilt Linux executable is byte-identical to
the frozen Linux control. No claim about the exact Windows binary difference
is needed for, or inferred from, source identity and these correctness checks.

Next collection work should measure the discarded temporary byte vector in map
deletion (`RallocVec::remove`) and other actual allocation/codec costs, not repeat
this key-width prototype unchanged. No Vec/Box speed claim or full-system
completion follows from this experiment.

## Map-entry byte discard — measured and rejected

The next bounded collection cost was the temporary host `Vec<u8>` in
`RallocVec::remove`. Its only production caller, `b_map_del`, discarded the
returned bytes immediately. This is not another Vec-versus-Box layout trial:
neither owner is needed for bytes that no caller consumes. Fixed metadata stays
boxed, growable/reused buffers keep their existing representation, and the
earlier inconclusive Vec/Box results remain unchanged.

### Documentation and ownership boundary

The global engineering Router, Rust rules and all four examples were checked,
alongside the documentation index and runtime memory/performance guidance.
The preceding architecture, VM, bytecode, contributor, CI and Ralloc gates remain
applicable. Ralloc 0.2.0's owned-buffer and move-only VM-allocation APIs, and
TinyOne's `RallocBytes` boundary, were reconfirmed. The relevant stable Rust
[GlobalAlloc](https://doc.rust-lang.org/std/alloc/trait.GlobalAlloc.html),
[atomic ordering](https://doc.rust-lang.org/std/sync/atomic/enum.Ordering.html)
and [slice copying](https://doc.rust-lang.org/std/primitive.slice.html#method.copy_within)
contracts were checked before editing. In particular, host-allocation counts
are build-specific observations: the compiler may eliminate an allocation.
No dependency or production allocator was added or replaced.

The prototype changed the private removal method to report whether the index
existed without constructing an owned byte copy. The same logical index check, overlapping
tail shift and length decrement remain. Invalid indices leave the buffer
unchanged; removal retains physical capacity. Stale tail bytes stay outside
logical access, and a later push overwrites the complete fixed-width record.
These bytes encode shallow heap identities, not Rust objects whose destructors
need to run during removal. The canonical key index, pointer sidecar repair,
generation validation, insertion order, logical heap charging and Ralloc
allocation/deallocation paths are unchanged. No production unsafe code changed.
The touched comments now describe actual fourfold growth and test-only physical
capacity inspection, rather than obsolete doubling/shadow-bookkeeping claims.

### Isolated allocation evidence

`tests/allocation/host.rs` owns the existing counting-System implementation,
shared by the call and map diagnostic binaries only. Extraction preserved all
144 existing call records exactly, including host counts/bytes/buckets, runtime
counters and complete Ralloc snapshots; the optional profile-allocation check
also passes. Evidence: `map-remove-control-calls-1.log` and
`map-remove-counter-extraction-2.json`. The first comparison parser incorrectly
named the runtime snapshot type and found zero records; its result is retained
as diagnostic evidence, not a test failure or a successful comparison.

The new ignored map diagnostic covers four key families (integer, string
content, heap identity and raw pointer), front/back deletion and reinsertion,
sizes 0/1/256, thresholds 0/8 and first/repeated executions: **144 cases**.
Every case checks explicit expected output and VM parity, a second deletion of
each missing key, balanced Ralloc allocations/deallocations, zero live payload
after cleanup and agreement between runtime/Ralloc growth and copy counters.
Compilation, output reservation and reporting are outside the interval;
construction, mutation and cleanup are inside.

The Windows MSVC profiles in `map-remove-{control,candidate}-host-1.log` and
`map-remove-host-comparison-1.json` show exactly one fewer host allocation and
deallocation, and 128 fewer requested host bytes, per successful deletion in
**every case**. Zero deletions change nothing. Reallocation counts/bytes and all
other size buckets are unchanged. Every runtime and Ralloc snapshot is identical.
For the 256-entry front-deletion warm adaptive cases:

| Key family | Host allocations before | After | Requested-byte reduction |
| --- | ---: | ---: | ---: |
| Integer | 268 | 12 | 32,768 |
| Heap identity | 268 | 12 | 32,768 |
| Raw pointer | 269 | 13 | 32,768 |
| String content | 1,293 | 1,037 | 32,768 |

Those byte counts are cumulative traffic, not peak memory or retained capacity.
The change does not reduce measured Ralloc work or prove raw allocator latency
improved. The isolated counters never enter the ordinary benchmark executable.

### Correctness and frozen comparison

Windows/Linux full hooks workspaces pass **511/512 tests**, respectively
(40 result groups each, 16 ignored diagnostic/platform cases). All-target
Windows Clippy with `-D warnings`, changed/new Rust formatting and `git diff
--check` pass. The new local regressions cover first/middle/last/only removal,
empty/exact-length/large invalid indices, capacity retention, hidden removed
storage and complete overwriting on reuse. Existing map identity, original key
type, pointer generation, ordering, byte-budget and runtime-failure tests remain
in the workspace gate. Evidence is `map-remove-{windows,linux}-candidate-tests-1.log`
and `map-remove-candidate-clippy-1.log`.

Completed ordinary builds were frozen before timings, with no Rust edits between
final builds and capture. Both OS controls and candidates pass all **49** harness
correctness checks. The two archives each contain 145 raw source/config/lock
entries; only `runtime/ralloc_vec.rs` differs. The 49-check harness stays byte
identical (`00fb59b55ee77f948290393140c3a070610907271af688a84b14ca25954ef511`).
Frozen identities and archive digests are recorded in
`map-remove-{control,candidate}-freeze-1.json`; exact source comparison is
`map-remove-source-comparison-1.json`.

The fixed runner `map-remove-bench-1.ps1` used nine repeats, sample scale eight,
two opposite execution orders and the unchanged correctness/variance gates.
Its SHA256 is `82619e8303a729420798a4a8237321951dd11ec3d1a9a310fb02a9bb21ca2bbd`.
Windows children used affinity 4 / Normal priority, with launcher affinity
restored to 4095 immediately after creation; the Balanced policy is unchanged.
Target rows cover VM/JIT deletion and deletion/reinsertion. Separate Windows
string-map, call, vector and cold-lowering guardrails also completed. No other
builds, profiles, Rust edits, compression or hashing ran during timing intervals.

### Timing outcome and rejection

All **48 captures** completed all 49 correctness checks and passed their
individual evidence-quality gates: 16 Windows target captures, 16 Windows
guardrail captures and 16 Linux target captures, giving 24 eligible pairs.
Candidate-minus-control changes are shown below. Windows' primary metric is
mean scheduled cycles; Linux's is mean thread CPU time. Negative means less
work/time. No Windows CPU-time rounding or Linux TSC counts are substituted for
those primary metrics.

| Platform / workload | Primary change, pair 1 / 2 | Best wall change, pair 1 / 2 |
| --- | ---: | ---: |
| Windows JIT deletion | -11.98% / -9.72% | -12.19% / -10.93% |
| Windows VM deletion | -7.83% / -8.66% | -8.95% / -8.85% |
| Windows JIT deletion/reinsertion | -11.08% / -9.57% | -11.03% / -9.69% |
| Windows VM deletion/reinsertion | -6.77% / -5.59% | -6.88% / -5.94% |
| Linux JIT deletion | -0.39% / +2.18% | +3.03% / +2.73% |
| Linux VM deletion | +2.02% / +2.32% | +2.23% / +1.86% |
| Linux JIT deletion/reinsertion | +2.78% / +2.95% | +4.64% / +2.92% |
| Linux VM deletion/reinsertion | +0.97% / +3.55% | +0.81% / +2.93% |

The first Linux JIT-deletion control has 5.34% CV, within that row's 10% gate;
its mixed mean/best direction is not a win. VM-deletion CVs are all below 0.69%,
and both orders consistently regress. JIT reinsertion also regresses in both
orders. Reduced instrumented host allocations on Windows therefore do not
justify retaining this cross-platform version. The Linux cause has **not** been
established: allocation elimination and generated-code effects are hypotheses,
not conclusions from the Windows counter data.

Windows guardrail scheduled-cycle changes were:

| Guardrail | Pair 1 | Pair 2 |
| --- | ---: | ---: |
| String-map lookup | -3.42% | +0.14% |
| Calls | +0.64% | +0.60% |
| Vector push/pop | +0.28% | -1.35% |
| Cold straight-line lowering | -1.97% | -4.24% |

The small call increase is retained in the record, not hidden by the map win.
A longer call-confirmation runner was prepared but **not executed**: once the
Linux mutation regressions were confirmed, further candidate guardrails could
not establish acceptance. Linux guardrails were likewise not launched. There
are no Linux host-allocation measurements or hardware-counter claims for this
experiment. Evidence is `map-remove-{windows,linux}-target-{events,summary}-1.*`
and `map-remove-windows-guardrail-{events,summary}-1.*` under `target/perf/full-system`.

The byte-discard prototype is rejected under the plan's realistic-workload
regression rule. The original `Option<Vec<u8>>` removal implementation has been
restored. The stronger local tests, shared diagnostic counter and 144-case map
profile remain; the growth/capacity comments are corrected without changing the
growth policy. Restored Windows/Linux full hooks suites again pass 511/512 tests,
and Clippy/changed-new Rust formatting pass. Both final ordinary releases pass
all 49 correctness checks. All 144 restored Windows host/runtime/Ralloc profiles
match the control exactly (`map-remove-restored-host-comparison-1.json`), and the
removal method itself matches the archived original without whitespace or
line-ending normalization (`map-remove-restored-method-1.json`). The remaining
`ralloc_vec.rs` changes are comments and tests only. Frozen restored artifacts
are identified by `map-remove-restored-freeze-1.json`; these newly linked
executables are not substituted into the earlier paired timings. No benchmark
or build process is left running, and no full-system completion is claimed.

## Requirement ledger

Historical ledger: consult the [reconciled acceptance map](optimization-acceptance.md)
for current dispositions and remaining work after subsequent trials.

All items below remain open unless their evidence explicitly proves completion.
An audit or proposed design alone is not an implemented optimization.

| Plan sections | Required result | Current state |
| --- | --- | --- |
| 1–2 | Repository, Rust, supplemental, Ralloc and versioned library gates | First workstream gate read; remaining gates tracked above |
| 3 | Separate frontend, verifier, lowering, execution, Ralloc and memory baselines; all workload classes | Accepted verifier/JIT subsets; 60 isolated runtime cases include codec/lock attribution and exact nested/changing-operand timing shapes; five symbol-enabled Linux profiles locate stack extraction, slot/codec and map-hashing costs; 252 preparation allocation samples distinguish origin-pipeline stages, and 240 further samples plus 32 timing captures expose action-heavy scaling regressions; the separate compact-target trial adds 120 candidate allocation samples and 64 timing captures, rejecting a repeatable Linux copy-store regression; a separate consecutive-expression-origin trial adds 264 new allocation samples and 12 eligible Linux timing captures, rejecting repeatable target regressions; the scoped preparation diagnostic adds 308 timed batches and 5,236 phase records with observer/variation limits; 16 subsequent collection-prefix captures are eligible but do not establish a repeatable cross-platform target gain; shared retained-object fixtures add 28 timing rows and 320 Windows/Linux counter records, while the allocated-prefix hint is rejected after 16 Windows captures (10 eligible); the subsequent size-aware prefix trial adds 320 fresh Windows/Linux counter records and 160 completed timing captures (156 eligible), with repeatable retained-object gains but mixed Windows churn; a fixed 16-process churn diagnostic and 12 full Priority 5 captures then expose short-sample CPU quantization and a remaining longer-batch Windows 64-byte regression; the stationary revision adds 160 current profile records and eight Priority 5 captures, with both Windows pairs and one eligible Linux pair within 5%, while the noisy Linux forward pair is excluded; its full retained-object timing matrix then adds 32 captures (30 eligible) and 98 eligible paired rows, with strong large-case gains and mixed 16-object results; the remaining stationary guardrails add 80 eligible captures and 40 eligible pairs, with all primary changes within 5% but three supplemental CPU/cycle changes above 5%; iterative encoder left descent adds eight target captures (six eligible) and four eligible paired rows, only one two-row pair per platform; its completed six-row guardrails add 48 eligible captures and 24 eligible paired rows, with all primary and supplemental increases within 5%, while 264 fresh isolated allocation records confirm one fewer deep-expression reallocation; a verifier metadata baseline adds 252 matching Windows/Linux allocation records and 28 eligible ordinary complete-verification captures, without a production change; a lazy verifier-label trial adds 252 candidate allocation records and 104 ordinary captures (84 eligible, 37 usable pairs), but two eligible Windows warm-API regressions reject retention and the remaining 48 Linux guardrail captures are not run; initial full capture rejected and wider phase/coverage gaps remain; the source-clock correctness follow-up adds 32 fixed ordinary cache/API captures, with 23 eligible captures and 11 usable pairs; every Windows row lacks two-order confirmation, including zero usable warm-source pairs, so these are limited overhead evidence rather than a speed or full-batch claim |
| 4 | VM, nonadaptive JIT and adaptive JIT differential coverage; allocator contracts | VM/JIT error-string parity restored through cold fused origins, current-source cache rebinding and dependency-validated disk-cache restoration; current Windows/Linux hooks workspaces pass 583/584 tests with no failures; optimized/unoptimized, threshold 0/1/8, repeated/warmed failures, imported permission sites, incremental source locations, malformed/stale sidecar recovery, unchanged-owner cache accounting and paired-action rewrite/reuse origins covered; scoped capture cleanup, lazy attribution, repeated failures and collection matcher prefix/truncation boundaries also covered; real native-exhaustion failures/recovery and zero-sized/unwinding owner boundaries now pass; retained cells/strings/structs/enums and seven first-fit placement/metadata boundaries also covered; broader boundary coverage pending |
| 5 | Explicit pipeline with reusable verified metadata | Shared certificate, symbolic construction, local analysis and final encoding implemented; lexer/compiler share source ownership; imported units contribute to one owned final assembly; wider pipeline work pending |
| 6–7 | Measured compiler/frontend cleanup and portable bytecode quality | Source ownership and final assembly accepted for recorded workloads; owned peephole compaction removes redundant copies/no-op scratch work, reducing optimizer cycles 27–46% and ordinary compilation 4–7% in two clean pairs each without measured retention growth; isolated token confirmation, wider frontend work and bytecode quality remain open |
| 8 | Efficient authoritative verifier and reusable metadata | Dense table accepted; input correctness and duplicate checks repaired; complete-verification (including certificate construction/drop) now has 252 allocation records and 28 eligible ordinary timing captures across metadata and source-derived fixtures; a 21-callsite lazy-label candidate removes large allocation counts and improves all 20 usable verifier pairs but is rejected for Windows warm-API cycles +6.29%/+1.26%; production code is restored with three exact-diagnostic tests retained; no diagnostic-allocation optimization is applied, and certificate-only attribution and broader acceptance remain open |
| 9–10 | Basic blocks, symbolic stack, versions and ordered local passes | Block-local versions and all initial ordered passes implemented; paired action/origin storage was withdrawn despite 6–11% small-case wins because store-heavy lowering regressed consistently on both platforms; separate private buffers restored, with scaling fixtures and origin rewrite/reuse coverage retained; a separate compact-target-only trial is also removed after 18.65%/8.35% Linux copy-store regressions; consecutive-expression-origin compression is also removed after 4.79%/5.28% deep-chain and 12.54%/11.57% late-gap regressions, retaining the new gap fixture and corrected origin diagnostic; scoped hooks-build attribution separates ordered passes and encoding with clock/variation limits; a common collection-prefix guard is removed after inconsistent target timing, retaining only its matcher boundary test; iterative left descent is retained for halved measured deep-chain traversal reservation and one fewer host reallocation, with all 24 cold/runtime guardrail pairs within 5% but mixed timing results and only one eligible original target pair per platform; broader local-IR performance/memory acceptance remains open |
| 11 | Typed indexes, width audit, compact-layout prototype and measurements | Full-variant width audit and 16-byte whole-runtime prototype evaluated; decode bridge rejected after measured regressions, direct 24-byte execution retained; narrowing temporary action targets reduced their measured size from 16 to 12 bytes but was withdrawn after two Linux copy-store regressions; broader typed-ID cleanup pending |
| 12–13 | General direct semantic operations and less value conversion | Scalar stores now include exact-bit float immediates: 28,679 -> 16,391 float-loop dispatches and 12,290 -> 2 pushes/pops each, with codecs unchanged; two Linux pairs reduce target CPU time 35.00–43.53%, cold lowering 7.18–7.37% and hardware instructions 31.86%; two matched-affinity Windows pairs reduce target cycles 37.41–37.70% and cold lowering 6.38–7.25%, with all seven workload guardrails passing; float-store family retained under these measured conditions; frame-resume borrowing retains earlier confirmed gains; broader runtime/representation work pending; the encoded I64 slot-pair trial is removed despite 8,192 fewer full decodes per warm integer loop: two eligible Linux target pairs regress CPU time 6.66%/4.16%, and both Windows target pairs are excluded |
| 14 | Bounded observed type feedback, misses and hysteresis | Initial I64 feedback tier implemented with bounded tables, sampling, misses, demotion and clone/VM coverage; 18 storage cases expose full-chunk budget loss for sparse large code; compact boxed-span prototype removed despite a 48% padded-loop win because two clean Windows pairs regress the ordinary mixed loop 8–9%; a distinct opcode-local typed-ID/compact-box prototype is also removed after consistent ordinary Linux hot/mixed/map increases and a Windows cold guardrail miss; its 64-capture audit has 48 eligible captures and 22 eligible pairs, with no eligible Windows sparse pairs; full-chunk boxed ownership is restored and broader acceptance remains open |
| 15–16 | Explicit VM call frames, preserved depth limits and measured safe reuse | Explicit frames and bounded same-run reuse implemented/tested; frame-resume dispatch and direct argument transfer have independent call timing pairs; transfer removes per-call argument containers while preserving suffix ownership, arity/depth ordering and failure cleanup; real native-pool pressure now checks main/callee frame failures and recovery of the same compiled instances on both platforms; broader frame/reuse latency evidence pending |
| 17 | TinyOne allocation distributions, Ralloc audit, measured allocator changes and safety | 60-case runtime/Ralloc, 144-case host-call/capture, 144-case Windows map-removal, 252-sample origin preparation and 240-sample preparation-scaling profiles; another 120 compact-target candidate records match 120 reused controls in counts and retained state, but smaller requested extents do not prevent Linux copy-store latency regressions; 264 new two-platform consecutive-origin samples show lower growth requests with unchanged installed state and zero Ralloc payloads, but the prototype is rejected by deep-chain and late-gap latency; preparation adds host allocation work but no Ralloc execution payloads; paired records save allocations but are rejected by store-heavy latency; the map diagnostic separates discarded 128-byte host copies from unchanged Ralloc work, but its removal prototype is also rejected by Linux timing; float-store fusion preserves every heap/Ralloc/codec/lock record; argument transfer removes repeated host allocations without changing allocator work; provenance/initialization defects fixed; 132 new inactive-observer preparation records match the reused control exactly; 132 subsequent prefix-trial records also match those controls, including all bucket comparisons; typed-owner early zero-sized destruction and unwind leaks are fixed; actual pool-pressure tests verify failed growth/clone ownership, 32 VM/JIT failures and 32 recoveries per OS with balanced native cleanup; 320 new retained-object records expose persistent early-hole scan costs; the allocated-prefix hint changes no non-scan counters but is withdrawn for insufficient scaling/timing benefit, with fixtures and placement tests retained; a new mixed-placement oracle exposed uninitialized aligned interior headers on the no-tail-split path; the minimal header initialization is retained for correctness with two-platform native and strict-provenance Miri coverage; the distinct size-aware prefix candidate preserves all non-scan counters and cuts retained-object scan work sharply, with two-platform latency gains and 81 then-current Miri tests, but was not accepted: process-isolation evidence does not erase mixed churn, and longer Windows batches still regress 64-byte allocation by 5.61% CPU / 5.57% cycles in the reverse pair; the revised stationary boundary passes 82 fresh Miri cases and preserves all non-scan profile fields, with both Windows Priority 5 pairs and one eligible Linux pair within 5%; 32 current retained-object captures supply 98 eligible paired rows with large-case gains, but small cases remain mixed and two whole family pairs are excluded; all 80 remaining guardrail captures are eligible, with 40 primary comparisons within 5% and three supplemental observations above 5%; retain the stationary boundary for repeated retained-object/fragmentation gains with 16 added bytes per arena and explicit small-allocation/contention tradeoffs; excluded evidence remains excluded and wider allocator audit is ongoing; the encoder follow-up adds 264 fresh isolated preparation records with one fewer deep-expression host reallocation, unchanged installed storage and zero Ralloc work; 132 invalid shared-target records are excluded |
| 18–21 | Measured collections, builtins, module permissions and closures | Direct capture-to-frame transfer removes closure-call vectors and has two clean pairs reducing cycles 41.6–42.3%; fresh-instance and heap-lock lifetime coverage passes; map-key narrowing is rejected after clean deletion regressions of 1.76%/5.04%; byte-discard removal is also rejected despite Windows gains because Linux VM deletion regresses 2.02%/2.32% and JIT reinsertion 2.78%/2.95%; numeric, string-key, removal/capacity and diagnostic coverage retained; 480 new two-platform constant-cardinality query records expose one owned string allocation per read/hit/miss/update, with 240 complete cross-platform counter matches; a subsequent borrowed-key/owned-index prototype removes one profiled key-copy allocation per nonempty text query but is rejected after repeated Linux map set/get, update, insertion and growth regressions and Windows deletion increases; production is restored, with three semantic regressions retained; broader collection/builtin/permission/closure work remains open; a separate get/has-only borrowing trial preserves mutation ownership and all mutation function bodies, saves one profiled text allocation only on reads, but is also rejected for repeated Windows update slowdowns |
| 22 | Trust-safe, independently bounded caches, eviction and retained-byte accounting | Independent LRU bounds, capacity accounting, lazy/error/panic cleanup and bypass/clone coverage implemented; unchanged-owner compile-only hits skip redundant accounting, reducing lookup cycles 12.97–13.66% on Windows and CPU time 10.52–12.02% on Linux; source rebinding remains charged; cold confirmations show small 0.28–1.22% Linux increases, not a cold win; broader cache and memory acceptance remains open; source-cache LRU ages now renumber at u64::MAX across all digest buckets, fixing demonstrated hit, byte-limit and clone eviction failures; exact-source identity, lifetime counters, owners and independent compiled retention stay unchanged |
| 23–24 | Private lazy installation and guarded bounded dynamic-call feedback | Inline guarded-call prototype preserves lazy installation, captures, identity failures, post-promotion module authority and depth limits; all 144 host/Ralloc profiles match the accepted transfer baseline; independent call feedback is now opt-in/default-off with no disabled-path sampling; three revisions plus same-executable Windows isolation do not establish general latency acceptance; Linux counters show only 0.235–0.243% fewer stable-call instructions with mixed cycles, so prototype acceptance remains open |
| 25–27 | Branch optimization, cold error work and precise separate source mappings | Eager line indexes removed with measured attribution benefit; boxed bytecode origins survive compilation/peephole, and cold JIT origins preserve fused arithmetic/builtin/store sites across quickening; both backends format failures lazily; in-memory rebinding and private disk-cache restoration preserve locations and byte accounting, including incremental replacement; 52 eligible pipeline timing pairs expose 43–52% higher cold lowering, smaller execution costs and platform-dependent disk-hit changes; consecutive expression-origin scratch compression is rejected by two Linux target regressions; a single-block map/remap bypass improves cold preparation but is rejected by two Windows warm-call regressions; sampled self attribution retains unresolved header/call-chain limitations; scoped hooks-build attribution now separates construction, passes and encoding but is not ordinary-release latency acceptance; diagnostics remain required for correctness, but whole-pipeline performance acceptance is incomplete |
| 28–29 | Cross-system data-structure audit and removal of repeated work | Bounded Vec/Box ownership policy preserved; current container timing recheck is inconclusive; optional detailed profiling is boxed only when present, shrinking the normal program header 344 -> 208 bytes; token/source/compiler/peephole and frame-resume ownership cleanup implemented; call transfer removes temporary argument/capture vectors without adding boxes or a new pool; bounded cache reads preserve the file-size allocation hint and charge actual capacity; unchanged-owner compile-only hits now reuse their proven storage charge without new containers or retained state; constant-cardinality map queries now have a 480-record host/codec/Ralloc baseline separating key-copy traffic from growth; the subsequent borrowed-query/owned-index trial is reverted for ordinary-runtime regressions, leaving production ownership unchanged; wider audit pending; the read-only map trial keeps MapKey64/entry80/MapData112 and fixed-box/bounded-Vec storage unchanged; fewer query allocations still fail ordinary mutation acceptance |
| 30 | Advanced interpreted loop tier only if measurements justify it | Conditional; primary architecture not ready for evaluation |
| 31 | Remove superseded paths while migrating | Discarded compiler snapshots, peephole input/output copies, rejected compact decode bridge and JIT call-argument split/capture concatenation paths removed; permanently generic dynamic calls share the ordinary opcode/request instead of duplicate handlers; unaccepted operand-pair inlining hint removed after measurement; owned capture snapshots remain only for existing ownership-requiring callers; remaining migration cleanup pending |
| 32–34 | Before/after evidence per optimization, optional profiling, evidence-based retention | Accepted verifier/frontend/peephole/frame-resume, call-transfer and float-store subsets; optional-profile allocation isolated; 144 host-call cases and 60 runtime profiles retained; operand-pair hint rejected by 14 Linux pairs; float-store evidence includes 14 Linux timing pairs, two hardware-counter pairs and 14 controlled Windows timing pairs; eight same-candidate affinity crosschecks expose scheduling-sensitive variability without proving its exact mechanism; all 108 float-store measurement invocations pass 45 correctness checks, with 22 noisy captures still rejected; 308 scoped preparation batches add diagnostic attribution only, with clock-floor and variation limits; 16 eligible collection-prefix captures yield eight pairs but no repeatable cross-platform target benefit, so the guard is withdrawn; encoder left descent adds 48 eligible guardrail captures and 264 fresh isolated allocation records, retaining a scoped scratch reduction with mixed timing results and incomplete repeated target confirmation; 132 shared-target diagnostic records are excluded and ordinary release build provenance is repaired; a separate opcode-local feedback-ID trial adds 64 timing captures (48 eligible), 22 eligible pairs and 3,136 correctness checks, but is rejected for ordinary-workload tradeoffs despite Linux sparse-target gains; remaining phase/whole-runtime acceptance is not inferred from these subsets; the distinct encoded slot-pair trial adds 120 matched diagnostic cases, 64 timing captures (49 eligible), 20 eligible pairs and 3,136 correctness checks; reject it for repeated integer-target regressions, preserving mixed-loop gains and three known late-build overlaps among already excluded cold captures; the borrowed-map trial adds 480 audited before/after query cases and 80 ordinary timing captures (64 eligible), forming 29 eligible whole pairs with 3,920 correctness checks; repeated realistic mutation regressions require rejection, so conditional wider guardrails are not run; read-only map borrowing adds 480 audited comparisons (144 read cases changed, all mutation/integer/zero and runtime/Ralloc fields unchanged) and 24 predeclared ordinary mutation captures (19 eligible, nine eligible pairs); Windows update cycles +2.20%/+1.51% reject retention and the 112 conditional captures are not run |
| 35 | Respect phase dependencies or document evidence for changes | Following baseline → correctness → verifier dependencies |
| 36 | Full TinyOne/Ralloc boundary tests, supported safety tools, formatting and lint | Current Windows/Linux hooks workspaces pass 583/584 tests, with 23 ignored tests and no failures across 46 result groups; both all-target Clippy runs, both default-feature workspace checks and the installed-nightly full-workspace formatting check pass; both matched control/candidate ordinary releases pass 49 printed checks; benchmark regressions now cover actual iteration compatibility and Windows-only batch scaling; nine recorder/phase correctness tests cover nesting, cleanup, thread isolation, phase counts, unreachable blocks and lazy failures; the new collection matcher prefix/truncation test also passes on control and restored code; phase-recorder and prefix-trial allocation records match their reused controls and preserve balanced host owners and zero Ralloc payloads; the two new encoder postorder/origin regressions pass on the control and candidate; both no-std C-library checks passed at the unchanged allocator checkpoint; 82 distinct unchanged Ralloc tests passed strict-provenance Miri at the stationary size-aware prefix checkpoint (56 unit and 26 integration cases), including exact full-pool exhaustion with no filtered unit tests; the earlier allocated-prefix-only hint separately passed 50 unit and 26 integration cases; 120 refreshed runtime allocation records, 320 allocated-prefix retained-object records and 320 subsequent size-aware prefix records and 160 new stationary-prefix records have balanced native cleanup and match across both platforms; older timing/allocation evidence remains historical, not current full-pipeline memory-safety coverage; remaining full-batch checks tracked above; three new map identity/ownership/stale-pointer regressions pass on control, candidate and restored sources; all 480 restored query counter records match their original native controls exactly, and all 144 removal diagnostic cases per OS pass; the existing three map regressions also pass the distinct read-only candidate and exact restoration; all 160 sources at that map checkpoint match its prior restored archive, with fresh Windows/Linux gates and 480 exact original query-counter matches; the latest 163-source verifier restoration preserves three semantic tests that pass on control, candidate and restored production, with fresh native gates and 49 release checks per OS; its production verifier matches the baseline exactly and all other 162 source entries are unchanged; four source-clock boundary regressions now pass (three failed on original production); the 163-source cache checkpoint passes 587 Windows and 588 Linux tests, 23 ignored per OS, all-target hooks Clippy, default all-target checks, full formatting and 49 release checks per OS |
| 37 | Correctness, safety, performance, cold/memory/allocator acceptance | Earlier verifier/frontend/peephole/frame-resume and call-transfer subsets retained; float-store fusion now has Linux target/cold/hardware and matched-affinity Windows target/cold/guardrail confirmation with unchanged heap/Ralloc/codec work; earlier unrestricted failures remain diagnostic and the exact call-outlier cause is not proven; Ralloc typed-owner fixes are retained for correctness, with native-pressure recovery and current strict-provenance Miri evidence; the first size-aware prefix failed a longer-batch Windows reverse guardrail; its stationary revision passes both current Windows Priority 5 pairs and one eligible Linux pair; all 80 remaining guardrail captures are eligible and their 40 primary comparisons stay within 5%; retain that revision for two-order retained-object/fragmentation gains on both platforms, with explicit 16-byte arena metadata, small-allocation and Windows contention costs; the excluded Linux forward and retained-family evidence, mixed small results and three supplemental increases above 5% still limit broader claims; retain left-descent encoding for measured scratch/growth reductions with all 24 cold/runtime pairs within the predeclared 5% limit and unchanged installed/Ralloc storage; Windows dead-store and Linux hot-loop point estimates are slower in both orders, and the original target confirmation remains limited to one eligible pair per platform; the later compact feedback-ID prototype is removed: its sparse-only gain does not justify repeated Linux hot/mixed/map slowdowns, a Windows cold guardrail miss, and additional opcode/handler complexity; no blanket Windows scheduling guarantee or full-batch completion claimed; the encoded slot-pair prototype is also removed after repeated realistic integer-loop regressions; all six scoped sources are restored to the 160-file control checkpoint, without a Vec/Box or allocator-policy change; borrowed map queries are also rejected: Linux JIT set/get CPU +3.55%/+7.17%, updates +7.43%/+5.24%, insertion +1.32%/+1.07% and growth +9.22%/+2.12%, plus Windows deletion cycles +1.72%/+0.85%; string-read and allocation gains do not justify these costs; both production files are restored exactly and all 160 source/archive entries verified, with only stdlib parity tests differing from the map-query control; the distinct get/has-only candidate is also rejected: Windows update cycles +2.20%/+1.51%, CPU +2.46%/+2.37% and wall +3.15%/+1.43%; Linux has only one eligible pair per screen row after five exclusions. Both production files are restored exactly; no new source/test or Vec/Box-policy change remains; the distinct lazy-verifier-label candidate is also removed: all 20 usable verifier pairs improve, but Windows warm source-API scheduled cycles rise 6.29%/1.26% and wall time 6.73%/1.45% in both qualifying orders; 104 captures complete, while 48 Linux guardrails are omitted after the Windows rejection; the original production verifier is restored exactly, retaining only three semantic tests and no Vec/Box change; retain the small source-clock rollover change for demonstrated LRU correctness, not speed: no consistent regression is established by the 11 usable normal-path pairs, but Windows has four confirmation gaps and individual eligible Windows verified-hit and Linux cold/VM comparisons exceed 5%; all such costs and exclusions remain explicit |
| 38 | Final cleanup, tests, benchmarks and current architecture/memory/JIT/Ralloc documentation | Pending |
| 39 | Full implementation report: compliance, architecture, measurements, cleanup, correctness and remaining opportunities | This worklog is preparatory; final report pending |
| 40 | Preserve proven facts; reduce work and fall back safely | Governs every phase |

## Continuation

The active goal remains the complete optimization batch. Bounded per-site
observed type feedback (section 14) now executes on the direct semantic tier;
finish its broader workload, sparse/large-code and performance acceptance,
then continue the full remaining implementation ledger, including dynamic
calls and broader cache/performance acceptance. Compiled retention now has
independent entry/byte limits and tested ownership/accounting boundaries.
Compiler token consumption now transfers lexical text without repeated clones
or a separate vector cursor. Lexer/compiler source ownership is now shared,
and exact diagnostic line lookup happens only on error, removing duplicate
source copies and eager indexes. Two independent line-heavy lexing pairs and
two ordinary compilation guardrail pairs support that narrower change.
Independent token-ownership and line-heavy compilation timing confirmation,
wider frontend work and precise fused runtime diagnostics remain open.
The later source-origin checkpoints implement fused runtime diagnostics,
restore suite parity and preserve source locations through disk-cache hits and
incremental rebuilds; the diagnostic pipeline's performance acceptance remains
open.
Imported units now contribute definitions without throwaway program snapshots;
the final compiler owns assembly and transfers completed buffers. Compacting
finished extents removes the measured move-only retention regression, and two
clean pairs per ordinary/declaration/module compilation workload confirm lower
cycles. Peephole optimization now consumes and compacts instruction buffers,
skips scratch allocation when no constant candidate exists, and keeps compact
finished extents. Two clean pairs for all four optimizer workloads and ordinary
compilation support that change. Repeated constant-chain passes still need
review; bytecode quality is not complete merely because those copies vanished.
Next prioritize the remaining runtime/feedback performance and workload audit,
including the recorded hot-loop regressions, collections/closure allocation
and dynamic-call feedback, alongside the full profiling ledger. Do not replace
that work with further frontend-only cleanup or repeated container rechecks.
Frame-resume chunk borrowing now removes repeated installed-chunk resolution;
two clean reversed-order pairs confirm call, mixed-type loop and encoded-slot
multiply improvements while all 52 detailed profiles stay unchanged. General
hot-loop/float confirmation, closure guardrails and contradictory map timings
remain open. Preserve all those results and the fixed binary/source identities;
do not turn repeated unchanged benchmark retries into the next workstream.
Direct argument/capture transfer now removes the measured per-call host vectors:
512 scalar calls use six host allocations instead of 518; one-capture closures
use ten instead of 1,034 and two reallocations instead of 514. Independent
function-heavy, function-value and closure pairs confirm lower cycles; all 72
Ralloc/codec/heap-lock records and the 52 dispatch/adaptation profiles are
unchanged. Direct transfer itself does not implement callee-identity feedback.
An inline guarded-call prototype now exists, with current workspace and Clippy
checks passing. Post-promotion module-authority and recursion-depth boundaries
are covered, but three measured revisions do not establish latency acceptance.
The prototype now has a separate default-off option: ordinary dynamic calls
never sample, scalar quickening remains independent, and generic fallbacks share
the ordinary call path. Boxing optional detailed profiling reduces the normal
program header from 344 to 208 bytes without allocating when disabled. All 144
host/Ralloc cases match the earlier accepted transfer baseline. Same-executable
Windows option comparisons remain mixed; Linux hardware attribution finds only
0.235–0.243% fewer stable-call instructions, also with mixed cycle direction.
Exact warmed static/polymorphic/unstable listings and profiles agree, and Linux
instruction totals differ by less than 0.00022%; ongoing feedback work does not
explain the large Windows gaps. These findings close option isolation and cold
profile ownership, not latency acceptance. Any further section-24 redesign must
identify a larger measured call-resolution cost before adding more specialization
complexity. Preserve the frozen pre-feedback control to separate resolution from
argument/capture costs. Continue the outstanding hot-loop/float/map and broader
runtime/feedback workload audit instead of another unchanged call timing retry.
That audit now has 60 exact process-isolated runtime profiles and five Linux
symbol captures. Pair extraction accounts for 11–12% of sampled self cycles in
the nested/float fixtures; physical stack traffic remains about 12,000 pushes
and pops per run. A minimal inlining hint was tested and removed: all 14 Linux
timing pairs pass the gate, but target/guardrail directions do not justify it,
and Windows code bytes do not change. The slot-plus-float-literal store is now
fused in the existing local encoder, with exact-bit, numeric/error, aliasing,
block-entry and maximum-slot coverage. Its 60 profiles preserve allocator and
codec work; the affected float loop removes 12,288 dispatches and as many
pushes/pops each. Two clean Linux pairs confirm 35.00–43.53% lower target CPU
time and 7.18–7.37% lower cold lowering time; hardware instructions fall 31.86%
in both orders. The subsequent changed Windows scheduling diagnostic supplies
14 matched-affinity timing pairs across all seven target/guardrail workloads:
float cycles fall 37.41–37.70% and cold lowering 6.38–7.25%; the call outlier
does not recur and all pairs pass. Same-candidate scheduling crosschecks
reproduce unrestricted variance, while also showing that unrestricted runs can
be stable at different throughput. Retain the float-store family with these
controlled-platform qualifications; do not infer an exact OS mechanism,
discard earlier rejected captures or treat affinity as a runtime speedup.
Resume the broader runtime/feedback workload and cache/allocator/source-mapping
ledger rather than another unchanged float/call/container timing retry.
The complete system goal remains active.
The large-code feedback audit now has 18 paired storage cases, a 47-check
benchmark harness and 32 Windows captures. Its compact boxed-span prototype
has been removed: the padded-loop benefit came with a consistent 8–9% ordinary
mixed-loop regression and larger cold headers. All source bytes match the
frozen control again, restored Windows/Linux tests and Clippy pass, and all
evidence is preserved. The full-chunk feedback limit is still unresolved.
Do not repeat this exact span or container experiment as the next workstream;
continue the remaining runtime, collection/allocation, cache and precise
source-mapping work from the measured cost ledger.
The renewed user-requested container
comparison was inconclusive and is not feedback performance evidence.
Do not repeat the already-supported Vec/Box or argument-transfer comparisons
as a substitute for the outstanding section-24 work.
Continue useful implementation/profiling work instead of treating
noisy timing retries as the whole optimization batch.
Keep cold/runtime acceptance
and remaining builder-memory costs tracked alongside it. The compact decode bridge has
been measured and removed; do not restore it without addressing its added
execution work. Definition-run compression removes the extra 4-MiB
action allocation in the deep diagnostic; it does not resolve the timing
regressions above. Preserve versioned snapshots and runtime failure ordering;
do not restore a parallel scalar optimizer or repeatedly revisit the same
container comparison without new evidence.
Keep native-code generation out of scope and continue the full remaining ledger.

The map-key width experiment is now also complete and rejected: 32 Windows
captures reveal smaller hash input and improved lookups but clean deletion
regressions of 1.76%/5.04%. Original production sources and both release targets
are restored; the numeric boundary test, layout observer and string-key benchmark
remain. Do not repeat this representation change unchanged. The discarded-byte
removal prototype has now also been measured and rejected: it saves one host
allocation per successful deletion in 144 Windows profiles and improves Windows
map timing, but 16 Linux captures establish mutation regressions. The original
removal implementation is restored, all 144 restored profiles match, and both
ordinary releases pass 49 checks after 511/512 Windows/Linux workspace tests.
Capacity/reuse regressions and the shared test-only allocation diagnostic remain.
Do not repeat the same discard API or infer Linux allocator behavior from the
Windows counter result. A follow-up must establish the ordinary Linux cause or
target another measured collection cost, while keeping the broader cache,
source-mapping and full-system ledger intact. The longer Windows call-confirmation
runner and Linux guardrails were not run after rejection; do not treat their
prepared scripts as measurements.

Complete the missing profiling/workload evidence and remaining phase ledger,
then perform the full requirement-by-requirement acceptance audit. This
checkpoint does not complete the optimization plan.

## Source-origin pipeline: compiler, peephole and portable VM checkpoint

The preceding Vec/Box recheck did not advance the full-system goal. This
continuation reread the complete 40-section objective and resumed sections
26–27. The original three runtime-location regressions failed at `vm/0` with
bare division errors (`runtime-locations-before-1.log`). The compiler had token
positions, but successful compilation dropped its source owner; neither
backend had a runtime source-origin chain.

Implemented the first stages, without changing executable instructions or
claiming the complete diagnostic migration is finished:

- `source/bytecode.rs` owns typed host-width byte offsets in fixed boxed
  tables, shared file text, and a first-failing-frame site. Compiler builders
  still grow with Vec, but installed origin tables expose no growth interface.
  Source string APIs are not silently restricted to the file loader's 1-MiB
  limit or truncated to 32-bit offsets.
- Compiler emission records the originating arithmetic/unary operator, call,
  builtin, allocation, indexed/field operation, assignment and print token.
  Function compilation transfers its own table and restores the parent table;
  all chunks from one file share its immutable source owner. End-of-program
  origin uses the end-of-input site.
- Peephole compaction remaps origins in lockstep with instruction offsets.
  Folded ranges select their final operator; unchanged following operations
  retain their own sites. Changed shared tables use copy-on-write, and final
  shrinking happens after all passes. Unchanged chunk owners remain shared.
- Program/function equality still compares their pre-existing executable and
  policy fields, deliberately excluding diagnostic context. Fingerprints,
  opcode layouts, JSON/binary formats, error variants and FFI envelope fields
  are unchanged. Artifact decoders explicitly have no source owners. A decoded
  artifact remains untrusted, independently of source metadata.
- Inclusive verified-memory accounting charges fixed origin tables and each
  distinct file/chunk owner once within a program. Accounting is memoized by
  the existing verification token; allocator/Arc overhead remains excluded.
- The portable VM records the innermost failing bytecode site on error and
  formats once at its outer run/thread-function boundary. It does not fetch
  source offsets, format diagnostics or allocate debug structures per successful
  instruction. A sourceless callee is not mislabeled as its caller. Existing
  VM frame allocation/recursion and Ralloc ownership are unchanged.

New passing regressions cover matching code/table extents, shared file owners,
deduplicated retention, optimizer clone isolation, multi-pass folding,
unchanged artifact bytes/fingerprints, absent artifact origins/authority,
host-width offsets, invalid-site fallback, sourceless callees, and portable-VM
array/index/field/builtin/unary/CRLF locations. The original three parity tests
now aggregate every optimization/threshold combination rather than stopping
at their first failure; both optimized and unoptimized VM cases pass, but all
their JIT configurations still lack locations. No failing test was ignored or
weakened to compare only an error substring.

`source-origins-storage-1.log` explicitly runs the new storage diagnostic:

| Fixture | Source bytes | Chunks / origins | Offset-table bytes | Diagnostic retained bytes |
| --- | ---: | ---: | ---: | ---: |
| Tiny `print 42` | 8 | 1 / 3 | 24 | 114 |
| Two functions | 55 | 3 / 13 | 104 | 289 |
| 4,096 comment lines and `print 42` | 65,544 | 1 / 3 | 24 | 65,650 |

These are current backing-storage/header charges, not before/after timings,
allocator overhead, RSS, or evidence of a speedup. Retaining source text is a
real memory change and needs the pending acceptance measurements.

Validation on the current Windows tree:

- `cargo test --workspace --features testing-hooks --locked --offline
  --no-fail-fast`: **508 passed, 12 failed, 17 ignored**, across 41 reported
  result groups (`source-origins-vm-tests-2.log`). The first capture, before the
  last portable-VM case and storage diagnostic, was 507/12/16. All 12 current
  failures show a located VM error versus a bare JIT error. They span the three
  new location tests plus nine existing overflow, call-depth/arity/kind,
  permission, cached lazy-function, and scalar-fallback parity tests. This is
  a known temporary regression, not a green full suite.
- Both all-target workspace Clippy runs with `testing-hooks` and `-D warnings`
  pass (`source-origins-vm-clippy-1/2.log`). Changed/new Rust formatting and
  `git diff --check` pass.
- The focused source suite passed eight tests (one existing diagnostic ignored
  at that point); the six-fixture, two-compiler portable-VM location test and
  explicit source-retention diagnostic pass separately.
- No new release timing, Linux validation or memory-safety run is claimed for
  this unfinished source-mapping pipeline. Earlier frozen release artifacts
  predate it and are not evidence for the current working source.

**Required next work:** carry original bytecode sites through symbolic
definitions, ordered actions, final JitOp selection and quickening. A fused
operation can fail at more than one constituent operation (especially map
lookup/add and multiply/map-set); a block-start or single arbitrary origin is
not sufficient. Preserve first-callee failure sites and caller sites for failed
frame entry while retaining existing cleanup. Compiled-cache hits must use the
current caller's diagnostic context even when executable fingerprints match;
disk-cache hits need source-origin restoration without changing frozen public
artifact formats. Incremental function replacement already moves/clones its
function-owned origin table, but complete cached-source restoration is not
implemented. Restore full VM/JIT parity, cover warmed/fused/cache/import error
paths, then measure allocation/retention and cold/runtime effects on both
platforms. The full 40-section goal remains active, with its other ledger gaps
unchanged.

## Source-origin pipeline: JIT fusion, quickening and in-memory cache checkpoint

The previous checkpoint's 12 VM/JIT message mismatches are now resolved. This
is required diagnostic correctness work for sections 4 and 26–27, not a measured
runtime optimization or completion of those sections. The Rust router and all
four examples were reconfirmed. No external dependency, artifact format,
capability policy, Ralloc ownership rule or encoded-value layout changed.

- `jit/origins.rs` owns construction-time origins and retained cold metadata.
  A primary boxed table holds four-byte original bytecode indexes. Sparse
  records retain distinct secondary/store sites for multi-operation fusions.
  `LoweredChunk` emits instructions and origins together; its instruction
  buffer exposes mutation only as a slice until the builder is consumed.
  Tables freeze as boxed slices and are shared immutably across chunk clones.
- Local definitions/actions carry origins through the existing in-place ordered
  passes. Final encoding preserves expression sites even when materialization
  is deferred. Collection fusions explicitly distinguish map lookup/add/store,
  multiply/map-set/store, and the array-length builtin. Installed instruction
  extent and origin ownership do not change during quickening.
- The JIT records the actual failing instruction and selects secondary/store
  stages only on an error. Source lookup/formatting occurs after releasing the
  executable-chunk borrow. Callee failures keep their own sites; failed call
  entry uses the caller's call instruction. A sourceless callee stays bare.
  Existing frame/operand cleanup remains in place.
- Compiled-cache run and compile-only hits rebind the current `VerifiedProgram`
  only after the unchanged content/authority key matches. This replaces source
  text/filenames without rebuilding lazy code or discarding quickening. Byte
  accounting covers the new owner even after failure; a larger owner can force
  eviction/bypass. Rebinding releases the previous verified owner.

Storage diagnostics (Windows x64; allocator/Arc control overhead and RSS
excluded) are recorded in `source-origins-jit-storage-1.log` and
`source-origins-jit-builder-1.log` under `target/perf/full-system/`:

| Fixture | Installed chunks | Cold JIT origin bytes, including table headers |
| --- | ---: | ---: |
| small | 1 | 48 |
| loop | 1 | 100 |
| function | 2 | 108 |
| map | 1 | 112 |
| many stores | 1 | 48 |

`JitOp` remains 24 bytes and `JitProgram` remains 208 bytes. `JitChunk` grows
88 → 96 bytes for the shared origin owner, separately from the bytes above.
Each sparse record is 16 bytes on this target. The symbolic builder grows
168 → 224 bytes; the 65,536-depth diagnostic adds 1,048,624 bytes of origin
scratch, while its existing expression/traversal reservations are unchanged.
These scratch vectors die at installation. This is an explicit cold-memory
cost, in addition to the previous checkpoint's retained source text/offsets;
it is not an allocation-count or latency result. No blanket Vec-over-Box claim
is made: growable construction/reuse and fixed installed metadata keep their
distinct ownership contracts.

Validation:

- The eight runtime-location tests compare complete VM/JIT errors and prior
  output with optimized and unoptimized compilers, thresholds 0/1/8, and two
  executions of each JIT instance. Listing checks prove the map/array fusion
  paths were selected. The warmed scalar regression observes promotion, hits,
  a type-guard miss, and the original error location on repeated runs.
- Unit regressions cover primary/sparse stage lookup, empty/out-of-range
  origins, exact frozen charges, shared origins across clone/quickening,
  sourceful/sourceless cache transitions, retained lazy code, larger-owner
  bypass, accounting, and owner release. Imported permission denial now checks
  the module's precise filename/line/column. The initial map-set fixture had
  its expected column miscounted as 38; inspection confirmed `*` is column 39,
  and that expectation was corrected without changing diagnostic behavior.
- Full Windows/Linux workspace suites with `testing-hooks`, `--locked
  --offline --no-fail-fast` report **529/530 passed, zero failed, 17 ignored**,
  across 41 result groups. First captures are
  `source-origins-jit-tests-1.log` and `source-origins-jit-linux-tests-1.log`.
- Windows all-target Clippy with `testing-hooks` and `-D warnings` passes
  (`source-origins-jit-clippy-1.log`). Both newly built ordinary releases pass
  all 49 `--correctness-only` checks (`source-origins-jit-release-checks-1.log`
  and `source-origins-jit-linux-release-checks-1.log`).
- Final full-suite reruns after the imported-module and sourceless-callee
  assertions retain those same totals (`source-origins-jit-tests-2.log` and
  `source-origins-jit-linux-tests-2.log`). Clippy passes again in
  `source-origins-jit-clippy-2.log`; changed/new Rust formatting and
  `git diff --check` pass. All test/build sessions are terminal.
- No before/after timing, RSS, allocation-count comparison or new Miri run is
  claimed for this pipeline. Earlier frozen binaries predate source origins
  and cannot establish its performance acceptance.

Next: restore dependency-validated disk-cache origins, including unchanged
modules during incremental replacement, without putting diagnostics in the
frozen public artifact formats or re-granting cached authority. Then measure
compile/lowering, cold/warm execution, allocation and retention costs against
an appropriate pre-origin control on both platforms. Source/debug metadata
performance acceptance and the rest of the 40-section ledger remain open.

## Source-origin pipeline: dependency-validated disk-cache checkpoint

Disk-cache hits and incremental rebuilds now retain runtime source locations.
This completes a missing diagnostic-correctness boundary for sections 4 and
26–27, not performance acceptance of the whole source pipeline or completion
of the optimization batch. The regression in
`source-origins-disk-before-2.log` demonstrates the original defect: the second,
cache-hit execution lost its filename/line/column and reported only
`Division by zero`. The preceding `before-1` capture was a fixture import
error, not evidence of that runtime defect.

- Private cache format v9 adds a `.origins` sidecar without changing either
  portable artifact format, fingerprints, policy or verification rules. Old
  versioned cache keys are not reused, migrated or deleted. Sidecar extents
  come from already verified main/function code, never serialized counts.
  Four-byte input indexes can refer only to recorded root/module sources.
  Four-byte byte offsets must fit the digest-validated source text.
- A snapshot digest binds the fingerprint, optimization mode and ordered input
  paths/content digests. A separate checksum detects damaged/torn bytes.
  Neither is authentication or a source of authority. Missing, oversized,
  malformed or mismatched sidecars are ordinary misses; writes publish the
  metadata last but are not an atomic three-file transaction.
- The probe reuses source bytes already read for dependency validation.
  `SourceInput` models unavailable bytes, owned bytes and a materialized shared
  source owner explicitly. UTF-8 conversion moves the allocation, and functions
  from the same file share it. Manifest/signature bytes are not retained as
  diagnostic text. Offset builders become fixed boxed tables; installation
  consumes its temporary vector instead of retaining a growable table.
- Restoration requires a private verification token and private program owner.
  It changes only diagnostic fields, preserves code/certificate/fingerprint
  allocations, and invalidates the memoized retained-byte scan. Shared tokens
  or program aliases are rejected without mutation. No additional verification
  pass is needed for that operation. The existing incremental patch still
  verifies its combined executable result.
- Unchanged modules retain their restored locations during incremental patching.
  Only functions owned by the module about to be replaced can lack old origins;
  replacement functions carry fresh origins through operand-index relocation.
  The original builtin/host-permission cache rejection, signature expiry checks,
  metadata prefilter, resolution checks and size/filesystem bypass policies stay
  in force. Artifact loading/verification now precedes input digests after the
  metadata prefilter; its effect on digest-invalid misses still needs timing.

Bounded reads now obtain length from the opened file handle, limit the read to
the permitted bytes plus one, and check the result before use. This prevents
file growth after a metadata check from admitting oversized cache inputs.
An initial `Take<File>`/empty-Vec implementation lost the file reader's size
hint. The focused storage diagnostic measured the resulting spare capacity,
then the reader was changed to reserve the checked file length. Aggregate
retained source-buffer limits now charge capacity rather than length.

Windows x64 observed backing storage, not allocator overhead or latency:

| File bytes | Initial bounded reader capacity | Size-reserved reader capacity |
| ---: | ---: | ---: |
| 31 | 63 | 31 |
| 4,096 | 8,192 | 4,096 |
| 5,000 | 8,192 | 5,000 |
| 65,537 | 131,072 | 65,537 |
| 1,048,576 | 2,097,152 | 1,048,576 |

Evidence: `source-origins-disk-read-capacity-before-1.log` and
`source-origins-disk-read-capacity-after-1.log`. Empty, one-byte, 32-byte and
33-byte inputs were also checked. The final Linux diagnostic observes the same
exact file-size capacities (`source-origins-disk-linux-storage-1.log`); no
Linux before measurement is claimed. Tests check byte contents and exact/over
limits, not a universal allocator guarantee about future capacity behavior.

Sidecar storage is `36 + 4 * (instruction_count + chunk_count)` bytes, bounded
by the existing verifier limits at 1,065,000 bytes. The three-chunk roundtrip
fixture emits 104 bytes and restores 328 bytes of source metadata within a
1,532-byte inclusive verified-program charge on both measured x64 targets.
These charges exclude allocator/Arc control overhead. Validated source-buffer
capacity is separately capped at `(verified_modules + 1) * 1 MiB`, at most
257 MiB; filenames, source/table headers and transient reads are additional.
Neither bound is a whole-process peak-memory cap.

Validation:

- Six disk-cache integration tests pass, including root and imported-module
  locations in VM/JIT, unchanged transitive dependencies during incremental
  replacement, moved source sites, same-bytecode whitespace changes, and repair
  of absent/empty/truncated/extended/corrupt/stale sidecars followed by a hit.
  Existing manifest resolution, spoofed timestamp, changed interface and bypass
  cases still pass (`source-origins-disk-parity-1.log`).
- Unit tests reject every truncated prefix, invalid checksum/header, invalid
  input owner/index and out-of-range offset even after recomputing the checksum.
  They cover invalid UTF-8/missing inputs, missing/unrecorded source metadata,
  snapshot mismatch, shared source ownership without copying validated text,
  immutable shared program aliases, exact chunk extents, certificate/code
  preservation and refreshed retained-byte charges.
- Final `cargo test --workspace --features testing-hooks --locked --offline
  --no-fail-fast` runs pass **541 Windows / 542 Linux tests, zero failed,
  17 ignored**, across 41 result groups. Logs:
  `source-origins-disk-tests-2.log` and
  `source-origins-disk-linux-tests-2.log`. Earlier `-1` suites pass 540/541
  before adding the bounded-reader regression.
- All-target workspace Clippy with `testing-hooks` and `-D warnings` passes
  (`source-origins-disk-clippy-2.log`). Its first run requested `as_chunks::<4>`;
  the parser now consumes fixed arrays directly, removing per-offset slice
  conversion. A temporary test-fixture `Cow<str>` borrow mismatch in
  `source-origins-disk-unit-1.log` was fixed before subsequent passing suites.
- Both rebuilt ordinary releases pass all 49 `--correctness-only` checks:
  `source-origins-disk-release-checks-1.log` and
  `source-origins-disk-linux-release-checks-1.log`. Changed/new Rust formatting
  and `git diff --check` pass. No unsafe implementation, Ralloc change or new
  Miri/sanitizer result is introduced by this checkpoint.

The full objective, global AGENTS router, Rust rules/four examples and relevant
cache, compiler, artifact, source, architecture and memory documentation were
reconfirmed. Existing dependency versions remain blake2 0.10.6, hex 0.4.3,
serde 1.0.228, serde_json 1.0.150 and test-only tempfile 3.27.0, with no feature
changes. Reviewed the applicable versioned hashing/hex/serde APIs and installed
serde_json documentation, plus
[Arc private mutation](https://doc.rust-lang.org/stable/std/sync/struct.Arc.html#method.get_mut),
[OnceLock reset](https://doc.rust-lang.org/stable/std/sync/struct.OnceLock.html#method.take),
[fixed array slices](https://doc.rust-lang.org/stable/std/primitive.slice.html#method.as_chunks),
[bounded reads](https://doc.rust-lang.org/stable/std/io/trait.Read.html#method.take),
and [temporary-file ownership/security](https://docs.rs/tempfile/3.27.0/tempfile/struct.NamedTempFile.html).
The installed nightly `Read`, `Take`, `default_read_to_end` and slice sources
were checked for the capacity and array-conversion decisions.

The user-requested Vec/Box rule remains ownership-based: growth/reuse buffers
use Vec, fixed metadata uses boxed slices, and installed `JitOps` exposes only
slices with less than 4 KiB spare capacity per chunk. Its comment now explicitly
says that the current timing recheck was inconclusive. The new bounded-reader
storage evidence does not establish a general Vec-over-Box speed advantage.

Next: measure the entire source-origin pipeline against an appropriate
pre-origin control on both platforms, separating compile/lowering, cache
hit/incremental/miss, cold/warm execution, allocations and retained memory.
Sidecar hashing/I/O, earlier artifact validation and added retained text are
known costs, not accepted speedups. No before/after whole-pipeline timing,
allocation-count or RSS result is claimed here. Continue the remaining
40-section ledger; the full goal stays active.

## Source-origin pipeline: latency costs and unchanged-owner cache hits

This checkpoint measures the three preceding diagnostic-origin changes, then
removes one identified redundant cache operation. It is not another Vec/Box
experiment and does not claim that Vec is faster. Fixed metadata stays boxed,
builders and changing-length stacks retain Vec, and installed `JitOps` still
exposes only slices with less than 4 KiB spare capacity per chunk.

### Controls and measurement conditions

The pre-origin control is the restored map-removal snapshot, not that rejected
prototype: `map-remove-restored-1.exe`, `map-remove-linux-restored-1`, and
`map-remove-restored-source-1.zip`. The first diagnostic candidate is frozen as
`source-origins-current-1.exe`, `source-origins-linux-current-1`, and
`source-origins-current-source-1.zip`. SHA256 identities and timestamps are in
their `*-freeze-1.json` records under `target/perf/full-system/`.
`source-origins-source-comparison-1.json` records 20 changed existing files and
four added origin/test files across 145/149 source entries. The benchmark,
Ralloc, runtime heap, memory and collection implementation files match exactly.
The shared benchmark source SHA256 is
`00FB59B55EE77F948290393140C3A070610907271AF688A84B14CA25954EF511`.

All captures use ordinary release builds without `testing-hooks`, nine repeats,
the same per-pair sample scale, and all 49 pre-timing correctness checks.
Each workload runs separately in control/current and current/control order.
Windows uses verified process affinity `0x4`, Normal priority and unchanged
Balanced power policy; scheduled thread cycles are primary. Linux uses the
existing Arch WSL environment, native `/tmp` fixtures and thread CPU time as
primary; its Windows launcher affinity is not a Linux affinity measurement.
The source working directory remains on `/mnt/c`. Windows uses Rust 1.98.0;
the Linux environment reports 1.99.0-nightly (2026-08-05). Harness Rust-version
metadata is collected at run time, not embedded toolchain provenance.
No builds, source edits, archive compression or profiling overlap timings.

`source-origins-bench-1.ps1` preserves complete raw stdout/stderr and scheduling
events. `source-origins-summary-1.ps1` checks raw completion records, pair
metadata and measurement options before deriving ratios. Pipeline evidence is
`source-origins-timing-summary-1.json`: **116 completed invocations, 108 eligible
captures and 52 eligible pairs**. Eight rejected captures remain present; their
six affected pairs have no decision-grade ratio. No quality threshold changed.

### Pipeline costs before the cache-accounting reduction

Values below are percent changes in the platform's mean primary metric,
first/second order. Positive means more work/time, not improvement. `CV` means
that pair is excluded. Best-wall ratios and all absolute values remain in the
summary, including the raw rejected results.

| Workload | Windows cycles | Linux thread CPU |
| --- | --- | --- |
| Ordinary compilation | +10.40 / +9.28 | CV / +7.88 |
| 256 declarations | +6.07 / +5.75 | +7.77 / +5.51 |
| 4,096 comment lines | CV / CV | +0.13 / +0.90 |
| Straight-line cold lowering | +43.84 / +43.17 | +49.53 / +44.16 |
| Float-loop cold lowering | +47.13 / +47.11 | +51.59 / +50.07 |
| Verified compile-only cache lookup | +16.06 / +14.61 | +11.22 / +11.38 |
| Cold source API | +8.85 / +5.50 | CV / +9.22 |
| Warm source API | +2.98 / +3.94 | +1.00 / -2.08 |
| Small JIT execution | -0.01 / +3.71 | +4.05 / -0.22 |
| Small VM execution | CV / +2.14 | +4.21 / +3.50 |
| Quickened hot loop | +4.00 / CV | +4.91 / +3.73 |
| Function calls | +6.54 / +6.84 | +6.30 / +0.41 |
| Large uncached module graph | -0.64 / +0.60 | +5.39 / +1.19 |
| Large graph disk hit | -16.69 / -17.26 | +15.43 / +14.92 |
| Medium graph incremental rebuild | Windows bypasses | +8.92 / +8.53 |

The added origin construction/retention is required to repair diagnostic
correctness, not an accepted performance optimization. The cold-lowering
increase is substantial even though its absolute cost is microseconds.
These combined measurements do not attribute it to a particular allocation,
boxing conversion or pass. Successful execution also has measured costs;
error-only formatting alone does not prove a zero-overhead success path.

Disk hits are platform-dependent, not universally improved by the new sidecar.
For orientation, candidate large-hit mean wall time is 10.78–10.84 ms on
Windows versus 11.08–11.11 ms uncached; Linux is 1.355–1.356 ms versus
1.663–1.682 ms uncached. Earlier historical cache-policy percentages are not
current guarantees. No bypass threshold or capability rule changed here.
Cache-miss/store, individual restoration phases, allocation counts and peak
memory still require attribution; an uncached compile is not a cache miss/store
measurement.

### Removing repeated accounting for unchanged owners

Inspection found that `CompiledPrograms::compile` recomputed the inclusive
storage charge and enforced limits on every hit, even when its source owner
was unchanged. The existing source-rebinding method already avoided redundant
Arc cloning; adding another clone guard would not fix the measured cost.
The retained change instead reuses the charge for the exact same immutable
`Program` owner. A compile-only hit cannot install lazy code, change feedback,
grow scratch or alter that source. Execution, changed-source rebinding,
cloning and limit changes retain their existing accounting paths. Content and
authority lookup still precede the owner comparison.

Only `jit/compiled_cache.rs` differs between the two 149-entry snapshots,
including two new unit tests; see
`source-origins-cache-accounting-source-comparison-1.json`.
No new collection, retained field, unsafe operation or dependency was added.
The candidate is frozen as `source-origins-cache-accounting-1.exe` (SHA256
`D2FDA9D69A7BC49C76FEE49E1314144F48F8F71A1D34C53DD5FFD0DE2A14DEB3`)
and `source-origins-linux-cache-accounting-1` (SHA256
`B0D15FA77E917A2D09CFE5D89B40C2F5C4FCE750431C9FF6CA656145E9BC94B1`).
Its archive SHA256 is
`AA4925B6732C92B78C1F5545BB9C6BD8E83B2DED3A4182C82E77D64F6BDD49D6`.

Two clean lookup pairs reduce Windows mean cycles from 203.01 to 175.28 and
199.31 to 173.46 (**13.66% / 12.97%**); best wall time improves
14.20% / 13.05%. Linux mean thread CPU falls from 71.81 to 64.26 ns and
72.95 to 64.18 ns (**10.52% / 12.02%**); best wall improves
10.72% / 11.97%. This supports the narrow accounting reduction, not a Vec win.

Guardrails include cold lowering, warm source API, hot loops and function
calls on both platforms. The initial Linux cold pairs are -1.09% / +5.50%;
the latter remains eligible evidence, not discarded as noise. A bounded
two-order confirmation doubles cold samples to 320,000 iterations per repeat
and measures **+0.28% / +1.22%** CPU, with best wall +0.41% / +0.57%.
These small increases are documented costs/uncertainty, not a cold speedup or
a proven explanation for the earlier larger shift. Windows cold mean cycles
are -0.88% / -0.05%, while best wall is +1.60% / +0.45%.

One Windows hot-loop capture failed its 5% CV gate at 6.38%; its pair remains
excluded. Two longer confirmations (1,600 iterations per repeat) pass and
measure +1.47% / -1.30% cycles and +1.61% / -1.88% best wall. Other original
guard pairs range from -2.25% to +2.92% in the primary metric; no runtime
speedup is claimed. The cache reduction is retained for its repeatable lookup
benefit with these explicit guardrails and small cold tradeoffs; whole-pipeline
acceptance is still open.

The complete accounting evidence is
`source-origins-cache-accounting-timing-summary-2.json`: **48 completed
invocations, 47 eligible captures and 23 eligible pairs**, including every
initial result and the eight longer confirmation captures. All 48 invocations
pass all 49 correctness checks. The earlier `-1.json` summarizes the original
40 captures only; no files were overwritten to hide rejected evidence.

### Correctness, documentation and next work

The new tests cover unchanged original/cloned verified owners after lazy
lowering, quickening, profiling and scratch retention at the exact byte budget;
LRU recency; growth that evicts another entry; and later source shrinkage.
Existing tests still cover oversized rebinding, source-less equivalents,
owner release, authority separation, panic/error cleanup and cache cloning.

- Windows/Linux `cargo test --workspace --features testing-hooks --locked
  --offline --no-fail-fast`: **543/544 passed**, zero failed, 17 ignored,
  41 result groups (`source-origins-cache-accounting[-linux]-tests-1.log`).
- Windows `cargo clippy --workspace --all-targets --features testing-hooks
  --locked --offline -- -D warnings`: pass (`...-clippy-1.log`).
- Both rebuilt ordinary releases pass all 49 `--correctness-only` checks
  (`...-release-checks-1.log`, `...-linux-release-checks-1.log`).
- Changed/new Rust passes installed-nightly rustfmt; `git diff --check` passes.
  No new Miri, allocation-count, hardware-counter or RSS claim is made.

Reconfirmed the complete objective, global Router, Rust README and all four
examples; parent/source-directory searches found no additional agent files.
Read the architecture, memory, performance, contributing and CI guidance and
the relevant Rust API cache contract. The Router's optional stable Rust
documentation remained mandatory, including
[pointer identity](https://doc.rust-lang.org/stable/std/ptr/fn.eq.html),
[HashMap lookup](https://doc.rust-lang.org/stable/std/collections/struct.HashMap.html#method.get_mut),
and [Vec conversion](https://doc.rust-lang.org/stable/std/vec/struct.Vec.html#method.into_boxed_slice).
The Windows timer contract and .NET asynchronous process-output/window
documentation were checked for orchestration. No external-crate API or Ralloc
ownership rule changed.

Next: attribute and reduce the now-measured cold origin-construction costs,
including host allocations and the local-IR origin scratch footprint; inspect
successful dispatch overhead and isolate disk-cache restoration phases. Do
not repeat the unchanged Vec/Box experiment or substitute the lookup win for
these missing results. Continue the complete 40-section ledger; the goal
remains active and this is a progress checkpoint, not a final implementation
report.

## Origin preparation: allocations and paired action records

Historical checkpoint: the paired-record layout described below was subsequently
withdrawn after the [larger action-heavy comparison](#action-storage-scaling-paired-records-withdrawn).
Its measurements remain evidence, not a description of the current layout.

This checkpoint makes progress on cold lowering and the data-structure audit;
it does not complete the 40-section batch or establish that Vec is faster than
Box. Fixed installed tables remain boxed, and `JitOps` retains its private,
slice-only bounded-capacity policy. The unchanged Vec/Box experiment was not
repeated.

### Allocation attribution

The new ignored `tests/jit_preparation_profile.rs` diagnostic uses the existing
test-only counting `System` wrapper and Ralloc counters. Seven fixtures cover
print, benchmark-matched straight-line/float-loop/function sources, and raw
verified expression depths 4, 1,024 and 65,536. The raw fixtures have no source
metadata. VM output and JIT output agree before measuring. Each interval
constructs and drops fresh standalone code or a fresh compiled cache; immutable
fingerprint/storage memoization is primed outside the interval, not compiled
code reuse. Every interval checks balanced host allocations/deallocations,
zero Ralloc allocations/deallocations/live payload bytes, and only one compiled
chunk so functions remain lazy.

The pre-origin archive was extracted into the new isolated
`target/perf/full-system/origin-preparation-control-1` directory, without
switching or overwriting workspace source. Its preserved 145 entries and
archive SHA256 `B52C4FBB67790C0D76607D421AAA3910C27AE2CCC8F0313A05AC3F24E18232DA`
were checked. Only the identical new diagnostic was added; the host counter
already matched byte-for-byte. `origin-preparation-control-source-1.json`
records every extracted source hash and the added diagnostic hash.

`origin-preparation-profiles-summary-1.json` validates **252 samples**: three
identical repeats of 14 fixture/route profiles, on two platforms and three
source versions. Windows and Linux counts match in every recorded field.
Standalone counts below are allocations / reallocations; deallocations equal
allocations. The fresh-cache route adds one allocation in these observations.

| Fixture | Pre-origin control | Origin pipeline before cleanup | Paired actions |
| --- | --- | --- | --- |
| Print | 10 / 0 | 14 / 0 | 13 / 0 |
| Straight-line | 12 / 7 | 16 / 14 | 15 / 11 |
| Float loop | 11 / 2 | 16 / 6 | 15 / 5 |
| Functions, main chunk only | 11 / 3 | 16 / 8 | 15 / 6 |
| Depth 4 | 11 / 4 | 15 / 6 | 14 / 6 |
| Depth 1,024 | 11 / 20 | 15 / 30 | 14 / 30 |
| Depth 65,536 | 11 / 32 | 15 / 48 | 14 / 48 |

The origin pipeline adds four or five host allocations in these cases, not
Ralloc execution payloads. Inspection identifies the expression/action origin
buffers, installed primary/sparse origin storage and shared table owner. This
does not attribute every nanosecond of the previously measured regression to
allocation calls. Requested growth extents are also recorded, but they are
neither bytes actually copied nor peak/RSS/allocator-reserved memory.
The initial Windows diagnostic compile failed because it accessed two private
`Program` fields; it was corrected to use the existing accessors. The failed
`...windows-before-1.log` remains; `...windows-before-2.log` is the passing
baseline used in the summary.

### Smaller ownership boundary

`LocalBlock` previously stored `Vec<Action>` beside `Vec<OpOrigin>`, requiring
parallel pushes and a zip during encoding. The retained change introduces
`LocatedAction`, whose action and origin are rewritten together. Branch/jump
targets use the existing checked `BytecodeOffset`, preserving the full
262,144-instruction verifier range instead of truncating to 16 bits. Only the
final encoder converts that target to an executable index. No unsafe code,
dependency, runtime handler, new installed field or Ralloc path was added.

The current 64-bit layout changes from a 16-byte action plus a 12-byte origin
in separate buffers to a 24-byte paired record (12-byte action). The builder
header falls **224 → 200 bytes**. Removing the parallel stream saves one host
allocation in all seven fixtures, plus three/two/one reallocations in the
straight-line/function/float cases. Straight-line requested reallocation
extents fall 2,800 → 2,576 bytes; initial requested bytes fall 1,480 → 1,464.
Installed retained bytes and compiled-op counts are unchanged in every case.

The deep expression case still needs **1,048,576 bytes** of expression-origin
scratch. Its action-plus-origin scratch saving is only 16 bytes: the separate
origin diagnostic field shrinks by 48 bytes while action storage grows by 32.
`origin-preparation-layout-before-1.log` and `...layout-actions-1.log` retain
the complete capacity breakdown. The new record fixes one ownership/growth
stream, not the remaining large expression-origin footprint.

### Ordinary release timing and guardrails

The control is the preceding unchanged-owner-accounting release, not the
pre-origin allocation control. `origin-preparation-actions-source-comparison-1.json`
checks 149 → 150 source/config entries: only `jit/local.rs`, `jit/encoding.rs`
and `jit/lowering.rs` changed, and the allocation diagnostic was added.
The benchmark source remains SHA256
`00FB59B55EE77F948290393140C3A070610907271AF688A84B14CA25954EF511`.
Both releases were rebuilt without `testing-hooks` before freezing:

- Windows `origin-preparation-actions-1.exe`: SHA256
  `59158FDBBA7EB4F1D432C338AEC7B73C6E54D45970990268C6903A0652620496`.
- Linux `origin-preparation-linux-actions-1`: SHA256
  `ECCFC05BD3FD8E128158952D40F4E48F152E2FB36BD1965C9BC12CEA731C4368`.
- Source `origin-preparation-actions-source-1.zip`: SHA256
  `A8170C0AEC899F00AF3084AD8840DCE4BAD98847803F6F55564194C939D64270`.

Nine repeats and two reversed control/candidate orders use 320,000
straight-line and 160,000 float preparations per repeat. Windows retains
measured affinity mask 4, Normal priority and unchanged Balanced policy;
its primary metric is mean scheduled cycles. Linux's primary metric is mean
thread CPU nanoseconds. Rust toolchains remain Windows 1.98.0 and the existing
Linux nightly. Metadata/options match within every pair. Timings did not
overlap builds, source edits, allocation profiling or archive creation.

| Platform / cold fixture | Primary before → after, pair 1 | Pair 2 | Change, pairs 1 / 2 |
| --- | --- | --- | --- |
| Windows straight-line, cycles | 8,425.84 → 7,472.19 | 8,330.30 → 7,499.23 | -11.32% / -9.98% |
| Windows float, cycles | 6,210.56 → 5,688.53 | 6,192.26 → 5,729.02 | -8.41% / -7.48% |
| Linux straight-line, ns | 1,539.37 → 1,401.72 | 1,557.40 → 1,396.43 | -8.94% / -10.34% |
| Linux float, ns | 1,178.56 → 1,096.83 | 1,176.86 → 1,105.65 | -6.93% / -6.05% |

Windows hot-loop guards measure -0.25% / +0.70% cycles (best wall +0.20% /
+1.12%); function calls -0.66% / +1.30%. Linux function calls vary -8.12% /
-0.30%; these are guardrails, not a claimed function-execution optimization.
One Linux hot-loop capture fails the 5% CV gate at 8.21%, excluding its pair.
The other original pair measures -9.32% CPU. Two longer unrestricted
confirmations (3,200 iterations per repeat) disagree at **+6.61% / -0.99%**,
despite low within-run CV. The +6.61% result remains eligible evidence; it was
not discarded or relabeled as noise. Its exact cause is not established.

A separate bounded scheduling check restricts only new benchmark children to
Linux virtual CPU 2 using installed util-linux `taskset` 2.42.2. Preflight
reports the ordinary allowed list 0–11 and the restricted child's list `2`.
Two reversed-order pairs at 3,200 iterations measure **-0.69% / -0.67%** CPU
and -0.87% / -0.48% best wall, all below 0.63% CV. These matched-CPU guardrails
support retaining the cold-construction cleanup, but do not prove affinity
caused the unrestricted outlier or promise uniform performance under every
scheduler state. No system-wide affinity or power policy changed; a Linux
virtual CPU is not asserted to be Windows logical CPU 2.

Evidence preserves all **40 completed invocations**, each passing 49
correctness checks: 36 unrestricted/Windows captures in
`origin-preparation-actions-timing-summary-2.json` (35 eligible captures,
17 eligible pairs), plus four separately labeled captures in
`origin-preparation-actions-affinity2-timing-summary-1.json` (four eligible,
two pairs). The original `...timing-summary-1.json` retains the initial 32
captures. Different scheduling contexts are not pooled into one ratio.

### Verification, compliance and remaining work

The new action regression covers dead-store removal, true-branch elimination,
false-branch rewriting above the u16 range, all three fused failure stages,
and reuse across three blocks with distinct source offsets. Existing VM/JIT
location, lazy-call, cache, permission and numeric-boundary coverage still
passes.

- Windows/Linux `cargo test --workspace --features testing-hooks --locked
  --offline --no-fail-fast`: **544/545 passed**, zero failed, 18 ignored,
  42 result groups (`origin-preparation-actions[-linux]-tests-1.log`).
- Windows all-target workspace Clippy with `testing-hooks` and `-D warnings`:
  pass (`origin-preparation-actions-clippy-1.log`).
- Both ordinary releases pass all 49 `--correctness-only` checks before
  freezing (`...actions[-linux]-release-checks-1.log`).
- All changed/new Rust passes installed-nightly rustfmt; `git diff --check`
  passes. No new Miri, peak-memory, RSS or hardware-event profile is claimed.

Reconfirmed the objective, global Router, Rust rules/examples and relevant
architecture, memory and performance contracts; no additional repository
agent instructions were found. Earlier required contributing/CI/Ralloc gates
remain applicable; no allocator implementation changed. Checked stable Rust's
[Vec conversion/growth contract](https://doc.rust-lang.org/stable/std/vec/struct.Vec.html#method.into_boxed_slice),
[System allocator](https://doc.rust-lang.org/stable/std/alloc/struct.System.html)
and [allocation-observation limits](https://doc.rust-lang.org/stable/std/alloc/trait.GlobalAlloc.html#safety),
plus the upstream [taskset command contract](https://man7.org/linux/man-pages/man1/taskset.1.html).
No external-crate API was added or changed.

The measured cold improvement does not restore the whole diagnostic pipeline
to its pre-origin costs. Next work includes the large expression-origin
scratch, larger action-heavy preparation, successful-dispatch attribution,
disk-restoration phases and the remaining full-system ledger. The goal stays
active; this is a measured progress checkpoint, not the final batch report.

## Action storage scaling: paired records withdrawn

This checkpoint adds missing large-block coverage and withdraws the previous
paired-record optimization. It does not claim that Vec is faster than Box.
Fixed installed metadata stays boxed, `JitOps` retains its private slice-only
interface and less-than-4-KiB installation slack policy, and changing-length
builders/stacks stay vectors. The unchanged Vec/Box experiment was not repeated.

### Shared scaling inputs and allocation evidence

`tests/preparation/fixtures.rs` now owns raw verified expression-chain,
dead-store and alternating-copy inputs shared by the allocation diagnostic and
the new `tinylang_bench/lowering.rs` registration. Raw bytecode deliberately
bypasses frontend folding; these are supported verified-input shapes, not a
claim about their frequency in source-compiled applications. Input generation,
verification, and exact VM/JIT output checks are outside each timing interval.
Each interval constructs and drops a fresh compiled cache. No new timers,
production allocation counters, dependencies or unsafe code were added.

The diagnostic now covers ten fixtures, two construction routes and three
samples: 60 records per source/platform. The former `deep_65536` fixture is now
named `expression_chain_65536`; its shape is unchanged. Source text fixtures
and expression depths 4/1,024 remain. `lowering-scale-profiles-summary-1.json`
validates **240 samples**: repeats are identical, every recorded field matches
between Windows and Linux, and installed-op counts and retained-byte charges
match between layouts. Every interval balances host owners, leaves zero
Ralloc execution payloads, and compiles only the main chunk.

| Raw fixture | Bytecode / compiled ops | Separate allocations / reallocations | Paired allocations / reallocations |
| --- | --- | --- | --- |
| 1,024 dead stores | 2,051 / 4 | 15 / 40 | 14 / 30 |
| 65,536 dead stores | 131,075 / 4 | 15 / 64 | 14 / 48 |
| 32,768 alternating copy rounds | 131,075 / 65,539 | 15 / 64 | 14 / 48 |
| Expression depth 65,536 | 131,075 / 131,075 | 15 / 48 | 14 / 48 |

These are standalone counts; a fresh cache adds one allocation. The large
store-heavy cases reduce summed requested reallocation extents by 2,097,120
bytes with paired records, and initial requested bytes fall by 16. Neither
quantity is measured copying, peak memory or RSS. The existing 1-MiB deep
expression-origin scratch is not removed by pairing actions.

### Matched ordinary releases and the contrary timing result

The pre-pairing, post-origin/cache-accounting source archive was verified and
extracted into a new isolated `lowering-scale-control-1` directory. Only four
identical measurement files were overlaid, producing 152 source/config entries
without switching workspace source. `lowering-scale-control-source-1.json`
records the archive identity, overlay and all resulting hashes.
`lowering-scale-freeze-1.json` freezes both ordinary releases and confirms that
the harness matches, with production differences confined to `jit/local.rs`,
`jit/encoding.rs` and `jit/lowering.rs`. The common benchmark source SHA256 is
`0E8842A8F23E66B3DCF0BF420E2C7D7A9C46BC1D27174A6A81CA646F7960C138`.

Two reversed orders and nine repeats use 16,000 preparations per repeat for
1,024 stores and 320 for each large case (`--sample-scale 32`). Windows uses
actual affinity mask 4, Normal priority and unchanged Balanced policy; Linux
children use virtual CPU 2, confirmed through `/proc/self/status`. These are
separate scheduling contexts, not equivalent physical CPUs. Windows's primary
metric is mean scheduled cycles; Linux's is mean thread CPU nanoseconds.
Fixture roots and all paired metadata/options match. Timing did not overlap
builds, source edits, profiles or archive compression.

| Fixture | Windows primary change, orders 1 / 2 | Linux primary change, orders 1 / 2 |
| --- | --- | --- |
| 1,024 dead stores | +11.25% / +7.55% | +11.18% / +8.92% |
| 65,536 dead stores | rejected / +2.80% | +14.68% / +9.95% |
| 32,768 copy rounds | -0.08% / +4.54% | +4.81% / +9.53% |
| Expression depth 65,536 | +0.98% / -0.09% | -2.83% / -6.86% |

`lowering-scale-timing-summary-1.json` validates **32 completed captures,
31 eligible captures and 15 eligible pairs** against raw JSON and completion
records. All captures pass 49 printed checks and the four raw fixture checks
during registration. The first Windows large dead-store control has 11.38%
CV, exceeding the 10% limit; its entire pair is excluded. The second order
passes. No failed gate is silently discarded or used to support a ratio.

The smaller cold-case gains in the previous checkpoint remain valid evidence,
but do not outweigh consistent store-heavy regressions. A plausible contributor
is the widened action scan: optimizer passes read 24-byte paired records instead
of 16-byte actions, although they do not need origins. This is an inference
from layout and access patterns, not a hardware-counter attribution. Fewer
allocation calls and smaller combined scratch do not establish lower latency.

### Restoration and verification

The three candidate source files were checked against their frozen hashes
before restoring the measured separate-buffer production implementation.
The origin rewrite/reuse test remains, including eliminated stores/branches,
targets 65,536/262,143 and distinct fused failure stages. No unrelated edits
were reverted. `lowering-scale-restored-freeze-1.json` checks all 152 entries:
only that retained test differs from the control; production encoding and the
other files match. The ordinary restored binaries were rebuilt and frozen,
not substituted retroactively into the timing captures:

- Windows SHA256 `E682FBD7E2BBE2D3646C6C2E0AA50ABB7EC3D8C556A063E141EFD66A2AAA2C22`.
- Linux SHA256 `E1A39BD021F70A300AFDD7814E820F48CE5DD31929A7934A62733557967D6463`.
- Source archive SHA256 `D94B2601E6F0067598B6F6CE283BA9A44898E2C99F6B88F915CB443E8C7626AD`.

Windows/Linux full `testing-hooks` workspaces pass **544/545 tests**, zero
failed and 18 ignored across 42 result groups. Windows all-target workspace
Clippy with `-D warnings` passes. Both ordinary releases pass 49
`--correctness-only` checks; changed/new Rust formatting and `git diff --check`
pass. Evidence is in `lowering-scale-restored-tests-{windows,linux}-1.log`,
`...restored-clippy-1.log` and `...restored-release-{windows,linux}-1.log`.
No new Miri, hardware-counter, peak-memory or RSS result is claimed.

The initial harness compile needed the existing static benchmark-name owner.
A subsequent registry test hit an existing PID-based temporary-directory
collision; a fresh process-local fixture root passed all 15 tests without
changing filesystem-fixture behavior or deleting old data. Those failed logs
remain alongside the passing `lowering-scale-harness-tests-3.log`. The first
timing launcher failed before creating a child; its configuration-only log is
retained, while the Windows `...events-2.log` contains the actual captures.

The Rust Router/rules/examples and relevant architecture, memory, performance
and acceptance contracts were checked. Prior required Ralloc and external API
gates remain applicable; allocator implementation and external-crate APIs did
not change. Next work should evaluate action/origin ownership without widening
the action scan and investigate expression-origin scratch, using these scaling
fixtures alongside small cold cases. No replacement layout is presumed faster.
The broader 40-section goal remains active and incomplete.

## Compact action targets: smaller records rejected

This checkpoint tests a narrower alternative to paired action/origin records:
keep the separate private buffers, but represent action branch/jump targets
with the existing checked `BytecodeOffset`. The experiment is **rejected and
removed**. It adds contrary performance evidence, not a retained optimization
or a Vec-over-Box claim. Fixed installed metadata remains boxed; builders and
changing-length stacks remain vectors. `JitOps` still exposes only slices and
reclaims installation slack at 4 KiB. The unchanged container comparison was
not repeated.

### Scope and allocation result

Only `jit/local.rs`, `jit/lowering.rs` and `jit/encoding.rs` differed from the
restored baseline. The symbolic branch boundary used checked conversion;
final encoding recovered the host index. Origins stayed separate throughout,
and no installed representation, runtime handler, dependency, unsafe code or
Ralloc implementation changed. Existing full-range tests covered slots up to
65,535 and branch targets beyond 65,535, both outcomes, thresholds 0/1/8 and
repeated VM/JIT execution. The origin rewrite/reuse regression remained intact.

The explicit layout diagnostic measured `Action` 16 -> 12 bytes, with
`Expression=16`, `TempId=4` and `SymbolicLowerer=224` unchanged. This compiler's
smaller action stride is not a stable Rust ABI promise. Deep expression-origin
scratch still reserves about 1 MiB at depth 65,536; this candidate does not
solve that cost.

`action-targets-profiles-summary-1.json` validates 240 records: 120 new candidate
samples and 120 reused control samples from the preceding scaling experiment.
Control production matches the restored snapshot; the retained unit test does
not affect those profiles. All three repeats are identical, recorded fields
match across Windows/Linux, and installed operation counts and retained-byte
charges are unchanged. Host allocations/deallocations balance and Ralloc
execution payload allocation/deallocation/live-byte counts remain zero.

Allocation and reallocation **counts do not change**. Initial requested bytes
fall by 16 per construction. Selected standalone reallocation requests are:

| Fixture | Allocations / reallocations, both versions | Summed requested reallocation bytes, control -> candidate |
| --- | --- | --- |
| 1,024 dead stores | 15 / 40 | 311,024 -> 278,288 |
| 65,536 dead stores | 15 / 64 | 19,922,672 -> 17,825,552 |
| 32,768 copy rounds | 15 / 64 | 21,757,652 -> 19,660,532 |
| Expression depth 65,536 | 15 / 48 | 14,679,840 -> 14,679,840 |

Fresh-cache construction adds one allocation on both sides. These are requested
extents, not bytes copied, simultaneous peak memory or RSS. The large store
cases save 2,097,120 requested reallocation bytes without making every workload
faster.

### Frozen comparison and all timing outcomes

`action-targets-freeze-1.json` checks all 152 source/config entries and confines
the source difference to the three files above. Both variants use the same
expanded benchmark harness, SHA256
`0E8842A8F23E66B3DCF0BF420E2C7D7A9C46BC1D27174A6A81CA646F7960C138`.
Controls are the ordinary releases in `lowering-scale-restored-freeze-1.json`,
not the rejected paired-record binaries. The candidate artifacts are:

- `action-targets-current-windows-1.exe`, SHA256
  `8CFA19A667FA1B782189ECC0C72A8C6E2E12BAF95608914C2C66EC7851166200`.
- `action-targets-current-linux-1`, SHA256
  `6E408ECCB41123E68575634478A5B88C507462AC5BD6704C58A7878C95DB406D`.
- `action-targets-current-source-1.zip`, SHA256
  `716F815C0C039B2D4793488F5710384D796D1F183242D4FC795B609BEB80418A`.

Eight workloads run in two reversed variant orders with nine repeats. Per
repeat, small stores use 16,000 preparations, each large raw case 160,
straightline 320,000, float lowering 160,000, hot loops 3,200 and calls 4,800.
Windows uses actual mask 4, Normal priority and unchanged Balanced policy;
Linux children use virtual CPU 2 and native `/tmp` fixtures. These are distinct
scheduling contexts, not equivalent physical CPUs. Windows's primary metric
is mean scheduled cycles; Linux's is mean thread CPU nanoseconds. No timing
overlapped builds, edits, allocation profiles or archive compression.

| Workload | Windows primary change, orders 1 / 2 | Linux primary change, orders 1 / 2 |
| --- | --- | --- |
| 1,024 dead stores | rejected / -2.94% | -5.27% / -5.61% |
| 65,536 dead stores | rejected / rejected | -4.64% / -5.83% |
| 32,768 copy rounds | -3.95% / -5.83% | **+18.65% / +8.35%** |
| Expression depth 65,536 | -3.30% / +1.02% | -3.25% / -2.90% |
| Straightline cold lowering | -1.58% / -0.84% | -5.16% / -2.59% |
| Float-loop cold lowering | -0.43% / -0.51% | +0.31% / +0.27% |
| Hot loop | +1.98% / +1.63% | -1.84% / -1.50% |
| Function calls | -0.05% / -0.50% | -4.57% / +0.87% |

`action-targets-timing-summary-1.json` validates **64 completed captures,
60 eligible captures and 29 eligible pairs** against planned/start/completion
records, raw schema-3 JSON, saved baselines, correctness output and paired
metadata/options. Every invocation passes 49 printed checks plus four raw
VM/JIT checks during registration. Windows small-store control order 1 has
26.08% CV. Large-store order 1 has 11.52%/10.73% CV, and its order-2 candidate
has 11.29%. All three complete pairs are excluded under the unchanged 10%
limit; hot rows retain the stricter 5% limit. Raw rejected captures remain.

Linux copy-store mean CPU time rises 7.361 -> 8.734 ms in the first order and
7.278 -> 7.886 ms in the reversed order. Best wall time also worsens by
20.31%/8.88%, with all four CVs below 3%. These raw verified workloads bypass
frontend folding; their frequency in source applications is not established,
but the supported input regression is repeatable. Small cold-case gains do
not justify it. Windows hot-loop changes are adverse too, not a neutral guard.
The first Windows copy-store best wall time is +0.62% despite lower mean cycles;
the differing metrics and mixed call/expression outcomes are not suppressed.

The planned longer Windows store/hot confirmation was not run after the Linux
regression repeated: it could not justify retaining this candidate. No extra
captures were used to obtain a favorable verdict. The exact cause of the
Linux regression is unproven; a smaller record or lower allocation-byte total
does not establish a faster optimizer loop. No hardware-counter attribution
is claimed.

### Restoration, verification and continuation

All candidate source hashes were rechecked before removing only the three
known changes. All 152 source/config entries then matched the prior restored
snapshot byte-for-byte, including the retained origin test and scaling harness.
No unrelated edits or saved evidence were removed. Both candidate workspaces
had passed 544/545 tests; the restored Windows/Linux workspaces pass those same
counts again, with zero failures and 18 ignored tests across 42 result groups.
Windows all-target workspace Clippy with `-D warnings` and formatting checks
for all 67 changed/new Rust files pass. Logs use the
`action-targets-restored-tests-{windows,linux}-1.log`,
`action-targets-restored-clippy-1.log` and `action-targets-restored-fmt-1.log`
names under `target/perf/full-system`.

Both rebuilt ordinary releases pass all 49 `--correctness-only` checks.
`action-targets-restored-audit-1.json` records the final source, test and artifact
checks. The rebuilt Windows executable is frozen separately as
`action-targets-restored-windows-1.exe`, SHA256
`D693ACAF55DDCF4201130A2FB35A10A5CA9EBE622062E825F81DDB0D1A015F59`;
it was not substituted into earlier timing pairs. The rebuilt Linux executable
matches the prior restored binary byte-for-byte, SHA256
`E1A39BD021F70A300AFDD7814E820F48CE5DD31929A7934A62733557967D6463`.
The already checked source archive
`lowering-scale-restored-source-1.zip` is reused (SHA256
`D94B2601E6F0067598B6F6CE283BA9A44898E2C99F6B88F915CB443E8C7626AD`)
because all 152 entries are identical. All build/verification and benchmark
process handles returned terminal results and are closed.

The full objective, global Router, Rust rules/four examples and relevant
architecture, memory and performance contracts were checked. Required prior
Ralloc/API gates remain applicable; no allocator or external-crate API changed.
The official [Rust layout contract](https://doc.rust-lang.org/stable/reference/type-layout.html)
and checked integer-conversion documentation were consulted before the
prototype. No new Miri, peak-memory or RSS result is claimed.

Do not repeat the unchanged Vec/Box, paired-record or compact-target experiment.
Further lowering work needs a different measured mechanism, such as the
remaining dense expression-origin scratch or successful-path diagnostic work,
and must retain large copy/store coverage alongside small source workloads.
The broader 40-section goal remains active and incomplete.

## Consecutive expression origins: scratch savings rejected

This checkpoint tests the remaining dense per-expression origin scratch. A
private consecutive-prefix representation saves backing storage, but repeats
4.79%/5.28% deep-expression and 12.54%/11.57% late-gap lowering regressions on
Linux. The prototype is **rejected and removed**. This is contrary performance
evidence, not an accepted optimization or a Vec-over-Box result. Fixed installed
metadata stays boxed; growing builders/stacks stay private vectors. `JitOps`
keeps its slice-only interface and installation spare-byte bound below 4 KiB.

### Mechanism and retained measurement coverage

Only `jit/local.rs` and `jit/encoding.rs` differ between the frozen candidate
and common control. The prototype stores a first offset while expressions have
consecutive bytecode origins. The first gap materializes the prefix into a
dense vector, which then retains capacity across block reuse. Checked offset
reconstruction returns the existing typed result rather than introducing a
production panic. Installed origins, runtime handlers, verifier limits, Ralloc
implementation, dependencies and unsafe code are unchanged.

The layout diagnostic measures `Action=16`, `Expression=16` and `TempId=4`
unchanged; the builder header grows 224 -> 232 bytes. At depth 65,536, total
origin scratch reservation falls 1,048,624 -> 48 bytes: one MiB of expression
origins disappears, while action origins remain. Expression and traversal
backing remain 4 MiB and 2 MiB respectively. These are observed compiler
layouts and reserved capacity, not stable Rust ABI or measured process peaks.

Three measurement-only changes are common to both variants and remain after
restoration:

- `tests/preparation/fixtures.rs` adds a stored depth-65,536 expression chain
  ending with Store/Load/Print/Halt. It has 131,077 bytecode and installed ops,
  and output `0\n`. The skipped Store origin forces a late gap after a long
  consecutive prefix. The existing chain shares its domain-local builder.
- The benchmark's expected registry includes
  `jit.codegen_stored_expression_chain_65536_cold`; the shared large-fixture
  registry now runs five VM/JIT checks outside timing.
- The ignored layout diagnostic assigns each instruction's actual bytecode
  offset before lowering. Its old default-zero origins were unsuitable for
  evaluating a consecutive-origin representation.

These raw verified fixtures bypass source folding; their frequency in source
applications is not established. They expose supported-input scaling and do
not stand in for the full source-workload baseline.

### Allocation evidence

`expression-origins-profiles-summary-1.json` validates **264 new samples**:
11 fixtures, standalone/fresh-cache routes, three repeats, two variants and
two platforms. All 88 profiles repeat identically, every recorded field
matches across Windows/Linux, and installed-op counts and retained-byte
charges are unchanged. Host allocations/deallocations balance. All Ralloc
execution-payload allocation/deallocation/live-byte assertions remain zero;
construction installs only main and preserves lazy functions.

Selected standalone results are:

| Fixture | Allocations / reallocations, control -> candidate | Initial requested bytes, control -> candidate | Summed reallocation request bytes, control -> candidate |
| --- | --- | --- | --- |
| Print | 14 / 0 -> 13 / 0 | 606 -> 590 | 0 -> 0 |
| Expression depth 1,024 | 15 / 30 -> 14 / 20 | 78,462 -> 78,446 | 229,152 -> 196,416 |
| Expression depth 65,536 | 15 / 48 -> 14 / 32 | 4,981,374 -> 4,981,358 | 14,679,840 -> 12,582,720 |
| Stored expression depth 65,536 | 16 / 50 -> 16 / 34 | 4,981,482 -> 5,505,762 | 14,680,064 -> 12,582,944 |
| Copy rounds 32,768 | 15 / 64 -> 15 / 64 | 4,850,302 -> 4,850,302 | 21,757,652 -> 21,757,652 |

The late-gap fallback raises initial requested bytes by 524,280 while reducing
summed reallocation requests by 2,097,120, a combined reduction of 1,572,840.
The dense copy/dead-store cases keep all allocation fields unchanged. These
are requested extents, not bytes copied, simultaneous peak memory or RSS.
Reducing growth calls and requested bytes does not establish faster lowering.

### Frozen comparison and timing decision

`expression-origins-control-freeze-1.json` checks all 152 source/config entries
and the three measurement-only differences from the preceding restored audit.
`expression-origins-consecutive-freeze-1.json` confines the candidate difference
to the two files above. Both use the same harness, SHA256
`7A15508109E1B58D123C16918390795E49ECB3653E39578BD5CBAA21D30CF920`.
All measured binaries are ordinary releases with testing hooks disabled.

Control artifacts under `target/perf/full-system`:

- `expression-origins-control-windows-1.exe`, SHA256
  `DFDBF7C09E19A54099307EA9EF35DBECA74DBD59AC141B2E2894DB8DE0EA3A4E`.
- `expression-origins-control-linux-1`, SHA256
  `2BF81E9C2AF88A070C08F26042A8F813D50FEC3A5321B3BFBB01B45892F555B0`.
- `expression-origins-control-source-1.zip`, SHA256
  `0983C80C6E1465CBD2E3EDD2F7A4B75FC65D583DC3C69640726774696D157FAC`.

Rejected candidate artifacts:

- `expression-origins-consecutive-windows-1.exe`, SHA256
  `AA52A588DEFE2F344F0B5A7D6EEA23DE75EF6FAE5ECC64C7796928C6B370A9B6`.
- `expression-origins-consecutive-linux-1`, SHA256
  `569EC9585E49750F4DCAA0EEA3F342B2ADF9699A9F9807F9D856307F33EBA3A6`.
- `expression-origins-consecutive-source-1.zip`, SHA256
  `CFC67CD8FB62DB8E986006C885C105E8E388ECBB7B01C040A2715C636E0B916F`.

Only the Linux scaling screen ran: three workloads, two reversed variant
orders, nine repeats and sample scale 32 (320 preparations per repeat).
Children use virtual CPU 2, unchanged Balanced host policy and native fixtures
in `/tmp/tinyone-expression-origins-c7XJkPff`. Mean thread CPU nanoseconds are
the primary metric, not TSC cycles. No timing overlaps builds, source edits,
allocation profiles or archive compression.

| Workload | Order | Mean CPU milliseconds, control -> candidate | CPU change | Best wall-time change |
| --- | --- | --- | --- | --- |
| Copy rounds 32,768 | 1 | 7.151 -> 7.029 | -1.70% | -1.04% |
| Copy rounds 32,768 | 2 | 6.900 -> 8.283 | +20.04% | +19.82% |
| Expression depth 65,536 | 1 | 3.256 -> 3.413 | +4.79% | +5.10% |
| Expression depth 65,536 | 2 | 3.241 -> 3.413 | +5.28% | +5.22% |
| Stored expression depth 65,536 | 1 | 3.176 -> 3.574 | +12.54% | +13.37% |
| Stored expression depth 65,536 | 2 | 3.217 -> 3.590 | +11.57% | +12.08% |

`expression-origins-timing-summary-1.json` validates **12 completed and eligible
captures / six eligible pairs**, all CVs at or below 1.56%, against plan/start/
completion events, raw schema-3 JSON, saved baselines and matching metadata/
options. Every invocation passes 49 printed correctness checks, plus the five
raw VM/JIT checks during registration. The events log and all raw captures are
preserved; only the derived summary's experiment label was corrected.

The two target regressions repeat in CPU and best wall time, which suffices to
reject the prototype. Copy-store results are mixed, not a consistent gain or
regression. Windows scaling and all planned guardrail batches were not run
after rejection; no Windows latency result is claimed. The exact mechanism of
the regression is unproven. Reconstruction/checking and late materialization
are hypotheses, not hardware-counter attribution.

### Restoration and verification

The candidate's three focused tests cover consecutive and gapped origins,
repeated/descending offsets, block reuse, large offsets and nonwrapping width
failure. Candidate Windows/Linux workspaces pass 547/548 tests; all-target
Windows Clippy, formatting, allocation assertions and both ordinary-release
correctness checks pass. The three private prototype-specific tests are
removed with the rejected owner. Existing origin rewrite/reuse and full-range
branch tests remain.

Both candidate source hashes were checked before restoration. All 152 current
source/config entries then match the common control byte-for-byte, retaining
the three measurement improvements and all unrelated work. Fresh restored
Windows/Linux workspaces pass **544/545 tests**, zero failures and 18 ignored
tests across 42 result groups. Windows all-target workspace Clippy with
`testing-hooks` and `-D warnings`, all 67 changed/new Rust formatting checks,
and both ordinary-release 49-check runs pass. Logs use
`expression-origins-restored-{tests,release}-{windows,linux}-1.log`,
`expression-origins-restored-clippy-1.log` and
`expression-origins-restored-fmt-1.log`.

`expression-origins-restored-audit-2.json` is the authoritative restoration
audit. The first manifest had a null formatting-log hash because silent
`rustfmt` success did not create a `Tee-Object` file; it is preserved and
superseded. The repeated unchanged formatting check explicitly records its
command, 67-file count and exit 0. The second audit rejects missing evidence
and invalid hashes. Initial sandbox WSL access was denied before tests ran;
approved access completed both Linux checks using a fresh native fixture root.

The rebuilt Windows executable is frozen separately as
`expression-origins-restored-windows-1.exe`, SHA256
`913480893129508DEAC73E09C0D91C09C87266F444ACD1DA562997CE4E594042`.
It was not timed or substituted into a prior pair. The Linux rebuild matches
the frozen control binary exactly. Since all 152 entries match, the checked
control source archive is reused without another duplicate. All verification
and timing handles returned terminal exit 0 and are closed.

The full objective, global Router, Rust rules/four examples, contributing
rules and relevant architecture/memory/performance contracts were checked.
Prior Ralloc/API gates remain applicable; no allocator or external-crate API
changed. The official [Vec API](https://doc.rust-lang.org/stable/std/vec/struct.Vec.html),
[checked arithmetic](https://doc.rust-lang.org/stable/std/primitive.usize.html#method.checked_add)
and [Rust layout contract](https://doc.rust-lang.org/stable/reference/type-layout.html)
were consulted before the prototype. No new Miri, OOM, peak-memory, RSS or
hardware-counter evidence is claimed.

Do not repeat this unchanged consecutive-prefix experiment, the Vec/Box
comparison, paired action records or compact action targets. Further work
needs a different measured mechanism or attribution of actual lowering passes
and successful-path diagnostic cost, with the expanded scaling fixtures kept
alongside source workloads. The broader 40-section goal remains active and
incomplete.

## Lowering source attribution: diagnostic limits preserved

The preceding turn made progress by rejecting consecutive-origin compression,
restoring its control and keeping the late-gap measurement fixture. This turn
did not rerun that candidate or the unchanged Vec/Box comparison. It first
recorded five symbol-enabled Linux cold-preparation invocations to investigate
work still performed after verification.

The full objective, global `C:/Users/conne/.codex/AGENTS.md` Router, Rust rules
and all four examples, contributing rules and relevant architecture, memory
and performance documentation were checked. No repository-local `AGENT.md` or
`AGENTS.md` was found outside generated/build directories. Prior Ralloc/API
documentation gates remain applicable; this experiment changes neither Ralloc
nor an external-crate API. The official
[Cargo profile](https://doc.rust-lang.org/cargo/reference/profiles.html) and
[environment override](https://doc.rust-lang.org/cargo/reference/environment-variables.html)
contracts, [perf record](https://man7.org/linux/man-pages/man1/perf-record.1.html),
[report](https://man7.org/linux/man-pages/man1/perf-report.1.html) and
[script](https://man7.org/linux/man-pages/man1/perf-script.1.html) documentation
were consulted. The subsequent prototype uses the documented
[Rust slice pattern](https://doc.rust-lang.org/stable/reference/patterns.html#slice-patterns)
to select a single-block path once.

`lowering-attribution-freeze-1.json` freezes all 152 source/config hashes,
the checked existing source archive and `lowering-attribution-symbol-linux-1`
(SHA256 `CECD6F668D65696E01B4A7F8894008001629A6B405AD900865A56EF0B687F620`).
This separate target directory uses the existing Linux nightly 1.99.0
(2026-08-05), ordinary release optimization, line-table debugging and no
stripping. Testing hooks are disabled; normal release configuration is
unchanged. The build's 49 correctness checks passed before sampling.

The five serial captures use `cycles:u`, 499 Hz, `dwarf,4096`, no inherited
tasks and no build-ID cache, with the child pinned to CPU 2. Each runs all 49
printed correctness checks, nine repeats and sample scale 64. The shared raw
fixture registry also checks its five fixtures outside timing. No build,
allocation profile, compression or other sample collection overlaps these
recordings. They cover the whole invocation, including correctness, setup and
warmup, not just the timed preparation loop.

`lowering-attribution-summary-1.json` validates every planned/start/record/end
event, all exported trace hashes, correctness/options and matching symbol/
source report sample and period totals. The reports show:

| Fixture | Samples | `lower_blocks` self-period share | Remap entry-line share |
| --- | ---: | ---: | ---: |
| straightline | 2,407 | 25.54% | 0.12% |
| float loop | 1,772 | 26.76% | 0.27% |
| copy stores 32,768 | 22,151 | 56.51% | 1.60% |
| expression chain 65,536 | 9,702 | 42.73% | 4.19% |
| stored expression chain 65,536 | 9,515 | 43.06% | 3.80% |

These are diagnostic attribution hints, not exact phase costs or expected
speedups. Inlining folds optimizer/encoder work into `lower_blocks`, and
`jit/op.rs:587` is the `remap_targets` entry line, not the entire remapping
operation. Several limitations prevent a stronger claim:

- Every header report warns `Invalid HEADER_EVENT_DESC: nre=1 sz=144 (min 64)`;
  the event description is unreadable and header sample timestamps/duration
  are zero. The decoder reports zero lost samples, but that does not repair
  the header or prove complete attribution. The exact cause is unproven.
- All five `run_benchmark` parent-filtered reports are empty. Saved caller
  and direct-decode probes show unusable/truncated caller information, so
  the reports cannot isolate the timed phase. A frame-pointer alternative
  was considered but was not built or sampled.
- The copy trace contains 10.37% unresolved kernel-labelled records despite
  the user-cycle configuration. Those records and unresolved libc addresses
  are left unattributed. Generic monomorphized symbol aliases are not used
  to infer concrete allocation classes.
- Native temporary traces were absent at a later check. Exports were saved
  immediately and their hashes still match capture events; independent
  native/export hash equality was not recorded. The cause of the native
  directory's disappearance was not established.
- Benchmark JSON reports an unknown Rust version; the separate frozen build
  manifest/log identifies the toolchain. These instrumented invocations are
  not substituted for ordinary-release acceptance measurements.

The raw `.data`, logs, flat reports, empty parent-filtered reports, unfiltered
source reports and caller/decode probes remain under `target/perf/full-system`.
Sampling/report handles 52427, 30251 and 5502 and build handle 76633 completed
with exit 0 and are closed. No profiler warning is silently discarded.

## Single-block map and remap bypass rejected

Source inspection identified a concrete redundant buffer independent of the
profiler limitations: `lower_blocks` always allocated `code.len() + 1` host
offsets, filled block ranges and scanned installed operations to remap targets.
Every verified target starts a block. With one block, its only entry is zero;
the verifier's required final Halt/Return additionally makes executable
one-block chunks branch-free. Encoding-only self-branch fixtures can target
zero, which already has its compiled coordinate.

Two boundary tests were added to the common control before changing production
code. They cover successful and failing single-block execution against the VM
at thresholds 0/1/8 with repeated runs, plus encoding-only unconditional and
fused conditional zero-target branches. Both pass on the original lowering.
The candidate selected the one-block case once, skipped the map/remap pass,
and extracted the unchanged per-block body into one private function. No
public API, new container, unsafe code, JitOp layout, source mapping, runtime
handler, cache policy or Ralloc behavior changed.

Control and candidate freezes are `single-block-control-freeze-1.json` and
`single-block-current-freeze-1.json`. The former adds only the two tests to
the previously restored source; its production prefix was compared against
the prior source archive. The latter differs only in `jit/chunk.rs`, with
identical benchmark/fixture sources. All binaries were copied after their
builds completed. Their manifests preserve complete source/archive/binary
hashes; the allocation control explicitly reuses 132 prior unchanged-control
samples, rather than describing them as fresh measurements.

`single-block-profiles-summary-1.json` validates 264 allocation samples:
132 new candidate plus 132 reused control, eleven fixtures, two routes,
three identical repeats and both platforms. Installed operation counts,
retained charges and reallocations are unchanged. Single-block fixtures
remove one allocation and its matching deallocation, saving exactly
`8 * (bytecode_ops + 1)` initially requested bytes on these 64-bit targets:

| Fixture | Removed requested bytes per preparation |
| --- | ---: |
| print | 32 |
| straightline | 208 |
| dead stores 1,024 / deep expression 1,024 | 16,416 |
| dead stores 65,536 / copy stores / expression chain | 1,048,608 |
| stored expression chain | 1,048,624 |
| multi-block float / functions | 0 |

These count host System allocation requests, not peak memory, RSS or bytes
copied. Every profiled interval asserts zero Ralloc payload activity and
balanced host allocation/deallocation counts.

Ordinary-release measurements comprise 48 captures and 24 eligible pairs:
Linux scaling, Windows scaling and Windows guardrails. Each invocation passes
49 correctness checks and uses nine repeats. Orders are control/candidate
then candidate/control, with sample scale 32 for scaling and 8/32/64 for the
documented guardrails. Windows processes run at Normal priority with affinity
mask 4; Linux children use CPU 2. Balanced power policy is unchanged. These
platform affinity labels are distinct, not a claim of physical-CPU equivalence.
No builds, edits, allocation profiles, compression or sampling overlap timing.

`single-block-timing-summary-1.json` checks plans/start/end records, metadata,
raw JSON, saved baselines and existing CV limits. No capture is discarded or
retried for its result. Primary changes below are candidate versus control;
negative means less work/time. Windows uses scheduled cycles, Linux mean
thread CPU time; best-wall changes and CVs remain in the summary.

| Workload | Linux order 1 / 2 | Windows order 1 / 2 |
| --- | ---: | ---: |
| copy stores 32,768 | +7.77% / -1.26% | -10.99% / -8.28% |
| expression chain 65,536 | -14.59% / -15.46% | -7.70% / -7.25% |
| stored expression chain 65,536 | -10.91% / -8.88% | -6.84% / -8.56% |
| dead stores 1,024 | not run | -13.51% / -3.67% |
| dead stores 65,536 | not run | -3.55% / -3.04% |
| straightline cold | not run | -3.19% / -0.83% |
| multi-block float cold | not run | +0.35% / +0.84% |
| warm hot loop | not run | -2.93% / +0.04% |
| warm function calls | not run | **+5.79% / +4.35%** |

Despite cold-preparation gains and lower allocation requests, warm function
calls regress in both eligible Windows orders. Their control/candidate CVs
are 1.46%/0.78% and 1.21%/1.14%; best wall time also regresses 6.27%/4.27%.
The production prototype is rejected under the whole-system acceptance rule.
The precise cause of this cross-path timing change was not established; it
is not attributed to a new call instruction or claimed to be allocator latency.
Linux guardrails were not run after this rejection.

The first sandboxed Windows scaling launcher failed before starting any child
when its process-environment property returned null. Its configuration-only
log remains. Approved launcher attempt 2 uses a fresh fixture root and runs
the complete planned batch; no timing or correctness result is attributed to
attempt 1. Successful timing handles 53787, 87126 and 13196 returned terminal
exit 0 and are closed.

The candidate's 152 source/config hashes were checked before restoration.
Production lowering is restored byte-for-byte to the common control, retaining
both boundary tests and unrelated work. Fresh full workspace suites pass
**546 Windows / 547 Linux tests**, zero failures and 18 ignored tests across
42 result groups. All-target workspace Clippy with testing hooks and
`-D warnings`, the 67-file changed/new Rust formatting check, and both
ordinary-release 49-check runs pass. Verification handles 51767 and 2848
completed with exit 0 and are closed.

`single-block-restored-audit-1.json` validates the restoration, comparison
evidence and logs. It reuses the checked control source archive, records the
fresh Windows rebuild separately (not timed), and verifies the Linux rebuild
is byte-identical to its frozen control. Logs use
`single-block-restored-{tests,release}-{windows,linux}-1.log`,
`single-block-restored-clippy-1.log` and `single-block-restored-fmt-1.log`.
No new Miri, OOM, RSS, peak-memory or normal-release hardware-counter
acceptance evidence is claimed.

The user's Vec/Box policy remains unchanged: fixed installed metadata is
boxed, changing-length builders/stacks retain private vectors, and `JitOps`
exposes slices with less than 4 KiB spare operation capacity per installation.
This trial neither proves Vec faster than Box nor justifies a container swap.
Do not repeat the unchanged bypass or rejected layout trials. A future retry
needs new evidence explaining the warm-call cost or a materially different
implementation. Precise lowering-phase attribution and the broader 40-section
baseline, allocator/cache audits, cleanup and final report remain incomplete.

## Scoped preparation attribution with observer limits

This continuation adds a bounded diagnostic, not an optimization candidate.
The preceding turn rejected and restored the single-block bypass. Its sampled
caller chains could not separate local passes from encoding, so no unchanged
Vec/Box, layout, origin-prefix or single-block trial was repeated.

### Scope and documentation

Re-read the complete 40-section plan, global AGENTS Router, Rust rules and all
four concrete Rust examples; no repository-local AGENT/AGENTS file was found.
The applicable contributing, architecture, performance and memory contracts
were consulted. This work does not change Ralloc, allocator dependencies,
runtime handlers, cache policy, instruction layout or installed containers.
The only external derive is the existing `thiserror` 2.0.18, whose
[versioned documentation](https://docs.rs/thiserror/2.0.18/thiserror/) was checked.
The recorder follows the checked
[Instant duration contract](https://doc.rust-lang.org/stable/std/time/struct.Instant.html#method.checked_duration_since),
[thread-local borrow/lifetime contract](https://doc.rust-lang.org/stable/std/thread/struct.LocalKey.html)
and [conditional-compilation rules](https://doc.rust-lang.org/stable/reference/conditional-compilation.html).

`jit/preparation_profile.rs` owns a 17-entry fixed measurement array, allocated
as one boxed record only for an explicit current-thread capture. There are
no per-span allocations, maps, atomics or retained timers in compiled chunks.
Inactive captures do not read the clock. The module, imports and call sites
are feature-gated out of builds without `testing-hooks`; the hooks build is
not asserted to have the same code generation as an ordinary build.

Typed capture state rejects nesting before invoking the nested closure.
Callbacks run outside TLS borrows. Checked duration/counter errors invalidate
the profile instead of silently saturating it. RAII releases capture state
when the operation returns or unwinds. Phase transitions exclude preceding
record-update work from the next phase. The outer wall interval includes
unattributed caller, accounting, scratch/owner destruction and observer work;
capture allocation and report extraction lie outside it.

The spans distinguish setup/map filling, symbolic construction, unreachable
encoding, stack/definition finalization, all seven ordered local passes,
encoding, target remapping and origin/chunk installation. A skipped
slot-version pass records zero calls. Four source fixtures moved, unchanged,
into `tests/preparation/sources.rs`, shared by allocation and phase diagnostics.
The five existing raw timing fixtures are unchanged.

### Verification and frozen provenance

Five recorder unit tests cover dense phase keys, inactive/reset behavior,
nesting, returned errors, panic cleanup, thread isolation and clock/overflow
failures. Four integration tests cover phase counts, unreachable blocks,
installed op/byte parity, first-call-only lazy compilation and repeated lazy
runtime failures at thresholds 0/1/8. All compare appropriate VM/JIT results.
An initial integration compile failed because the test accessed private
`JitOptions` fields; it now uses the existing public builder. The failed and
passing focused logs are both retained.

Full workspace suites pass **555 Windows / 556 Linux tests**, with zero
failures and 20 ignored tests across 43 result groups. Both all-target
workspace Clippy runs with hooks and `-D warnings`, both all-target checks
without hooks, the 71-file changed/new Rust formatting check and
`git diff --check` pass. Each rebuilt ordinary release passes all 49 printed
correctness checks. These ordinary binaries were not benchmarked here and
are not claimed byte-identical to the prior control.

`preparation-phases-freeze-1.json` freezes eight executables (clock, phase,
allocation and ordinary-release harnesses on each OS) and 155 source/config
hashes in `preparation-phases-source-1.zip`. Relative to the prior 152-entry
restored audit, exactly seven existing files differ and three are added.
All fixture constants match the prior archive exactly. Windows uses Rust
1.98.0; Arch Linux WSL uses nightly 1.99.0 dated 2026-08-05. No dependency,
feature default or profile configuration was changed. A parenthesization
error in the initial freeze script was corrected before any executable or
source archive was frozen; it did not invalidate a collected measurement.

Windows and Linux captures run sequentially from the frozen executables,
with no overlapping builds, edits, compression or other profiles. Windows
uses affinity mask 4 and Normal priority; Linux uses `taskset --cpu-list 2`.
The existing Balanced power policy is unchanged. Event logs record start,
finish, scheduling checks and output hashes. Each platform uses a fresh
fixture directory. Capture handles 89839 and 24780 completed with exit 0.

### Results and limits

`preparation-phases-summary-1.json` validates **308 timed batch records and
5,236 phase records**: eleven fixtures, two construction routes and seven
alternating active/inactive samples per OS. These are two phase-harness
invocations, not 308 independent acceptance runs. Every phase sum matches
its accounted total and stays within its capture interval; phase counts are
stable across repetitions and platforms. Only fresh main preparation is
timed; lazy lowering is tested for attribution but has no latency baseline
in this capture.

All **132 new allocation records match the 132 reused control records** in
`expression-origins-profile-{windows,linux}-control-1.log`, including host
allocation/deallocation/reallocation counts, requested bytes and buckets,
installed op counts and retained bytes. Every interval retains the balanced
host-owner, lazy-installation and zero-Ralloc-payload assertions. The inactive
observer and fixture extraction did not change these measured counts.

Fourteen empty-span calibration samples report mean total/recorded costs of
**77.02/35.48 ns on Windows** and **42.95/18.23 ns on Linux**. The recorded
floor is substantial relative to trivial phases. Nothing is subtracted from
the raw measurements. Same-hooks-binary standalone observation changes print
totals by +107%/+165% and float-loop preparation by +139%/+191% on
Windows/Linux. These are observation effects, not runtime regressions or
ordinary-release profiler overhead estimates.

Large standalone records give the following coarse attribution, as percentages
of the full observed capture (not normalized to the sum of named phases):

| Fixture | Symbolic construction, Windows / Linux | Encoding, Windows / Linux | Slot versions, Windows / Linux |
| --- | --- | --- | --- |
| 65,536 dead stores | 68.27% / 75.22% | 3.65% / 3.03% | Not run |
| 32,768 copy stores | 56.76% / 61.35% | 8.39% / 6.66% | 7.47% / 8.99% |
| 65,536 expression links | 44.56% / 53.75% | 28.83% / 25.98% | Not run |
| 65,536 links with stored result | 43.44% / 53.67% | 29.76% / 25.88% | Not run |

These are inspection leads, not exact normal-release pass costs or projected
speedups. Eighteen of the 44 route/platform summaries have active or inactive
total CV above 5%. Windows deep-1,024 standalone CV is 23.46%/25.90%; Linux
print is 25.18%/39.93%. Windows expression-chain symbolic construction has
18.13% phase CV, despite its useful coarse separation from encoding.
Individual phase variation is retained in the JSON. Negative observed deltas
(for example Windows copy-store -5.76%, with inactive CV 10.33%) are not
treated as gains or calibrated overhead. No retry, relaxed acceptance gate,
or selective latency acceptance is used.

The evidence now supports inspecting symbolic construction and deep-chain
encoding before another container/layout change. It does not explain the
earlier warm-call regressions or replace ordinary-release paired timing.
Precise caller/drop costs, lazy preparation timing, frontend/verifier
breakdowns, full memory/Ralloc audits, cache acceptance and the rest of the
40-section plan remain open. No new Miri, OOM, peak/RSS, fragmentation or
hardware-counter result is claimed.

The user's Vec/Box rule remains enforced: fixed installed metadata is boxed,
builders and changing-length stacks own private vectors, and `JitOps` exposes
only slices with less than 4 KiB spare operation storage per installation.
The Vec-versus-Box timing recheck remains inconclusive. This diagnostic
neither claims a Vec speed advantage nor changes the container policy.

## Collection-prefix screening not retained

The preceding diagnostic identifies symbolic construction as a large part of
observed preparation, with the observer limits above. Inspection found that
all three existing collection matchers require a leading `Load`, but every
instruction is offered to them. This trial adds only one early leading-`Load`
screen in `collection_superinstruction`; no IR, container, runtime handler,
allocator, dependency or build-profile change accompanies it. The complete
plan, Rust Router/rules/examples and applicable architecture, VM, bytecode
and measurement contracts were consulted before the trial.

One new regression test exercises all 36 current portable opcodes at offsets
0/1/4 for the three collection windows, including each truncated window.
It passes on the existing implementation before adding the screen. These
mutated windows test matcher preconditions, not execution validity. Existing
fusion, source-origin and full VM/JIT suites provide behavior coverage.

### Provenance and allocation evidence

`collection-prefix-control-freeze-1.json` reuses the exact ordinary Windows
and Linux executables and matching 155-entry source/config archive from
`preparation-phases-freeze-1.json`. That source archive predates the new test.
`collection-prefix-current-freeze-1.json` freezes the candidate's two ordinary
executables, two separate allocation executables and its own source archive.
Only `jit/chunk.rs` differs; its production delta is exactly the leading-Load
guard and its explanatory comment. The remainder is the new boundary test.
Both candidate workspace suites and Clippy gates pass before freezing.

Separate allocation runs produce **132 new records**, compared with 132
reused control records from
`expression-origins-profile-{windows,linux}-control-1.log`. The preceding
phase audit already verifies those controls against its frozen inactive
observer. All host allocation/deallocation/reallocation counts, requested
extents, installed operation counts and retained bytes are unchanged. The
final audit explicitly verifies all 44 control/candidate bucket comparisons
as well as numeric fields. Repeats and platforms agree; balanced host-owner
and zero-Ralloc-payload assertions pass. These are allocation diagnostics,
not peak/RSS, memory-copy or latency measurements.

### Paired target timing

All **16 ordinary-release captures are eligible**, forming eight pairs:
two target workloads, two alternating orders and two operating systems.
Each invocation uses nine repeats, scale 32, all 49 printed correctness
checks and the raw registration parity assertions. The same frozen control
and candidate binaries are used throughout. No builds, edits, allocation
profiles, compression or other timing overlap these captures. Linux uses
CPU 2 and a fresh native `/tmp` fixture directory; Windows uses affinity
mask 4, Normal priority and fresh native fixtures. Balanced power policy
is unchanged. All capture handles finish with exit 0.

Primary deltas below are candidate minus control: mean thread CPU time on
Linux and mean scheduled cycles on Windows. Negative is faster. Order 1 is
control first; order 2 is candidate first. CV columns list control/candidate.

| Platform / target | Order 1 primary delta | Order 2 primary delta | Order 1 CV | Order 2 CV |
| --- | --- | --- | --- | --- |
| Linux / dead stores 65,536 | -2.64% | -7.09% | 2.35% / 1.65% | 5.07% / 6.43% |
| Linux / copy stores 32,768 | -21.94% | +0.91% | 1.39% / 0.40% | 0.37% / 0.45% |
| Windows / dead stores 65,536 | -1.33% | +0.63% | 9.91% / 6.48% | 7.17% / 9.11% |
| Windows / copy stores 32,768 | +1.60% | +0.11% | 7.67% / 8.94% | 8.50% / 6.99% |

Linux dead-store gains repeat, but the large first copy-store gain does not.
Eligible within-invocation variation does not explain that between-invocation
change. Windows dead-store direction reverses, and neither copy-store pair
shows a gain. The small Windows deltas relative to 6–10% CV do not establish
a precise regression either. There is no repeatable cross-platform target
benefit sufficient to retain this change. The planned deep-chain scaling
and runtime guardrail suites are therefore **not run** on either OS; they
are not claimed as passed. No retries or relaxed gates force a conclusion.
`collection-prefix-timing-summary-1.json` preserves exact values, wall-time
metrics, matching metadata, raw JSON parity and nonoverlapping intervals.

### Restoration and remaining scope

The guard is removed. Only the new matcher boundary test remains relative
to the preceding checkpoint. `collection-prefix-restored-audit-1.json`
verifies the production source against the control and all 155 current
source/config hashes. It checks all 465 entries across control, candidate
and restored source archives. All measured binaries and raw logs remain
unchanged. The restored Linux ordinary binary hashes exactly to the frozen
control; the newly linked Windows binary is separately frozen and was not
timed or substituted into the comparison.

Restored workspaces pass **556 Windows / 557 Linux tests**, with zero
failures and 20 ignored tests across 43 result groups. Both all-target
Clippy runs with hooks and `-D warnings`, both default-feature all-target
checks, all 71 changed/new Rust formatting checks and `git diff --check`
pass. Each ordinary rebuilt release passes all 49 printed correctness
checks. Initial restoration launchers stopped when PowerShell treated
compiler progress on native stderr as terminating; the second attempts
check native exit status and pass under separate log names. This launcher
issue occurred after timing and did not alter any measurement.

The requested Vec/Box rule is unchanged: fixed metadata stays boxed,
growable builders/stacks stay private vectors, and installed `JitOps`
exposes slices with less than 4 KiB spare operation storage per chunk.
The prior container comparison remains inconclusive; no new Vec speed
claim follows from this trial. Do not repeat this unchanged guard or the
rejected container/layout trials. Broader frontend/verifier and workload
baselines, local-IR/runtime acceptance, memory/Ralloc/cache audits, cleanup
and the full 40-section report remain incomplete.

## Ralloc ownership on unwind and native exhaustion

The preceding turn completed restoration of the rejected collection-prefix
screen. This continuation advances the outstanding allocation-failure and
cleanup audit instead of repeating that guard or the Vec/Box comparison.
The full objective was reread and all 155 previous source/config hashes
matched `collection-prefix-restored-audit-1.json` before editing.

### Documentation and reproduced defects

Applicable Router/Rust rules and examples, contributing/CI, memory-model,
architecture and allocator-boundary documentation were consulted, together
with Ralloc's inline API, arena, block, region, backend, synchronization and
C ownership contracts. Ralloc remains the existing local 0.2.0 crate, with
no production external dependencies added or changed. The referenced
`docs/clippy_doc_lints.md` backlog is absent; no separate Ralloc README was
found. Rust's [unwinding cleanup guidance](https://doc.rust-lang.org/stable/nomicon/exception-safety.html),
[drop-in-place safety](https://doc.rust-lang.org/stable/std/ptr/fn.drop_in_place.html),
[ownership transfer with forget](https://doc.rust-lang.org/stable/core/mem/fn.forget.html),
[aligned dangling pointers](https://doc.rust-lang.org/stable/core/ptr/struct.NonNull.html#method.dangling)
and [catch-unwind limits](https://doc.rust-lang.org/stable/std/panic/fn.catch_unwind.html)
were checked before implementing the relevant changes. The
[official Miri workflow](https://github.com/rust-lang/miri) was also checked.

New regressions fail against the prior implementation:

- `RallocBox::drop` frees backing only after `T`'s destructor returns.
  An intentional unwind therefore frees a nested byte-buffer field but
  leaks the outer allocation. Both ordinary and raw-recovered 4,096-aligned
  cases record one deallocation for two allocations.
- The zero-sized constructor returns a handle without consuming the incoming
  value's destructor obligation. A nonpanicking counter observes one drop
  during construction when zero are expected; a panicking zero-sized value
  unwinds before the test reaches the intended drop boundary.

`allocator-unwind-control-tests-windows-1.log` and
`allocator-unwind-control-zero-sized-windows-1.log` preserve those failures.
They are correctness reproductions, not latency comparisons.

### Retained changes

`RallocBox` now transfers zero-sized ownership without early destruction.
A private stack-local storage owner is installed before invoking a value's
destructor; it frees backing after field cleanup on normal return or unwind.
It neither catches a production panic nor promises cleanup on abort. No
allocation is added for the guard, and the public handle's fields,
C ABI, arena strategy and byte-buffer representation are unchanged.

Four unit tests cover allocated-value and field unwinds, nested ownership,
over-alignment, raw recovery, zero-sized unwinding, and zero-sized normal,
raw and `into_inner` transfers. Existing native-box tests now use per-test
drop counters instead of one global counter that independent tests reset.
This removes a concrete shared-state race in the tests, not an allocator
optimization. All ten native-box tests pass with eight test threads.

`tests/allocation_failure.rs` adds one process-exclusive test over the real
fixed pool. Its only preparation compile error was use of the private
`Value` alias; the test now uses the existing public `RuntimeValue` type.
The failed and passing logs are retained; no API visibility was widened.

On both Windows and Linux, 58 held 512-KiB buffers plus the original test
owners account for 31,457,408 live usable payload bytes. This intentionally
leaves no region able to serve another 512-KiB request; it is artificial
pressure, not representative peak memory or host allocation failure.
Failed 1-MiB byte-buffer and move-only VM-allocation growth preserve exact
pointers, lengths and byte contents. Failed TinyMemory cloning preserves
the original frame's first/last values. All failed-growth intervals balance
new native allocations and deallocations.

The same pressure then triggers main-frame, callee-frame, callee-buffer and
array-growth failures. For each fixture, portable VM and JIT thresholds
0/1/8 each run twice: **32 failures per OS**. Output, complete error text
and locations match; every interval returns native live bytes to its prior
level. Releasing pressure permits **32 successful recovery runs per OS**,
reusing the same JIT instances and checking complete cleanup again. Final
native allocation/deallocation totals balance with zero live payload.

### Verification and scope

Both current hooks workspaces pass **561 Windows / 562 Linux tests**, zero
failures and 20 ignored tests across 44 result groups. Both all-target
Clippy runs with hooks and `-D warnings`, both default-feature all-target
checks, all 73 changed/new Rust formatting checks and `git diff --check`
pass. Both ordinary rebuilt releases pass all 49 printed correctness checks.
Both no-default-feature Ralloc C-library checks pass with isolated target
directories and a temporary `panic=abort` profile override; no repository
profile or feature default changes.

Strict-provenance Miri passes the four new unit tests, the broader 45-test
unit subset, and 26 native-box/buffer/initialization/fragmentation tests.
The exact all-arena 8-KiB exhaustion case also completes with terminal exit 0
from its original session 45330, observed at 2026-09-09 09:51:53 UTC. That is
**72 distinct Ralloc tests** (76 executions because the four focused tests
also occur in the broader subset), including all 46 current unit tests.
The exhaustive test reports 2,886.36 seconds and a final progress marker of
570 million basic blocks. It was not restarted or stopped after an arbitrary
timeout. Strict provenance and default validation, aliasing and leak checks
remain enabled. This safety-tool runtime is not ordinary allocator latency;
no full-system Miri or new latency result is claimed.

`allocator-ownership-freeze-1.json` records all 156 current source/config
hashes and verifies every entry of `allocator-ownership-source-1.zip`.
Relative to the preceding restored checkpoint, only the two Ralloc source/
test files above change and the core exhaustion test is added. Both ordinary
release binaries are separately frozen with testing hooks disabled; neither
was timed or substituted into earlier experiments. Completed evidence logs
are hashed, while the then-active exhaustion log was explicitly excluded.
The separate `allocator-ownership-miri-completion-1.json` records its terminal
result and final log hash, rechecking the original checkpoint, current source,
archive contents, binaries and completed evidence without rewriting the freeze.
The first two freeze attempts stopped before creating
an archive or binary copy: one could not read the actively written log with
an exclusive reader, and one lacked the explicitly loaded ZIP assembly.
Both failed logs are retained; the corrected third attempt completes with
exit 0. These checkpoint-script failures invalidate no timing capture.

These are accepted ownership corrections and safety evidence, not a new
Vec-over-Box speed result. Fixed installed metadata stays boxed, growable
private builders/stacks retain Vec, and installed JitOps keep the bounded
slice-only policy. Broader runtime/memory baselines, allocator performance
and fragmentation acceptance, frontend/verifier coverage, cache audits,
cleanup and the full 40-section report remain incomplete.

## Current runtime allocation census and next target

The following continuation reread the full objective, rechecked all 156 frozen
source/config hashes and monitored the same live exhaustion test to completion.
It also ran the existing release `allocation_profile` diagnostic on both
platforms, with testing hooks, `--locked --offline`, `--ignored --nocapture`
and one test thread. Both original native handles exit 0. No Rust source or
configuration changes in this continuation.

`allocator-ownership-runtime-profile-audit-1.json` preserves **120 current-source
records**, 60 per platform: 15 sources, thresholds 0/8, first/repeated execution.
All 60 cross-platform counterparts match in heap, allocator, runtime, execution
and JIT counters, including all 32 size buckets and 14 alignment buckets.
Every interval balances native allocations/deallocations, ends with zero live
payload and no allocation failures, and agrees on runtime/Ralloc growth and
copied-byte counters. Histograms cover every successful native allocation.
The raw logs, two executed hooks-build binaries and audit script are hashed;
the ordinary frozen binaries and original source checkpoint remain unchanged.

Selected first-execution records at threshold 8 (identical on both platforms):

| Workload | Native allocations | Peak usable payload bytes | Allocation header visits |
| --- | ---: | ---: | ---: |
| Small script | 1 | 64 | 1 |
| 512 direct calls | 2 | 256 | 3 |
| Repeated depth-11 recursion | 201 | 832 | 1,513 |
| 256 retained strings | 261 | 24,768 | 31,507 |
| 256 retained cells | 261 | 33,024 | 31,916 |
| 4,096 allocate/free cell cycles | 2 | 192 | 3 |

The string case contains 256 requests in (16,32] bytes; the cell case contains
256 requests in (32,64] bytes. Each also has five frame/container requests, and
all requests use 16-byte alignment. Arrays and maps each grow four times,
copying 5,376 and 10,752 bytes respectively in their 256-element fixtures.
These are per-fixture request distributions, not a weighted production mix.

The long-lived cases expose repeated first-fit scans even at small live
payloads. In contrast, the churn case's 4,096 logical heap allocations/frees
use the existing one-spare-cell path (`TinyHeap::alloc_cell`, `free`, and
`shutdown`), explaining its two native allocations including frame memory.
Adding a competing pool on the strength of the logical churn count would
therefore target work already removed. Scan-prefix reuse is a candidate to
evaluate against retained-object workloads, not an implemented or accepted
allocator optimization. Placement, alignment, fragmentation, failure cleanup
and normal low-occupancy cases must remain guarded.

Coverage limits remain explicit: `struct_enum` currently runs the enum fixture,
not a standalone struct-heavy source. Counts do not attribute object ages or
host allocation sizes, separate deallocation latency, measure fragmentation
or RSS, or replace the missing broader frontend/verifier/workload baselines.
The profile builds/runs overlapped the independent Miri test; their results
are work counts only, with no timing claim. Required next evidence includes
ordinary-release retained-object scaling and missing workload/phase coverage.
The full 40-section goal remains active and incomplete.

## Retained-object scaling: allocated-prefix hint withdrawn

This continuation adds a shared retained-object workload family and evaluates
one bounded first-fit hint. **The hint is rejected and removed.** The retained
changes are three fixture/measurement files, benchmark registration and four
allocator placement tests. All 159 current source/config hashes match the
frozen control; allocator production code matches the ownership checkpoint.
This is additional measurement and regression coverage, not an accepted
allocator optimization or a Vec-over-Box result.

### Gates and reproducibility

The complete objective, global AGENTS router, Rust rules and four concrete
examples were read. No additional repository/ancestor agent guidance was
found. Applicable architecture, memory, performance, contributor, CI and audit
documentation, allocator source/API/header contracts and retained-object syntax
were inspected. Rust's [strict-provenance model](https://doc.rust-lang.org/stable/std/ptr/index.html#strict-provenance),
[`with_addr` contract](https://doc.rust-lang.org/stable/std/primitive.pointer.html#method.with_addr)
and [aliasing guidance](https://doc.rust-lang.org/stable/nomicon/aliasing.html)
were consulted. TinyOne remains 1.5.1 and Ralloc 0.2.0; no dependency, feature,
public API or native-code backend was introduced.

Artifacts below live in `target/perf/full-system/`. The control and candidate
`scan-prefix-*-freeze-1.json` manifests each cover 159 source/config entries,
verified source archives, two ordinary release binaries, workspace suites,
Clippy and release correctness. The control additionally verifies that the
arena production body is identical to the ownership checkpoint. Instrumented
profile executables are frozen separately from ordinary timing executables.

### Shared sources and placement coverage

`tests/allocation/retained.rs` defines cells, strings, standalone structs and
enums retained in a root vector until value/length readback and shutdown. This
closes the earlier enum-only `struct_enum` fixture gap. A private bounded
fixture enum selects the source and golden checksum before generating loops.
The benchmark adds 24 warm VM/JIT rows at 16/256/4,096 objects and four cold
JIT rows at 256 objects. Compilation and golden VM/JIT checks occur outside
timing; every interval includes a fresh heap, retained reads and cleanup.
Cold JIT additionally includes lowering and compiled-owner drop.

The new ordinary regression checks 0/1/16 objects against golden VM output
and JIT thresholds 0/1/8, twice per JIT instance. The ignored scaling diagnostic
checks 0/1/16/256/4,096 objects at thresholds 0/8, first/repeated execution:
80 records per platform and source variant. Its reset is process-exclusive.

Four allocator tests pass on control, candidate and restored code: small early
holes after larger requests; over-aligned leading-space reuse; full-region
exhaustion/rejected requests/recovery; and split-hole/backward-coalescing reuse.
They check placement, live-owner statistics and final coalescing. No new
production testing API or alternate allocator remains.

### Measured scan work and the failed hypothesis

`scan-prefix-{control,candidate}-profile-audit-1.json` preserves **320 records**
in total. Each variant has 80 exactly matching Windows/Linux counterparts,
including complete raw opcode records and request histograms. Across variants,
every recorded counter except allocation header visits is identical. Every
interval balances native allocations/frees, ends with zero live payload and
has no allocation failures. Runtime and Ralloc growth/copy totals agree.
These are instrumented work counts, not latency or RSS measurements.

At 4,096 retained objects, every case performs 4,103 native allocations:

| Objects | Control header visits | Hint header visits | Visits removed |
| --- | ---: | ---: | ---: |
| Cells | 7,917,554 | 7,897,057 | 0.26% |
| Strings | 7,793,918 | 7,765,240 | 0.37% |
| Structs | 8,150,503 | 8,138,191 | 0.15% |
| Enums | 8,029,652 | 8,013,247 | 0.20% |

The prototype kept one private typed offset before which all blocks were
allocated, advanced it only across proven allocated blocks and rewound it on
free. It preserved first-fit placement, alignment padding and earlier small
holes. Arena/optional-arena layout grew 40 -> 48 bytes; the block header stayed
16 bytes, with no per-allocation metadata change, new allocation or new lock.

At 16 objects, visits fell 33-56%; that did not extrapolate to long-lived
scaling. Code inspection explains the limit: growth of the root vector frees
an early block; subsequent same-sized object allocations leave an undersized
free remainder. The hint cannot skip that still-free block, so later requests
again scan almost the complete live prefix. The dominant quadratic scan work
remains. A request-aware search boundary would require separate placement,
alignment and coalescing proofs; it is not implemented or accepted here.

### Ordinary Windows timing and rejection

The frozen runner uses nine repeats, scale 8, Windows processor affinity 0x4,
Normal priority and unchanged Balanced power policy. It runs control/candidate
then candidate/control for each object kind, seven rows per invocation.
No builds, source/docs edits, profiles, Miri, compression or other benchmark
runs overlap latency capture. All 16 invocations complete 49 printed checks
(784 passes), plus 12 untimed retained-fixture golden/parity checks each.

`scan-prefix-windows-timing-summary-1.json` audits 16 captures: **10 eligible,
six variance-rejected, 28 fully eligible paired rows**. Rejected complete
groups and their partners are excluded, not rescued using individual rows.
The launcher terminal exit 1 correctly records quality rejection, not a
correctness failure. Rejections are candidate strings pair 1 (16.85% CV),
candidate enums pair 1 (12.01%), both structs pair 2 (11.00%/24.30%), and both
enums pair 2 (36.07% and 12.44%/23.34%). All exceed the unchanged 10% limit.

Selected eligible paired mean scheduled-cycle changes (negative is less):

| Workload | Control-first pair | Candidate-first pair |
| --- | ---: | ---: |
| VM retained cells, 256 | +2.02% | +2.61% |
| JIT retained cells, 256 | -0.27% | +0.90% |
| Cold JIT retained cells, 256 | -0.51% | -0.11% |
| VM retained cells, 4,096 | +0.37% | -0.27% |
| JIT retained cells, 4,096 | -1.15% | -1.09% |
| JIT retained structs, 4,096 | -0.17% | Excluded |
| JIT retained strings, 4,096 | Excluded | -1.79% |

These modest/mixed results do not establish a useful general runtime benefit
for the additional state, and the counter evidence shows the intended scaling
cost was not removed. The hint is therefore withdrawn. Additional allocator
guardrail timing and Linux latency were not pursued after this rejection;
neither is claimed to pass. Frozen Linux builds and counts are not Linux
performance evidence. Do not repeat this unchanged allocated-prefix trial.

### Safety, restoration and remaining scope

Control, candidate and restored hooks workspaces each pass **566 Windows /
567 Linux tests**, 21 ignored, across 45 result groups. Both restored all-target
Clippy runs with warnings denied, default-feature workspace checks, no-std
C-library checks and ordinary release runs (49 checks each) pass. All 76
changed/new Rust files pass formatting; `git diff --check` passes.

The candidate passes 50 allocator unit tests under strict-provenance Miri,
including exact full-pool exhaustion, plus 26 native box/buffer, initialization
and fragmentation tests. Default aliasing, validation and leak checks remain
enabled. The four new placement tests additionally pass Miri on the restored
implementation. Together with the unchanged production allocator's previous
72-case Miri checkpoint, this covers 76 distinct allocator tests; it is not a
full TinyOne Miri claim. Interpreter test durations are not performance data.

Two launcher mistakes remain recorded: the first Linux control command selected
stable and stopped on unavailable toolchain components before compiling; it
was rerun with installed nightly, without changing toolchains. After the
candidate's 50 Miri unit tests passed, an incorrect `vm_buffers` test target
stopped the follow-up command; `native_buffer` was then selected and all four
integration groups passed. Neither failure is hidden or counted as a pass.

The source restoration and completed evidence are tracked by
`scan-prefix-restored-audit-1.json`. Fixed installed metadata remains boxed;
growing builders/stacks remain private vectors; installed `JitOps` exposes
only slices and retains less than 4 KiB spare storage per chunk. The existing
Vec/Box timing comparison remains inconclusive and was not repeated. Wider
runtime/allocation baselines, allocator improvements, frontend/verifier work,
whole-system acceptance and the final cleanup/report remain open. The full
40-section goal is active and incomplete.

## Aligned-header initialization and Vec/Box safeguards

The next allocator search experiment was paused before production changes: a
new full-scan placement oracle exposed a correctness defect in the unchanged
allocator. This checkpoint fixes that defect; it does not install a search
hint or claim an allocator or container speedup.

### Reproduction and ownership correction

`aligned-header-control-freeze-1.json` verifies 160 source/config entries and
the matching source archive. Relative to the previous restored checkpoint,
only a test-module declaration and three new tests were added; the production
allocator was byte-identical. The isolated regression failed with
`InvalidRegion` instead of a usable size of 16 on Windows, Linux and
strict-provenance Miri. All three control commands completed with exit 101;
the Miri failure was a test assertion, not an interpreter UB diagnostic.

The case fills a 64-byte allocation with `0x5a`, keeps an adjacent live barrier,
frees the first allocation, then requests 16 bytes aligned to 32. On these
64-bit test targets, alignment leaves a 48-byte leading free block and an
exact 32-byte allocation block. Its header lands inside the former payload.
When there is no splittable tail, `allocate_from_block` previously set only
the allocated flag, leaving the old payload bytes as the block's size.

The no-tail-split branch now initializes the selected header from the full
available block size before marking it allocated. The existing split branch,
placement arithmetic, coalescing, layout, APIs and allocation-search strategy
are unchanged. Arena storage remains 40 bytes and the reserved block header
remains 16 bytes on the tested targets. No new container, pool, dependency,
per-arena state or runtime instrumentation was introduced.

The mixed test performs four 256-step sequences, checking exact first-fit
placement, metadata integrity, alignment, usable extent, unchanged live
payload bytes and complete final coalescing. Its scan always starts at the
arena base; it deliberately reuses the unchanged slot/alignment arithmetic,
so it is not an independent proof of that arithmetic. Large alignment fit
counts can differ with the backing address. A separate backward-coalescing
test covers revisiting an earlier hole after it grows.

Reconfirmed the full objective, global Router, Rust rules and all four required
examples, current allocator/block code, Ralloc 0.2.0 configuration and the
memory/performance contracts. The unchanged subsystem/supplemental gates are
recorded in the preceding allocator checkpoints. Checked the official
[pointer-write initialization contract](https://doc.rust-lang.org/stable/std/ptr/fn.write.html),
[byte initialization](https://doc.rust-lang.org/stable/std/ptr/fn.write_bytes.html),
[borrowed slice safety](https://doc.rust-lang.org/stable/std/slice/fn.from_raw_parts.html)
and [provenance-preserving address changes](https://doc.rust-lang.org/stable/std/primitive.pointer.html#method.with_addr).
Ralloc still has no production external dependencies. The safety comment
states why the selected aligned slot is writable and why initialization is
required even when the storage was used before.

### Completed checks and limits

- Windows/Linux hooks workspace suites pass **569/570 tests**, with 21 ignored
  tests across 45 result groups. Each includes all 53 allocator unit tests,
  including exact full-pool exhaustion. All seven first-fit tests pass.
- Both all-target hooks Clippy runs with warnings denied, default-feature
  workspace checks, no-std C-library checks and rebuilt ordinary releases
  pass. Each ordinary release passes all 49 printed correctness checks.
- Fresh strict-provenance Miri passes **52 unit and 26 integration tests**,
  including the new reproduction and mixed oracle, native boxes/buffers,
  initialization and fragmentation. Default aliasing, validation and leak
  checks remain enabled. Only the exact full-pool unit case is filtered out;
  its previous 48-minute Miri completion is historical, not coverage of the
  changed production code. This is not a full TinyOne Miri result.
- All 77 changed/new Rust files pass formatting and `git diff --check` passes.
  An initial PowerShell launcher failed to parse an interpolated variable
  before starting any check; correcting the delimiter allowed both native
  verification sequences to complete. No failed check is counted as passing.

Fresh evidence uses `aligned-header-*-1.log`; the control failures retain their
original filenames in the control manifest. Source/binary/evidence hashes are
recorded in `aligned-header-fixed-audit-1.json`. These are correctness results,
not latency measurements. Earlier timing and allocation profiles have not been
relabelled as measurements of this fix.

### Requested Vec/Box confirmation

The renewed request was handled as an ownership/evidence check, without
repeating the unchanged inconclusive timing experiment. Before the allocator
fix, `vec-box-safeguards-current-3.log` passes 71 JIT tests (six diagnostics
ignored); `vec-box-layout-current-3.log` explicitly passes three storage
diagnostics. The subsequent full suites cover the same ordinary safeguards.
Current Windows layout is `JitOp=24`, `JitChunk=96`, `JitProgram=208`, with an
eight-byte optional detailed-profile owner. Call-vector backing is 0, 352,
704 and 1,408 bytes at depths 0, 1, 8 and 16, versus 1,408 bytes reserved at
every depth by the maximum-depth boxed-option alternative. This is a storage
comparison, not a boxed-stack latency result or a comparison against every
possible boxed design.

Fixed installed certificates, origins, chunk slots, edge counters and feedback
remain boxed. Changing-length builders and stacks remain private vectors with
their existing ownership, cleanup and retention bounds. Installed `JitOps`
remains the explicit exception: its private Vec exposes only slices, never a
growth API, and installation retains less than 4 KiB of spare operation storage
per chunk; the full capacity is charged. The
[boxed-slice conversion contract](https://doc.rust-lang.org/stable/std/vec/struct.Vec.html#method.into_boxed_slice)
confirms how larger spare extents are reclaimed. The previous timing recheck
was inconclusive, so this does **not** prove that this exception is faster than
Box. Container changes still require a concrete ownership or measured benefit,
not a blanket preference for Vec.

The allocator hint remains unimplemented. Wider full-system baselines,
optimization acceptance, cleanup and final reporting remain open; the complete
40-section goal is active.

## Size-aware first-fit prefix trial: completed measurements, acceptance pending

This is a distinct experiment from the withdrawn allocated-prefix-only hint.
It addresses the measured early-small-hole scan cost without changing first-fit
placement. The source and ordinary release binaries are frozen, all planned
measurements are complete, and the candidate remains **provisional**, not an
accepted full-system optimization. The Windows churn comparison is mixed.

### Mechanism, ownership and correctness

Two additional placement tests were first run on the unchanged production
allocator: an equal-sized request must revisit an earlier free block, and an
alignment-rejected hole must remain available to a later ordinary request.
The control retains the preceding aligned-interior-header initialization fix.

The candidate adds a private `SearchPrefix` containing a block-boundary offset
and an upper bound on every free block before it. A request can start at that
boundary only when its required block size is **strictly greater** than the
bound; otherwise it starts at the arena base. Rejected free blocks and leading
alignment fragments contribute to the bound. Success records the actual end
of the allocated block, including an unsplittable tail; exhaustion records the
arena end. Freeing rewinds the boundary to the actual leftmost coalesced free
block. Keeping an overestimated bound after rewind can cause extra scans but
cannot skip a fitting earlier block. Invalid requests do not change the hint.

The test-only integrity walk additionally checks that the boundary is in range,
does not bisect a block, and has no preceding free block larger than its bound.
The base-start placement walk, mixed allocation sequences, backward coalescing
and alignment-header regression all remain enabled. The placement oracle
shares the existing alignment arithmetic; it is not an independent proof of
that arithmetic.

Only `crates/tinyone_ralloc/src/arena.rs` differs between the frozen control and
candidate. There is no new container, allocation class, pool, dependency,
public API, per-block metadata or production unsafe block. On the tested 64-bit
targets, `Arena` grows from **40 to 56 bytes**, while the reserved block header
stays **16 bytes**. The four native-pool arena owners therefore add 64 bytes;
this is not a whole-process metadata total, since other arena owners can exist.

The complete objective, global Router, Rust rules and four required examples
were reconfirmed. Allocator/block code and the memory/performance contracts
were checked; the preceding allocator checkpoints record the unchanged
subsystem, supplemental and official pointer-API gates. Ralloc 0.2.0 still has
no production external dependencies; TinyOne remains at package version 1.5.1.

Both control and candidate Windows/Linux hooks workspaces pass **571/572
tests**, with 21 ignored and no failures across 45 result groups. Candidate
all-target hooks Clippy with warnings denied, default-feature workspace checks,
no-std C-library checks and ordinary release builds pass on both platforms.
Each ordinary release passes all 49 printed correctness checks. All nine
first-fit tests pass. All 77 changed/new Rust formatting checks and
`git diff --check` pass before timing.

Fresh candidate strict-provenance Miri passes **55 unit plus 26 integration
tests**, including exact full-pool exhaustion: no unit case is filtered out.
The integration targets are `native_box`, `native_buffer`, `initialization`
and `fragmentation`. Default aliasing, validation and leak checks remain on;
only `-Zmiri-strict-provenance` is added. This is 81 Ralloc tests, not a full
TinyOne Miri run, and Miri duration is not performance evidence.

### Work counts and retained-object latency

There are **320 fresh profile records**, 160 per variant. All 80 Windows/Linux
counterparts match within each variant. Across variants, all 160 complete
canonical records match after excluding only `allocation_probes`: heap state,
allocation/free/growth/copy/failure counters, size/alignment histograms,
lookup/coalescing visits, codec/lock work and execution/JIT statistics are
unchanged. Native allocation/deallocation balances, failures remain zero and
retained native payload is zero after cleanup.

For 4,096 retained objects, default threshold 8, cold execution:

| Object kind | Allocations | Peak native payload bytes | Control allocation probes | Candidate allocation probes |
| --- | ---: | ---: | ---: | ---: |
| Cells | 4,103 | 524,672 | 7,917,554 | 4,863 |
| Strings | 4,103 | 393,600 | 7,793,918 | 4,685 |
| Structs | 4,103 | 917,888 | 8,150,503 | 5,015 |
| Enums | 4,103 | 655,744 | 8,029,652 | 4,944 |

Counts alone are not latency claims. The ordinary, uninstrumented
`runtime.jit_retained_{kind}_4096` rows produced these paired mean changes:

| Object kind | Windows scheduled cycles, pairs 1 / 2 | Linux thread CPU time, pairs 1 / 2 |
| --- | ---: | ---: |
| Cells | -80.93% / -80.63% | -83.72% / -83.86% |
| Strings | -74.54% / -74.93% | -78.72% / -79.02% |
| Structs | -87.31% / -86.90% | -91.25% / -91.03% |
| Enums | -79.43% / -77.46% | -83.56% / -83.34% |

Windows cells above uses the separately filtered 4,096-object confirmation,
not the rejected initial cells group. The original second cells pair is also
eligible (-81.08% JIT cycles); the first entire control capture is excluded
because its VM count-16 row has CV 31.47%. All 56 Linux target pairs and all
49 eligible original Windows target pairs have lower primary means, including
VM, count-256 cold JIT and smaller retained-object rows. This does not establish
statistical significance for every small change or performance outside these
workloads. Absolute Windows/Linux metrics are not compared against each other.

### Guard costs and unresolved Windows churn

The initial Windows 64-byte allocation/drop pair changes cycles by +2.40% and
**+7.19%**. The latter capture is eligible (candidate CV 9.78%), so its increase
is not discarded. A separately planned larger-sample confirmation, scale 64,
gives +0.24% and +2.71%, with CVs 0.79%/0.93% and 1.41%/1.17%. Linux gives
-0.10% and +3.01% CPU time at scale 8. These rows measure allocation **and
drop**, not isolated allocation-only or free-only latency.

The larger Windows scale-32 confirmations give resize +0.47%/+1.23%, calls
+0.10%/-1.09%, and hot loop -0.97%/+3.39% in cycles. Churn gives **+7.51%**
then **+0.05%**. The first churn pair has low CV (0.60%/0.65%); the reverse
pair has 0.60%/0.93%. The first increase therefore cannot be dismissed as a
variance-rejected run. The original eligible scale-8 churn pair is -5.30%; its
reverse pair is excluded because the control CV is 11.38%. Different sample
scales are not pooled or substituted for one another. The Windows result is
mixed, and its process-to-process variation is not causally explained.

All 48 Linux guard captures are eligible. Churn changes CPU time by
+0.86%/-0.92%; the 4-KiB allocation/drop row increases +3.33%/+1.67%, zero fill
+1.61%/+0.73%, and resize +1.00%/+0.78%. Other Linux guard changes are below
5%. Fragmentation decreases about 46% in both orders on both platforms; that
microbenchmark gain does not waive runtime regressions.

The four-worker, 32 allocation/drop operations per worker case uses all
permitted CPUs: Windows mask 4095 and Linux CPU list `0-11`. All eight captures
are eligible. Mean **elapsed time** increases +0.89%/+4.05% on Windows and
+0.68%/+0.96% on Linux. Main-thread CPU/cycle counters exclude the workers'
execution and are not used as aggregate contention throughput. Workers are
reused between rounds; synchronization remains part of the measured workload.
These filtered suites are not complete Priority 3 or Priority 5 gate runs.

### Evidence accounting and decision boundary

All timings used nine repeats, unchanged Balanced power policy, ordinary frozen
release binaries and control/candidate then candidate/control ordering. Except
for the multicore check, Windows used affinity `0x4`, Normal priority, and
Linux used CPU 2 with native `/tmp` fixtures. No source/docs edits, builds,
profiles, Miri runs, compression or other latency runs overlapped timing.

Across eight completed suites there are **160 captures, 156 eligible captures,
178 paired rows and 168 eligible paired rows**. Every invocation passes 49
printed correctness checks (7,840 in total), plus 12 additional untimed
retained-fixture parity checks. The other two whole-capture exclusions are
the initial Windows candidate hot-loop pair 1 (CV 5.40% > 5%) and control
resize pair 2 (CV 19.81% > 10%). All four excluded captures are preserved;
no otherwise stable row from one is accepted selectively. The two eligible
increases above 5% described above are preserved separately from quality
exclusions. A read-only status command had a PowerShell brace error; it did
not launch, alter or invalidate a benchmark.

`size-bound-control-freeze-1.json` has SHA256
`6BB4199A1CB279F2E793CE61003A0BC6D7BF7BEB2BDF09583487D27F4C526D4B`;
`size-bound-candidate-freeze-1.json` has SHA256
`EF7A73D6781559247C5592E89C0B656B035FDE49376EA7FD656ECFCFB0606260`.
The final `size-bound-completed-timings-audit-1.json` has SHA256
`BBD4F6559FEE027C6F0BC44B7EEAB1E0627933E686607F56A130F6E168F890C7`.
All are under `target/perf/full-system`. The final audit rechecks all 160 current
source/config hashes, both frozen archives/binaries and prior evidence hashes,
all profile records, chronological process/pair order, scheduling records,
raw JSON versus saved baselines and completion events, correctness counts,
Miri completion and whole-capture eligibility. No saved decision baseline
exists for a rejected capture. All known benchmark processes have finished.

The repeatable retained-object gains justify continued evaluation of this
small first-fit-preserving mechanism, but do not turn the mixed Windows churn
result into a clean acceptance. Isolating that process-level variation is the
next bounded check; rerunning until a preferred outcome appears is not an
acceptance method. The candidate stays explicitly provisional in the working
tree. Wider baselines, whole-batch acceptance, cleanup and the final report
remain open under the complete 40-section goal.

The requested Vec/Box safeguards are unchanged. Fixed metadata stays boxed;
private builders and changing-length stacks retain Vec; installed `JitOps`
exposes only slices, retains less than 4 KiB of spare operation storage per
chunk, and charges its full capacity. The previous container comparison remains
inconclusive and was not repeated. Neither the allocator counts nor its latency
gains are evidence that Vec is faster than Box.

After all timings, `cargo test -p tinylang --lib --features testing-hooks
--locked --offline jit::` again passes 71 JIT safeguards, with no failures and
six ignored diagnostics (`vec-box-safeguards-size-bound-1.log`, exit 0).
This checks storage bounds, ownership, cleanup and slice-only installation;
it is not another Vec/Box latency experiment.

## Allocator guardrails with longer Windows batches

The preceding Vec/Box recheck confirmed unchanged storage safeguards but did
not advance a remaining full-system optimization requirement. This continuation
completed the fixed churn-isolation diagnostic and followed the failed full
Priority 5 capture into a concrete measurement fix. The size-aware prefix
candidate is still **not accepted**.

### Fixed process-isolation evidence

The predeclared Windows churn sequence was AABB, BBAA, ABBA, BAAB, with A the
frozen control and B the frozen candidate: 16 fresh processes, nine repeats,
scale 32, affinity 4, Normal priority, and unchanged Balanced power policy.
All 16 captures and all four complete blocks are eligible. Candidate versus
control block-mean scheduled-cycle changes are -2.18%, -0.63%, -1.19%, and
-0.03%. These small changes do not establish a churn speedup.

Two adjacent control processes alone differ by -5.48%, despite both passing
their within-process variance gates. That supplies evidence of between-process
variation, but does not establish the cause of the earlier +7.51% candidate
outlier or erase it. No adaptive retries were made. Independent post-audit
checks all 51 recorded evidence hashes, chronological/nonoverlapping process
intervals, and equality of raw, saved, completion-event and audit documents
for all 16 captures. Evidence: `size-bound-churn-isolation-postaudit-1.json`,
SHA256 `C808115946EBBA3011A72DEC8B89C42C79C98FD4AF29B1A50A63F914C748799D`.

### Measurement and compatibility changes

The original four full Windows Priority 5 captures all pass quality gates,
but the native forward gate rejects the 64-byte allocation row: CPU +18.75%,
cycles +5.18%. The reverse pair reports CPU +25.00% while cycles change -0.41%;
resize also reports CPU +5.88% while cycles change -2.77%. Their short CPU
batch totals fall on coarse accounting steps: 62.5 ms for control 64-byte
allocation, 31.25 ms for 4-KiB allocation, and approximately 44–47 ms for
resize. These observations motivate longer batches; they are not permission
to discard CPU metrics or waive a failed gate. The
[GetThreadTimes contract](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-getthreadtimes)
specifies 100-ns representation units, not a promise that these workload
samples resolve every 100 ns.

Only the benchmark harness changes: the four short Windows allocation,
zero-fill and resize base batches are 32 times longer. Linux counts and
every workload body are unchanged. The additional CLI sample scale remains
identical within pairs. This reduces relative accounting granularity, not
allocator work, and is not a precision guarantee for arbitrary machines.
The existing CPU-sample filter and CV policy remain unchanged.

A new failing regression also proved that Priority 5 previously accepted
missing/invalid row iteration counts. The gate now requires each baseline
row to contain the same actual iteration count as its current counterpart,
so matching CLI sample-scale metadata alone cannot admit a shorter-batch
baseline. Tests cover each of the five rows, missing/null/string/zero/different
counts, matching batches, and the unchanged rejection of a 5.01% CPU regression.
A separate inventory test checks all four scaled rows and five unscaled
allocator/memory rows. All 17 benchmark tests pass after the fix.

The Rust router/examples, relevant memory/performance and Ralloc buffer
contracts, [cfg evaluation](https://doc.rust-lang.org/stable/std/macro.cfg.html),
[black_box limits](https://doc.rust-lang.org/stable/std/hint/fn.black_box.html),
and the locked serde_json 1.0.150
[integer extraction contract](https://docs.rs/serde_json/1.0.150/serde_json/enum.Value.html#method.as_u64)
were checked. No allocator/runtime production code or container policy changed
in this measurement checkpoint.

### Matched rebuild and completed guardrails

Both variants pass 573 Windows and 574 Linux workspace tests, with 21 ignored
tests across 45 result groups, all-target hooks Clippy, and 49 ordinary-release
correctness checks per binary. The two new benchmark tests account for the
increase from 571/572. Existing strict-provenance Miri coverage of 81 Ralloc
cases still applies to the identical allocator source; this is not a new
whole-TinyOne Miri result.

The control allocator was restored from its prior archive only for the matched
build/checkpoint, with exact hash checks; the candidate was restored
byte-for-byte afterward. Both retain the aligned-header correctness fix.
The first checkpoint packaging attempt failed because a PowerShell compression
assembly was not loaded; no archive was created. Loading that assembly fixed
packaging without replacing any prior evidence or relabeling a binary.

New freezes are `size-bound-precision-control-freeze-1.json` (SHA256
`0BAA55A05D18DA6261B77310375DC96107FC65A5D3A4BBD92E9D4D77CC67A668`)
and `size-bound-precision-candidate-freeze-1.json` (SHA256
`F6F0009AA082A172DF49E89BBA4DE9FE38E6760632A762FDD009A416FF060843`).
Each contains 160 source/config hashes, a verified archive and two ordinary
release binaries. Only benchmark source differs from each prior same-variant
freeze; only `arena.rs` differs between the new control and candidate.

Four new Windows captures and four new Linux captures run the full five-row
Priority 5 selection at nine repeats / scale 16, control/candidate then
candidate/control, with the same recorded affinity and power conditions.
All eight captures are eligible and all native processes are terminal.
No build, profile, compression or other timing overlapped them. The forward
candidate invokes the actual native gate; the reverse pair is audited against
every available mean CPU/cycle metric at the same 5% threshold.

| Row | Windows forward CPU / cycles | Windows reverse CPU / cycles | Linux forward CPU / cycles | Linux reverse CPU / cycles |
| --- | ---: | ---: | ---: | ---: |
| 64-byte allocation | +1.95% / +0.75% | **+5.61% / +5.57%** | +1.05% / +1.02% | +1.07% / +1.07% |
| 4-KiB allocation | +3.11% / +2.52% | +2.21% / +1.68% | -2.06% / -2.87% | +0.13% / +0.13% |
| 64-to-4096 resize | 0.00% / -0.07% | +1.56% / +1.64% | -18.08% / -18.09% | -0.79% / -0.99% |
| Reset 1024 | -2.03% / -1.43% | -0.49% / -0.56% | -1.25% / -1.37% | -2.19% / -2.18% |
| Snapshot 1024 | -0.47% / -0.43% | -1.22% / -1.28% | +2.27% / +2.22% | +1.44% / +1.44% |

The longer Windows reverse 64-byte batches average 45.6543 -> 48.2178 ns CPU
and 118.5035 -> 125.1096 scheduled cycles per operation. CPU batch totals are
2.3375 -> 2.46875 seconds; CVs are 0.398% / 0.289%. This still fails the
guardrail, so the original discrepancy cannot be dismissed solely as short-
sample quantization. The forward pair passes but has 9.04% / 5.39% CV on that
row. Both Linux pairs pass; the large forward resize decrease is not a
repeatable speedup claim (control CV 8.65%, reverse decrease only 0.79%).

The completed audit preserves all four original short-batch captures as well
as all eight new captures: 12 captures, 30 paired rows, 588 printed correctness
checks and 69 evidence hashes. It verifies source/binary pairing, exact row
iterations, raw/saved/event equality, scheduling and chronological order,
quality flags, and native versus independently recomputed gate outcomes.
Evidence: `size-bound-precision-timings-audit-1.json`, SHA256
`E5A05740A83A950653EA4A7FDB0053CDF8B3BCA75D3A59DA124AE8C91A62DBEC`.

The first size-aware hint remains unaccepted. Its repeated small-slot path
advances the saved boundary on every allocation and rewinds it on free.
Keeping the boundary at the selected block's start could leave repeated reuse
stationary, at the cost of revisiting one allocated header on retained growth.
That is the next bounded hypothesis, not a measured machine-store reduction
or an accepted allocator improvement. It requires a placement/state regression,
native and Miri checks, fresh probe attribution, and matched timing before
acceptance. Wider full-system baselines, cleanup and the final report remain
open under the complete 40-section objective.

## Stationary first-fit boundary: safety and guardrail checkpoint

The repeated Vec/Box check confirmed existing ownership and storage bounds but
did not advance the full-system optimization requirements. This continuation
verified the new stationary-boundary revision and completed its fixed Priority 5
measurement sequence. The allocator candidate remains **provisional**.

### State change and safety evidence

The first size-aware candidate cached the end of the selected allocated block.
The revision caches that block's start instead, revisiting its allocated header
on subsequent retained growth. It writes a changed prefix only when the two
fields differ, and shortens the boundary on free only when the fully coalesced
free block begins earlier. The strict request-size bound, first-fit placement,
alignment behavior, failure contracts and aligned-header initialization fix are
unchanged. No per-block metadata, new pool, container conversion, dependency,
public API or production unsafe operation was added.

The proposed optimization-state test failed before the revision: repeated
allocation moved the prefix from offset 0 to 80. It now checks sixteen reuse
cycles both with and without a retained leading allocation, preserving the
same pointer and both prefix fields after allocation and free, valid metadata,
zero final payload, and complete coalescing. This was a performance-state
invariant, not evidence of an earlier semantic or memory-safety defect. Source
branches do not establish a machine-store count.

All current checks complete successfully: 574 Windows / 575 Linux workspace
tests (21 ignored, 45 result groups per platform), both all-target hooks Clippy
runs, both default workspace checks, both no-std C-library checks, and 49
ordinary-release correctness checks per binary. Strict-provenance Miri passes
82 Ralloc cases: all 56 unit tests, including full-pool exhaustion, and 26 cases
across fragmentation, initialization, native-box and native-buffer targets.
There are no filtered unit cases or disabled default aliasing, validity or leak
checks. This is not full-TinyOne Miri coverage. The native layout remains
Arena/Option<Arena> 56 bytes and block header 16 bytes: unchanged from the first
size-aware candidate, 16 bytes per arena above the no-hint control.

The Rust router and four examples were checked. The implementation uses the
previously checked field-wise equality and pointer-provenance contracts; the
new verification introduces no external library API. Evidence scripts and logs
use the `size-bound-stationary-` prefix under `target/perf/full-system`.
The source freeze contains 160 source/config files and both ordinary binaries.
Only `arena.rs` differs from the precision candidate; only `arena.rs` differs
from the matched precision control. Benchmark counts, bodies and gate policy
are identical between the timing variants.

### Allocation-work tradeoff

All 160 new retained-object profile records have complete request histograms,
balanced native allocation/deallocation, zero failures and zero final payload.
All 80 Windows/Linux counterparts match exactly. Each record also matches
both older variants in every field except allocation probes.

For 4,096 retained objects at threshold 8 during cold execution:

| Kind | No-hint probes | First size-aware probes | Stationary probes | Peak usable payload |
| --- | ---: | ---: | ---: | ---: |
| Cells | 7,917,554 | 4,863 | 8,960 | 524,672 B |
| Strings | 7,793,918 | 4,685 | 8,782 | 393,600 B |
| Structs | 8,150,503 | 5,015 | 9,112 | 917,888 B |
| Enums | 8,029,652 | 4,944 | 9,041 | 655,744 B |

Each case still makes 4,103 allocations. The extra 4,097 probes relative to the
first candidate are the measured cost of revisiting headers; the remaining
reduction versus the no-hint allocator is not yet a latency claim for this
revision. Profiles and ordinary-release timings are separate processes/builds.

### Completed Priority 5 sequence

Four Windows captures followed by four Linux captures use frozen binaries,
nine repeats, scale 16, fixed affinity and unchanged Balanced power policy.
Each platform runs control/candidate then candidate/control. All eight native
processes completed and all 392 printed correctness checks passed. Seven
captures are quality-eligible. No builds, Miri, profiles, source edits or other
timings overlapped this sequence; no adaptive retries were made.

| Row | Windows forward CPU / cycles | Windows reverse CPU / cycles | Linux reverse CPU / cycles |
| --- | ---: | ---: | ---: |
| 64-byte allocation | +0.52% / +0.05% | +3.63% / +3.59% | +0.62% / +0.60% |
| 4-KiB allocation | +3.67% / +3.84% | +2.95% / +2.55% | +2.70% / +2.70% |
| 64-to-4096 resize | +1.04% / +1.14% | +1.05% / +1.35% | +3.79% / +3.46% |
| Reset 1024 | -1.22% / -1.18% | -0.71% / -0.25% | +2.71% / +2.73% |
| Snapshot 1024 | -0.58% / -0.66% | -0.54% / -0.48% | -0.11% / -0.11% |

All three eligible pairs stay within the unchanged 5% limit on every available
mean CPU/cycle metric. This still records small positive allocation costs; it
does not mean the revision eliminates all overhead or prove a particular
machine-store reduction. The Windows forward candidate passes the actual
native gate; the reverse comparisons are independent post-run gate audits.

The Linux forward control fails quality because its 64-byte row has 12.1366%
CV, above 10%. The entire forward pair is excluded, including its apparently
favorable rows. Its raw results and failure are preserved; no control baseline
was saved, so the forward candidate correctly skipped the native gate. Thus
**no Linux native gate ran**, and Linux has only one eligible independent pair.
The Windows launcher exits 0; the Linux launcher exits 1 for the quality
rejection, not an unobserved process or a claimed regression gate pass.

The audit checks all 160 current source hashes, source/binary pairing, exact
iteration counts, raw/saved/event equality, chronological nonoverlap across
platforms, recorded scheduling, native outcomes and all-metric thresholds.
It records eight captures, twenty paired rows and 65 evidence hashes. A
read-only preview calculation initially hit a PowerShell pipeline syntax error;
correcting that calculation did not alter or rerun any capture.

Checkpoint hashes:

- Source/binary/verification freeze `size-bound-stationary-freeze-1.json`:
  `DE00CDD884524E83677D4310D0CEB94FD3920E485225493A505A5F1175767AB4`.
- Profile audit `size-bound-stationary-profile-audit-1.json`:
  `AA318E9BF5A3219DC8140818527F689F3606D26D76F4525B6621C67ABFF356AF`.
- Timing audit `size-bound-stationary-timings-audit-1.json`:
  `A5F68DA80CFFBB3EEFEAC2552ECD0707651B3DE5A8ECFF7C166D0148A99C8A78`.

The next predeclared work is the full 28-row retained-object timing matrix,
including small cases, using two orders on each platform and the same frozen
control/candidate binaries. Current runtime/churn acceptance, the excluded
Linux guardrail confirmation, wider baselines, cleanup and final reporting
remain open. Do not promote the first candidate's runtime gains to measurements
of this revision or repeat the unchanged excluded capture until it passes.
Vec/Box ownership safeguards and the full 40-section objective are unchanged.

### Completed stationary retained-object timing matrix

The predeclared matrix is now complete on both platforms: 32 captures, 30
eligible captures, 112 paired rows, 98 eligible paired rows and 1,568 printed
correctness checks. Each family includes portable VM and JIT runs at 16, 256
and 4,096 objects, plus a separate 256-object JIT cold-run row. Each row uses
nine repeats, with actual iteration counts 8,000 / 1,600 / 40 respectively;
the cold-run row uses 1,600. Both variants use the same counts. Each platform
runs control/candidate followed by candidate/control at scale 8, fixed affinity
and unchanged Balanced policy. Windows finishes before Linux begins.

For eligible 4,096-object JIT pairs, changes in the primary metric are:

| Kind | Windows forward cycles | Windows reverse cycles | Linux forward CPU time | Linux reverse CPU time |
| --- | ---: | ---: | ---: | ---: |
| Cells | -82.24% | -81.03% | Excluded | -83.71% |
| Strings | -75.38% | Excluded | -78.24% | -77.89% |
| Structs | -86.72% | -86.64% | -90.93% | -91.02% |
| Enums | -78.75% | -78.59% | -83.78% | -83.76% |

These are fresh comparisons against the no-hint control using the stationary
candidate, not reused latency results from the first candidate. Windows uses
scheduled-thread cycles and Linux uses thread CPU time; absolute measurements
are not compared between operating systems. Each eligible 256-object cold-run
row also decreases: 12.43–25.42% cycles on Windows and 21.29–28.20% CPU time on
Linux. These rows are not isolated compiler or lowering measurements.

Small cases are mixed. Seven of 49 eligible Windows paired rows and three of
49 Linux rows have higher primary means, all at 16 objects. The Windows
16-cell VM forward pair rises from 43,716.4056 to 46,046.1244 cycles (+5.33%),
with 1.38% / 2.56% within-process CV. Its reverse pair decreases 4.93%; the
eligible Linux reverse pair decreases 0.45%. Preserve both directions: the
forward result is not discarded as noise, and neither a consistent regression
nor its exact cause is established. The other positive means range from 0.06%
to 1.67%; direction alone does not establish significance for those small
differences. These observations do not justify a universal small-case win.

Two whole seven-row comparisons are excluded:

- Windows reverse strings: the control's 16-object JIT row has 12.89% CV.
- Linux forward cells: the control's 16-object JIT row has 19.30% CV.

Both exceed the unchanged 10% limit. Their candidate partners are eligible,
but all seven rows in each pair remain excluded, including apparently favorable
large and cold rows. Raw results and rejection events are preserved and neither
control saved a decision baseline. Both launchers exit 1 for the recorded
quality failure after completing all sixteen captures, not because a process
was abandoned. There were no adaptive retries and no overlapping builds,
profiles, Miri, compression, source edits or other timings.

`size-bound-stationary-targets-audit-1.json` has SHA256
`DD7FB4C7D3A0D9397E1092BB2DA732613FEFCC85C7DC8C1951D7467DC32C4AFB`.
It checks all current source hashes, matched frozen binaries, prior profile
compatibility, complete per-family row names and actual iteration counts,
raw/saved/event equality, quality flags, chronological order, recorded affinity,
native Linux fixture storage, the full Miri checkpoint and 103 evidence hashes.
The control profiles are explicitly reused: their production sources match
the precision control except for the benchmark-only measurement changes.
All benchmark process handles are now terminal.

The large retained-object benefit is supported, but this does not finish
allocator acceptance. Remaining current-candidate checks include heap churn,
contention, zero-fill, capacity/fragmentation boundaries and ordinary runtime
guardrails. The Linux Priority 5 forward exclusion and the mixed 16-cell result
remain visible. Continue that work from the frozen source/binary checkpoint;
do not repeat excluded captures unchanged until they pass. Wider full-system
requirements, final cleanup and the implementation report remain open.

### Completed stationary allocator guardrails and retention decision

The remaining fixed sequence is complete: 36 Windows single-thread captures,
36 Linux single-thread captures, four Windows multicore contention captures,
then four Linux multicore contention captures. All 80 captures are eligible,
all 40 pairs are complete, all 3,920 printed correctness checks pass, and all
four launchers exit 0. The ordinary frozen binaries and all 160 source/config
hashes are unchanged. No builds, profiles, Miri, compression, source edits or
other timing suites overlapped the sequence; no adaptive retries were used.

Each capture has nine repeats. Calls, hot loops and heap churn use the
predeclared scale 32; the other rows use scale 8. Windows zero-fill retains the
benchmark's existing additional 32-fold base-count multiplier. The hot-loop
CV limit remains 5%, and the other limits remain 10%. Windows single-thread
work uses affinity 0x4 and scheduled cycles; Linux uses CPU 2 and thread CPU
time. Contention uses all twelve permitted logical CPUs (Windows mask 4095,
Linux list 0-11) and elapsed time, not the coordinating thread's CPU/cycles.
Power policy stays Balanced. Absolute metrics are not compared across OSes.

Paired changes in those predeclared primary metrics are:

| Workload | Windows forward | Windows reverse | Linux forward | Linux reverse |
| --- | ---: | ---: | ---: | ---: |
| Zero-fill 4096 | -2.58% | +1.02% | +4.84% | -2.51% |
| Arena capacity boundary | +1.47% | +2.64% | +0.33% | +2.44% |
| Fragmented arena cycle | -46.08% | -46.47% | -44.55% | -44.23% |
| Execution-context setup | -1.19% | +1.18% | -1.13% | +0.11% |
| Function calls | +0.28% | -4.41% | +0.71% | -2.13% |
| Quickened hot loop | -6.90% | -0.77% | -0.04% | -3.90% |
| Vector push/pop | -2.22% | +0.10% | -0.40% | +4.02% |
| Map set/get | +3.93% | -0.09% | +1.19% | -2.17% |
| Heap churn | -0.57% | -0.01% | +0.84% | -1.57% |
| Four-worker contention (wall time) | +0.99% | +2.08% | -2.96% | -0.22% |

All 40 primary comparisons stay within a 5% increase. This is **not** an
all-metric Priority 5 gate pass: these are separately filtered guardrail rows,
and three supplemental observations exceed 5%. Windows forward capacity has
+7.00% mean CPU time (+1.47% cycles), and forward maps has +6.00% CPU time
(+3.93% cycles). Their reverse CPU changes are -0.96% and -1.66%. Linux forward
zero-fill has +6.12% mean elapsed time/TSC cycles despite +4.84% thread CPU
time; its reverse decreases about 2.51%. All remain eligible observations.
Windows CPU quantization and Linux TSC descheduling are measurement limits,
not proven explanations for these individual differences. None is erased or
relabeled as a win. Windows contention is also not an improvement.

The independent audit checks source/binary pairing, non-scan profile equality,
exact names and actual iterations, raw/saved/event equality, quality flags,
fixed ordering and nonoverlap, scheduling, native Linux fixtures, and the
existing complete 82-case Ralloc Miri checkpoint. It records 251 evidence
hashes. The first audit attempt exposed a PowerShell singleton-array bug: the
one-workload contention plan became a string, so indexing expected `c` instead
of `contention`. Preserving the array shape fixes that audit bug; capture data,
workload order, thresholds and benchmark code were not changed or rerun.
`size-bound-stationary-guardrails-audit-1.json` has SHA256
`1C2B904F4EC6DEFF8FB13FAC09D5B04459EA75A0406A25D85264A736A742AF87`.
Its provisional status records the pre-decision audit checkpoint, not the
following scoped retention decision.

Retain the stationary size-aware boundary for its demonstrated retained-object
and fragmented-scan benefit. The 4,096-struct and 4,096-enum JIT workloads each
have two eligible comparisons on each platform with lower best elapsed time
and lower primary CPU-work metrics. Fragmentation also improves both measures
in both orders on both platforms. The profile records identify sharply fewer
allocation probes without changing allocation counts, request histograms,
peak usable payload or final cleanup. The 574/575 native workspace tests,
both-platform Clippy/default/no-std checks and 82 strict-provenance Miri cases
remain tied to this exact unchanged production source.

This is a bounded tradeoff, not universal allocator acceptance: the two-word
hint adds 16 bytes per arena on the measured 64-bit builds, some short
allocation and capacity rows cost more, Windows contention rises 0.99-2.08%,
and small runtime results remain mixed. The earlier +5.33% 16-cell VM forward
result is retained beside its -4.93% reverse result. The Linux Priority 5
forward pair and two retained-family pairs remain excluded; no Linux native
Priority 5 gate ran, and its reverse pair is the only eligible Linux Priority
5 confirmation. Those gaps limit claims; this new guardrail matrix does not
replace them or rehabilitate the first candidate's failed Windows guardrail.
There is no claim of fewer machine stores, universal churn improvement, or a
speed advantage for Vec over Box.

The requested container policy is unchanged: fixed installed metadata is boxed;
changing-length stacks/builders use private vectors; installed `JitOps` exposes
only slices, retains less than 4 KiB spare capacity per chunk and charges all
capacity. Its container timing comparison remains inconclusive. No container
or Rust implementation changed during this guardrail work.

The planned allocator experiment is now closed with this scoped retain decision
and all raw failures preserved. Do not restart its unchanged timing sequence
to obtain cleaner numbers. Next inspect the existing preparation-phase evidence
for an attributable cold-lowering cost. Wider compiler/verifier/local-IR,
feedback, cache and allocation coverage, final cleanup and the full 40-section
implementation report remain active; this checkpoint does not complete them.

## Iterative encoder left descent: verified prototype

The closed stationary allocator experiment was followed by inspection of the
existing preparation-phase evidence, not another allocator or Vec/Box timing
retry. The hooks-only phase summary attributes roughly 26–30% of observed
deep-expression preparation time to encoding. Observer overhead and variation
make that an inspection lead, not an ordinary-release cost or predicted gain.
The relevant encoder, local representation, lowering and origin source hashes
still matched that phase checkpoint before this change.

`LocalEncoder` already uses a private, reusable `Vec<Emit>` for iterative
postorder. An unfused binary previously pushed its operator, right child and
left child, immediately popping the left child again. The prototype instead
descends into that left child directly, keeping only the pending right child
and operator on the work vector. Leaves and fused children resume the pending
work. This preserves left/right/operator order and source origins, removes one
source-level push/pop pair per unfused binary, and keeps leaf-only blocks free
of traversal allocations. No recursive tree boxes, new container, unsafe code,
runtime handler, dependency, benchmark fixture or Ralloc implementation changed.
This is not a measured machine-instruction or machine-store count reduction.

Two regressions were added and passed on the old encoder before changing the
algorithm (`encoder-left-descent-control-tests-windows-1.log`). They compare
exact operations and origins for left-deep, right-deep and balanced trees across
reuse, including source positions above 65,536, and verify that fused children
resume pending siblings before their parent. Existing differential, deep-tree,
numeric and diagnostic suites remain part of the full verification.

The existing storage diagnostic reports the following Windows before/after
traversal reservations; the Linux candidate reports the same new extents:

| Expression depth | Before | Prototype |
| --- | ---: | ---: |
| 4 | 128 B | 64 B |
| 1,024 | 32,768 B | 16,384 B |
| 65,536 | 2,097,152 B | 1,048,576 B |

Other reported builder capacities and operation counts are unchanged. The
builder header remains 224 bytes, expressions/actions 16 bytes each. These
are temporary scratch reservations, not installed memory, host allocation
counts, whole-process peak memory or RSS. Evidence is in the control/candidate
layout logs and `encoder-left-descent-layout-{windows,linux}-1.log`.

The repository router, Rust rules and concrete examples were followed, with
the applicable architecture, performance and ownership documentation. Relevant
standard-library contracts were checked, including
[Vec push/pop and capacity behavior](https://doc.rust-lang.org/stable/std/vec/struct.Vec.html)
and [loop control](https://doc.rust-lang.org/stable/reference/expressions/loop-expr.html#break-expressions).
No external crate API or direct Ralloc allocation path was changed.

### Verification and fixed target comparison

`encoder-left-descent-verify-1.ps1` completed on both platforms: 576 Windows
and 577 Linux workspace tests passed with testing hooks, 21 ignored and 45
result groups each. Both all-target Clippy runs with warnings denied and both
default-feature all-target workspace checks passed. Both ordinary release
builds passed all 49 printed correctness checks. Commands were locked/offline.
The unchanged allocator's earlier 82-case strict-provenance Miri checkpoint
is separate evidence; no new encoder or whole-TinyOne Miri pass is claimed.

`encoder-left-descent-freeze-1.json` records 160 source/configuration hashes,
the source archive and ordinary Windows/Linux binaries. Only `jit/encoding.rs`
differs from the stationary allocator control. Its SHA256 is
`E0209E4A4E176C3FB7DDC46CD0C6DB1AFE897A837CF971E8C92476C518E7CF80`;
the freeze SHA256 is
`F45CBBD38F0F466C31E32046180030FB4C69370FC6E89AC22E5F13EF23D7DB9D`.

The predeclared target matrix used the existing
`jit.codegen_expression_chain_65536_cold` and
`jit.codegen_stored_expression_chain_65536_cold` rows, 320 actual iterations,
nine repeats and a 10% CV gate. Each platform ran control/candidate followed by
candidate/control; Windows completed before Linux. Windows used affinity 4
and Normal priority under Balanced power; Linux used CPU 2 and native temporary
fixtures. Each timed iteration creates and drops a fresh JIT cache from input
verified outside timing. Builds, profiles and source edits did not overlap
timing, and rejected captures were not retried.

All eight processes completed their 49 correctness checks (392 total). Six
captures are eligible. The Windows forward candidate's expression-chain CV
was 11.40%; the Linux forward control's was 11.34%. Both entire two-row pairs
are excluded, including their otherwise acceptable stored-expression rows.
The launchers therefore ended with the expected quality-failure exit 1, not a
correctness failure. Only the reverse pair on each platform is usable:

| Platform | Cold target | Primary metric change | Best wall change | CV before / after |
| --- | --- | ---: | ---: | ---: |
| Windows | Expression chain | cycles −5.80% | −7.47% | 8.32% / 9.45% |
| Windows | Stored expression chain | cycles −7.46% | −7.97% | 0.60% / 2.56% |
| Linux | Expression chain | thread CPU −1.97% | −2.11% | 1.00% / 0.60% |
| Linux | Stored expression chain | thread CPU −2.30% | −2.76% | 0.55% / 1.38% |

`encoder-left-descent-targets-audit-1.json` verifies eight paired rows, four
eligible paired rows, capture order/nonoverlap, exact options/metadata, source
and binary provenance, and 28 evidence hashes. Its SHA256 is
`676504D25522E2755DE532C8AA26B8EB1E489D89C11A46384C8065323C8896C7`.
One usable pair per platform is not repeated confirmation, especially with
the Windows expression row's variation. The prototype is **not accepted**:
broader cold/runtime guardrails and preparation allocation accounting remain
next, followed by an explicit retain/remove decision. Do not retry the rejected
target pairs unchanged to manufacture a clean result. The full 40-section
objective, final cleanup and implementation report remain active.

### Vec/Box ownership check at this checkpoint

The renewed user request was checked against the actual storage lifetimes.
Fixed certificates, block/source/origin tables, chunk positions, edge counters
and feedback remain boxed slices. Builders and operand/call stacks use vectors
because their initialized lengths change; the encoder scratch vector remains
private and dies after chunk construction. The eight-entry operand pool clears
values before retention, charges all capacity and is not copied into clones.
Frames already own their separate allocations; no per-frame box was added.

Installed `JitOps` is still an explicit exception, not a proven Vec speed win:
slice-only access prevents length changes, installation leaves less than 4 KiB
of spare operation capacity per chunk, and accounting charges the full extent.
Its earlier Vec/Box timing recheck remains inconclusive and was not repeated.
The current standard-library
[boxed-slice conversion contract](https://doc.rust-lang.org/stable/std/vec/struct.Vec.html#method.into_boxed_slice)
was rechecked; no unconditional conversion was introduced based on preference.

Fresh Windows checks pass 73 JIT unit tests (six diagnostics ignored) and all
three explicitly selected layout diagnostics in
`vec-box-safeguards-encoder-left-descent-1.log` and
`vec-box-layout-encoder-left-descent-1.log`. They cover the installation slack
boundary, fixed extent across quickening, clone independence, cleared pool
reuse and capacity accounting. Call-frame vector backing remains 0/352/704/
1,408 bytes at depths 0/1/8/16 versus 1,408 bytes for a maximum-depth boxed
option array. That demonstrates a shallow-call storage tradeoff, not latency
superiority over every boxed design. No Rust implementation changed during
this renewed ownership check; all 160 candidate source hashes still match.

## Completed encoder guardrails, allocation accounting and memory-focused retention

Retain iterative left descent as a scoped scratch-allocation optimization.
The completed comparison confirms halved traversal reservation at the measured
depths and one fewer successful host reallocation on deep expressions, without
changing installed storage. Timing guardrails pass their predeclared limits,
but results are mixed and the original targets still have only one eligible
pair per platform. This decision does not establish general runtime latency
acceptance, a Vec speed advantage over Box, or full-system completion.

### Fixed cold/runtime guardrails

`encoder-left-descent-guardrails-1.ps1` predeclared six exact rows, two orders
per platform, nine repeats and sample scale 32. Order 1 runs control/candidate;
order 2 runs candidate/control. Each process measures one row. Windows uses
affinity 4, Normal priority and Balanced power; Linux uses CPU 2 and native
temporary fixtures. All Windows captures precede Linux captures. No build,
profile, source edit or compression overlaps timing, and there are no retries.
Cold rows create and drop a fresh cache from input verified outside timing;
runtime rows measure execution, not cold preparation. The primary regression
limit is 5%, with a 10% CV limit except the hot loop's stricter 5% CV limit.

All 48 captures and 24 paired rows are eligible; all 2,352 printed correctness
checks pass. The following changes use Windows mean scheduled cycles and Linux
mean thread CPU time. Positive means slower, not an improvement:

| Exact row | Actual iterations | Windows order 1 | Windows order 2 | Linux order 1 | Linux order 2 |
| --- | ---: | ---: | ---: | ---: | ---: |
| `jit.codegen_straightline_cold` | 160,000 | −1.683% | −0.304% | −8.031% | −2.911% |
| `jit.codegen_float_loop_cold` | 160,000 | +0.312% | −0.775% | −0.608% | −1.075% |
| `jit.codegen_dead_stores_65536_cold` | 320 | +2.855% | +2.539% | −2.345% | −1.297% |
| `jit.codegen_copy_stores_32768_cold` | 320 | +3.337% | −0.899% | −3.866% | −1.426% |
| `runtime.jit_function_calls` | 19,200 | +2.290% | −0.632% | +1.697% | −3.232% |
| `runtime.jit_hot_loop_4096_quickened` | 3,200 | −2.002% | −0.673% | +0.217% | +2.582% |

No primary or supplemental mean CPU, mean cycles, mean wall or best wall
increase exceeds 5%. The largest supplemental increase is +4.921% best wall
for Windows copy stores in order 1. Passing that bound is not proof of zero
performance cost. Windows dead-store cycles are higher in both orders, with
6.10–9.05% capture CVs; Linux hot-loop CPU is also higher in both orders.
Neither result is called a win or discarded. The first Linux straightline
control has 9.35% CV and the candidate's best wall is +0.30%, so its −8.03%
mean CPU result is not presented as a general cold-start gain.

`encoder-left-descent-guardrails-audit-1.json` verifies all options, ordering,
nonoverlap, frozen binaries, current source hashes and 150 evidence hashes.
Its SHA256 is
`5665E3C1A92CD6063E781B6A317D37EBFD1351695B57AAD5E7CF181A3232971C`.
The earlier Windows and Linux forward target pairs remain excluded; these
different guardrail rows do not supply the missing target confirmation.

### Fresh, isolated preparation allocation comparison

The first diagnostic attempt is invalid and preserved. Its two Windows source
trees shared a Cargo target directory: the candidate build reported fresh
artifacts and reused the control executable hash. All 132 resulting records
are excluded. Linux ran no profiles because the wrapper wrongly required a
`release/deps` executable, while Cargo reported a different artifact layout;
that build also used shared artifacts. See
`encoder-left-descent-profile-attempt-1-invalid.json`. Frozen ordinary-release
timing binaries and their completed comparisons were not affected.

The corrected `encoder-left-descent-profiles-2.ps1` builds into four new,
separate platform/variant target directories. The control tree is extracted
from its frozen archive with path containment, inventory and all 160 hashes
checked. Cargo JSON must identify the expected manifest, newly compiled core
and test artifacts, and an executable inside the isolated directory. Control
and candidate executable hashes are distinct on each platform. Builds use
locked/offline release mode with testing hooks; no timing suite is running.

The existing preparation diagnostic and allocation observer are unchanged.
Eleven fixtures, two ownership routes, two variants, two platforms and three
repeats produce 264 new records, none reused. All repeats are identical, every
counter matches across platforms, and all 44 paired profiles preserve installed
operations, function counts and retained bytes. Host owners balance; every
interval asserts zero Ralloc allocations, deallocations and live payload bytes.
Each fixture also checks VM/JIT output and lazy installation outside the
allocation interval.

Only successful reallocation counts and requested growth bytes change, in four
fixtures on both routes and platforms (16 paired profiles). The standalone
Windows counts below have the same before/after deltas everywhere:

| Fixture | Host reallocations before → after | Requested growth bytes before → after | Unchanged retained bytes |
| --- | ---: | ---: | ---: |
| `deep_4` | 6 → 5 | 672 → 544 | 1,426 |
| `deep_1024` | 30 → 29 | 229,152 → 196,384 | 111,586 |
| `expression_chain_65536` | 48 → 47 | 14,679,840 → 12,582,688 | 7,078,882 |
| `stored_expression_65536` | 50 → 49 | 14,680,064 → 12,582,912 | 7,078,990 |

Initial allocation/deallocation counts, initial requested bytes and all four
allocation buckets are unchanged, as are every field of the other seven
fixtures. Avoiding the final growth request removes 2 MiB of cumulative
requested growth on each 65,536-expression case; this is compatible with a
1 MiB smaller final scratch reservation. Requested extents are not bytes
copied, peak memory, RSS or ordinary-release latency.

`encoder-left-descent-allocation-audit-2.json` records 88 profile groups,
44 pairs and 27 evidence hashes. Its SHA256 is
`7A77701BB4055C50BE8BA0428EA1FF688491E15C9B707E7D0D90D33B140E1F5C`.

### Build repair, decision boundary and next work

The invalid shared-target attempt could leave ordinary release outputs paired
with the wrong source/features. A package-scoped Cargo release cleanup for
`tinylang` and `tinyone_ralloc` was previewed first, with every resolved path
checked inside the intended platform release directory and against reparse
points. Only regenerable release artifacts were removed: 119 files / 101.1 MiB
on Windows and 117 files / 116.4 MiB on Linux. No source, frozen binary, source
archive, isolated diagnostic build or measurement evidence was removed.
Both ordinary default-feature releases were then rebuilt with fresh package
artifacts and passed all 49 correctness checks. These rebuilds do not replace
the frozen executables in earlier measurements. Repair manifests and hashed
logs are `encoder-left-descent-release-repair-{windows,linux}-1.json`.
The measurement workflow now documents isolated source-variant builds and
Cargo-reported executable paths.

The retained implementation is still the previously verified encoder hash
`E0209E4A4E176C3FB7DDC46CD0C6DB1AFE897A837CF971E8C92476C518E7CF80`.
Its 576/577-test native workspace gates, Clippy and default-feature checks
remain applicable because no Rust source changed during this follow-up.
No new encoder or full-TinyOne Miri coverage is claimed. The deterministic
scratch/growth reductions, preserved evaluation order and bounded timing
tradeoffs justify this small safe change; they do not establish a universally
faster encoder or runtime. Broader local-IR performance acceptance remains open.

The Vec/Box ownership check above is unchanged: growing owners retain vectors,
fixed metadata retains boxed slices, and installed operations remain the
slice-only, capacity-charged exception with an inconclusive timing comparison.
Do not repeat unchanged container, closed allocator or rejected encoder target
captures. Return to the remaining runtime/feedback workload and performance
audit, starting with the recorded hot-loop and sparse-feedback limitations;
the complete 40-section ledger, final cleanup and final report remain active.

At handoff, all 160 frozen source/configuration entries still match. The
target, guardrail and allocation audits' 28, 150 and 27 evidence hashes,
respectively, plus the freeze and both repaired releases' artifacts/logs,
verify unchanged. Nightly rustfmt checks pass for the four touched JIT owner/
encoder files; `git diff --check` and the four edited documents' trailing-space
check pass. All launched builds, diagnostics and timing processes are terminal.

## Opcode-local feedback IDs rejected by ordinary-workload guardrails

Outcome: the distinct opcode-local ID prototype is removed. The fixed
full-chunk boxed feedback table and all 160 pre-experiment source/configuration
hashes are restored to the retained encoder-left-descent checkpoint. No
growable feedback container, new pool, larger budget, allocator change or
Vec-for-Box substitution remains. This is another measured rejection, not
section-14 completion or a general container-speed conclusion.

The batch reread the full objective, repository engineering router, Rust
rules and four concrete examples, and the architecture, memory, VM,
performance, contribution and CI documentation. The unchanged Ralloc
ownership/failure contracts were rechecked; no Ralloc API, external crate API
or dependency changed. The standard-library
[partition-point contract](https://doc.rust-lang.org/stable/std/primitive.slice.html#method.partition_point)
and [Rust layout guarantees](https://doc.rust-lang.org/stable/reference/type-layout.html#the-rust-representation)
were consulted for this design. Actual layouts were measured on both native
targets rather than inferred from Rust's default representation.

### Question, design and boundary coverage

The control's 11-byte feedback entries cover every lowered operation. Even
a program with only two useful scalar sites can lose feedback when irrelevant
padding pushes the table above the existing 64-KiB program budget. A fresh
18-case control diagnostic reproduced that limitation before editing.

Unlike the earlier rejected first-to-last-site span, this prototype carried
a typed two-byte chunk-local feedback index in sampled/hot opcodes, independent
of the full-width instruction offset. It allocated one lazy fixed boxed slice,
sized to potential scalar sites and capped to the remaining whole-entry
budget; hot-range activation assigned entries contiguously without growth.
Assigned entries never became unseen again. The assigned-prefix search ran
only during activation; execution indexed the table directly from the opcode.
Terminal generic binary/slot families prevented overlapping hot ranges from
rearming an old site after its opcode stopped carrying an ID. Adaptive slot
operands used the already verifier-bounded slot type; packed comparisons used
previously unused metadata bits while preserving the immediate and target.
Sampling thresholds, hysteresis, original error sites and generic semantics
were unchanged. No unsafe code was added.

Only five production files changed: the JIT feedback, operation, chunk,
execution-accounting and VM modules. Three other source changes were tests.
Six new regressions covered offsets beyond 65,535, zero/sub-entry/partial
budgets, overlapping and out-of-order activation, terminal fallbacks with
unused entries, ID preservation through rewrites and packed target updates,
and widely separated scalar sites with warmed clones, unsupported numeric
representations, errors and VM parity. The wide-offset and one-entry-budget
tests failed on the control before implementation. The shared-budget fixture
was changed to contain unreachable potential scalar operations instead of
irrelevant returns, retaining actual pressure on the unchanged program bound.

The candidate passed 582 Windows and 583 Linux workspace tests, with 21 ignored
tests and 45 result groups on each platform. Both hooks-enabled all-target
Clippy runs with warnings denied, both default-feature all-target checks,
both explicit layout/storage diagnostics, and both ordinary-release 49-check
correctness suites passed. Native layouts stayed at JitOp 24 bytes, JitChunk
96 bytes, JitProgram 208 bytes and SiteFeedback 11 bytes.

In the diagnostic, all enabled sparse shapes used 22 feedback bytes. The
control used 143 bytes without padding, 22,671 bytes with 1,024 prefix or
suffix prints, and no feedback at all for the 8,192-print shapes. The new
large cases observed 16 samples, two promotions and 47/33 hits at thresholds
1/8. Disabled feedback stayed at zero, and installed code/header sizes did
not grow. These are backing-storage and execution-profile observations, not
host allocator-call counts, RSS or allocator-latency measurements. The
profiled retained charge can increase when a formerly denied case begins
specializing; for the 8,192-prefix case it rose from 1,085,799 to 1,086,081
bytes despite the small table. No broader memory-speed claim follows.

### Fixed ordinary-release matrix

Source and executable identities were frozen before timing. The unchanged
harness ran eight exact rows at scale 32 with nine repeats, first
control/candidate and then candidate/control on Windows, followed by the
same two orders on Linux. Every invocation ran all 49 correctness checks.
Windows children used affinity 0x4 and Normal priority; Linux children used
native fixtures and taskset CPU 2. The Balanced power policy was unchanged.
No source edits, builds, profiles or other timing jobs overlapped the matrix.
CV limits were 10%, except 5% for the ordinary hot loop; primary increases
above 5% were flagged. No excluded capture was retried.

All 64 invocations completed and passed 3,136 correctness checks. Windows
provided 16 eligible captures but only six eligible whole pairs; Linux
provided all 32 eligible captures and 16 eligible pairs. Thus the audit uses
22 eligible pairs, not all 32 possible pairs. Every Windows sparse-target,
sparse-lowering and sparse-first-run pair was excluded. Their raw favorable
or unfavorable point estimates are not performance evidence.

The table shows percentage changes in eligible primary means: thread cycles
on Windows and thread CPU time on Linux. Positive is slower; an em dash means
the whole pair failed the predefined variation gate.

| Workload | Linux order 1 | Linux order 2 | Windows order 1 | Windows order 2 |
| --- | ---: | ---: | ---: | ---: |
| Sparse loop | -65.82% | -64.02% | — | — |
| Sparse cold lowering | +0.76% | -1.20% | — | — |
| Sparse first run | -33.72% | -31.50% | — | — |
| Ordinary hot loop | +1.38% | +3.13% | — | -0.94% |
| Ordinary mixed loop | +3.13% | +2.01% | — | -3.98% |
| Function calls | -2.31% | +0.27% | — | +1.59% |
| Map set/get | +2.08% | +2.29% | +1.57% | — |
| Small cold lowering | +4.57% | +0.03% | +6.24% | +0.86% |

Linux sparse-loop best wall time also improved 65.37%/64.72%, and sparse-first-
run best wall improved 33.02%/33.31%. However, the ordinary Linux hot, mixed
and map rows were slower in both orders in CPU, cycles and wall means. Those
individual increases are below the 5% flag, but that flag is not permission
to disregard consistent realistic-workload regressions under section 37.
The Windows forward cold pair increased cycles 6.24%, CPU 6.95% and wall mean
6.27%; the reverse pair stayed within 5%. The eligible Windows mixed reverse
pair improved cycles while CPU increased 0.41% and best wall increased 0.85%,
so it is not an unqualified mixed-loop win either.

The large isolated sparse-fixture gains and smaller boxed table do not justify
those ordinary-runtime costs plus broader opcode/handler complexity. The
prototype is not required for semantic correctness or an established later
optimization. It was therefore removed rather than accepted with the
remaining Windows evidence gaps. The exact machine-code cause of the smaller
ordinary regressions has not been established. No new allocation-call profile,
hardware-counter claim, Miri run or Ralloc latency claim was added after this
rejection; those planned acceptance steps would not rescue a rejected change.

### Evidence and restoration

All artifacts remain under target/perf/full-system:

- Control: encoder-left-descent-freeze-1.json, SHA-256
  F45CBBD38F0F466C31E32046180030FB4C69370FC6E89AC22E5F13EF23D7DB9D.
- Candidate source/binary/verification freeze: feedback-inline-freeze-1.json,
  SHA-256 87A2820A4C8999B3D135F160F86102364DF166A743FE2040A0FAFC1E79C5FF7E.
- The recoverable 160-entry candidate source archive, including all six
  experimental regressions: feedback-inline-source-1.zip, SHA-256
  190D78954F0DE8A3227A64F32B735687CFDB5276618DB53FA6A98C4ECAA1B38D.
- Matrix audit with raw records, eligibility, supplemental metrics and
  182 evidence hashes: feedback-inline-matrix-audit-1.json, SHA-256
  F46F86BFAFA89B325B16B5F72B845043DCEAF363BD6CF376D1ADB7BF1BA1A0AC.

The first control storage launch was invalid because PowerShell treated
Cargo's progress on stderr as a terminating error; process state was checked
before a separately named successful diagnostic. The preimplementation
regression log and initial fixture-adjustment failure remain preserved.
Neither failed launch nor expected test failure is a timing observation.

Before restoration, all candidate source hashes were checked to avoid
overwriting intervening edits. The eight changed files were then restored
from their saved pre-experiment contents using scoped patches; every one of
the control's 160 source hashes matched exactly. Prototype-only tests were
restored with their implementation; existing tests were not weakened. No
frozen source, executable, raw log, baseline or audit was deleted.

Fresh post-restoration verification passes 576 Windows and 577 Linux tests,
with 21 ignored tests and 45 result groups per platform, plus both hooks-enabled
all-target Clippy gates, both default-feature all-target checks, and both
layout/storage diagnostics. Both ordinary default-feature executables were
rebuilt and passed all 49 release correctness checks; the default build paths
match their separately frozen restored copies. The eight-file nightly rustfmt
check, git diff whitespace check and changed-document trailing-space checks
pass. No prototype build remains the default executable.

The final rejection/restoration record is feedback-inline-rejection-1.json,
SHA-256 6EBE998ADEDA7E727AED94BC64261D0EEF27B5D9B42B00AD8FAA8A1DC0C68198.
It verifies all 160 restored sources, both 160-entry recoverable source
archives, 241 evidence hashes, and the fresh native/default-release gates.
All launched test, lint, build, diagnostic and timing processes are terminal.

Do not rerun this unchanged ID prototype or the earlier span/container
experiments. The current full-chunk boxed table still has the documented
large sparse-code limitation. A later redesign needs a different measured
cost/benefit case, not a claim that compact tables or vectors are generally
faster. Continue the remaining runtime/feedback workload audit and the full
40-section ledger; no full-system completion is claimed.

## Encoded I64 slot-pair execution rejected despite fewer decodes

This continuation tested a different measured runtime cost, not another
feedback-ID, span, inlining-hint or Vec/Box variant. The entire 40-section
objective, router, Rust rules/examples, architecture, VM, memory,
performance and contribution/CI gates were revisited, together with the
Ralloc owned-buffer and TinyAllocator byte-owner contracts. The relevant
stable Rust [little-endian conversion](https://doc.rust-lang.org/stable/std/primitive.i64.html#method.from_le_bytes)
and [fallible integer conversion](https://doc.rust-lang.org/stable/std/convert/trait.TryFrom.html)
contracts were checked. No dependency or external API changed.

The control remains encoder-left-descent-freeze-1.json, SHA-256
F45CBBD38F0F466C31E32046180030FB4C69370FC6E89AC22E5F13EF23D7DB9D.
All 160 source/configuration hashes matched before the experiment. Fresh
60-case control profiles on each platform confirmed the ordinary integer
loop's remaining full-value decoding work.

The prototype guarded both actual encoded operand tags inside the existing
BinarySlotsStore family. For two I64 values it read the little-endian
payloads, performed checked scalar arithmetic or comparison, and wrote a
canonical integer/boolean result without materializing intermediate Values.
Either non-I64 tag retained the shared dynamic fallback. The existing
ScalarOp definition moved to the runtime semantic owner; JIT-specific
lowering methods stayed in their subsystem. A closed Integer/Boolean result
avoided loosely related flags, and the previously duplicated checked-I64
overflow formatter was shared. Both inputs were read before any destination
write, including aliasing destinations; arithmetic errors retained their
operation origin and store errors retained their store origin.

Five production files and one integration-test file changed. No opcode,
container, cache budget, permission boundary, allocator, dependency, benchmark
fixture or unsafe code changed. JitOp/JitChunk/JitProgram stayed at
24/96/208 bytes. Four unit tests covered all ten operations against shared
results/errors and canonical bytes, both live tags across all other value
representations, aliased writes, bounds and unchanged memory on failures.
The integration regression covered arithmetic boundaries, all three
destination choices, and either operand changing to a float, against the VM
at thresholds 0/1/8 and repeated runs. The first control fixture mistakenly
used unsupported semicolons and failed to compile; the corrected fixture
passed before implementation. Both logs remain; this was a fixture correction,
not a runtime defect or performance observation.

The candidate passed 581 Windows and 582 Linux tests, 21 ignored tests and
45 result groups each, both hooks-enabled all-target Clippy gates with
warnings denied, both default-feature all-target checks, layout/storage
diagnostics, and 49 ordinary-release correctness checks per platform.
The 160-entry recoverable archive is slot-pair-i64-source-1.zip, SHA-256
0514A330EDAAD17B2EEE0E01586B98F251B3FF08336693D5EE48A2E294287D66.
The candidate freeze is slot-pair-i64-freeze-1.json, SHA-256
D578200EA6A405A4562924574E2A9701845EF5A176CC4A9C51EA52A9E81D0D97.

### Diagnostic work reduction, not speedup evidence

Four frozen hooks-build diagnostics each completed all 60 cases: 15 sources,
thresholds 0/8, cold/warm execution. Each build recorded its exact Cargo
artifact and source root; before/after hashes checked all 160 sources.
The 120 paired cases have exactly equal allocator counters, dispatch/stack
totals, encoding counts, heap-lock counts, growth and copied-byte counters.
Only full Value decoding counts differ, in 16 paired cases across platforms:

| Case, identical on Windows and Linux | Control decodes | Candidate decodes |
| --- | ---: | ---: |
| Integer, threshold 0, cold or warm | 12,290 | 4,098 |
| Integer, threshold 8, cold | 8,209 | 17 |
| Integer, threshold 8, warm | 8,193 | 1 |
| Changing operand, threshold 0, cold or warm | 16,386 | 12,290 |
| Changing operand, threshold 8, cold | 8,225 | 4,129 |
| Changing operand, threshold 8, warm | 8,193 | 4,097 |

These are codec-call counts, not instruction counts, latency, RSS, or a claim
that the remaining native work is negligible. The profile audit is
slot-pair-i64-profile-audit-1.json, SHA-256
BFDC7F058A232C2EEB3016F4E0BB00D6583BA2E2043E686D76ACC0B9288C268D.

### Fixed ordinary-release timing and rejection

The unchanged harness ran eight exact rows, nine repeats, scale 32, first
control/candidate then candidate/control on Windows, followed by Linux.
Each of 64 invocations passed all 49 printed correctness checks: 3,136 total.
Windows used inherited affinity 0x4 and scheduled cycles; Linux used taskset
CPU 2, native temporary fixtures and thread CPU time. Balanced power policy
was unchanged. CV limits were 5% for all hot_loop rows and 10% otherwise;
the primary regression flag remained 5%. No completed capture was retried.

Only 23/32 Windows and 26/32 Linux captures pass their evidence gates,
yielding 9 Windows and 11 Linux eligible whole pairs. The following primary
changes use only those pairs; positive values are slower. A dash means the
pair is excluded, not unchanged.

| Workload | Windows forward | Windows reverse | Linux forward | Linux reverse |
| --- | ---: | ---: | ---: | ---: |
| Integer, unquickened | +0.27% | -2.74% | -0.58% | +0.64% |
| Float loop | +0.03% | -8.36% | -3.63% | +1.59% |
| Portable VM loop | +3.38% | — | -1.07% | — |
| Integer, quickened | — | — | +6.66% | +4.16% |
| Changing-type loop | -12.93% | — | -5.90% | -8.29% |
| Function calls | -11.18% | -19.90% | — | -15.17% |
| Map set/get | +1.31% | — | -1.31% | — |
| Small cold lowering | — | — | — | — |

The quickened Linux target goes from 384.36 to 409.95 microseconds forward
and 406.87 to 423.81 microseconds reverse in mean CPU time. Mean cycles
increase 6.62%/4.16%, mean wall time 6.62%/4.16%, and best wall time
5.98%/0.67%. Both whole pairs pass the 5% CV gate. The first primary increase
exceeds the predeclared 5% regression flag; both realistic target comparisons
are slower. Mixed-loop and call improvements do not cancel that regression.
There is no correctness requirement or necessary future architecture that
justifies keeping this extra path. It is rejected under section 37.

Separate Cargo processes appeared late in measurement. Their observed start
times, about 22:16:15 and 22:26:33 UTC on 2026-09-09, overlap the reverse
Windows cold control and both reverse Linux cold captures respectively.
All three already fail their CV gates and are excluded. Earlier variability
is not attributed to those builds. The initial Linux launch refused a
competing build before any timing; after it ended, the same fixed plan ran
under a separately named launch log. No external process was stopped.
The read-only final audit does not require unrelated builds to stop, but
verifies that this trial's benchmarks have ended and all frozen identities
match. The timing audit preserves 184 evidence hashes:
slot-pair-i64-matrix-audit-1.json, SHA-256
3E8C2CD066AAE06F3F1F7875D496421D368EC7A325FE2AAF6869839D96423557.

All candidate source hashes were checked before restoration. The six scoped
files were restored from their exact pre-experiment contents; all 160 control
source hashes match. Prototype-only tests remain recoverable in the archive,
and existing tests were not weakened. No frozen archive, executable, raw
capture or rejected evidence was deleted. Fresh post-restoration gates pass
576 Windows and 577 Linux tests, 21 ignored tests and 45 result groups each,
both all-target Clippy/default-feature checks, layout/storage diagnostics,
and 49 rebuilt ordinary-release correctness checks per platform.

The final rejection/restoration record is slot-pair-i64-rejection-1.json,
SHA-256 8BEE819978DEAF3C018AE33B34105846A5228E97436AC76B2FFAC344993ACF41.
It verifies 265 evidence hashes, both 160-entry source archives, the restored
160-file source inventory, all four diagnostic executables and their build
identities, and the fresh native/default-release gates. The six-file nightly
format check, diff whitespace check and document trailing-space check pass.
Every process launched for this trial is terminal. Other independently
launched builds are outside this trial and were not controlled or terminated.
No new Miri, hardware-instruction, peak-memory or full-pipeline claim is made.

Vec/Box ownership policies remain unchanged: this trial establishes no
general speed advantage for either. Do not rerun this unchanged prototype.
Further representation work needs a different measured execution-cost
explanation, not acceptance by decode counts alone. The full 40-section goal
remains active.

## Constant-cardinality map-query allocation baseline

This is progress on the collection and repeated-work audit, not an accepted
runtime optimization. The full 40-section objective remains active. It does
not repeat the rejected numeric-key narrowing, removal, feedback-ID or
encoded slot-pair prototypes, and establishes no Vec-over-Box speed claim.

### Scope and ownership contracts

The source review found that canonical map queries own a fresh String copy
for string contents and clone pointer field names. This happens even for
reads, misses and updates to existing keys. The index owns its stored keys;
the insertion-ordered entries remain Ralloc-owned. Heap generations, pointer
cast-insensitive equality, validation of every stored pointer base, original
encoded key identity, logical byte budgets and error ordering must survive
any future borrowed-query change.

The objective and repository/Rust instructions were reread during continuation;
the memory, stdlib, performance, contributor and CI contracts were checked.
The relevant standard-library contracts were also inspected:
[Cow ownership](https://doc.rust-lang.org/stable/std/borrow/enum.Cow.html),
[HashMap borrowed-key Hash/Eq requirements](https://doc.rust-lang.org/stable/std/collections/struct.HashMap.html#method.get),
and [lifetime variance](https://doc.rust-lang.org/reference/subtyping.html).
No Cow implementation, custom hasher, new dependency or unsafe code is added.
In particular, a future borrowed query must not let a heap reference escape the
heap-lock window or substitute a borrow for an owned installed index key.

Only tests/map_profile.rs changes among the 160 source/configuration files.
The added ignored test reuses the existing process-local host allocation
observer; it is absent from production and the ordinary latency harness.
The default command/edit launchers failed before execution with an ACL setup
error. The same installed apply_patch executable was used through the approved
command route without changing permissions. An initial legacy Windows argument
quoting error affected only the new test, was caught by rustfmt, and was repaired
before the source freeze, builds or measurements.

### Fixed probe and measured cost

Each fresh execution constructs a one-entry map. Key shapes are an integer,
8-byte string, 256-byte string and field pointer. Operations are get, has,
missing has, missing delete and existing-key update, each with 0/1/4,096
queries. JIT thresholds 0/8 and first/repeated executions produce 240 cases
per platform. Every case first checks exact VM output, then JIT parity,
balanced Ralloc allocation/deallocation, zero live Ralloc payload at return
and agreement of the runtime/allocator growth and copy counters.

Compilation, output reservation and reporting are outside the host interval;
execution setup, queries and runtime cleanup are inside. Each diagnostic ran
alone. Windows used stable Rust 1.98.0 and Linux used the existing Arch WSL
nightly 1.99.0 toolchain with separate Cargo artifacts. Both core libraries
and diagnostic binaries were freshly compiled from the recorded source tree;
each executable was copied and hashed before execution.

All 480 new query cases pass. The audit pairs all 240 Windows/Linux cases:
host counts, requested byte totals, size buckets, complete runtime counters
and complete Ralloc records match exactly. For the repeated threshold-8 get
case, subtracting the zero-query execution gives:

| Key | Additional host allocations for 4,096 gets | Additional requested host bytes |
| --- | ---: | ---: |
| Integer | 0 | 0 |
| 8-byte string | 4,096 | 32,768 |
| 256-byte string | 4,096 | 1,048,576 |
| Field pointer (five-byte field name) | 8,192 | 40,960 |

The one-query controls scale exactly to these 4,096-query deltas on both
platforms. For strings and integers the same incremental counts hold for all
five operations in the repeated threshold-8 profiles. Field-pointer misses
use a four-byte field name and therefore request 32,768 additional bytes,
still in 8,192 allocations. These pointer totals include other runtime
metadata copies; they must not all be attributed to map canonicalization.
The audit retains every count/threshold/phase, not just the table subset.

This is cumulative requested allocation traffic, not peak memory, retained
capacity, allocator-usable bytes or portable allocation guarantees. It shows
where work is spent; it does not establish that a borrowed representation is
faster, nor that either Vec or Box should replace the other.

### Evidence and validation

Evidence is under target/perf/full-system:

- map-query-control-freeze-1.json, SHA256
  60D9D1BDE3B0860790FC254E193B1C174B9FDECD403422984FE09D4D56392B23.
- map-query-control-source-1.zip, SHA256
  58B27E0BBB1E1D2424562D7A2F9E2BF1933CE4709EC7791B9507EA3834E99B63.
- map-query-control-profile-{windows,linux}-1.json identify the frozen
  diagnostic executables, raw logs, build messages and source manifest.
- map-query-control-audit-1.json, SHA256
  1A09AA8CD375988AFC9F27D174CE0A3404465C674AE9FF0650E02F211557C8FF,
  verifies 33 evidence hashes, all 160 current and archived source hashes,
  480 query records and the native gates. The only source/configuration
  difference from the encoder-left-descent control is the new diagnostic.

Fresh workspace hooks suites pass 576 Windows / 577 Linux tests, with 22
ignored tests and 45 result groups per platform. The ignored query probe is
run explicitly as above. The existing removal probe also passes all 144 cases
on each platform, adding 288 separately executed regression records.
Both platforms pass all-target hooks Clippy with warnings denied,
all-target default-feature checks, layout/storage diagnostics and all 49
ordinary-release correctness checks. Ordinary non-instrumented benchmark
binaries were rebuilt and frozen after profiling.

Layouts remain JitOp 24 bytes, JitChunk 96, JitProgram 208, feedback site 11,
MapKey 64/alignment 16, indexed entry 80 and MapData 112 on both native builds.
Fixed metadata stays boxed; installed opcode Vec storage stays slice-only,
bounded by its existing slack policy and charged at full capacity. Builders
and genuinely growable stacks remain Vec-backed. The prior Vec/Box timing
comparison is still inconclusive. No ordinary timing, new Miri, hardware
instruction, peak-memory or full-system acceptance result is claimed here.

The next distinct candidate is a scoped borrowed query with an owned index,
starting with ownership/error/generation tests and before/after allocation
profiles. It still needs ordinary string-map timing and mutation/integer/
unrelated guardrails before retention. Do not treat this control-only
checkpoint as an implemented lookup optimization or completion of the goal.

## Borrowed map queries rejected by ordinary mutation workloads

Date: 2026-09-12. Status: PROGRESS, not full-system acceptance. The
constant-cardinality baseline above led to a distinct borrowed-query prototype.
Its allocation savings are real, but it is removed under section 37 of the
requested plan because realistic map programs regress repeatedly. The full
40-section objective remains active.

### Prototype, ownership and retained boundary tests

The trial used MapKey<'a> with Cow<'a, str> text payloads and an installed
HashMap<MapKey<'static>, usize>. Read-only lookup shortened an immutable view
of the owned index to the query lifetime; it retained no borrowed text.
Canonicalization borrowed validated heap strings or a pointer field under the
same heap lock. An exhaustive MapWrite enum separated existing-entry updates
from insertion; only insertion converted the query into owned index metadata.
Encoded entries stayed insertion-ordered and Ralloc-owned. Original encoded
keys were preserved on update.

The indexed integer domain remained I64/U8/U16/U32 with i128 canonical values.
The known narrower-signed/U64 indexing gap was not silently expanded.
Every operation still checked stored pointer bases even for unrelated query
types. Pointer casts stayed irrelevant to key identity. No allocator, hasher,
dependency, unsafe code, bytecode, cache or Vec/Box policy was introduced.

Three tests in crates/tinyone_core/tests/stdlib_parity.rs remain after rollback:

- Equal-but-distinct, empty and UTF-8 strings; original encoded-key identity;
  freeing the original string while independently owned index text survives;
  repeated updates, deletion and reinsertion.
- Field-pointer identity across different fields and casts, original encoded
  cast metadata on update, and insertion-order/new-metadata behavior after
  deletion and reinsertion.
- Nine stale-pointer and error-precedence operations, including unrelated
  string queries and direct integer get/set; exact Runtime category,
  map-queries.to:10:7 source location and output-before-error parity.

The tests use explicit expected results established against the control,
plus VM/JIT thresholds 0/1/8 with repeated execution. The first control run
exposed a test expectation error: boolean comparisons print "true", not "1".
That expectation was corrected before production edits; its failed log is
preserved as borrowed-map-control-boundaries-1.log. Both corrected control
runs and the candidate/restored suites pass.

The complete objective, engineering router, Rust rules and four examples were
read. Heap/context/generation, memory-model and Ralloc-owned storage contracts
were checked; relevant std Cow ownership, HashMap lookup and lifetime-variance
documentation was consulted. No external dependency API or allocator
implementation changed.

### Allocation savings are not runtime acceptance

Frozen, isolated release diagnostics produce 240 cases per platform.
The before/after audit pairs all 480 cases: 240 change, saving exactly one
allocation/deallocation and the key's text-byte length per query for the
profiled nonempty string/field-pointer keys. Integer and zero-query controls
are unchanged. Every runtime/Ralloc field, reallocation count/byte total and
unrelated host size bucket matches its control.

For 4,096 warm threshold-8 string reads or updates, 8-byte keys reduce host
allocations from 4,105 to 9 and requested bytes from 35,774 to 3,006.
For 256-byte keys, allocations likewise fall to 9 and requested bytes fall
from 1,052,326 to 3,750. Field-pointer hits reduce host allocations from
8,212 to 4,116 and requested bytes from 44,472 to 23,992. The remaining
pointer-query allocation is other runtime metadata work, not an unremoved
copy of this canonical key. These are cumulative host-traffic observations,
not peak-memory, retained-capacity or per-query latency measurements.

Layouts remain MapKey 64 bytes/alignment 16, indexed entry 80 and MapData 112;
JitOp 24, JitChunk 96, JitProgram 208 and feedback site 11. Hash-input
diagnostics are unchanged, without claiming a portable hashing guarantee.

### Fixed ordinary-release matrix and rejection

The frozen ordinary control and candidate run ten target/mutation workloads
on Windows and Linux, with two opposite-order pairs, nine repeats and sample
scale 16. Exact filters and actual iteration counts are checked against the
predeclared runner. Windows uses observed Normal priority and affinity 0x4;
Linux uses taskset CPU 2 and native /tmp fixtures. Balanced power is unchanged.
There are no concurrent builds, source edits, archives or allocation profiles
during either timing matrix, and no adaptive retries.

All 80 invocations pass 49 correctness checks, totaling 3,920. The fixed CV10%
gate excludes 15 Windows captures and one Linux capture. Only complete
eligible pairs are compared: 10 Windows and 19 Linux pairs, 29 of 40 total.
The other 11 pairs remain excluded, including six individually eligible
captures whose partners failed quality. The quality-only exit code 1 for
each completed matrix is not a runtime failure.

The table shows candidate cost changes for the forward / reversed pair.
Negative is lower cost. Windows primary is mean scheduled-thread cycles;
Linux primary is mean thread CPU time. These are complete TinyOne program
fixtures, not isolated operation latencies.

| Workload | Windows primary change | Linux primary change |
| --- | ---: | ---: |
| JIT string lookup (4,096) | -17.74% / -18.36% | -5.20% / -5.26% |
| VM string lookup (4,096) | -11.49% / excluded | -0.98% / -0.74% |
| JIT map set/get (256) | -0.96% / excluded | +3.55% / +7.17% |
| VM map set/get (256) | +1.47% / -1.98% | -0.61% / +5.74% |
| JIT integer hit (256) | excluded / -3.13% | -0.63% / +5.15% |
| JIT integer miss (256) | excluded / excluded | -1.24% / -0.03% |
| JIT integer update (256) | excluded / excluded | +7.43% / +5.24% |
| JIT in-capacity insertion (256) | +0.87% / excluded | +1.32% / +1.07% |
| JIT deletion (256) | +1.72% / +0.85% | -1.29% / excluded |
| JIT capacity growth (4,096) | excluded / excluded | +9.22% / +2.12% |

The string-JIT benefit repeats on both platforms, but it does not cancel
Linux JIT set/get, update, insertion and growth increases, or the smaller
repeatable Windows deletion cost. The 5% primary regression flag was never
permission to ignore smaller consistent regressions. This ownership rewrite
is not necessary for correctness or a required future architecture, so the
plan's exception does not apply.

Mean/best wall and Linux TSC remain supplemental in the audit. For example,
Linux update mean wall increases 7.43%/5.24% and best wall 6.06%/5.20%.
The mixed-direction map-VM and hit results are not called consistent
regressions, and excluded pairs support neither wins nor regressions.
No cause is assigned to the noisy captures.

The predeclared conditional guardrails are not run after target rejection:
heap-key identity, vector, hot/mixed/float loops, calls and cold straight-line
lowering. In particular, the existing pointer_key_validation benchmark
actually uses heap-cell identity, not a field-pointer fixture; this trial
makes no field-pointer latency or cold-start claim.

### Evidence, exact restoration and fresh validation

Evidence is under target/perf/full-system:

- borrowed-map-candidate-freeze-1.json, SHA256
  382FCA548DF626457B8D714C01CE7B5045958F7C3561DF94E58FFE4D08CDD955,
  and borrowed-map-candidate-source-1.zip, SHA256
  38FEC723EB757A9ED05C5458AF24F8C37C99E6CCFF0288BC7809EAD3F78D55FD.
- borrowed-map-profile-audit-1.json, SHA256
  4BDD7C9CC01B10BADACA1EAD3DDEFF72713598D84421B23CD92A462D679766A9,
  records all 480 before/after query comparisons.
- borrowed-map-verified-1.json identifies candidate source/build/test evidence
  and all four frozen ordinary binaries. Candidate correctness suites pass
  579 Windows / 580 Linux tests before timing.
- borrowed-map-timing-audit-1.json, SHA256
  AF76BA9DC5E163C39D119C58E8B8AFD56DA3BB8B09FBA5357A43CCE39FA78D49,
  verifies 233 evidence hashes, exact run order, raw/event/saved JSON
  agreement, metrics, counters, scheduling records and quality exclusions.
- borrowed-map-restored-freeze-1.json, SHA256
  887EB8BBDADD5B5F3071A6490255F6D534C97BF80F0BDAA338DF45A09F0A75C1,
  and borrowed-map-restored-source-1.zip, SHA256
  2CEA9DB962ABACA9CBDE83978F35BF78765B4676AB109E52A2D9F3FFFB97211E.
- borrowed-map-restored-audit-1.json, SHA256
  1AEB0460FC1E2E127B26CA05B191DA429779332D9B60C79976F85475DC37DA0B,
  verifies all 160 restored source/archive entries, 36 checkpoint evidence
  hashes and the earlier control/timing evidence chains.

Both production files match their recorded original hashes exactly:
runtime/heap.rs E755E8E56A6E16505EF1838D5A15DE91738408B89612D9823562A4AB4E6AAAF8;
runtime/stdlib.rs 65FB20F89B905659A31EEA12FE9E271F117283DAC3A2363313446B21B57278F0.
Only stdlib_parity.rs differs from the map-query control's 160-file inventory.
The candidate archive remains available for inspection; no unrelated user
changes were reset, staged or removed.

Fresh restored workspaces pass 579 Windows / 580 Linux tests, 22 ignored
tests and 45 result groups each, plus all-target hooks Clippy with warnings
denied, default-feature all-target checks and layout/storage diagnostics.
All 480 restored query records exactly match their original native controls
in every host, runtime and Ralloc field. The 144 removal cases per OS pass;
no additional full before/after removal-counter audit is claimed.
Both freshly rebuilt ordinary releases pass all 49 correctness checks.
The restored Linux binary is byte-identical to the original Linux control.

The stable-format check's failed log is retained: it cannot apply the
repository's nightly-only settings. The installed-nightly full-workspace
cargo fmt --all --check passes without modifying files. Two audit/setup
issues were corrected before acceptance: the canonical Windows fixture path
has a verbatim prefix, and this PowerShell requires the compression assembly
to be loaded explicitly. Neither was a runtime or benchmark failure.

Fixed metadata remains boxed. Installed opcode Vec storage remains slice-only,
reclaims slack at the existing 4 KiB boundary and accounts for full capacity;
builders and genuinely growable stacks remain Vec-backed. The Vec/Box speed
comparison is still inconclusive. There is no new Miri, hardware-instruction,
RSS, peak-memory or full-system acceptance claim.

Do not retry this combined candidate unchanged. A distinct read-only borrowing
experiment could preserve the original map-write control flow and measure
whether string-read savings survive without integer-mutation costs. That is
a separate hypothesis requiring its own frozen control and validation, not
an accepted optimization or completion of the remaining plan.

## Read-only map borrowing rejected by mutation screen

The preceding turn was progress: it rejected the combined borrowed-key/typed-write
trial on ordinary workloads, restored production exactly and retained three
control-compatible semantic regressions. This turn tests the distinct narrower
hypothesis suggested there. It is also rejected; neither candidate should be
retried unchanged. The full forty-section objective remains active. This is an
evidence checkpoint, not the section 39 final implementation report.

### Scope and ownership

The experiment changes only runtime/heap.rs and runtime/stdlib.rs relative to
the verified borrowed-map-restored checkpoint. MapKey text uses Cow, installed
index keys remain independently owned with a static lifetime, and a read-only
HashMap view shortens the key lifetime for a temporary lookup. A statically
selected text-construction callback shares canonicalization: get/has borrow;
set/update and delete still construct owned query keys. There is no MapWrite
state or whole-enum into_owned conversion.

Exact function-body comparisons confirm b_map_set, b_map_set_i64,
b_map_set_encoded, b_map_get_i64, b_map_del and validate_map_pointer_base remain
unchanged. The shared key type, hashing/equality/drop code and canonicalization
implementation still change, so this is not a claim that mutation machine code
is identical. The cause of the measured latency difference is not isolated.

Heap strings remain immutable and query borrows cannot outlive the same heap
lock. Installed text never borrows from runtime strings or pointer fields.
Generation checks, validation of every stored pointer base, canonicalization
before target validation, original encoded-key identity on update, insertion
order and cast-insensitive pointer equality remain intact. The existing
I64/U8/U16/U32 indexed integer domain is preserved; other integer-width indexing
debt is not silently changed. No unsafe, allocator, custom hasher, dependency,
bytecode, cache or Vec/Box policy change is introduced.

The full objective, engineering router, Rust rules/examples, subsystem and
contribution/testing documents were read before implementation. Ownership was
checked against heap_str, the context heap lock, RallocBytes and Ralloc storage
APIs. Relevant standard-library contracts were checked in the official
[Cow documentation](https://doc.rust-lang.org/std/borrow/enum.Cow.html),
[Cow hash implementation](https://doc.rust-lang.org/src/alloc/borrow.rs.html),
[HashMap lookup contract](https://doc.rust-lang.org/std/collections/struct.HashMap.html#method.get),
[lifetime variance reference](https://doc.rust-lang.org/reference/subtyping.html)
and [FnOnce documentation](https://doc.rust-lang.org/std/ops/trait.FnOnce.html).

### Correctness and allocation attribution

All seven existing map regressions pass, including equal-but-distinct strings,
freeing the original inserted string, field/cast identity, insertion order and
precise stale-pointer error/output parity across VM and JIT thresholds 0/1/8. No new test
was added just to increase test count. Candidate native suites pass 579 Windows
and 580 Linux tests, with 22 ignored and 45 result groups each. All-target hooks
Clippy with warnings denied, default-feature all-target checks, nightly
formatting, layout/storage diagnostics, 144 removal cases per OS and 49 ordinary
release correctness checks per OS pass.

Both newly compiled frozen release diagnostic executables have confirmed source
manifest, testing-hooks feature and fresh core/test artifact identities. They
run separately. Their 240 query cases each are compared with the original
map-query control counters, whose exact equality to the restored control was
already audited. All 480 comparisons pass: precisely 144 cases remove one text
allocation and its requested key-byte length per read. Get/Has/Missing on
nonempty string or field-pointer keys change; Update/DeleteMissing, integer
queries and zero-query controls do not. Every runtime/Ralloc field, reallocation
count/byte total and unrelated allocation-size bucket is identical.

For 4,096 repeated threshold-8 queries, both platforms report:

| Key | Get allocations, control → candidate | Get requested bytes, control → candidate | Update / missing-delete |
| --- | ---: | ---: | --- |
| 8-byte string | 4,105 → 9 | 35,774 → 3,006 | Exactly unchanged |
| 256-byte string | 4,105 → 9 | 1,052,326 → 3,750 | Exactly unchanged |
| Field pointer | 8,212 → 4,116 | 44,472 → 23,992 | Exactly unchanged |

These are cumulative host-allocation traffic observations, not peak memory,
RSS or latency. Field-pointer reads still have other runtime metadata copies.
MapKey remains 64 bytes/alignment 16, indexed entry 80 and MapData 112. Observed
hash inputs remain integer 24, string 12, heap 24 and pointer 46 bytes. JitOp 24,
JitChunk 96, JitProgram 208 and feedback-site 11-byte layouts are unchanged.

### Predeclared ordinary screen and rejection

Before any ordinary capture, the protocol and decision audit were fixed and
hashed. The first stage runs JIT map set/get, update and deletion on Windows
and Linux: two opposite-order control/candidate pairs per row, nine repeats,
scale 16 and exactly 8,000 iterations per row. Windows uses affinity 0x4 and
Normal priority; Linux uses taskset CPU 2 with native /tmp fixtures. Balanced
power policy is unchanged. 10% CV applies to every screen row; a comparison is
eligible only when both captures qualify. There are no adaptive retries.

The seven remaining string/map target rows and seven heap-identity/vector/
loop/call/cold guardrails are conditional on the completed screen finding no
consistent realistic-workload regression. They would add 112 captures. No
builds, source edits, archives, hashing sweeps or allocation profiles overlap
ordinary captures.

All 24 captures complete and each passes all 49 correctness checks (1,176 total).
Windows has 12 eligible captures and six eligible pairs. Linux excludes five
captures, leaving seven eligible captures and only one eligible pair per row.
The audit verifies 76 evidence hashes, 19 eligible captures and nine eligible
pairs overall.

Primary candidate change from control, forward / reverse order:

| Screen workload | Windows scheduled cycles | Linux thread CPU time |
| --- | ---: | ---: |
| JIT map set/get | −1.87% / −2.12% | Excluded / −2.20% |
| JIT map update | +2.20% / +1.51% | −1.50% / Excluded |
| JIT map deletion | −0.54% / +4.29% | +1.92% / Excluded |

The five Linux exclusions are forward set/get control (13.75% CV), reverse
update candidate/control (29.10%/16.41%) and reverse deletion candidate/control
(18.93%/12.97%). Excluded point estimates do not enter the table or decision.
No cause is attributed to the increased variance.

Windows updates also increase mean CPU time by 2.46%/2.37%, mean wall time by
3.15%/1.43%, and both best-cycle measurements by 3.21%/1.90%. The repeatable
eligible update cost violates section 37; 5% is a reporting flag, not permission
to retain a smaller consistent regression. There is no correctness or necessary
future-architecture exception. Reject the candidate and skip all 112 conditional
captures. No current string-read, raw-pointer, cold-start or broader runtime
latency improvement is claimed from this stopped experiment.

### Restoration and retained documentation

Both production files are restored exactly, and all 160 current source hashes
match the prior borrowed-map-restored source archive. That unchanged archive
is reused rather than creating a duplicate. Heap source hash is
E755E8E56A6E16505EF1838D5A15DE91738408B89612D9823562A4AB4E6AAAF8;
stdlib source hash is
65FB20F89B905659A31EEA12FE9E271F117283DAC3A2363313446B21B57278F0.
The existing three semantic regressions are unchanged; unrelated user changes
are preserved. The rejected candidate archive and all raw evidence remain.

Fresh restored workspaces pass 579 Windows / 580 Linux tests, 22 ignored
tests and 45 result groups each, all-target hooks Clippy with warnings denied,
default-feature all-target checks and layout/storage diagnostics. All 480
restored query records exactly match their original native controls in every
host count/byte/bucket, runtime and Ralloc field. All 144 removal cases per OS
pass; no new full before/after removal-counter comparison is claimed. Both
freshly rebuilt ordinary releases pass all 49 correctness checks. The restored
Linux binary is byte-identical to the original control. Fresh nightly
full-workspace formatting also passes. The restoration audit checks all 160
current/archive sources, 35 evidence hashes and the original control and new
timing evidence chains.

Separately, docs/syntax/types.md incorrectly said live strings cannot be
explicitly freed. Its ownership paragraph now permits unsafe free(text) and
explains that aliases become stale. This matches existing TinyHeap::free
behavior and the passing VM/JIT original-string identity regression; no
language behavior changed.

The candidate manifest generator initially failed PowerShell parsing because
a JavaScript replacement string expanded a dollar-quote sequence. Literal
replacement fixed it before manifest writes or timing, and all experiment
scripts parsed before capture. An orchestration command initially ended after
the successful allocation audit; both candidate verification scripts were
then run independently to completion. These were tooling/setup errors, not
runtime failures.

### Evidence identities

All paths below are relative to target/perf/full-system; SHA-256 values are
complete. Frozen diagnostic binaries, build identities and raw counter records
are linked by the allocation audit; raw ordinary logs, quality exclusions,
scheduling records and saved eligible baselines are linked by the screen audit.

| Evidence | SHA-256 |
| --- | --- |
| Reused borrowed-map-restored-freeze-1.json | 887EB8BBDADD5B5F3071A6490255F6D534C97BF80F0BDAA338DF45A09F0A75C1 |
| Reused borrowed-map-restored-source-1.zip | 2CEA9DB962ABACA9CBDE83978F35BF78765B4676AB109E52A2D9F3FFFB97211E |
| map-read-borrow-candidate-freeze-1.json | 6657FC3A3A6DD15B711058A4F185B5188852A578B96370B08CBDBC3C79C599DE |
| map-read-borrow-candidate-source-1.zip | A15F990382F560CF3BA5E96F904D66D580415FD85049B4C447E2BEDE79B17DAB |
| map-read-borrow-profile-audit-1.json | 6934077508FE58F9F895FF4EBB836AC790A03FF00FC23734CC457A620036EB6F |
| map-read-borrow-verified-1.json | 4C82FD4B772052FE480A61F677A94F78C1623560917B9195509E39A6D0DD86AC |
| map-read-borrow-matrix-1.ps1 | 9C32D18E6B287731673EBD452F98A9472AEFA9D63127983D10F742DC1222175C |
| map-read-borrow-screen-audit-1.ps1 | 07DF1D5D68058C27FEDD309EBF50015D6D8C0BEDF6169B1A20AF4EF1A53241D9 |
| map-read-borrow-screen-audit-1.json | ABAB7CC679CED6AD441DFB33F492110AFE99266FFB790379C95F4F93BB2A088E |
| map-read-borrow-restored-audit-1.json | 95696BD312D98F07E4D2958B0DD1561639E411DE68C663DCA77CA103523AD627 |
| map-read-borrow-candidate-windows-1.exe | 40147794786C0E3810C812E6BCD4ACEE33EE610290E7F814782187DA6DE9EF77 |
| map-read-borrow-candidate-linux-1 | 3BB3088CF4EC6BA53B9985F0083C77A12C229FA1F993C418292D934D3B0F6695 |
| map-read-borrow-restored-windows-1.exe | 39764A9716D67918AFE514F37B49B5A5A3EC750B6FB8712E868D26BE7C41DA63 |
| map-read-borrow-restored-linux-1 | FC82EC7992BFDA4E6D64AE1D0447EE00488F64783567AF12AE8C59E46FC6CD86 |

Windows screen terminal: 2026-09-12T21:20:18.6048798Z, exit 0.
Linux screen terminal: 2026-09-12T21:27:37.9996600Z, exit 1 for its five quality
exclusions, not correctness failures. No benchmark, build or profile remains
active at this checkpoint.

Fixed metadata remains boxed. Installed opcode Vec storage is still private,
slice-only, reclaims slack at the existing 4 KiB boundary and charges its full
capacity. Builders and genuinely growable stacks retain Vec storage. The
Vec/Box timing result remains inconclusive. There is no new Miri,
hardware-instruction, RSS, peak-memory or full-system acceptance claim.

The narrower hypothesis is now answered: preserving mutation ownership and
control-flow bodies did not remove all ordinary mutation costs of this shared
borrowed-key representation. Continue the remaining whole-system plan from
the restored checkpoint; neither allocation counts nor this negative result
establish Vec superiority or complete the goal.

## Verifier metadata allocation and timing baseline

This checkpoint adds measurement coverage, not a production optimization.
All library sources match the restored read-only-map control. The only
existing Rust file changed is the benchmark registry; three new Rust files
share raw verifier fixtures, provide a test-only allocation diagnostic, and
register isolated metadata timing rows. No production visibility, allocator,
certificate ownership, or Vec/Box policy changed.

The candidate cost is eager diagnostic-label construction: successful budget,
identifier/owner and function checks call `format!` before discovering that no
error needs reporting. That is a source-level hypothesis, not an attribution
of every allocation below to formatting. Module tables, traversal scratch and
certificates also allocate. No lazy-label implementation has been applied.

### Measurement contract

Public `BytecodeVerifier::verify(&Program)` includes budget and module checks,
all chunk verification, certificate construction **and destruction**. It is not
certificate-only timing. Fixture construction, source compilation, VM/JIT
expected-output checks, input cloning and report formatting are outside the
observed interval. No internal compiler/verifier API was exposed.

The historical `compiler.emit_bytecode` row still calls
`compile_source_unoptimized`: lexing, parsing/emission and mandatory
verification are included. Its name and timed body are preserved for
historical comparisons; it does not establish emission-only cost.

Raw fixtures keep a three-op main printing `42`, varying unused functions
(0/1/16/256/4096), root names/slots (the same counts), or one struct's fields
(0/1/16/256). These isolate metadata scaling, not general large-program
throughput. Four new ordinary rows cover 256 raw functions, 4096 names, 256
struct fields, and the existing real 256-declaration source. Shared raw
fixtures receive VM/JIT output checks during benchmark setup, independently
of the existing 49 printed pre-timing checks.

### Allocation observations

The release diagnostic reuses the existing test-only counting `System`
wrapper. Fourteen shapes, 0/1/32 verifier calls and three samples produce
126 records per OS. Every interval asserts unchanged input, balanced host
allocation/deallocation calls, zero runtime/Ralloc counters, and empty
zero-call observations. Three samples agree exactly; every 32-call record
is exactly 32 times its one-call record. All 126 matching Windows/Linux
records agree across every recorded host count/byte/bucket field.

Representative one-call observations, identical on both builds:

| Raw fixture | Allocations = deallocations | Reallocations | Allocated bytes requested | New realloc extents requested |
| --- | ---: | ---: | ---: | ---: |
| Empty functions/names | 4 | 0 | 128 | 0 |
| 1 function | 17 | 3 | 663 | 61 |
| 256 functions | 2,829 | 774 | 116,700 | 23,680 |
| 4,096 functions | 45,073 | 12,298 | 1,863,964 | 380,800 |
| 1 name | 5 | 1 | 136 | 16 |
| 4,096 names | 4,100 | 4,096 | 32,896 | 65,536 |
| 0 struct fields | 11 | 3 | 399 | 55 |
| 256 struct fields | 267 | 259 | 6,031 | 11,319 |

These are build-specific complete-verifier allocation observations, not
label-only counts, bytes copied, latency, RSS, peak memory or Rust allocation
guarantees. The latter limitation follows the standard library's
[GlobalAlloc contract](https://doc.rust-lang.org/stable/std/alloc/trait.GlobalAlloc.html#safety);
the diagnostic adds no unsafe code to the existing observer.

Windows' native test printed success with all assertions passing, but its
coordinator exited 1: a literal-space regex counted 125 of 126
PowerShell-wrapped records. A separate whitespace-tolerant audit recovers all
126 original records without rerunning or overwriting the capture. The native
Windows exit code was not separately persisted. Linux's coordinator exited 0.
The allocation audit records this recovery and its limit.

### Ordinary release baseline

Seven exact verifier filters ran twice per OS, sequentially, with nine repeats,
scale 16 and a 10% CV limit. Windows used affinity `0x4`, Normal priority and
rustc 1.98.0; Arch WSL used CPU 2, native `/tmp` fixtures and rustc
1.99.0-nightly (2026-08-05). The existing Balanced power policy was unchanged.
No builds, profiles, source edits or hash sweeps ran during captures.

Both coordinators exited 0. All 28 captures qualify, all 1,372 printed
correctness checks pass, and all saved JSON rows match stdout. Maximum CV
is below 2.03% on Windows and 1.88% on Linux. Each cell preserves capture
1 / capture 2. These are separate same-platform baselines, not a cross-OS
comparison or an improvement over an earlier executable.

| Verifier row | Windows mean scheduled cycles | Linux mean thread CPU us |
| --- | ---: | ---: |
| functions_256 | 1,020,785 / 1,015,174 | 186.417 / 185.494 |
| names_4096 | 1,879,369 / 1,892,804 | 198.865 / 198.312 |
| struct_fields_256 | 123,187 / 121,906 | 22.228 / 22.008 |
| declarations_256 | 1,381,934 / 1,382,299 | 264.302 / 265.154 |
| loop_cfg | 3,881 / 3,876 | 0.856 / 0.853 |
| function_cfg | 16,518 / 16,640 | 3.265 / 3.320 |
| heap_structs | 11,182 / 11,181 | 2.284 / 2.296 |

### Verification and provenance

The 163-entry freeze has 159 prior files unchanged, one changed benchmark
registry and three additions. Source/archive entries were checked at freezing;
final audits recheck all current sources and the archive hash. Windows/Linux
hooks workspaces pass 580/581 tests in 46 groups, with 23 ignored diagnostics
each. All-target hooks Clippy with `-D warnings`, default-feature all-target
checks, full nightly formatting and `git diff --check` pass. Each ordinary
release also passes 49 correctness checks. Cargo JSON identifies the expected
manifest, freshly built benchmark and absence of `testing-hooks` in both
ordinary timing executables.

Machine-local evidence remains under `target/perf/full-system/`. The timing
audit hashes 106 evidence files and chains the allocation audit, retaining
raw captures, build messages and quality metadata.

| Evidence | SHA-256 |
| --- | --- |
| verifier-baseline-freeze-1.json | 72AE2277269EDF44E3CA1048AB6A030199919D879ACA9DFDEDEA46428D5ED1BD |
| verifier-baseline-source-1.zip | D2C315194271DAAFC803B79FF6B8B38D082B8A724F1DA8284B76A90C59B862DC |
| verifier-baseline-allocation-audit-1.json | 8FCC658D527BBE6A1F3DF8B7F9EF6CD0C5173C7534CCDFCBC1D98AC57783D82B |
| verifier-baseline-timing-audit-1.json | 32382AF57022617BA3500E683F8884212651A0194B354CC557E00660A7AA47C4 |
| verifier-baseline-windows-1.exe | DAD139065C7F1D9910EEDF0A06E2B507CFA58117B40D2BC0A6C46F0C1E57DF1B |
| verifier-baseline-linux-1 | 7F23A7B30A2E13F7B227F6F6173F05FC3B31F91ED1D9EECFA870DAFA79859103 |

Windows timing finished at 2026-09-12T22:22:29.8169773Z; Linux finished at
2026-09-12T22:25:49.5730406Z. All build/profile/benchmark handles are terminal.
There is no new hardware-counter, Miri, peak-memory, verifier-speedup or
full-system completion claim. Fixed metadata stays boxed; installed `JitOps`
retains private slice-only Vec access, 4 KiB slack reclamation and full-capacity
accounting. Vec/Box superiority remains unproven.

Next, use this baseline to select a small diagnostic-allocation change only
after reading its full helper/caller contracts. Preserve exact error text,
ordering, resource limits and module ownership. Any candidate still needs
matched allocation evidence, ordinary verifier timings and end-to-end
compilation/module/cold/runtime guardrails. Certificate-only attribution,
broader phase/workload coverage and remaining full-system implementation,
cleanup and reporting requirements stay open.

## Lazy verifier diagnostic labels: rejected whole-system trial

The lazy-label production candidate is **not retained**. Although it removes
substantial complete-verifier allocation traffic and improves every usable
verifier pair, Windows `api.run_source_jit_warm` regresses in both qualifying
opposite-order pairs. Its production changes and candidate-only rendering test
are removed; three independently useful semantic tests remain. The complete
40-section plan, including wider coverage, cleanup and final reporting, stays
open.

### Scope, documentation and correctness

Reconfirmed the full objective, the applicable home AGENTS router, Rust rules
and all four required examples. The verifier architecture, bytecode rules,
module/capability semantics, Rust API and performance contracts were consulted
alongside the previously read contributor, CI, memory and VM documentation.
Checked the standard library's
[format_args!](https://doc.rust-lang.org/stable/std/macro.format_args.html),
[Arguments](https://doc.rust-lang.org/stable/std/fmt/struct.Arguments.html) and
[Display](https://doc.rust-lang.org/stable/std/fmt/trait.Display.html) contracts
and [thiserror 2.0.18](https://docs.rs/thiserror/2.0.18/thiserror/).
Exact-version format documentation was unavailable through the initial URL;
the official stable pages retrieved reported 1.98.0/1.98.1. No dependency,
unsafe code, public API, container policy or allocator implementation changed.

The candidate replaced 21 eager label constructions with borrowed
`format_args!` values passed synchronously to three private `impl Display`
parameters. It did not store formatting arguments. The existing function
chunk-name String, CFG traversal, certificates, index/stack checks and
permission algorithms were deliberately unchanged. All predicates, error
templates and check order stayed the same.

Three semantic tests first passed on the original production implementation
and remain after restoration:

- Exact UTF-8 byte boundaries, count-before-text checking and the first
  cumulative text-budget crossing.
- Debug-escaped names and budget errors preceding invalid-identifier errors.
- Root/qualified identifier roles, invalid qualifiers/local names, extra
  separators and unknown-module diagnostics, including the Compile category.

A fourth, candidate-only test used a Display implementation that panicked if a
successful metadata check rendered its label. That test passed with the
candidate and was removed with the Display-taking implementation. All eleven
candidate verifier unit tests passed. Full candidate hooks workspaces passed
584 Windows / 585 Linux tests, with 23 ignored tests in 46 result groups each.

### Allocation evidence

Frozen, process-isolated hooks diagnostics produced 126 candidate records per
OS. The native candidate exit code was persisted as zero on both platforms.
Across 252 matched before/after records, 144 changed and no recorded host
count/byte/bucket field increased. All Windows/Linux peers match; three samples
agree exactly, 32 calls scale exactly from one call, zero-call intervals are
empty, host allocation/deallocation counts balance, input Programs are
unchanged, and runtime/Ralloc counters remain zero.

Representative one-call values, identical on both builds:

| Raw fixture | Allocations before → candidate | Reallocations before → candidate |
| --- | ---: | ---: |
| 256 functions | 2,829 → 1,037 | 774 → 6 |
| 4,096 functions | 45,073 → 16,401 | 12,298 → 10 |
| 4,096 names | 4,100 → 4 | 4,096 → 0 |
| 256 struct fields | 267 → 6 | 259 → 0 |

These are complete-verifier allocation requests, not label-only counts,
allocator latency, bytes copied, RSS or peak memory. The original baseline
Windows log-parser recovery and missing separately persisted native exit code
remain documented in the preceding baseline; they were not overwritten.

### Fixed ordinary timing protocol and exclusions

A three-repeat Windows calibration was explicitly ineligible for decisions.
At scale 16, candidate name/field batches averaged about 58/82 ms. The paired
protocol was then fixed before any paired capture: nine repeats; scale 32 for
those two rows, scale 8 for the large uncached module graph, scale 16 otherwise;
CV at most 10%, except the hot-loop guardrail at 5%. Both variants use identical
actual iterations. Windows uses affinity `0x4` / Normal priority and scheduled
cycles as primary; Linux uses CPU 2, native `/tmp` fixtures and thread CPU as
primary. The existing Balanced policy is unchanged.

Windows CPU-time values are coarse, sometimes absent, and average only the
samples accepted by the harness's CPU-time sanity check. They are supporting
diagnostics, not precise primary evidence. Wall time and Linux TSC are also
supplemental; scheduled cycles are not retired instructions.

All captures run sequentially, first control/candidate and then
candidate/control. No build, profile, source edit, archive creation or hash
sweep overlaps ordinary timing. A pair is excluded if either side fails the
quality gate. No noisy capture was retried or replaced.

The target stage completed all 56 captures: 46 qualify, yielding 20 usable
pairs out of 28. Every usable pair improves, but eight row/platform
combinations have only one qualifying pair. The following percentages are
changes in the platform's primary mean metric, in capture 1 / capture 2 order:

| Verifier row | Windows scheduled cycles | Linux thread CPU |
| --- | ---: | ---: |
| functions_256 | -65.09% / excluded | -58.12% / excluded |
| names_4096 | -99.04% / excluded | -96.67% / -96.63% |
| struct_fields_256 | -97.90% / excluded | -97.11% / -96.90% |
| declarations_256 | -66.99% / excluded | -59.43% / -59.82% |

The other three fixtures have usable improvements too, but Linux loop/function
CFG and Windows loop CFG lack their second qualifying pair. Raw Programs and
source-derived fixtures measure full public verification, including certificate
construction/drop, not certificate-only or emission-only work.

### Whole-system rejection

All 48 predeclared Windows guardrail captures completed: 38 qualify, yielding
17 usable pairs out of 24. The first ordinary and declaration-heavy full
compilation pairs improve by about 12% and 23%; their reverse-order pairs are
excluded. Cold dispatch lowering has no usable pair. Other exclusions affect
module compilation, VM API and input/heap API confirmation.

The warmed source API has two usable pairs, however:

| Order | Control mean scheduled cycles | Candidate mean scheduled cycles | Change | Control / candidate CV |
| --- | ---: | ---: | ---: | ---: |
| Control → candidate | 41,964.951 | 44,604.468 | +6.29% | 6.39% / 5.12% |
| Candidate → control | 39,212.430 | 39,705.472 | +1.26% | 1.01% / 3.00% |

Mean wall time also increases by 6.73% / 1.45%; best scheduled cycles increase
by 7.68% / 0.31%. The coarse Windows CPU-time figures are not used to claim
additional precision. This row reuses a JitCache primed outside timing and
includes source API lookup and execution; it is not isolated verification.
The exact cause of its adverse result was not established. No attribution to
code layout, formatter overhead or runtime instruction changes is claimed.

Under §37, consistent realistic-workload regressions disqualify this candidate
even when smaller than 5%. Allocation and verifier-only wins are insufficient,
and no correctness or required-architecture exception applies. The remaining
48 Linux guardrail captures were not run after this complete Windows-stage
rejection: they could not reverse the Windows failure. This is a documented
early stop, not a claim that the planned 152-capture matrix completed.

In total, 104 ordinary captures completed, 84 qualify, and 37 of 52 pairs are
usable. All 5,096 printed correctness checks pass. The target and Windows
decision audits independently match raw stdout to saved JSON, verify exact
filters/options/iterations and opposite run order, preserve rejected raw logs,
and check scheduling, native exits, terminal records and frozen identities.

### Restoration and evidence

The restored verifier matches the exact saved pre-candidate file, including
its three semantic tests. Independently, its production prefix before
`cfg(test)` matches the frozen baseline byte-for-byte after line-ending
normalization. The 163-entry restored archive has no additions and only
verifier.rs differs from that baseline; the other 162 source entries match.

Fresh restored hooks workspaces pass 583 Windows / 584 Linux tests, with
23 ignored tests and 46 result groups each. Both all-target hooks Clippy runs
with `-D warnings` and default-feature all-target checks pass. Fresh ordinary
releases both pass all 49 checks; Cargo JSON confirms the intended manifest,
fresh builds and no testing-hooks in timing binaries. Full nightly formatting
and `git diff --check` also pass. The restored Linux binary matches the frozen
control hash; the Windows binary has its own recorded hash and is not newly
timed here. No restored Windows speed claim is made.

Machine-local evidence is retained under `target/perf/full-system/`:

| Evidence | SHA-256 |
| --- | --- |
| verifier-lazy-label-candidate-freeze-1.json | 62F3FB6E2043295C571F1BFD87E61CCC1DC3E4152E5109426A86FA69AD35A619 |
| verifier-lazy-label-candidate-source-1.zip | 1B6F77773DFB118182F1250BD98F789BFA0F8FEF66D7F2F83544E01194330E3B |
| verifier-lazy-label-candidate-windows-1.exe | DD7EC5CF79E9FFE87BB1EEBA880F2D4768C947D8C32CFCA2FE1053CC8236C8A6 |
| verifier-lazy-label-candidate-linux-1 | CC82EB326B734F651F7A53DA56CDB2BACDAD4AFD8B2ADD7AAF35CA75E2EB7429 |
| verifier-lazy-label-candidate-allocation-audit-1.json | C381439B25CF7CBFA66AE0F7FEAA46D19CA43D32AA116A375FA7E9A1B055D8B7 |
| verifier-lazy-label-allocation-comparison-1.json | 8850C677865A436A735CE3B3658923F62F5E06D70BBFFD5306F562B7221403F4 |
| verifier-lazy-label-targets-audit-1.json | FF03E21E9452CA8E7845AECEE7FB265C2DB6446D6CFCD55B70257EB4E7DAFDF8 |
| verifier-lazy-label-windows-guardrail-decision-audit-1.json | 00B07B931C9E79CA645664F40ED48C5DB1D5417E5153C626C3617F4B2791D50C |
| verifier-lazy-label-restored-freeze-1.json | 59E25425517AA815195555B88BB5A0663F7249B5D9F8B239B1451DF96E01C4DA |
| verifier-lazy-label-restored-source-1.zip | 8CF279666484D0373AD81710BE8DB643526C8B8F1A392305F372B095CB767888 |
| verifier-lazy-label-restored-windows-1.exe | CEE62F7FF01042847C01A27C1D009CEDF1D5B43C9B36DB042B8E4A06BCB30F81 |
| verifier-lazy-label-restored-linux-1 | 7F23A7B30A2E13F7B227F6F6173F05FC3B31F91ED1D9EECFA870DAFA79859103 |

The target audit hashes 171 evidence files; the Windows decision audit hashes
145. Neither is a full-system acceptance report. No new Miri, hardware-event,
peak-memory, allocator-latency or Vec/Box advantage is claimed. Fixed feedback
remains boxed; installed JitOps retains private slice-only Vec access, the
4 KiB slack-reclamation rule and full-capacity accounting.

The first final integrity audit passed source/evidence and formatting/diff
checks but failed its documentation assertion because Windows PowerShell
misdecoded a UTF-8 section-sign literal in the script. All required text was
present. An ASCII-safe second audit preserves the first script and gate logs,
rechecks the evidence, and records this recovery without replacing captures.

Do not retry this unchanged rejected production candidate as if its verifier
wins establish acceptance. The observed formatting cost remains a measured
opportunity, but a different design would need its own source/binary identity,
exact-diagnostic checks and whole-system confirmation. Certificate-only
attribution, broader phase/workload coverage and the remaining 40-section
implementation, cleanup and reporting requirements stay open.

## Source-cache clock rollover: correctness fix, limited overhead evidence

The source cache's saturating access clock can stop preserving true LRU order.
Once the clock reaches `u64::MAX`, later hits all receive the same age. Eviction
then follows the digest/bucket-index tie-break, and may evict a recently used
source while keeping an older one. This is an overflow-boundary correctness
failure in the documented cache policy, not evidence of a common runtime cost.

Four local regression tests cover colliding digests, cross-bucket byte-limit
eviction, independently cloned recency, and empty/disabled/oversized caches
including compilation errors. Before changing production, three tests failed:
the collision and clone sequences each recorded one hit instead of two, and
the cross-bucket sequence recorded zero instead of one. The empty-cache
accounting test already passed. All 13 source-cache tests pass after the fix.
Both original and corrected runs produce the expected program output; the bug
causes unnecessary source recompilation, not substitution of different source.

The private `next_source_access` renumbers live ages before advancing beyond
the maximum clock value. It sorts a temporary vector of mutable references
across every digest bucket, then assigns consecutive ages. Only age fields
change: source/program owners, collision-bucket order, exact identity,
source-byte charges, limits and compiled-cache policy stay intact. Lifetime
activity counters still saturate. Clones keep independent recency.

The rollover path takes O(hash-table capacity + N log N) time and O(N)
temporary reference storage for N resident sources. The sort is in-place;
the temporary vector is dropped before lookup resumes. Ordinary calls only
test/increment the clock; they do not build that vector. The number of live
entries fits far below u64::MAX on 32/64-bit address spaces, so
renumbering leaves room for the next access. No retained field, dependency,
unsafe code, Ralloc payload, frame allocation or installed container changes.

This is not a new Vec-over-Box decision. Fixed installed edge/feedback metadata
remains boxed. Installed `JitOps` retains private slice-only Vec access, reclaims
spare capacity at the 4 KiB threshold and charges full remaining capacity. That
earlier Vec/Box speed comparison remains inconclusive; builders and this
short-lived reference collection do not establish a blanket speed preference.

### Documentation and source identity

The full 40-section objective, home AGENTS router, Rust README and four Rust
examples were reread. Repository and ancestor searches found no additional
guidance. Cache sections of architecture, VM, memory-model and Rust API
documentation, the contribution/CI rules and benchmark quality rules were
checked. The router's optional stable Rust documentation was treated as
required, including [HashMap mutable values](https://doc.rust-lang.org/stable/std/collections/struct.HashMap.html#method.values_mut)
and [slice key sorting](https://doc.rust-lang.org/stable/std/primitive.slice.html#method.sort_unstable_by_key).
No external-crate API or Ralloc behavior changed.

All 163 source/configuration entries matched the prior restored verifier
checkpoint before the test patch. Production cache code was independently
compared at the failing control checkpoint; only tests had changed. The current
freeze changes only `jit/cache.rs` relative to that checkpoint; all other 162
entries, including the three retained verifier diagnostic tests, are unchanged.
The frozen archive contains and verifies all 163 entries.

Two large approved script launches were denied before execution, and a direct
patch attempt could not write its target. Read-only checks found no partial
scripts or freeze and sufficient disk space; permissions were not modified.
Small apply_patch updates succeeded, and both assembled scripts were compared
exactly with their intended text before execution. No timing capture was
replaced or retried as part of this recovery.

### Fixed normal-path overhead check

The protocol was fixed before timing: four rows on Windows and native-filesystem
Arch WSL, two opposite-order control/candidate pairs, nine repeats, sample scale
16 and CV at most 10%. Windows used affinity 0x4, Normal priority and scheduled
thread cycles as the primary metric; Linux used CPU 2 and thread CPU time.
The host Balanced power policy was unchanged. All captures ran sequentially,
without concurrent builds, source edits, profiling, archives or hash sweeps.
The prior restored ordinary binaries are the controls; testing-hooks builds
were not timed. The ordinary rows do not reach clock rollover.

All 32 planned captures completed and printed all 49 correctness checks:
1,568 successful checks in total. Only 23 of 32 captures qualify, yielding
11 of 16 pairs. Windows has seven eligible captures and three usable pairs;
Linux has sixteen eligible captures and eight usable pairs. Nine Windows
captures exceeded the CV gate and remain diagnostic; no adaptive retry was
used. An excluded member excludes the entire pair.

Primary mean changes below are candidate relative to control; positive is
slower. A dash means the pair is ineligible, not zero cost.

| Row | Windows order 1 | Windows order 2 | Linux order 1 | Linux order 2 |
| --- | ---: | ---: | ---: | ---: |
| Warm source JIT API | - | - | +2.01% | -0.93% |
| Cold source JIT API | -2.93% | - | -3.12% | +5.24% |
| Source VM API | - | +3.92% | -0.97% | +5.40% |
| Verified compiled-cache hit | - | +7.31% | +0.40% | -6.82% |

No row has a demonstrated regression in both eligible opposite-order pairs.
That is not a clean guardrail result: every Windows row lacks two usable
pairs, and warm-source lookup lacks any usable pair. Individual eligible
increases above 5% remain material observations: Windows verified-hit cycles
+7.31% (wall +7.54%), Linux cold API CPU +5.24% (wall +5.00%) and Linux VM API
CPU +5.40% (wall +5.42%). Their causes are not established. Windows CPU-time
quantization, excluded captures and reversed signs prevent broader claims;
5% is not a free acceptance allowance.

Whole frozen benchmark file sizes rose from 2,509,312 to 2,514,432 bytes on
Windows and 2,404,144 to 2,409,296 bytes on Linux. These are executable file
extents, not isolated helper code size, RSS or retained runtime memory.
Rollover allocation latency and peak process memory were not measured.

### Correctness retention and evidence

Retain this small change for the demonstrated deterministic-LRU requirement.
Do not describe it as an optimization win, a Vec/Box win, complete normal-path
overhead confirmation, or full-system acceptance. Wider cache/memory workload
coverage and Windows overhead confirmation remain open.

Fresh checks at the frozen source checkpoint passed:

- Full workspace testing-hooks tests: 587 Windows / 588 Linux passed, with
  23 ignored and 46 result groups on each platform.
- Workspace all-target Clippy with testing-hooks and `-D warnings`.
- Default-feature workspace all-target check.
- Fresh ordinary release benchmark build and all 49 correctness checks per OS.
- Full nightly formatting check and `git diff --check`.

No new Miri or allocator-specific speed/safety claim is made for this safe
metadata-only change. Existing Ralloc tests run within the workspace gate;
ignored and platform-specific cases are not silently treated as passing.

Evidence under `target/perf/full-system/`:

- `source-clock-control-tests-1.log`: expected original-code failure, exit 101.
- `source-clock-candidate-cache-tests-1.log`: all 13 source-cache tests pass.
- `source-clock-candidate-freeze-1.json`:
  `CC08370F66C0ED246A19B9F04C6831009911320FE03329AF7295C74781A99485`.
- `source-clock-candidate-source-1.zip`:
  `293BBCBFD3E696D75E017422BE89814A11F8A0ED9F213BAA5883A9D329FA483B`.
- `source-clock-candidate-verified-{windows,linux}-1.json`: native build
  provenance, binary identity and ordinary release checks.
- `source-clock-{windows,linux}-guardrails-coordinator-1.log` and both
  terminal records: all fixed captures, including excluded results.
- `source-clock-guardrails-audit-1.json`:
  `244CA16508AD7C3C83134B96E0B2AA671DC48FEE5B6740A7F6E2C2D80C0A185A`;
  verifies 101 evidence hashes, exact raw/saved metrics, scheduling, order,
  source/binary identity and quality decisions.

### Evidence-loss interruption before final verification

The final integrity script stopped before its first source-freeze check:
the saved manifest could no longer be resolved. Read-only checks confirmed
that the candidate freeze/archive, timing audit, both platforms' built/frozen
binaries and earlier target evidence were missing. The target, target/perf
and target/perf/full-system directories all report a creation time of
2026-09-13 00:35:00 UTC. Only the newly created final audit script remains.
The cause of the recreation has not been established; no deletion command was
issued by this workstream.

The test and timing outcomes above were observed before this loss, but their
raw files cannot currently be rechecked. No final-check JSON was produced and
the final source/archive/evidence audit did not pass. The source-clock helper,
four tests and documentation remain present; the edited Rust file still passes
its formatting check and git diff --check passes. Do not regenerate missing
timing records from summaries or claim that matching archives were restored.

The full 40-section objective remains active. The source-clock implementation
is preserved for correctness, but final evidence verification is incomplete.
Restore the original evidence if available, or explicitly establish new
baseline identities and measurements without overwriting the historical
results or presenting a rerun as recovery of the original captures.

## Fresh current-source correctness checkpoint after evidence loss

This is a new checkpoint of the preserved worktree, not a restoration of the
missing historical comparisons. No implementation, test, dependency, or
container-policy changes were made in this recovery. The new evidence is in
`_perf-evidence/recovery-20260913-1/`, outside Cargo's target directory and
covered by the existing local-output ignore rule.

The source freeze contains all 289 Git-listed source/configuration/document
files (2,957,271 bytes). Every archived entry is checked against its recorded
SHA-256. The gates checked those same 289 live hashes before and after running.
Only this worklog and `performance.md` were subsequently updated to report
the new checkpoint; the final integrity script also checks the other 287
live files and records both intentional documentation changes.

### Fresh verification

| Gate | Windows | Arch Linux / WSL |
|---|---:|---:|
| Workspace tests with testing hooks | 587 passed, 23 ignored | 588 passed, 23 ignored |
| Test-result groups | 46 | 46 |
| All-target hooks Clippy, warnings denied | passed | passed |
| Default all-target workspace check | passed | passed |
| Fresh ordinary release build | passed | passed |
| Ordinary release correctness checks | 49 passed | 49 passed |
| Separately invoked existing storage diagnostics | 2 passed | 2 passed |

The six specifically checked regression passes include all four source-clock
rollover cases and both installed-operation capacity/slack tests. The two
storage diagnostics were explicitly invoked with `--ignored --exact`; they
are separate from the full suite's 23 ignored tests, not a claim that every
ignored experiment ran. The initial diagnostic invocation used an incorrect
module path and matched zero tests. Its wrapper rejected that result; both
zero-test logs and the original runner are preserved. The corrected runner
uses the exact test names found in the fresh full-suite logs and requires
one passing test per invocation.

### Current Vec/Box storage observations

The Windows and Linux diagnostic rows agree. Installed operations remain a
private Vec exposed only as slices. Installation reclaims spare operation
capacity when it reaches 4 KiB, and retention accounting charges all capacity
that remains. Fixed edge-counter and feedback tables remain boxed slices;
growing builders and stacks remain Vecs. No container was switched here.

| Lowered fixture | Chunks | Initialized code bytes | Reserved code bytes | Spare code bytes |
|---|---:|---:|---:|---:|
| small | 1 | 104 | 128 | 24 |
| loop | 1 | 234 | 498 | 264 |
| function | 2 | 286 | 286 | 0 |
| map | 1 | 312 | 624 | 312 |
| many_stores | 1 | 104 | 104 | 0 |

These code-byte columns include operation storage and fixed edge counters;
they are not total program memory or process RSS. The five fixtures retain
0-312 bytes of spare code capacity and satisfy the existing less-than-4-KiB
per-chunk installation bound. Warmed feedback storage is 0, 99, 0, 132, and
0 bytes respectively, within the separate 64-KiB feedback budget.

The frame diagnostic observes Vec reserved bytes of 0, 352, 352, 704, and
1,408 at depths 0, 1, 4, 8, and 16, with 0, 1, 1, 2, and 3 capacity-growth
events. Its 1,408-byte boxed maximum is a calculation from 16 optional frame
slots, not an allocated Box competitor. Capacity-growth events are not
allocator-call counts. These results support the bounded storage accounting;
they do not establish Vec speed superiority, allocation-call savings, peak
RSS improvement, or acceptance of the older inconclusive Vec/Box comparison.

### New provenance and remaining scope

- Source manifest: `recovery-current-freeze-1.json`, SHA-256
  `F9FCCA983A1B96C131B8C788BAB7D0ABFA07BAA49D1F4201B601EC559DFE8EE7`.
- Source archive: `recovery-current-source-1.zip`, SHA-256
  `C6724006DFAC9B5AAE09BCE1E5F862A708C5DBA7E0914667CCED9F3718410D7C`.
- Frozen ordinary Windows binary: `recovery-current-windows-1.exe`, SHA-256
  `857AFEEBEAFCDF37B75843F834C16A4FFC0D8D7DBEACA123EBFD2979A83EBD28`.
- Frozen ordinary Linux binary: `recovery-current-linux-1`, SHA-256
  `33FD85E37C0E6126AAA81BC78493EC5837DC8078F39020A468DF3DA5AC490200`.
- Native gate logs and `recovery-current-verified-{windows,linux}-1.json`
  identify the exact ordinary, non-testing-hooks Cargo artifacts.
- `storage-diagnostics-{windows,linux}-2.json` ties each explicit diagnostic
  to its source freeze, command, successful raw log, and preserved initial
  zero-test invocation.
- `audit-current-1.ps1` verifies the source/archive identities, native test
  totals, required regression passes, ordinary artifact features, saved
  binary hashes, full cross-platform storage rows, and evidence hashes.
  It runs full formatting and whitespace checks and records the result in
  `recovery-current-final-1.json`.

No ordinary performance captures were made in this recovery. Matching a
previously reported binary hash does not restore an unavailable historical
file, source snapshot, or timing record. The full 40-section objective remains
active: current correctness has fresh evidence, while historical performance
acceptance, broader baseline coverage, cleanup, and final reporting remain open.

## Explicit source-compilation stage attribution

This is a new testing-only attribution checkpoint for sections 3, 6-8 and
32-34, not an accepted optimization or restoration of historical evidence.
The ordinary compiler methods, parser, lexer, verifier, runtime, Ralloc,
dependencies and existing Vec/Box policy are unchanged. Two feature-gated
wiring edits expose a new compiler-owned profile module; the other two new
Rust files are its integration tests and fixed eight-fixture source list.
Profile storage is a fixed four-element array with typed not-run/success/
failure outcomes, not another resizable container or shared observer.

### Boundary and correctness checks

The explicit hooks entry point uses the same source compiler constructor
arguments, owned compilation, optional peephole pass and authoritative
verification as the corresponding public verified-source API. It measures
construction/source ownership/lexing; fused parsing, semantic/name work,
emission and assembly; peephole; and verification/certificate/verified-owner
construction. Parsing and emission remain a single pass with no AST.
File/module resolution, disk caching, lazy fingerprinting and certificate-only
attribution are not measured separately. Named phases exclude final returned
owner destruction; the two outer batch routes both include it.

Four new unit tests cover explicit phase identities/outcomes, clock failure
without changing subsequent compiler operations, invalid/repeated intervals
and panic cleanup. Three integration tests cover all eight fixtures in both
modes with exact verified-program/fingerprint and VM/JIT output parity;
lexical, parsing/import and verification rejection with original diagnostics;
and runtime error/source ownership after inputs drop at thresholds 0/1/8.

Pre-build exact-text checking caught and corrected a misplaced test block
before any build ran. Candidate 1 then passed both full test suites, but
Clippy rejected a unit argument in the ignored empty-clock test. Candidate 2
changes that closure to evaluate black_box first and return Ok(()) separately.
The candidate-1 source/archive and failed logs remain preserved; no timing
was taken from that candidate.

| Candidate-2 gate | Windows | Arch Linux / WSL |
|---|---:|---:|
| Full hooks workspace tests | 594 passed, 25 ignored | 595 passed, 25 ignored |
| Test-result groups | 47 | 47 |
| All-target hooks Clippy, warnings denied | passed | passed |
| Default all-target workspace check | passed | passed |
| Fresh ordinary release correctness | 49 passed | 49 passed |
| Separately invoked exact ignored diagnostics | 2 passed | 2 passed |

The two ignored diagnostics are the empty-recorder floor and source-stage
report; their results are separate from the full suite's ignored count.
Fresh diagnostic executables use separate Cargo target and build directories,
release optimization level 3 and exactly default plus testing-hooks. Cargo
JSON identifies their source manifests, non-fresh flags and actual executable
paths; frozen copies are hashed before and after each direct invocation.

### Fixed samples and observations

After all builds finished, direct frozen executables ran sequentially:
Windows floor, Windows stages, Linux floor, Linux stages. Each source route
has seven samples with alternating control/observed order. The four small
fixtures use 2,000 compilations per batch; line-heavy and constant-chain
cases use 256, and declaration/string-binding cases use 128. Generation,
VM/JIT output checks and reporting are outside the timed batches.

Each OS supplies 112 complete batch rows, 448 phase rows and seven floor
rows. All expected identities, iterations, phase call counts, skipped
peephole records and interval sums are checked. Structural metadata agrees
across platforms. No row was discarded and no timing was adaptively rerun.
Machine/toolchain versions are saved: Windows uses Rust 1.98.0; Arch/WSL
uses 1.99.0-nightly. Cross-OS times are not a controlled platform comparison.

The following optimized-route means illustrate source-shape-specific costs.
Shares divide by the sum of measured named phases, not full compilation
wall time. These are hooks-build observations, not production acceptance.

| Source shape / measured phase | Windows mean, us | Linux mean, us | Phase share, Windows / Linux |
|---|---:|---:|---:|
| 4,096 header lines / construction and lexing | 67.883 | 62.435 | 97.64% / 98.52% |
| 128-term constant chain / peephole | 259.401 | 114.018 | 77.92% / 84.06% |
| 256 declarations / parsing, emission and assembly | 768.278 | 370.807 | 46.98% / 45.87% |
| 256 declarations / verification | 533.999 | 271.759 | 32.65% / 33.62% |

Variation remains material: 5/16 Windows groups and 3/16 Linux groups contain
at least one observed/control/captured/phase sample CV above 10% (sample
standard deviation, n-1). Windows flags are optimized print, both line-heavy
routes, optimized declarations and optimized constant chain. Linux flags
are optimized float loop, unoptimized line-heavy and optimized string
bindings. In particular, Windows optimized line-heavy and constant-chain
outer observed CVs are 24.63% and 19.05%; their means do not establish a
reliable ordinary latency baseline. All seven raw values and all per-phase
CVs remain in frontend-analysis-1.json.

The four-phase empty-recorder batches average 365.465 ns/capture on Windows
and 207.673 on Linux; the mean recorded empty phase is 33.723 and 17.914 ns.
These floors are reported separately, never subtracted. Observed/control
ratios of means range 0.991-1.218 on Windows and 0.993-1.364 on Linux.
The control is the verified public API inside the same hooks executable;
differences include clocks, aggregation, wrapper code generation and noise.
Ratios below one are not negative observer cost or optimization wins.

### Evidence and remaining scope

All fresh evidence is in `_perf-evidence/frontend-stages-20260913-1/`, outside
target/. The pre-edit freeze contains 289 files / 2,962,940 bytes; candidate 2
contains 292 / 2,987,826. Only this worklog, performance.md and architecture.md
are intentionally updated after the candidate freeze.

- Candidate-2 manifest SHA-256:
  `7D038344318F521279B1072B385E8BFB63F264B247D23444C6D488B28ABB5835`.
- Candidate-2 archive SHA-256:
  `FB928E1FEAAA3487B3D7347012A72C2CFC7B91206AC380C2884390F85B102827`.
- Complete raw-row/variation analysis SHA-256:
  `A71C325CA2C4731DC3A272767A8282B1E58FA831E55AA6C5590637F61A4573E8`.
- Ordinary Windows binary SHA-256:
  `4D4EA9DFF2B66C11C5D685A10EC8097E3FEAEBDEC580D0BF30A0753B6E6BF0E4`;
  it differs from the prior recovery build, so byte identity is not claimed.
- Ordinary Linux binary SHA-256:
  `33FD85E37C0E6126AAA81BC78493EC5837DC8078F39020A468DF3DA5AC490200`;
  it matches the extant prior recovery binary, not missing historical files.
- Build/run JSON binds all four diagnostic binaries to exact source, features,
  commands, raw logs and non-overlapping run intervals. The final audit checks
  all three source archives, the implementation delta, live files, full gates,
  diagnostic identities, raw analysis, formatting, whitespace and evidence.
  `audit-frontend-1.ps1` records these checks in `frontend-final-1.json`.

These results suggest targeted inspection of constant folding, line-heavy
source construction and declaration-heavy assembly/verification. Any change
still needs fresh ordinary before/after and whole-system guardrail evidence.
No Vec/Box container change or speed superiority is established. All 40
sections remain active; broader baselines, historical performance acceptance,
cleanup and final reporting remain open.

## Constant-chain continuation trial (rejected; production restored)

A fresh, bounded follow-up to the source-stage diagnostic investigated repeated
left-associated integer folding. The earlier hooks profile attributed 77.92% /
84.06% of named-phase time for the 128-term chain to peephole work on Windows /
Linux; Windows outer variation prevented treating that as ordinary acceptance.
This trial did not reopen the prior owned-buffer or Vec/Box experiments.

### Scope and retained changes

Both production candidates are rejected. The complete production portion of
bytecode/peephole.rs is restored exactly to the pre-edit source (line endings
normalized for comparison). No runtime, allocator, dependency, opcode, installed
container, source-owner or Vec/Box policy change remains.

Retained coverage consists of three tests: bounded chains shrink in place
without replacing their instruction buffer; 21,120 additional exact comparisons
against the independent copying reference cover arithmetic continuations and
every branch boundary; and folded origins preserve the final operator,
copy-on-write behavior and shared source-file/function owners. Five additional
VM/JIT source cases cover chained floor division/comparison, mixed numeric
values, overflow, zero division and invalid Boolean negation with error order.

The trial's one-pass test failed on control exactly as expected, then passed
on both candidates through 32,767 terms. That algorithm-specific claim is not
retained: the final test uses 3/32/128 terms and checks progress, final output
and buffer identity across however many passes the production algorithm needs.

The ordinary harness adds compile.constant_chain_128, using the existing source
and golden checks. It measures full compilation, unlike the optimizer-only row.
All old workload bodies and iteration counts, and the 49 printed correctness
checks, remain unchanged. The unchanged ownership policy still uses boxed
fixed tables and slice-only installed operation access; Vec speed superiority
over boxed slices is not established.

### Candidates and fixed protocol

Candidate 1 extended a successful integer fold through adjacent continuations,
preserving checked errors, Boolean comparison results, interior branch targets,
invalid-jump behavior and final-operator origins. It also extracted the matcher
behind an after function. Candidate 2 restored the original at matcher exactly
and reused it with fixed two-/three-instruction stack windows after a fold.
Neither reassociated expressions or introduced float folding or a heap container.

Each candidate used fresh isolated ordinary release builds, with Cargo JSON
recording source manifests, default-only features, opt-level 3, fresh=false and
actual executable paths. Source and binary hashes were checked around builds
and runs. Control revision 1 was frozen before production edits. Candidate 2
used the same frozen control binary but new paired measurements, not old timings.

Both platforms ran control/candidate and candidate/control orders, nine repeats,
correctness enabled, no quick mode, one executable at a time after all gates
and builds completed. Windows used logical CPU 4 / mask 0x10, inherited before
entry and read back, normal priority and the unchanged Balanced power scheme.
Arch WSL used taskset CPU 4; its recorded Windows PID is the WSL launcher, not
a claimed Linux native PID. Per-platform labels distinguish the two campaigns.
Windows primary work is scheduled thread cycles; Linux primary work is thread
CPU time, not its TSC counter. Cross-platform values are not causal OS comparisons.

The predeclared screen comprised eight selections, followed conditionally by
16 broader selections, including all five allocator/memory guardrails. Both
screens failed retention criteria, so those conditional campaigns were not run.
The ordinary 10% CV gate and 5% hot-loop gate were not relaxed. An excluded pair
could receive one same-order recovery with both sample scales multiplied by four.

Candidate 1 supplied 64 captures / 32 eligible pairs without quality recovery.
Candidate 2 supplied 68 captures: its initial Windows forward chain candidate
had 12.56% CV, and its initial Linux reverse hot-loop candidate exceeded the
5% limit. Both original pairs remain recorded and excluded; both permitted
longer recovery pairs passed. The revised screen therefore has 32 eligible
pairs, no missing confirmations, and no result-selected retiming.

### Measurements and rejection

Percentages below are candidate/control changes; negative is lower. Each cell
lists forward / reverse order. These are rejected trial results, not retained
optimization claims.

| Candidate / row / metric | Windows, % | Linux / WSL, % |
| --- | ---: | ---: |
| 1 / optimizer chain / best wall | -99.23 / -99.20 | -98.30 / -98.31 |
| 1 / full chain compilation / best wall | -83.99 / -85.88 | -84.80 / -84.69 |
| 1 / straight-line optimizer / primary work | +5.05 / +5.28 | +5.61 / +1.79 |
| 2 / optimizer chain / best wall | -99.34 / -99.34 | -98.70 / -98.66 |
| 2 / full chain compilation / best wall | -85.66 / -84.92 | -85.07 / -85.17 |
| 2 / straight-line optimizer / primary work | +1.84 / +1.06 | +1.76 / +6.15 |
| 2 / full ordinary compilation / primary work | -1.33 / -1.56 | -1.26 / +6.52 |
| 2 / warm source JIT / primary work | +0.66 / +0.80 | +2.49 / +1.01 |

All target chain comparisons improved best wall and primary work. That does
not erase the guardrails: candidate 1 repeated a >5% Windows straight-line
regression, with five eligible guard comparisons exceeding 5%. Candidate 2
removed that particular threshold failure on Windows, but Linux reverse
straight-line best wall also rose 5.61%, and two eligible guard comparisons
exceeded 5%. Its smaller warm-source adverse shifts repeated in both orders
on both platforms. Smaller control-flow shifts and all CPU/cycle/wall values
remain in the complete analysis; they are not called neutral or hidden.

The full plan requires removing consistently regressing realistic-workload
optimizations unless a necessary correctness/architecture exception is
documented. No such exception is asserted here. Both production variants were
removed. The remaining repeated-chain cost is an open optimization problem,
not a reason to relax the acceptance gate or rerun either unchanged candidate.

Symbol inspection found outlined matcher/continuation functions, but the
first scan still contained inlined matching; a per-instruction-call cause was
not established. GNU objdump's COFF symbol boundaries split some instructions,
so those diagnostic logs are not reliable instruction-count evidence. The
next investigation must establish a distinct mechanism, not reuse that
unproven explanation.

### Final verification and evidence

After restoring production and adapting the buffer test, a new control revision
2 was archived and built. Final gates passed:

| Gate | Windows | Arch Linux / WSL |
| --- | ---: | ---: |
| Full hooks workspace tests | 597 passed, 25 ignored | 598 passed, 25 ignored |
| Test-result groups | 47 | 47 |
| All-target hooks Clippy, warnings denied | passed | passed |
| Default all-target workspace check | passed | passed |
| Fresh ordinary release correctness | 49 passed | 49 passed |

Each rejected candidate also passed those full correctness gates. Passing
correctness was not treated as performance acceptance.

Evidence is preserved outside target/ in
`_perf-evidence/peephole-chain-20260913-1/`. Five source snapshots each contain
292 files; final pre-report source totals 3,005,010 bytes. Only performance.md
and this worklog are intentionally updated after that final source freeze.
The archive and final audit preserve the exact production rollback, retained
test/harness delta, all build/gate identities and the dirty-worktree inventory.

The first Windows screen completed before runner 1 failed Linux launch:
Get-Command returned both System32 and WindowsApps WSL paths, causing parameter
binding to fail before native execution. The failed preparation is preserved.
A resume manifest froze all 16 completed Windows pairs / 32 captures, and
runner 2 resumed only unmeasured Linux work through explicit System32 WSL with
new filenames. No completed Windows timing was rerun or overwritten.

Independent JavaScript checked all 132 raw JSON captures, 66 pair attempts,
paired metadata/options/iterations and 396 percentage deltas against the
PowerShell analyses. Two raw captures fail quality; their complete pairs are
excluded. JSON exposes aggregate statistics, so this is not a reconstruction
of unavailable per-repeat variance samples.

- First screen analysis SHA-256:
  `0FF5A9C919CB79AC71F5994070CCBCBC11D328D47439096A46CA24060F3FFF40`.
- Revised screen analysis SHA-256:
  `44AD575F8112836A194AE078BFBBA797D930DB1A7A93C504540C395C248B5212`.
- Final source manifest SHA-256:
  `5DCCE3F8EEAA07901A9487715164E510A1CAFBD2FA6E668650496531FA985550`.
- Final source archive SHA-256:
  `FF790F5818646E7C6461CB114D8D92BF6C8162CC97078259DEB07A6D2C2057D8`.
- Final ordinary Windows binary SHA-256:
  `A9EEA25E9BB25E2590DECC990007F0832C4814FDCBC6C3A16836F6D0A090179C`.
  It differs from trial control 1; Windows binary identity or measured
  performance recovery is not claimed.
- Final ordinary Linux binary SHA-256:
  `C3BEC7BCECB7762729AE4CB5AEEF72415E510476B5CF3BF75DB79EACFA6EE903`.
  It matches trial control 1, not missing historical target/perf evidence.

Earlier raw performance artifacts remain missing. This checkpoint does not
restore them, establish a Vec/Box speed ordering, complete whole-system
performance acceptance, or mark any of the full plan's 40 sections finished.

## Fresh short-call feedback startup baseline

This returns to the full plan's runtime/call-feedback ledger (sections 3, 21,
23, 24 and 32-34). It adds cold-lifecycle attribution, not another unchanged
warm-call or Vec/Box trial, and not a production optimization. Before editing,
all 292 live sources and 1,098 evidence hashes matched the completed peephole
checkpoint. Only benchmark wiring and the new benchmark-owned calls.rs module
change. Runtime, compiler, verifier, bytecode, cache, allocator, dependencies
and all existing container policies are unchanged. Fixed arrays describe
shapes/counts; the registration builder reserves its known 32 rows.

Read the complete objective, home router, Rust README/four examples and the
repository's index, architecture, bytecode, VM, memory, performance, Rust API,
contributing and CI references. Consulted stable Rust's
[black_box](https://doc.rust-lang.org/stable/std/hint/fn.black_box.html),
[drop](https://doc.rust-lang.org/stable/std/mem/fn.drop.html) and
[slice APIs](https://doc.rust-lang.org/stable/std/primitive.slice.html#method.as_chunks),
Cargo's [build directories](https://doc.rust-lang.org/cargo/reference/build-cache.html)
and [artifact messages](https://doc.rust-lang.org/cargo/reference/external-tools.html#artifact-messages),
Microsoft's [inherited affinity](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-setprocessaffinitymask)
and [hidden process launch](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.management/start-process?view=powershell-5.1),
and [taskset](https://www.man7.org/linux/man-pages/man1/taskset.1.html).
No third-party Rust API or dependency was added.

### Interval and verification

The 32 new call_cold.off/on rows cover static calls, function values,
one-capture closures and alternating function identities at 0, 1, 8 and 32
calls. Every timed invocation constructs, executes and drops a fresh
JitProgram, including main/lazy-callee lowering and returned-memory/code
cleanup. The harness's three warmups also construct fresh owners, so they
cannot pre-promote later timed code. Source construction/compilation,
verification, immutable fingerprint initialization, golden checks and reporting
stay outside timing. Scalar quickening is unchanged; detailed profiling is
disabled on both sides. Zero-call closures still construct their closure and
captures: zero calls does not mean zero heap work.

Two new tests check matching names/counts, exercise each registered closure
three times, and assert exact VM/golden output, lazy chunk counts and actual
call samples/promotions/hits/misses at 0, 1, 7, 8, 9, 31, 32 and 33 calls.
The boundary matrix uses thresholds 0/1/8, both call-feedback choices and two
independent fresh constructions (384 JIT executions). Static sites never
sample; enabled stable sites promote at the eighth entry; alternating sites
stop after 32 samples with no promotion. These are correctness tests, not
profiled timing measurements.

Both full hooks workspaces pass: Windows 599 tests and Linux 600, with
25 ignored and 47 result groups each. Both all-target hooks Clippy runs
(warnings denied) and default all-target workspace checks pass. Fresh isolated
ordinary release builds each pass the unchanged 49 printed correctness checks.
Each timing invocation also checks 16 VM and 32 off/on JIT fixture outputs
during untimed registration.

Initial full test runs failed five FFI tests per OS because separate Cargo
target/build directories exposed existing worker discovery assumptions.
Windows selected the worker's libtest executable, which rejects the worker
flag; Linux reported no worker. A direct Windows diagnostic preserves the
libtest rejection and valid JSON from the actual worker. gates-2.ps1 uses the
supported TINYONE_SANDBOX_WORKER setting to identify the real per-platform
target/debug worker; both full gates then pass. Original failure logs and
runner remain. Production FFI discovery was not changed, and its nonstandard
layout limitation is not claimed fixed.

The first campaign wrapper stopped before any native capture because
PowerShell wrapped its parsed JSON selection array as one item. run-2.ps1
assigns that array directly. The pre-launch failure is preserved; no completed
measurement was discarded or repeated for either tooling issue.

### Fixed ordinary captures

The immutable protocol specifies base 600 iterations, scale 16 (9,600 actual
iterations), nine repeats, normal priority and unchanged Balanced power.
Windows inherits CPU-4 affinity mask 0x10 before entry and reads it back;
Arch WSL uses taskset CPU 4. Windows uses Rust 1.98.0 and Linux the installed
1.99.0-nightly; absolute cross-platform times are not controlled comparisons.
Windows scheduled thread cycles and Linux thread CPU time are primary;
Linux TSC is secondary. Coarse Windows CPU time does not overrule cycles.

All 128 captures completed sequentially from 04:58:23 to 05:05:43 UTC on
2026-09-13, after builds/tests finished. The recorded Windows PID for a Linux
invocation belongs to the WSL launcher, not the native benchmark. Eight
captures exceed 10% CV, excluding six complete pairs; 58 of 64 pairs qualify.
No quality retry, longer recovery sample, relaxed gate or favorable-subset
selection was used. Independent JavaScript recomputed all 384 metric deltas
from all raw JSON records and checked options, metadata, binary/source
identities, pair order and non-overlapping processes against the separate
PowerShell analysis. JSON contains aggregate metrics, not raw per-repeat
samples; no variance samples were reconstructed.

Primary changes are enabled relative to disabled; positive is slower.
Both orders appear for every shape. Excluded pairs retain their raw numbers
in analysis-1.json.

| Calls / shape | Windows cycles, forward / reverse | Linux thread CPU, forward / reverse |
|---|---:|---:|
| static / 0 | -0.16% / +0.87% | -0.71% / -0.66% |
| static / 1 | +0.33% / -1.48% | -1.89% / -2.39% |
| static / 8 | +2.52% / excluded | +0.83% / +3.87% |
| static / 32 | -0.07% / excluded | +0.20% / +0.54% |
| function_values / 0 | -1.34% / excluded | -1.99% / -17.13% |
| function_values / 1 | -0.02% / -7.32% | +0.04% / -0.56% |
| function_values / 8 | +2.62% / +2.13% | -2.48% / +2.08% |
| function_values / 32 | +0.35% / +1.87% | excluded / +3.04% |
| closures / 0 | +0.66% / +0.60% | excluded / +0.11% |
| closures / 1 | +0.56% / +3.01% | -0.40% / excluded |
| closures / 8 | +0.65% / +1.24% | +0.59% / +3.72% |
| closures / 32 | +3.08% / +2.90% | +7.30% / +27.74% |
| alternating / 0 | +11.13% / -0.09% | +1.35% / -2.33% |
| alternating / 1 | +5.75% / -0.19% | +0.12% / +2.55% |
| alternating / 8 | +0.58% / +0.95% | +0.06% / +2.19% |
| alternating / 32 | +0.50% / +2.01% | +3.82% / -3.99% |

Thirty-two-call closures are slower in both orders on both platforms:
Windows cycles +3.08%/+2.90%, Linux CPU +7.30%/+27.74%. Best wall time also
worsens in those four pairs (+0.74%/+2.21% Windows, +3.39%/+20.52% Linux).
Eight-call closures are adverse in both metrics in all four pairs.
Function-value and alternating results are mixed or incomplete. Large
zero-call changes also occur, including Windows forward alternating +11.13%
and Linux reverse function-value -17.13%; these are not evidence of executed
call-handler work. No specific scheduler, predictor, frequency, allocation
or code-generation cause was established.

Excluded captures: Windows reverse static_8 off/on (14.80%/119.85% CV),
static_32 on (130.71%), function_values_0 on (13.98%); Linux forward
function_values_32 on (14.38%), closures_0 on (41.77%); Linux reverse
closures_1 off/on (13.48%/14.70%). Eligible partners are not standalone
comparisons. Eligible lifecycle means range 3.27-26.21 us on Windows and
1.60-19.09 us on Linux: whole cold invocations, not isolated handlers.

### Disposition and evidence

Call feedback remains experimental and disabled by default. This fills the
missing short-lived startup interval, not whole-feature before/after
acceptance or a Vec/Box speed ordering. No production optimization is retained
or rejected by this benchmark-only change. Broader memory/allocation
attribution, a concrete feedback redesign, full-system acceptance and final
cleanup remain open. Do not repeat these unchanged rows as the next workstream.

Evidence is in _perf-evidence/call-cold-20260913-1/, outside target/.
Source manifests contain 292 pre-edit and 293 measured files; only two
documentation files change after measurement.

- Measured manifest SHA-256:
  29011F5CEDB6CF96109B8E1891E1F082EC2512EC3F343F2E5056AEE0A7B28484.
- Measured archive SHA-256:
  2ED5CDE08F2DC4146B6E0B3A0DBAC31BDF63336EB543A91A5301DC008AC634B0.
- Ordinary Windows binary SHA-256:
  25CA30EAA09C7DC74388AA33AEB2F00409EA4B40FF56AC21AEE08198EC988100.
- Ordinary Linux binary SHA-256:
  8BF1BDEDA5DE14053DB8995F8C674C9829DA5D076E6C7C31365F0E50F536CEFB.
- Complete analysis SHA-256:
  98D00A6DA111F17A56ECF892EDA63CB26FF72A3B47A02C2E40EBD92761F996A3.
- Independent recomputation SHA-256:
  A55B172834ADD2A5571F1A4F33D96E7A1946E9D3D26EBD2C827B1C42AD7B0EAF.

audit-1.ps1 checks all three archives, the exact live inventory and allowed
delta, build/gate records, campaign and both analyses, formatting and
whitespace; call-cold-final-1.json records current source/evidence hashes.
Missing target/perf history is not restored. All 40 sections remain active;
this is not the full-system completion report.

## Settled initial call guards rejected by cold-call screen

The preceding cold-start baseline was progress on the full optimization plan.
This distinct trial tests avoidable guard-state writes (sections 14, 24, 29
and 32-37); it does not repeat the unchanged off/on baseline. The complete
40-section objective remains active, and this is not a completion report.

### Hypothesis, scope and documentation

A newly promoted GuardedCall has zero hits and zero misses. Its next 32
matching calls request opcode rewrites even though there are no misses to
clear. The candidate initializes hits to HITS_TO_CLEAR_MISSES instead. A real
miss resets hits to zero in the unchanged record method; every subsequent
recovery transition is identical. For the fresh 32-call stable fixture, the
eight sampling calls leave 24 initial guard rewrites that this candidate skips.

Only that initializer and one comment changed in production. No handler
validation, frame/capture ownership, permission check, Ralloc path, dependency,
JitOp layout, default option or container policy changed. Call feedback remains
opt-in. Installed fixed tables remain boxed; growable stacks/builders and
the private slice-only JitOps owner retain the existing bounded Vec policy.

Read the home engineering router, Rust README and all four examples; INDEX,
CONTRIBUTING, CI_CD, architecture, VM and performance documentation; current
guard implementation/callers, cold benchmark module and public guard tests.
Previously read bytecode, memory-model and Rust API contracts remain unchanged.
The [stable Rust integer API](https://doc.rust-lang.org/stable/std/primitive.u8.html#method.saturating_add)
confirms the existing saturating-counter behavior. No external crate API or
Ralloc-dependent implementation changed, and no dependency replacement was made.

### Tests and controlled measurement

The constructor-specific no-rewrite assertion fails against the original code
(exit 101), then passes with the candidate. It is removed when the candidate
is rejected. Three semantic tests remain on the restored implementation:

- Initial hit counts 0 through 32 all converge to the same first-miss state.
- Hit runs of 0, 1 or 31 between misses cannot clear the four-miss budget.
- A cloned guard with three misses deoptimizes after 31 hits and another miss;
  the original recovers after 32 hits, then permits a new four-miss budget.
  Thresholds 1/8 preserve exact portable-VM output and memory snapshots.

Both candidate and restored trees pass 602 Windows / 603 Linux tests, with
25 ignored tests and 47 result groups per platform; all-target hooks Clippy
with warnings denied and default all-target checks also pass. Each fresh
ordinary control, candidate and restored release executable passes all 49
printed correctness checks. The unchanged cold benchmark registration and
boundary tests retain their additional golden-output checks.

Tests use supported explicit TINYONE_SANDBOX_WORKER paths to the actual
workers in isolated Cargo target/debug directories. Automatic discovery under
separate Cargo build directories is still not fixed. The initial targeted
unit-name filter selected no integration tests; a separate unfiltered run
passed all seven before the full gates. Miri/sanitizers were not rerun for
this safe-Rust test-only retained delta; this is not a new allocator audit.

Fresh per-platform/per-variant Cargo target and build directories, JSON
manifest/profile/features/fresh checks, executable copies and source archives
establish the compared identities. The runtime harness is unchanged. Windows
uses Rust 1.98.0 / Cargo 1.98.0; Arch WSL uses installed Rust 1.99.0-nightly
(7608eb7b0, 2026-08-05). CPU4, Windows inherited mask 0x10/readback, Normal
priority and unchanged Balanced power policy match the recorded protocol.

All 48 captures completed in the fixed order, with no retries or exclusions:
24 pairs, nine repeats, scale 16, 9,600 actual iterations per row. Builds and
native gates finish before timing; capture process intervals do not overlap.
The interval is 2026-09-13T05:43:55 through 05:46:59 UTC. PowerShell recomputation
from raw files independently matches all 144 JavaScript-calculated metric
deltas; complete paired metadata, source/binary hashes and run order match.
Raw JSON has aggregate metrics, not individual repeat samples.

Numbers below are candidate/control percentage changes, forward / reverse.
Negative is better; all pairs meet the original 10% CV gate. Names share the
call_cold prefix. Windows primary is scheduled thread cycles; Linux primary
is thread CPU time, not TSC or retired instructions.

| Row | Windows primary | Linux primary | Windows best wall | Linux best wall |
| --- | --- | --- | --- | --- |
| on.closures_32 | -2.60% / +0.14% | -2.60% / +6.00% | -3.40% / -0.15% | -3.31% / +4.12% |
| on.function_values_32 | -1.44% / -2.57% | +1.89% / -0.49% | -2.34% / -2.17% | +2.59% / -0.20% |
| off.closures_32 | -1.00% / -2.34% | -3.37% / -1.43% | -1.68% / -0.42% | -1.54% / +0.06% |
| on.closures_8 | +0.19% / -0.03% | +0.37% / -1.57% | -2.74% / +0.14% | -0.38% / -1.22% |
| on.static_32 | +0.30% / -1.24% | -2.28% / +0.13% | +0.59% / -0.20% | -2.43% / -0.25% |
| on.alternating_32 | -2.00% / -1.77% | -4.39% / +0.76% | -0.51% / -2.18% | -4.74% / +0.23% |

The primary closure target improves in the first order, but the reverse
Windows primary is +0.14% and reverse Linux CPU is +6.00% (best wall +4.12%).
Function-value results also conflict across Linux orders. Unchanged-path
controls shift too: feedback-off closures improve in primary metrics in all
four pairs, despite executing no changed guard logic. These shifts do not
establish a scheduler, predictor, frequency or code-generation cause.

### Disposition and retained evidence

The predeclared repeatable primary-plus-best-wall target gate fails. Reject
the candidate; the conditional 64-capture warm/runtime/compiler/allocator
campaign is not run. No relaxed threshold or favorable subset substitutes
for the required result. Fewer requested opcode writes alone are insufficient
for acceptance, and this is not isolated handler timing or Vec superiority.

Production is restored to the control. Only two test-bearing Rust files and
two documentation files differ at handoff. The final Linux ordinary binary
matches its control hash. The Windows final binary differs despite restored
production source; no Windows binary-equivalence or recovery-timing claim is
made. No runtime/container speedup or default-enable decision is retained.

Evidence remains under _perf-evidence/call-guard-clean-20260913-1/.
README.md fixes the protocol; analysis-1.json contains every paired metric.
Control, candidate and restored build/gate logs and all raw captures remain.
The control source archive exactly matches the preceding final archive
(DBE69229CC263A531FDAE22800592A7FD20F5FDEC447BBC32D31FB3B8CBB2083).
The candidate archive is
E840CC74504543913064F36B939CCBE95DA9DA3AECB6139D3F0D90254AE35015.
Final documentation alone follows the restored validation snapshot.
audit-final-1.ps1 verifies four archives, live source inventory, exact allowed
delta, restored production, builds, gates and evidence; its final proof is
call-guard-clean-final-1.json. Older missing target/perf evidence is not restored.

Do not rerun this unchanged initializer or the prior cold off/on campaign as
fresh progress. Return to the full-plan ledger for a distinct unfinished
workstream; broader stage/memory attribution, full-system acceptance, cleanup
and the comprehensive final report remain open.

## Reproducible original/current whole-system comparison

This continuation returns to the full 40-section objective, particularly
sections 3, 17, 32, 37 and 39. The preceding settled-guard trial made
progress but did not provide whole-system acceptance. No earlier rejected
prototype is repeated here. This comparison adds new source-reproducible
evidence; it does not recover the missing historical target/perf artifacts.

### Outcome and acceptance boundary

All 256 predeclared captures completed, with no measurement retry or
exclusion beyond the fixed quality gates. Four captures fail CV10%, leaving
124 of 128 usable pairs. Independent JavaScript recomputation agrees with
all 768 metric deltas, work counts, quality decisions and 32 classifications.

Fourteen rows improve in both the primary metric and best wall time in both
orders on both platforms; eleven worsen under that same criterion. Three
rows are mixed and four lack complete quality-qualified pairs. Forty-seven
eligible pairs have a greater-than-5% primary or best-wall regression.
These are repeated directions under the fixed protocol, not statistical
significance or proof of a single optimization's effect. No aggregate
whole-system speedup, Priority 3/5 acceptance or Vec superiority follows.

Compilation, verification, several lowering cases, function/control-flow
execution, cold source execution, fragmented allocation and uncached large
modules show useful gains. Integer loops, vector/map execution, warm source
execution and ordinary buffer construction expose important adverse costs.
The current optimization batch is not uniformly faster and is not complete.

### Frozen sources, builds and validation

Evidence directory: `_perf-evidence/system-20260913-1/`.
The before snapshot is the exact 246-file Git archive of starting revision
`523242d95eba2a4787ceefd5ce34f341f649a647`. The after snapshot starts from
the preceding audited 293-file live-source archive, SHA256
`FECECD6DAD5F4046C8F40182701AC5353898163AC444B85A1523A823E333C6EC`.
Only the copied after benchmark root is mechanically restored to the
original harness; no live Rust file, test, allocator or container is changed.
The two harnesses are byte-identical, SHA256
`27ACFCE67D6C110BF1056C1303F4268A211FDB5CCA9DF1AD297EE1A675BA1324`.
Both Cargo.lock files also match.

`source-freeze-1.json` records all 539 compared source files and the
unchanged 293-file live checkpoint. Its SHA256 is
`5D229947FE3868698026452C8207F9F893B468D6687CEB3C97679B29840EB0DC`.
The original source archive is `before-git-1.zip`, SHA256
`3FE59C8AE1E2930EB749C22582718E1CEB5176FBC68A105375918790253549E2`;
the current source with the shared harness is
`after-shared-harness-source-1.zip`, SHA256
`3C8A10102769DD5AF4A5331BA2E3B1029E162E85734C8F82EF2536CE06B44802`.

Each platform/variant has fresh, separate Cargo target and build directories
outside the snapshots. Locked/offline ordinary release builds report the
intended source manifests, optimization level 3, non-test profiles, default
core features and fresh=false. All four Ralloc artifacts independently
report their matching snapshot paths and default,rlib features without
testing hooks. The copied ordinary executables are frozen as:

| Platform | Original SHA256 | Current/shared-harness SHA256 |
| --- | --- | --- |
| Windows | `A393E39DA9AA749DC9B9CCDDE2DCBA3B4ED5AFF2959F3CE9B1C26F8AF16F1A66` | `688D2D9C3BFC23D566E64888DBD462089F96E94FD7852D0FF7A1CD4CE93A4D39` |
| Arch WSL | `A94F010C33FDC91ECCBE27B9B2CC7CC54A079899EE422BC6917193C1ADDAC20D` | `7C06B7A040E4CC56ADA4F9DF976B60DFBDA39CB161A0A88094F1BEDE567E2C72` |

All four builds pass the original 22 correctness cases and all 13 shared
harness unit tests. Every timed process also passes those 22 prechecks.
This does not validate every original-source bug or replace the current
runtime/Ralloc gates: the preceding restored-source final-gates-*-2 records
remain 602 Windows /603 Linux passed, 25 ignored and 47 groups each, with
hooks Clippy and default all-target checks. Their explicit sandbox-worker
selection and unchanged auto-discovery limitation remain as documented.

Windows uses Rust 1.98.0, LLVM22.1.8; Arch WSL uses Rust 1.99.0-nightly
(7608eb7b0), LLVM23.1.0. Before/after use the same toolchain within each OS;
absolute cross-OS comparisons are not implied. The host is an i7-8850H.
Windows inherits mask0x10 and verifies Normal priority; Linux executes
through taskset logical CPU4. These are OS-local CPU identities, not proof
of the same physical scheduling location across Windows and WSL.
Balanced power remains unchanged. Module fixtures use native Windows Temp
and Linux /tmp. Benchmark Git discovery reaches the live parent checkout,
so the independent source/archive/build hashes establish identity instead.

### Fixed matrix and all results

`README.md` and `selections-1.json` fixed the 32 rows before timing.
Windows forward then reverse orders complete before Linux forward then
reverse. Each forward pair runs original/current; reverse runs
current/original. Nine repeats, three normal warmups, no quick mode or
skipped correctness. No agent-launched build, test or profiling workload
overlaps timing, and measured capture process intervals do not overlap.
The archived original harness lacks the current Windows x32 short-allocation
base adjustment, so the three short allocator rows use scale128 on both
platforms. All actual work counts match their predeclared selection.

Every cell below is primary-change / best-wall-change, always
100 * (current/original - 1). Negative is faster. Windows primary is mean
scheduled thread cycles; Linux primary is mean thread CPU time. Linux TSC
and all other aggregate metrics remain in the raw JSON and analysis.
An asterisk marks an unusable pair; its numbers are retained, not evidence
for acceptance. Hot-loop CV5% and other-row CV10% remain unchanged.

| Workload | Windows forward | Windows reverse | Linux forward | Linux reverse | Classification |
| --- | --- | --- | --- | --- | --- |
| `frontend.lex` | -7.95% / -9.09% | -7.82% / -8.95% | -4.72% / -5.10% | -8.75% / -8.26% | gain |
| `compiler.emit_bytecode` | -39.51% / -31.42% * | -29.19% / -31.36% | -17.72% / -19.27% | -22.72% / -22.07% | incomplete |
| `optimizer.straightline` | -25.54% / -28.01% | -29.00% / -27.66% | -33.69% / -33.96% | -35.02% / -34.00% | gain |
| `optimizer.control_flow_passthrough` | -43.09% / -44.26% | -45.88% / -45.54% | -47.85% / -48.25% | -49.72% / -49.53% | gain |
| `verifier.loop_cfg` | -39.74% / -39.78% | -40.02% / -40.10% | -51.40% / -51.60% | -49.64% / -50.46% | gain |
| `verifier.function_cfg` | -27.31% / -27.51% | -27.64% / -27.02% | -40.06% / -39.76% | -39.59% / -38.88% | gain |
| `compile.full_pipeline` | -33.39% / -33.45% | -33.22% / -33.27% | -19.68% / -19.50% | -18.71% / -20.10% | gain |
| `program.from_binary_artifact` | -16.23% / -17.22% | -17.02% / -17.73% | -27.02% / -26.91% | -26.56% / -27.12% | gain |
| `jit.codegen_straightline_cold` | +215.75% / +215.90% | +200.15% / +203.47% | +208.89% / +210.11% | +209.75% / +211.24% | adverse |
| `jit.codegen_dispatch_cold` | -17.41% / -17.62% | -20.99% / -19.55% | -50.25% / -50.11% | -50.42% / -51.17% | gain |
| `jit.codegen_heap_cold` | -72.90% / -73.03% | -72.90% / -73.18% | -83.27% / -83.35% | -82.98% / -82.72% | gain |
| `jit.cache_hit_verified_dispatch` | -40.76% / -41.15% | -41.49% / -41.27% | +7.68% / +6.57% | +3.59% / +5.71% | mixed |
| `runtime.vm_hot_loop_4096` | +1.83% / +2.60% | +2.98% / +2.74% | +5.01% / +5.62% | +1.05% / +1.66% | adverse |
| `runtime.jit_hot_loop_4096_quickened` | +10.60% / +10.35% | +10.88% / +10.76% | +11.27% / +10.38% | +8.85% / +9.85% | adverse |
| `runtime.jit_hot_loop_4096_no_quickening` | +11.75% / +11.92% | +10.74% / +10.58% | +9.36% / +10.39% | +8.47% / +8.58% | adverse |
| `runtime.jit_function_calls` | -27.20% / -27.25% | -27.68% / -27.54% | -18.60% / -18.71% | -18.74% / -18.38% | gain |
| `runtime.jit_control_interrupts` | -27.58% / -28.43% | -21.96% / -21.63% | -22.84% / -23.40% | -9.68% / -10.02% | gain |
| `runtime.jit_heap_structs` | +0.15% / +0.82% | +1.99% / +1.60% | +7.24% / +8.03% | +5.85% / +6.37% | adverse |
| `runtime.jit_builtin_heavy` | +5.93% / +5.57% | +5.14% / +7.10% | +5.75% / +6.67% | +3.67% / +3.24% | adverse |
| `runtime.jit_vec_push_pop_256` | +4.22% / +4.74% | +10.28% / +10.28% | +8.31% / +7.84% | +7.85% / +8.73% | adverse |
| `runtime.jit_map_set_get_256` | +8.07% / +8.52% | +6.83% / +6.89% | +6.99% / +7.96% | +5.80% / +7.32% | adverse |
| `runtime.jit_heap_churn` | +7.39% / +7.67% | +6.26% / +7.57% | +5.92% / +5.64% | -7.78% / +0.95% * | incomplete |
| `api.run_source_jit_cold` | -7.06% / -7.39% | -7.85% / -7.84% | -8.98% / -9.04% | -9.26% / -5.51% | gain |
| `api.run_source_jit_warm` | +10.61% / +9.94% | +10.33% / +10.86% | +11.25% / +10.85% | +5.98% / +6.96% | adverse |
| `allocator.ralloc_buffer_64` | +8.13% / +9.28% | +12.09% / +11.71% | +14.53% / +14.79% | +15.00% / +14.98% | adverse |
| `allocator.ralloc_buffer_4096` | +83.80% / +83.71% | +83.26% / +80.85% | +175.72% / +177.73% | +178.22% / +178.83% | adverse |
| `allocator.ralloc_resize_64_to_4096` | +38.72% / +38.94% | +31.65% / +38.13% * | +53.55% / +55.30% | +51.61% / +51.72% | incomplete |
| `allocator.ralloc_fragmented_arena_cycle` | -43.27% / -43.40% | -43.62% / -43.54% | -42.23% / -42.17% | -41.78% / -42.53% | gain |
| `memory.reset_1024` | -2.27% / -2.31% | +0.07% / +0.19% | -0.96% / +0.43% | -0.22% / +0.38% | mixed |
| `memory.snapshot_1024` | +35.25% / +34.13% | -0.25% / -0.32% | -7.29% / -5.37% | +2.52% / +2.82% | mixed |
| `compiler.module_graph_large_uncached` | -19.41% / -19.31% | -18.80% / -19.52% | -31.79% / -31.77% | -35.20% / -35.64% | gain |
| `compiler.module_graph_large_cache_hit` | -8.31% / -8.28% | -9.35% / -9.81% | +24.37% / +13.48% * | +13.79% / +14.82% | incomplete |

Quality failures are exactly: Windows-forward original compiler emission
CV14.02%; Windows-reverse original resize CV15.72%; Linux-forward current
large module-cache hit CV19.18%; Linux-reverse original churn CV12.62%.
No retry, favorable subset or relaxed gate is substituted.

### Cost interpretation and next owner-level work

The largest complete repeated regressions are tiny straight-line cold
lowering (+200.15% to +215.75% primary) and 4-KiB buffer construction
(+83.26%/+83.80% Windows; +175.72%/+178.22% Linux). Their best-wall
absolute values, in nanoseconds per invocation, are:

| Workload/platform | Original -> current, forward | Original -> current, reverse |
| --- | --- | --- |
| Tiny cold lowering / Windows | 1,024 -> 3,235 | 1,064 -> 3,230 |
| Tiny cold lowering / Linux | 490 -> 1,518 | 499 -> 1,553 |
| 4-KiB buffer / Windows | 42.09 -> 77.33 | 42.81 -> 77.42 |
| 4-KiB buffer / Linux | 37.30 -> 103.59 | 37.28 -> 103.95 |

The buffer constructor and resize rows include owner destruction, not
separate deallocation latency. They also span a changed safety contract:
current `RallocBuffer::try_new_aligned` initializes every returned byte
before exposing safe slices, and growth initializes its new tail. The
original implementation lacks these writes. The diff in
`crates/tinyone_ralloc/src/buffer.rs` confirms this stronger current
guarantee; it must not be removed to recover the old timing. This matrix
does not isolate how much of the measured gap comes from initialization
versus the remaining allocator changes. A safety-equivalent allocation
baseline and separated initialization/allocation/drop attribution are the
next allocator work, with the full Ralloc documentation and safety gates.

The cold-lowering regression likewise requires owner-level phase and
allocation attribution before proposing a fix. It is not evidence against
any single local-IR pass, feedback field or installed container. Source API
cold latency improves even while this tiny lowering interval worsens, so
the complete pipeline and lifecycle must accompany any local change.

The vector-language row is slower in all four primary/best-wall pairs
(+4.22% to +10.28% primary); maps are +5.80% to +8.07%. These are program
workloads, not a Rust Vec-versus-Box experiment. Fixed chunk, edge-count and
scalar-feedback tables remain boxed. Private JitOps exposes only installed
slices, accounts for retained capacity and reclaims spare op extents of at
least 4KiB. Growing builders/stacks/bounded reuse pools remain Vec. No new
ownership/container change or speed claim is justified.

### Evidence and remaining full-plan requirements

`campaign-1.json` preserves the exact 256-process sequence and non-overlap
proof. `analysis-1.json` contains all 128 pairs, every metric and all 47
eligible >5% flags; its SHA256 is
`EEC884FFEBB2C680E2C875836DA88B2041345790D3875A051EC68F0C8622ABA7`.
`independent-analysis-1.json` records the independent 768-delta audit.
All raw/stdout/stderr/start/running/process/result files and every eligible
saved baseline remain. `setup-incidents-1.md` distinguishes preflight/tool
setup failures from the zero measurement retries.

This new common-harness matrix advances the whole-system evidence gap,
but does not close sections 3, 17, 32, 37 or 39. Newer float/mixed/closure/
enum/retained-object shapes, isolated verifier/lowering phases, full host
and Ralloc distributions, separate destruction cost and peak memory remain
outside this matrix. There is no native-code backend, no accepted feedback
default change, no blanket regression acceptance and no full-plan completion.
Do not rerun these unchanged 256 captures as new progress. Prioritize the
measured safety-equivalent allocator and tiny-lowering attribution, then
the repeated runtime/collection regressions and remaining coverage/cleanup.

Only this worklog and performance.md change after the measured live-source
hash check. `final-audit-1.ps1` verifies the paired evidence, measured source
archives, unchanged snapshot/library sources, allocator build paths and
documentation-only live delta; `system-final-1.json` is its proof and
`final-live-source-1.json`/zip preserve the documented checkpoint.

## Initialized-copy constructor trial not promoted

Continued the full 40-section objective after rehashing all 2,085 evidence
files and 293 live sources from the preceding whole-system checkpoint.
The new work remains under `_perf-evidence/allocation-init-20260913-1/`.
No production implementation, benchmark, test or container policy changed.

### Documentation and observed work

The global Router, Rust rules and all four required examples were read,
along with architecture, memory-model, VM, bytecode, types, Rust API,
performance, contributing and CI documentation. No applicable local
allocator AGENT(S) or allocator-specific Markdown was found. The existing
`docs/clippy_doc_lints.md` reference is absent. Ralloc 0.2.0 has no production
external dependencies. The relevant allocator/owner implementation and
Rust's [initialized-slice contract](https://doc.rust-lang.org/stable/std/slice/fn.from_raw_parts_mut.html),
[byte initialization](https://doc.rust-lang.org/stable/std/ptr/fn.write_bytes.html)
and [nonoverlapping copy](https://doc.rust-lang.org/stable/std/ptr/fn.copy_nonoverlapping.html)
contracts were checked before the isolated Rust edits. No dependency changed.

`VmAllocator::allocate` does not add another zero fill; it delegates to the
initialized RallocBuffer. `RallocBytes::from_slice` then copies over that
zeroed extent. Strings, supplied buffers, fresh cells and encoded-value
payloads reach this caller. TinyMemory separately initializes default slots
after allocation, and its fallible clone constructs defaults before copying
the source bytes. Those frame paths were inspected but not changed or timed
by this trial: raw zero bytes encode I8(0), not the required default I64(0).

An original-plus-zeroing control was considered, then discarded before
materialization: the historical allocator also differs in pointer provenance
and aligned-header correctness. No such historical control was built or run.

### Controlled candidate and safety gates

Both 293-file snapshots start from the previously audited current source
and original harness. Both receive identical copy rows, copy-ownership tests
and a common `RallocBuffer::try_from_slice` / `VmAllocator::allocate_from_slice`
API. The control's copy constructor uses zeroed allocation followed by copy;
its existing runtime caller is unchanged. The candidate initializes the
fresh extent directly from the live input slice, then installs the owner,
and routes RallocBytes through the copy API. Zeroed construction, aligned
allocation, grown-tail initialization, failure ownership and typed-box
cleanup remain intact. This is not the unchanged preceding executable.

Only four files differ from the measured-current snapshot in the control;
the candidate adds the TinyAllocator caller change. The variants differ
only in the buffer copy constructor and that caller. Shared API, tests,
harness and all other source bytes are checked for equality.

The source freeze is `F4CAF3384EB607F1C0FF82BF12587C6A19637096DF86D7012A8A4126E8A425C5`;
the shared harness is `4CA39B57C1A89245BF1EEFC460974877CD593CE358028B70D0DB83CC69ABF521`.
Both source archives are independently byte-checked, not just extracted.
Four fresh default-feature release builds use isolated Cargo target **and**
build directories, opt-level 3, one codegen unit and LTO off. Windows uses
Rust 1.98.0 / LLVM 22.1.8; Linux uses Rust 1.99.0-nightly
(7608eb7b0) / LLVM 23.1.0. Compiler versions match within each platform.

| Variant | Frozen executable SHA256 |
| --- | --- |
| Windows control | `691C40ED2DD230132B8CADEFC879EAD36C59C5E849DE68AA96497E22D77FBF41` |
| Windows candidate | `701B066EAADD26720B5684BC322C9DAA63B02E482B84A4999C8C1FD02941A33F` |
| Linux control | `C10F16684BD383756222DCB13B57C7C10C80EB5ABF7329991D0F58353A9883BE` |
| Linux candidate | `69ED8CC6DB1A53EE30ABEC5EED1C9081603137B9567C6C8FF339FBC8D67D16CC` |

Each build passes 22 release prechecks, 13 harness tests and 73 allocator
tests (56 library, seven initialization, ten native-box), plus all-target
Clippy for TinyOne and Ralloc with warnings denied. Six candidate initialization
tests pass Miri with its default checks, covering reused typed padding,
independent native-source copies, source destruction, VM ownership transfer
and zeroed growth. The oversized-copy case is native-tested, not Miri-tested.
These scoped gates do not replace the broader current-source 602/603-test
checkpoint or constitute an unrestricted proof of soundness.

Archived Linux release-library disassembly retains one memset and one memcpy
relocation in the control constructor, versus only memcpy in the candidate.
This confirms that the extra fill survives this control compilation; it is
not retired-instruction measurement or exact whole-runtime attribution.
The stripped benchmark executable's missing symbols are not treated as
missing measurements. The library archives and selected-function output
are retained in `copy-disassembly-1.json` and its referenced artifacts.

### Fixed screen and outcome

The screen completes all 32 declared captures: four rows, both orders, both
platforms, nine repeats and three normal warmups. Every capture passes all
22 prechecks. Logical CPU4, Normal priority and the unchanged Balanced policy
are verified; CPU4 is an OS-local identity, not proven the same physical core
across Windows and WSL. Builds, tests and profiling do not overlap timing.
Copy inputs are prepared outside timing; allocation, copying, consumption
through black_box and drop are inside. Runtime rows include 256 retained
cells, their root vector, execution and cleanup, not cell allocation alone.

Entries below are percent changes in primary work / best wall; negative is
faster. Primary is scheduled-thread cycles on Windows and thread CPU time
on Linux. These are nominal paired directions, not statistical significance.

| Workload | Windows forward | Windows reverse | Linux forward | Linux reverse |
| --- | --- | --- | --- | --- |
| `allocator.ralloc_copy_64` | -5.27 / -6.23 | -6.74 / -6.51 | -3.70 / -4.63 | -5.83 / -5.02 |
| `allocator.ralloc_copy_4096` | -28.36 / -28.28 | -23.89 / -24.03 | -42.08 / -42.42 | -35.12 / -43.54 * |
| `runtime.vm_heap_allocation_256` | -3.60 / -3.55 | -0.87 / -1.29 | +0.01 / +0.28 | +4.46 / +4.76 |
| `runtime.jit_heap_allocation_256` | -8.73 / -9.01 | -0.62 / -1.22 | +2.46 / +2.02 | +1.01 / -0.78 |

`*` The Linux reverse candidate 4-KiB copy capture has CV **35.1174%**
(10% limit), exits 1 and correctly saves no decision baseline. It remains
in raw data and the table but its pair is ineligible. All other captures
pass their limits: 31 eligible captures, 15 eligible pairs. There are zero
measurement retries and no eligible >5% adverse primary/best-wall pairs.

The 64-byte copy microbenchmark improves in all four pairs. The 4-KiB
confirmation is incomplete. Linux VM allocation is nominally slower in
both orders (the first CPU delta is near zero); Linux JIT CPU time is also
higher in both orders, with mixed best-wall direction. Windows runtime
improvements vary substantially by order. These data do not establish
consistent integrated-runtime benefit.

The candidate is **not promoted**. The 88 conditional guardrail captures
are not run because the fixed screen fails its quality/benefit conditions.
The experimental APIs and their dependent tests remain only in evidence
snapshots; they were never installed in the production worktree.
`analysis-screen-1.json` hashes to
`E1FA2730635EAABE725377C93B8C2FDFFD7C6BB11BB07C223B9934825370CC2B`.
`independent-analysis-1.json` independently recomputes all 96 numeric deltas
from 32 raw JSON documents, checks 16 paired metadata sets, all eligibility
decisions and four classifications, and agrees that guardrails cannot proceed.

Ordinary zeroed-buffer creation was not improved by this trial. Separate
allocation/destruction latency, integrated copy costs, allocator distributions,
peak/retained memory and the earlier runtime/cold-lowering regressions remain
open. Do not rerun these unchanged 32 captures as new progress. The result
advances attribution without closing full-plan sections 3, 17, 28, 32, 37 or 39.
Fixed metadata stays boxed; private JitOps still exposes slices, charges
retained capacity and reclaims spare extents of at least 4 KiB. No Vec speed
superiority, safety rollback, native backend or full-goal completion follows.

Only this worklog and performance.md change in live source. The final audit
checks all retained evidence, both frozen snapshots/archives and that
documentation-only live delta; `allocation-init-final-1.json` and
`final-live-source-1.json`/zip preserve the completed checkpoint.

## Full-plan acceptance reconciliation

The new [acceptance audit](optimization-acceptance.md) maps each of the 40
requirements to its current implementation/evidence status and remaining work.
It supersedes the historical ledger as a status summary without erasing prior
decisions. This is documentation progress, not full-goal completion or another
optimization trial. No new build, test or timing campaign was run.

Before editing, all 901 evidence files and 293 sources in the initialized-copy
checkpoint matched their hashes. All six logs supporting the 602/603-test full
code gates also matched. Comparison against that gate's 293-source freeze
found only the already-recorded worklog/performance documentation differences.
Existing container boundaries, tests, profiling intervals and source-restoration
code were inspected directly. The Vec/Box speed claim remains unproven.

The audit distinguishes allocator counts/buckets from missing owner lifetimes
and separate allocation/free/growth latency. It prioritizes cold-lowering and
integrated-runtime regressions, those measurement gaps, missing workload
comparisons and final cleanup. Historical unavailable evidence, rejected
candidates and opt-in call feedback are not relabeled as completed work.

The VM page's stale disk-cache restoration statement is corrected against
the existing private sidecar implementation and tests; performance acceptance
remains open. Only four Markdown files change, including the new audit page.
`_perf-evidence/acceptance-audit-20260913-1/` records link/row checks, unchanged
non-documentation sources and the final documentation checkpoint.
