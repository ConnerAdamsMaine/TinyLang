---
title: FFI Reference
---

# FFI Integration

TinyLang compiles to a shared library (`libtinylang.so` / `.dylib` / `.dll`)
and a Rust crate. This section covers how to integrate TinyLang into a host
application from both C and Rust.

For the ABI contract and JSON schema reference, see [`abi/`](../abi/INDEX.md).

## Documents in This Area

| File                                   | Description                                                                                                                        |
| -------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| [`c-integration.md`](c-integration.md) | Building, linking, and calling TinyLang from C or C++; entry-point reference; ownership and threading rules; complete code examples |
| [`rust-api.md`](rust-api.md)           | Rust crate public API: compilation, execution, JIT, artifact I/O, and verification functions                                       |

## ABI Drift Tooling

The generated C header is `crates/tinylang_core/include/tinylang.h`. It is generated from the
Rust FFI source when `cbindgen` is available, while exported C symbols keep the
existing `tinylang_*` names.

Use the no-dependency drift check before changing `crates/tinylang_core/src/ffi.rs`
or its generated header:

```sh
./scripts/check_abi_drift.sh
```

For review artifacts, emit a deterministic symbol manifest:

```sh
python3 tools/abi_manifest.py manifest
```

If `cbindgen` is installed, regenerate the canonical packaged header:

```sh
python3 tools/abi_manifest.py generate-header
```

When `cbindgen` is not on `PATH`, generation fails with a clear message and the
`check`/`manifest` commands continue to work without network installs.
