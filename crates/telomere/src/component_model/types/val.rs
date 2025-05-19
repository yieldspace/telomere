use crate::component_model::types::{PrimValType, TypeId};
use crate::component_model::Label;

#[derive(Debug, Clone, PartialEq, Hash, Eq)]
pub enum ValType {
    Type(TypeId),
    Primitive(PrimValType),
}

#[derive(Debug, Clone, PartialEq, Hash, Eq)]
pub struct LabelValType {
    label: Label,
    ty: ValType,
}
