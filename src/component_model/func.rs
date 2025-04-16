use crate::component_model::{CanonOpt, CoreFuncIdx, TypeIdx};

pub struct ComponentFunction {
    core_func_idx: CoreFuncIdx,
    opts: Vec<CanonOpt>,
    ty: TypeIdx,
}
