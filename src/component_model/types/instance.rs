use std::collections::HashMap;
use crate::component_model::{AliasIdx, AliasType, ComponentType, CoreModuleType, CoreType, CoreTypeIdx, FuncType, InstanceIdx, Type, TypeIdx};

#[derive(Debug, Clone)]
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
}

#[derive(Debug, Clone)]
pub enum InstanceExportType {
    CoreModule(CoreModuleType),
    Func(FuncType),
    #[cfg(feature = "component-gated-feature-value-imports-exports")]
    Value(ValueBound),
    Type(Type),
    Component(ComponentType),
    Instance(InstanceType),
}

#[derive(Debug, Clone)]
pub enum ExternDesc {
    CoreModule(CoreModuleType),
    Func(FuncType),
    #[cfg(feature = "component-gated-feature-value-imports-exports")]
    Value(ValueBound),
    Type(TypeBound),
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