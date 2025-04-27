use crate::component_model::ValType;

#[derive(Debug, Clone)]
pub struct FuncType {
    pub params: Vec<LabelValType>,
    pub result: Option<Box<ValType>>,
}

#[derive(Debug, Clone)]
pub struct LabelValType {
    pub label: Label,
    pub t: ValType,
}

#[derive(Debug, Clone)]
pub struct Case {
    pub label: Label,
    pub t: Option<ValType>,
}

#[derive(Debug, Clone)]
pub struct Label {
    pub len: usize,
    pub label: String, // TODO: check label format https://github.com/WebAssembly/component-model/blob/main/design/mvp/Explainer.md#import-and-export-definitions
}
