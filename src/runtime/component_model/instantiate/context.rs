use crate::common::InstanceAddr;
use crate::component_model::FlattenComponent;
use crate::runtime::component_model::ComponentInstantiated;
use crate::{Module, Registry, Store};
use std::cell::Cell;
use std::collections::HashMap;
use std::sync::Arc;
use typed_arena::Arena;

pub struct ScopedTable {
    pub core_functions: Vec<(InstanceAddr, String)>,
    pub core_memories: Vec<(InstanceAddr, String)>,
    pub core_tables: Vec<(InstanceAddr, String)>,
}

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
        todo!()
    }
}
