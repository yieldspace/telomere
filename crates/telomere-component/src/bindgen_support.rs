//! Hidden support types used by `telomere-component-bindgen` generated bindings.
//!
//! This is not a supported embedder API. It exists solely so generated bindings
//! can depend on the component type representation without exposing the runtime
//! implementation details as public modules.

pub use crate::error::ComponentError;
pub use crate::func::ComponentReturn;
pub use crate::ir::types::{DefValType, Type, ValType};
