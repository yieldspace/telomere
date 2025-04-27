use crate::component_model::{FuncIdx, FuncType};

#[derive(Debug, Clone)]
pub enum ResourceType {
    Resource(Option<FuncType>),
    ResourceWithAsyncCallback(FuncIdx, Option<FuncType>),
}
