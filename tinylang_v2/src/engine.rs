//! Experimental native execution of verified, checked-i64 TinyLang functions.

mod backend;
pub mod cli;
#[cfg(feature = "cranelift")]
pub mod cranelift;
mod error;
mod interpreter;
mod ir;
pub mod llvm;
pub mod native;
mod typing;

pub use backend::{ExecutionBackend, PreparedFunction};
pub use error::{EngineError, TrapKind};
pub use ir::{BinaryOp, Instruction, ScalarFunction};

/// Compile through the authoritative frontend without changing source semantics.
pub fn compile_scalar_source(source: &str, name: &str) -> Result<ScalarFunction, EngineError> {
    let program = tinyone::compile_source_unoptimized_verified(source)?;
    let index = program
        .program()
        .functions()
        .iter()
        .position(|function| function.name() == name)
        .ok_or_else(|| EngineError::UnknownFunction(name.to_owned()))?;
    ScalarFunction::lower(&program, index)
}
