pub mod binary;
pub mod common;
pub mod parser;
pub mod runtime;

pub use binary::IoReadBinaryReader;
pub use common::Stack;
pub use parser::core::WasmParser;
pub use parser::core::WasmParserError;
pub use parser::Module;
pub use runtime::vm::run_module_function;
