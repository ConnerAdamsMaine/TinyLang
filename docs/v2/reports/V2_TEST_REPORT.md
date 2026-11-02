# TinyLang v2 test report

Mission 2, 2026-11-01, x86_64 Windows MSVC.

## Executed baseline and preserved behavior

```powershell
cargo test --manifest-path .\tinylang_v2\Cargo.toml --locked
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

Initial [engine contract](../../../tinylang_v2/tests/engine_contract.rs): missing engine
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
cargo test --manifest-path .\tinylang_v2\Cargo.toml --all-features --locked
cargo clippy --manifest-path .\tinylang_v2\Cargo.toml --all-targets --all-features --locked -- -D warnings
rustup run stable cargo fmt --manifest-path .\tinylang_v2\Cargo.toml -- --check
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

## Limits of evidence

No passing build is treated as runtime proof. No full-language native parity,
general static type fuzzing, multi-architecture JIT certification, sustained
coverage-guided fuzz campaign, sanitizer run or physical hardware test is
claimed. Existing fuzz targets and adversarial ABI mutation tests remain
available but are not a substitute for those missing campaigns.
