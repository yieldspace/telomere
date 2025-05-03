use crate::component_model::FuncType;

#[derive(Debug, Clone, PartialEq)]
pub enum ResourceType {
    /// sub resourceで作成されたresourceはidを持ちます
    Resource(Option<FuncType>, Option<usize>),
    ResourceWithAsyncCallback(FuncType, Option<FuncType>, Option<usize>),
}
