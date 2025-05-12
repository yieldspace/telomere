use crate::component_model::idx::GlobalIdx;
use crate::component_model::types::TypeId;
use crate::component_model::{Component, Instance};

#[derive(Debug, Clone, PartialEq)]
pub enum Sort {
    Component(GlobalIdx<Component>, TypeId),
    Instance(GlobalIdx<Instance>, TypeId),
    Type(TypeId),
}
