use crate::component::decoder::ComponentParseError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ComponentError {
    #[error("decode error: {0}")]
    Decode(String),
    #[error("link error: {0}")]
    Link(String),
    #[error("runtime error: {0}")]
    Runtime(String),
    #[error("export not found: {0}")]
    ExportNotFound(String),
    #[error("invalid argument: {0}")]
    InvalidArgument(String),
}

impl From<ComponentParseError> for ComponentError {
    fn from(value: ComponentParseError) -> Self {
        Self::Decode(value.to_string())
    }
}
