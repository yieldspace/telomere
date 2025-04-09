use std::collections::HashMap;
use std::ops::Deref;
use std::sync::Arc;
use crate::component_model::{Component, ComponentImport};
use crate::{Module};
use crate::component_model::types::ExternDesc;

pub struct ComponentInstance {
    instances: Vec<ComponentInstance>,
    core_module_instances: Vec<CoreModuleInstance>,
}

pub struct CoreModuleInstance {
    module: Module,
}

pub struct ComponentFunctionInstance {

}

pub struct ComponentRegistry {
    components: HashMap<String, ComponentInstance>,
    core_modules: HashMap<String, CoreModuleInstance>,
    component_functions: HashMap<String, ComponentFunctionInstance>,
    children: HashMap<String, ComponentRegistry>,
}

impl ComponentRegistry {
    pub fn new() -> Self {
        Self {
            components: HashMap::new(),
            core_modules: HashMap::new(),
            component_functions: HashMap::new(),
            children: HashMap::new(),
        }
    }

    pub fn create_child<IntoString>(&mut self, name: IntoString) -> &mut ComponentRegistry where IntoString: Into<String> {
        let child = ComponentRegistry::new();
        let name = name.into();
        self.children.insert(name.clone(), child);
        self.children.get_mut(&name).unwrap()
    }
}

pub fn instantiate(component: Component, registry: &ComponentRegistry) -> ComponentInstance {
    // importを処理する
    for import in &component.imports {
        match import.deref() {
            _ => {}
        }
    }
    // exportを処理する
    // child componentを処理する
    ComponentInstance {
        instances: vec![],
        core_module_instances: vec![],
    }
}
