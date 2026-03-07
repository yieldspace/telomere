use crate::decoder::ComponentParseError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ComponentError {
    #[error("decode error: {0}")]
    Decode(String),
    #[error("validation error: {0}")]
    Validation(String),
    #[error("link error: {0}")]
    Link(String),
    #[error("runtime error: {0}")]
    Runtime(String),
    #[error("trap: {0}")]
    Trap(String),
    #[error("unsupported: {0}")]
    Unsupported(String),
    #[error("export not found: {0}")]
    ExportNotFound(String),
    #[error("invalid argument: {0}")]
    InvalidArgument(String),
}

impl From<ComponentParseError> for ComponentError {
    fn from(value: ComponentParseError) -> Self {
        use crate::decoder::ComponentParseError::*;

        let message = value.to_string();
        match value {
            Unsupported(_) => Self::Unsupported(message),
            InvalidMagic(_, _, _)
            | WrongMagic(_, _)
            | InvalidVersion(_)
            | InvalidLayer(_)
            | InvalidSectionType(_)
            | InvalidCoreSort(_)
            | CoreWasmError(_)
            | IoError(_) => Self::Decode(message),
            _ => Self::Validation(message),
        }
    }
}
