---
title: Rust API
---

# Rust Crate API Reference

Add `tinylang` to your `Cargo.toml` dependencies (path or crate source),
then import the public API:

```rust
use tinylang::{compile_source, run_source, JitCache, TinyLangError};
```

All fallible functions return `tinylang::Result<T>`, an alias for
`Result<T, TinyLangError>`. `TinyLangError` has two variants:
- `TinyLangError::Compile(msg)` — failure during lexing, parsing, or verification
- `TinyLangError::Runtime(msg)` — failure during execution

---

## Compilation

### `compile_source(source: &str) -> Result<Arc<Program>>`

Compile a TinyLang source string through lex → compile → optimize → verify. Returns a reference-counted, verified `Program` ready for execution.

```rust
let program = tinylang::compile_source("let x = 6 * 7\nprint x")?;
```

### `compile_source_with_filename(source: &str, filename: &str) -> Result<Arc<Program>>`

Same as `compile_source` but attaches `filename` to diagnostic messages.

### Verified compilation and disk-cache APIs

The `compile_source_verified`, `compile_source_verified_with_filename`,
`compile_file_verified`, and unoptimized verified variants return
`VerifiedProgram` directly. Prefer them when compilation is followed by VM or
JIT execution; the verified token avoids repeating verification and shares a
memoized fingerprint across clones.

`compile_file_cached_verified_with_options(path, optimize)` additionally uses
the dependency-validated disk cache and returns `(VerifiedProgram,
CompileCacheStatus)`. Status is `Hit`, `Incremental`, `Miss`, or `Bypassed`.
`Bypassed` means the graph was deliberately compiled from current source
because it falls below the size-aware cache threshold or resides on a
WSL-mounted Windows drive where validation is slower; it does not mean stale
output was reused. The shorter `compile_file_cached` and
`compile_file_cached_verified` variants use optimized compilation and omit
status where appropriate.

### `compile_file(path: impl AsRef<Path>) -> Result<Arc<Program>>`

Read the file at `path` and compile it. Resolves imports relative to the file's directory.

```rust
let program = tinylang::compile_file(std::path::Path::new("example.to"))?;
```

### `compile_source_unoptimized(source: &str) -> Result<Arc<Program>>`

Compile without the peephole optimizer. Useful for testing the verifier against unoptimized bytecode.

### `compile_file_unoptimized(path: impl AsRef<Path>) -> Result<Arc<Program>>`

Read, resolve imports, compile, and verify a file without running the peephole
optimizer. This is the API used by the CLI's `-O0` mode.

### `compile_source_unoptimized_with_filename(source: &str, filename: &str) -> Result<Arc<Program>>`

Same as `compile_source_unoptimized` but attaches `filename` to diagnostic messages.

### `lex_source(source: &str) -> Result<usize>`

Lex `source` and return the token count. Does not compile.

```rust
let count = tinylang::lex_source("let x = 42")?;
```

### `optimize_program(program: Arc<Program>) -> Arc<Program>`

Run the peephole optimizer over an already-compiled program. This function is infallible.

---

## Execution

All execution functions take a `mode: &str` that must be `"vm"` or `"jit"`.
`stdout` is any `&mut dyn Write`.

### `run_source(source: &str, mode: &str, stdout: &mut dyn Write, inputs: Vec<String>) -> Result<TinyMemory>`

Compile and run a source string. Writes program output to `stdout`. `inputs` pre-populates the deterministic input queue consumed by `read()`, `read_int()`, and `read_str()`. Returns the final heap state as `TinyMemory`.

```rust
let mut out = Vec::new();
tinylang::run_source("print 6 * 7", "jit", &mut out, vec![])?;
assert_eq!(String::from_utf8(out).unwrap(), "42\n");
```

### `run_source_report(source: &str, mode: &str, stdout: &mut dyn Write, inputs: Vec<String>) -> Result<TinyRunReport>`

Same as `run_source` but returns a `TinyRunReport` containing the final `TinyMemory` plus heap statistics (live objects/bytes, peak, total allocations/frees, shutdown frees).

### `run_program(program: Arc<Program>, mode: &str, stdout: &mut dyn Write, inputs: Vec<String>) -> Result<TinyMemory>`

Run a pre-compiled program. Runs `BytecodeVerifier::verify` internally before execution.
Consumes the `Arc<Program>`; clone it first if you need to keep a reference.

```rust
let program = tinylang::compile_source("print 42")?;
let mut out = Vec::new();
tinylang::run_program(program, "vm", &mut out, vec![])?;
```

### `run_program_report(program: Arc<Program>, mode: &str, stdout: &mut dyn Write, inputs: Vec<String>) -> Result<TinyRunReport>`

Same as `run_program` but returns heap statistics.

### `run_program_with_env(program: Arc<Program>, mode: &str, stdout: &mut dyn Write, inputs: Vec<String>, sys_args: Vec<String>, sys_env: HashMap<String, String>) -> Result<TinyMemory>`

Run with explicit program arguments and environment variables (consumed by `sys_argc()`, `sys_argv()`, `sys_env_has()`, `sys_env_get()`).

```rust
use std::collections::HashMap;

let program = tinylang::compile_source("print sys_argc()")?;
let mut out = Vec::new();
let env = HashMap::new();
tinylang::run_program_with_env(
    program,
    "vm",
    &mut out,
    vec![],
    vec!["arg1".to_string()],
    env,
)?;
```

The corresponding `run_verified_program*` functions accept
`&VerifiedProgram` and do not re-run the verifier. Use these with the verified
compiler and loader APIs on startup-sensitive paths.

### Configured JIT runner variants

`run_program_with_jit_options` and
`run_program_with_env_and_jit_options` mirror the corresponding program
runners and accept a final `JitOptions` value. Existing runner functions use
`JitOptions::default()`.

```rust
let options = tinylang::JitOptions::new().with_hot_back_edge_threshold(2);
tinylang::run_program_with_jit_options(
    program,
    "jit",
    &mut out,
    vec![],
    options,
)?;
```

For diagnostic attribution, append `.with_execution_profile(true)` and read
`JitProgram::execution_profile()` after a run. The returned profile is opt-in
and reports lowered-opcode dispatches, operand-stack pushes/pops/peak depth,
sampling attempts, specialization hits/misses, and promotions/demotions.
Encoded arithmetic that fails before returning a guard result is excluded from
the hit counter; normal JIT execution leaves detailed profiling disabled.
`JitProgram::feedback_bytes()` reports lazy per-site table backing bytes under
the shared 64-KiB program budget, excluding owner headers and allocator
overhead. Cold programs and threshold-zero execution allocate no type tables.

Dynamic-callee feedback is a separate experimental opt-in:
`JitOptions::new().with_call_feedback(true)`. It is disabled by default pending
latency acceptance, and threshold zero disables it even when requested.
Scalar quickening does not require this option. Ordinary and lazy function
chunks use the same immutable configuration; cloned programs preserve it.
When detailed profiling is enabled, the profile also reports call samples,
guard hits/misses, promotions and deoptimizations. The profile is allocated only
on request and its header is included in `JitProgram::retained_bytes()`.

---

## Artifacts

### `write_artifact(program: &Program, path: impl AsRef<Path>) -> Result<()>`

Serialize `program` to a JSON artifact file at `path`.

```rust
tinylang::write_artifact(&program, std::path::Path::new("out.tobc.json"))?;
```

### `load_artifact(path: impl AsRef<Path>) -> Result<Program>`

Deserialize and verify a JSON or compact binary artifact file. Enforces
resource limits before constructing program tables.

```rust
let program = tinylang::load_artifact(std::path::Path::new("out.tobc.json"))?;
```

`write_binary_artifact(program, path)` writes the compact versioned `.tob`
representation. `load_verified_artifact(path)` auto-detects binary magic or
JSON and preserves the `VerifiedProgram` capability for direct execution.

---

## JIT

### `JitCache`

A cache of compiled `JitProgram` instances keyed by content fingerprint and
policy trust. An untrusted artifact and a trusted source program with identical
content occupy distinct entries. Cache hits reuse quickened programs across calls.

```rust
let program = tinylang::compile_source("let i = 0\nwhile i < 1000 { i = i + 1 }\nprint i")?;
let mut cache = tinylang::JitCache::new();
let mut out = Vec::new();
cache.run_program(&program, &mut out, vec![])?;
// Second call reuses the compiled and potentially quickened JitProgram:
cache.run_program(&program, &mut out, vec![])?;
```

Use `JitCache::with_options(options)` to configure the cache. A hot-back-edge
threshold of zero disables adaptive quickening while keeping lowering,
superinstructions, caching, and JIT execution enabled.

Exact-source verified compilations use a separate bounded LRU cache. It
defaults to 128 entries and 8 MiB of retained source text. Configure it while
constructing the cache:

```rust
let cache = tinylang::JitCache::with_options(options)
    .with_source_cache_limits(64, 4 * 1024 * 1024);
```

Either limit may be zero to disable exact-source retention without disabling
the compiled JIT cache.

Compiled entries have their own deterministic LRU policy: 128 entries and
16 MiB of charged retained-entry storage by default. Configure it with
`with_compiled_cache_limits(max_entries, max_retained_bytes)`. Either zero
disables compiled retention (both effective limits become zero), independently
of source caching. Oversized programs still run, but their compiled state is
not retained. Lazy chunks, feedback and idle operand capacity are accounted
after both successful and failing runs. A host output panic discards the
affected entry and resumes the original panic.

The borrowed `compile`/`compile_verified` APIs keep one non-reusable temporary
result when an entry cannot fit. It is outside the reusable-entry byte limit,
reported as `transient_bytes`, and cleared by the next mutable cache operation.
It is not included in `len()` and is not copied by cache cloning. Run methods
do not keep this temporary result. Use `JitProgram::compile_verified` directly
when the caller needs to own a standalone compiled result.

`compiled_cache_stats() -> JitCompiledCacheStats` reports resident programs,
charged `retained_bytes`, `transient_bytes`, `table_capacity`, and lifetime
hits, misses, successful compilations, evictions and bypasses. Byte charges
include occupied entry headers, keys, all reachable verified metadata and
constants, buffer **capacities**, installed lazy code, feedback and idle operand
buffers. Shared verified data is conservatively charged in full per entry;
do not add these figures to source-cache bytes as an exclusive-memory total.
Allocator/Arc overhead, unused hash-table buckets and opaque profiling-tree
node slack are excluded. This is not RSS or a peak-execution-memory limit.
`JitProgram::retained_bytes()` exposes the program portion of the same charge.

`clear_compiled_cache()` releases compiled entries, table capacity and any
temporary result, preserving source entries and lifetime activity counters.
Reducing limits immediately evicts entries and shrinks the table. Cloning
recomputes charges because cloned programs have no idle operand pool and their
cloned instruction vectors may have less spare capacity.

Key methods on `JitCache`:

- `JitCache::with_options(options) -> JitCache` — create a configured cache
- `cache.options() -> JitOptions` — inspect its immutable JIT configuration

- `JitCache::new() -> JitCache` — create an empty cache
- `cache.len() -> usize` — number of cached programs
- `cache.source_cache_len() -> usize` — number of exact sources with a cached verified compilation
- `cache.source_cache_bytes() -> usize` — retained exact-source text bytes
- `cache.source_cache_max_entries() -> usize` — configured entry limit
- `cache.source_cache_byte_limit() -> usize` — configured retained-source byte limit
- `cache.compiled_cache_max_entries() -> usize` — effective compiled-entry limit
- `cache.compiled_cache_byte_limit() -> usize` — effective retained-entry byte limit
- `cache.compiled_cache_stats() -> JitCompiledCacheStats` — retention and lifetime cache activity
- `cache.clear_compiled_cache()` — release compiled storage without clearing sources
- `cache.is_empty() -> bool` — true when the cache holds no programs
- `cache.compile(program: &Program) -> Result<&JitProgram>` — compile and cache without running; verifies the program first
- `cache.compile_verified(program: &VerifiedProgram) -> Result<&JitProgram>` — compile without duplicate verification or hashing
- `cache.run_program(program, stdout, inputs) -> Result<TinyMemory>` — compile (if not cached) and run
- `cache.run_verified_program(program, stdout, inputs) -> Result<TinyMemory>` — verified-token run path
- `cache.run_program_report(program, stdout, inputs) -> Result<TinyRunReport>` — same, but includes heap statistics
- `cache.run_program_with_env(program, stdout, inputs, sys_args, sys_env) -> Result<TinyMemory>` — run with explicit args and environment
- `cache.run_source(source, stdout, inputs) -> Result<TinyMemory>` — compile and verify new source, then reuse its verified compilation and JIT state on exact-source hits
- `cache.run_source_report(source, stdout, inputs) -> Result<TinyRunReport>` — same, with heap statistics
- `cache.stats() -> JitCacheStats` — aggregate stats across all cached programs

The compiled-program fields in `stats()` aggregate **currently resident**
entries, not evicted programs or the compile-only temporary result. Use
`compiled_cache_stats()` for cache-lifetime activity counters.

Source-cache statistics include `source_bytes`, `source_hits`,
`source_misses`, `source_compilations`, `source_evictions`, and
`source_bypasses`.

### `write_jit_listing(program: &Program, path: impl AsRef<Path>) -> Result<()>`

Compile `program` through the JIT and write the human-readable assembly listing to `path`.

```rust
tinylang::write_jit_listing(&program, std::path::Path::new("listing.txt"))?;
```

Use `write_verified_jit_listing(&verified, path)` when a trusted compiler or
artifact loader already returned `VerifiedProgram`; it reuses the capability
and memoized fingerprint instead of verifying again. The raw-program function
continues to verify before lowering.

---

## Verification

### `BytecodeVerifier::verify(program: &Program) -> Result<()>`

Check operand availability, consistent control-flow stack depths and static
operands across all chunks. Resource limits (functions, ops, slots, strings,
structs and modules) are enforced before allocating the indexed traversal
table. Returns `Ok(())` on success.

This compatibility call constructs and drops the immutable verification
certificate. Use `VerifiedProgram` when the caller needs to retain that proof
and its metadata for subsequent execution or lowering.

```rust
tinylang::BytecodeVerifier::verify(&program)?;
```

### `VerifiedProgram`

A capability wrapper that records that verification has already run. A single
shared owner retains an `Arc<Program>`, a lazily initialized fingerprint, and
immutable per-chunk verification metadata. Cloning the token shares all three
with one reference-count increment. Construct it via
`VerifiedProgram::verify(program)` or obtain it from a verified compiler or
artifact-loader API.

```rust
let verified = tinylang::compile_source_verified("print 1")?;
// Borrow the inner Program without consuming:
let _inner: &tinylang::Program = verified.program();
let fingerprint: &str = verified.fingerprint(); // memoized and clone-shared
// Consume and recover an owned Program (the type-system guarantee is lost):
let program = verified.into_program();
```

Key methods on `VerifiedProgram`:

- `VerifiedProgram::verify(program: Program) -> Result<VerifiedProgram>` — verify and wrap; takes an owned `Program`, not `Arc<Program>`
- `verified.program() -> &Program` — borrow the inner program
- `verified.into_program() -> Program` — consume and return the inner program

### Feature-gated runtime cost counters

With the `testing-hooks` feature enabled, external performance harnesses can
measure heap-lock acquisitions, value encodes/decodes, Ralloc growth events,
and allocator bytes copied:

```rust
use tinylang::testing::{reset_runtime_cost_counters, runtime_cost_counters};

reset_runtime_cost_counters();
// Run one isolated workload here.
let costs = runtime_cost_counters();
println!("{costs:?}");
```

These counters are process-wide and include spawned runtime threads. Reset
them only when the harness owns an exclusive measurement interval; they are a
diagnostic testing surface, not part of the production runtime API.
