# TinyLang v2 runtime memory model

Status: **existing model verified in focused tests; v2 automatic RC blocked**.

[The current memory model](../../memory-model.md) remains authoritative.
VM locals are fixed Ralloc-owned, 64-byte encoded slots initialized to
`I64(0)`. The operand stack is separate. Heap aliases use runtime ownership
and stale-reference checks; the language still exposes explicit unsafe
free operations. No new tracing garbage collector or automatic language
retain/release pass is introduced.

The scalar profile cannot allocate language heap values. It owns bounded
zeroed machine-word local and operand buffers for each invocation. Typed
eligibility preserves I64/Bool distinctions despite the common machine width.
These are Rust-owned scalar buffers, not a new VM slab allocator. Executable storage
is owned by the prepared native function and must not outlive its executable
mapping. Generated code may access only validated frame indices.

Existing persistent ABI sessions continue to own their VM/program and enforce
heap, object, call-depth and instruction limits. An invocation error does not
pretend a value was returned. Existing handle domains and lifetime checks are
preserved.

Executed acceptance: 3 fragmentation, 3 initialization and 10 native-owner
allocator tests passed. They exercise reuse, initialized growth/padding,
ownership handoff, exact destruction, alignment, zero-sized owners and thread
movement. The 6 persistent-VM and 11 cache-retention core tests also passed.
Both native Windows memory paths were checked for RW-to-RX publication; the
Cranelift lifetime test also verified unmapping on drop. These tests are not
evidence of a new slab allocator, cycle collection or
compile-time reference-count optimization.
Optional LLVM object-execution tests check the published code pages are RX
and keep MCJIT ownership alive during calls. They do not claim sanitizer or
cross-platform memory certification.

Remaining: agreed automatic ownership semantics, retain/release insertion
and proof-based elimination, cycles, native resource lifetime rules, FFI
heap-value conversion, slab size classes and sanitizer/fault-injection
certification for the new engines.
