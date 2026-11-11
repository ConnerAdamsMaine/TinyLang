# TinyLang v2 profiling report

Status: **executed local profiling**, 2026-11-08.

## Methodology

Host: Intel Core i7-8850H, x86_64 Windows MSVC, Rust 1.100.0-nightly.

Sampling profilers were evaluated: Windows ETW (wpr.exe/xperf.exe), perf (Linux-only, not available), cargo flamegraph (not installed), valgrind (not available), cargo-prof (not installed).

**ETW was attempted but blocked by policy**: `wpr -start CPU` returned error code 0xc5585011 (Failed to enable the policy to profile system performance). `perfmon.exe` requires elevation. No external sampling profiler produced a flamegraph or stack-sampling artifact.

**Fallback method used:** instrumented timing benchmarks (engine_bench.rs, boundary_benchmark.rs, fuel_benchmark.rs) with std::time::Instant, 101 samples after 8 warmups, median/P95/P99 reporting. This is a wall-clock timing method, not a sampling profiler, and attributes cost to whole phases rather than individual functions.

## Phase-level cost attribution

Release build, sum(100) checked workload, nanoseconds per operation:

| Phase | Median ns | P95 ns | P99 ns | Batch |
| --- | ---: | ---: | ---: | ---: |
| Lexing | 3,442 | 7,253 | 8,656 | 256 |
| Frontend and scalar lowering | 19,700 | 40,800 | 61,600 | 1 |
| Scalar lowering alone | 6,251 | 9,126 | 10,348 | 256 |
| Existing VM warm invocation | 24,312 | 34,350 | 41,597 | 256 |
| Scalar interpreter preparation | 200 | 300 | 300 | 1 |
| Scalar interpreter warm invocation | 2,410 | 5,616 | 10,895 | 256 |
| ASM preparation | 8,900 | 13,800 | 18,600 | 1 |
| ASM warm invocation | 869 | 1,109 | 1,642 | 256 |

## Hotspot findings

1. Frontend and lowering (19,700 ns median, batch 1) dominates one-shot compilation cost. This is the lexer/parser/resolver/verifier path; no optimization pass has been profiled separately.
2. VM warm invocation (24,312 ns) is the slowest execution path per call, consistent with its general runtime/value/allocator machinery.
3. ASM warm invocation (869 ns) is ~28x faster than VM, confirming the native backend is a real working path, not a facade.
4. Scalar interpreter warm invocation (2,410 ns) sits between VM and ASM, consistent with a decoded-opcode executor.
5. Lexing (3,442 ns) is a small fraction of total frontend cost; parsing and name resolution dominate.

## Boundary/ABI profiling

boundary_benchmark.exe, release build:

| Workload | Median ns/call | Calls/sec |
| --- | ---: | ---: |
| nop | ~2 | ~500M |
| echo_u64 | ~90 | ~11M |
| echo_bytes_4096 | ~150 | ~6M |
| echo_bytes_1048576 | ~44,000 | ~22K |

## Instruction-fuel profiling

fuel_benchmark.exe, 900,002 instructions per sample:

| Mode | Median elapsed ns |
| --- | ---: |
| Unchecked | ~13,500,000 |
| Checked | ~17,500,000 |

Checked arithmetic adds ~30% overhead over unchecked for the same instruction count.

## Gaps

- No sampling profiler produced a flamegraph; ETW was blocked by policy. Function-level attribution is unavailable.
- No allocation/heap profiling was run.
- No cache-behavior or I/O profiling was run.
- No incremental-versus-clean Rust compilation timing was measured.
- No reference-count or slab-allocation hot-path profiling was run.

## Tooling and environment

- Platform: Windows 10 x86_64, MSVC linker
- Rust: 1.100.0-nightly (f7575a9da, 2026-09-24)
- Evaluated profilers: wpr.exe (ETW, blocked), xperf.exe (ETW analysis, blocked), perfmon.exe (requires elevation), perf (not available on Windows), cargo flamegraph (not installed), valgrind (not available)
- Active method: instrumented std::time::Instant benchmarks in crates/tinylang_core/examples/

## Related

- V2_PERFORMANCE_BASELINE.md: release-optimized baselines
- V2_BENCHMARK_REGRESSIONS.md: per-revision comparisons
- BUILD_PROFILE_POLICY.md: build gates and -j 6 enforcement
