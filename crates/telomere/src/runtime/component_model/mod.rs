mod canon;
mod context;
mod error;
mod func;
#[allow(clippy::missing_safety_doc)]
pub mod instantiate;
mod linker;

pub use crate::runtime::component_model::instantiate::instantiate;
pub use error::ComponentVMError;
pub use func::*;
pub use linker::Linker;
use std::collections::HashMap;
use telomere_wasm::common::InstanceHandle;
use telomere_wasm::{Registry, Store};

#[derive(Debug)]
pub struct ComponentModelInstance {
    pub core_instances: Vec<CoreInstantiated>,
    pub core_functions: Vec<CoreFunctionInstantiated>,
    pub functions: Vec<ComponentFunctionInstantiated>,
    pub export: HashMap<String, InstanceExport>,
}

impl ComponentModelInstance {
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
