use crate::component_model::{CanonOpt, CoreFuncRef, FuncIdx, TypeIdx};

#[derive(Debug, Clone)]
pub enum CoreFunction {
    Export(CoreFuncRef),
    CanonLower(FuncIdx, Vec<CanonOpt>),
    ResourceNew(TypeIdx),
    ResourceDrop(TypeIdx),
    ResourceRep(TypeIdx),
}
