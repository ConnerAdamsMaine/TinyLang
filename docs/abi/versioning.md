---
title: ABI Versioning
---

# ABI Versioning and Stability

**ABI version 2 uses the `tinylang_*` JSON C entry points.** Consumers must
check `tinylang_abi_version()` against `TINYLANG_ABI_VERSION` (2) before use.
The v1 ABI remains frozen on `release/v1`; this renamed library is not a
drop-in replacement. The experimental `tinylang_v2_*` binary ABI is a
separate interface and is not included in the core crate.

TinyLang will not keep every old implementation or historical language version
available forever. Pin the specific release or source revision you depend on.
Send comments, concerns, and questions to the [TinyLang community
forum](https://tl.404connernotfound.dev).

## What Constitutes a Breaking Change

The following changes break binary or source compatibility for callers:

**Function-level breaks:**
- Removing or renaming an entry point declared in `tinylang.h`
- Changing the type or order of any parameter
- Changing the return type of any entry point

**Response-level breaks:**
- Removing a key from a success `value` object
- Adding a key to a success `value` object
- Changing the type of an existing key in any response shape
- Removing one of the four envelope shapes (`ok/value`, `compile`,
  `runtime`, `panic`)
- Changing the meaning of `"kind"` values

**Bytecode-level breaks:**
- Reordering or removing any opcode in `Op` ordinal positions 1–29
- Reordering or removing any Phase-1 builtin in slots 0–34 of `BUILTINS`
- Changing the JSON artifact `"format"` or `"version"` field values

## What Is Not a Breaking Change

- Adding new entry points to `tinylang.h`
- Adding new opcode ordinals above the frozen Phase-1 range
- Adding new Phase-2 builtin slots above index 34
- Changing internal implementation details with no observable effect on
  inputs or outputs
- Changing error message text within the `"error"` field (do not parse
  error strings)

## Current Stability Status

| Area | Status | Notes |
| --- | --- | --- |
| Function signatures in `tinylang.h` | STABLE | Declared at ABI version 2 |
| Response envelope shape (4 kinds) | STABLE | Frozen now |
| `value` object keys per endpoint | STABLE | Frozen by the committed JSON schema |
| `memory` array encoding | STABLE | Frozen by the committed JSON schema and contract tests |
| Phase-1 opcode ordinals (1–29) | STABLE | Frozen; artifact round-trips depend on them |
| Phase-2 opcode ordinals (30+) | INTERNAL | May change with a future artifact version; not exposed as C declarations |
| Phase-1 builtin slots (0–34) | STABLE | Frozen |
| Phase-2 builtin slots (35+) | INTERNAL | Order may change with a future language/artifact version |
| Artifact `format`/`version` fields | STABLE | `"tinylang-bytecode"` / `1` |

## ABI Version 2 Contract

The following are stable and will not change without a major ABI version bump:

1. All function signatures in `tinylang.h`
2. All four response envelope shapes
3. All `value` object keys for every entry point
4. The `memory` array encoding
5. Phase-1 opcode ordinals and Phase-1 builtin slot order used by artifact version 1

The exact nullability, ownership, threading, panic, and error rules are part
of the contract in [`contract.md`](contract.md) and the generated header.

## Compatibility Guidance

At startup, compare the library result from `tinylang_abi_version()` with the
header constant `TINYLANG_ABI_VERSION`. Refuse to load the library when the
values differ; do not infer compatibility from the package version.

For JSON responses, branch on the documented `ok` and `kind` fields and
validate against the committed schema. ABI version 2 response objects are
closed shapes; unknown fields are not part of this ABI. Do not parse
human-readable `error` text. A `compile`, `runtime`, or `panic` response is an
application-visible failure, not an ABI mismatch.

For bytecode artifacts, accept only `format: "tinylang-bytecode"` and
`version: 1` unless the consumer explicitly supports another format version.
The JSON C API retains artifact layout version 1; Rust artifacts use JSON
layout version 2 or binary layout version 5. Recompile artifacts from the
v1 release line because the format name and binary magic have changed.
ABI version 2 does not promise compatibility for internal Rust layouts, heap
addresses, JIT listing text, or the Rust API.

## Decay Policy

Deprecated features will be marked in `tinylang.h`
with a `// DEPRECATED(vX.Y): reason` comment and kept for at least one
minor version cycle before removal. Removals require a major version
bump.
