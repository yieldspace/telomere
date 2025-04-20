use crate::component_model::{CanonOpt, CoreFuncIdx, FuncIdx, FuncType, Reference, TypeIdx};

pub enum ComponentFunction {
    CanonLift {
        core_func_idx: CoreFuncIdx,
        opts: Vec<CanonOpt>,
        ty: TypeIdx,
    },
    SuperTyped(FuncType, FuncIdx, Reference),
    Typed(TypeIdx, Reference),
}
