use crate::component_model::FuncIdx;

#[derive(Debug, Clone)]
pub enum ResourceType {
    Resource(Option<FuncIdx>),
    ResourceWithAsyncCallback(FuncIdx, Option<FuncIdx>),
}
