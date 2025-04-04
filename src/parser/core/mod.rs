mod parser;
mod types;
mod values;
mod error;
pub use parser::WasmParser;
pub use error::WasmParserError;
pub type Result<R> = std::result::Result<R, WasmParserError>;
pub use values::*;