---
title: Architecture
---

# TinyLang Implementation Architecture

This document describes the current implementation, not a permanent
architecture promise. TinyLang v1 keeps its ABI frozen, but the allocator, VM,
JIT, memory design, syntax, and semantics may evolve. TinyLang v2 is expected
to make major changes as the language boundary and implementation model are
redesigned. Send comments, concerns, and questions to the [TinyLang community
forum](https://tl.404connernotfound.dev).

This document describes how the TinyLang implementation is structured
internally: the pipeline each TinyLang program travels through, the major
modules, and the invariants each stage owns.

## Pipeline Overview

```
source text
    │
    ▼
┌─────────┐
│  Lexer  │  syntax/lexer.rs, syntax/token.rs
└────┬────┘
     │  Vec<Token>
     ▼
┌──────────┐
│ Compiler │  compiler/mod.rs, compiler/parser.rs,
│          │  compiler/state.rs, compiler/symbols.rs,
│          │  compiler/modules.rs
└────┬─────┘
     │  Program (unoptimized)
     ▼
┌───────────┐
│ Optimizer │  bytecode/peephole.rs
└────┬──────┘
     │  Program (optimized)
     ▼
┌──────────┐
│ Verifier │  bytecode/verifier.rs
└────┬─────┘
     │  (error or proceeds)
     ▼
  ┌──┴──┐
  │     │
  ▼     ▼
 VM    JIT
```

The public API surface (`api.rs`) ties these stages together. The
`compile_*_verified` entry points return a `VerifiedProgram` capability that
keeps both the immutable `Program` and its memoized fingerprint. Compatibility
entry points still return `Arc<Program>`. Execution APIs accepting the verified
capability skip duplicate verification and fingerprint work.

File compilation may first consult the dependency-validated disk cache. A
valid compact binary artifact bypasses lexing, parsing, optimization, and
module assembly, but is still decoded under resource limits and verified.
Tiny source graphs are compiled directly when the validation overhead would
cost more than rebuilding them.

## Module Map

```
TinyLang/src/
├── lib.rs              Public re-exports; feature gates for testing
├── api.rs              compile_source, compile_file, lex_source, optimize_program
├── cli.rs              CLI argument parsing and dispatch
├── runner.rs           run_program, run_program_report, run_program_with_env,
│                       run_source, run_source_report
├── error.rs            TinyLangError (Compile | Runtime), Result<T>
├── source.rs           SourceMap — source ownership and error-only line lookup
├── source/bytecode.rs  Shared file owners, fixed bytecode origins, error sites
├── builtins.rs         BUILTINS table, builtin_index lookup
├── artifact_io.rs      load_artifact, write_artifact (file-level I/O)
├── ffi.rs              extern "C" entry points and JSON response helpers
│
├── syntax/
│   ├── mod.rs
│   ├── lexer.rs        Lexer: source text → Vec<Token>
│   └── token.rs        TokenKind enum
│
├── bytecode/
│   ├── mod.rs
│   ├── opcode.rs       Op enum (31 opcodes), name/from_name/ordinal
│   ├── instr.rs        Instr { op, arg: i64, arg2: i64 }
│   ├── program.rs      Program, Function, StructDef, ModuleDef,
│   │                   VerifiedProgram, fingerprint (Blake2b512)
│   ├── artifact.rs     Program::to_artifact / from_artifact (JSON serde),
│   │                   resource limits, reject_over_limit
│   ├── peephole.rs     PeepholeOptimizer — constant folding pass
│   └── verifier.rs     BytecodeVerifier — stack-input + control-flow check
│
├── compiler/
│   ├── mod.rs          Compiler entry point, module resolution wiring
│   ├── parser.rs       Recursive-descent parser + bytecode emitter
│   ├── state.rs        CompilerState, CompilerSharedState
│   ├── symbols.rs      SymbolTable — scoped slot allocation
│   └── modules.rs      Module import/export resolution, resolve_import
│
├── runtime/
│   ├── mod.rs          Public runtime re-exports
│   ├── vm.rs           VM interpreter, TinyRunReport
│   ├── heap.rs         TinyHeap, HeapObject, HeapData, TinyHeapStats
│   ├── memory.rs       TinyMemory — stack-frame slot vector
│   ├── context.rs      TinyRuntimeContext — heap + I/O + sys args/env
│   ├── value.rs        Value (Int | Heap | Pointer), RuntimeValue
│   ├── aggregate.rs    Array/struct/buffer/cell runtime operations
│   ├── arithmetic.rs   runtime_add, runtime_sub, runtime_mul, runtime_div
│   ├── builtins.rs     runtime_call_builtin dispatcher
│   ├── stdlib.rs       Phase-2 stdlib bridge (vec, map, io, string, …)
│   ├── pointers.rs     Raw-pointer operations (ptr_load, ptr_store, …)
│   ├── format.rs       runtime_print formatting
│   ├── limits.rs       Runtime resource constants
│   └── typing.rs       TypeKind enum, type_of, smallest_fit, promote
│
└── jit/
    ├── mod.rs
    ├── op.rs           JitOp enum — decoded, operand-unboxed instructions
    ├── chunk.rs        JitChunk — compiled function/main chunk
    ├── origins.rs      Fixed cold bytecode origins and sparse fused-operation sites
    ├── execution.rs    ChunkExecution — installed-chunk borrow and adaptive accounting
    ├── frame.rs        Call targets/cursors and bounded execution-local frame reuse
    ├── call_feedback.rs Experimental opt-in callee observations and guarded entry metadata
    ├── program.rs      JitProgram — full compiled program + hot-path quickening
    ├── cache.rs        JitCache — content/authority-keyed program cache
    └── vm.rs           JitVm — JitOp interpreter
```

## Stage Details

### Lexer

`Lexer::new(&source_map).tokenize()` scans the source in one pass and
returns a flat `Vec<Token>`. Tokens carry their `TokenKind` and a source
position for error messages. The lexer rejects non-ASCII characters that are
not part of a string literal.

The lexer borrows the compiler's immutable source owner; it does not duplicate
source text or construct another diagnostic map. Standalone `lex_source` creates
one owner for that call. `SourceMap` stores source and filename, with no eager
line-index allocation. Diagnostic formatting locates a failing line on demand,
without a source-table lookup in successful dispatch. Columns and spans remain byte-based,
including UTF-8, CRLF and end-of-input behavior. Repeated direct diagnostic
lookups are linear in the preceding source length, not indexed queries.

### Compiler

`Compiler::new(source, filename, resolver, ...)` constructs a single-pass
recursive-descent compiler. Parsing and bytecode emission happen in the same
pass — there is no AST. `compile()` consumes the compiler and returns an
unoptimized `Program`.

The testing-hooks-only `compiler/profile.rs` provides an explicit source
compilation capture entry point; it does not instrument these ordinary
methods. A fixed four-element profile records constructor/lexing, fused
parsing/emission/assembly, optional peephole optimization and authoritative
verification with verified-owner construction. Outcomes are typed as not run,
succeeded or failed; errors retain the ordinary diagnostics, and all capture
state belongs to the call. No profile Vec, Box, TLS or global observer is
needed. Lazy fingerprinting and final returned-owner destruction are outside
the named intervals. This source-only wall-clock diagnostic does not split
the fused pass into artificial stages or include module/file resolution;
see [performance guidance](performance.md) for its observer and scope limits.

The lexer builds a vector, then the parser takes its owning iterator. Lookahead
borrows the remaining token slice; consuming a token moves its text instead of
cloning it. There is no separate cursor index or parser-side resizing, and no
per-token boxing. The original token backing allocation remains until the
iterator is dropped; consumed text is released or transferred to its next
owner. Token source positions remain unchanged.

Imported units contribute declarations and module metadata to the shared
compilation session; they do not assemble temporary `Program` snapshots or
allocate a discarded main chunk. Final assembly requires sole ownership of
the session and moves its completed tables into the result. Function bodies
likewise transfer their code and local-name buffers instead of cloning them.
Finished buffer extents are compacted through boxed slices while retaining the
existing program representation; this avoids retaining builder capacity in
every function's debug-name table. Shrinking may relocate a buffer, but does
not clone its elements. Compiler-only lookup tables are dropped at final
assembly. Compiled main/function chunks now retain fixed boxed source-offset
tables and share one immutable source-text owner per file. The peephole pass
compacts origins alongside bytecode; optimizing a clone uses copy-on-write
only for changed origin tables. Host-width byte offsets avoid imposing the
file loader's source-size limit on the string compilation API. Retention
accounting includes diagnostic tables and each distinct source owner once.
This metadata is excluded from executable equality, content fingerprints and
JSON/binary artifacts; decoding an artifact does not invent source information.

The portable VM records the first failing bytecode site while unwinding and
formats it once at its execution boundary, preserving
`TinyLangError::Runtime(String)`. The JIT retains original bytecode indexes in
immutable cold tables, with separate constituent sites for fused operations.
Quickening changes executable elements without changing their origins; source
lookup and formatting happen only after failure. Compiled-cache hits rebind
the current verified source owner while retaining equivalent compiled code
and adaptive state. Dependency-validated disk-cache hits restore source owners
from a separate private sidecar. The diagnostic pipeline's cold/runtime
performance acceptance is still open.

Key compiler subsystems:

- **SymbolTable** — lexically scoped slot allocator. Each `let` declaration
  claims the next stack slot in the current scope. Block exit does not reclaim
  slots; slots are zero-initialized at frame entry and hidden after their scope
  exits.
- **Module resolution** — all nested compilers share one `ModuleResolver`.
  Canonical paths, manifest probes (including missing manifests), parsed
  manifests, import results, and source text are memoized for the compilation
  session. A source file is therefore read once even when reached through
  several aliases. Circular imports are detected in `CompilerSharedState`.

### Compile Cache

The CLI enables a sibling `.tinylang-cache/` by default. Its metadata records
the compiler/cache format version, optimization mode, root path, content
digests for every source and manifest probe, canonical path resolutions, and
logical module names. A hit loads the compact `.tob` artifact and verifies its
fingerprint before use. Input contents are still digested and recorded
resolutions are still canonicalized; file metadata alone is not a trust
decision. Missing, stale, malformed, version-incompatible, or unverifiable
entries are ordinary misses. Cache format v9 also requires a private `.origins`
sidecar; the public JSON and binary artifact formats are unchanged. The sidecar
stores fixed-width source-input indexes and byte offsets, not source text or
authority. Its snapshot digest binds the executable fingerprint, optimization
mode and ordered input paths/digests, so identical bytecode produced from
different whitespace cannot reuse stale locations. A checksum detects damaged
or torn combinations; neither digest authenticates the cache.

Origin counts and the exact sidecar size come from the verified program, not
untrusted counts in the sidecar. Already digest-checked source buffers move into
one shared source owner per file; manifests and signatures are not retained as
diagnostic text. Source-buffer capacity is bounded by the verified source-file
count times the existing 1 MiB input limit. The probe installs all origin tables
privately, without changing code, verification certificates or policy, and
refreshes retained-byte accounting before publishing the result.

Metadata decoding, input comparison, resolution validation, and artifact
loading form one cache probe shared by hit and incremental paths. The probe
does not reload metadata or re-hash every input after discovering one changed
module. Executable artifacts remain compact binary files. Graphs with at
most two imported modules and at most 4 KiB of loaded source/manifest text are
bypassed. Linux processes running under WSL also bypass disk caching for source
trees on mounted Windows drive paths such as `/mnt/c`, where validation was
slower than recompilation at every measured graph size. A bounded 64-entry
process-local LRU remembers only size-based policy decisions. Bypassed entries
always compile current source and never reuse a program based on metadata.

When exactly one existing module source changed, TinyLang attempts a bounded
incremental rebuild. It recompiles that module and its imports, relocates table
indexes by semantic name/content, replaces only declarations owned by that
module, and verifies the combined program. The source bytes and digest already
read by the cache probe are reused by the incremental compiler and cache
writer. Unchanged chunks retain their restored locations; only the module about
to be replaced may temporarily lack old origins. Replacement functions carry
fresh source maps through index relocation. Declaration-topology changes or
any failed invariant fall back to a full compile. Cache failures never prevent
a correct source build.

### Optimizer

`PeepholeOptimizer::optimize(program)` runs bounded forward passes over each
chunk's instruction stream. It folds adjacent constant pushes through
arithmetic and comparison opcodes, collapsing patterns like
`PUSH_INT 2, PUSH_INT 3, MUL` into `PUSH_INT 6`. It optimizes straight-line
regions inside control-flow-heavy chunks, never consumes an interior branch
target, and remaps jump offsets after each shrinking pass.

Optimization consumes each owned instruction vector and compacts it in place.
A no-fold scan skips branch-target and remapping allocations when there is no
constant candidate; a target-protected candidate cannot trigger a rewrite.
Successful passes write only behind unread instructions, remap branches, and
truncate the initialized prefix. Finished code discards spare capacity once,
after all passes, rather than cloning input and allocating output every pass.
Already-compact unchanged chunks keep their allocation. This stage still
accepts unverified programs; invalid targets are left for the verifier to
reject, not used as unchecked indexes or silently repaired.

### Verifier

`BytecodeVerifier::verify(program)` runs a bounded depth-first traversal over
the control-flow graph of every chunk. It tracks operand availability and depth at each reachable
instruction and rejects programs where:

- Stack depth is inconsistent at a join point (a target reached from two paths
  with different depths).
- Stack depth exceeds `MAX_STACK_DEPTH` (65,536).
- A branch target is out of range.
- A slot index, string index, field index, or struct index is out of range.
- Builtin argument count is outside the declared `[min_args, max_args]` range.
- Module metadata is inconsistent, forged, cyclic, or names a missing export.
- A call, function value, struct construction, enum construction, or global
  load crosses a module boundary without the required export/import authority.
- A chunk does not end with the required terminal opcode (`HALT` for main,
  `RETURN` for functions).
- Work steps exceed `MAX_VERIFIER_STEPS` (10,000,000) — guards against
  adversarially crafted dense control-flow graphs.

The traversal uses a dense instruction-indexed table to break backward-edge
loops. A typed nonzero encoding stores reachable depth plus one; `None` means
unseen. Each entry occupies four bytes, including the maximum depth of 65,536.
A repeated target with the same depth is not re-queued; a different depth is
rejected as a stack mismatch. Static operand and module checks still inspect
unreachable instructions before traversal. The table is temporary and bounded
by the already-checked total instruction budget.

All verification runs before any allocation for execution.

The VM and JIT share this verifier. Dynamic function-name entry points
(`closure_new` and `thread_spawn`) separately enforce the invoking function's
same-module/import-and-export authority because their target is supplied as
runtime data, not encoded in an instruction. Native shared-library imports currently fail
closed: arbitrary DLL/SO execution is outside the verified bytecode boundary
until a versioned native ABI and isolation policy are implemented.

### VM Backend

`VM::new(program, memory, inputs)` verifies the program and constructs an
interpreter. `vm.run(stdout)` enters the main instruction loop. Each opcode
maps directly to a Rust operation against the operand stack, the stack-frame
memory, and the heap context.

Function calls push a new frame onto a call-depth counter (limit: 16) and
dispatch to the function's bytecode chunk. All VM operations return `Result`;
there are no `unwrap` or `panic` calls on production paths.

`VM::new_unchecked(program, memory, inputs)` skips re-verification and is
used internally by `runner.rs` which has already verified the program.

### JIT Backend

The JIT is an **adaptive bytecode tier**, not a native machine-code JIT.

**Compilation** (`JitProgram::compile`):

1. Accepts or constructs a `VerifiedProgram` (same verifier as the VM path).
2. Consumes the verifier's immutable basic-block ranges and entry stack depths.
   Chunk entry, branch targets and instructions following terminators start
   blocks. Unreachable blocks retain ordinary encoding without invented stack
   facts; static verification still covers their operands.
3. Tracks constants, local reads and binary expressions symbolically within
   each reachable block. Stores, calls, heap operations and IO materialize
   earlier values in evaluation order. Boundaries restore the physical operand
   stack, including non-empty merges. Expression traversal uses an explicit
   work vector rather than host recursion.
4. Runs ordered local passes over source-ordered definitions/actions: constant
   propagation, copy propagation, slot-version propagation, checked I64
   constant folding, branch folding, dead temporaries, and safe dead stores.
   Versioned reads reject stale aliases after a source write. Generation-stamped
   dense slot facts are invalidated at every opaque effect and block boundary;
   no repeated full-table clearing is needed. Folding failures remain runtime
   operations, and only adjacent proven non-failing literal stores can be
   removed. Floating-point arithmetic stays in shared runtime semantics.
5. Performs final fusion into direct scalar families such as `BinarySlotsStore`,
   `BinarySlotIntStore` and `BinarySlotFloatStore`, retaining existing in-place
   encodings where applicable. Float immediates preserve literal bits and use
   shared numeric semantics, without assuming the source slot's runtime type.
   Unfused expressions descend directly into their left child, queuing only
   the pending right child and operator. Leaves and fused children resume that
   work in postorder, preserving failure order and source origins.
   These use typed 16-bit slot indexes (not slot counts); bytecode block offsets
   use 32 bits. Temporary IDs use checked nonzero `u32` indexes with a compact
   optional representation. Three collection-specific idioms remain explicit effect barriers
   during the local-IR migration.

The expression arena, ordered action stream, opaque effect table and pass
scratch storage are private to lowering and released at chunk installation.
Their capacities are reused across blocks, without carrying facts across
control-flow boundaries. The encoder no longer independently folds branches
or removes discarded expressions; those decisions belong to the ordered passes.
Adjacent definitions share one end-of-run record. Every action flushes that
pending record before entering the stream, so runs cannot hide a write or
observation. Copy propagation reports whether any aliases need revision checks;
an empty proposal set skips the otherwise ineffectual version walk. Action
and origin buffers are private to `LocalBlock`, which appends and clears them
together and exposes only read-only slices to the encoder. Rewrites preserve
entry indexes. A combined-record prototype removed an allocation but regressed
store-heavy lowering, so the separate layout was restored. The larger scaling
fixtures and origin rewrite/reuse regression test remain. This is temporary
builder storage; installed diagnostic tables remain separate from executable
ops. See the [scaling checkpoint](optimization-worklog.md#action-storage-scaling-paired-records-withdrawn).

Function chunks are lowered into reserved slots only on their first call.
`JitProgram` retains the verified program as the single owner of strings,
functions, structs, fields, modules, and enum metadata instead of cloning
those tables. Human-readable listing generation explicitly lowers all chunks.

Installed chunk slots and edge counters are boxed slices: their lengths are
fixed even while lazy installation or quickening changes elements. Operations
use a `JitOps` owner that exposes only slice access. Its underlying vector
keeps small builder allocations intact; installation reclaims spare operation
capacity when it reaches 4 KiB. Earlier measurements discouraged unconditional
shrinking, but the current Vec/Box recheck does not establish a speed advantage
for either container. Live operand/call stacks and bounded reuse pools remain vectors, whose
lengths change during execution. Frames contain owning handles, not references
into the call vector, so relocating a frame header does not move its payload.

**Hot-loop quickening**:
Each compiled chunk carries an `edge_counts: Box<[u16]>` parallel to its ops.
`JitVm` increments the counter at every backward branch. When a counter reaches
the configured threshold (8 by default), `JitChunk::activate_range(target, end)`
arms type sampling in `[target, end)`. Eight consecutive actual-I64 observations
are required before installing the typed variants below. Pure branch-counter
elision does not assume operand types:

| Cold op | Hot op |
| --- | --- |
| `Add` | `AddInt` |
| `Sub` | `SubInt` |
| `Mul` | `MulInt` |
| `Div` | `DivInt` |
| `Compare(op)` | `CompareInt(op)` |
| `Jump(target)` | `JumpHot(target)` |
| `JumpIfZero(t)` | `JumpIfZeroHot(t)` |

`*Int` variants recognize the common `I64` pair once, perform checked
arithmetic directly, and fall back to the generic numeric implementation for
other value kinds. `*Hot` variants skip the branch-counter increment.
Type feedback is payload-free and stored in lazy, fixed-length boxed tables
under one 64-KiB budget per program. Each table covers the entire lowered
chunk, including operations that cannot sample; large chunks can exhaust the
budget even when few sites are adaptive. Sampling records at most two shapes and
32 observations. Unsupported stable tags and unstable sites remain generic.
Four specialization misses without 32 consecutive hits trigger resampling;
two promotions per site bound rewriting, after which instability selects a
terminal generic handler. Misses always use the shared generic semantics.
Clones own independent feedback tables; no runtime values or captures survive
in them. Oversized tables are skipped without restricting execution.

`JitOptions` configures this threshold for `JitProgram`, `JitCache`, and the
configured runner APIs. Threshold zero disables quickening while retaining JIT
lowering and superinstructions.

On each frame resume, `ChunkExecution` borrows the installed chunk and the
program's adaptive counters, optional profile and shared feedback budget.
Instruction fetch and feedback updates use that borrow instead of repeatedly
resolving a chunk-table index. Fetch bounds and representation guards remain
checked. The borrow ends on call, return or error; it cannot outlive lazy
installation or hide rewrites made by a recursive call. This adds no retained
metadata, runtime heap allocation, new opcode or instruction-decode layer.

**Caching** (`JitCache`):
Programs are identified by their Blake2b512 content fingerprint (16 bytes,
encoded as 32 hex characters) together with policy trust. The content digest
round-trips through untrusted artifact loading; the additional trust state
prevents cache reuse from lending or removing runtime authority. On a cache
hit, quickened state from a previous run is preserved across repeated calls.
`run_source` and `run_source_report` also retain the first verified compilation
of each exact source string. A 16-byte content digest selects a collision
bucket, then exact source equality is required before reuse; new or changed
source is compiled and verified before entering either cache. The exact-source
cache uses deterministic least-recently-used eviction and defaults to 128
entries and 8 MiB of retained source text. `JitCacheStats` reports retained
entries/bytes, hits, misses, compilations, evictions, and bypasses. The compiled
fingerprint cache remains independent, so evicting source text does not discard
already-quickened JIT state for an equivalent recompiled program while that
compiled entry remains within its own budget.

The source access clock renumbers live ages before overflow, preserving LRU
order across collision buckets without changing owners, byte charges or
lifetime activity counters.

`jit/compiled_cache.rs` owns compiled retention: 128 entries and 16 MiB of
charged entry data by default, configurable independently of source caching.
It accounts lazy code, feedback and cleared operand capacity after executions,
including errors, and evicts least-recently-used entries. Oversized runs bypass
retention; the borrowed compile-only API has one separately reported temporary
result, cleared by the next mutation. A host writer panic removes the affected
entry before propagating. Clones omit transient/scratch storage and recount
their allocation extents. Immutable deep bytecode accounting is memoized with
the verification token in `bytecode/retention.rs`; normal cache-run accounting
does not rescan all bytecode or installed chunks. The opt-in profile still
requires inspecting its small opcode map. Byte charges include capacities and
shared verified data in full, but exclude opaque allocator/Arc overhead,
unused hash buckets and profile-tree slack; they are not RSS measurements.
Compile-only hits with the exact same immutable source owner reuse their
stored charge: they cannot grow code, feedback or scratch. Rebinding to a
different source owner still recalculates retention before returning a result;
executions and limit changes continue to account their own mutations.

Branch-safe JIT lowering fuses slot-to-slot moves, slot/immediate arithmetic,
slot/immediate comparison followed by a conditional jump, and direct
slot-zero jumps. Once observed types justify specialization, the I64 arithmetic and branch forms read
encoded slots directly, avoiding redundant decoding and operand-stack traffic.
Every fused operation retains the generic numeric/truthiness fallback for
float, unsigned, and narrow signed values, and candidate ranges containing an
interior branch target are never fused.

`JitOptions::with_execution_profile(true)` enables diagnostic per-opcode
dispatch, operand-stack accounting, sampling, guard hits/misses and adaptive
transitions. It is off by default. Each completed
run also returns its cleared transient operand stack to a bounded JIT-local
pool; recursive calls keep distinct stacks until they return. An immutable
verification certificate supplies reachable per-chunk stack peaks and numeric
permission ownership. The JIT uses these facts for operand capacity and borrowed
capabilities instead of instruction-count estimates and repeated module-name
searches. Program clones preserve compiled/adaptive state and historical
counters, but do not copy scratch pools. Idle-stack count/capacity gauges start
at zero on the clone and track checkout as well as recycling.

JIT calls suspend explicit cursors in a vector, without recursing through the
host interpreter. Active calls own distinct Ralloc frame memory and preserve
the configured TinyLang depth limit. A separate execution-local pool retains
only fully cleared exact-size frames, bounded by eight frames and 64 KiB.
Idle memory is released before heap allocation/growth, on a failed fresh frame
allocation before retry, and on both successful and failed execution exit.
This pool is never part of a cached `JitProgram`.

Call exits leave argument ownership with the suspended caller. Frame entry
drains the operand suffix directly into encoded callee slots; closures borrow
and decode current captures into the leading slots without intermediate
vectors. This preserves the caller's stack prefix/capacity and drops remaining
arguments on transfer failure. The heap borrow ends before lowering or entering
the callee. Dynamic callable identity, arity and depth checks remain in force;
no closure-instance data is cached by this transfer path.

Closure literals compile to `MakeClosure` (function index plus capture count),
which builds the closure directly from the operand stack without an
intermediate name string or capture array. Mutable shared captures live in heap
cells created by `MakeCell` and accessed through `LoadCell`/`StoreCell`; a cell
capture passes the cell itself, so sibling closures and the defining frame
share one binding. The Cranelift backend runs these four ops through the
interpreter fallback.

Runtime maps retain insertion-ordered Ralloc entries and maintain a canonical
key index for average constant-time `map_get`, `map_has`, and `map_set`
lookup. Integer widths normalize by value, strings index by content, and heap
or pointer identities include their allocation generation. Pointer-key access
still validates every stored pointer key before consulting the index. That
validation reads only the encoded address, generation, and pointer kind, and
runs under the map operation's existing heap lock; it does not decode field
strings or acquire one lock per pointer entry. Indexed hits decode only the
requested value slot because canonical index equality already covers the key.

### Heap

`TinyHeap` is a generational slab. Slots hold `Option<HeapObject>` and a
parallel `generations: Vec<u64>` counter. When an object is allocated, it
claims the next free slot (or appends), and the generation is incremented.
When freed, the slot is set to `None` and added to `free`.

Every `HeapRef { address, generation }` carries the generation at allocation
time. Before any access, the runtime checks `stored_generation == current` and
returns `TinyLangError::runtime("Stale heap reference …")` if they differ. This
catches use-after-free and prevents a new allocation at the same address from
being confused with the old object.

`RawPointer { address, kind, index, field, generation, cast }` derives from a
`HeapRef` and adds a kind tag (`"null"`, `"object"`, `"array"`, `"buffer"`,
`"field"`) plus an element index or field name. Before raw-pointer use, the
runtime validates the base heap reference (generation check), the kind, and the
index or offset bounds.

Common heap accesses reuse one validated object lookup instead of repeating
generation/occupancy checks inside the same exclusive heap borrow. Vector pop
decodes the selected Ralloc slot through a borrowed slice, avoiding a temporary
host `Vec<u8>`. Cell churn retains at most one 64-byte payload for reuse; the
heap slot generation still increments and logical byte/accounting totals still
drop to zero on free. The cache is released at shutdown.

Builtin arguments are borrowed directly from the VM/JIT operand stack and the
stack is truncated after the call, so ordinary builtins do not allocate a host
argument vector. The runtime-owned `TinyAllocator` keeps exact allocation-table
and heap totals but disables its unreachable diagnostic ring; standalone
`TinyAllocator::with_defaults()` logging behavior is unchanged. An empty memory
hook registry also bypasses its read lock, while registered hooks retain the
same synchronized dispatch behavior.

### FFI Layer

`ffi.rs` exposes `extern "C"` entry points that all funnel through `respond()`.
`respond` wraps `response_cstring` in `catch_unwind(AssertUnwindSafe(...))`.
`response_cstring` wraps the actual callback in a second `catch_unwind`. If
either unwind guard fires, the fallback is a static byte literal that requires
no allocation. All JSON responses follow `{"ok": true/false, "kind": "…",
"value"/"error": …}`. See `docs/ffi/c-integration.md` for the full contract.

## Key Invariants

1. **Verify before execute.** Every public execution path calls
   `BytecodeVerifier::verify` exactly once before any instruction is run.
   Internal paths that chain from an already-verified call use `*_unchecked`
   constructors to avoid redundant re-verification.

2. **No panic on production paths.** All operations that can fail return
   `Result`. The only `unwrap`/`expect` calls in non-test code are inside
   `catch_unwind` guards in `ffi.rs` or in benchmark binaries.

3. **Limits before allocation.** Resource limits (artifact sizes, code counts,
   heap bytes, buffer sizes) are checked before `Vec::collect`, `File::open`,
   or heap allocation. Hostile inputs fail with a structured error rather than
   triggering unbounded allocation.

4. **Generation tags prevent dangling references.** `HeapRef` and `RawPointer`
   both carry a generation counter. A freed-then-reallocated slot has a
   different generation, so stale references are caught at the next access.

5. **ABI changes are explicit.** The renamed JSON `extern "C"` entry points
   declare ABI version 2 while retaining their response shapes. The v1
   contract remains frozen on `release/v1`.
