mod canon;
mod context;
mod error;
mod func;
pub mod instantiate;
mod linker;

use crate::common::InstanceAddr;
use crate::component_model::FlattenComponent;
use crate::runtime::component_model::instantiate::{
    instantiate_next, InstantiateContext, InstantiateInstr,
};
use crate::{Registry, Store};
pub use error::ComponentVMError;
pub use func::*;
pub use linker::Linker;
use std::collections::HashMap;

#[derive(Debug)]
pub struct ComponentInstantiated {
    pub children: Vec<ComponentInstantiated>,
    pub core_instances: Vec<CoreInstantiated>,
    pub core_functions: Vec<CoreFunctionInstantiated>,
    pub functions: Vec<ComponentFunctionInstantiated>,
    pub export: HashMap<String, InstanceExport>,
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

#[derive(Debug)]
pub enum InstanceExport {
    Instance,
}

#[derive(Debug)]
pub struct CoreInstantiated {
    pub(crate) id: InstanceAddr,
    pub(crate) registry: Registry,
}

#[derive(Debug)]
pub struct CoreFunctionInstantiated {}

pub fn instantiate(
    component: FlattenComponent,
    instrs: &mut Vec<InstantiateInstr>,
    store: &mut Store,
    linker: &Linker,
) -> Result<ComponentInstantiated, ComponentVMError> {
    let mut instantiated = ComponentInstantiated::new();
    let ptr = instrs.as_ptr();
    let mut ctx = InstantiateContext::new(store, component, &mut instantiated);
    unsafe {
        instantiate_next(ptr, 0, &mut ctx).unwrap();
    }
    Ok(instantiated)
}
