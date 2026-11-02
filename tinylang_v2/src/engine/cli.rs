use std::io::Write;

use super::{EngineError, ExecutionBackend, ScalarFunction};

const USAGE: &str = "Usage: tinylang-v2 [--backend interpreter|asm|cranelift|llvm] [--fuel N] FILE FUNCTION [I64 ...]\nLLVM emits IR only; execution backends invoke one isolated checked-i64 function.\n";

#[derive(Debug, thiserror::Error)]
pub enum CliError {
    #[error("{0}\n{USAGE}")]
    Usage(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Engine(#[from] EngineError),
}

pub fn run(args: impl IntoIterator<Item = String>, stdout: &mut dyn Write) -> Result<(), CliError> {
    let mut args = args.into_iter();
    let mut backend = "interpreter".to_owned();
    let mut fuel = 1_000_000;
    let filename = loop {
        let arg = args
            .next()
            .ok_or_else(|| CliError::Usage("Missing source file".to_owned()))?;
        match arg.as_str() {
            "--help" | "-h" => {
                stdout.write_all(USAGE.as_bytes())?;
                return Ok(());
            }
            "--backend" => {
                backend = args
                    .next()
                    .ok_or_else(|| CliError::Usage("Missing backend".to_owned()))?;
                if !matches!(
                    backend.as_str(),
                    "interpreter" | "asm" | "cranelift" | "llvm"
                ) {
                    return Err(CliError::Usage(format!("Unknown backend {backend:?}")));
                }
            }
            "--fuel" => {
                let raw = args
                    .next()
                    .ok_or_else(|| CliError::Usage("Missing fuel".to_owned()))?;
                fuel = raw
                    .parse::<u64>()
                    .map_err(|_| CliError::Usage(format!("Invalid unsigned fuel {raw:?}")))?;
            }
            _ if arg.starts_with('-') => {
                return Err(CliError::Usage(format!("Unknown option {arg:?}")));
            }
            _ => break arg,
        }
    };
    let name = args
        .next()
        .ok_or_else(|| CliError::Usage("Missing function name".to_owned()))?;
    let values: Vec<i64> = args
        .map(|raw| {
            raw.parse::<i64>()
                .map_err(|_| CliError::Usage(format!("Invalid i64 argument {raw:?}")))
        })
        .collect::<Result<_, _>>()?;
    let program = tinylang::compile_file_verified(&filename).map_err(EngineError::from)?;
    let index = program
        .program()
        .functions()
        .iter()
        .position(|function| function.name() == name)
        .ok_or(EngineError::UnknownFunction(name))?;
    let function = ScalarFunction::lower(&program, index)?;
    if backend == "llvm" {
        if !values.is_empty() {
            return Err(CliError::Usage(
                "LLVM emission takes no invocation arguments".to_owned(),
            ));
        }
        stdout.write_all(super::llvm::emit_llvm_ir(&function)?.as_bytes())?;
        return Ok(());
    }
    let backend = match backend.as_str() {
        "asm" => ExecutionBackend::NativeAsm,
        "cranelift" => ExecutionBackend::Cranelift,
        _ => ExecutionBackend::Interpreter,
    };
    let result = backend.prepare(&function)?.invoke(&values, fuel)?;
    writeln!(stdout, "{result}")?;
    Ok(())
}
