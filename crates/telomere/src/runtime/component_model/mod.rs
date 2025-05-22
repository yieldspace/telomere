mod canon;
mod context;
mod error;
mod func;
#[allow(clippy::missing_safety_doc)]
pub mod instantiate;
mod linker;

use crate::common::InstanceHandle;
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
    pub core_instances: Vec<CoreInstantiated>,
    pub core_functions: Vec<CoreFunctionInstantiated>,
    pub functions: Vec<ComponentFunctionInstantiated>,
    pub export: HashMap<String, InstanceExport>,
}

impl ComponentInstantiated {
    fn new() -> Self {
        Self {
            core_instances: vec![],
            core_functions: vec![],
            functions: vec![],
            export: HashMap::new(),
        }
    }
}

#[derive(Debug)]
pub enum InstanceExport {
    Instance,
}

#[allow(dead_code)]
#[derive(Debug)]
pub struct CoreInstantiated {
    pub(crate) id: InstanceHandle,
    pub(crate) registry: Registry,
}

#[derive(Debug)]
pub struct CoreFunctionInstantiated {}

pub fn instantiate(
    instrs: &mut [InstantiateInstr],
    store: &mut Store,
    linker: &Linker,
) -> Result<ComponentInstantiated, ComponentVMError> {
    let mut instantiated = ComponentInstantiated::new();
    let ptr = instrs.as_ptr();
    let mut ctx = InstantiateContext::new(store, &mut instantiated, linker);
    unsafe {
        instantiate_next(ptr, 0, &mut ctx).unwrap();
    }
    Ok(instantiated)
}
