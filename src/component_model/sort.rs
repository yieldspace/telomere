#[cfg(feature = "component-gated-feature-value-imports-exports")]
use crate::component_model::ValueIdx;
use crate::component_model::{
    CoreModule, CoreSort, CoreSortWithIdx, Func, GlobalIdx, InlineComponent, Instance, Type,
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
    Func(GlobalIdx<Func>),
    #[cfg(feature = "component-gated-feature-value-imports-exports")]
    Value(ValueIdx),
    Type(Type),
    Component(GlobalIdx<InlineComponent>),
    Instance(GlobalIdx<Instance>),
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

impl TryFrom<SortWithIdx> for GlobalIdx<CoreModule> {
    type Error = ComponentParseError;

    fn try_from(value: SortWithIdx) -> Result<Self, Self::Error> {
        if let SortWithIdx::Core(CoreSortWithIdx::Module(idx)) = value {
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
        if let SortWithIdx::Func(idx) = value {
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
        if let SortWithIdx::Component(idx) = value {
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
        if let SortWithIdx::Instance(idx) = value {
            Ok(idx)
        } else {
            Err(ComponentParseError::InvalidSortWithIdx(
                value,
                "Instance".to_string(),
            ))
        }
    }
}
