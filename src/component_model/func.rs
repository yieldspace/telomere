use crate::component_model::{CanonOpt, CoreFuncIdx, TypeIdx};

pub struct ComponentFunction {
    pub core_func_idx: CoreFuncIdx,
    pub opts: Vec<CanonOpt>,
    pub ty: TypeIdx,
}
