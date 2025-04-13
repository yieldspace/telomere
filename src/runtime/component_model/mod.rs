use crate::common::InstanceAddr;
use crate::component_model::{CanonicalFuncKind, Component, CoreFuncIdx, CoreType, Idx};
use crate::{Module, Registry, Store};
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

pub struct Linker {}

impl Linker {}

pub fn instantiate(
    component: Component,
    store: &mut Store,
    linker: Linker,
) -> ComponentInstantiated {
    let component_instance = ComponentInstantiated::new();

    let mut compiled_core_instances = vec![];

    for core_instance in &component.core_instances {
        let compiled = core_instance.instantiate(store, &component, &component_instance);
        compiled_core_instances.push(compiled);
    }

    todo!()
}
