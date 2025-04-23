use crate::common::InstanceHandle;
use crate::component_model::FlattenComponent;
use crate::runtime::component_model::{ComponentInstantiated, CoreInstantiated};
use crate::{Registry, Store};

pub struct InstantiateContext<'a> {
    pub(crate) store: &'a mut Store,
    pub instantiated: &'a mut ComponentInstantiated,
    pub component: FlattenComponent,
    pub core_functions: Vec<(InstanceHandle, String)>,
    pub core_memories: Vec<(InstanceHandle, String)>,
    pub core_tables: Vec<(InstanceHandle, String)>,
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

    pub fn push_core_module_instance(&mut self, instance: InstanceHandle, registry: Registry) {
        self.instantiated.core_instances.push(CoreInstantiated {
            id: instance,
            registry,
        });
    }
}
