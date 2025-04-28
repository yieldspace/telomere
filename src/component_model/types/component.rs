use crate::component_model::{
    CoreModuleType, CoreType, FuncType, GlobalIdx, ImportDecl, Instance, InstanceDecl,
    InstanceType, Type,
};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub struct ComponentType {
    pub(crate) imports: HashMap<String, ComponentImportType>,
    pub(crate) exports: HashMap<String, ComponentExportType>,
    pub(crate) core_types: Vec<GlobalIdx<CoreType>>,
    pub(crate) types: Vec<Type>,
    pub(crate) instances: Vec<GlobalIdx<Instance>>,
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

#[derive(Debug, Clone, PartialEq)]
pub struct ComponentExportType {}

#[derive(Debug, Clone, PartialEq)]
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
