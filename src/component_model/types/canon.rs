use crate::common::ResultType;
use crate::component_model::flatten::Flattenable;
use crate::component_model::{CoreFuncType, FuncType, ResourceType};

pub trait CanonicalFuncType {
    fn canon_lower(ty: FuncType) -> Self;
    fn canon_resource_new(ty: ResourceType) -> Self;
    fn canon_resource_drop(ty: ResourceType) -> Self;
    fn canon_resource_rep(ty: ResourceType) -> Self;
}

impl CanonicalFuncType for CoreFuncType {
    fn canon_lower(ty: FuncType) -> Self {
        let FuncType { params, result } = ty;
        let params = params
            .into_iter()
            .map(|param| param.t.flat())
            .flatten()
            .collect::<Vec<_>>();
        let result = result.map(|x| x.flat()).unwrap_or_default();
        Self(ResultType(params), ResultType(result))
    }

    fn canon_resource_new(ty: ResourceType) -> Self {
        Self(
            crate::common::ResultType(vec![crate::common::ValType::I32]),
            crate::common::ResultType(vec![crate::common::ValType::I32]),
        )
    }

    fn canon_resource_drop(ty: ResourceType) -> Self {
        Self(
            crate::common::ResultType(vec![crate::common::ValType::I32]),
            crate::common::ResultType(vec![crate::common::ValType::I32]),
        )
    }

    fn canon_resource_rep(ty: ResourceType) -> Self {
        Self(
            crate::common::ResultType(vec![crate::common::ValType::I32]),
            crate::common::ResultType(vec![crate::common::ValType::I32]),
        )
    }
}
