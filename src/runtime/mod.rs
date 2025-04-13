//mod core;
#[macro_use]
mod trace;

#[allow(clippy::missing_safety_doc)]
pub(crate) mod vm;
//FIXME:
const TABLE_UNINITIALIZED: u32 = 0x00;

mod instantiate;
pub use instantiate::aliasing;
pub use instantiate::instantiate;
pub use instantiate::link_host_function_with_export_name;
pub use instantiate::link_host_function_with_function_idx;
pub use vm::special_function_return;
pub use vm::get_global;
pub use vm::run_module_function;
pub use vm::ResultValue;
