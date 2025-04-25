use crate::component_model::{CoreModuleIdx, CoreModuleType, InstanceIdx, LazyValue, Reference};
use crate::Module;


#[derive(Clone)]
pub struct CoreModule {
    // Defined(Module),
    // Typed(CoreModuleType, Reference),
    // /// Typedだが，exportを経由しており型が変化したもの．
    // SuperTyped(CoreModuleType, CoreModuleIdx, Reference),
    pub value: Option<Module>,
    pub ty: CoreModuleType,
}

impl CoreModule {
    pub fn new(
        value: Option<Module>,
        ty: CoreModuleType,
    ) -> Self {
        Self {
            value,
            ty,
        }
    }
}
