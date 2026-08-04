use crate::ir::idx::GlobalIdx;
use crate::ir::types::CoreModuleType;
use crate::ir::{Component, CoreModule, Func, Instance, TypeId};

#[derive(Debug, Clone, PartialEq)]
// Retained conservatively; current decoding uses `Sort` rather than `CoreSort`.
#[allow(dead_code)]
pub enum CoreSort {
    Module(GlobalIdx<CoreModule>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Sort {
    Module(GlobalIdx<CoreModule>, CoreModuleType),
    Component(GlobalIdx<Component>, TypeId),
    Instance(GlobalIdx<Instance>, TypeId),
    Func(GlobalIdx<Func>, TypeId),
    Type(TypeId),
}
impl Sort {
    pub(crate) fn type_id(&self) -> Option<TypeId> {
        match self {
            Sort::Module(_, _) => None,
            Sort::Component(_global_idx, type_id) => Some(*type_id),
            Sort::Instance(_global_idx, type_id) => Some(*type_id),
            Sort::Func(_global_idx, type_id) => Some(*type_id),
            Sort::Type(type_id) => Some(*type_id),
        }
    }
}
