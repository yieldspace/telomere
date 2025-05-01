use std::io;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("Signature is not valid: {0}")]
    InvalidSignature(String),
    #[error("Reading binary failed")]
    BinaryError(#[from] io::Error),
}
