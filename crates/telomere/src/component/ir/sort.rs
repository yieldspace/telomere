use crate::component::ir::idx::GlobalIdx;
use crate::component::ir::{Component, CoreModule, Func, Instance, TypeId};

#[derive(Debug, Clone, PartialEq)]
pub enum CoreSort {
    Module(GlobalIdx<CoreModule>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Sort {
    Component(GlobalIdx<Component>, TypeId),
    Instance(GlobalIdx<Instance>, TypeId),
    Func(GlobalIdx<Func>, TypeId),
    Type(TypeId),
}
impl Sort {
    pub(crate) fn type_id(&self) -> TypeId {
        match self {
            Sort::Component(_global_idx, type_id) => *type_id,
            Sort::Instance(_global_idx, type_id) => *type_id,
            Sort::Func(_global_idx, type_id) => *type_id,
            Sort::Type(type_id) => *type_id,
        }
    }
}
