# TinyLang

TinyLang 2.0 is a compiler, verified bytecode runtime, adaptive JIT, and CLI
for a portable systems programming language.

## Rust API

```rust
use tinylang::run_source;

let mut output = Vec::new();
run_source("print 42", "vm", &mut output, Vec::new()).unwrap();
assert_eq!(output, b"42\n");
```

The Cargo package is `tinylang-v2`; the Rust library and command-line
executable remain `tinylang`. Use `tinylang --help` for options.

```toml
[dependencies]
tinylang = { package = "tinylang-v2", version = "2.0.0" }
```

## C integration

This crate includes `include/tinylang.h` and
`tinylang-response-schema.json`. Link against `tinylang.dll`,
`libtinylang.so`, or `libtinylang.dylib`. Check `tinylang_abi_version()`
against `TINYLANG_ABI_VERSION` (2) before using the JSON C entry points.

Execution through the C API requires the `tinylang-sandbox-worker`
executable beside the host executable, or an explicit path in
`TINYLANG_SANDBOX_WORKER`. A full Cargo build produces it; distributors
must ship it alongside the library. The experimental binary ABI and SDKs
are separate and are not included in this package.

## Version 2 migration

Use `tinylang` Rust imports, `TinyLangError`, `tinylang_*` C entry points,
and `TINYLANG_*` environment variables. Project module mappings use
`tinylang.json`. Recompile source and re-sign module metadata when moving
from the v1 release line: JSON artifacts use `tinylang-bytecode`, binary
artifacts start with the eight-byte `TINYLANG` magic, and module signing
uses language version 2. Artifact layout versions are independent of
package and ABI versions.

## Publication status

This source uses the `tinylang-v2` Cargo package name at version 2.0.0.
The unrelated crates.io package `tinylang` is not this project. Package
verification does not prove upload authorization or publish a registry
release; registry publication requires a separate approval.

See the [project documentation](https://github.com/ConnerAdamsMaine/TinyOne/tree/main/docs)
and [publication checklist](https://github.com/ConnerAdamsMaine/TinyOne/blob/main/docs/CI_CD.md#cratesio-publication).

## License

See `LICENSE`. Non-commercial use is permitted under the supplied custom
license; commercial use requires a separate license.
