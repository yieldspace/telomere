use crate::component_model::types::{LabelValType, PrimValType, TypeId, ValType};
use crate::component_model::{Label, ResourceId};
use crate::parser::component_model::ParseResult;

#[derive(Debug, Clone, PartialEq, Hash, Eq)]
pub enum DefValType {
    Primitive(PrimValType),
    Record(Vec<LabelValType>),
    Variant(Vec<Case>),
    List(ValType, Option<usize>),
    Own(TypeId),
    Borrow(TypeId),
}

#[derive(Debug, Clone, PartialEq, Hash, Eq)]
pub struct Case {
    pub label: Label,
    pub ty: Option<ValType>,
}

impl Case {
    pub fn new(label: Label, ty: Option<ValType>) -> Self {
        Self { label, ty }
    }
}
