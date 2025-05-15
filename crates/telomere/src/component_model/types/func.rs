use crate::component_model::types::{LabelValType, ValType};

#[derive(Clone)]
pub struct FuncType {
    pub params: Vec<LabelValType>,
    pub result: Option<ValType>,
}
