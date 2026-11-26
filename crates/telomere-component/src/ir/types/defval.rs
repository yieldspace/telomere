use crate::decoder::{ComponentParseError, ParseResult, Validator};
use crate::ir::types::{LabelValType, PrimValType, TypeId, ValType};
use crate::ir::Label;

#[doc(hidden)]
#[derive(Debug, Clone)]
pub enum DefValType {
    Primitive(PrimValType),
    Record(Vec<LabelValType>),
    Variant(VariantType),
    Flags(Vec<Label>),
    List(ValType, Option<usize>),
    Own(TypeId),
    Borrow(TypeId),
}

/// Which constructor declared a variant-shaped type.
///
/// The `option`, `result` and `enum` constructors are sugar for particular
/// variant shapes, and the component model treats them as the *same type*
/// as the variant they desugar to (subtyping and layout only ever look at
/// the cases). The dynamic value representation is not part of that
/// equivalence, though: a value of an `option` type is represented as
/// `ComponentValue::Option`, while a value of a variant type that merely
/// has the same shape is represented as `ComponentValue::Variant`. The
/// declared constructor is therefore recorded alongside the cases so the
/// lifting code can pick the representation the type was declared with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VariantForm {
    /// Declared with the `variant` constructor.
    Variant,
    /// Declared with the `option` constructor (cases `none` / `some`).
    Option,
    /// Declared with the `result` constructor (cases `ok` / `err`).
    Result,
    /// Declared with the `enum` constructor (payloadless cases).
    Enum,
}

/// A variant-shaped defined type: its cases plus the constructor that
/// declared it. Structural operations (subtyping, size, flattening) only
/// consult `cases`; `form` exists solely to choose the dynamic value
/// representation at lift time.
#[derive(Debug, Clone)]
pub struct VariantType {
    pub cases: Vec<Case>,
    pub form: VariantForm,
}

impl VariantType {
    pub fn new(cases: Vec<Case>, form: VariantForm) -> Self {
        Self { cases, form }
    }

    /// A variant declared with the `variant` constructor itself.
    pub fn declared(cases: Vec<Case>) -> Self {
        Self::new(cases, VariantForm::Variant)
    }
}

impl std::ops::Deref for VariantType {
    type Target = [Case];

    fn deref(&self) -> &[Case] {
        &self.cases
    }
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
            (Own(_), Borrow(_)) => Err(ComponentParseError::TypeMismatch(
                "expected borrow, found own".to_owned(),
            ))?,
            (Borrow(_), Own(_)) => Err(ComponentParseError::TypeMismatch(
                "expected own, found borrow".to_owned(),
            ))?,
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
