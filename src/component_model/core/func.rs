use crate::component_model::{CanonOpt, ComponentFunction, CoreFuncRef, FuncIdx, GlobalIdx, ResourceType, TypeIdx};

#[derive(Debug, Clone)]
pub enum CoreFunction {
    Export(CoreFuncRef),
    CanonLower(GlobalIdx<ComponentFunction>, Vec<CanonOpt>),
    ResourceNew(ResourceType),
    ResourceDrop(ResourceType),
    ResourceRep(ResourceType),
}
