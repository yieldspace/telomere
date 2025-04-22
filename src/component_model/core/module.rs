use crate::component_model::{CoreModuleIdx, CoreModuleType, InstanceIdx, Reference};
use crate::Module;
#[allow(clippy::large_enum_variant)]
#[derive(Clone)]
pub enum CoreModuleReference {
    Imported(String),
    /// Exported from another instance, but it is imported or imported child.
    Instance(InstanceIdx, String),
    TypeOverwritten(CoreModuleIdx),
    /// Type only, so it can't instantiate but can import.
    Exported(String),
}

#[derive(Clone)]
pub struct CoreModule {
    // Defined(Module),
    // Typed(CoreModuleType, Reference),
    // /// Typedだが，exportを経由しており型が変化したもの．
    // SuperTyped(CoreModuleType, CoreModuleIdx, Reference),
    pub value: Option<Module>,
    pub ty: CoreModuleType,
    pub reference: Option<CoreModuleReference>,
}

impl CoreModule {
    pub fn new(value: Option<Module>, ty: CoreModuleType, reference: Option<CoreModuleReference>) -> Self {
        Self {
            value,
            ty,
            reference,
        }
    }
}
