# Native ASM backend

Status: **real Windows x86-64 scalar JIT implemented and tested; full-language
backend remains partial**.

The existing [decoded-opcode JIT](../../architecture.md) is retained. The new
experimental ASM backend consumes [ScalarFunction](../../../crates/tinylang_core/src/execution/engine/ir.rs)
and, for whole call closures, [ScalarProgram](../../../crates/tinylang_core/src/execution/engine/program.rs);
it is distinct from that executor.

Verified acceptance: real x86-64 emission; checked arithmetic and floor-division
guards; explicit branches/loops; instruction fuel; argument arity and i64
calling convention; bounded host-owned frames; owned executable mappings;
RW-to-RX transition without persistent WX; per-instruction error reporting;
repeated-invocation and VM differential tests. The eight dedicated ASM tests
run in a 30-second-bounded isolated executable. The integrated pipeline also
executes 64 generated source functions, branches, loops, erased-generic bodies
and compiler-artifact round trips.

The emitter uses a leaf frame-pointer calling convention and volatile
registers, with bounded host-owned slots/stack rather than large native-stack
allocations. Branch displacements are patched before publication. Native
instructions perform arithmetic and control flow; no interpreter dispatch
or arithmetic callback is substituted.

Windows executable storage uses VirtualAlloc RW, VirtualProtect RX and an
instruction-cache flush. VirtualQuery tests observed RW `0x04` and RX `0x20`;
no temporary or persistent WX is used. Prepared functions own mappings and
release them on drop. Other targets report unavailable rather than fallback.

No support is claimed yet for globals, heap values, foreign calls, calls
outside one lowered program closure, SIMD, general register allocation, AOT
linking, debug/unwind metadata, or a shared native code cache.

## Program profile: whole-call-chain images (executed 2026-11-10)

`ExecutionBackend::NativeAsm::prepare_program` emits one Windows x86-64 image
for a lowered whole call closure (`ScalarProgram::lower`). The image carries a
host-owned frame bank of `(depth_limit + 1)` segments indexed by call depth;
one instruction-fuel register is threaded through every call; call-depth
budget, trap categories and failing instruction indexes match the reference
interpreter exactly, including per-fuel trap-trace equality.

Calling convention: volatile registers only; R10 carries the per-invocation
context (depth, trap, entry fuel, frame bank, failing instruction), R8/R9 the
current frame's slots/stack bases, R11 remaining fuel; the hardware stack
carries only return addresses. The image preserves all nonvolatile registers
and RSP. Host entry goes through an 11-byte cue (`endbr64`, `mov r10, rcx`,
fuel load); internal calls jump straight to bodies, so a nested entry skips
the cue.

Two emitter defects were found and fixed through executed differential
testing, not inspection alone. First, two ModRM bytes encoded `add r9, rax` /
`mov r9, rax` where `r8` was intended, so restored callee frames pointed at
the wrong bank segment. Second, the first program image kept its context in
RDI, a nonvolatile register on Windows x64, clobbering the host caller's
RDI; this was observed directly as `STATUS_ACCESS_VIOLATION` (`0xc0000005`)
in a Rust test caller. The context now lives in volatile R10. Re-encoding
every context access from the verified single-function encodings then exposed
a follow-up modrm slip in the unverified R10 bytes (wrong reg fields and
missing `REX.B` on `[r10]` cmp/mov forms), which reproduced the same crash
until the corrected bytes landed. The final image was independently verified
byte-by-byte: the recursive call's rel32 resolves to the body offset (cue
skipped), every trap edge resolves to its stub, and stubs order
overflow=1, division-zero=2, fuel=3, depth=4, propagate.

Evidence: `engine_program_native_parity.rs`: 12/12 differential tests against
the scalar interpreter and the authoritative VM (argument order, depth
boundary 16/17, mutual recursion via raw bytecode, zero-argument wrapper
cost, trap categories, identical trap traces at every fuel level including a
1..=67 sweep of the non-terminating fixture).

The optional Cranelift backend executes the same program profile
(`cranelift/program.rs`, under `--features cranelift`). It mirrors the ASM
profile's semantics rather than its registers: one `repr(C)` invocation
context (`depth` u64 @0x00, `trap` u32 @0x08, `fuel` u64 @0x10, bank pointer
@0x18, `pc` u64 @0x20; trap tags overflow=1, division-zero=2, fuel=3,
depth=4) reaches every emitted closure member, each frame derives its
slots/stack bases from a uniform bank segment addressed by its call depth,
and each call checks `depth + 1 > limit` before writing the callee's segment
or branching. Arguments leave in operand-stack order (the deepest value is
argument zero), a callee trap propagates through the caller without touching
the caller's trap tag or failing instruction, and `Return` hands the context
depth back to the caller. The context pc is recorded before each instruction
can trap and the shared budget is one fuel unit per executed instruction
across the whole chain, so failing instruction indexes and fuel minima equal
the interpreter's. The checked binary/floor-division lowering is shared with
the single-function profile through a trap-sink trait, so trap categories
cannot drift between profiles. Unlike the ASM image this backend is not
Windows-bound: its gating is `target_arch = "x86_64"`, checked at runtime
like the single-function profile.

Evidence: `engine_program_cranelift_parity.rs`: 11/11 differential tests
with the same fixtures as the native suite. Full regression executed
2026-11-10: default 671 passed / 0 failed / 15 ignored; `--features
cranelift` 686 passed / 0 failed / 15 ignored; 50 suites each. LLVM's
program profile is now implemented as emission (`llvm/program.rs`) and is
covered by structural tests (`llvm_program_emission`) and real llvmlite
execution parity (`test_llvm_execution.py`); see the LLVM section of
LLVM_CRANELIFT_INTEGRATION.md.

The public scalar CLI selects `--backend asm`; selection must fail explicitly
on unsupported targets. Its input is source compiled and verified through
the existing frontend, never user-supplied machine code. Executed
end-to-end on 2026-11-10, this exposed and fixed a wiring defect: the CLI
escalated a leaf `CALL` rejection to the program profile only when the error
opcode string was `"CALL"`, but scalar lowering emits the Debug variant name
`"Call"`, so every program run failed before the fix. A CLI test
(`cli_program_fallback_executes_call_closures_and_reports_missing_backends`)
now executes the recursive fixture through the interpreter and ASM program
paths; its Cranelift expectation is feature-aware, asserting fact(5)=120
when the feature is compiled in and the explicit `unavailable` diagnostic
otherwise. LLVM previously reported the gap for programs with direct calls;
that changed with the program-profile emitter (see LLVM's section in
LLVM_CRANELIFT_INTEGRATION.md), and the CLI test now asserts program
emission. Executed end-to-end with the
release Cranelift CLI: fact(5)=120, fact(10)=3628800, and fact(18) fails
with `Call stack overflow after 16 nested call(s) at instruction 10` on
stderr, exit code 1.

Executed test, protection and performance evidence is recorded in
[test report](V2_TEST_REPORT.md) and [baseline](V2_PERFORMANCE_BASELINE.md).
