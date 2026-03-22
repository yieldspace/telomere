mod base;
mod custom_section;
mod error;
mod instruction;
mod jump_resolver;
mod parser;
mod type_checker;
mod types;
mod validate;
pub(crate) mod values;
pub use error::{ProposalFeature, WasmParserError};
pub(crate) use instruction::InstructionParser;
pub use parser::WasmParser;
pub type Result<R> = std::result::Result<R, WasmParserError>;
pub use values::*;
mod instruction_generator;
#[cfg(feature = "simd")]
mod simd_instruction;
