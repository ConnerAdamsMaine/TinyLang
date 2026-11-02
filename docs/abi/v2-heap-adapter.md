# Rust heap adapter for invocation conversion

`VM::import_heap_value`, `VM::copy_heap_value` and `VM::release_heap_value`
provide narrow access to the existing interpreter heap. `VmHeapValue` carries
owned strings, byte buffers and arrays of `RuntimeValue`. This is a Rust
integration API for the argument-conversion candidates, not a stable C layout
and not a new allocator or ownership system.

Import forwards to existing allocation methods and therefore uses the VM's
heap byte/object limits. Copy produces host-owned storage with fallible
reservation, so no borrowed heap storage or lock escapes. Array copies are
shallow: nested heap references remain VM-local and require separate copying
if a foreign response needs a recursive value representation.

Release invokes the language heap's existing explicit free operation. It
invalidates every alias to that object and does not recursively free children.
Consequently, releasing a foreign retained handle must not blindly free the
language object while the VM still owns aliases. The eventual FFI ownership
policy must distinguish handle lifetime from underlying object lifetime.

All `RuntimeValue` heap references passed to these methods must originate from
the same VM. Existing index/generation validation rejects stale references but
does not establish cross-VM provenance. This low-level adapter must never be
used to decode a foreign integer as a heap reference; v2 handles must carry
and validate VM ownership before calling it. Native heap addresses, record
layouts and encoded internal array slots are not exposed by this adapter.

The forwarding methods are mechanically necessary compatibility work shared
by the three invocation candidates. They do not select tagged, packed or
stack-based interchange, and no performance advantage is claimed. Structs,
enums and other heap variants still need explicit conversion support.

Three tests in `crates/tinylang_core/tests/vm_interchange.rs` cover:

- Identity-function arguments/results for UTF-8 strings with embedded NULs,
  arbitrary bytes, signed integer arrays and empty containers.
- Shallow nested references, stale children, wrong-type access and repeated
  release rejection.
- Heap byte/object caps, capacity after failed import and generation checks
  after a freed slot is reused.

The tests pass in the normal and release test profiles. Existing 6 persistent-VM, 4 fuel,
59 runtime-parity and 42 v1 ABI tests also pass; core library Clippy passes
with warnings denied. All 33 v2 release tests also pass (one manual benchmark
is ignored by default). Allocation-failure injection and a full foreign call
using heap arguments are not yet covered.
