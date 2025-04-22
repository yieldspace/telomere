use crate::component_model::{CanonOpt, CoreFuncIdx, FuncIdx, FuncType, Reference, TypeIdx};

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
        Self {
            value,
            ty,
        }
    }
    
}

pub enum FuncValue {
    CanonLift {
        core_func_idx: CoreFuncIdx,
        opts: Vec<CanonOpt>,
        ty: TypeIdx,
    },
}