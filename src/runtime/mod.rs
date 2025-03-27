//mod core;
#[macro_use]
mod trace;
pub mod vm;
//FIXME:
const TABLE_UNINITIALIZED: u32 = 0xFFFFFFFF;

mod instantiate;
pub use instantiate::instantiate;
