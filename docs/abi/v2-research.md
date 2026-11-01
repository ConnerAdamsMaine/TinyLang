# ABI research notebook

Initial documentation review, 2026-10-11. These are design inputs, not executed
external benchmarks or proof of TinyLang performance. Performance and size
implications below are engineering hypotheses unless explicitly stated.
Further implementation inspection is required before final selection.

| Runtime and exact reference | Mechanism | Advantages | Disadvantages / stability | Performance and binary-size implications | TinyLang applicability |
| --- | --- | --- | --- | --- | --- |
| [Goose specification, sections 7.10–7.11](https://github.com/aardappel/goose/blob/master/docs/goose_spec.md), [runtime external declarations](https://github.com/aardappel/goose/blob/master/src/runtime/runtime_ext.h) | Generated C calls and export wrappers; scalars, fixed aggregates and selected borrowed reference/slice shapes; unsupported shapes rejected. Foreign code must respect borrowed lifetimes. | Explicit type restrictions; native C interoperability; compiler can specialize wrappers. | Generated layouts are an ABI commitment; variable-size runtime representations cannot be exposed indiscriminately. | Direct calls avoid a generic dispatcher, but per-signature wrappers may increase code size. Large by-value aggregates can require copies. Neither effect measured here. | Test typed adapters as a performance candidate. Keep internal VM layouts opaque; do not import Goose's allocator/stack design. |
| [Rust Reference: type layout](https://doc.rust-lang.org/reference/type-layout.html), [Rustonomicon: FFI](https://doc.rust-lang.org/nomicon/ffi.html) | `repr(C)` fixes C-compatible field ordering/alignment; `repr(transparent)` delegates layout and ABI to its nontrivial field. `extern "C"` establishes calling convention, not ownership or pointer validity. | Small interoperable descriptors and explicit unsafe boundary. | C representation remains target-dependent; adding fields can break by-value signatures. Raw pointers require live valid storage. Rust-native enums/containers are unsuitable frozen wire layouts. | Borrowed slices avoid boundary allocations; validation still costs time. Monomorphized adapters may trade code size for speed. | Retain explicit little-endian wire encoding and C descriptors; validate lengths before forming Rust references; contain panics. |
| [Lean 4 FFI, type translation and borrowing](https://lean-lang.org/doc/reference/latest/Run-Time-Code/Foreign-Function-Interface/) | C ABI translates scalar types directly; other values may use tagged `lean_object*`. Owned arguments consume an ownership token; borrowed arguments do not. Exported results are owned. | Clear ownership convention enables efficient borrowing and explicit retention. | The documented interface is unstable and relies on runtime object representation. Initialization and foreign threads have additional obligations. | Borrowing reduces reference-count work; boxing/unboxing and runtime support have costs. Binary-size impact unavailable. | Document consume/borrow/release separately from physical representation. Do not add compiler reference counting or expose tagged native pointers. |
| [Swift ABI Stability Manifesto: layouts, metadata, calling conventions](https://github.com/swiftlang/swift/blob/main/docs/ABIStabilityManifesto.md) | Public ABI stability is distinct from internal calling conventions; resilient opaque layouts use metadata/value witnesses while frozen layouts permit direct access. | Runtime and library internals can evolve behind an opaque boundary. | Metadata and indirect operations add complexity; freezing a layout constrains future changes. Native Swift conventions are not simply the C ABI. | Indirection can cost calls/loads; witness machinery and specialized thunks can increase binary size. No Swift binaries measured. | Keep VM/allocator state behind handles; version external layouts independently. Consider descriptor-based values as an alternative candidate. Exported C ownership details require further Swift-specific inspection. |
| [Lua 5.4 C API, sections 4–4.1](https://www.lua.org/manual/5.4/manual.html#4) | An explicit state and virtual value stack mediate calls between Lua and C. Each C activation receives a distinct stack; host code manages stack capacity. | Runtime values need not be exposed as native structs; supports dynamic arguments and results. | Stack indices and lifetime discipline move responsibility into API usage. A stack interface alone is not a portable serialized format. | Multiple push/query calls may cost more than a single packed call, but avoid per-signature native wrappers. Size impact unavailable. | A state-owned value stack is a genuinely different candidate for invocation; compare call count, allocation and lifetime complexity. Do not copy nonlocal error unwinding through Rust frames. |
| [CPython Stable ABI scope/performance](https://docs.python.org/3/c-api/stable.html#stable-abi-scope-and-performance), [reference ownership](https://docs.python.org/3.13/c-api/intro.html#reference-counts) | Limited API hides implementation details; stable function calls replace layout-dependent macros. Reference ownership distinguishes borrowed and owned references. | Preserves compatibility while internals evolve; documents ownership independently of pointer type. | Stable ABI does not guarantee all behavioral compatibility. Version-specific macros can outperform stable calls. | Function indirection may inhibit inlining; actual size and speed require measurement. | Freeze a small C surface, test behavioral contracts as well as symbol names, and keep errors and ownership explicit. No CPython-style object header dependency is needed. |

## Next experiment: VM invocation

Update 2026-10-16: the reusable VM prerequisite and separate session-ownership
comparison are implemented; see [the experiment](v2-session-execution.md).
The following argument/result architecture candidates remain outstanding.

Baseline: schema validation exists, execution returns UNKNOWN_OPCODE. A timing
comparison to this baseline cannot be called an execution speedup because it
does not do equivalent work.

Candidate hypotheses, not yet implemented or selected:

- A: adapt the existing interpreter to a reusable, fuel-bounded session and
  marshal canonical values at each invocation. Expect least semantic drift.
- B: pre-validate export signatures and prepare typed argument/result slots,
  preserving the same runtime execution semantics. Expect less repeated
  conversion at a measurable preparation and retained-memory cost.
- C: use a session-owned value stack with explicit push/invoke/read operations,
  inspired by Lua. Expect different batching and ownership tradeoffs, with
  potential extra boundary calls.

All must execute equivalent compiler-produced programs, reject equivalent
invalid inputs, enforce the same limits, and preserve documented side effects.
None qualifies merely by replacing invocation with a smaller profile-0 runner.
No winner, speedup, memory saving or size improvement is claimed yet.

## Implementation inspection, 2026-10-16

These references are moving upstream branches as retrieved on this date, not
commit-pinned snapshots. Inspection is not execution testing. Costs and size
effects remain hypotheses, with no external binary or timing measurements.

| Source and inspected mechanism | Tradeoff and TinyLang implication |
| --- | --- |
| [Goose runtime_ext.h](https://github.com/aardappel/goose/blob/master/src/runtime/runtime_ext.h), `gs_rref`, `sl_u8`, `gs_bld_append` | Runtime-object declarations duplicate generated packed layouts. Builder append copies bytes and updates stack/header state directly. This is compact but couples foreign code to allocator and code-generator layouts. TinyLang can borrow the explicit pointer/length boundary idea without exposing its VM heap representation. Direct append could avoid generic dispatch, but its layout coupling is unsuitable as a replacement for the stable wire protocol. |
| [Rust extern_abi.rs](https://github.com/rust-lang/rust/blob/master/compiler/rustc_abi/src/extern_abi.rs), `ExternAbi` | C and System explicitly carry unwind policy; Rust conventions are separate variants. This supports keeping the exported C convention and panic containment explicit. Convention choice alone neither validates pointers nor establishes ownership. Register/call and binary-size differences are target-dependent and unmeasured here. |
| [Lean lean.h](https://github.com/leanprover/lean4/blob/master/src/include/lean/lean.h), ownership typedefs and array object declarations | Owned, borrowed and unique arguments share the object-pointer representation; the typedefs document obligations rather than enforce distinct C types. Arrays expose runtime size/capacity and header fields. Explicit ownership vocabulary is useful for TinyLang, but relying on host callers to manipulate runtime headers would constrain future changes. Borrowing may avoid retain work; generated adapter size is unmeasured. |
| [Swift HeapObject.h](https://github.com/swiftlang/swift/blob/main/include/swift/Runtime/HeapObject.h), `swift_retain`, `SWIFT_REFCOUNT_CC`, `SwiftRAII` | Retain has a specialized calling-convention annotation, while RAII balances retain/release across copies and moves. A runtime export is therefore not automatically a generic C embedding entry point. TinyLang should keep ownership cleanup scoped around opaque handles. Retain operations and wrappers have possible call/code costs, not measured savings. No compiler-managed reference counting is proposed. |
| [Lua lapi.c](https://github.com/lua/lua/blob/master/lapi.c), `index2value`, `lua_xmove`, `lua_pcallk` | Values are addressed relative to the activation stack. Transfers check shared global state; protected calls check stack counts and execution state. This provides a concrete alternative to copying a packed argument frame, but adds stack discipline and potentially multiple API calls. TinyLang must likewise reject values belonging to another VM and contain failures without unwinding across Rust/C frames. Binary-size effect unavailable. |
| [CPython object.h](https://github.com/python/cpython/blob/main/Include/object.h), `PyType_HasFeature`, `Py_LIMITED_API` guards | Limited-API builds call `PyType_GetFlags` instead of reading the opaque type's field; newer entry points are version-gated. This demonstrates the optimization-versus-layout-stability tradeoff concretely. TinyLang should expose capability/version queries and stable accessors instead of heap layout. Call and library-size costs require TinyLang measurements. |

## Current compiler boundary audit

The current compiler emits `TINYONEB` version 5 through
`Program::to_binary_artifact`. `VerifiedProgram::from_binary_artifact` decodes
and verifies without trusting serialized host authority; its separate trusted
entry point must not be used for foreign bytes. This decoder supports functions,
strings, structs, modules and enums. The v2 loader currently accepts only TLBC
profile 0, so full compiler interoperability remains incomplete.

`Program::callable_function_from` already applies root/module export rules.
Future export discovery should reuse those rules instead of exposing every
internal function index. An index accepted by the Rust execution adapter is
not itself permission for a foreign caller to access private module functions.

Registry execution must occur outside the global mutex. Session release and
runtime destruction must account for outstanding execution, and panic handling
must discard a damaged VM. Value decoding must retain VM provenance for heap
objects. A size probe must not execute a mutating invocation twice. These are
open acceptance criteria, not guarantees supplied by the ownership benchmark.
