use crate::component_model::{CanonOpt, CoreFuncRef, Func, GlobalIdx, ResourceType};

#[derive(Debug, Clone)]
pub enum CoreFunc {
    Export(CoreFuncRef),
    CanonLower(GlobalIdx<Func>, Vec<CanonOpt>),
    ResourceNew(ResourceType),
    ResourceDrop(ResourceType),
    ResourceRep(ResourceType),
}
