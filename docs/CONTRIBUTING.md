---
title: Contributing
---

# Contributing to TinyLang

## Prerequisites

- Rust toolchain with Cargo (stable, edition 2024)
- A C compiler (`cc`) for the FFI smoke test
- `uv` for the Python tooling used by `tools-test` and the full release gate
- Python 3 for `tools/hash.py` (optional; used for release manifests)
- **cargo-make** (`cargo install cargo-make`) for multi-step build workflows
- **taplo** (`cargo install taplo-cli`) for TOML formatting (invoked by `cargo make format`)

## Branches and Release Lines

TinyLang v1 is in maintenance while v2 is developed. The support window is
listed under [Release Lines](../README.md#release-lines).

| Branch | Line | Open PRs here for |
|--------|------|-------------------|
| `main` | TinyLang v2 development | New features, language/runtime/JIT changes, the v2 ABI and SDKs, `crates/tinylang_core/` |
| `release/v1` | TinyLang v1 maintenance | Bug fixes, security fixes, v1 ABI-conformance fixes, documentation corrections |

Rules for `release/v1`:

- No new syntax, opcodes, builtins, artifact format changes, or exported FFI
  symbols. That branch's published header and response schema stay frozen,
  and `tools/abi_manifest.py check` must pass. Do not backport the v2 rename.
- Fix a v1 defect on `release/v1` first, then forward-port it to `main` with
  `git cherry-pick -x <commit>` when `main` has the same defect. Open a
  separate PR instead when the code has diverged too far to cherry-pick.
- Never merge `main` into `release/v1` or `release/v1` into `main`; move
  changes between lines only by cherry-pick or a dedicated PR.
- Tag v1 releases (`v1.x.y`) only from `release/v1`.

Work on both lines side by side with a Git worktree instead of switching
branches in one checkout. Each worktree keeps its own `target/` directory, so
neither line rebuilds the other's artifacts:

```sh
# From the primary checkout on main
git fetch origin
git worktree add ../TinyLang-v1 release/v1
```

Remove it with `git worktree remove ../TinyLang-v1` once its work is pushed.

## Build System

This repo uses three layers of build tooling:

| Layer | Command | What it does |
|-------|---------|--------------|
| Raw cargo | `cargo build`, `cargo test`, `cargo check` | Operates on default workspace members only |
| Cargo aliases | `cargo b`, `cargo t`, `cargo c`, `cargo f`, `cargo r` | Thin wrappers with `--workspace` / `--bin` flags |
| xtask | `cargo xtask <task>`, `cargo gate` | Multi-step orchestration via `crates/xtask/` |
| cargo-make | `cargo make <task>`, `makers <task>` | Complex workflows with dependency chains, formatting, dist |

For day-to-day development, `cargo c` (check) and `cargo t` (test) are the fastest.
For CI-style verification, use `cargo gate` (xtask) or `cargo make A` (makers).

**Note:** The `cargo make format` task formats TOML files using `taplo` by default.
If you don't have taplo installed and can't install it, set `NO_TAPLO=1` in your
environment to skip the TOML formatting step — `cargo fmt` will still run for Rust files.
Install taplo with `cargo install taplo-cli` for full formatting.
## Building

```sh
# Build the CLI and library
cargo build --manifest-path crates/tinylang_core/Cargo.toml

# Build the release binary
cargo build --release --manifest-path crates/tinylang_core/Cargo.toml
```

## Developer Harness

Use the repo-level xtask harness for local checks and CI gates:

```sh
# Show the command plan without running it
cargo run --manifest-path crates/xtask/Cargo.toml -- release-gate --dry-run

# Run the full release gate
cargo run --manifest-path crates/xtask/Cargo.toml -- release-gate
```

Supported tasks are `check`, `test`, `test-hooks`, `fmt-check`, `clippy`,
`bench-smoke`, `tools-test`, `package-check`, and `release-gate`. The harness uses the active
`crates/tinylang_core/Cargo.toml` and `crates/tinylang_ralloc/Cargo.toml` manifests plus the Python tools
under `Tools/`; it does not restore or require the removed root `stdlib/`.

The release gate packages and builds every publishable crate and requires a
clean working tree for that step. See the
[publication checklist](CI_CD.md#cratesio-publication) for dirty-tree dry runs,
package order, the registry ownership prerequisite, and v2 migration details.

## Running Tests

```sh
# Standard test suite (122 tests, all should pass)
cargo test --manifest-path crates/tinylang_core/Cargo.toml

# Language fixture suite (requires testing-hooks feature)
cargo test --manifest-path crates/tinylang_core/Cargo.toml --features testing-hooks

# Single test suite
cargo test --manifest-path crates/tinylang_core/Cargo.toml --test runtime_parity
cargo test --manifest-path crates/tinylang_core/Cargo.toml --test abi_api_soundness
cargo test --manifest-path crates/tinylang_core/Cargo.toml --test stdlib_parity

# Run the C FFI smoke test (requires pre-built cdylib)
cargo build --manifest-path crates/tinylang_core/Cargo.toml
cargo test --manifest-path crates/tinylang_core/Cargo.toml --test abi_api_soundness \
  c_header_ffi_smoke_covers_ownership_null_and_mode_contracts
```

For host validation, run the Python tooling through `uv` and compile the C,
C++, and Rust consumer fixtures:

```sh
uv run --no-project python -m unittest discover -s Tools -p 'test_*.py'
bash scripts/consumer-compile.sh                 # Unix
powershell -File scripts/consumer-compile.ps1    # Windows
bash scripts/consumer-smoke.sh                   # Unix native C smoke
```

CI runs the C smoke executable with native compilers on Ubuntu, macOS, and
Windows. C++, Rust, and Zig consumers use the same generated `tinylang.h`.

## Code Quality

```sh
# Format check (must pass before commit)
cargo fmt --manifest-path crates/tinylang_core/Cargo.toml --all --check

# Apply formatting
cargo fmt --manifest-path crates/tinylang_core/Cargo.toml --all

# Lints (must be clean)
cargo clippy --manifest-path crates/tinylang_core/Cargo.toml --all-targets -- -D warnings
```

## Benchmarks

```sh
# Full benchmark suite
cargo build --release --manifest-path crates/tinylang_core/Cargo.toml --bin tinylang_bench
./crates/tinylang_core/target/release/tinylang_bench

# Quick smoke run
cargo build --release --manifest-path crates/tinylang_core/Cargo.toml --bin tinylang_bench
./crates/tinylang_core/target/release/tinylang_bench \
  --quick --repeats 1
```

## Source-Tree Integrity Check

```sh
./tools/hash.py --tree . \
  --include .py --include .rs --include .toml --include .md \
  --format json
```

---

## Adding a Language Feature

Most language changes touch the compiler, possibly the verifier, and possibly
the VM/JIT. Follow this sequence:

### 1. Add a test fixture first

Add a `.to` file to `crates/tinylang_core/tests/language/pass/`, `fail_compile/`, or
`fail_runtime/` depending on expected behavior. Run the language suite to
confirm the fixture fails before your change:

```sh
cargo test --manifest-path crates/tinylang_core/Cargo.toml --features testing-hooks 2>&1 | grep FAIL
```

### 2. Extend the lexer if needed (`syntax/lexer.rs`, `syntax/token.rs`)

New keywords become new `TokenKind` variants. The lexer is a manual single-pass
scanner; add a branch to the character-dispatch loop.

### 3. Extend the compiler (`compiler/parser.rs`)

The parser is recursive descent. Each statement or expression form has its own
`parse_*` method that emits bytecode directly. Add a branch to the appropriate
`parse_statement` or `parse_expression` dispatcher.

### 4. Add new opcodes if needed (`bytecode/opcode.rs`)

Append new `Op` variants to the enum. Add them to `name()`, `from_name()`, and
`ordinal()`. **Ordinals must be stable once assigned.** Assign the next
sequential value; do not reuse a retired ordinal. Update the verifier's
stack-effect table and the VM and JIT dispatch loops.

### 5. Update the verifier (`bytecode/verifier.rs`)

The verifier's `verify_chunk` BFS tracks stack depth. Add the stack effect of
any new opcode to the match arm inside `verify_chunk`. Also add any new operand
range checks (slot indexes, string indexes, etc.).

### 6. Update the VM (`runtime/vm.rs`)

Add a match arm in `run_chunk` for the new opcode. Operations that can fail must
return `Result`.

### 7. Update the JIT (`jit/op.rs`, `jit/chunk.rs`)

- In `JitOp::from_instr`: translate the new `Op` to a `JitOp` variant.
- In `JitOp::listing`: add a display string.
- In `JitChunk::compile` (if applicable): add any superinstruction patterns.
- In `JitVm` (`jit/vm.rs`): add the execution branch.

### 8. Update the artifact format if needed (`bytecode/artifact.rs`)

If the new opcode uses operands stored in the program's string or field tables,
ensure `to_artifact` serializes them and `from_artifact` deserializes and
validates them under the existing limits.

### 9. Run the full suite

```sh
cargo test --manifest-path crates/tinylang_core/Cargo.toml
cargo test --manifest-path crates/tinylang_core/Cargo.toml --features testing-hooks
cargo clippy --manifest-path crates/tinylang_core/Cargo.toml --all-targets -- -D warnings
```

---

## Adding a Builtin Function

### Phase-1 (core) builtin

Phase-1 builtins occupy the first 35 slots of `BUILTINS`. These are frozen —
do not insert into or reorder them.

To add a Phase-2 stdlib bridge builtin:

1. **`TinyLang/src/builtins.rs`** — append a new entry after index 34 in
   `BUILTINS`. The name must not conflict with any existing builtin.

   ```rust
   BuiltinDef {
       name: "your_fn",
       min_args: N,
       max_args: N,
       requires_unsafe: false,
   }
   ```

2. **`runtime/builtins.rs`** — add a dispatch arm in
   `runtime_call_stdlib_builtin` that calls your new function.
3. **`runtime/stdlib.rs`** — implement the function as `pub fn b_your_name(…)`
   following the existing pattern. Return `Result<Value>`. Do not panic.
4. **TinyLang wrapper modules** — deferred for now. The root `stdlib/` tree has
   been intentionally removed while the standard library surface moves into the
   runtime/system layer.
5. **`TinyLang/tests/stdlib_parity.rs`** — add at least one test covering the new
   builtin via `run_source`.

### Rules for all builtins

- Never use `.unwrap()` or `.expect()` in a builtin implementation.
- Check argument types explicitly; return `TinyLangError::runtime(…)` on type
  mismatch rather than panicking.
- Respect heap limits (MAX_HEAP_BYTES, MAX_ARRAY_LENGTH, MAX_BUFFER_BYTES).
  Validate sizes before allocating.
- If the builtin touches the filesystem, it must require `unsafe` at the call
  site and must check size or count limits before reading.

---

## Adding Stdlib Surface

The active stdlib direction is runtime/system-backed builtins, not a large
root `stdlib/` tree written in TinyLang. Add the Rust builtin first, cover it
with parity tests, and only add TinyLang wrapper modules if that tree is
explicitly restored later.

1. Add the builtin entry in `TinyLang/src/builtins.rs`.
2. Dispatch it through `TinyLang/src/runtime/builtins.rs`.
3. Implement it in `TinyLang/src/runtime/stdlib.rs`.
4. Test it through `TinyLang/tests/stdlib_parity.rs`.

---

## Adding Test Fixtures

### Language fixture (`.to` file)

Place fixtures under `crates/tinylang_core/tests/language/`:

- `pass/` — programs expected to compile and print deterministic output.
- `fail_compile/` — programs expected to fail at compile time.
- `fail_runtime/` — programs expected to compile but fail at runtime.

The language suite reads fixture files and checks their outcomes automatically
when run with `--features testing-hooks`.

### Integration test (Rust)

Add a `#[test]` function to the appropriate file:

- `TinyLang/tests/runtime_parity.rs` — language behavior, VM/JIT parity
- `TinyLang/tests/stdlib_parity.rs` — stdlib behavior
- `TinyLang/tests/abi_api_soundness.rs` — FFI contracts, artifact limits, verifier

All test functions must:
- Use `run_source` or `compile_source` rather than constructing `Program` by hand
  unless specifically testing the `Program` struct.
- Assert both `vm` and `jit` modes agree when testing language behavior.
- Return nothing (`fn test_name()`) and use `assert!` / `assert_eq!` rather
  than returning `Result`.

---

## Error Handling Rules

- Production code must never call `.unwrap()`, `.expect()`, `panic!()`, or
  `unreachable!()` outside of `catch_unwind` guards or `#[cfg(test)]` blocks.
- All operations that can fail must return `Result<T>` and propagate errors
  with `?`.
- Errors must be `TinyLangError::compile(…)` for compilation-time failures or
  `TinyLangError::runtime(…)` for execution-time failures.
- Resource limits must be checked **before** any allocation. Do not allocate a
  `Vec` from an untrusted count field before validating that count against its
  limit.

---

## FFI Entry Points

When adding a new `extern "C"` function:

1. Add the declaration to `tinylang.h` with a `# Safety` doc comment.
2. Add the Rust implementation in `ffi.rs`.
3. If the function returns `char *`, it **must** route through `respond()` to
   get the double-`catch_unwind` panic boundary.
4. If the function is `void`-returning, it **must** install its own
   `catch_unwind` guard — `respond()` cannot be used for void functions.
5. Nullable `const char *` parameters must be annotated `/* nullable */` in the
   header.
6. Add a test in `TinyLang/tests/abi_api_soundness.rs` exercising the null-pointer
   and error cases.

---

## Commit Message Convention

```
area(scope): short imperative description

Longer explanation if needed. Reference specific file:line for non-obvious
decisions.
```

Examples:
```
feat(compiler): add else-if chain support
fix(jit): eliminate double-verification on cached run paths
docs(stdlib): document map_del return value
test(abi): add adversarial test for empty artifact code array
```
