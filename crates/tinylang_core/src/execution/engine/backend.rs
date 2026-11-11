use super::native::{NativeFunction, NativeProgram};
use super::{EngineError, ScalarFunction, ScalarProgram};

/// Program-level counterpart of [`PreparedFunction`]: a prepared whole-call-
/// closure execution. The interpreter variant is the dynamic reference; the asm
/// variant is one machine-code image executing the same closure.
pub enum PreparedProgram {
    Interpreter(ScalarProgram),
    NativeAsm(NativeProgram),
    #[cfg(feature = "cranelift")]
    Cranelift(Box<super::cranelift::CraneliftProgram>),
}

/// Host-target constraints declared by an execution backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendTargetRequirement {
    /// Portable interpreter execution.
    Any,
    /// A 64-bit x86 host.
    X86_64,
    /// A 64-bit x86 host running Windows.
    WindowsX86_64,
}

/// Static build and code-generation properties of an execution backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackendCapabilities {
    /// Host target required by this backend.
    pub target: BackendTargetRequirement,
    /// Whether the backend emits machine code instead of interpreting IR.
    pub emits_machine_code: bool,
    /// Cargo feature required to compile the backend, if any.
    pub required_feature: Option<&'static str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionBackend {
    Interpreter,
    NativeAsm,
    Cranelift,
}

impl ExecutionBackend {
    /// Returns static target and build-feature requirements for this backend.
    ///
    /// These requirements do not guarantee runtime code generation will succeed;
    /// `prepare` remains authoritative for backend availability.
    pub const fn capabilities(self) -> BackendCapabilities {
        match self {
            Self::Interpreter => BackendCapabilities {
                target: BackendTargetRequirement::Any,
                emits_machine_code: false,
                required_feature: None,
            },
            Self::NativeAsm => BackendCapabilities {
                target: BackendTargetRequirement::WindowsX86_64,
                emits_machine_code: true,
                required_feature: None,
            },
            Self::Cranelift => BackendCapabilities {
                target: BackendTargetRequirement::X86_64,
                emits_machine_code: true,
                required_feature: Some("cranelift"),
            },
        }
    }
}

pub enum PreparedFunction {
    Interpreter(ScalarFunction),
    NativeAsm(NativeFunction),
    #[cfg(feature = "cranelift")]
    Cranelift(Box<super::cranelift::CraneliftFunction>),
}

impl ExecutionBackend {
    pub fn prepare(self, function: &ScalarFunction) -> Result<PreparedFunction, EngineError> {
        match self {
            Self::Interpreter => Ok(PreparedFunction::Interpreter(function.clone())),
            Self::NativeAsm => Ok(PreparedFunction::NativeAsm(NativeFunction::compile(function)?)),
            Self::Cranelift => {
                #[cfg(feature = "cranelift")]
                {
                    Ok(PreparedFunction::Cranelift(Box::new(super::cranelift::CraneliftFunction::compile(function)?)))
                }
                #[cfg(not(feature = "cranelift"))]
                {
                    Err(EngineError::BackendUnavailable {
                        backend: "cranelift",
                        reason: "build with --features cranelift".to_owned(),
                    })
                }
            }
        }
    }
}

impl PreparedProgram {
    pub fn invoke(&self, args: &[i64], fuel: u64) -> Result<i64, EngineError> {
        match self {
            Self::Interpreter(program) => program.interpret(args, fuel),
            Self::NativeAsm(program) => program.invoke(args, fuel),
            #[cfg(feature = "cranelift")]
            Self::Cranelift(program) => program.invoke(args, fuel),
        }
    }
}

impl PreparedFunction {
    pub fn invoke(&self, args: &[i64], fuel: u64) -> Result<i64, EngineError> {
        match self {
            Self::Interpreter(function) => function.interpret(args, fuel),
            Self::NativeAsm(function) => function.invoke(args, fuel),
            #[cfg(feature = "cranelift")]
            Self::Cranelift(function) => function.invoke(args, fuel),
        }
    }
}

impl ExecutionBackend {
    /// Prepares a whole call closure for execution from its `entry` function.
    ///
    /// The checked scalar program profile has three real backends: the dynamic
    /// reference interpreter, the emitted x86-64 image and, under the
    /// `cranelift` feature, Cranelift-emitted code. LLVM has no program-profile
    /// backend yet; this is reported as [`EngineError::BackendUnavailable`]
    /// instead of silently falling back.
    pub fn prepare_program(self, program: &ScalarProgram) -> Result<PreparedProgram, EngineError> {
        match self {
            Self::Interpreter => Ok(PreparedProgram::Interpreter(program.clone())),
            Self::NativeAsm => {
                #[cfg(all(target_arch = "x86_64", target_os = "windows"))]
                {
                    Ok(PreparedProgram::NativeAsm(NativeProgram::compile(program)?))
                }
                #[cfg(not(all(target_arch = "x86_64", target_os = "windows")))]
                {
                    let _ = program;
                    Err(EngineError::BackendUnavailable {
                        backend: "x86-64 ASM",
                        reason: "this backend requires Windows x86-64".to_owned(),
                    })
                }
            }
            Self::Cranelift => {
                #[cfg(feature = "cranelift")]
                {
                    Ok(PreparedProgram::Cranelift(Box::new(super::cranelift::CraneliftProgram::compile(program)?)))
                }
                #[cfg(not(feature = "cranelift"))]
                {
                    let _ = program;
                    Err(EngineError::BackendUnavailable {
                        backend: "cranelift",
                        reason: "build with --features cranelift".to_owned(),
                    })
                }
            }
        }
    }
}
