use thiserror::Error;

pub type Result<T> = std::result::Result<T, TynraError>;

#[derive(Debug, Error)]
pub enum TynraError {
    #[error("{0}")]
    Compile(String),
    #[error("{0}")]
    Runtime(String),
}

impl TynraError {
    pub(crate) fn compile(message: impl Into<String>) -> Self {
        Self::Compile(message.into())
    }

    pub(crate) fn runtime(message: impl Into<String>) -> Self {
        Self::Runtime(message.into())
    }
}
