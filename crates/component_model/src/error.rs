use telomere_wasm::WasmParserError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ComponentParseError {
    #[error("Reading binary failed: {0}")]
    ReadError(#[from] std::io::Error),
    #[error("Core Wasm error: {0}")]
    CoreWasmError(#[from] WasmParserError),
    #[error("Invalid {2}: expected {1:?}, found {0:?}")]
    InvalidSignature(Box<[u8]>, Box<[u8]>, String),
    #[error("Invalid core instance type: {0}")]
    InvalidCoreInstanceType(u8),
    #[error("Invalid instance type: {0}")]
    InvalidInstanceType(u8),
    #[error("Index error: {0}")]
    IndexError(String),
    #[error("Invalid name: {0}")]
    InvalidName(String),
    #[error("Invalid sort type: {0}")]
    InvalidSortType(u8),
    #[error("Invalid core sort type: {0}")]
    InvalidCoreSortType(u8),
    #[error("Invalid alias type: {0}")]
    InvalidAliasType(u8),
    #[error("Invalid canon type: {0}")]
    InvalidCanonType(u8),
    #[error("Invalid canon opt: {0}")]
    InvalidCanonOpt(String),
    #[error("Type error: {0}")]
    TypeError(String),
}
