use crate::common::InstanceAddr;
use crate::component_model::FlattenComponent;
use crate::runtime::component_model::{ComponentInstantiated, CoreInstantiated};
use crate::{Registry, Store};

pub struct InstantiateContext<'a> {
    pub(crate) store: &'a mut Store,
    pub instantiated: &'a mut ComponentInstantiated,
    pub component: FlattenComponent,
    pub core_functions: Vec<(InstanceAddr, String)>,
    pub core_memories: Vec<(InstanceAddr, String)>,
    pub core_tables: Vec<(InstanceAddr, String)>,
}

impl<'a> InstantiateContext<'a> {
    pub fn new(
        store: &'a mut Store,
        component: FlattenComponent,
        instantiated: &'a mut ComponentInstantiated,
    ) -> Self {
        Self {
            store,
            component,
            instantiated,
            core_functions: vec![],
            core_memories: vec![],
            core_tables: vec![],
        }
    }

    pub fn push_core_module_instance(&mut self, instance: InstanceAddr, registry: Registry) {
        self.instantiated.core_instances.push(CoreInstantiated {
            id: instance,
            registry,
        });
    }
}
