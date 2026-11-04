---
title: CI/CD
---

# CI/CD

> Native preference: version pins live in native files (`rust-toolchain.toml`, `.python-version`), not
> duplicated in shell. Cargo already exports `CARGO_TARGET_DIR` -- use it directly; do not redefine
> `TARGET_DIR` as a second source of truth.

## Toolchain pinning

| Tool        | Pin file                                                             | Composite input                                      | Bump procedure                                                                                                    |
| ----------- | -------------------------------------------------------------------- | ---------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------- |
| Rust        | `rust-toolchain.toml` (`channel = "stable"` + `components`)          | `setup-rust` `inputs.toolchain` (default reads file) | Edit `rust-toolchain.toml`, update `docs/CI_CD.md` table, verify `cargo make ci-gate` locally                     |
| Python      | `.python-version` (`3.12`)                                           | `setup-python` `inputs.python-version`               | Edit `.python-version` + `build/ci-tasks.toml` `[env] PYTHON_VERSION`, verify `uv run`                            |
| uv          | `setup-python` `inputs.uv-version` (`latest`)                        | `setup-python`                                       | Bump in `setup-python/action.yml` default                                                                         |
| GCC/Clang   | `setup-gcc` `inputs.gcc-version` / `clang-version`                   | `setup-gcc`                                          | Edit `setup-gcc/action.yml` defaults + this doc; keep `build/` owned by `@mrdwarf7` so Connor can't silently bump |
| mold        | `setup-rust` `MOLD_VERSION='2.41.0'` (Linux)                         | `setup-rust` `inputs.use-mold`                       | Bump in `setup-rust/action.yml`                                                                                   |
| cargo tools | `setup-rust` `inputs.tools` (`cargo-binstall,cargo-make,just,taplo`) | `setup-rust`                                         | Edit `setup-rust` `inputs.tools` default                                                                          |

**Rules for agents:**

- Do not add `env:` version lists to composite actions -- use `inputs` with defaults (tested: `env` is disallowed in action files).
- `nightly.yml` must `rm -f rust-toolchain.toml` before `setup-rust` so `inputs.toolchain = nightly` takes precedence.
- `cranelift.yml` is isolated for the same reason (codegen backend affects language performance).
- GCC pinning matters for C ABI artifacts; do not trust "developer not lazy", pin explicitly and require owner sign-off.

### Rust formatting toolchain

Rust formatting requires an installed nightly toolchain with `rustfmt` because
`.rustfmt.toml` uses nightly-only options. `format.yml` already selects nightly
through `RUSTUP_TOOLCHAIN`; the xtask `fmt-check` and `release-gate` commands
select it explicitly through `rustup run nightly cargo fmt`. Other xtask Cargo
commands retain the repository's stable toolchain selection.

The matching standalone check is `rustup run nightly cargo fmt --all --check`.
Bare `cargo fmt` under the pinned stable toolchain ignores those options and
does not reproduce the CI formatting gate.

## Paths

- `Makefile.toml` defines `ROOT` (shorthand for `CARGO_MAKE_WORKSPACE_WORKING_DIRECTORY` -- 15+ chars, comment explains), `SCRIPT_DIR = "${ROOT}/scripts"`, `TOOL_DIR = "${ROOT}/tools"`.
- `CARGO_TARGET_DIR` is cargo-native; extend files must not re-define it.
- `build/ci-tasks.toml` owns ci gates; `build/ffi-tasks.toml` owns ffi (strict split, no bleed).
- Root `Makefile.toml` is alias-only: `ma f` -> `format`, etc. Extend files declare local `env` only if strictly file-scoped.

## Scripts

- `scripts/consumer_compile.sh`, `consumer_smoke.sh`, `consumer_smoke_msvc.sh`, `check_abi_drift.sh` -- all `#!/usr/bin/env bash`, fail hard (no `||` fallback), use `CARGO_TARGET_DIR`.
- `scripts/.archive/` holds legacy single-line wrappers (`ci_gate.sh`, `consumer-compile.ps1`) -- do not create new one-line `cargo make <x>` wrappers; call `cargo make` directly.
- Workflows use `shell: bash` on all runners (including `windows-latest`); no `pwsh` except mold setup needs.

## Workflows

- `format.yml` -- fmt+taplo check (matrix ubuntu/windows), auto-fix push on `push` to `main`/`master`.
- `build.yml` -- `check` + `clippy -D warnings` matrix.
- `test.yml` -- `cargo test --workspace` + `language_suite` + Python tool tests +
  ABI drift + verified crate packaging (no upload).
- `ffi.yml` -- cdylib build + consumer_compile + consumer_smoke (3 OS) + setup-gcc pins.
- `nightly.yml` / `cranelift.yml` -- isolated, dispatch/schedule only.

The allocator C suite builds its feature-specific library under
`$CARGO_TARGET_DIR/c-suite` (or `target/c-suite`). It must not overwrite the
parent Cargo test artifacts: doing so invalidates the allocator dependency
used by subsequent core doctests. The release-artifact test similarly uses
a temporary target directory, including when its parent test uses the release
profile or enables additional features. Both tests verify that the parent
allocator library remains unchanged.

## CODEOWNERS

- `* @ConnerAdamsMaine @mrdwarf7` (owner first)
- `/docs/` and `*.md` -> `@ConnerAdamsMaine`
- `/build/` and `/Makefile.toml` -> `@mrdwarf7` (intentional friction, owner can still override but review required)
- See `.github/CODEOWNERS` for full patterns (last match wins).

## Local

```bash
ma fmt          # format
ma ci-gate      # full gate (no script wrapper)
ma ci-gate-fast # check+test
ma ffi-consumer-compile
```

## GitHub binary releases

GitHub binary releases are separate from registry publication. For each of
Windows x86-64 MSVC and Linux x86-64 GNU, validate the same clean commit:

```sh
cargo run --locked --manifest-path crates/xtask/Cargo.toml -- release-gate
cargo test --locked --workspace --all-features -- --test-threads=1
cargo test --locked --workspace --all-features --release -- --test-threads=1
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo build --locked --release -p tinylang-v2 --lib --bins
```

Run the C/C++/Rust consumer fixtures and native C smoke test on each platform
as well. Use an actual Linux environment for Linux execution tests, not only
a Windows cross-compilation.

Package the CLI, `tinylang_bench`, `tinylang-sandbox-worker`, shared library,
`include/tinylang.h`, `tinylang-response-schema.json`, readme, and license
together. Windows binaries use `.exe`; include `tinylang.dll.lib` for MSVC
consumers. Use ZIP for Windows and tar.gz for Linux, preserving Linux executable
permissions. Do not distribute debug/test binaries or the experimental SDK
workspace as the core runtime.

Smoke-test the extracted archives from outside the source checkout, including
VM/JIT execution, bytecode loading, ABI version 2, and the sandbox worker.
Record the source commit, toolchains, Linux libc requirements, validation
results, and SHA-256 checksums in the release notes. Upload to a GitHub draft
for review before publication. No crates.io upload is needed for this step.

## Crates.io publication

The workspace publishes three crates in dependency order:

| Package | Version | Purpose |
| --- | --- | --- |
| `tinylang_test_support` | `0.1.0` | Versioned development dependency for packaged tests |
| `tinylang_ralloc` | `0.2.0` | Runtime allocator (`ralloc` Rust library) |
| `tinylang-v2` | `2.0.0` | Compiler, runtime, CLI, JSON C API, and sandbox worker |

`tinylang-xtask` is private (`publish = false`). The v2 fuzz targets
(`crates/tinylang_core/fuzz/`) and experiments (`crates/tinylang_core/experiments/`)
are separate private workspaces excluded from this publication. Keep every dependency between published workspace crates
versioned as well as path-based. Shared licensing and repository metadata
live in the root `[workspace.package]`; do not replace the custom license
with an unrelated SPDX identifier.

**Naming and authorization:** the core package is `tinylang-v2`, while its
Rust library and CLI remain `tinylang`. The unrelated crates.io package
`tinylang` is not this project. Keep package selectors, publication checks,
and header-generation validation aligned with `tinylang-v2`. A package check
or publish dry run verifies contents and builds, not permission to publish;
confirm registry ownership and release approval before any upload.

Use a current stable Cargo with workspace packaging/publishing support.
From a clean checkout:

```sh
cargo run --manifest-path crates/xtask/Cargo.toml -- release-gate
cargo publish --workspace --exclude tinylang-xtask --dry-run --locked
```

For an uncommitted preparation branch, inspect the current changes without
uploading:

```sh
cargo package --workspace --exclude tinylang-xtask --allow-dirty --locked
cargo publish --workspace --exclude tinylang-xtask --dry-run --allow-dirty --locked
```

`package-check` is the packaging-only xtask. Cargo verifies all three
archives, using a temporary local registry for unpublished workspace
dependencies. Do not add `--no-verify`. Archives under `target/package/`
must contain their readme and license. The core archive also contains its
CLI sources, `include/tinylang.h`, `tinylang-response-schema.json`, test
fixtures, and `tinylang-sandbox-worker` source. The root C header is only a
source-checkout forwarding header; generate the canonical header with
`uv run --no-project python tools/abi_manifest.py generate-header`.

On Windows, Cargo may warn that the same-named `tinylang` library and CLI
share a debug `.pdb` filename. Release builds disable debug information.
Keep the public names consistent; use separate library and binary builds
when collecting debug symbols.

Only after ownership, validation, and release approval are resolved, publish
from the clean release commit:

```sh
cargo publish --workspace --exclude tinylang-xtask --locked
```

No repository workflow uploads crates automatically.

### V2 migration boundary

- Rust imports use `tinylang`, with `TinyLangError`.
- The JSON C ABI uses `tinylang_*` exports and `TINYLANG_ABI_VERSION = 2`.
  Its response shapes remain unchanged; the experimental `tinylang_v2_*`
  binary ABI is separate.
- Use `TINYLANG_*` environment variables, `tinylang.json` module maps, and
  the `tinylang-sandbox-worker` executable.
- Recompile artifacts: the JSON format name is `tinylang-bytecode` and
  the binary magic is the eight-byte `TINYLANG`. JSON layout versions 1/2
  and binary layout version 5 are independent of package/ABI versions.
- Re-sign module metadata with language version 2 and the renamed signing
  domain prefixes. Do not reuse v1 signatures or caches.
- Existing GitHub URLs retain the repository's actual name. The v1 release
  branch is unchanged; do not backport this breaking rename to it.
