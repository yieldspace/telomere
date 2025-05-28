use crate::common::InstanceHandle;
use crate::component_model::{CoreInstance, CoreModule, CoreRelation, GlobalIdx};
use crate::parser::component_model::ParsedComponent;
use crate::runtime::component_model::instantiate::InstantiateResult;
use crate::runtime::component_model::{
    ComponentModelInstance, ComponentVMError, CoreInstantiated, Linker,
};
use crate::{Registry, Store};
use std::collections::HashMap;

#[allow(dead_code)]
pub struct InstantiateContext<'a> {
    component: ParsedComponent,
    pub current: Option<usize>,
    pub(crate) store: &'a mut Store,
    pub instantiated: &'a mut ComponentModelInstance,
    pub linker: &'a Linker,
}

impl<'a> InstantiateContext<'a> {
    pub fn new(
        component: ParsedComponent,
        store: &'a mut Store,
        instantiated: &'a mut ComponentModelInstance,
        linker: &'a Linker,
    ) -> Self {
        Self {
            component,
            current: None,
            store,
            instantiated,
            linker,
        }
    }

    pub fn get_core_module(&self, idx: GlobalIdx<CoreModule>) -> InstantiateResult<&CoreModule> {
        if let Some(instance) = self.component.core_modules.get(&idx) {
            match instance {
                CoreRelation::Defined(inst) => Ok(inst),
                CoreRelation::ImportModule(_) => todo!(),
                CoreRelation::FromExport(_, _) => todo!(),
                CoreRelation::FromCoreExport(_, _) => todo!(),
            }
        } else {
            Err(ComponentVMError::TypeMismatch(format!(
                "core module not found: {:?}",
                idx
            )))
        }
    }

    pub fn get_core_instance(
        &self,
        idx: GlobalIdx<CoreInstance>,
    ) -> InstantiateResult<&CoreInstance> {
        if let Some(instance) = self.component.core_instances.get(&idx) {
            match instance {
                CoreRelation::Defined(inst) => Ok(inst),
                CoreRelation::ImportModule(_) => Err(ComponentVMError::TypeMismatch(
                    "core instance cannot import".into(),
                )),
                CoreRelation::FromExport(_, _) => Err(ComponentVMError::TypeMismatch(
                    "core instance cannot export from instance".into(),
                )),
                CoreRelation::FromCoreExport(_, _) => unimplemented!("module link proposal"),
            }
        } else {
            Err(ComponentVMError::TypeMismatch(format!(
                "core instance not found: {:?}",
                idx
            )))
        }
    }
}
