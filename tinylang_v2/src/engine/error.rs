#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum TrapKind {
    #[error("Runtime.Memory_Overflow")]
    Overflow,
    #[error("Division by zero")]
    DivisionByZero,
    #[error("Instruction fuel exhausted")]
    FuelExhausted,
}

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error(transparent)]
    Compile(#[from] tinylang::TinyLangError),
    #[error("Unknown function {0:?}")]
    UnknownFunction(String),
    #[error("{function} instruction {instruction}: unsupported opcode {opcode}")]
    UnsupportedInstruction {
        function: String,
        instruction: usize,
        opcode: String,
    },
    #[error("{function} instruction {instruction}: invalid scalar IR: {message}")]
    InvalidIr {
        function: String,
        instruction: usize,
        message: String,
    },
    #[error("Expected {expected} arguments, received {actual}")]
    InvalidArguments { expected: usize, actual: usize },
    #[error("{kind} at instruction {instruction}")]
    Trap { kind: TrapKind, instruction: usize },
    #[error("{backend} unavailable: {reason}")]
    BackendUnavailable {
        backend: &'static str,
        reason: String,
    },
    #[error("Execution allocation failed: {0}")]
    Allocation(String),
    #[error("Code generation failed: {0}")]
    Codegen(String),
}
