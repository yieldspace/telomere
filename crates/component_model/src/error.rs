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
}
