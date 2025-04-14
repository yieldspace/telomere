mod error;
mod linker;

use crate::common::InstanceAddr;
use crate::component_model::Component;
use crate::{Registry, Store};
pub use error::ComponentVMError;
pub use linker::Linker;
use std::collections::HashMap;

pub struct ComponentInstantiated {
    children: Vec<ComponentInstantiated>,
    core_instances: Vec<CoreInstantiated>,
    export: HashMap<String, InstanceExport>,
}

impl ComponentInstantiated {
    fn new() -> Self {
        Self {
            children: vec![],
            core_instances: vec![],
            export: HashMap::new(),
        }
    }

    pub(crate) fn get_core_instance(&self, idx: usize) -> Option<&CoreInstantiated> {
        self.core_instances.get(idx)
    }
}

pub enum InstanceExport {
    Instance,
}

pub struct CoreInstantiated {
    pub(crate) id: InstanceAddr,
    pub(crate) registry: Registry,
}

pub fn instantiate(
    component: Component,
    store: &mut Store,
    linker: &Linker,
) -> Result<ComponentInstantiated, ComponentVMError> {
    let mut component_instance = ComponentInstantiated::new();

    for core_instance in &component.core_instances {
        let compiled = core_instance.instantiate(store, &component, &component_instance, linker);
        component_instance.core_instances.push(compiled);
    }

    todo!()
}
