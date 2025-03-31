//mod core;
#[macro_use]
mod trace;

#[allow(clippy::missing_safety_doc)]
pub mod vm;
//FIXME:
const TABLE_UNINITIALIZED: u32 = 0x00;

mod instantiate;
pub use instantiate::instantiate;
