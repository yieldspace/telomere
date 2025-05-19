use crate::component_model::types::{LabelValType, PrimValType, ValType};
use crate::component_model::{Label, ResourceId};

#[derive(Debug, Clone, PartialEq, Hash, Eq)]
pub enum DefValType {
    Primitive(PrimValType),
    Record(Vec<LabelValType>),
    Variant(Vec<Case>),
    List(ValType, Option<usize>),
    Tuple(Vec<ValType>),
    Option(ValType),
    Result(Option<ValType>, Option<ValType>),
    Own(ResourceId),
    Borrow(ResourceId),
}

#[derive(Debug, Clone, PartialEq, Hash, Eq)]
pub struct Case {
    label: Label,
    ty: ValType,
}
