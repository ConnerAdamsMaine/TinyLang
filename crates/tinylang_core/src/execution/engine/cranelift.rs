//! Cranelift execution of the verified checked-i64 profile (feature `cranelift`).
//! Currently supports x86-64 hosts; other ISAs fail explicitly.

mod executable;
mod lowering;
mod memory;
mod program;

pub use executable::CraneliftFunction;
pub use program::CraneliftProgram;
