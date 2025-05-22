use crate::component_model::TypeId;
use std::hash::Hash;

#[derive(Debug, Clone, PartialEq)]
pub enum ExternDesc {
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
