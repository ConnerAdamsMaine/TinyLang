//! LLVM IR emission for the verified checked-i64 profile.
//!
//! Every module exports exactly this host C ABI (opaque-pointer LLVM syntax):
//! `uint32_t tinylang_scalar(int64_t *slots, int64_t *stack, int64_t *output,
//!                          uint64_t *fuel, uint64_t *error_instruction)`.
//! The caller supplies distinct, aligned, writable buffers: `slot_count()` i64
//! slots initialized to zero with exactly `parameter_count()` arguments copied
//! into the prefix, `max_stack()` i64 stack entries, and the three scalar outputs.
//! Empty unused buffers may use a dangling aligned pointer. Pointer lifetimes
//! cover the call. The callee does not allocate, retain pointers, or check arity.
//!
//! Status 0 writes output; 1 means overflow, 2 division by zero, 3 fuel exhausted.
//! Traps write the zero-based source IR instruction index and leave output
//! untouched. Fuel is unsigned and decremented before each executed instruction;
//! exhaustion at zero does not decrement. `error_instruction` is untouched on
//! success. Arithmetic uses LLVM signed-overflow intrinsics; division is guarded
//! signed floor division, matching the authoritative VM rather than C truncation.
//! No target triple is imposed: the consuming LLVM compiler chooses its target.
//! Emission alone does not claim executable-backend availability or validation.
//!
//! The program profile (`emit_program_llvm_ir`) mirrors the Cranelift program
//! ABI exactly: one shared invocation context (depth@0 u64, trap@8 u32,
//! fuel@16 u64, bank@24 ptr, pc@32 u64), a uniform frame bank addressed by
//! call depth, the same trap tags (1 overflow, 2 division by zero, 3 fuel
//! exhausted, 4 call-stack overflow) and the same argument/trap propagation
//! conventions. It emits every closure member as
//! `define i64 @tinylang_program_N(ptr %context)`; the entry member's name is
//! documented in the header comment.

mod emission;
mod program;

pub use emission::emit_llvm_ir;
pub use program::emit_program_llvm_ir;
