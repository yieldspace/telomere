#[cfg(feature = "component-gated-feature-value-imports-exports")]
use crate::component_model::ValueIdx;
use crate::component_model::{
    ComponentIdx, CoreModuleIdx, CoreSort, CoreSortWithIdx, FuncIdx, InstanceIdx, TypeIdx,
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

#[derive(Debug, Clone)]
pub enum SortWithIdx {
    Core(CoreSortWithIdx),
    Func(FuncIdx),
    #[cfg(feature = "component-gated-feature-value-imports-exports")]
    Value(ValueIdx),
    Type(TypeIdx),
    Component(ComponentIdx),
    Instance(InstanceIdx),
}

impl SortWithIdx {
    pub(crate) fn eq_sort(&self, sort: &Sort) -> bool {
        match self {
            SortWithIdx::Core(_) => match sort {
                Sort::Core(CoreSort::Func) => sort == &Sort::Core(CoreSort::Func),
                Sort::Core(CoreSort::Table) => sort == &Sort::Core(CoreSort::Table),
                Sort::Core(CoreSort::Memory) => sort == &Sort::Core(CoreSort::Memory),
                Sort::Core(CoreSort::Global) => sort == &Sort::Core(CoreSort::Global),
                Sort::Core(CoreSort::Type) => sort == &Sort::Core(CoreSort::Type),
                Sort::Core(CoreSort::Module) => sort == &Sort::Core(CoreSort::Module),
                Sort::Core(CoreSort::Instance) => sort == &Sort::Core(CoreSort::Instance),
                _ => false,
            },
            SortWithIdx::Func(_) => sort == &Sort::Func,
            #[cfg(feature = "component-gated-feature-value-imports-exports")]
            SortWithIdx::Value(_) => sort == &Sort::Value,
            SortWithIdx::Type(_) => sort == &Sort::Type,
            SortWithIdx::Component(_) => sort == &Sort::Component,
            SortWithIdx::Instance(_) => sort == &Sort::Instance,
        }
    }
}

impl TryFrom<SortWithIdx> for CoreModuleIdx {
    type Error = ComponentParseError;

    fn try_from(value: SortWithIdx) -> Result<Self, Self::Error> {
        if let SortWithIdx::Core(CoreSortWithIdx::Module(idx)) = value {
            Ok(idx)
        } else {
            Err(ComponentParseError::InvalidSort(
                value,
                "CoreModule".to_string(),
            ))
        }
    }
}

impl TryFrom<SortWithIdx> for FuncIdx {
    type Error = ComponentParseError;

    fn try_from(value: SortWithIdx) -> Result<Self, Self::Error> {
        if let SortWithIdx::Func(idx) = value {
            Ok(idx)
        } else {
            Err(ComponentParseError::InvalidSort(value, "Func".to_string()))
        }
    }
}

impl TryFrom<SortWithIdx> for TypeIdx {
    type Error = ComponentParseError;

    fn try_from(value: SortWithIdx) -> Result<Self, Self::Error> {
        if let SortWithIdx::Type(idx) = value {
            Ok(idx)
        } else {
            Err(ComponentParseError::InvalidSort(value, "Type".to_string()))
        }
    }
}

impl TryFrom<SortWithIdx> for ComponentIdx {
    type Error = ComponentParseError;

    fn try_from(value: SortWithIdx) -> Result<Self, Self::Error> {
        if let SortWithIdx::Component(idx) = value {
            Ok(idx)
        } else {
            Err(ComponentParseError::InvalidSort(
                value,
                "Component".to_string(),
            ))
        }
    }
}

impl TryFrom<SortWithIdx> for InstanceIdx {
    type Error = ComponentParseError;

    fn try_from(value: SortWithIdx) -> Result<Self, Self::Error> {
        if let SortWithIdx::Instance(idx) = value {
            Ok(idx)
        } else {
            Err(ComponentParseError::InvalidSort(
                value,
                "Instance".to_string(),
            ))
        }
    }
}
