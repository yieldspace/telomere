use crate::component::decoder::{ComponentParseError, ParseResult, Validator};
use crate::component::ir::types::{LabelValType, PrimValType, TypeId, ValType};
use crate::component::ir::Label;

#[derive(Debug, Clone)]
pub enum DefValType {
    Primitive(PrimValType),
    Record(Vec<LabelValType>),
    Variant(Vec<Case>),
    List(ValType, Option<usize>),
    Own(TypeId),
    Borrow(TypeId),
}
impl DefValType {
    pub fn assert_subtype_of(&self, parent: &Self, _validator: &Validator) -> ParseResult<()> {
        use DefValType::*;
        match (self, parent) {
            (Primitive(a), Primitive(b)) => a.assert_subtype_of(b),
            (Record(_), Record(_)) => {
                todo!()
            }
            (Variant(_), Variant(_)) => {
                todo!()
            }
            (List(_, _), List(_, _)) => {
                todo!()
            }
            (Own(_), Own(_)) => {
                todo!()
            }
            (Borrow(_), Borrow(_)) => {
                todo!()
            }
            _ => Err(ComponentParseError::TypeMismatch(
                "defvaltype mismatch".to_owned(),
            ))?,
        }
    }
}
#[derive(Debug, Clone)]
pub struct Case {
    pub label: Label,
    pub ty: Option<ValType>,
}

impl Case {
    pub fn new(label: Label, ty: Option<ValType>) -> Self {
        Self { label, ty }
    }
}
