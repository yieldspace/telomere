use crate::component_model::flatten::FlatType;
use crate::component_model::{CanonicalOptions, CoreFuncType, FuncType, ResourceType};

pub trait CanonicalFuncType {
    fn canon_lower(ty: FuncType, opt: &CanonicalOptions) -> Self;
    fn canon_resource_new(ty: ResourceType) -> Self;
    fn canon_resource_drop(ty: ResourceType) -> Self;
    fn canon_resource_rep(ty: ResourceType) -> Self;
}

impl CanonicalFuncType for CoreFuncType {
    fn canon_lower(ty: FuncType, opt: &CanonicalOptions) -> Self {
        ty.flat(opt, FlatType::Lower)
    }

    fn canon_resource_new(_ty: ResourceType) -> Self {
        Self(
            crate::common::ResultType(vec![crate::common::ValType::I32]),
            crate::common::ResultType(vec![crate::common::ValType::I32]),
        )
    }

    fn canon_resource_drop(_ty: ResourceType) -> Self {
        Self(
            crate::common::ResultType(vec![crate::common::ValType::I32]),
            crate::common::ResultType(vec![crate::common::ValType::I32]),
        )
    }

    fn canon_resource_rep(_ty: ResourceType) -> Self {
        Self(
            crate::common::ResultType(vec![crate::common::ValType::I32]),
            crate::common::ResultType(vec![crate::common::ValType::I32]),
        )
    }
}
