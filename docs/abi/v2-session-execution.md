# Persistent execution prerequisite

Measured on Windows x64 with rustc 1.99.0, MSVC and the i7-8850H host described
in the baseline report. This experiment selects session ownership, not the
foreign invocation representation. VM_INVOKE still returns UNKNOWN_OPCODE.

The existing consuming Rust execution API cannot preserve globals and heap
objects for subsequent foreign calls. The new nonconsuming methods restart
main at instruction zero or call an internal function index. They share fuel
across nested calls, restore global memory on ordinary errors, and retain
effects completed before a trap. They do not resume a suspended instruction
pointer. A panic requires discarding the VM. Internal function indices and
heap references are not stable foreign identifiers.

## Candidates and evidence

`crates/tinylang_core/experiments/session-execution` implements four configurations:

- Baseline reconstructs and verifies a VM on every invocation, matching the
  limitation of a consuming execution interface. It is a synthetic Rust
  comparison, not the currently unimplemented foreign opcode.
- A owns the VM directly and requires exclusive access from its caller.
- B owns it behind a mutex, testing whether local synchronization is cheap
  enough to incorporate into every call.
- C dedicates a worker thread to each session, using bounded request/reply
  channels to serialize execution.

All compile and pass the same fixture, with baseline's reset behavior explicitly
distinguished from the retained state of A/B/C. The fixture increments a global
and halts. Fuel 4 traps after the store; fuel 5 completes. Every timed iteration
asserts the result and exact global state. Each case has 100,000 warm-up calls
and seven measured samples of 100,000 calls. Assertions and result inspection
are included in timings; serialization and C ABI entry are absent.

| Measurement | Baseline | A | B | C |
| --- | ---: | ---: | ---: | ---: |
| Success median ns/call | 1408.203 | 244.653 | 251.316 | 1022.921 |
| Success min–max ns/call | 1348.636–1523.391 | 232.872–271.695 | 242.442–283.794 | 1004.933–1063.918 |
| Trap median ns/call | 1428.333 | 321.800 | 351.319 | 992.990 |
| Trap min–max ns/call | 1417.407–1505.656 | 309.477–430.703 | 325.973–569.183 | 910.457–1211.810 |
| Success throughput, million calls/s (reciprocal median) | 0.710 | 4.087 | 3.979 | 0.978 |
| Final-crate release build, ms (one observation) | 780.744 | 763.633 | 967.312 | 2243.890 |
| Linked executable, bytes | 700416 | 700416 | 701440 | 762368 |
| Size change from baseline | — | 0% | +0.146% | +8.845% |
| Retains state | No | Yes | Yes | Yes |
| Ownership complexity | Reconstruct each call | Exclusive value | Lock/poison handling | Thread/channels/shutdown |
| Foreign ABI change in experiment | None | None | None | None |
| Allocation count / peak memory / copying | Unavailable | Unavailable | Unavailable | Unavailable |
| Library size / initialization cost | Unavailable | Unavailable | Unavailable | Unavailable |

Success latency reductions relative to reconstruction are 82.63%, 82.15%, and
27.36% for A/B/C using `(baseline - candidate) / baseline`. Because baseline
does not retain state and performs verification on each call, these are not
equivalent-semantics FFI speedup claims. Among equivalent persistent candidates,
A has the lowest median; A/B ranges overlap. No statistically established
mutex penalty is claimed. C's observed channel overhead and additional binary
size are not justified for this small synchronous workload. Contended access,
worker startup and long-running programs remain unmeasured.

Raw samples: [baseline](evidence/session-execution-baseline.csv),
[A](evidence/session-execution-a.csv), [B](evidence/session-execution-b.csv),
[C](evidence/session-execution-c.csv), [builds](evidence/session-execution-build.csv).

## Selection and integration

Select A for session storage. Registry access must subsequently provide an
exclusive lease without holding the registry mutex throughout execution;
this experiment does not establish concurrent foreign-call safety.
VM_CREATE now allocates global memory and owns the persistent VM, retaining
its verified program after the program handle is released. It does not run
main. Inline ownership avoids another infallible Box allocation, but increases
the bounded 256-slot registry's static storage. Peak memory was not measured.

Validated `VmExecutionLimits` install requested heap bytes, object count and
call depth within existing hard caps. Substituting those fields into existing
allocation and call-depth checks is a mechanically necessary compatibility
adjustment, not an allocator redesign. Array growth checks the same byte limit
before mutation. Fuel does not bound native builtins, threads, JIT or wall time;
foreign invocation must restrict unsupported host operations before exposure.

Six persistent-VM tests pass in debug and release, covering retained globals,
trap effects, surviving heap results, object/byte limits, rejected array growth,
call-depth recovery and invalid limits. The four fuel tests, 59 runtime-parity
tests, 42 v1 ABI tests and 28 v2 release tests pass. Core and v2 Clippy pass with
warnings denied; v2 formatting check passes.

The [post-integration A run](evidence/session-execution-integrated.csv) reports
success median 231.383 ns (226.356–232.569) and trap median 305.892 ns
(296.392–325.371), again seven samples. Executable size remains 700416 bytes.
The 37.81-second rebuild includes changed runtime dependencies and cannot be
compared with the final-crate build observations above.

## Reproduction

Warm dependencies with a release build first. For each feature `baseline`,
`a`, `b`, `c`, execute the following commands substituting FEATURE. Use only
one feature at a time. Time the `cargo rustc` process externally; the unused
cfg distinguishes that final-crate invocation from the warm-up build.

```text
cargo test --locked --manifest-path crates/tinylang_core/experiments/session-execution/Cargo.toml --target-dir target/session-execution --no-default-features --features FEATURE
cargo rustc --release --locked --manifest-path crates/tinylang_core/experiments/session-execution/Cargo.toml --target-dir target/session-execution --no-default-features --features FEATURE -- --cfg session_measurement
target/session-execution/release/session-execution.exe
cargo test -p tinylang --release --test persistent_vm
```

Release configuration is opt-level 3, one codegen unit, LTO disabled. Builds
are sequential and share warm dependencies. This report does not complete
the separate three-way foreign argument/result conversion experiment.

## Registry integration follow-up

The direct-owner integration now moves the VM out of its registry slot before
execution and restores it afterward. Explicit `VmRunning` and `VmFailed`
payload states distinguish exclusive execution from a quarantined session.
Release of a running session returns BUSY, as does another invocation of that
session. The runtime remains live and cannot be destroyed while it owns the
session. Execution holds no registry mutex, allowing another session and
unrelated registry operations to proceed, including reentrant access.

An ordinary error restores the VM with completed effects. A caught unwind
discards the VM, returns PANIC and leaves a failed handle that rejects further
execution with INTERNAL but permits release. Process-aborting failures are not
recoverable through this mechanism. Object destruction now occurs after the
slot is invalidated and the registry lock is released.

Three additional tests exercise nested entry, an executing worker racing
release/invoke, and injected panic isolation. The full release suite passes
31 tests; the optional benchmark is ignored by default. This is registry wiring
of the selected A ownership strategy, not another value representation or
foreign entry point. The dispatcher still does not expose VM_INVOKE.

An integrated Rust-path measurement runs push/store/halt through registry
checkout, panic containment and restoration. After 100,000 warm-up iterations,
seven samples of 100,000 iterations yield median 222.380 ns/call, range
219.157–296.730. Raw samples are in
[session-registry-execution.csv](evidence/session-registry-execution.csv).
The workload has three instructions rather than the earlier five-instruction
increment fixture; do not subtract these timings to estimate lock overhead
or compare them as a speedup. No serialization or C ABI entry is measured.

```text
cargo test --release --locked -p tinylang-v2 --target-dir target/abi-v2 --lib measure_checked_execution -- --ignored --nocapture --test-threads=1
```
