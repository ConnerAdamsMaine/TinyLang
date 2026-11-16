---
title: CLI Reference
---

# CLI Reference

```
usage: tynra [OPTIONS] [path]
```

Tynra exits with status `0` on success, status `1` on any error.

Build the CLI executable before using these commands:

```sh
cargo build --manifest-path crates/tynra_core/Cargo.toml
```

Cargo writes the debug executable to `target/debug/tynra`
(`target/debug/tynra.exe` on Windows). For optimized local use,
build with `cargo build --release --manifest-path crates/tynra_core/Cargo.toml`; the
release executable is `target/release/tynra` or
`target/release/tynra.exe`. The examples below assume the executable
is available on `PATH` as `tynra`.

---

## Flags

### `path`

The `.to` source file to compile and execute. If `--run-bytecode` is specified, this argument is not required.

```sh
tynra example.to
```

---

### `--mode {jit,vm}`

Select the execution backend. Default is `jit`.

- `jit` — compile verified bytecode into the adaptive JIT tier and run it. Superinstructions and hot-loop quickening reduce dispatch overhead on repeated or long-running programs.
- `vm` — interpret bytecode with the portable VM. Simpler and easier to debug; useful for comparing behavior against the JIT.

```sh
# JIT mode (default)
tynra example.to

# VM mode
tynra --mode vm example.to
```

`-j` / `--jit` and `--vm` are direct aliases for the corresponding mode, so
`tynra --vm example.to` is equivalent to `--mode vm`.

---

### `-O0`, `--no-optimize` / `-O1`, `--optimize`

Control bytecode optimization for source compilation. Optimization is enabled
by default. `-O0` is useful when comparing raw compiler output with optimized
bytecode; `-O1` explicitly restores the current optimization tier.

```sh
tynra -O0 --emit-bytecode raw.tobc.json example.to
tynra -O1 example.to
```

---

### `--no-cache`

Source files use a dependency-validated `.tynra-cache/` beside the root
program by default. Cache hits skip lexing, parsing, optimization, and module
assembly. Tynra hashes the root, imported sources, manifests (including
previously missing manifest probes), and canonical import resolutions before
loading a compact artifact, then verifies the artifact and fingerprint.

A single changed module with stable function/struct topology is rebuilt and
relocated incrementally; other changes safely fall back to a full build. Use
`--no-cache` for reproducibility checks or read-only workflows. `--verbose`
prints `cache=hit`, `cache=incremental`, `cache=miss`, or `cache=off`.

---

### `--jit-threshold N` / `--no-jit-quickening`

Set the number of backward-edge executions before a loop is quickened. The
default is `8`; valid values are `0..65535`, and `0` disables quickening.
`--no-jit-quickening` is the readable alias for threshold zero.

```sh
tynra --jit-threshold 2 example.to
tynra --no-jit-quickening example.to
```

---

### `--check`

Compile and verify the source file without executing it. Exits `0` if the program compiles and passes verification, `1` otherwise.

```sh
tynra --check example.to
```

---

### `--emit-bytecode PATH`

After compilation, write a bytecode artifact to `PATH`. A `.tob` suffix selects
the compact binary format; every other suffix writes readable JSON. Either can
be executed later with `--run-bytecode`.

```sh
tynra \
  --check --emit-bytecode out.tobc.json example.to

tynra \
  --check --emit-bytecode out.tob example.to
```

See [`bytecode.md`](bytecode.md) for the artifact format.

---

### `--emit-jit PATH`

Write the human-readable JIT assembly listing to `PATH` after compilation. Shows the decoded `JitOp` sequence and superinstruction fusions.

```sh
tynra \
  --emit-jit listing.txt example.to
```

---

### `--run-bytecode PATH`

Execute a pre-compiled JSON or compact binary artifact from `PATH` without
recompiling from source. Format detection uses the binary magic, not the file
suffix. The artifact is verified before execution.

```sh
# Compile once
tynra \
  --check --emit-bytecode out.tobc.json example.to

# Run the artifact
tynra \
  --run-bytecode out.tobc.json
```

Artifact runs execute **deny-all**: a serialized policy inside an artifact is
data, not a grant, so an artifact can never grant host capabilities that its
compilation context did not have. Host authority for artifact pipelines is an
embedding-host decision, not a CLI flag.

---

### Authorized execution (package runtimes)

These four flags are the shipped authorized-execution contract used by
authority-verified package runtimes (e.g. TPack). They require the full `tynra`
binary (not the `tynra-v2` scalar-engine CLI).

### `--source-from-stdin`

Read the program source from stdin (authorized package runtimes bind verified
bytes this way).

### `--expect-source-digest SHA256`

Bind execution to exact source bytes: the run refuses to start unless the
compiled source's SHA-256 matches `SHA256` (`authorized execution refused:
source digest mismatch` on mismatch).

### `--expect-artifact-digest SHA256`

Bind an artifact run to exact bytes via `load_authorized_artifact`. The digest
authenticates the artifact's bytes; it does **not** make a serialized policy a
grant — artifact execution stays deny-all.

### `--authorized-permissions TOML`

Apply an explicit manifest-pinned permission policy (`AuthorizedPermissions`)
from a TOML fragment. Default is deny-all; denied entries and omitted fields
fail closed. Requires an authorized source path (`--source-from-stdin` or
`--expect-source-digest`) and is refused together with `--run-bytecode`
(`--authorized-permissions cannot be combined with --run-bytecode: artifact
policy is data, not a grant`).

---

### `--input VALUE`

Append one string item to the deterministic input queue. Repeat to supply multiple items. Items are consumed in order by `read()`, `read_int()`, and `read_str()`.

```sh
tynra \
  --input 10 --input 20 example.to
```

```tynra
# example.to
let a = read_int()
let b = read_int()
print a + b   # 30
```

---

### `--stdin`

Read stdin line-by-line and append each line to the deterministic input queue.

```sh
printf "10\n20\n" | tynra --stdin example.to
# Output: 30
```

---

### `--verbose`

Print a compiler/runtime summary to stderr after execution. Includes execution metadata, slot counts, function count, and other program statistics.

```sh
tynra \
  --verbose example.to
```

---

## Common Workflows

### Compile-once / run-many

```sh
tynra \
  --check --emit-bytecode program.tobc.json program.to

tynra \
  --run-bytecode program.tobc.json --input 7
tynra \
  --run-bytecode program.tobc.json --input 12
```

### Inspect the JIT listing

```sh
tynra \
  --emit-jit /dev/stdout --check program.to 2>/dev/null
```

### Validate without running (CI)

```sh
tynra --check program.to && echo "OK"
```

---
