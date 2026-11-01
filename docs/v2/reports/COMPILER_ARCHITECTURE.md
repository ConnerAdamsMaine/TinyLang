# TinyLang v2 compiler architecture

## Mission 2 contract and implemented milestone

The published [v2 roadmap](../../v2-roadmap.md) preserves dynamic runtime
values, erased generics and the existing compiler. [Proposal status](../../Community_Forum.md#unpublished-proposals)
identifies TLP-0001, TLP-0002 and TLP-0003 as unpublished, unaccepted drafts.
No new source typing, implicit conversion or reference-counting semantics
are authorized by those documents.

This milestone adds an experimental checked-i64 function execution profile
inside the standalone ABI crate. The existing lexer, parser, module resolver,
optimizer and verifier remain authoritative. Native engines consume an
immutable, verified scalar IR, not independently parsed source.

Alternatives considered: rewriting the frontend would duplicate semantics;
calling the decoded-opcode executor a native JIT would misrepresent its
implementation. A bounded scalar lowering is the first executable native
vertical slice, not a general replacement compiler.

### Dependency-aware execution

1. Integration owner: reconcile specifications and establish the IR contract.
2. Integration owner: test and implement lowering, CFG stack-height inference,
   checked interpreter and explicit errors.
3. Native owner: emit x86-64 machine code from that contract with W^X,
   checked arithmetic, instruction fuel and independent differential tests.
4. External-backend owner: lower the same contract to Cranelift and LLVM;
   declare tool/platform limits explicitly.
5. SDK owner, independent of IR: support compiler-binary format 2 while
   preserving format-1 defaults; execute actual compiler artifacts via the DLL.
6. Integration owner: validate existing ABI/VM/allocator behavior, run cross-
   backend tests and release benchmarks, commission independent high-risk
   review, and update the nine mission reports.

No worktree checkout can include the substantial uncommitted local v2 work.
Agents use exclusive file ownership in the existing working tree. Existing
changes are not reverted, bulk-formatted or committed.

### Shared IR contract

`ScalarFunction::lower(&VerifiedProgram, function_index)` produces a checked
capability. Its getters expose name, parameter count, slot count, maximum
operand-stack depth, instructions and per-instruction entry stack heights.
Unreachable instructions have no height. Only i64 constants, local loads and
stores, checked arithmetic, integer comparisons, jumps,
conditional jumps, pop and return are supported. Comparisons produce Bool,
represented internally as machine 0/1, not language integers. All other opcodes fail
explicitly, including calls, globals, heap values, bool and float literals.

`Instruction` variants: `Constant(i64)`, `Load(usize)`, `Store(usize)`,
`Binary(BinaryOp)`, `Negate`, `Jump(usize)`, `JumpIfZero(usize)`, `Pop`, `Return`.
`BinaryOp`: `Add`, `Subtract`, `Multiply`, `Divide`, `Less`, `LessEqual`,
`Greater`, `GreaterEqual`, `Equal`, `NotEqual`.
Every executed IR instruction consumes one fuel unit before doing work.
Integer division rounds down, matching the VM, including negative operands.
Arithmetic traps on signed-i64 overflow and divide-by-zero. Argument arity
must match; local slots start at zero. No interpreter fallback is permitted
inside a backend reporting native execution.

Finite type-flow analysis propagates I64/Bool states through slots, operand
stacks and CFG joins. Arithmetic, negation, comparison inputs and returns must
be I64; branches may consume I64 or Bool. Joined types cannot silently coerce
Bool into arithmetic or an integer result. An explicit 4,194,304-cell analysis
budget supplements the 4,096-instruction/slot/stack bounds.

`EngineError` distinguishes invalid IR, unsupported instructions, unknown
functions, arity errors, backend availability, allocation/codegen failures
and located traps. `TrapKind` distinguishes overflow, division by zero and
fuel exhaustion. Function and bytecode-instruction locations are preserved;
source-located general typing diagnostics remain outside this profile.

Acceptance verified: real compiled function execution, native/VM value and trap parity,
finite loop fuel, malformed-CFG rejection, unchanged ABI tests, SDK artifact
integration, and measured cold/warm costs without unsupported speedup claims.

## Backend capabilities

| Backend | Execution | Validated target | Boundary |
| --- | --- | --- | --- |
| Core VM and decoded-opcode executor | Existing full language | Existing core test host | Existing semantic reference; not new machine-code emission |
| Scalar interpreter | Shared checked IR | Windows x86-64 | I64 arguments/results and Bool control flow |
| ASM | Real emitted machine code | Windows x86-64 | Other targets explicitly unavailable |
| Cranelift | Real emitted machine code, optional feature | Windows x86-64 | Unix mapping code untested; other ISAs unavailable |
| LLVM | CLI IR emission; generated objects executed in optional validation | Windows x86-64, LLVM 20.1.8 | No linked Rust LLVM executor or native COFF/executable pipeline |

`ExecutionBackend::prepare` retains a `PreparedFunction`; invocations share
compiled code but allocate independent bounded frames. This is explicit
prepared-function reuse, not an automatic program-wide code cache.
`tinylang-v2` selects interpreter, ASM, Cranelift or LLVM emission without
changing the existing CLI or C ABI. LLVM's generated scalar frame interface
is `i32 tinylang_scalar(slots, stack, output, fuel, error_instruction)`, with
five pointers to host-owned storage; it is not the frozen language C ABI.
Status codes are 0 success, 1 overflow, 2 division by zero and 3 fuel exhaustion.
The optional LLVM test compiles this IR into real AMD64 ELF objects, loads
those objects through LLVM MCJIT and compares execution with the actual VM.
See [LLVM tool configuration](LLVM_CRANELIFT_INTEGRATION.md).

See [test evidence](V2_TEST_REPORT.md), [performance](V2_PERFORMANCE_BASELINE.md)
and [remaining mission scope](V2_IMPLEMENTATION_STATUS.md).
