# TinyLang v2 binary artifact format

Status: experimental v2.0 profile 0  
Transport opcode: [`PROGRAM_LOAD`](v2-opcodes.md#0x1000--program_load)

This document specifies `artifact_format = 1` only. Format 2 separately adapts
the current compiler's `TINYLANG` version-5 artifact through its existing
untrusted verifier (8 MiB maximum). Format 2 is experimental compiler
interoperability, not a new TLBC profile or a promise of cross-version compiler
artifact compatibility. The decoder is selected explicitly by the opcode
record; neither format falls back to the other.

## Purpose

The `TLBC` format is a deterministic, sectioned bytecode container. It is not
the frozen v1 JSON artifact and must never be parsed through `serde_json`.
Every multibyte field is little-endian. Parsers use byte offsets and checked
arithmetic; native structs are not mapped onto input bytes.

Profile 0 deliberately supports an executable main chunk only. It is a real
TinyLang `Program` verified by `BytecodeVerifier`, but has no strings,
functions, structs, modules, fields, or enum metadata. Later profiles may add
those pools as new canonical sections without changing profile 0.

## Container header

The fixed header is 32 bytes:

| Offset | Size | Type | Field | Profile-0 rule |
| ---: | ---: | --- | --- | --- |
| `0x00` | 4 | bytes | magic | ASCII `TLBC`. |
| `0x04` | 2 | u16 | major | 2. |
| `0x06` | 2 | u16 | minor | 0. |
| `0x08` | 2 | u16 | header_bytes | 32. |
| `0x0a` | 2 | u16 | directory_entry_bytes | 24. |
| `0x0c` | 4 | u32 | total_bytes | Exact container length. |
| `0x10` | 2 | u16 | section_count | Exactly 2. |
| `0x12` | 2 | u16 | flags | Zero. |
| `0x14` | 4 | u32 | reserved | Zero. |
| `0x18` | 8 | u64 | reserved | Zero. |

The section directory immediately follows the header. Directory entries are
sorted by section kind and are 24 bytes each:

| Offset | Size | Type | Field |
| ---: | ---: | --- | --- |
| `0x00` | 2 | u16 | section_kind |
| `0x02` | 2 | u16 | flags, zero |
| `0x04` | 4 | u32 | byte_offset |
| `0x08` | 4 | u32 | byte_length |
| `0x0c` | 4 | u32 | item_count |
| `0x10` | 8 | u64 | reserved, zero |

Profile 0 requires exactly these entries:

| Kind | Name | Offset | Length | Count |
| ---: | --- | ---: | ---: | ---: |
| 1 | Metadata | 80 | 16 | 1 |
| 2 | Main code | 96 | `instruction_count * 24` | instruction count |

Sections are contiguous, eight-byte aligned, non-overlapping, and consume the
entire file. There are no ignored gaps, trailers, duplicate kinds, or unknown
profile-0 sections.

## Metadata section

| Offset | Size | Type | Field | Rule |
| ---: | ---: | --- | --- | --- |
| `0x00` | 4 | u32 | profile | Zero. |
| `0x04` | 4 | u32 | slot_count | At most the VM verifier limit. |
| `0x08` | 8 | u64 | reserved | Zero. |

## Main-code section

Each instruction is exactly 24 bytes:

| Offset | Size | Type | Field |
| ---: | ---: | --- | --- |
| `0x00` | 2 | u16 | opcode ordinal |
| `0x02` | 2 | u16 | instruction flags, zero |
| `0x04` | 4 | u32 | reserved, zero |
| `0x08` | 8 | i64 | argument 1 |
| `0x10` | 8 | i64 | argument 2 |

The main section contains 1–262,144 instructions and must pass the normal
TinyLang bytecode verifier. In particular, it must end in `HALT`; jumps, stack
depths, indices, arities, and operands must be valid.

Stable opcode ordinals are:

```text
 1 PUSH_INT       10 LT             19 RETURN         28 BUILTIN
 2 LOAD           11 LTE            20 HALT           29 PUSH_NULL
 3 STORE          12 GT             21 PUSH_STRING    30 POP
 4 ADD            13 GTE            22 MAKE_ARRAY     31 LOAD_GLOBAL
 5 SUB            14 EQ             23 INDEX          32 MAKE_ENUM
 6 MUL            15 NE             24 SET_INDEX      33 PUSH_BOOL
 7 DIV            16 JUMP           25 MAKE_STRUCT    34 PUSH_FLOAT
 8 NEG            17 JUMP_IF_ZERO   26 GET_FIELD      35 PUSH_FUNCTION
 9 PRINT          18 CALL           27 SET_FIELD      36 CALL_VALUE
```

Profile 0 has empty metadata pools. Instructions that reference strings,
functions, structs, fields, modules, or enum variants therefore fail ordinary
bytecode verification. Their opcodes remain assigned so later profiles use the
same instruction representation.

## Loader behavior

`PROGRAM_LOAD` first checks response capacity, then validates the complete
container, constructs `Program::new(main_code, slot_count)`, and calls
`VerifiedProgram::verify`. Only after verification succeeds is the program
inserted into the runtime's Program-handle table.

Container, opcode, or verifier failure produces a response with
`TINY_STATUS_INVALID_ARTIFACT`; no handle is inserted. Registry exhaustion
produces `TINY_STATUS_OUT_OF_MEMORY`. Success returns one 16-byte Handle record
and transfers one host-owned reference, releasable through `HANDLE_RELEASE`.
