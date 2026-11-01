# Optimization acceptance audit

Status: **in progress**, reconciled against the 2026-09-13 initialized-copy
checkpoint. This is the current requirement map, not the section 39 final
implementation report. It supersedes the older worklog ledger as a status
summary; the worklog preserves the individual decisions and historical results.

## Container decision

There is no established Vec-over-Box speed advantage. The installed policy is
specific to each owner's lifecycle, not a blanket container preference:

| Owner | Current storage and enforced boundary |
| --- | --- |
| Installed chunk slots, edge counters, scalar feedback | Boxed slices; lazy installation and quickening change elements, not lengths. |
| Installed operations | Private `JitOps(Vec<JitOp>)`, slice-only access, full retained-capacity charge, and reclamation when spare bytes reach 4 KiB. |
| Operand/call stacks, lowering builders, idle-frame pool | Vec where the initialized length changes. The frame pool is execution-local, limited to eight frames and 64 KiB, and cleared under allocation pressure. |

[JitOps and its boundary tests](../crates/tinyone_core/src/jit/chunk.rs),
[chunk ownership/accounting](../crates/tinyone_core/src/jit/program.rs) and
[frame ownership](../crates/tinyone_core/src/jit/frame.rs) were checked directly.
Existing tests cover empty/nonempty buffers around the reclamation threshold,
fixed extent through quickening, independent mutable clones and shared immutable
origins. This audit adds no new container implementation or timing result.

Vec retains capacity when emptied; converting to a boxed slice discards excess
capacity. These are [Rust API guarantees](https://doc.rust-lang.org/stable/std/vec/struct.Vec.html#guarantees),
not a prediction about this program's latency. An unconditional conversion of
installed operations still needs an isolated, safety-equivalent comparison.

## Evidence that can support current decisions

- [Whole-system comparison](optimization-worklog.md#reproducible-originalcurrent-whole-system-comparison):
  256 fixed captures, 128 pairs, 124 eligible pairs; 14 rows improve repeatedly,
  11 regress, three are mixed, and four have incomplete quality confirmation.
  Comparisons use the same original harness within each platform. They compare
  the original revision with the current implementation, not individual causes.
- [Initialized-copy trial](optimization-worklog.md#initialized-copy-constructor-trial-not-promoted):
  32 captures, 15 eligible pairs and one excluded capture. The 64-byte copy
  microbenchmark improves; Linux runtime results do not establish integrated
  benefit. Candidate APIs/tests remain only in evidence snapshots. Not promoted.
- [Full current-code gates](optimization-worklog.md#settled-initial-call-guards-rejected-by-cold-call-screen):
  602 Windows / 603 Linux passing tests, 25 ignored on each, 47 result groups;
  hooks all-target Clippy and default-feature all-target checks passed.
  All six test/lint/check logs were rehashed for this audit. Only documentation
  differs from that source freeze. These tests were not rerun for this doc edit.
- The latest sealed copy checkpoint's 901 evidence files and 293 live sources
  were rehashed before editing, with no mismatches. Its proof is
  `_perf-evidence/allocation-init-20260913-1/allocation-init-final-1.json`,
  SHA256 `930D9ABEF2E2284C8BC4C8945DC9EC5FFF28481C52368E875688424977F1D3DB`.
- Earlier `target/perf/` evidence was lost when target/ was recreated.
  Historical worklog observations do not become independently re-auditable
  merely because a newer test or benchmark passed. In particular, historical
  82-case Ralloc Miri results are not a new whole-runtime safety run. The newer
  six-case Miri run tests the unpromoted copy candidate, not a shipped copy API.

Nominal paired directions are not statistical significance. Excluded captures
remain visible. Neither the 256-capture comparison nor the 32-capture trial
was retried, and neither should be repeated unchanged as new progress.

## All 40 requirements

“Implemented” below means the mechanism is present, not that all performance
and memory acceptance gates have closed. “Historical” identifies evidence whose
original files are unavailable. No percentage-complete score is implied.

| Section | Current disposition and remaining acceptance work |
| --- | --- |
| 1. Repository documentation | Ongoing gate. Global Router, Rust rules and four examples apply; no local AGENT(S) or .agents directory was found. Recheck relevant subsystem and supplemental documentation before each implementation change. |
| 2. External documentation | Ongoing gate. [Version/documentation record](optimization-worklog.md#starting-state-and-documentation) exists; no dependency changes in this audit. Final report must consolidate change-specific API/version evidence, not claim every dependency was newly reviewed. |
| 3. Full baseline | Open. Current ordinary whole-system timings exist, but phase breakdowns, per-owner allocation lifetimes, separate allocate/free/grow latency and full memory/workload coverage remain incomplete. See the measurement gaps below. |
| 4. Semantic reference | Implemented test infrastructure; full acceptance open. VM/JIT differential suites cover values, errors, locations, numeric boundaries, permissions, limits and ownership. [Boundary tests](../crates/tinyone_core/tests/optimization_boundaries.rs) and the full gates are evidence, not exhaustive safety proofs. |
| 5. Pipeline | Implemented. VerifiedProgram shares immutable program/certificate/fingerprint facts; the [architecture](architecture.md) defines stage ownership. Parsing/emission are intentionally fused and there is no AST. Cross-layer cost acceptance remains open. |
| 6. Frontend/compiler | Retained ownership and in-place peephole changes; aggregate lexing/pipeline results improve. Large-module cache hits are platform-mixed/incomplete. [Constant-chain variants](optimization-worklog.md#constant-chain-continuation-trial-rejected-production-restored) were rejected; they are not pending production optimizations. |
| 7. Bytecode | Portable opcode representation retained; local peephole compaction and origin remapping are implemented. No enlarged specialized source opcode set was introduced. Keep artifact/index/branch boundaries in the final audit. |
| 8. Verifier | Dense traversal, authoritative static checks and reusable [certificates](../crates/tinyone_core/src/bytecode/certificate.rs) implemented; both CFG timing rows improve. Fine-grained phase attribution remains incomplete. Lazy diagnostic labels were rejected after API regressions. |
| 9. Basic blocks | [Symbolic lowering](../crates/tinyone_core/src/jit/lowering.rs) consumes verified blocks and preserves canonical nonempty merges. Correctness tests exist; tiny straight-line cold lowering is still about 3x original cost. |
| 10. Local passes | [Ordered passes](../crates/tinyone_core/src/jit/local.rs) implement propagation, versions, checked folding, branch/dead-temporary/dead-store work before encoding. Error/effect-order tests exist. Cold-cost and complexity acceptance remain open. |
| 11. JitOp layout | 24-byte representation retained. The 16-byte decode-bridge prototype was rejected in historical measurements; it is absent from production. Typed slots/temporaries/offsets and wide-index fallbacks remain. Do not describe compact-layout evaluation as a shipped win. |
| 12. Dispatch | Direct scalar/store/branch families are present. Current hot-loop/builtin/collection regressions prevent broad dispatch-speed claims; operation-count savings alone do not close acceptance. |
| 13. Value codec | Existing encoded I64 paths and generic fallbacks retained. Encoded slot-pair prototype rejected. Float-immediate stores use shared semantics, not adaptive F64 specialization. Reconcile codec and allocator work with whole-runtime costs. |
| 14. Type feedback | Bounded tag-only sampling, saturating counters, hysteresis and safe fallback implemented. Dense boxed tables share a 64-KiB program budget. Sparse-span/opcode-local-ID trials rejected; current runtime performance acceptance remains open. |
| 15. Calls | JIT uses explicit suspended frames and preserves the configured logical depth limit; portable VM remains recursive/reference. Direct argument/capture transfer is present; current aggregate function-call timing improves. |
| 16. Frame reuse | Execution-local exact-size frame reuse and cleared operand pools implemented, with allocation-pressure release and clone/cleanup tests. Historical work-count gains need current lifetime/retention evidence before full memory acceptance. |
| 17. Ralloc | Initialization, alignment/header, provenance and typed-drop safety corrections retained. Stationary size-aware prefix retained with documented tradeoffs. No universal allocator gain: buffer construction regresses and separate cost/distribution/lifetime work is open. |
| 18. Collections | Lifecycle fixtures and canonical key indexing exist; borrowed-query/key-width/removal candidates were rejected. Current vec/map workload rows regress. Reads, writes, growth, clone, iteration and destruction still need a complete current lifecycle comparison. |
| 19. Builtins | Borrowed arguments and known direct lowering retained with permission checks. Builtin-heavy runtime regresses in the current comparison. Expansion is conditional on integrated benefit, not required merely to add variants. |
| 20. Modules/permissions | Verified numeric ownership is reused; callee authority and dynamic-name checks remain. [Sandbox tests](../crates/tinyone_core/tests/module_sandbox.rs) cover boundaries. Isolated permission cost and module-cache platform differences remain open. |
| 21. Closures | Current captures transfer directly; dynamic identity, generation, arity and authority checks remain. Creation/capture-allocation/decode/destruction costs are not fully separated. No closure-instance state may enter a cache. |
| 22. Caches | Trust-sensitive identity, independent LRU bounds, capacity charging, lazy/error/panic growth handling and source-clock rollover are implemented. [Retention tests](../crates/tinyone_core/tests/cache_retention.rs) exist. Charges exclude opaque overhead and are not RSS/peak memory; warm API regressions remain open. |
| 23. Lazy lowering | Functions are built privately and installed on demand; listing is intentionally eager. Tiny cold-lowering regression remains unresolved. Do not replace lazy lowering with unmeasured eager work. |
| 24. Call feedback | Experimental, OFF by default. Callee-only guarded facts and fallback exist; short-call timing and the settled-guard trial do not justify promotion. Captures remain per-instance and guards preserve authority/depth checks. |
| 25. Branches | Compare/branch, truthiness and counter-elision families exist with block-entry tests. Current integer hot-loop results regress; newer mixed/branch workloads need matched ordinary-release comparisons. |
| 26. Errors | Source errors are built lazily; fused constituent failure sites and callee origins are covered by [location tests](../crates/tinyone_core/tests/runtime_locations.rs). Success-path cost and diagnostic allocation acceptance remain open. |
| 27. Debug layout | Immutable boxed origin side tables are separate from executable ops, including disk-cache restoration. Action/origin-combination and consecutive-origin prototypes were rejected. Precise diagnostics remain; cold/retained-source costs still need acceptance. |
| 28. Data structures | Bounded Vec/Box policy and several owner audits exist; repository-wide review is not complete. Do not confuse language vector workload regressions with a Rust container comparison. Exact retained-capacity accounting is not an allocator-overhead census. |
| 29. Repeated work | Verified ownership, stack bounds, source owners and unchanged-owner cache charges are reused. TinyMemory default initialization followed by clone/copy remains a candidate cost, not a proven removable write; zero bytes encode I8(0), not default I64(0). |
| 30. Advanced tier | Deferred, intentionally not implemented. No evidence establishes a need or benefit over the current interpreter; primary regressions and baseline gaps come first. Native machine-code generation remains out of scope. |
| 31. Migration cleanup | Partial. Superseded local encoder folds and rejected prototypes have been removed at their checkpoints. Three collection idioms remain intentional effect barriers, not automatically dead code. Final handler/metadata/pool ownership audit is still required. |
| 32. Optimization comparisons | Partial. Matched original/current timings and isolated trial decisions exist; newer workload classes, memory, lifetimes and separate Ralloc latency are missing. Historical per-class results cannot substitute for unavailable raw records. |
| 33. Profiling cleanliness | Feature-gated allocator/compiler/preparation instrumentation and opt-in execution profiling are present. Host allocation interception belongs only to diagnostic tests. Ordinary timing builds exclude heavy hooks; retain that boundary in future measurements. |
| 34. Evidence workflow | Ongoing. Fixed screens rejected several appealing local gains when runtime/quality guards failed. Next work must identify an owner-level cause; another unchanged matrix is not progress. |
| 35. Work order | Primary architecture exists, but baseline/allocator/acceptance phases remain incomplete. Resolve current regression attribution and missing measurements before additional interpreted tiers or feedback-default expansion. |
| 36. Tests/safety | Current 602/603 full-code gates and six verified logs are available; ignored tests stay explicit. Safety tools apply to their recorded snapshots only. Final format/lint/test/allocator/consumer scope must be reconciled, not inferred from test totals. |
| 37. Acceptance | OPEN. Repeated realistic-workload regressions need attribution and either a fix/removal or explicit correctness/necessary-architecture justification. Aggregate frontend wins and allocator microbench wins cannot waive this rule. |
| 38. Final cleanup | OPEN. This audit corrects a stale VM disk-cache statement, but does not claim a finished code/dead-path/configuration/documentation cleanup or a fresh final release gate. |
| 39. Final report | OPEN. This page is the acceptance map. Final delivery still needs current before/after memory and allocator results, consolidated compliance, removals, test commands/results and clear retained/deferred/experimental/rejected dispositions. |
| 40. Core principle | Ongoing invariant: reuse verified facts, reduce repeated work and fall back safely. Current measurements, not a preference for Vec, Box or a particular runtime technique, decide acceptance. |

## Measurement gaps that actually remain

Existing [Ralloc instrumentation](../crates/tinyone_ralloc/src/instrumentation.rs)
records allocation/free/reallocation totals, requested/usable bytes, peak live
payload, scan/coalescing probes and fixed size/alignment buckets. It does not
record allocation identity, owner class, lifetime, or separate operation times.
The [host recorder](../crates/tinyone_core/tests/allocation/host.rs) counts
allocations and four size buckets; it is not a whole-process lifetime/peak tracer.

The [runtime census](../crates/tinyone_core/tests/allocation_profile.rs) has
15 shapes, each with two thresholds and cold/warm execution. Its `struct_enum`
label is an enum fixture, not combined coverage. The separate
[retained-object fixtures](../crates/tinyone_core/tests/allocation/retained.rs)
supply real struct coverage, but fixture existence is not a current measured
struct result. Cold execution in these profiles starts after JIT construction;
it does not include source compilation or all lowering work.

The ordinary buffer rows time allocation **and destruction**. The resize row
also constructs and destroys its owner; the zero-fill row constructs a safe
zeroed buffer before its explicit fill. Neither subtraction of these rows nor
their labels establishes isolated raw allocation, zeroing or free latency.
Retained-object timing includes construction, reads, execution and shutdown.

Outstanding coverage must distinguish:

- Requested payload, usable payload, allocator metadata, retained native arenas,
  host allocations, cache charges and OS resident/peak memory.
- Frequency/size/alignment by owner, successful/failed growth, lifetime and
  cleanup, including mixed-size and long-/short-lived objects.
- Existing combined frontend stages versus genuinely separate verifier/lowering
  phases; diagnostic clock/observer cost versus ordinary-release latency.
- Newer float, mixed-type, closure, enum, retained-object and collection lifecycle
  shapes versus the 32 rows in the original/common harness.

## Ordered next work and acceptance boundary

1. Attribute tiny cold lowering and repeated runtime/collection regressions
   using the current frozen source, existing phase/host-allocation diagnostics,
   and narrowly chosen ordinary-release checks. Account for observer overhead.
   Do not blame Vec, initialization or one pass from aggregate timing alone.
2. Close allocator/lifetime/memory gaps with a declared interval and owner model.
   Any allocation-only control must preserve current initialization, provenance,
   aligned-header and failure ownership contracts. The rejected copied-buffer
   API is not permission to weaken these contracts.
3. Compare missing representative workload classes and reconcile each retained
   optimization with section 37. A correctness-required cost may be justified
   explicitly; a consistently regressing optional optimization may not.
4. Complete the dead-path/configuration/doc audit, run the final relevant gates,
   then produce the section 39 report. Keep unavailable historical evidence and
   deferred/experimental items distinct from completed acceptance.

Do not rerun an unchanged completed campaign to fill time. A new measurement
needs a named unresolved question, a different controlled intervention or a
genuinely missing workload/interval. This documentation checkpoint changes no
implementation, tests, benchmarks, dependencies, feature defaults or containers.
