use crate::{
    component_model::types::ValType,
    parser::component_model::{ComponentParseError, ParseResult, Validator},
};

#[derive(Clone,Debug)]
pub struct FuncType {
    pub params: Vec<ValType>,
    pub result: Option<ValType>,
}
impl FuncType {
    pub fn assert_subtype_of(&self, parent: &Self, validator: &Validator) -> ParseResult<()> {
        if self.params.len() != parent.params.len() {
            Err(ComponentParseError::TypeMismatch(
                "arity mismatch".to_owned(),
            ))?
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
