# TinyLang v2 benchmark regressions

Status: **baseline recorded, no regressions investigated**, 2026-11-08.

## Baseline

The recorded baseline is the 2026-11-01 release run in
[V2_PERFORMANCE_BASELINE.md](V2_PERFORMANCE_BASELINE.md). All current
measurements were re-executed 2026-11-08 on the same host (Intel Core
i7-8850H, x86_64 Windows MSVC, Rust 1.100.0-nightly) with the same
`sum(100) = 4950` checked workload, 101 samples after 8 warmups.

## Current measurements (2026-11-08)

| Metric | Median ns | P95 ns | P99 ns | Batch |
| --- | ---: | ---: | ---: | ---: |
| Lexing | 3,442 | 7,253 | 8,656 | 256 |
| Frontend and scalar lowering | 19,700 | 40,800 | 61,600 | 1 |
| Scalar lowering alone | 6,251 | 9,126 | 10,348 | 256 |
| Existing VM warm invocation | 24,312 | 34,350 | 41,597 | 256 |
| Scalar interpreter preparation | 200 | 300 | 300 | 1 |
| Scalar interpreter warm invocation | 2,410 | 5,616 | 10,895 | 256 |
| ASM preparation | 8,900 | 13,800 | 18,600 | 1 |
| ASM warm invocation | 869 | 1,109 | 1,642 | 256 |

## Comparison against baseline

The 2026-11-01 baseline was recorded with a Cranelift-enabled release build.
The 2026-11-08 run used a default (no-Cranelift) release build. The two are
**not directly comparable**: the baseline includes Cranelift codegen weight in
the binary and may have different CPU frequency/cache state. No before/after
percent differences are computed from these two runs because the build
configuration differs.

| Metric | 2026-11-01 (Cranelift) | 2026-11-08 (default) | Comparable? |
| --- | --- | --- | --- |
| Lexing | 3,725 | 3,442 | No (different build config) |
| Frontend and lowering | 55,900 | 19,700 | No (different build config) |
| Scalar lowering | 6,133 | 6,251 | No (different build config) |
| VM warm | 19,476 | 24,312 | No (different build config) |
| Scalar interpreter warm | 2,503 | 2,410 | No (different build config) |
| ASM warm | 893 | 869 | No (different build config) |

## Investigated regressions

**None investigated.** No material regression has been identified between the
2026-11-01 baseline and the 2026-11-08 re-measurement. The runs differ in
build configuration (Cranelift-enabled vs default), so no silent rebaselining
has occurred. A controlled re-baseline with identical build configuration is
required before any percent difference can be asserted.

## Confidence and variance

- Sample count: 101 per metric, 8 warmups per metric.
- Reporting: median, P95, P99. P99 is the maximum observed value in this
  sample set, not a sustained tail-latency guarantee.
- CPU scheduling, frequency scaling, and OS cache state were not controlled.
- This is a single-host, single-workload measurement. It is not a regression
  gate and does not establish a performance floor for the language.

## Material regression procedure

1. Record absolute values and percent difference against the baseline.
2. Save baseline artifacts before overwriting.
3. Investigate the cause; never silently overwrite or rebaseline an
   unfavorable result.
4. If a real regression is justified by correctness or a documented tradeoff,
   explain and quantify that tradeoff.

## Related

- V2_PERFORMANCE_BASELINE.md: release-optimized baselines
- V2_PROFILING_REPORT.md: profiler findings and methodology
- BUILD_PROFILE_POLICY.md: build gates and -j 6 enforcement
