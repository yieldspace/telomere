use crate::common::InstanceAddr;
use crate::component_model::{FlattenComponent, InstanceIdx};
use crate::runtime::component_model::{ComponentInstantiated, CoreInstantiated, Linker};
use crate::{Module, Registry, Store};
use std::collections::HashMap;

pub struct InstantiateContext<'a> {
    pub current: Option<usize>,
    pub(crate) store: &'a mut Store,
    pub instantiated: &'a mut ComponentInstantiated,
    pub component: FlattenComponent,
    pub core_functions: Vec<(InstanceAddr, String)>,
    pub core_memories: Vec<(InstanceAddr, String)>,
    pub core_tables: Vec<(InstanceAddr, String)>,
    pub instances: HashMap<usize, InstantiatedInstance>,
    pub linker: &'a Linker,
}

impl<'a> InstantiateContext<'a> {
    pub fn new(
        store: &'a mut Store,
        component: FlattenComponent,
        instantiated: &'a mut ComponentInstantiated,
        linker: &'a Linker,
    ) -> Self {
        Self {
            current: None,
            store,
            component,
            instantiated,
            core_functions: vec![],
            core_memories: vec![],
            core_tables: vec![],
            instances: Default::default(),
            linker,
        }
    }

    pub fn push_core_module_instance(&mut self, instance: InstanceHandle, registry: Registry) {
        self.instantiated.core_instances.push(CoreInstantiated {
            id: instance,
            registry,
        });
    }
}

pub enum InstantiatedInstanceExport {
    Module(Module),
    Instance,
}

pub struct InstantiatedInstance {
    pub exports: HashMap<String, InstantiatedInstanceExport>,
}
