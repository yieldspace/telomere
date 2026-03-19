#![allow(dead_code, unused_imports)]

mod view;
pub use view::*;
mod root_handle;
pub(crate) use root_handle::GcRootHandle;
mod gc_ref;
pub use gc_ref::GcRef;
mod header;
pub use header::Header;
pub use header::ObjectType;
pub use header::HEADER_LEN;
mod memory_pool;
pub use memory_pool::MemoryPool;
mod object;
pub(crate) use object::FunctionInstanceData;
pub(crate) use object::InstanceData;
