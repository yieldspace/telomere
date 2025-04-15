mod canon;
mod context;
mod error;
mod func;
pub mod instantiate;
mod linker;

use crate::common::InstanceAddr;
use crate::component_model::FlattenComponent;
use crate::{Registry, Store};
pub use error::ComponentVMError;
pub use func::*;
pub use linker::Linker;
use std::collections::HashMap;

pub struct ComponentInstantiated {
    children: Vec<ComponentInstantiated>,
    core_instances: Vec<CoreInstantiated>,
    core_functions: Vec<CoreFunctionInstantiated>,
    functions: Vec<ComponentFunctionInstantiated>,
    export: HashMap<String, InstanceExport>,
}

impl ComponentInstantiated {
    fn new() -> Self {
        Self {
            children: vec![],
            core_instances: vec![],
            core_functions: vec![],
            functions: vec![],
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

pub struct CoreFunctionInstantiated {}

pub fn instantiate(
    component: FlattenComponent,
    store: &mut Store,
    linker: &Linker,
) -> Result<ComponentInstantiated, ComponentVMError> {
    todo!()
}
