use crate::component_model::{
    CanonOpt, CanonicalOptions, CoreFuncRef, Func, GlobalIdx, ResourceType,
};

#[derive(Debug, Clone)]
pub enum CoreFunc {
    Export(CoreFuncRef),
    CanonLower(GlobalIdx<Func>, CanonicalOptions),
    ResourceNew(ResourceType),
    ResourceDrop(ResourceType),
    ResourceRep(ResourceType),
}
