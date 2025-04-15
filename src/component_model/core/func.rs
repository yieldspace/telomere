use crate::component_model::{CanonOpt, CoreFuncIdx, FlattenComponent, FuncIdx, Idx, TypeIdx};
use crate::Store;

pub enum CoreFunction {
    Export(),
    CanonLower(FuncIdx, Vec<CanonOpt>),
    ResourceNew(TypeIdx),
    ResourceDrop(TypeIdx),
}
