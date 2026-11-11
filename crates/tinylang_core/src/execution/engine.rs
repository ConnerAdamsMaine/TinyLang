//! Experimental native execution of verified, checked-i64 TinyLang functions.
#![deny(unsafe_op_in_unsafe_fn)]

mod backend;
pub mod cli;
#[cfg(feature = "cranelift")]
pub mod cranelift;
mod error;
mod interpreter;
mod ir;
pub mod llvm;
pub mod native;
mod program;
mod typing;

pub use backend::{BackendCapabilities, BackendTargetRequirement, ExecutionBackend, PreparedFunction, PreparedProgram};
pub use error::{EngineError, TrapKind};
pub use ir::{BinaryOp, Instruction, ScalarFunction};
pub use program::ScalarProgram;

/// Compile through the authoritative frontend without changing source semantics.
pub fn compile_scalar_source(source: &str, name: &str) -> Result<ScalarFunction, EngineError> {
    let program = crate::compile_source_unoptimized_verified(source)?;
    let index = program
        .program()
        .functions()
        .iter()
        .position(|function| function.name() == name)
        .ok_or_else(|| EngineError::UnknownFunction(name.to_owned()))?;
    ScalarFunction::lower(&program, index)
}
