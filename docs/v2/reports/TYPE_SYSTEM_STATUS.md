# TinyLang v2 type system

Status: **partially implemented scalar IR; general static typing blocked**.

The existing language is dynamically typed. Generic function declarations
record parameters but erase them at runtime. [The roadmap](../../v2-roadmap.md)
does not define a general static checker, unification rules, nullability or
trait constraints. TLP-0001/2/3 are unpublished working drafts.

The new native profile requires signed-i64 arguments and results. Integer
arithmetic is checked. Comparisons produce Bool for control flow, represented
internally as machine 0/1 without changing its language type. It rejects
unsupported value opcodes rather than casting them.
CFG analysis infers operand-stack heights, rejects underflow and unequal
joins, bounds frames, verifies jump targets and disallows reachable falloff.
Finite type-flow inference propagates I64/Bool through stack values and local
slots; CFG joins conservatively union possible types. Arithmetic, negation,
comparison inputs and returns require I64. Bool and joined I64/Bool cannot
silently enter integer operations. Branches accept either domain. Analysis
is limited to 4,194,304 cells; integer division preserves VM floor semantics.

This is a backend eligibility/type contract, **not source type inference**.
It neither accepts new annotations nor rejects otherwise valid dynamic
programs outside an explicitly requested native profile. Existing source
compile errors retain the compiler's diagnostics; profile errors identify
the function and bytecode instruction. General source-located typed-HIR
diagnostics and cross-module type resolution are not implemented.

Tests: 9 [engine contract](../../../crates/tinylang_core/tests/engine_contract.rs) tests cover scalar
lowering, heap rejection, CFG joins, checked errors, VM loop parity and exact
instruction fuel. [Execution pipeline](../../../crates/tinylang_core/tests/execution_pipeline.rs)
adds 64 deterministic generated integer functions shared by execution backends,
generic-body and artifact execution, and legitimate Bool branch flow.
Six [adversarial tests](../../../crates/tinylang_core/tests/scalar_adversarial.rs) exercise
malformed bytecode, unsupported values, zero-stack loops and analysis limits.
