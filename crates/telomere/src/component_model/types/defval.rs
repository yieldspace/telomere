use crate::component_model::types::{LabelValType, PrimValType, TypeId, ValType};
use crate::component_model::{Label, ResourceId};
use crate::parser::component_model::ParseResult;

#[derive(Debug, Clone, PartialEq, Hash, Eq)]
pub enum DefValType {
    Primitive(PrimValType),
    Record(Vec<LabelValType>),
    Variant(Vec<Case>),
    List(ValType, Option<usize>),
    Tuple(Vec<ValType>),
    Option(ValType),
    Result(Option<ValType>, Option<ValType>),
    Own(TypeId),
    Borrow(TypeId),
}

#[derive(Debug, Clone, PartialEq, Hash, Eq)]
pub struct Case {
    label: Label,
    ty: ValType,
}
