use crate::component_model::{CanonOpt, CoreFuncIdx, FuncType, TypeIdx};

#[derive(Clone)]
pub struct ComponentFunction {
    pub(crate) value: Option<FuncValue>,
    pub(crate) ty: FuncType,
    // CanonLift {
    //     core_func_idx: CoreFuncIdx,
    //     opts: Vec<CanonOpt>,
    //     ty: TypeIdx,
    // },
    // SuperTyped(FuncType, FuncIdx, Reference),
    // Typed(FuncType, Reference),
}

impl ComponentFunction {
    pub fn new(value: Option<FuncValue>, ty: FuncType) -> Self {
        Self { value, ty }
    }
}

#[derive(Debug, Clone)]
pub enum FuncValue {
    CanonLift {
        core_func_idx: CoreFuncIdx,
        opts: Vec<CanonOpt>,
        ty: TypeIdx,
    },
}
