use crate::component_model::{
    AliasType, ComponentType, CoreModuleType, CoreSort, ExportName, FuncType, ImportName, Sort,
    Type,
};
use crate::parser::component_model::ComponentParseError;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub struct InstanceType {
    pub(crate) imports: HashMap<ImportName, ExternDesc>,
    pub(crate) exports: HashMap<ExportName, ExternDesc>,
}

impl InstanceType {
    pub(crate) fn new() -> Self {
        Self {
            imports: Default::default(),
            exports: Default::default(),
        }
    }

    pub(crate) fn get_export_type(
        &self,
        name: &ExportName,
    ) -> Result<&ExternDesc, ComponentParseError> {
        self.exports
            .get(name)
            .ok_or_else(|| ComponentParseError::ExportNotFound(name.clone()))
    }
}

impl TryFrom<ExternDesc> for InstanceType {
    type Error = ComponentParseError;

    fn try_from(value: ExternDesc) -> Result<Self, Self::Error> {
        if let ExternDesc::Instance(instance_type) = value {
            Ok(instance_type)
        } else {
            Err(ComponentParseError::InvalidType("InstanceType".to_string()))
        }
    }
}

impl PartialEq<Sort> for ExternDesc {
    fn eq(&self, other: &Sort) -> bool {
        match other {
            Sort::Core(CoreSort::Module) => matches!(self, ExternDesc::CoreModule(_)),
            Sort::Func => matches!(self, ExternDesc::Func(_)),
            #[cfg(feature = "component-gated-feature-value-imports-exports")]
            Sort::Value => matches!(self, ExternDesc::Value(_)),
            Sort::Type => matches!(self, ExternDesc::Type(_)),
            Sort::Component => matches!(self, ExternDesc::Component(_)),
            Sort::Instance => matches!(self, ExternDesc::Instance(_)),
            _ => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExternDesc {
    CoreModule(CoreModuleType),
    Func(FuncType),
    #[cfg(feature = "component-gated-feature-value-imports-exports")]
    Value(ValueBound),
    Type(Type),
    Component(ComponentType),
    Instance(InstanceType),
}

#[derive(Debug, Clone)]
pub enum TypeBound {
    Eq(Type),
    Sub,
}

#[derive(Debug, Clone)]
#[cfg(feature = "component-gated-feature-value-imports-exports")]
pub enum ValueBound {
    Eq(usize),
    Type(ValType),
}

#[derive(Debug, Clone)]
pub enum InstanceDecl {
    CoreModuleType(CoreModuleType),
    Type(Type),
    Alias(AliasType),
    ExportDecl(ExportDecl),
}

#[derive(Debug, Clone)]
pub struct ImportDecl {
    pub name: ImportName,
    pub ed: ExternDesc,
}

#[derive(Debug, Clone)]
pub struct ExportDecl {
    pub name: ExportName,
    pub ed: ExternDesc,
}
