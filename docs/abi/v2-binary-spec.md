# TinyLang v2 deterministic binary ABI

Status: experimental v2.0 draft  
Canonical header: `tinylang_v2/include/tinylang_v2.h`

## 1. Scope and invariants

The protocol transports exactly one request or response frame per raw ABI
call. It uses no native C layout on the wire. Every multibyte integer and every
IEEE-754 bit pattern is little-endian, independent of host endianness.

Implementations must parse fields from their documented byte offsets. Casting
wire bytes to a C struct, Rust `repr(C)` struct, or packed struct is forbidden.
All additions, multiplications, integer conversions, and pointer-range checks
must be checked before a slice is formed or VM state is accessed.

The v2.0 implementation limit is 67,108,864 bytes per complete frame. A host
may enforce a smaller documented limit.

## 2. Frame header

```text
     0                   1                   2                   3
     0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
    +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
00  |                    magic = ASCII "TLV2"                       |
    +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
04  | schema_major | schema_minor | header_length |     opcode      |
    +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
0C  |                            flags                              |
    +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
10  |                            status                             |
    +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
14  |                        payload_length                         |
    +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
18  |                                                               |
    +                        correlation_id                         +
1C  |                                                               |
    +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
```

| Offset | Size | Type | Field | Rule |
| ---: | ---: | --- | --- | --- |
| `0x00` | 4 | bytes | magic | Exactly `54 4c 56 32` (`TLV2`). |
| `0x04` | 2 | u16 | schema_major | Exactly 2 for this specification. |
| `0x06` | 2 | u16 | schema_minor | Decoder accepts a supported minor version only. |
| `0x08` | 2 | u16 | header_length | Exactly 32 in v2.0. |
| `0x0a` | 2 | u16 | opcode | Operation identifier. |
| `0x0c` | 4 | u32 | flags | Bit 0 is `RESPONSE`; remaining bits must be zero. |
| `0x10` | 4 | u32 | status | Zero in requests; response result in responses. |
| `0x14` | 4 | u32 | payload_length | Bytes following the header, excluding the header. |
| `0x18` | 8 | u64 | correlation_id | Opaque host value copied into the response. |

The complete frame length is exactly `header_length + payload_length`. There
is no trailing frame padding and no concatenation of frames inside one call.
A request has `flags == 0` and `status == 0`. A response has flag bit 0 set and
mirrors the request's opcode and correlation ID.

## 3. Status model

The C function result reports whether the boundary operation itself completed.
`TINY_STATUS_OK` means a syntactically valid response frame was written. The
response header's status field reports protocol, application, or VM failure.
Consequently an unknown but well-formed opcode produces a response frame whose
status is `TINY_STATUS_UNKNOWN_OPCODE`, while the C function returns OK.

Boundary failures that prevent a trustworthy response—invalid pointers,
overlap, an invalid frame, an invalid runtime handle, or insufficient output
capacity—are returned directly. No response is produced for them.

| Code | Name | Meaning |
| ---: | --- | --- |
| 0 | `OK` | Success. |
| 1 | `INVALID_ARGUMENT` | Invalid ABI descriptor, length, pointer rule, or overlap. |
| 2 | `INVALID_FRAME` | Malformed, truncated, noncanonical, or overlong wire data. |
| 3 | `UNSUPPORTED_VERSION` | Unsupported schema major or minor. |
| 4 | `UNKNOWN_OPCODE` | Opcode is not implemented. |
| 5 | `BUFFER_TOO_SMALL` | Output is unchanged; `out_written` is the minimum safe dispatch capacity. |
| 6 | `INVALID_HANDLE` | Zero, malformed, inactive, or foreign handle. |
| 7 | `STALE_HANDLE` | Slot exists but its generation does not match. |
| 8 | `OUT_OF_MEMORY` | Runtime or optional pool capacity is exhausted. |
| 9 | `VM_TRAP` | TinyLang execution trapped. |
| 10 | `INTERNAL` | Runtime invariant or synchronization failure. |
| 11 | `PANIC` | A Rust panic was caught before crossing the C ABI. |
| 12 | `BUSY` | Resource has outstanding children, such as owned buffers. |
| 13 | `INVALID_ARTIFACT` | Binary artifact container or verified bytecode is invalid. |

When a response status is nonzero, its payload begins with this fixed detail:

```text
offset  size  field
0x00      2   domain          (1 = protocol, 2 = VM, 3 = host integration)
0x02      2   reserved        (zero)
0x04      4   detail_code
0x08      8   fault_offset    (UINT64_MAX when no byte offset applies)
0x10      N   optional opcode-defined detail
```

## 4. Value records

Successful value payloads contain zero or more records. Each record begins on
an eight-byte boundary relative to the containing payload.

```text
offset  size  field
0x00      2   value_tag
0x02      2   value_flags       (zero in v2.0)
0x04      4   value_length      (value bytes only)
0x08      8   auxiliary         (tag-specific)
0x10      N   value bytes
          P   zero bytes until the next eight-byte boundary
```

`value_length` excludes both the 16-byte value header and padding. All padding
must be zero. Unknown tags or flags are rejected rather than ignored.

| Tag | Name | Value bytes | Auxiliary |
| ---: | --- | --- | --- |
| 0 | Null | none | zero |
| 1 | Bool | one byte, exactly 0 or 1 | zero |
| 2–5 | u8/u16/u32/u64 | fixed-width LE integer | zero |
| 6–9 | i8/i16/i32/i64 | fixed-width two's-complement LE integer | zero |
| 10–11 | f32/f64 | IEEE-754 binary bits in LE order | zero |
| 12 | Bytes | uninterpreted bytes | zero |
| 13 | UTF-8 | shortest-form valid UTF-8, no terminator | zero |
| 14 | Handle | none | nonzero opaque 64-bit handle |
| 15 | Enum | u32 discriminant, zero u32 reserved, then zero or one value record | schema/type ID |
| 16 | Slice | u64 element count followed by packed elements | fixed-width element tag |
| 17 | Sequence | concatenated value records | record count |

Floating-point values preserve their exact IEEE bit patterns. Protocol
serialization itself performs no textual conversion. Language bindings that
cannot preserve NaN payloads must use the canonical quiet NaNs `0x7fc00000`
and `0x7ff8000000000000` when encoding newly created NaNs.

Slice elements may be Bool, integer, float, or Handle. Their element width is
implied by the auxiliary tag; no per-element padding is present. A sequence is
used for variable-width or heterogeneous values. Nesting is bounded to 32.

### Binding requirements

- C and C++ use the fixed-width declarations in `tinylang_v2.h`; C++ callers
  receive the same `extern "C"` symbols and must not allow exceptions across
  them.
- Python bindings use `ctypes.c_uint64` (or an equivalent CFFI type) for both
  handle kinds and keep the backing request and response objects alive for the
  entire raw call.
- JavaScript bindings represent handles and other u64 fields as `BigInt` and
  use `DataView` operations with `littleEndian=true`. A Node `Buffer` or typed
  array view must preserve its byte offset when converted to an `ArrayBuffer`
  view.
- Bindings may copy wire bytes into language-managed storage, but neither the
  runtime nor a binding may retain a borrowed C buffer after the call returns.

Reference C, Python, and Node implementations live under
`tinylang_v2/bindings/` and are checked against the committed golden vectors.

## 5. Handles and provenance

No VM pointer appears on the wire. `TinyHandle` and `TinyRuntimeHandle` are
opaque nonzero u64 values. The reference implementation divides handles into a
slot component and generation component, but consumers must not inspect them.
Destroying an object invalidates its generation, allowing stale references to
be rejected before VM heap access.

Runtime and object allocations occupy disjoint internal handle domains.
Passing a runtime value to an object operation, or an object value to a runtime
operation, returns `INVALID_HANDLE` without releasing either resource. The
current reference implementation reserves bit 31 of the low slot component for
objects; runtime values leave it clear. The high 32-bit generation component is
unchanged. This encoding remains private, not a client bitfield contract.

Compatibility: domain separation changes generated object numbers, not frame
layouts, handle width, C declarations, symbols or the ABI version. Earlier
untagged object numbers are no longer accepted as object handles. Handles are
live, process-local references: clients must recreate runtimes and objects when
replacing or reloading the library, and must not construct, persist or transfer
handles between library instances. Runtime numeric encoding is unchanged.

Runtime and object slots whose generation reaches `u32::MAX` are permanently
retired on release. Generations never wrap to an earlier identity. Retired
slots are unavailable for allocation; if no usable slots remain, creation
returns `OUT_OF_MEMORY`. This preserves the opaque u64 representation and
trades eventual capacity exhaustion for deterministic stale-handle rejection.

Raw pointers exist only in C ABI buffer descriptors. For every call:

- a null pointer is valid only with a zero length or capacity;
- request, response, and output-length storage must not overlap;
- the runtime borrows host buffers only for the call and never retains them;
- the host must not mutate input or concurrently access output during the call;
- byte buffers require no alignment, but the pointed-to allocation must be live;
- lengths are checked against `usize`, `isize`, the protocol limit, and range
  overflow before Rust slices are formed.

## 6. Ownership and allocation

`tinylang_v2_call` uses host-owned request and response allocations. Envelope
parsing and response serialization allocate no transfer buffers. Opcode
execution may allocate explicitly runtime-owned state (for example, a verified
Program) only after response capacity preflight. If response capacity is
insufficient, the function returns `BUFFER_TOO_SMALL`, sets `out_written` to
the minimum safe dispatch capacity, leaves response bytes unchanged, and
performs no opcode side effect. This capacity is exact for non-side-effecting
operations. A side-effecting operation may require a conservative bound large
enough for either its success or error response; on success, `out_written` is
always the actual encoded frame length.

Hosts that prefer runtime-owned transfer storage may explicitly call
`tinylang_v2_buffer_acquire`. Such buffers are zero-initialized, tied to one
runtime and owner token, and must be returned exactly once using
`tinylang_v2_buffer_release`. A runtime cannot be destroyed while one of its
buffers remains outstanding. A copied or already released descriptor is stale
and must not be dereferenced or released again.

VM heap objects remain VM-owned. They cross the boundary only as generation-
checked handles and are released through opcode-defined handle operations, not
through a host allocator or C `free`.

## 7. Initial control opcodes

| Opcode | Name | Request | Response |
| ---: | --- | --- | --- |
| `0x0000` | NOP | empty | empty |
| `0x0001` | ABI_INFO | empty | one u32 value containing `major << 16 | minor` |
| `0x0002` | ECHO | canonical value records | identical value records |

The first reserved VM opcodes are `PROGRAM_LOAD` (`0x1000`), `VM_CREATE`
(`0x1001`), `VM_INVOKE` (`0x1002`), and `HANDLE_RELEASE` (`0x1003`). Their
normative payload schemas and side-effect rules are defined in
[`v2-opcodes.md`](v2-opcodes.md). The reference dispatcher implements
`PROGRAM_LOAD`, `VM_CREATE`, and `HANDLE_RELEASE`; `VM_INVOKE` remains declared
but unimplemented. Until a dispatcher implements an opcode, it must return a
response carrying `UNKNOWN_OPCODE`; declaration alone never implies
implementation. Remaining stable VM opcodes occupy `0x1004`–`0x7fff`.
Experimental opcodes occupy `0x8000`–`0xffff` and must not be emitted by stable
bindings.

## 8. Required decoder order

1. Validate ABI descriptors, numeric conversions, and non-overlap.
2. Validate runtime handle generation.
3. Check the 32-byte minimum and frame limit.
4. Check magic, version, header length, request flags, and request status.
5. Checked-add header and payload lengths and require an exact input length.
6. Validate every value record, reserved byte, UTF-8 sequence, count, nesting
   limit, and zero-padding byte.
7. Determine response size and verify output capacity.
8. Only then dispatch an opcode or expose values to the VM.

No panic, C++ exception, Rust reference, allocator object, or borrowed pointer
may cross the ABI boundary.
