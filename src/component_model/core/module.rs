use crate::component_model::{CoreModuleIdx, CoreModuleType, Reference};
use crate::Module;

pub enum CoreModule {
    Defined(Module),
    Typed(CoreModuleType, Reference),
    SuperTyped(CoreModuleType, CoreModuleIdx, Reference),
}
