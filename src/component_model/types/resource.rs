use crate::component_model::FuncType;

#[derive(Debug, Clone, PartialEq)]
pub enum ResourceType {
    Resource(Option<FuncType>),
    ResourceWithAsyncCallback(FuncType, Option<FuncType>),
}
