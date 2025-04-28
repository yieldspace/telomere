use crate::Module;

#[derive(Clone)]
pub struct CoreModule {
    // Defined(Module),
    // Typed(CoreModuleType, Reference),
    // /// Typedだが，exportを経由しており型が変化したもの．
    // SuperTyped(CoreModuleType, CoreModuleIdx, Reference),
    pub value: Module,
}

impl CoreModule {
    pub fn new(value: Module) -> Self {
        Self { value }
    }
}
