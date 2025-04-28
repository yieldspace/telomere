use crate::component_model::{
    AliasType, ComponentType, CoreModuleType, CoreSort, CoreType, FuncType, Sort, Type, TypeIdx,
};
use crate::parser::component_model::ComponentParseError;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub struct InstanceType {
    pub(crate) core_types: Vec<CoreType>,
    pub(crate) types: Vec<Type>,
    pub(crate) instances: Vec<InstanceType>,
    pub(crate) exports: HashMap<String, InstanceExportType>,
}

impl InstanceType {
    pub(crate) fn new() -> Self {
        Self {
            core_types: vec![],
            types: vec![],
            instances: vec![],
            exports: Default::default(),
        }
    }

    pub(crate) fn get_export_type(
        &self,
        name: &String,
    ) -> Result<&InstanceExportType, ComponentParseError> {
        self.exports
            .get(name)
            .ok_or_else(|| ComponentParseError::ExportNotFound(name.clone()))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum InstanceExportType {
    CoreModule(CoreModuleType),
    Func(FuncType),
    #[cfg(feature = "component-gated-feature-value-imports-exports")]
    Value(ValueBound),
    Type(Type),
    Component(ComponentType),
    Instance(InstanceType),
}

impl PartialEq<Sort> for InstanceExportType {
    fn eq(&self, other: &Sort) -> bool {
        match other {
            Sort::Core(CoreSort::Module) => matches!(self, InstanceExportType::CoreModule(_)),
            Sort::Func => matches!(self, InstanceExportType::Func(_)),
            #[cfg(feature = "component-gated-feature-value-imports-exports")]
            Sort::Value => matches!(self, InstanceExportType::Value(_)),
            Sort::Type => matches!(self, InstanceExportType::Type(_)),
            Sort::Component => matches!(self, InstanceExportType::Component(_)),
            Sort::Instance => matches!(self, InstanceExportType::Instance(_)),
            _ => false,
        }
    }
}

#[derive(Debug, Clone)]
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
    Eq(TypeIdx),
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
    pub name: String,
    pub ed: ExternDesc,
}

#[derive(Debug, Clone)]
pub struct ExportDecl {
    pub name: String,
    pub ed: ExternDesc,
}
