use crate::component_model::{ExternDesc, Label, ValType};
use crate::parser::component_model::ComponentParseError;
use crate::parser::component_model::ParseResult;

#[derive(Debug, Clone, PartialEq)]
pub struct FuncType {
    pub params: Vec<LabelValType>,
    pub result: Option<Box<ValType>>,
}

impl FuncType {
    pub fn new(params: Vec<LabelValType>, result: Option<Box<ValType>>) -> Self {
        Self { params, result }
    }

    pub fn assert_type(&self, params: Vec<ValType>, result: Option<ValType>) -> ParseResult<()> {
        if self.params.len() != params.len() {
            return Err(ComponentParseError::TypeMismatch(format!(
                "params length mismatch: expected {}, found {}",
                self.params.len(),
                params.len()
            )));
        }
        for (i, (act, exp)) in self.params.iter().zip(params.iter()).enumerate() {
            if &act.t != exp {
                return Err(ComponentParseError::TypeMismatch(format!(
                    "param type mismatch at index {}: expected {:?}, found {:?}",
                    i, exp, act.t
                )));
            }
        }
        match (&self.result, result) {
            (None, None) => Ok(()),
            (Some(act), Some(exp)) if **act != exp => Err(ComponentParseError::TypeMismatch(
                format!("result type mismatch: expected {:?}, found {:?}", act, exp),
            )),
            _ => Ok(()),
        }
    }
}

impl TryFrom<ExternDesc> for FuncType {
    type Error = ComponentParseError;

    fn try_from(value: ExternDesc) -> Result<Self, Self::Error> {
        if let ExternDesc::Func(ty) = value {
            Ok(ty)
        } else {
            Err(ComponentParseError::InvalidType("FuncType".to_string()))
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct LabelValType {
    pub label: Label,
    pub t: ValType,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Case {
    pub label: Label,
    pub t: Option<ValType>,
}

impl Case {
    pub fn new(label: Label, t: Option<ValType>) -> Self {
        Self { label, t }
    }
}
