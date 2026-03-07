use crate::decoder::{ComponentParseError, ParseResult, Validator};
use crate::ir::types::{LabelValType, PrimValType, TypeId, ValType};
use crate::ir::Label;

#[derive(Debug, Clone)]
pub enum DefValType {
    Primitive(PrimValType),
    Record(Vec<LabelValType>),
    Variant(Vec<Case>),
    Flags(Vec<Label>),
    List(ValType, Option<usize>),
    Own(TypeId),
    Borrow(TypeId),
}
impl DefValType {
    pub fn assert_subtype_of(&self, parent: &Self, validator: &Validator) -> ParseResult<()> {
        use DefValType::*;
        match (self, parent) {
            (Primitive(a), Primitive(b)) => a.assert_subtype_of(b),
            (Record(fields), Record(parent_fields)) => {
                if fields.len() != parent_fields.len() {
                    Err(ComponentParseError::TypeMismatch(
                        "record arity mismatch".to_owned(),
                    ))?;
                }
                for (field, parent_field) in fields.iter().zip(parent_fields.iter()) {
                    field.assert_subtype_of(parent_field, validator)?;
                }
                Ok(())
            }
            (Variant(cases), Variant(parent_cases)) => {
                if cases.len() != parent_cases.len() {
                    Err(ComponentParseError::TypeMismatch(
                        "variant arity mismatch".to_owned(),
                    ))?;
                }
                for (case, parent_case) in cases.iter().zip(parent_cases.iter()) {
                    if case.label != parent_case.label {
                        Err(ComponentParseError::TypeMismatch(
                            "variant label mismatch".to_owned(),
                        ))?;
                    }
                    match (&case.ty, &parent_case.ty) {
                        (Some(ty), Some(parent_ty)) => {
                            ty.assert_subtype_of(parent_ty, validator)?;
                        }
                        (None, None) => {}
                        _ => {
                            Err(ComponentParseError::TypeMismatch(
                                "variant payload mismatch".to_owned(),
                            ))?;
                        }
                    }
                }
                Ok(())
            }
            (Flags(labels), Flags(parent_labels)) => {
                if labels.len() != parent_labels.len() {
                    Err(ComponentParseError::TypeMismatch(
                        "flags arity mismatch".to_owned(),
                    ))?;
                }
                for (label, parent_label) in labels.iter().zip(parent_labels.iter()) {
                    if label != parent_label {
                        Err(ComponentParseError::TypeMismatch(
                            "flag label mismatch".to_owned(),
                        ))?;
                    }
                }
                Ok(())
            }
            (List(ty, len), List(parent_ty, parent_len)) => {
                ty.assert_subtype_of(parent_ty, validator)?;
                if len != parent_len {
                    Err(ComponentParseError::TypeMismatch(
                        "list length mismatch".to_owned(),
                    ))?;
                }
                Ok(())
            }
            (Own(id), Own(parent_id)) => {
                id.assert_subtype_of(*parent_id, validator)?;
                Ok(())
            }
            (Borrow(id), Borrow(parent_id)) => {
                id.assert_subtype_of(*parent_id, validator)?;
                Ok(())
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
