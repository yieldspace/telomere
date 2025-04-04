use crate::component_model::types::Type;
use crate::component_model::{
    Alias, CanonicalFuncKind, ComponentExport, ComponentImport, CoreInstance, CoreType, Instance,
    Sort,
};
use crate::parser::component::SortMap;
use crate::Module;
use std::sync::Arc;

#[derive(Debug)]
pub struct Component {
    pub modules: Vec<Arc<Module>>,
    pub core_instances: Vec<Arc<CoreInstance>>,
    pub core_types: Vec<Arc<CoreType>>,
    pub components: Vec<Arc<Component>>,
    pub instances: Vec<Arc<Instance>>,
    pub aliases: Vec<Arc<Alias>>,
    pub types: Vec<Arc<Type>>,
    pub canons: Vec<Arc<CanonicalFuncKind>>,
    pub imports: Vec<Arc<ComponentImport>>,
    pub exports: Vec<Arc<ComponentExport>>,
}

impl Component {
    pub fn new() -> Self {
        Self {
            modules: vec![],
            core_instances: vec![],
            core_types: vec![],
            components: vec![],
            instances: vec![],
            aliases: vec![],
            types: vec![],
            canons: vec![],
            imports: vec![],
            exports: vec![],
        }
    }
}

impl<'a> From<SortMap<'a>> for Component {
    fn from(value: SortMap) -> Self {
        let SortMap {
            modules,
            core_instances,
            core_types,
            components,
            instances,
            aliases,
            types,
            canons,
            imports,
            exports,
            ..
        } = value;
        Self {
            modules,
            core_instances,
            core_types,
            components,
            instances,
            aliases,
            types,
            canons,
            imports,
            exports,
        }
    }
}
