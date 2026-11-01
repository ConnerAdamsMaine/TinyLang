# Repaired v2 boundary baseline

Measured 2026-10-11 on Intel Core i7-8850H, 6 cores / 12 logical processors,
Windows x64, rustc 1.99.0 MSVC. Crate lockfile is committed with the imported
crate; this is its independent workspace's default release profile (opt-level
3, no explicit LTO), not the main workspace's customized profile. No custom
RUSTFLAGS were set by this session. CPU frequency/governor was not controlled.

```powershell
cargo build --release --locked --manifest-path tinylang_v2/Cargo.toml --target-dir target/abi-v2 --lib --example boundary_benchmark
./target/abi-v2/release/examples/boundary_benchmark.exe
cargo test --release --locked --manifest-path tinylang_v2/Cargo.toml --target-dir target/abi-v2
```

The benchmark performs one complete warm-up batch then seven timed batches.
Each batch checks successful status; after timing it verifies response size,
response status and exact payload bytes. Buffers are allocated before timing.
The call crosses the Rust `extern "C"` function boundary in a linked executable;
it does **not** measure dynamic-loader dispatch or Python/Node conversion.
Measurements include loop/assertion/black-box overhead and frame validation;
they exclude initial encoding, allocation, startup, compilation and VM work.
Echo additionally includes copying and value validation. This is not a separate
encoder/decoder throughput measurement.

Raw output: [baseline-boundary.csv](evidence/baseline-boundary.csv). Its first
line records initialization; the following lines are a CSV sample table.

| Workload | Iterations per sample | Samples | Median ns/call | Min–max ns/call |
| --- | ---: | ---: | ---: | ---: |
| NOP | 4,000,000 | 7 | 30.292 | 29.300–32.974 |
| U64 echo | 2,000,000 | 7 | 62.937 | 62.398–64.177 |
| 4 KiB bytes echo | 1,000,000 | 7 | 95.433 | 94.719–98.819 |
| 1 MiB bytes echo | 4,000 | 7 | 31,270.775 | 30,978.550–32,011.625 |

An initial short-batch run was superseded by these longer workloads (roughly
95–132 ms per batch). These are descriptive baseline samples, not a
statistically established speedup.

| Measurement | Baseline | A | B | C |
| --- | --- | --- | --- | --- |
| Correctness | 25 tests pass in debug and release | Unavailable | Unavailable | Unavailable |
| Call latency | NOP median 30.292 ns | Unavailable | Unavailable | Unavailable |
| Throughput | Per-sample calls/s and payload bytes/s in raw CSV | Unavailable | Unavailable | Unavailable |
| Allocation count | Unavailable (not instrumented) | Unavailable | Unavailable | Unavailable |
| Peak memory | Unavailable (not measured) | Unavailable | Unavailable | Unavailable |
| Data copying | Echo copies canonical payload once, by source inspection; not instrumented | Unavailable | Unavailable | Unavailable |
| Compilation + linking | 29.21 s initial release build, one observation including dependencies | Unavailable | Unavailable | Unavailable |
| Linked executable file size | 301,056 bytes | Unavailable | Unavailable | Unavailable |
| DLL file size | 1,481,728 bytes (import library: 6,272 bytes) | Unavailable | Unavailable | Unavailable |
| Initialization | 9,000 ns first runtime creation, one observation | Unavailable | Unavailable | Unavailable |
| ABI compatibility | Existing golden vectors and Windows consumers pass | Unavailable | Unavailable | Unavailable |
| Intentional limitations | VM invocation absent; other targets untested | Unavailable | Unavailable | Unavailable |
| Implementation complexity | Existing wire decoder and global bounded registry | Unavailable | Unavailable | Unavailable |

Candidates have not been implemented or measured. Improvement percentages are
therefore unavailable; no fabricated zeros or comparisons are reported.
