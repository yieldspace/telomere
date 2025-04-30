use crate::component_model::{ExternDesc, ValType};
use crate::parser::component_model::ComponentParseError;

#[derive(Debug, Clone, PartialEq)]
pub struct FuncType {
    pub params: Vec<LabelValType>,
    pub result: Option<Box<ValType>>,
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

#[derive(Debug, Clone, PartialEq)]
pub struct Label {
    pub label: String, // TODO: check label format https://github.com/WebAssembly/component-model/blob/main/design/mvp/Explainer.md#import-and-export-definitions
}

impl Label {
    pub fn new<IntoString: Into<String>>(label: IntoString) -> Self {
        Self {
            label: label.into(),
        }
    }
}
