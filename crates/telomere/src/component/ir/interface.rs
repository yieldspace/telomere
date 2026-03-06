use crate::component::ir::types::{CoreModuleType, ValType};
use crate::component::ir::TypeId;
use std::hash::Hash;

#[derive(Debug, Clone, PartialEq)]
pub enum ExternDesc {
    Module(CoreModuleType),
    Component(TypeId),
    Instance(TypeId),
    Eq(TypeId),
    Sub,
    Func(TypeId),
    Value(ValType),
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum PlaceholderType {
    Import,
    Export,
}
