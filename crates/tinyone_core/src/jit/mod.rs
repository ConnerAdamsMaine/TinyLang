pub(crate) mod cache;
mod call_feedback;
pub(crate) mod chunk;
mod compiled_cache;
mod encoding;
mod execution;
mod feedback;
pub(crate) mod frame;
mod local;
pub(crate) mod lowering;
pub(crate) mod op;
mod origins;
#[cfg(feature = "testing-hooks")]
pub(crate) mod preparation_profile;
pub(crate) mod program;
pub(crate) mod scalar;
pub(crate) mod vm;

pub use cache::{
    DEFAULT_HOT_BACK_EDGE_THRESHOLD,
    DEFAULT_SOURCE_CACHE_MAX_BYTES,
    DEFAULT_SOURCE_CACHE_MAX_ENTRIES,
    JitCache,
    JitCacheStats,
    JitExecutionProfile,
    JitOpcodeProfile,
    JitOptions,
    JitStats,
};
pub(crate) use chunk::JitChunk;
pub use compiled_cache::{DEFAULT_COMPILED_CACHE_MAX_BYTES, DEFAULT_COMPILED_CACHE_MAX_ENTRIES, JitCompiledCacheStats};
pub(crate) use op::{JitBuiltin, JitOp};
pub use program::{JitProgram, write_jit_listing, write_verified_jit_listing};
pub(crate) use vm::JitVm;
