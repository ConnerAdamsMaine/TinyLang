---
title: Memory Model
---

This is the current TinyLang memory and allocator design. It may change
during v1 as implementation work continues; the v1 ABI freeze does not
freeze the internal allocator or memory model. TinyLang v2 is expected
to make major changes. Send comments, concerns, and questions to the
[TinyLang community forum](https://tl.404connernotfound.dev).

The TinyLang runtime implementation manages two memory regions: the
**stack-frame memory** (fixed slots per function call) and the **heap**
(dynamic allocation for all aggregate values). This document covers how
each region works, how references are validated, and which resource
limits apply.

For the value types that use heap allocation, see
[syntax/types.md](syntax/types.md). For the opcodes that interact with
memory, see [bytecode.md](bytecode.md).

# Stack-Frame Memory

Each function call owns a fixed number of slots determined at compile time
(`Function.slot_count`). `TinyMemory` stores 64-byte encoded values in a
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
owner is separate from TinyLang's encoded byte-buffer frame and heap storage.

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

`TinyHeap` is a **generational slab**:

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
`TinyLangError::runtime("Stale heap reference ...")`.

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

TinyLang does not use garbage collection or compile-time borrow checking
in the current TinyLang implementation. The Rust runtime owns the heap
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

All limits are enforced before allocation. Exceeding a limit produces a
`TinyLangError::Runtime` rather than unbounded host allocation.

- **Dynamic array length:** 65,536 elements. `push` beyond the limit or
  `array(count, ...)` with `count` above it fails.
- **Single buffer allocation:** 1,048,576 bytes (1 MiB). `buffer(size)`
  above the limit fails.
- **Total live heap payload:** 4 MiB. Any allocation that would exceed
  the total live bytes fails.
- **Live heap object slots:** 1,000,000 objects. Any allocation at the
  limit fails.
- **Nested TinyLang calls:** 16 calls. `CALL` at the call-depth limit
  fails.

# Shutdown Drain

At program exit, the runtime drains all remaining live heap objects. The
`TinyRunReport` includes:

- `heap_before_shutdown` -- statistics immediately before the drain.
- `heap_after_shutdown` -- statistics after the drain; `shutdown_frees`
  counts objects freed by the drain.

The drain is not triggered by `unsafe free` -- only by runtime
shutdown.
