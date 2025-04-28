use crate::component_model::{CanonOpt, CoreFunc, FuncType, GlobalIdx};

#[derive(Clone)]
pub enum Func {
    CanonLift {
        core_func_idx: GlobalIdx<CoreFunc>,
        opts: Vec<CanonOpt>,
        ty: FuncType,
    },
}
