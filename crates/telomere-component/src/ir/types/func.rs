use crate::decoder::{ComponentParseError, ParseResult, Validator};
use crate::ir::types::ValType;
use crate::ir::Label;

#[derive(Clone, Debug)]
pub struct FuncType {
    pub params: Vec<ValType>,
    pub param_names: Vec<Label>,
    pub result: Option<ValType>,
}
impl FuncType {
    pub fn assert_subtype_of(&self, parent: &Self, validator: &Validator) -> ParseResult<()> {
        if self.params.len() != parent.params.len() {
            Err(ComponentParseError::TypeMismatch(
                "arity mismatch".to_owned(),
            ))?
        }
        if self.param_names.len() == parent.param_names.len() {
            for (actual, expected) in self.param_names.iter().zip(parent.param_names.iter()) {
                if actual != expected {
                    Err(ComponentParseError::TypeMismatch(format!(
                        "expected parameter named `{expected}`, found `{actual}`"
                    )))?
                }
            }
        }
        for (a, b) in self.params.iter().zip(parent.params.iter()) {
            a.assert_subtype_of(b, validator)?
        }
        match (&self.result, &parent.result) {
            (None, None) => {}
            (Some(a), Some(b)) => a.assert_subtype_of(b, validator)?,
            _ => Err(ComponentParseError::TypeMismatch(
                "result type mismatch".to_owned(),
            ))?,
        };
        Ok(())
    }
}
