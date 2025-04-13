use crate::WasmParserError;
use thiserror::Error;

/// `ComponentParseError` represents the possible errors that can occur in the component model parser.
#[derive(Error, Debug)]
pub enum ComponentParseError {
    /// Error occurring in the core WASM module.
    #[error("error at core wasm module")]
    CoreWasmError(#[from] WasmParserError),
    /// Error from the underlying layer.
    #[error("error from underlying layer: {0:?}")]
    IoError(#[from] std::io::Error),
}
