use crate::WasmParserError;
use thiserror::Error;

/// `ComponentParseError` represents the possible errors that can occur in the component model parser.
#[derive(Error, Debug)]
pub enum ComponentParseError {
    /// Error when a module is set multiple times.
    #[error("module can't set multiple times")]
    MultipleModule,
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
    /// Error occurring in the core WASM module.
    #[error("error at core wasm module")]
    CoreWasmError(#[from] WasmParserError),
    /// Error from the underlying layer.
    #[error("error from underlying layer")]
    IoError(#[from] std::io::Error),
    /// Error for invalid section type.
    #[error("invalid section type: {0:?}")]
    InvalidSectionType(u8),
    /// Error for invalid instance expression.
    #[error("invalid instance expression: {0:?}")]
    InvalidInstanceExpr(u8),
    /// Error for invalid core sort.
    #[error("invalid core sort: {0:?}")]
    InvalidCoreSort(u8),
    /// Error for invalid core module declaration type.
    #[error("invalid core module decl type: {0:?}")]
    InvalidCoreModuleDecl(u8),
    /// Error for invalid core alias target magic.
    #[error("invalid core alias target magic: {0:?}")]
    InvalidCoreAliasTargetMagic(u8),
    /// Error for invalid sort.
    #[error("invalid sort: {0:?}")]
    InvalidSort(u8),
    /// Error for invalid alias target.
    #[error("invalid alias target: {0:#x}")]
    InvalidAliasTarget(u8),
    /// Error for invalid index with a description and value.
    #[error("invalid {0} idx: {1:?}")]
    InvalidIdx(String, u32),
    /// Error for invalid module ID.
    #[error("invalid module id: {0:?}")]
    InvalidModuleId(u32),
    /// Error for invalid instance ID.
    #[error("invalid instance id: {0:?}")]
    InvalidInstanceId(u32),
    /// Error for invalid component ID.
    #[error("invalid component id: {0:?}")]
    InvalidComponentId(u32),
    /// Error for invalid primitive value type.
    #[error("invalid prim val type: {0:?}")]
    InvalidPrimValType(u8),
    /// General type error with a description.
    #[error("type error: {0:?}")]
    TypeError(String),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assert_magic_matches() {
        let magic = [0x00, 0x61, 0x73, 0x6d];
        let expected = [0x00, 0x61, 0x73, 0x6d];
        let name = "component";
        assert!(ComponentParseError::assert_magic(magic, expected, name).is_ok());
    }

    #[test]
    fn assert_magic_does_not_match() {
        let magic = [0x00, 0x61, 0x73, 0x6e];
        let expected = [0x00, 0x61, 0x73, 0x6d];
        let name = "component";
        let result = ComponentParseError::assert_magic(magic, expected, name);
        assert!(result.is_err());
        if let Err(ComponentParseError::InvalidMagic(exp, act, n)) = result {
            assert_eq!(*exp, expected);
            assert_eq!(*act, magic);
            assert_eq!(n, name);
        } else {
            panic!("Expected InvalidMagic error");
        }
    }
}
