use crate::component_model::{CanonOpt, FuncIdx, TypeIdx};

pub enum CoreFunction {
    Export(),
    CanonLower(FuncIdx, Vec<CanonOpt>),
    ResourceNew(TypeIdx),
    ResourceDrop(TypeIdx),
}
