mod canon;
mod context;
mod error;
mod func;
#[allow(clippy::missing_safety_doc)]
pub mod instantiate;
mod linker;

use crate::common::InstanceHandle;
use crate::component_model::{
    CompiledState, CoreInstance, CoreModule, CoreSortWithIdx, GlobalIdx, Instance, SortWithIdx,
};
use crate::runtime::component_model::instantiate::{
    instantiate_next, InstantiateContext, InstantiateError, InstantiateResult,
};
use crate::{Registry, Store};
pub use error::ComponentVMError;
pub use func::*;
pub use linker::Linker;
use std::collections::HashMap;

#[derive(Default)]
pub struct ComponentInstantiated {
    pub core_instances: HashMap<GlobalIdx<CoreInstance>, CoreInstanceInstantiated>,
    pub core_functions: Vec<CoreFunctionInstantiated>,
    pub instances: HashMap<GlobalIdx<Instance>, InstanceInstantiated>,
    pub functions: Vec<ComponentFunctionInstantiated>,
    pub export: HashMap<String, InstanceExport>,
}

impl ComponentInstantiated {
    fn new() -> Self {
        Self::default()
    }
}

#[derive(Debug)]
pub enum InstanceExport {
    Instance,
}

pub struct CoreInstanceInstantiated {
    pub(crate) handle: InstanceHandle,
    #[allow(dead_code)]
    pub(crate) registry: Registry,
}

pub struct InstanceInstantiated {
    pub(crate) exports: HashMap<String, SortWithIdx>,
}

impl InstanceInstantiated {
    pub fn get_export_core_module(
        &self,
        name: &String,
    ) -> InstantiateResult<GlobalIdx<CoreModule>> {
        let value = self
            .exports
            .get(name)
            .ok_or(InstantiateError::ExportNotFound(name.clone()))?;
        let SortWithIdx::Core(CoreSortWithIdx::Module(idx, _)) = value else {
            return Err(InstantiateError::ExportTypeMismatch(
                name.clone(),
                "Core Module".to_string(),
                value.to_string(),
            ));
        };
        Ok(*idx)
    }
    pub fn get_export_core_instance(
        &self,
        name: &String,
    ) -> InstantiateResult<GlobalIdx<CoreInstance>> {
        let value = self
            .exports
            .get(name)
            .ok_or(InstantiateError::ExportNotFound(name.clone()))?;
        let SortWithIdx::Core(CoreSortWithIdx::Instance(idx, _)) = value else {
            return Err(InstantiateError::ExportTypeMismatch(
                name.clone(),
                "Core Instance".to_string(),
                value.to_string(),
            ));
        };
        Ok(*idx)
    }

    pub fn get_export_instance(&self, name: &String) -> InstantiateResult<GlobalIdx<Instance>> {
        let value = self
            .exports
            .get(name)
            .ok_or(InstantiateError::ExportNotFound(name.clone()))?;
        let SortWithIdx::Instance(idx, _) = value else {
            return Err(InstantiateError::ExportTypeMismatch(
                name.clone(),
                "Instance".to_string(),
                value.to_string(),
            ));
        };
        Ok(*idx)
    }
}

#[derive(Debug)]
pub struct CoreFunctionInstantiated {}

pub fn instantiate(
    compiled: &CompiledState,
    store: &mut Store,
    linker: &Linker,
) -> Result<ComponentInstantiated, ComponentVMError> {
    let mut instantiated = ComponentInstantiated::new();
    let ptr = compiled.instrs.as_ptr();
    let mut ctx = InstantiateContext::new(store, &compiled, &mut instantiated, linker);
    unsafe {
        instantiate_next(ptr, 0, &mut ctx).unwrap();
    }
    Ok(instantiated)
}
