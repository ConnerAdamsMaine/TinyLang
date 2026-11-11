# TinyLang v2 build profile policy

Status: **enforced and verified**, 2026-11-08.

## The `fast` profile

The authoritative workspace-root `Cargo.toml` defines a dedicated `fast`
profile. It exists solely to minimize iteration latency while verifying that
the project can be checked, compiled, and linked without optimization. It is
**not** proof that the resulting program works or that runtime performance is
representative.

```toml
[profile.fast]
inherits     = "dev"
opt-level     = 0
debug         = 0
debug-assertions = false
overflow-checks = false
codegen-units = 256
incremental   = false
strip         = "none"
panic         = "unwind"
lto           = "off"

[profile.fast.package."*"]
inherits = "fast"
```

The profile is merged without damaging the existing `dev`, `release`, or
`test` behavior. `opt-level = 0` and `debug = 0` keep compilation fast;
`codegen-units = 256` maximizes parallel codegen within the job cap. The
`release` profile remains at `opt-level = 3`, `codegen-units = 1`,
`strip = "symbols"` for production builds and benchmarks.

## The `-j 6` compilation limit

Every Cargo operation that may compile code is invoked with an explicit
`-j 6`. This is enforced at the workspace level in `.cargo/config.toml`:

```toml
[build]
jobs = 6
```

Six is treated as a maximum for the **combined** build activity, not a license
for every parallel agent to start its own six-job build simultaneously. Builds
are coordinated through a single shared `target` directory; concurrent builds
are not launched against the same workspace.

## Compilation gates

| Gate | Command | What it proves |
| --- | --- | --- |
| Type-check | `cargo check --workspace --all-targets --profile fast -j 6` | Types resolve; no codegen or linking |
| Compile and link | `cargo build --workspace --profile fast -j 6` | Compilation and linkage of selected targets |
| Functional test | `cargo test --workspace -j 6` | Executed tests pass |
| Release build | `cargo build --workspace --release -j 6` | Optimized compilation and linkage |
| Release test | `cargo test --workspace --release -j 6` | Functional correctness under optimization |
| Benchmark | `cargo bench --workspace -j 6` | Repeatable measured performance |

`cargo check` is a type-check gate only; it does not confirm code generation
or linking. `cargo build --profile fast` confirms compilation and linkage of
selected targets only; **it does not establish functional correctness**.
Release/profiling builds and repeatable benchmarks establish measured
performance; results from `fast` must never be substituted for them.

## Build-time and memory baselines

Measured 2026-11-08, Intel Core i7-8850H, x86_64 Windows MSVC, Rust
1.100.0-nightly.

| Operation | Wall time |
| --- | ---: |
| `cargo check --workspace --profile fast -j 6` | ~3.8 s (incremental) |
| `cargo build --workspace --profile fast -j 6` | ~20 s (incremental) |
| `cargo test --workspace -j 6` | ~41 s |
| `cargo build --workspace --release -j 6` | ~56 s (first release build) |

Peak memory during the release build was within normal linker pressure for the
workspace; no memory-threshold breach was observed. If `-j 6` causes memory
pressure, concurrent builds are stopped and peak memory is inspected rather
than launching more builds.

## Exceptions

No exception to `-j 6` has been taken. Cargo itself has not rejected any
invocation. No feature-specific build has bypassed the job cap.

## Related

- `V2_PERFORMANCE_BASELINE.md`: release-optimized runtime baselines
- `V2_PROFILING_REPORT.md`: profiler findings and methodology
- `V2_BENCHMARK_REGRESSIONS.md`: per-revision comparisons
