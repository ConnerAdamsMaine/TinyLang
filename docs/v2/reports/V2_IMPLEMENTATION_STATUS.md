# TinyLang v2 implementation status

Mission 2, 2026-11-01. This is an incremental implementation, not certification
of the entire language roadmap.

| Subsystem | Status | Evidence and boundary |
| --- | --- | --- |
| Grammar and frontend | Implemented existing path | Existing compiler, resolver, verifier and dynamic values preserved; 59 runtime parity tests passed |
| General static typing and inference | Blocked | TLP-0001/2/3 are unpublished, unaccepted drafts; published roadmap retains erased generics |
| Checked scalar IR | Partially implemented | Immutable i64 function profile, CFG stack/type-flow inference and checked operations; Bool arithmetic/return coercions rejected |
| Interpreter and VM | Implemented existing path | Persistent VM, 4 fuel tests and 6 session tests passed; new scalar interpreter uses the shared IR |
| Native ASM | Partially implemented, tested | Real Windows x86-64 emission; loops, checked arithmetic/floor division, fuel, W^X and VM parity |
| Cranelift | Partially implemented, tested | Real optional Windows x86-64 JIT; same IR, VM parity, W^X and mapping release |
| LLVM | Partially implemented, tested | IR verified, compiled to AMD64 ELF objects and executed via optional LLVM 20.1.8; native Rust/COFF linkage not implemented |
| Runtime ownership | Partially implemented | Existing Ralloc/VM model preserved; 16 allocator tests passed; automatic RC and VM slab redesign not implemented |
| SDKs | Partially implemented, tested | Python executes real compiler artifacts through DLL; Node codec supports format 2; heap/aggregate marshalling incomplete |
| Standard libraries | Implemented existing path | 25 VM/decoded-JIT parity tests passed; no new `std`/`stx` features |
| Testing and performance | Implemented milestone | Default 73 / Cranelift 77 tests passed; 435 preserved checks, 25 SDK checks and 4 real LLVM tests; release baseline recorded |

## Specification conflicts and decisions

The mission presupposes accepted TLP specifications, static typing and automatic
reference counting. The local [proposal status](../../Community_Forum.md#unpublished-proposals)
and [roadmap](../../v2-roadmap.md) do not establish these contracts. Inventing
them would silently change existing programs. Native development is therefore
explicitly opt-in and restricted to already-defined checked i64 semantics.

The existing `jit` backend is a decoded-opcode executor, not ASM emission.
The new `asm` path is separate and must execute emitted instructions. No
unavailable backend may fall back while reporting native success.

The current VM returns Bool from comparisons and uses floor integer division.
The prior type prose described comparisons as integer 0/1 and has been
corrected to match execution semantics. Regression tests exposed and corrected the initial
scalar interpreter's truncating division and missing Bool eligibility checks.
No VM baseline was normalized to conceal these differences.

## Ownership and integration

The integration owner maintains compiler/IR/interpreter, selection, CLI, tests,
benchmarks and these reports. Three agents exclusively own ASM, external
backends, and SDK files. They share the established [IR contract](COMPILER_ARCHITECTURE.md).
The dirty working tree contains substantial pre-existing work; none is
reverted, committed or replaced by a clean checkout. Independent high-risk
review reported no significant issues; parent verification rebuilt and
executed the integrated code rather than relying on agent reports.
Missing standalone LLVM tools were resolved for execution validation using
an isolated pinned llvmlite wheel, not by pretending emission was execution.
The clang-specific test remains explicitly ignored because clang is absent.

Remaining milestones: accepted type/ownership proposals; typed HIR and general
inference; calls/globals/heap lowering; automatic retain/release optimization;
general SDK values and host registration; backend ABI integration; source-
level native debug metadata; architecture/platform expansion; sustained fuzz,
sanitizer and regression-performance runs.
