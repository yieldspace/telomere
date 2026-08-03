mod base;
mod custom_section;
/// Error types and proposal identities reported by the core parser.
#[warn(missing_docs)]
mod error;
mod instruction;
mod jump_resolver;
mod optimizer;
/// The public core WebAssembly module parser.
#[warn(missing_docs)]
mod parser;
mod type_checker;
mod types;
mod validate;
pub(crate) mod values;
/// Re-exports the core parser error types.
pub use error::{ProposalFeature, WasmParserError};
pub(crate) use instruction::InstructionParser;
/// Re-exports the core WebAssembly binary parser.
pub use parser::WasmParser;
/// The result type returned by core parser operations.
pub type Result<R> = std::result::Result<R, WasmParserError>;
/// Re-exports parser value helpers used by the core parser surface.
pub use values::*;
mod instruction_generator;
#[cfg(feature = "simd")]
mod simd_instruction;
