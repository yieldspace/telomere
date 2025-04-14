pub mod binary;
#[macro_use]
pub mod common;
pub mod component_model;
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
pub use runtime::aliasing;
pub use runtime::get_global;
pub use runtime::instantiate;
pub use runtime::link_host_function_with_export_name;
pub use runtime::link_host_function_with_function_idx;
pub use runtime::run_module_function;
pub use runtime::special_function_return;
pub use runtime::ResultValue;
