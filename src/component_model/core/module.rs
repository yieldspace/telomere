use crate::component_model::{CoreModuleIdx, CoreModuleType, Reference};
use crate::Module;

pub enum CoreModule {
    Defined(Module),
    Typed(CoreModuleType, Reference),
    /// Typedだが，exportを経由しており型が変化したもの．
    SuperTyped(CoreModuleType, CoreModuleIdx, Reference),
}
