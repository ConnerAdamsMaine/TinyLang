---
title: TinyLang v2 Roadmap
---

TinyLang is now in the v2 language generation. V2 expands the language
core while preserving the existing VM, JIT, Ralloc-backed heap, bytecode
verifier, and VM/JIT parity requirements.

# V2 language commitments

## First-class functions and closures

Function declarations are values: referencing a top-level function
produces a first-class function value, and a function-valued variable
can be called with ordinary call syntax.

Closures are heap values containing a function identity and captured
values. `closure_new(function_name, captures)` constructs one, and
calling the result prepends its captures to the explicit arguments. The
bytecode surface is `PUSH_FUNCTION` plus `CALL_VALUE`; both the
interpreter and JIT implement the same dispatch and call-depth checks.

Anonymous functions use `fn(parameters) { ... }` expressions, with the
same optional parameter and return annotations as named functions.
Free local bindings are captured by value, and nested closures forward
captures through intermediate closures when needed. Reassigning a captured
local changes the closure's invocation-local copy; top-level bindings
remain live global accesses. Capturing a heap-valued variable copies its
value/handle rather than deep-cloning the referenced object.
Runtime errors in source closure bodies retain their source locations
across VM and JIT execution.

V2 follow-up work includes richer closure diagnostics. The runtime
representation and indirect-call ABI remain unchanged.

## Generic functions

V2 accepts generic function declarations:

    fn identity<T>(value: T) -> T { return value }

Generic parameters are recorded in `Function` metadata and bytecode
artifacts. They are erased during execution because TinyLang values
remain dynamically typed; one function body serves every runtime
instantiation. V2 follow-up work includes generic constraints,
explicit-instantiation syntax, and static type checking where those
constraints provide value.

# Implementation tracks

- Continue closure diagnostic coverage and preserve parity for source-level
  closures, captured calls, nested captures, and module-returned closures.
- Define generic constraints and type-argument diagnostics without
  weakening runtime safety.
- Keep `Program` fingerprints, artifacts, verifier rules, VM, JIT, and
  FFI representations synchronized as the language grows.
- Maintain parity tests for direct calls, function values, captured
  calls, generic declarations, artifacts, and both execution backends.
- Preserve the frozen v1 ABI on `release/v1`. The renamed JSON C exports on
  `main` declare ABI version 2; the experimental binary ABI stays separate.

# V2 status

The first v2 slice is implemented: first-class top-level function
values, source-level closure literals with lexical captures, callable
heap closures, erased generic declarations, generic metadata in
artifacts, verifier support, and VM/JIT parity coverage.
