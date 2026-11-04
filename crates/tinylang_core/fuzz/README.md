# TinyLang v2 ABI fuzzing

The `frame` target sends arbitrary bytes through the exported raw C ABI. It
therefore exercises descriptor validation, frame parsing, value parsing,
response sizing, panic containment, and runtime-handle validation together.
The `artifact` target wraps arbitrary bytes in a canonical `PROGRAM_LOAD`
request and exercises the `TLBC` container decoder and bytecode verifier.

Install `cargo-fuzz` and use the committed golden vectors as initial corpus:

```powershell
cargo install cargo-fuzz
cargo +nightly fuzz run frame crates\tinylang_core\tests\vectors `
  --fuzz-dir crates\tinylang_core\fuzz

cargo +nightly fuzz run artifact crates\tinylang_core\tests\artifacts `
  --fuzz-dir crates\tinylang_core\fuzz
```

Useful bounded CI or pre-commit run:

```powershell
cargo +nightly fuzz run frame crates\tinylang_core\tests\vectors `
  --fuzz-dir crates\tinylang_core\fuzz -- -max_total_time=60
```

Any crashing input should be minimized and committed as an additional golden
invalid vector before fixing the parser.
