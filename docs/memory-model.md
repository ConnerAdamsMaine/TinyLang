---
title: Memory Model
---

This is the current Tynra memory and allocator design. It may change
during v1 as implementation work continues; the v1 ABI freeze does not
freeze the internal allocator or memory model. Tynra v2 is expected
to make major changes. Send comments, concerns, and questions to the
[Tynra community forum](https://tl.404connernotfound.dev).

The Tynra runtime implementation manages two memory regions: the
**stack-frame memory** (fixed slots per function call) and the **heap**
(dynamic allocation for all aggregate values). This document covers how
each region works, how references are validated, and which resource
limits apply.

For the value types that use heap allocation, see
[syntax/types.md](syntax/types.md). For the opcodes that interact with
memory, see [bytecode.md](bytecode.md).

# Stack-Frame Memory

Each function call owns a fixed number of slots determined at compile time
(`Function.slot_count`). `TynraMemory` stores 64-byte encoded values in a
Ralloc-owned byte allocation; it is separate from the `Vec<Value>` operand
stack. Active calls have distinct frame allocations.

- Slots are **zero-initialized** at frame entry (value `I64(0)`).
- Slots are **not freed** between loop iterations -- a slot retains its
  last-written value until the function returns.
- Slot count is fixed; the compiler allocates new slots for every `let`
  declaration. Block exit does not reclaim slots; names are hidden after
  their scope but the slot remains allocated in the frame.
- At function return, the VM releases the frame. The JIT may clear and retain
  it for same-size calls within that execution: at most eight idle frames and
  64 KiB of encoded slots. It drops idle frames before heap allocation/growth,
  retries a failed frame allocation after clearing the pool, and releases all
  pooled frames when the run ends. Neither values nor frame allocations are
  retained across executions.
- Function chunks can read top-level slots through `LOAD_GLOBAL` when
  the variable was declared before the function. They cannot assign
  those slots directly; shared mutation should happen through heap
  objects stored in a top-level slot.
- A spawned thread receives a snapshot of the top-level frame taken when
  `thread_spawn` is called. Heap references in that snapshot still
  resolve through the shared runtime heap; coordinate shared mutation
  with mutexes or atomic values.
- Spawned threads inherit the runtime's system arguments and
  environment. Deterministic input queues are not shared between
  threads; pass input values as function arguments before spawning.

Ralloc's safe byte-buffer APIs initialize new storage and newly exposed growth
tails to zero. This is distinct from raw C allocation, whose bytes need not be
initialized. In particular, reusing storage after a typed allocation cannot
expose that type's uninitialized padding through a safe byte slice. Pointer
arithmetic within each arena preserves the backing region's provenance;
integer addresses are used only for bounds and registry comparisons.

An alignment-adjusted allocation header may fall inside a freed block's former
payload. Ralloc initializes that header before reading or marking it allocated,
including when the remaining space is too small to split off a trailing block.
Old payload bytes must never supply the new allocation's size metadata.

Ralloc's first-fit scan retains a size-aware prefix hint: a block boundary and
an upper bound on free-block sizes before it. Only a strictly larger request
may skip that prefix; equal or smaller requests rescan from the arena base.
Freeing rewinds the boundary to an earlier coalesced block when necessary.
The successful boundary is the selected header's start, so repeated reuse can
leave the hint unchanged. This does not change placement, alignment, payload
ownership or the native arena pool. On the measured 64-bit builds it adds
16 bytes per arena, not per allocation. The
[retention decision](optimization-worklog.md#completed-stationary-allocator-guardrails-and-retention-decision)
records retained-object/fragmentation gains and the small-case, contention and
measurement tradeoffs; it is not a universal allocation-latency improvement.

The Rust `RallocBox<T>` owner transfers a zero-sized value's destructor
obligation without allocating or dropping it during construction. Raw handoff
and `into_inner` preserve that ownership. For allocated values, a stack-local
storage guard releases the box's backing after the value and its fields are
dropped, including when a destructor unwinds. It does not catch or suppress
the panic, and does not promise cleanup after process abort. This typed Rust
owner is separate from Tynra's encoded byte-buffer frame and heap storage.

# Compiled Cache Retention

The JIT cache owns code and immutable verified metadata, not execution heaps
or Ralloc frames. Its default reusable-entry limits are 128 programs and
16 MiB of charged retained data. Charges include nested bytecode/string/name
capacities, reserved lazy chunk slots, installed operations/counters, feedback,
and the full capacity of cleared idle operand vectors. Lazy growth is charged
after success or runtime failure; oversized entries are evicted or bypassed.
Detailed JIT profiles are optional boxed owners. Their headers and recorded map
storage are charged only when profiling is enabled; normal programs retain only
the optional owner pointer, not inline space for every diagnostic counter.

Fixed installed metadata uses boxed slices. Installed operation storage is the
explicit exception: `JitOps` hides its vector behind slice-only access, reclaims
spare capacity at 4 KiB and charges the full remaining capacity. Its Vec/Box
timing comparison is inconclusive; growing stacks and builders use vectors
for their changing initialized length, not a blanket speed preference.

Source-compiled programs also retain immutable source text and fixed boxed
bytecode-offset tables. A verified owner charges each shared source/table once.
Installed JIT chunks charge their shared cold origin tables, including sparse
constituent sites for fused operations; cloned chunks share those tables but
are charged inclusively. Cache hits with identical bytecode can replace the
diagnostic source owner, so rebound compile-only hits recheck retention.
Unchanged compile-only hits reuse the stored charge; every run still accounts
for lazy code, feedback and scratch growth, including failing runs.
Disk-cache hits now retain the same diagnostic ownership using fixed boxed
offset tables and already validated file buffers moved into source strings.
The private sidecar is transient: four bytes per instruction, four per chunk,
and 36 header/checksum bytes. Its maximum extent is derived from verified
instruction/function counts. Aggregate retained source-buffer capacity is
bounded separately from source-map headers, filenames and offset tables; this
is not a whole-process peak-memory bound.

Lowering's temporary action and origin buffers share one private `LocalBlock`
owner. They grow and clear together; optimizer rewrites preserve their indexes.
The combined-record prototype was withdrawn after store-heavy latency
regressions despite fewer allocation calls. Expression origins also use separate
scratch storage; all builder capacities die at installation. None of these
buffers is a Ralloc-owned execution payload or installed diagnostic table.

The final encoder's private work vector stores pending right children and
operators while descending directly into left children. At the measured
65,536-expression depth, its reservation falls from 2 MiB to 1 MiB and
preparation performs one fewer host reallocation. Installed storage is
unchanged. The [encoder retention checkpoint](optimization-worklog.md#completed-encoder-guardrails-allocation-accounting-and-memory-focused-retention)
records the allocation evidence and timing tradeoffs; this is not a Vec/Box
speed comparison or a whole-process peak-memory measurement.

Shared verified owners are charged in full per compiled entry. Allocator/Arc
overhead, unused hash buckets and opaque profile-tree node slack are excluded;
table capacity is reported separately. These figures are neither RSS nor a
peak-memory cap. The borrowed compile-only API can hold one separately charged
temporary result outside the reusable-entry budget until the next mutation.
Run methods do not retain such a result. The exact-source cache has independent
entry/source-text limits. See [Rust API](ffi/rust-api.md) for cache controls and
the distinction between resident-storage and lifetime activity statistics.

# Heap Architecture

`TynraHeap` is a **generational slab**:

- An `objects: Vec<Option<HeapObject>>` vector holds all heap objects.
  Each slot is either `None` (free) or `Some(HeapObject)`.
- A parallel `generations: Vec<u64>` vector holds the generation counter
  for each slot. New slots start at generation 1; reused slots increment
  immediately before allocation.
- A `free: Vec<usize>` list holds the indices of currently free slots.

**Allocation:** Claims the next free slot from `free` (or appends a new
slot if `free` is empty), increments the generation, and stores the
object.

**Deallocation (`unsafe free`):** Sets the slot to `None` and adds
the index to `free`. The generation increments if that slot is later
reused.

# `HeapRef` and Generation Validation

Every reference to a heap object is a
`HeapRef { address: usize, generation: u64 }`.

Before any access to a heap object, the runtime checks:

    stored_generation[address] == ref.generation

If they differ -- because the slot was freed and possibly reallocated
since `ref` was created -- the runtime returns
`TynraError::runtime("Stale heap reference ...")`.

This catches **use-after-free** and prevents a new allocation at the
same address from being mistaken for the old object.

# `RawPointer` and Validation

A `RawPointer { address, kind, index, field, generation, cast }` derives
from a `HeapRef` and adds:

- `kind` -- `"null"`, `"object"`, `"array"`, `"buffer"`, or `"field"`.
- `index` -- element or byte offset (for array and buffer pointers).
- `field` -- field name (for struct-field pointers).
- `generation` -- generation at pointer creation time.
- `cast` -- optional type annotation set by `cast_ptr`.

Before any pointer use, the runtime validates, in order:

1.  **Base generation** --
    `stored_generation[address] == pointer.generation`.
2.  **Kind** -- the live object at `address` matches `pointer.kind`.
3.  **Bounds** -- `index` is within the object's element or byte
    count.

A stale base object, kind mismatch, or out-of-bounds access each produce
a structured runtime error rather than undefined behavior.

# Ownership Rules

Tynra does not use garbage collection or compile-time borrow checking
in the current Tynra implementation. The Rust runtime owns the heap
for the entire run.

**Aliasing:** Copying a `HeapRef` or `RawPointer` aliases the same heap
object. It does not clone, move, or transfer ownership.

**Freeing:** `unsafe free(value)` releases the heap slot. Freeing is
**shallow** -- if the freed object contains references to other heap
objects, those referenced objects are not freed; they remain live until
separately freed or the run ends.

**Stale references:** After `unsafe free(value)`, all `HeapRef` and
`RawPointer` values that point to the freed slot become stale. Any
access to them produces a runtime error, even if a new object is later
allocated at the same address.

**Pointers to fields/elements:** Raw pointers to array elements or
struct fields remain valid across mutation of the same live object. They
become stale when the base object is freed.

# Resource Limits

The limits below describe logical runtime accounting, not whole-process memory.
Some language entry points preflight requests, but several heap constructors build
native storage before checking the heap budget. Rust heap imports also bypass
some per-container language limits. Host metadata, buffered I/O, frame storage,
container spare capacity and transient copies are not covered by the heap-byte
limit. See the implementation audit below.

- **Dynamic array length:** 65,536 elements. `push` beyond the limit or
  `array(count, ...)` with `count` above it fails.
- **Single buffer allocation:** 1,048,576 bytes (1 MiB). `buffer(size)`
  above the limit fails.
- **Total charged live heap payload:** 4 MiB. Any allocation that would exceed
  the charged live bytes fails; this does not bound physical reservation.
- **Live heap object slots:** 1,000,000 objects. Any allocation at the
  limit fails.
- **Nested Tynra calls:** 16 calls. `CALL` at the call-depth limit
  fails.

# Shutdown Drain

At program exit, the runtime drains all remaining live heap objects. The
`TynraRunReport` includes:

- `heap_before_shutdown` -- statistics immediately before the drain.
- `heap_after_shutdown` -- statistics after the drain; `shutdown_frees`
  counts objects freed by the drain.

The drain is not triggered by `unsafe free` -- only by runtime
shutdown.

# Implementation Memory Audit

Audit baseline: commit `6f54f60`, package `tynra-v2` 2.1.0, Windows
x86-64/MSVC, rustc 1.99.0. This audit inspected the current implementation,
ran focused tests and allocation profiles, and exercised temporary Rust API
probes. No runtime implementation changed. Measurements are build-specific
allocation/accounting observations, not RSS measurements or latency claims.

## Current strengths

- Ralloc-backed execution payloads have Rust owners that release storage on
  normal return and unwinding. Failed reallocations preserve the original
  owner and contents.
- Ordinary same-heap references reject freed and reused slots by generation.
  Explicit free is shallow and invalidates aliases; it does not discover dead
  children.
- The fixed native pool contains four 8-MiB arenas, shared process-wide by
  frames, heap payloads and other Ralloc callers. This is 32 MiB of static
  backing, not a measurement of resident pages. Allocation headers/alignment
  reduce usable capacity; one allocation cannot span arenas.
- JIT frame reuse is bounded to eight idle frames and 64 KiB per execution.
  Compiled caches have separate entry/retained-byte limits and do not retain
  execution heaps.
- C runtime/object handles validate ownership and domain as well as generation.
  Owned-buffer tokens reject stale descriptors and cross-runtime release.

## Confirmed findings, in priority order

### 1. Thread creation can escape a failed allocation

`runtime/stdlib.rs::b_thread_spawn` starts an OS thread before allocating its
language heap handle. A persistent VM with a two-object budget allocated an
atomic and the worker-name string, then rejected `thread_spawn` with
`Heap object limit 2 exceeded`. The worker nevertheless ran and changed the
atomic to 99. Losing the join handle detaches the worker.

The lifecycle has additional source-inspected gaps: one-shot VM/JIT reports
explicitly shut down the shared heap without joining outstanding workers;
freeing a thread object drops rather than joins its handle. Worker execution
uses `Unlimited` fuel, and constructing its VM restores the program call-depth
policy rather than the parent's tightened per-session depth. These latter
cases need dedicated concurrency regressions; existing threading tests join
successful workers.

Required fix: reserve handle/object/byte/thread capacity before spawning, use
fallible OS thread creation, and retain runtime ownership of every worker
independently of its language handle. Define cancellation and join behavior
before shutdown; never join while holding the heap mutex. Failed spawn must
start no user work. A worker must inherit the execution policy deliberately.

### 2. Logical limits do not enforce a physical memory budget

`TynraHeap::alloc` checks a completed `HeapObject`. Constructors such as
`alloc_string`, `alloc_buffer_with` and `alloc_array` have already allocated
native payloads by then. A VM limited to one heap byte rejected a 1-MiB Rust
buffer import only after allocating and freeing the full MiB.

`RallocVec` grows capacity fourfold and retains it on pop/clear; heap accounting
charges initialized elements only. A 257-element array followed by `vec_clear`
reported zero live payload bytes while a persistent VM still held 65,536
container bytes plus its 128-byte global frame. Three imported arrays grown
from 20,000 to 20,001 elements each reserved 15,360,000 native container bytes
while charging 3,840,192 logical bytes, below the 4-MiB policy.

Heap slot vectors, generations/free lists, map indexes and copied string keys
also consume host memory outside that policy. Empty objects can exhaust the
object budget while charging little or no payload. Slot-table capacity remains
at its high-water mark after free.

Buffered I/O is another independent owner. A function appended 5,120,000 bytes
to `io_stdout` under a 2,048-byte heap budget and returned successfully. This
buffer uses the host allocator and is not constrained by Ralloc's fixed pool.
Formatting builds recursive snapshots and intermediate strings, so a small
aliased graph can also produce much larger output and deep host recursion.

Required fix: distinguish logical payload, reserved native bytes, metadata,
frames/stacks, buffered output and temporary growth/copy headroom. Preflight and
reserve each controlled cost before side effects, then commit or roll back.
Use fallible host reservations on budgeted paths. A smaller logical limit must
not be presented as a process-memory bound.

### 3. Dead values accumulate throughout a run or persistent session

The runtime has neither reachability collection nor retain/release ownership
tracking. Overwriting a slot, discarding a return value, leaving a block and
returning from a function do not reclaim the referenced heap object. A fixture
that called a function creating a discarded one-element array 1,024 times
retained all 1,024 arrays, charging 65,536 bytes until shutdown. Persistent VM
tests already confirm that repeated allocations eventually hit the session
budget, even when the host discards previous results.

This is session-lifetime retention, not evidence of a native leak after
shutdown. Reclamation must account for aliases, captured cells, container edges,
globals and host-retained values; freeing everything at function return would
invalidate legitimate escaping objects. Cycles are possible through mutable
containers and cells.

### 4. Rust heap identities have no VM provenance

`HeapRef` contains only slot and generation. Passing VM A's first string value
to VM B's `copy_heap_value` returned B's first string when both occupied slot 1,
generation 1. This probe deliberately violated the documented same-VM API
precondition; it demonstrates a missing enforcement boundary, not native
undefined behavior or a bypass of the C handle registry. Release has the same
identity ambiguity and could target B's unrelated occupant.

Required fix: establish heap-domain identity in the public Rust ownership
boundary, and validate nested references/pointers consistently. Preserve the
existing C runtime/object ownership checks. Audit the internal generation-zero
wildcard separately; ordinary constructors currently issue nonzero identities.

### 5. Value representation imposes substantial fixed costs

On this build, both `RuntimeValue` and encoded slots are 64 bytes: an integer
array uses eight times the integer payload size before capacity slack. A map
entry uses 128 encoded bytes plus a host index; the measured index key/value
pair is another 80 bytes before bucket slack and copied key text. Field
pointers stored in slots are limited to 27 UTF-8 bytes and allocate a host
string on decoding.

The fixed width is an implementation choice, not a stable external wire
requirement. Prototype compact internal handles/values and interned field or
shape IDs after ownership rules are explicit. Preserve wide integers, floating
point behavior, stale checks and all pointer semantics. Do not assume a target
width or a speed improvement before measuring it.

## Measurements worth preserving

The existing JIT allocation profile passed in cold and warm executions with
hot-back-edge thresholds 0 and 8. Representative counts per execution:

- 4,096 explicit cell allocate/free operations: two native allocations,
  including the global frame; the one-cell spare prevents payload churn.
- 512 simple function calls: two native allocations via bounded frame reuse.
- Repeated depth-10 recursion: 201 native allocations, peak native payload
  832 bytes. The eight-frame pool leaves some churn but has a small footprint.
- A 256-element array filled and emptied: zero logical bytes at shutdown,
  16 KiB logical peak and 20,736 bytes native peak, including frame and growth
  overlap. The container retains capacity until its object is freed.
- The retained-object profile passed through 4,096 cells, strings, structs and
  enums. Every measured fresh execution balanced native allocation/free
  counts and ended with zero live native payload after its report was dropped.

These counters exclude host collection allocations and static arena backing.
They justify keeping bounded reuse while evaluating reclamation and accounting;
they do not justify changing the growth factor or allocator algorithm alone.

## Proposed memory-management direction

Preserve Tynra's stated deterministic, non-tracing cleanup direction with
three explicit lifetime domains:

1. **Frame scratch:** locals, operand stacks and proven non-escaping temporary
   payloads. Reclaim as a group on frame exit, with bounded reusable storage.
   Begin with compiler-proven non-escaping sites; do not reset a region that
   can still be referenced by a caller, global, closure or worker.
2. **Escaping/shared objects:** stable generational handles with explicit
   ownership accounting. Use counted strong edges for acyclic shared objects;
   retain/release on slot overwrite, stack transfer, container mutation,
   captures, globals, host roots and worker results. Drain destruction through
   an iterative work queue rather than recursive host drops. Handle release
   removes a root; it must not blindly free an object with other owners.
3. **External resources:** runtime-owned thread/resource records with explicit
   close/join/cancel states. Their lifetimes cannot depend on whether a user
   happens to retain a heap wrapper.

The current cloneable `RuntimeValue` API does not register host roots. A shared
ownership design therefore needs an owned/rooted Rust handle boundary, not just
a reference-count field added to `HeapObject`. Specify whether derived pointers
keep their base alive or remain checked, non-owning aliases; raw-pointer
semantics must not accidentally turn into counted ownership.

Reference counting alone cannot reclaim arbitrary strong cycles. Before
shipping shared reclamation, choose and enforce a cycle policy: region-owned
cycles reclaimed at region teardown, weak back-edges, or rejection of strong
cross-region cycles. Transparent reclamation of arbitrary cycles would require
an additional mechanism and reconsideration of the non-tracing goal. Do not
promise bounded persistent-session memory while leaving this unresolved.

## Implementation sequence and acceptance gates

1. **Close lifecycle and isolation defects.** Add regressions for failed spawn,
   outstanding-worker shutdown, inherited policy, cross-VM Rust identities,
   late allocation checks and Rust-import per-container limits. A 1,048,577-byte
   buffer import currently succeeds despite the language buffer cap. Reserve
   and roll back capacity atomically; maintain existing mutation/error ordering
   where it is part of the contract. Keep public explicit free shallow unless
   a separately versioned ownership contract changes it.
2. **Publish honest accounting.** Add per-runtime logical/reserved/metadata,
   frame/operand, output and transient-peak measurements, plus process-wide
   native pool availability and largest-free-block diagnostics. Keep cache
   retention separate. Verify failure before disallowed allocation or output
   growth and reconcile counters after both success and traps. Measure host
   allocations and process working set separately from native payload counters.
3. **Specify and prototype deterministic reclamation.** This is the design
   bottleneck: define strong/weak edges, escaping regions, roots, manual-free
   interaction and cycle handling first. Cover every `HeapData` variant,
   pointer/reference carrier and VM/JIT transfer path. Repeated temporary-only
   calls should plateau after warm-up; retained globals, returned values and
   shared captures must survive. Test replacement, aliasing, cycles, shutdown
   and destructor cascades under allocation failure.
4. **Reduce representation and allocation costs.** Compare compact slots and
   interned record shapes with the current codec. Reuse dead local slots when
   capture/liveness analysis proves it safe, reduce staging copies and bound
   formatting depth/output. Evaluate size-class pools for small fixed payloads,
   in-place native growth and 2x versus 4x collection growth using peak physical
   reservation as well as throughput. Preserve existing bounded cell/frame
   reuse unless measurements show a better tradeoff.
5. **Make capacity a runtime policy.** After per-owner reservation works,
   evaluate independently owned runtime arenas and page-backed growth with
   explicit hard ceilings. The existing backend/page abstractions do not yet
   replace the global static pool. Test concurrent VM fairness, fragmentation,
   large contiguous requests and release of idle capacity before raising caps.

Retain interpreter/JIT parity, stale-reference failures, C handle isolation,
failed-reallocation ownership and post-trap recovery as regression gates.
Do not replace the allocator, add a tracing collector or relax limits as a
substitute for resolving ownership and reservation.

## Verification performed

- Full `tynra_ralloc` tests with all features, serial execution: 86 passed.
  Platform-gated C-suite and Send/Sync binaries contained no runnable tests
  on this host.
- Focused core integration tests: allocator audit, real native exhaustion and
  recovery, persistent VM, compiled-cache retention, owned-buffer transfer,
  closure/call transfer, threading, runtime parity, Rust heap interchange,
  VM session contracts and retained-object parity. All executed tests passed.
- Runtime unit tests and the explicit map-index layout diagnostic passed.
  Both ignored allocation-distribution and retained-object profiles were
  invoked explicitly and passed. Across these commands, 285 test executions
  passed; the documentation-only double-deallocate test remained ignored.
- Temporary executable probes established the numerical findings above and
  finished with zero live Ralloc payload. They did not inject host-allocator
  OOM or establish a whole-process peak-memory bound.
- Miri could not run: its component is unavailable for the installed stable
  Windows/MSVC toolchain. Earlier Miri results in the optimization worklog are
  historical evidence, not a fresh result from this audit.

This was a focused memory audit, not the full workspace/release gate or a
complete unsafe-code proof. Optional Cranelift and LLVM/native execution
backends were not revalidated. Cargo also reported the existing library/bin
`tynra.pdb` output-name collision.
