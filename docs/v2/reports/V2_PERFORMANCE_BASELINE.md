# TinyLang v2 performance baseline

Status: **executed local release baseline**, 2026-11-01.

The [benchmark](../../../crates/tinylang_core/examples/engine_bench.rs) executes the compiler-
produced `sum(100)` and independently checks the result is 4950. It separates
lexing, frontend plus lowering, scalar lowering, backend preparation, and
warm execution. VM and native measurements include argument/frame/result
overhead, not just arithmetic instruction timing.

```powershell
cargo run --release -p tinylang-v2 --example engine_bench --locked
cargo run --release -p tinylang-v2 --features cranelift --example engine_bench --locked
```

101 samples per metric follow 8 warmups; warm execution batches 256
invocations. Output reports median, P95 and P99 nanoseconds per operation,
sample count and batch size. Preparation does not include source compilation.
Preparation samples include destruction of the freshly prepared owner.
This is a small local baseline, not a performance regression gate or a
full-language speedup claim.

Host: Rust 1.100.0-nightly (`f7575a9da`, 2026-09-24), LLVM 23.1.1 reported by
rustc, target x86_64-pc-windows-msvc; Intel Core i7-8850H, 6 cores/12 logical
processors. rustc's bundled LLVM version alone does
not establish an available external LLVM toolchain.

## Checked workload measurements

Cranelift-enabled release build; every execution checked `sum(100) = 4950`.
Times are nanoseconds per operation.

| Metric | Median | P95 | P99 | Batch |
| --- | ---: | ---: | ---: | ---: |
| Lexing | 3,725 | 8,262 | 9,109 | 256 |
| Frontend and scalar lowering | 55,900 | 60,600 | 67,300 | 1 |
| Scalar lowering alone | 6,133 | 8,648 | 13,751 | 256 |
| Existing VM warm invocation | 19,476 | 22,497 | 54,557 | 256 |
| Scalar interpreter preparation | 200 | 300 | 400 | 1 |
| Scalar interpreter warm invocation | 2,503 | 2,856 | 5,239 | 256 |
| ASM preparation | 12,900 | 17,800 | 22,900 | 1 |
| ASM warm invocation | 893 | 1,262 | 1,492 | 256 |
| Cranelift preparation | 690,900 | 766,100 | 843,200 | 1 |
| Cranelift warm invocation | 1,835 | 1,935 | 1,978 | 256 |

### Program-profile rows (executed 2026-11-10)

Same host and release configuration (`--features cranelift`), same checked
`sum(100) = 4950` workload and harness; whole-call-closure program profile
added to the same [harness](../../../crates/tinylang_core/examples/engine_bench.rs).
Program preparation covers whole-closure lowering and, for ASM, image
emission and cross-call patching; for Cranelift, per-function IR lowering and
module finalization. Warm invocation includes the frame bank and depth
machinery.

| Metric | Median | P95 | P99 | Batch |
| --- | ---: | ---: | ---: | ---: |
| Program scalar lowering | 6,248 | 8,855 | 10,642 | 256 |
| Program interpreter preparation | 300 | 400 | 400 | 1 |
| Program interpreter warm invocation | 3,224 | 3,987 | 16,358 | 256 |
| Program ASM preparation | 9,500 | 10,000 | 15,400 | 1 |
| Program ASM warm invocation | 878 | 1,215 | 1,461 | 256 |
| Program Cranelift preparation | 742,400 | 999,000 | 1,268,400 | 1 |
| Program Cranelift warm invocation | 1,620 | 2,170 | 2,410 | 256 |

The program ASM image adds about 0.02 µs median over the single-function ASM
row in the same run (878 vs 857) for bank indexing and depth bookkeeping; the
original baseline comparison read 1,096 vs 893. Program preparation is
costlier than leaf preparation because the whole closure is emitted and
patched. Program Cranelift warm invocation lands between the program ASM and
interpreter rows (1,620 vs 878 and 3,224 median in the same run), consistent
with its per-instruction context pc store and fuel check; its preparation
dominates at 742,400 ns because every closure member is compiled through
Cranelift. The earlier same-session default (non-Cranelift) release run
reproduced the ASM ordering with more scheduler noise: single-function ASM
warm 1,645 vs program ASM warm 1,088 median. LLVM has no program backend and
is not benchmarked; its absence is asserted by the CLI test's explicit
`BackendUnavailable` check.

This is equivalent checked scalar work, not equivalent full-language
capability. The VM retains its general runtime/value/allocator machinery.
No native backend existed for a before/after optimization comparison, and no
full-language speedup is claimed. CPU scheduling, frequency and OS cache
state were not controlled.

## CLI process wall time and binary size

31 fresh processes per backend, Cranelift-enabled release CLI, persistent
[example](../../../crates/tinylang_core/examples/scalar.tinylang), `sum 100`; every output checked
4950. These measurements include PowerShell launch, process startup, file
compilation/cache handling, preparation and execution. Source/OS caches were
warm, so they are not uncached cold-source measurements. With 31 samples P99
is the maximum observed value, not a sustained tail-latency guarantee.

| Backend | Median ms | P95 ms | P99 ms |
| --- | ---: | ---: | ---: |
| ASM | 8.401 | 10.620 | 14.407 |
| Cranelift | 11.063 | 18.128 | 25.123 |

Release CLI file size: 1,244,160 bytes default build; 5,977,600 bytes with
Cranelift. Both were built with `cargo build --release --bin tinylang-v2`
and executed successfully. These are CLI sizes, not DLL/object-file sizes.

## Optional LLVM 20.1.8 object baseline

The [repeatable LLVM benchmark](../../../crates/tinylang_core/examples/llvm_bench.py) uses the
same checked `sum(100) = 4950` workload, 8 warmups and 101 samples:

```powershell
& '.\target\llvm-validation\Scripts\python.exe' '.\crates\tinylang_core\examples\llvm_bench.py'
```

Use the pinned environment setup in [LLVM integration](LLVM_CRANELIFT_INTEGRATION.md).
Times are nanoseconds per operation:

| Metric | Median | P95 | P99 | Batch |
| --- | ---: | ---: | ---: | ---: |
| LLVM IR parse/verify, object emission, loading/preparation and drop | 13,703,800 | 15,816,800 | 17,059,000 | 1 |
| LLVM ctypes invocation, reused host frame | 3,259 | 3,658 | 4,771 | 256 |

Preparation excludes the Rust frontend/IR-emission subprocess. Warm calls
reuse their host buffers and include ctypes overhead, unlike the Rust
execution APIs that allocate independent frames. **Do not rank these numbers
as equivalent API-overhead measurements.** LLVM 20.1.8 must not be conflated
with rustc's LLVM 23.1.1; native Rust LLVM embedding and full-language LLVM
performance remain outside this milestone.

No claims yet for reference-count elimination, slab latency, incremental
compilation, FFI call performance, resident memory or sustained regression
thresholds. Those need separate equivalent-workload measurements.
