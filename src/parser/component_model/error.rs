use crate::component_model::{CoreModuleExportType, CoreSort, Sort, SortWithIdx};
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
    /// Error for invalid magic number with expected and actual values.
    #[error("invalid {2} magic: {0:?} != {1:?}")]
    InvalidMagic(Box<[u8]>, Box<[u8]>, String),
    /// Error for invalid magic number with a single byte.
    #[error("invalid {1} magic: {0:?}")]
    WrongMagic(u8, String),
    /// Error for invalid version.
    #[error("invalid version: {0:?}")]
    InvalidVersion([u8; 2]),
    /// Error for invalid layer.
    #[error("invalid layer: {0:?}")]
    InvalidLayer([u8; 2]),
    /// Error for invalid section type.
    #[error("invalid section type: {0:?}")]
    InvalidSectionType(u8),
    /// Error for invalid core sort.
    #[error("invalid core sort: {0:?}")]
    InvalidCoreSort(u8),
    #[error("invalid signature: {0:?}")]
    InvalidSignature(String),
    #[error("export `{0:?}` not found")]
    ExportNotFound(String),
    #[error("Sort with idx `{0:?}` is invalid (expected {1})")]
    InvalidSortWithIdx(SortWithIdx, String),
    #[error("Sort `{0:?}` is invalid (expected {1})")]
    InvalidSort(Sort, String),
    #[error("Index `{0:?}` is not found in {1}")]
    InvalidIdx(usize, String),
    #[error("Expected {0} Type")]
    InvalidType(String),
    #[error("Invalid core export `{0}` type: {1:?} != {2:?}")]
    InvalidExportType(String, CoreModuleExportType, CoreSort),
    #[error("Unsupported: {0}")]
    Unsupported(String),
}

impl ComponentParseError {
    /// Asserts that the provided `magic` array matches the expected `expected` array.
    ///
    /// # Parameters
    /// - `magic`: The actual magic number array.
    /// - `expected`: The expected magic number array.
    /// - `name`: A string representing the name of the magic number being checked.
    ///
    /// # Returns
    /// - `Ok(())` if the magic number matches the expected value.
    /// - `Err(ComponentParseError::InvalidMagic)` if the magic number does not match the expected value.
    pub fn assert_magic<const N: usize>(
        magic: [u8; N],
        expected: [u8; N],
        name: &str,
    ) -> std::result::Result<(), Self> {
        if magic == expected {
            Ok(())
        } else {
            Err(Self::InvalidMagic(
                Box::new(expected),
                Box::new(magic),
                name.to_string(),
            ))
        }
    }
}
