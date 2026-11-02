# TinyLang v2 VM opcode schemas

Status: experimental v2.0 draft  
Wire envelope: [`v2-binary-spec.md`](v2-binary-spec.md)

This document defines the experimental v2 VM opcodes and their request,
response, ownership, capacity, and side-effect rules. `PROGRAM_LOAD`,
`VM_CREATE`, `VM_INVOKE`, and `HANDLE_RELEASE` have reference-runtime
implementations. `VM_INVOKE` currently supports canonical scalar arguments
and results; heap values, handles, aggregates, and unit results remain
unsupported.

## Common rules

Opcode payloads are ordered top-level value records. Record order and exact
tag are part of the schema; bindings must not reorder, omit, or coerce values.
Unknown trailing records are rejected in schema minor version 0.

Every handle is checked for runtime ownership, object kind, liveness, and
generation before VM access. A handle value of zero is never valid. Program,
VM, and heap-object handles share the wire representation but are distinct
runtime kinds.

Response capacity is validated before any externally observable mutation:

- fixed-size operations require capacity for their complete response;
- `VM_INVOKE` includes `max_result_bytes`, and the host response buffer must
  have room for the 32-byte frame header plus that bound before execution;
- serialization may use fewer bytes than the declared bound;
- exceeding the declared result bound produces a bounded `VM_TRAP` response,
  not `BUFFER_TOO_SMALL` and not a partially encoded value;
- after dispatch begins, `BUFFER_TOO_SMALL` is forbidden.

This rule prevents retrying a side-effecting invocation merely to resize an
output buffer.

## `0x1000` — PROGRAM_LOAD

Loads and verifies a deterministic TinyLang binary artifact. Source text and
JSON artifacts are not accepted by this opcode. Format 1 is
[artifact profile 0](v2-artifact-spec.md), an executable main-code-only
program with no external metadata pools. Format 2 is the existing compiler's
`TINYLANG` binary artifact, decoded by its untrusted verified loader. It supports
the compiler's functions and metadata pools without granting serialized host
permissions. Format 2 currently requires compiler artifact version 5 and at
most 8 MiB; its compiler-specific version is independent of the TLV2 frame.

Request records:

| Index | Tag | Name | Constraint |
| ---: | --- | --- | --- |
| 0 | U32 | `artifact_format` | `1` = TLBC v2; `2` = compiler binary artifact. Other values are invalid. |
| 1 | Bytes | `artifact` | 1–16,777,216 bytes; format-specific verifier input. |

Success response:

| Index | Tag | Name | Meaning |
| ---: | --- | --- | --- |
| 0 | Handle | `program` | Runtime-owned immutable verified-program handle. |

The output size is fixed at 48 bytes: a 32-byte frame plus one 16-byte Handle
record. Capacity is checked before artifact verification or handle insertion.
Verification failure returns `INVALID_FRAME` for malformed transport values or
a nonzero response status with domain 2 for an invalid program artifact.
The explicit format selects the decoder: there is no magic-based fallback.
Unknown compiler versions, trailing data and malformed compiler artifacts
produce `INVALID_ARTIFACT`. Accepting format 2 does not freeze the compiler's
internal artifact schema as a portable v2 format. Clients must generate a
supported compiler artifact version; feature discovery remains unfinished.

Ownership: the returned program handle is owned by the runtime and remains
live until `HANDLE_RELEASE`. Creating a VM retains its own internal reference;
releasing the host's program handle does not invalidate an already-created VM.

## `0x1001` — VM_CREATE

Creates a VM from a verified program and explicit resource limits.

Request records:

| Index | Tag | Name | Constraint |
| ---: | --- | --- | --- |
| 0 | Handle | `program` | Live Program handle belonging to this runtime. |
| 1 | U64 | `max_heap_bytes` | 1–4,194,304 bytes. |
| 2 | U32 | `max_heap_objects` | 1–1,000,000 objects. |
| 3 | U32 | `max_call_depth` | 1–16 frames. |
| 4 | U32 | `vm_flags` | Bit 0 permits JIT; all other bits zero in v2.0. |

Success response:

| Index | Tag | Name | Meaning |
| ---: | --- | --- | --- |
| 0 | Handle | `vm` | Mutable VM handle bound to the supplied limits. |

The output size is fixed at 48 bytes and is checked before session insertion.
The v2 session retains an immutable reference to the verified program and its
requested limits. Releasing the original Program handle therefore does not
invalidate the VM handle. Requests above the runtime caps return
`INVALID_ARGUMENT` in the response; malformed zero limits remain
`INVALID_FRAME` boundary failures. Failure is atomic: no handle or live VM
remains registered.

The implementation allocates global execution memory and a persistent VM at
creation, without executing main code. Requested heap-byte/object limits and
call-depth limits are installed in that VM; the program's own call-depth cap
can only be tightened. The Rust runtime supports non-consuming, fuel-bounded
main/function calls. The v2 dispatcher serializes access with an exclusive
session lease and executes functions in the existing interpreter.

## `0x1002` — VM_INVOKE

Invokes one function on an existing VM.

`export_id` is the zero-based function-table index in the loaded artifact.
Unqualified root-program functions are callable through the host boundary.
Qualified module functions are callable only when their owning module
explicitly exports the local function name. This ID mapping is specific to
the artifact's function table.

The current value adapter supports `NULL`, `BOOL`, signed and unsigned integer
records, and `F32`/`F64` records. It preserves each input tag without numeric
coercion. Heap values, handles, enums, slices, sequences, and other non-scalar
arguments return `INVALID_ARGUMENT`; unsupported result values return the same
status after language execution. Function arity is checked before execution.
The VM's dynamic runtime semantics determine whether the accepted values are
valid for the function body. Interpreter output is currently discarded by the
reference dispatcher; it is not returned in the ABI response.

Request records:

| Index | Tag | Name | Constraint |
| ---: | --- | --- | --- |
| 0 | Handle | `vm` | Live VM handle belonging to this runtime. |
| 1 | U32 | `export_id` | Stable numeric export identifier from the artifact. |
| 2 | U64 | `fuel` | Nonzero instruction budget for this invocation. |
| 3 | U32 | `max_result_bytes` | 0–16,777,216; includes result records and padding. |
| 4 | Sequence | `arguments` | Auxiliary count and exactly that many canonical records. |

Success response payload contains zero or one top-level scalar result value
record. Argument count and supported scalar tags are validated before
entering the VM. Since v2 does not yet carry static parameter/result type
signatures for these exports, runtime type failures are reported as
`VM_TRAP`.

Before execution, the raw call verifies that response capacity is at least
`32 + max_result_bytes`. The serializer writes at most `max_result_bytes` and
sets the response header to the actual encoded length. A larger host buffer is
permitted. A result that cannot fit the declared bound returns a bounded
domain-2 `VM_TRAP` response. The invocation is not retried. Error responses
include the 16-byte status detail when `max_result_bytes` is at least 16
bytes; with a smaller bound they contain only the 32-byte response header.
`BUFFER_TOO_SMALL` is returned only during preflight and never after execution
starts.

VM language side effects commit according to TinyLang execution semantics even
when execution traps. Boundary failures discovered before execution have no VM
side effects. A dispatcher must document whether a specific host-call facility
has effects outside the VM; the base v2 ABI provides no automatic rollback of
external I/O.

## `0x1003` — HANDLE_RELEASE

Releases one host-owned reference.

Request records:

| Index | Tag | Name | Constraint |
| ---: | --- | --- | --- |
| 0 | Handle | `handle` | Live handle belonging to this runtime. |

Success response is empty and therefore has an actual length of 32 bytes. The
operation conservatively requires 48 bytes of output capacity before dispatch,
so either the success frame or a 16-byte status-detail payload always fits.
On `BUFFER_TOO_SMALL`, `out_written` is 48 and the reference is untouched. On
success, `out_written` is 32. Capacity and handle validity are checked before
the reference count is changed. Releasing an already released or generation-
mismatched handle returns `STALE_HANDLE`; it never reaches the VM allocator.

Release is kind-independent at the wire level. The registry dispatches the
correct destructor by its stored handle kind. Runtime destruction remains
`BUSY` while host-owned child handles or owned transfer buffers are live.

## Dispatch implementation gate

An implementation may advertise these opcodes only after it has tests proving:

1. exact schema rejection for missing, reordered, mistagged, and trailing data;
2. stale, cross-runtime, and wrong-kind handle rejection;
3. capacity preflight before every side effect;
4. atomic handle insertion and cleanup on failure;
5. fuel, heap, object-count, call-depth, and result-size enforcement;
6. deterministic response bytes for equivalent VM results.
