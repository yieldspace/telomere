pub mod binary;
pub mod common;
pub mod parser;
pub mod runtime;

pub use binary::IoReadBinaryReader;
pub use common::Module;
pub use common::Stack;
pub use common::VMError;
pub use common::WasmValue;
pub use parser::core::WasmParser;
pub use parser::core::WasmParserError;
pub use runtime::vm::run_module_function;
pub use runtime::vm::ResultValue;
pub use runtime::vm::instantiate;
pub use common::Instance;