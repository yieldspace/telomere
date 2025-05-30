use crate::component_model::TypeId;
use std::hash::Hash;
use crate::component_model::types::CoreModuleType;

#[derive(Debug, Clone, PartialEq)]
pub enum ExternDesc {
    // todo: CoreModule(CoreModuleType),
    Component(TypeId),
    Instance(TypeId),
    Eq(TypeId),
    Sub,
    Func(TypeId),
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum PlaceholderType {
    Import,
    Export,
}
