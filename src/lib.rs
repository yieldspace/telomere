pub mod binary;
pub mod common;
pub mod parser;
pub mod runtime;

pub use parser::Module;
pub use parser::core::WasmParser;
pub use parser::core::WasmParserError;
pub use runtime::vm::run_module_function;
pub use common::Stack;
pub use binary::new_binary_reader;