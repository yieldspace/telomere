use crate::component_model::{CanonOpt, CoreFuncIdx, CoreFunction, FuncType, GlobalIdx, TypeIdx};

#[derive(Clone)]
pub enum ComponentFunction {
    CanonLift {
        core_func_idx: GlobalIdx<CoreFunction>,
        opts: Vec<CanonOpt>,
        ty: FuncType,
    },
}
