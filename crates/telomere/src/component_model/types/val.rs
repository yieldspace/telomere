use crate::component_model::types::{PrimValType, TypeId};
use crate::component_model::Label;
use crate::parser::component_model::{ComponentParseError, ParseResult, Validator};

use super::{DefValType, Type};

#[derive(Debug, Clone)]
pub enum ValType {
    Type(TypeId),
    Primitive(PrimValType),
}
impl ValType {
    pub fn assert_subtype_of(&self, parent: &Self, validator: &Validator) -> ParseResult<()> {
        match (self, parent) {
            (ValType::Type(a), ValType::Type(b)) => validator
                .get_type(*a)?
                .assert_subtype_of(validator.get_type(*b)?, validator)?,
            (ValType::Type(a), ValType::Primitive(prim_val_type)) => {
                validator.get_type(*a)?.assert_subtype_of(
                    &Type::DefVal(DefValType::Primitive(prim_val_type.clone())),
                    validator,
                )?
            }
            (ValType::Primitive(prim_val_type), ValType::Type(b)) => {
                Type::DefVal(DefValType::Primitive(prim_val_type.clone()))
                    .assert_subtype_of(validator.get_type(*b)?, validator)?
            }
            (ValType::Primitive(a), ValType::Primitive(b)) => {
                if a != b {
                    Err(ComponentParseError::TypeMismatch(
                        "prim valtype mismatch".to_owned(),
                    ))?
                }
            }
        }
        Ok(())
    }
}
#[derive(Debug, Clone)]
pub struct LabelValType {
    pub label: Label,
    pub ty: ValType,
}

impl LabelValType {
    pub fn new(label: Label, ty: ValType) -> Self {
        Self { label, ty }
    }
    pub fn assert_subtype_of(&self, parent: &Self, validator: &Validator) -> ParseResult<()> {
        if self.label == parent.label {
            Err(ComponentParseError::TypeMismatch(
                "label mismatch".to_owned(),
            ))?
        }
        self.ty.assert_subtype_of(&parent.ty, validator)
    }
}
