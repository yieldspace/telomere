use crate::component_model::{CanonOpt, CanonicalOptions, CoreFuncRef, Func, FuncType, GlobalIdx, ResourceType};

#[derive(Debug, Clone)]
pub enum CoreFunc {
    Export(CoreFuncRef),
    CanonLower(GlobalIdx<Func>, FuncType, CanonicalOptions),
    ResourceNew(ResourceType),
    ResourceDrop(ResourceType),
    ResourceRep(ResourceType),
}
