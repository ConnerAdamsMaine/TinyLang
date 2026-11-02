---
title: VM and JIT
---

# VM and JIT Operation

The TinyLang implementation has two execution backends that share the same verified bytecode:
the **VM** (portable interpreter) and the **JIT** (adaptive bytecode
tier). Both are selected via `--mode vm` or `--mode jit` on the CLI, or
`"vm"` / `"jit"` in the Rust and C APIs.

For bytecode format and opcode semantics, see [`bytecode.md`](bytecode.md).
For heap and memory operation, see [`memory-model.md`](memory-model.md).

---

## VM Backend

### Dispatch Loop

`VM::run` iterates over the main chunk's instruction stream. Each `Instr { op, arg, arg2 }` dispatches to a match arm in `run_chunk`. The match arm reads operands from `arg`/`arg2`, manipulates the operand stack and stack-frame memory, and may call into the heap or runtime helpers.

All operations return `Result`. There are no `unwrap` or `panic` calls on production paths. A runtime error immediately unwinds through `?` and surfaces as a `TinyLangError::Runtime` to the caller.

### Operand Stack

A `Vec<Value>` inside the VM holds the operand stack. Instructions push and pop values. At any point during execution, the stack depth matches what the verifier computed at compile time for that instruction offset.

### Stack-Frame Memory

`TinyMemory` owns a Ralloc allocation of fixed-width, 64-byte encoded slots.
Each active function call has its own frame; the operand stack is a separate
`Vec<Value>`. Slots initially contain `I64(0)` and persist between loop
iterations until explicitly overwritten.

### Function Calls

`CALL arg arg2` pops `arg2` arguments from the operand stack, allocates the
callee's `TinyMemory`, initializes its first `arg2` slots, and recurses into
`run_chunk` for the callee's bytecode. The portable VM remains the semantic
reference.

The default and maximum call-depth limit is **16** nested TinyLang function
calls; verified VM settings can select a lower limit. Exceeding it returns
`TinyLangError::Runtime` with `Call stack overflow after N nested call(s)`.

The JIT uses an explicit vector of suspended call frames instead of recursive
host interpreter calls. Each cursor retains its chunk, resume PC, and operand
stack. Returns resume the caller; errors release active and idle frame memory.
The same logical depth limit applies. Cleared, exact-size Ralloc frames may be
reused within a run under independent eight-frame and 64-KiB bounds, but never
across runs. See [memory-model.md](memory-model.md) for allocation-pressure
handling and initialization guarantees.

JIT call exits retain arguments on the suspended caller's operand stack.
Frame entry drains that suffix directly into the callee's encoded slots,
preserving the caller's earlier operands and capacity. Dynamic calls consume
the callable before the argument suffix; closures decode their current
captures directly into the leading slots, without temporary argument/capture
vectors. The closure's heap borrow ends before lazy lowering or callee
execution. Arity, generation, callable-kind and logical-depth checks remain
authoritative, and failed transfers drop all remaining drained values.

### Module Capabilities

Imported functions carry a capability set from their package-manifest entry.
Before dispatching a host-facing builtin, the VM selects the grant for the
currently running function's owning module and rejects a missing grant. The
root chunk and root functions retain the authority given to the embedding
application; authority is never inherited from a caller into an imported
module. This check is performed at runtime, including dynamically invoked
module functions. Artifact metadata preserves grants but is not a signature:
untrusted artifacts still need an external OS/process sandbox because their
root chunk is privileged.

The JIT repeats the same check for generic builtin dispatch and for its direct
`free` superinstruction. See [modules](syntax/modules.md) for the manifest
format and the capability-to-builtin map.

### Error Propagation

Every operation that can fail returns `Result`. The `?` operator propagates errors up the call chain. The VM never calls `panic!`, `unwrap`, or `expect` on production paths.

Source-compiled VM and JIT execution retain the first failing bytecode
site and lazily adds `filename:line:column` plus a source line and caret to its
runtime message. The original error category is preserved. A callee failure
is not relabeled at its caller, even when the callee has no source metadata.
Artifact-only code still reports its original message without a fabricated
location. JIT fusion preserves distinct arithmetic, builtin and store sites;
quickening and repeated execution leave those origins unchanged. Cross-backend
error-string parity is restored in the current suites. Dependency-validated
disk-cache hits restore source origins through a private, snapshot-bound
sidecar; malformed or mismatched sidecars are cache misses. Diagnostic
performance acceptance remains open; see the [acceptance audit](optimization-acceptance.md).

---

## JIT Backend

Startup lowering is lazy: the main chunk is decoded immediately, while each
function chunk is decoded and fused only on its first call. The JIT retains a
single `VerifiedProgram` for all strings and declaration metadata rather than
cloning those tables. Listing generation is the deliberate exception and
lowers every chunk so the output is complete.

The JIT is an **adaptive bytecode tier** — it compiles TinyLang bytecode into a lower-level internal bytecode (`JitOp`) with decoded operands, then interprets `JitOp` and quickens hot loops in-place. It does not produce native machine code.

### Compilation Phase (`JitProgram::compile`)

1. **Verify or accept capability** — raw `Program` entry points verify once;
   `VerifiedProgram` entry points preserve an earlier proof.
2. **Use verified blocks lazily** — the certificate supplies chunk ranges and
   reachable entry stack depths. Branches cannot enter the interior of a block.
3. **Track symbolic values** — constants, local reads and binary expressions
   need not push temporary runtime values. Effects flush earlier values in
   order, and block boundaries restore the canonical stack, even at non-empty
   merges. Deep expression encoding uses an explicit traversal stack.
4. **Optimize locally in order** — constant propagation, copy propagation,
   slot-version propagation, checked I64 constant folding, branch folding,
   dead-temporary elimination, and safe dead-store elimination. Definitions
   and actions preserve source order. Copies of overwritten slots retain their
   snapshots; opaque effects and block boundaries invalidate all slot facts.
   Folding leaves failures for runtime. Dead stores are initially limited to
   adjacent non-failing literal writes with no intervening observation.
5. **Encode direct operations** — scalar expressions select regular store
   families or existing in-place encodings. Three collection-specific idioms
   still lower as effect barriers during the local-IR migration:

   | Bytecode sequence | JIT superinstruction |
   | --- | --- |
   | `PUSH_INT n, STORE s` | `StoreInt(s, n)` |
   | `LOAD s, PUSH_INT n, ADD, STORE s` | `AddSlotInt(s, n)` |
   | `LOAD s, PUSH_INT n, SUB, STORE s` | `SubSlotInt(s, n)` |
   | `LOAD a, LOAD b, ADD, STORE c` | `BinarySlotsStore(Add, a, b, c)` |
   | `LOAD a, PUSH_INT n, ADD, STORE b` | `BinarySlotIntStore(Add, a, n, b)` |
   | `LOAD a, PUSH_FLOAT bits, ADD, STORE b` | `BinarySlotFloatStore(Add, a, bits, b)` |

Verified local indexes use a typed `u16` in the new families, preserving index
65,535 in a 65,536-slot frame. Block offsets use `u32`, preserving the verifier's
full 262,144-instruction budget. Generic scalar semantics still determine mixed
numeric results and runtime errors. The local passes remove literal branches
and unused non-failing temporaries before final instruction fusion; discarded
arithmetic is still evaluated if it can fail. Floating-point arithmetic is not
constant-folded. Growable expression/action/effect and pass buffers are scratch
storage reused between blocks, not retained in installed chunks.
The float-immediate store family keeps the literal's exact Fp64 bits and uses
shared numeric semantics for all ten arithmetic/comparison modes. It does not
assume the source slot is a float or install an adaptive F64 specialization.
The source is loaded before the result overwrites the destination, including
when those slots alias; the operation does not touch the operand stack.
Consecutive definitions are recorded as runs ending before each action, keeping
their exact order without an action record for every temporary. Version analysis
is needed only when copy propagation proposes aliases; no later pass creates
new alias proposals.

### Dispatch Loop (`JitVm`)

`JitVm` interprets decoded `JitOp` values; runtime value conversion still
occurs where the operation needs it. Each frame resume borrows its installed
chunk once through `ChunkExecution`, rather than looking up the chunk on every
instruction fetch and feedback update. Instruction fetch still checks the
program counter. Calls, returns and errors end the borrow, so lazy function
installation and recursive resumption always observe current adaptive code.
The borrow updates the program's existing statistics and feedback-byte budget
directly, without allocating or copying execution state.

Original bytecode indexes live in an immutable shared `ChunkOrigins`, outside
the 24-byte `JitOp`. Its primary table is a fixed boxed slice of four-byte
indexes; a sparse boxed table retains additional sites for fused operations.
Lowering grows these tables privately and freezes them at installation.
An error-only stage tag distinguishes constituent failures, and the executor
looks up their source after releasing the executable-chunk borrow. Successful
dispatch neither reads the origin table nor formats a diagnostic.

### Hot-Loop Quickening

Every compiled chunk carries an `edge_counts: Box<[u16]>` parallel to its ops. At every backward branch, `JitVm` increments the counter for that branch.

When a counter first reaches the configured threshold (**8** by default), the
chunk arms eligible sites in `[branch_target, branch_instruction + 1)` for type
sampling. Hotness alone never selects an I64 handler. The following typed
transitions require eight consecutive observations of actual I64 inputs:

| Cold op | Hot op | Difference |
| --- | --- | --- |
| `Add` | `AddInt` | Checked direct I64 path; generic fallback otherwise |
| `Sub` | `SubInt` | Same |
| `Mul` | `MulInt` | Same |
| `Div` | `DivInt` | Same |
| `Compare(op)` | `CompareInt(op)` | Direct I64 comparison; generic fallback otherwise |
| `Jump(t)` | `JumpHot(t)` | Skips back-edge counter increment |
| `JumpIfZero(t)` | `JumpIfZeroHot(t)` | Same |

Pure `Jump`/`JumpIfZero` counter elision requires no type evidence. Scalar,
slot/immediate, compare/branch and slot-truthiness sites use a bounded state
machine: unseen, sampling, monomorphic, small-polymorphic, specialized, or
generic. Sampling retains at most two representation shapes and lasts at most
32 observations. Stable unsupported representations and unstable sites stay
generic; no F64 or mixed-numeric specialization is currently installed.

Every specialization miss executes shared generic semantics and records the
observed tags. Four misses without 32 intervening consecutive hits restart
sampling. At most two promotions are allowed per site; further instability
makes it permanently generic. Counters saturate. In-place rewrites preserve
operation length, operands, stack effects and branch targets.

Feedback tables are lazy boxed slices with a shared **64-KiB per-program** byte
budget, including lazily compiled functions. Each dense table reserves an entry
for every lowered operation in its chunk, not just adaptive sites. Large chunks
with few adaptive sites can therefore exceed the budget. A table that will not
fit is not installed: scalar sites stay generic, while branch-counter elision
remains independent. Tables retain tags only, never values
or heap identities. `JitProgram::feedback_bytes()` reports their backing bytes,
excluding owner headers and allocator overhead. Clones copy independent
feedback state and preserve the same per-program bound.

`JitOptions::with_hot_back_edge_threshold` changes the threshold for direct JIT
programs, caches, and configured runner calls. Threshold `0` disables
quickening without disabling JIT lowering or superinstructions. The CLI exposes
the same controls as `--jit-threshold N` and `--no-jit-quickening`.

### Experimental Dynamic-Call Feedback

Dynamic-callee feedback is **off by default** and remains under performance
evaluation. `JitOptions::with_call_feedback(true)` enables it independently of
scalar quickening; threshold zero still disables all adaptive behavior.
Ordinary calls and permanently generic fallbacks share `CallValue`. Only a
fresh chunk built with the option enabled arms unseen call sites, so back edges
cannot rearm a site that has already fallen back.

Eight consecutive successful entries to the same function may install a guard
with verified arity, slot and stack requirements. Sampling stops after 32
unstable observations. Four guard misses without 32 intervening consecutive
hits leave the site permanently generic. Guard failures execute the ordinary
call path. Function IDs and layout facts fit inline in the existing 24-byte
JitOp; no separate call-feedback table is allocated.

Closure kind, generation and current captures are resolved on every call.
Neither captures nor a closure instance survive in a guard. Callee-module
authority and the logical depth limit remain authoritative after promotion,
including in cloned programs. Correctness coverage does not establish a
latency improvement; see the optimization worklog for rejected measurements.

### JitCache

`JitCache` identifies executable programs by their Blake2b512 content fingerprint
(16 bytes, represented as 32 hex characters) and policy trust. Generic artifact
loading preserves the content fingerprint but removes authority to use its
serialized permissions. The cache keeps those executions separate from trusted
source or authenticated artifacts in either insertion order. A cache hit reuses
the compiled and potentially quickened `JitProgram`; hot ranges carry over.
Equivalent bytecode can have different filenames or source positions. After
the content/authority match, both run and compile-only hits bind the current
verified owner, preserving lowered origins and adaptive state while recharging
retained bytes. A larger diagnostic owner can evict or bypass an existing entry.

Compiled retention defaults to 128 entries and 16 MiB of charged entry data.
`with_compiled_cache_limits` sets independent entry/byte limits; either zero
disables compiled retention. LRU eviction and post-run accounting cover lazy
chunks, constants/verified metadata, feedback, and idle operand capacity,
including growth before a runtime error. Oversized runs execute normally and
bypass retention. A host output panic drops its mutated entry before the same
panic resumes. No execution-owned heap or Ralloc frame enters this cache.

`compiled_cache_stats()` reports the byte charge, table capacity and lifetime
cache activity. The charge includes buffer capacity and shared verified data
in full; allocator/Arc overhead, unused hash buckets and opaque profile-tree
slack are excluded. It is not RSS or peak execution memory. The borrowed
compile-only APIs may additionally hold one non-reusable result, separately
reported as `transient_bytes` and cleared on the next mutable operation.
Cloning omits that result and scratch pools, and recalculates retained charges.
`clear_compiled_cache()` releases compiled storage without clearing sources.

The source entry points keep a second cache of verified compilations. Source
content is bucketed by a 16-byte Blake2 digest and compared byte-for-byte on a
hit, so digest collisions cannot substitute a different program. This removes
lexing, parsing, optimization, and verification from repeated
`JitCache::run_source` calls without weakening first-run validation. Retention
is bounded by deterministic LRU eviction: the defaults are 128 exact sources
and 8 MiB of source text. Configure both limits with
`JitCache::with_source_cache_limits`; zero disables exact-source retention.
`JitCacheStats` exposes source bytes, hits, misses, compilations, evictions, and
bypasses in addition to compiled-program statistics.

The lowered JIT also recognizes branch-safe slot-to-slot moves, in-place
slot/immediate multiply and floor-divide, compare-then-jump, and direct
zero-jump sequences. Hot variants inspect encoded I64 slots directly and avoid
the intermediate stack values; non-I64 tags take the same generic numeric or
truthiness path as ordinary bytecode. Lowering leaves a sequence unfused when
any branch targets its interior. For opt-in attribution,
`JitOptions::with_execution_profile(true)` records opcode dispatches and
operand-stack traffic, sampling attempts, specialization hits/misses and
promotions/demotions. Encoded arithmetic that errors before returning a guard
result is not included in the hit count. These detailed counters remain off
by default, while completed runs reuse a bounded pool of cleared
operand vectors. Cloning a `JitProgram` copies its compiled/adaptive state and
historical counters, but starts with an empty scratch pool. Cached stack counts
and capacities describe only currently idle storage, not checked-out stacks.
Detailed profiles are boxed only when requested; absent counters do not occupy
the normal program header. Retention accounting includes the allocated profile
header as well as its recorded map storage. Clones own independent profiles;
resetting counters keeps the profile allocation but releases its map entries.

Compatibility methods accepting raw `Program` values verify before insertion.
`compile_verified` and the `run_verified_program*` methods preserve an earlier
verification capability and its memoized fingerprint.

---

## Choosing a Backend

| Scenario | Recommendation |
| --- | --- |
| Debugging or correctness checking | `vm` — simpler dispatch, easier to correlate with bytecode |
| Production / hot loops | `jit` — superinstructions and quickening reduce dispatch overhead |
| Running the same program repeatedly | `jit` with `JitCache` — quickened state carries over across calls |
| Comparing behavior between backends | Run both; the test suites assert VM/JIT parity |
