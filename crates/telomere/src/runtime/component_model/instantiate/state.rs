use crate::component_model::{CoreInstance, CoreModule, GlobalIdx};
use std::collections::HashMap;
use telomere_wasm::common::InstanceHandle;

#[derive(Default)]
pub struct InstantiateState {
    pub(crate) core_instances: HashMap<GlobalIdx<CoreInstance>, InstanceHandle>,
}

impl InstantiateState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert_core_instance(&mut self, idx: GlobalIdx<CoreInstance>, handle: InstanceHandle) {
        self.core_instances.insert(idx, handle);
    }

    pub fn get_core_instance(&self, idx: &GlobalIdx<CoreInstance>) -> Option<&InstanceHandle> {
        self.core_instances.get(idx)
    }
}
