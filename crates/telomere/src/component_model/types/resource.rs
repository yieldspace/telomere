use crate::component_model::FuncType;

#[derive(Debug, Clone, PartialEq)]
pub enum ResourceType {
    Resource(Option<FuncType>, Option<usize>),
    ResourceWithAsyncCallback(FuncType, Option<FuncType>, Option<usize>),
}
