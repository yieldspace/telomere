use crate::decoder::ComponentParseError;
use telomere::WasmParserError;
use thiserror::Error;

/// An error raised while decoding, linking, or executing a component.
#[derive(Debug, Error)]
pub enum ComponentError {
    /// A core or component parser exceeded its explicit nesting depth limit.
    #[error("nesting depth exceeds the limit of {limit}")]
    NestingTooDeep {
        /// The fixed maximum nesting depth accepted by the parser or decoder that reported the error.
        limit: u32,
    },
    /// The binary could not be decoded as a Component Model module.
    #[error("decode error: {0}")]
    Decode(String),
    /// The decoded component violated a type-system or ordering rule.
    #[error("validation error: {0}")]
    Validation(String),
    /// A required import or typed function signature could not be resolved.
    #[error("link error: {0}")]
    Link(String),
    /// The component runtime reached an unrecoverable execution error.
    #[error("runtime error: {0}")]
    Runtime(String),
    /// Guest execution trapped after successful decoding and linking.
    #[error("trap: {0}")]
    Trap(String),
    /// The component uses a recognized feature that this runtime does not implement.
    #[error("unsupported: {0}")]
    Unsupported(String),
    /// A requested root export or nested function name was absent.
    #[error("export not found: {0}")]
    ExportNotFound(String),
    /// A host or caller supplied values incompatible with a component signature.
    #[error("invalid argument: {0}")]
    InvalidArgument(String),
}

impl From<ComponentParseError> for ComponentError {
    fn from(value: ComponentParseError) -> Self {
        use crate::decoder::ComponentParseError::*;

        match value {
            NestingTooDeep { limit } | CoreWasmError(WasmParserError::NestingTooDeep { limit }) => {
                Self::NestingTooDeep { limit }
            }
            value => {
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
    }
}
