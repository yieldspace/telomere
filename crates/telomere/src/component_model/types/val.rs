use crate::component_model::types::{PrimValType, TypeId};
use crate::component_model::Label;
use crate::parser::component_model::ParseResult;

#[derive(Debug, Clone, PartialEq, Hash, Eq)]
pub enum ValType {
    Type(TypeId),
    Primitive(PrimValType),
}

#[derive(Debug, Clone, PartialEq, Hash, Eq)]
pub struct LabelValType {
    pub label: Label,
    pub ty: ValType,
}

impl LabelValType {
    pub fn new(label: Label, ty: ValType) -> Self {
        Self { label, ty }
    }
}
