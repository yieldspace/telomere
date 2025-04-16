use crate::component_model::{
    CanonOpt, CoreFuncRef, CoreInstanceIdx, FuncIdx, InstanceIdx, TypeIdx,
};

pub enum CoreFunction {
    Export(CoreFuncRef),
    CanonLower(FuncIdx, Vec<CanonOpt>),
    ResourceNew(TypeIdx),
    ResourceDrop(TypeIdx),
}
