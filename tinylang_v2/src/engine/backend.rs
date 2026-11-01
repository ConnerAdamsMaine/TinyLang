use super::{EngineError, ScalarFunction, native::NativeFunction};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionBackend {
    Interpreter,
    NativeAsm,
    Cranelift,
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
            Self::NativeAsm => Ok(PreparedFunction::NativeAsm(NativeFunction::compile(
                function,
            )?)),
            Self::Cranelift => {
                #[cfg(feature = "cranelift")]
                {
                    Ok(PreparedFunction::Cranelift(Box::new(
                        super::cranelift::CraneliftFunction::compile(function)?,
                    )))
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
