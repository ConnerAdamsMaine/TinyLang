# Native ASM backend

Status: **real Windows x86-64 scalar JIT implemented and tested; full-language
backend remains partial**.

The existing [decoded-opcode JIT](../../architecture.md) is retained. The new
experimental ASM backend consumes [ScalarFunction](../../../crates/tinylang_core/src/engine/ir.rs)
and is distinct from that executor.

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

No support is claimed yet for native function calls, globals, heap values,
foreign calls, recursive frames, SIMD, general register allocation, AOT
linking, debug/unwind metadata, or a shared native code cache.

The public scalar CLI selects `--backend asm`; selection must fail explicitly
on unsupported targets. Its input is source compiled and verified through
the existing frontend, never user-supplied machine code.

Executed test, protection and performance evidence is recorded in
[test report](V2_TEST_REPORT.md) and [baseline](V2_PERFORMANCE_BASELINE.md).
