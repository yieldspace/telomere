#[cfg(feature = "component-gated-feature-value-imports-exports")]
use crate::component_model::ValueIdx;
use crate::component_model::{
    ComponentType, CoreModule, CoreSort, CoreSortWithIdx, ExternDesc, Func, FuncType, GlobalIdx,
    InlineComponent, Instance, InstanceType, Type,
};
use crate::parser::component_model::ComponentParseError;

#[derive(Debug, PartialEq)]
pub enum Sort {
    Core(CoreSort),
    Func,
    Value,
    Type,
    Component,
    Instance,
}

pub trait SortLike {
    fn eq_sort(&self, sort: Sort) -> bool;
}

#[derive(Debug, Clone, PartialEq)]
pub enum SortWithIdx {
    Core(CoreSortWithIdx),
    Func(GlobalIdx<Func>, FuncType),
    #[cfg(feature = "component-gated-feature-value-imports-exports")]
    Value(ValueIdx),
    Type(Type),
    Component(GlobalIdx<InlineComponent>, ComponentType),
    Instance(GlobalIdx<Instance>, InstanceType),
}

impl TryFrom<SortWithIdx> for ExternDesc {
    type Error = ComponentParseError;

    fn try_from(value: SortWithIdx) -> Result<Self, Self::Error> {
        match value {
            SortWithIdx::Core(CoreSortWithIdx::Module(_, ty)) => Ok(ExternDesc::CoreModule(ty)),
            SortWithIdx::Func(_, ty) => Ok(ExternDesc::Func(ty)),
            SortWithIdx::Type(ty) => Ok(ExternDesc::Type(ty)),
            SortWithIdx::Component(_, ty) => Ok(ExternDesc::Component(ty)),
            SortWithIdx::Instance(_, ty) => Ok(ExternDesc::Instance(ty)),
            _ => Err(ComponentParseError::InvalidSortWithIdx(
                value,
                "not valid for externdesc".to_string(),
            )),
        }
    }
}

impl TryFrom<SortWithIdx> for GlobalIdx<CoreModule> {
    type Error = ComponentParseError;

    fn try_from(value: SortWithIdx) -> Result<Self, Self::Error> {
        if let SortWithIdx::Core(CoreSortWithIdx::Module(idx, _)) = value {
            Ok(idx)
        } else {
            Err(ComponentParseError::InvalidSortWithIdx(
                value,
                "CoreModule".to_string(),
            ))
        }
    }
}

impl TryFrom<SortWithIdx> for GlobalIdx<Func> {
    type Error = ComponentParseError;

    fn try_from(value: SortWithIdx) -> Result<Self, Self::Error> {
        if let SortWithIdx::Func(idx, _) = value {
            Ok(idx)
        } else {
            Err(ComponentParseError::InvalidSortWithIdx(
                value,
                "Func".to_string(),
            ))
        }
    }
}

impl TryFrom<SortWithIdx> for Type {
    type Error = ComponentParseError;

    fn try_from(value: SortWithIdx) -> Result<Self, Self::Error> {
        if let SortWithIdx::Type(idx) = value {
            Ok(idx)
        } else {
            Err(ComponentParseError::InvalidSortWithIdx(
                value,
                "Type".to_string(),
            ))
        }
    }
}

impl TryFrom<SortWithIdx> for GlobalIdx<InlineComponent> {
    type Error = ComponentParseError;

    fn try_from(value: SortWithIdx) -> Result<Self, Self::Error> {
        if let SortWithIdx::Component(idx, _) = value {
            Ok(idx)
        } else {
            Err(ComponentParseError::InvalidSortWithIdx(
                value,
                "Component".to_string(),
            ))
        }
    }
}

impl TryFrom<SortWithIdx> for GlobalIdx<Instance> {
    type Error = ComponentParseError;

    fn try_from(value: SortWithIdx) -> Result<Self, Self::Error> {
        if let SortWithIdx::Instance(idx, _) = value {
            Ok(idx)
        } else {
            Err(ComponentParseError::InvalidSortWithIdx(
                value,
                "Instance".to_string(),
            ))
        }
    }
}
