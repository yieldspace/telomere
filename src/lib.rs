pub mod binary;
#[macro_use]
pub mod common;
mod component_model;
pub mod parser;
pub mod runtime;

pub use binary::IoReadBinaryReader;
pub use common::Instance;
pub use common::Module;
pub use common::Registry;
pub use common::Stack;
pub use common::Store;
pub use common::VMResult;
pub use common::WasmValue;
pub use parser::core::WasmParser;
pub use parser::core::WasmParserError;
pub use runtime::instantiate;
pub use runtime::vm::get_global;
pub use runtime::vm::run_module_function;
pub use runtime::vm::ResultValue;
