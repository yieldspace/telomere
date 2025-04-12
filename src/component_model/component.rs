use std::collections::HashMap;
use crate::component_model::types::{ExternDesc, Type};
use crate::component_model::{Alias, CanonicalFuncKind, ComponentExport, ComponentImport, CoreFunc, CoreInstance, CoreModule, CoreType, Instance};
use crate::parser::component::SortMap;
use crate::Module;
use std::sync::{Arc, Weak};
use crate::component_model::id::{CoreInstanceIdx, CoreModuleIdx};

#[derive(Debug)]
pub struct Component {
    pub modules: Vec<CoreModule>,
    pub imports: HashMap<String, ExternDesc>,
}

impl Component {
}


pub struct ComponentBuilder {
    modules: Vec<CoreModule>,
    core_instances: Vec<CoreInstance>,
    core_functions: Vec<CoreFunc>,
}

impl ComponentBuilder {
    pub fn new() -> Self {
        Self {
            modules: vec![],
            core_instances: vec![],
            core_functions: vec![],
        }
    }

    pub fn build(self) -> Component {
        let Self {
            modules,
            core_instances,
            core_functions,
        } = self;
        Component {
            modules,
        }
    }

    pub fn register_core_module(&mut self, module: CoreModule) {
        self.modules.push(module);
    }

    pub fn get_core_module(&self, index: usize) -> Option<&CoreModule> {
        self.modules.get(index)
    }

    pub fn register_core_instance(&mut self, instance: CoreInstance) {
        self.core_instances.push(instance);
    }

    pub fn get_core_instance(&self, index: usize) -> Option<&CoreInstance> {
        self.core_instances.get(index)
    }
}
