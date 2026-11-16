# Tynra

Tynra 2.0 is a compiler, verified bytecode runtime, adaptive JIT, and CLI
for a portable systems programming language.

## Rust API

```rust
use tynra::run_source;

let mut output = Vec::new();
run_source("print 42", "vm", &mut output, Vec::new()).unwrap();
assert_eq!(output, b"42\n");
```

The Cargo package is `tynra-v2`; the Rust library and command-line
executable remain `tynra`. Use `tynra --help` for options.

```toml
[dependencies]
tynra = { package = "tynra-v2", version = "2.2.0" }
```

## C integration

This crate includes `include/tynra.h` and
`tynra-response-schema.json`. Link against `tynra.dll`,
`libtynra.so`, or `libtynra.dylib`. Check `tynra_abi_version()`
against `TYNRA_ABI_VERSION` (2) before using the JSON C entry points.

Execution through the C API requires the `tynra-sandbox-worker`
executable beside the host executable, or an explicit path in
`TYNRA_SANDBOX_WORKER`. A full Cargo build produces it; distributors
must ship it alongside the library. The experimental binary ABI and SDKs
are separate and are not included in this package.

## Version 2 migration

Use `tynra` Rust imports, `TynraError`, `tynra_*` C entry points,
and `TYNRA_*` environment variables. Project module mappings use
`tynra.json`. Recompile source and re-sign module metadata when moving
from the v1 release line: JSON artifacts use `tynra-bytecode`, binary
artifacts start with the eight-byte `TYNRA\0\0\0` magic, and module signing
uses language version 2. Artifact layout versions are independent of
package and ABI versions.

## Publication status

This source uses the `tynra-v2` Cargo package name at version 2.2.0. Package
verification does not prove upload authorization or publish a registry
release; registry publication requires a separate approval.

See the [project documentation](https://github.com/ConnerAdamsMaine/TinyOne/tree/main/docs)
and [publication checklist](https://github.com/ConnerAdamsMaine/TinyOne/blob/main/docs/CI_CD.md#cratesio-publication).

## License

See `LICENSE` for the MIT license.
