use crate::component_model::{
    CoreModuleType, CoreTypeIdx, FuncType, ImportDecl, InstanceDecl, InstanceIdx, InstanceType,
    Type, TypeIdx,
};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct ComponentType {
    pub(crate) imports: HashMap<String, ComponentImportType>,
    pub(crate) exports: HashMap<String, ComponentExportType>,
    pub(crate) core_types: Vec<CoreTypeIdx>,
    pub(crate) types: Vec<TypeIdx>,
    pub(crate) instances: Vec<InstanceIdx>,
}

impl ComponentType {
    pub fn new() -> Self {
        Self {
            imports: HashMap::new(),
            exports: HashMap::new(),
            core_types: Vec::new(),
            types: Vec::new(),
            instances: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ComponentExportType {}

#[derive(Debug, Clone)]
pub enum ComponentImportType {
    CoreModule(CoreModuleType),
    Func(FuncType),
    #[cfg(feature = "component-gated-feature-value-imports-exports")]
    Value(crate::component_model::types::instance::ValueBound),
    Type(Type),
    Component(ComponentType),
    Instance(InstanceType),
}

#[derive(Debug, Clone)]
pub enum ComponentDecl {
    Import(ImportDecl),
    Instance(InstanceDecl),
}
