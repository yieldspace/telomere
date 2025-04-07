use crate::component_model::id::{
    ComponentIdx, CoreInstanceIdx, CoreModuleIdx, CoreTypeIdx, InstanceIdx,
};
use crate::component_model::types::{Type, TypeKind};
use crate::component_model::{
    Alias, CanonicalFuncKind, Component, ComponentExport, ComponentImport, CoreInstance, CoreType,
    Instance,
};
use crate::Module;
use std::sync::Arc;

#[derive(Debug)]
pub struct SortMap<'a> {
    parent: Option<&'a SortMap<'a>>,
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
    type_table: Vec<TypeKind>,
}

impl<'a> SortMap<'a> {
    pub fn new(parent: Option<&'a SortMap<'a>>) -> Self {
        SortMap {
            parent,
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
            type_table: vec![],
        }
    }

    pub fn add_core_module(&mut self, module: Arc<Module>) {
        self.modules.push(module);
    }

    pub fn add_core_instance(&mut self, instance: Arc<CoreInstance>) {
        self.core_instances.push(instance);
    }

    pub fn add_core_type(&mut self, core_type: Arc<CoreType>) {
        self.core_types.push(core_type);
    }

    pub fn add_component(&mut self, component: Arc<Component>) {
        self.components.push(component);
    }

    pub fn add_instance(&mut self, instance: Arc<Instance>) {
        self.instances.push(instance);
    }

    pub fn add_alias(&mut self, alias: Arc<Alias>) {
        self.aliases.push(alias);
    }

    pub fn add_type(&mut self, core_type: Arc<Type>) {
        self.type_table.push(TypeKind::Type(Arc::downgrade(&core_type)));
        self.types.push(core_type);
    }

    pub fn add_canon(&mut self, canon: Arc<CanonicalFuncKind>) {
        self.canons.push(canon);
    }

    pub fn add_import(&mut self, import: Arc<ComponentImport>) {
        self.imports.push(import);
    }

    pub fn add_export(&mut self, export: Arc<ComponentExport>) {
        self.exports.push(export);
    }

    pub fn get_core_module_idx(&self, id: usize) -> Option<CoreModuleIdx> {
        self.modules
            .get(id)
            .map(|m| CoreModuleIdx(Arc::downgrade(m)))
    }

    pub fn get_core_instance_idx(&self, id: usize) -> Option<CoreInstanceIdx> {
        self.core_instances
            .get(id)
            .map(|i| CoreInstanceIdx(Arc::downgrade(i)))
    }

    pub fn get_core_type_idx(&self, id: usize) -> Option<CoreTypeIdx> {
        self.core_types
            .get(id)
            .map(|t| CoreTypeIdx(Arc::downgrade(t)))
    }

    pub fn get_instance_idx(&self, id: usize) -> Option<InstanceIdx> {
        self.instances
            .get(id)
            .map(|i| InstanceIdx(Arc::downgrade(i)))
    }

    pub fn get_component_idx(&self, id: usize) -> Option<ComponentIdx> {
        self.components
            .get(id)
            .map(|i| ComponentIdx(Arc::downgrade(i)))
    }
}
