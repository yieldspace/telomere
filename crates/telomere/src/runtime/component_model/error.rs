use thiserror::Error;

#[derive(Error, Debug)]
pub enum ComponentVMError {
    #[error("link error: {0}")]
    LinkError(String),
    #[error("type mismatch: {0}")]
    TypeMismatch(String),
}
