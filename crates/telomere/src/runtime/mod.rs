//mod core;
#[macro_use]
mod trace;
pub(crate) mod instantiate;
pub(crate) mod memory_effect;
pub(crate) mod scheduler;
#[allow(clippy::missing_safety_doc)]
pub(crate) mod vm;
pub use instantiate::aliasing;
pub use instantiate::instantiate;
pub use instantiate::instantiate_native_module;
pub use instantiate::link_host_function_with_export_name;
pub use instantiate::link_host_function_with_function_idx;
pub use vm::get_global;
pub use vm::run_module_function;
pub use vm::special_function_return;
pub use vm::ResultValue;
