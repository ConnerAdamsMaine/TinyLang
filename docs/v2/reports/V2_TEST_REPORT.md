# TinyLang v2 test report

Mission 2, 2026-11-01, x86_64 Windows MSVC.

## Executed baseline and preserved behavior

```powershell
cargo test -p tinylang-v2 --locked
cargo test -p tinylang --lib --locked
cargo test -p tinylang --test runtime_parity --test instruction_fuel --test persistent_vm --test abi_api_soundness --test cache_retention --test stdlib_parity --locked
cargo test -p tinylang_ralloc --test native_box --test initialization --test fragmentation --locked
```

Initial standalone ABI baseline: 42 passed, 1 manual benchmark ignored.
Core library: 270 passed, 11 existing manual diagnostics ignored.
Focused core: 149 passed (44 ABI, 11 cache, 4 fuel, 6 persistent VM, 59 parity,
25 standard-library parity).
Allocator: 16 passed (3 fragmentation, 3 initialization, 10 native ownership).
Existing workspace missing-lints-inheritance manifest warnings remain; this
milestone does not modify those unrelated manifests.

## New acceptance

Initial [engine contract](../../../crates/tinylang_core/tests/engine_contract.rs): missing engine
API failed before implementation; first four tests passed after implementation.
The tests directly cover compiled source, typed eligibility, errors and CFG
validation. VM differential loops and exact fuel are included afterward.

Execution-selection and CLI tests were run before implementation and exposed
the missing backend/CLI APIs. Public VM reference tests use
`RuntimeValue` and `VM::from_verified_with_limits`.

Final integrated results:

| Check | Result |
| --- | --- |
| Full standalone default suite | 73 passed, 2 explicitly ignored |
| Full standalone with Cranelift | 77 passed, 2 explicitly ignored |
| Strict Clippy, all targets/features, `-D warnings` | Passed without warnings |
| Stable standalone formatting check | Passed |
| Python, rebuilt actual DLL/compiler artifact | 10 passed, no skips; add(19,23)=42 and full lifecycle |
| Node, actual compiler artifact codec | 15 passed, no skips; native FFI explicitly unavailable |
| Optional LLVM library/object execution | 4 passed, no skips on Windows x86-64; 27 emitted-object invocations against the actual VM |
| Release CLI, persistent example | ASM add=42; Cranelift sum(100)=4950; LLVM emits frame IR |
| Independent high-risk code review | No significant issues reported |

The two ignored standalone tests are the existing manual execution benchmark
and the new LLVM execution gate requiring external clang. Explicitly running
the LLVM gate fails with a missing-tool diagnostic; no LLVM execution is
claimed from that ignored clang test. A separate real LLVM 20.1.8 path was
subsequently installed locally and passed; see below. The 73/77 suites
overlap and must not be added together.

```powershell
cargo test -p tinylang-v2 --all-features --locked
cargo clippy -p tinylang-v2 --all-targets --all-features --locked -- -D warnings
rustup run stable cargo fmt -p tinylang-v2 -- --check
```

Native tests run in bounded integration executables, not production-sensitive
inputs. The ASM suite has eight tests; the pipeline includes 64 generated
programs plus direct loops, branches, generic bodies and artifact reloads.
Windows mapping tests verify RW/RX protections and Cranelift unmapping.

Regression findings were fixed rather than normalized away: invalid scalar
`set` fixtures now use the established plain assignment syntax; floor-division
and Bool eligibility tests failed before their fixes and pass afterward.
The type-flow budget test actually runs; nested-test and lint issues were
corrected before final validation.

The reviewer did not independently execute LLVM, non-Windows paths, SDK DLL
lifecycle or performance measurements. Parent fresh builds/tests cover the
Windows SDK and subsequent LLVM object-execution results above; those exclusions are not expanded
into unsupported certification.

Exact SDK rebuild/env/test commands are maintained in
[SDK artifact integration](../../ffi/v2-sdk-artifacts.md). The parent rebuilt
the DLL and regenerated the compiler artifact rather than using a stale DLL.

## Real LLVM execution after resolving the missing-tool blocker

The first optional-library import failed, and the bootstrap interpreter had
no pip. A new isolated test venv restored pip locally; llvmlite 0.46.0 then
provided LLVM 20.1.8 without a system LLVM install. The initial object-format
test failed because this binding selects ELF on Windows, not COFF. Runtime
target-machine inspection established that behavior; tests now validate
the actual declared ELF target rather than calling it COFF.

All four final tests pass with actual object compilation/loading/execution
and RX-page checks. They include LLVM status codes (overflow 1, division-zero
2, fuel 3), original instruction locations and unchanged failure output.
The actual core VM, not a normalization helper, supplies differential results.
Python syntax validation passed. Reproduction commands and dependency pin
are in [LLVM integration](LLVM_CRANELIFT_INTEGRATION.md).

## Program-profile call-chain parity (executed 2026-11-10)

`engine_program_native_parity.rs` runs 12 differential tests in which the
native program image, the scalar interpreter and the authoritative VM (via
fixtures mirroring `engine_program_parity.rs`) must agree exactly: results,
the one shared minimum fuel budget, trap categories and failing instruction
indexes, including a fuel 1..=67 sweep asserting identical trap traces for a
non-terminating recursive fixture, and the depth boundary where the 17th
nested call traps. 12/12 pass.

The optional Cranelift backend executes the same program profile with the
same fixtures in `engine_program_cranelift_parity.rs` (compiled under
`--features cranelift`); 11/11 pass. Its lowering shares the verified
binary/floor-division lowering with the single-function profile through a
trap-sink trait, so trap categories cannot drift between profiles. Without
the feature the same backend reports `BackendUnavailable` explicitly; the
gap test covers both configurations.

| Check | Result |
| --- | --- |
| Program native parity suite (`engine_program_native_parity`) | 12 passed, 0 failed |
| Program Cranelift parity suite (`engine_program_cranelift_parity`, feature `cranelift`) | 11 passed, 0 failed |
| Full default regression (50 suites) | 671 passed, 0 failed, 15 ignored |
| Full regression with `--features cranelift` (50 suites) | 686 passed, 0 failed, 15 ignored |
| Release CLI program fallback, executed end-to-end | `asm` fact(5)=120, `interpreter` fact(10)=3628800; release `--backend cranelift` fact(5)=120, fact(10)=3628800, fact(18) traps `Call stack overflow after 16 nested call(s) at instruction 10`; `llvm` emits program-profile IR (see the 2026-11-10 milestone section below, which supersedes the earlier gap report) |
| Strict clippy, all targets, `-D warnings` (2026-11-10 re-run) | Passed without warnings in both build configurations |
| Formatting on all files touched 2026-11-10 | `rustfmt --check` clean (pre-existing diffs elsewhere in the restructured tree are untouched) |
| Emitted-image independent byte review | 654-byte `fact` image decoded by hand: call rel32 resolves to the body offset (cue skipped), every trap edge resolves, stub order overflow=1, division-zero=2, fuel=3, depth=4, propagate |

## LLVM program-profile milestone (2026-11-10)

The LLVM backend gained whole call-closure emission (`llvm/program.rs::
emit_program_llvm_ir`) mirroring the Cranelift program ABI exactly. Two
defects found by executed tests were fixed rather than normalized: the shared
leaf `binary` helper originally emitted leaf-profile trap blocks into program
IR (the structural tests caught tag-1/2 status codes written to
`%error_instruction` instead of the context tag field), so trap emission now
routes through the shared `TrapSink` trait with a program-profile sink; and
header comments initially carried a leading space that broke the harness's
entry-symbol regex.

| Check | Result |
| --- | --- |
| Program emission structural tests (`llvm_program_emission`) | 6 passed, 0 failed |
| Full default regression | 676 passed, 0 failed, 15 ignored |
| Full regression with all features | 712 passed, 0 failed, 28 ignored |
| Program parity suites (native / Cranelift / engine) | 12/12, 11/11, 12/12 |
| Strict clippy, all targets/features, `-D warnings` | Passed without warnings |
| `rustfmt --check` on touched files | Clean |
| Real llvmlite execution (`test_llvm_execution.py`, isolated venv, LLVM 20.1.8) | 7 passed: 4 leaf + 3 program-profile (recursion/zero-arg parity with the real VM, trap tags 1/2/4, shared-fuel boundary min-1→tag 3/min→success, RX-page check) |
| Release CLI, `--backend llvm` on a call closure | Emits program IR with `; entry:` header, all closure members and cross-call sites |

Two emitter defects found by these executed tests were fixed rather than
normalized: cross-call frame restore encoded `r9` where `r8` was intended, and
the program image initially kept its invocation context in nonvolatile RDI,
clobbering the host caller (observed as `STATUS_ACCESS_VIOLATION`,
`0xc0000005`). The context now lives in volatile R10. A follow-up modrm slip
in the re-encoded R10 bytes reproduced the same crash until every byte was
re-derived from the verified single-function encodings; the parity suite
pinned both failures before the fixes landed.

## Limits of evidence

No passing build is treated as runtime proof. No full-language native parity,
general static type fuzzing, multi-architecture JIT certification, sustained
coverage-guided fuzz campaign, sanitizer run or physical hardware test is
claimed. Existing fuzz targets and adversarial ABI mutation tests remain
available but are not a substitute for those missing campaigns.
