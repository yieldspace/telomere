use crate::component_model::{CanonOpt, CoreFuncRef, FuncIdx, TypeIdx};

pub enum CoreFunction {
    Export(CoreFuncRef),
    CanonLower(FuncIdx, Vec<CanonOpt>),
    ResourceNew(TypeIdx),
    ResourceDrop(TypeIdx),
}
