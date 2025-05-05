mod instance;
mod module;

use crate::component_model::CoreTypeRef;
pub use instance::*;
pub use module::*;

#[derive(Debug, Clone, PartialEq)]
pub enum CoreType {
    Ref(CoreTypeRef),
    ModuleType(CoreModuleType),
}
