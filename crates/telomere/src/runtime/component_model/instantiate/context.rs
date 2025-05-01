use crate::common::InstanceHandle;
use crate::component_model::{
    CompiledState, CoreInstance, CoreModule, CoreSortWithIdx, GlobalIdx, Instance, Relation,
    SortWithIdx,
};
use crate::runtime::component_model::instantiate::error::InstantiateError;
use crate::runtime::component_model::instantiate::InstantiateResult;
use crate::runtime::component_model::{
    ComponentInstantiated, CoreInstanceInstantiated, InstanceInstantiated, Linker,
};
use crate::{Module, Registry, Store};
use std::collections::HashMap;

pub struct CurrentState {
    idx: GlobalIdx<Instance>,
    imports: HashMap<String, SortWithIdx>,
}

impl CurrentState {
    pub fn new(idx: GlobalIdx<Instance>, imports: HashMap<String, SortWithIdx>) -> Self {
        Self { idx, imports }
    }

    pub fn get_import_core_module(
        &self,
        name: &String,
    ) -> InstantiateResult<GlobalIdx<CoreModule>> {
        let import = self
            .imports
            .get(name)
            .ok_or_else(|| InstantiateError::ImportNotFound(name.clone()))?;
        if let SortWithIdx::Core(CoreSortWithIdx::Module(idx, _)) = &import {
            Ok(*idx)
        } else {
            Err(InstantiateError::ImportTypeMismatch(
                name.clone(),
                "Core Instance".to_string(),
                import.to_string(),
            ))
        }
    }

    pub fn get_import_core_instance(
        &self,
        name: &String,
    ) -> InstantiateResult<GlobalIdx<CoreInstance>> {
        let import = self
            .imports
            .get(name)
            .ok_or_else(|| InstantiateError::ImportNotFound(name.clone()))?;
        if let SortWithIdx::Core(CoreSortWithIdx::Instance(idx, _)) = &import {
            Ok(*idx)
        } else {
            Err(InstantiateError::ImportTypeMismatch(
                name.clone(),
                "Core Instance".to_string(),
                import.to_string(),
            ))
        }
    }

    pub fn get_import_instance(&self, name: &String) -> InstantiateResult<GlobalIdx<Instance>> {
        let import = self
            .imports
            .get(name)
            .ok_or_else(|| InstantiateError::ImportNotFound(name.clone()))?;
        if let SortWithIdx::Instance(idx, _) = &import {
            Ok(*idx)
        } else {
            Err(InstantiateError::ImportTypeMismatch(
                name.clone(),
                "Instance".to_string(),
                import.to_string(),
            ))
        }
    }
}

pub struct InstantiateContext<'a> {
    pub current: Option<CurrentState>,
    pub(crate) store: &'a mut Store,
    pub instantiated: &'a mut ComponentInstantiated,
    pub(crate) compiled: &'a CompiledState,
    pub core_functions: Vec<(InstanceHandle, String)>,
    pub core_memories: Vec<(InstanceHandle, String)>,
    pub core_tables: Vec<(InstanceHandle, String)>,
    pub resolved_imports: HashMap<ResolvedImportKey, ResolvedImportMap>,
    pub instances: HashMap<usize, InstantiatedInstance>,
    pub linker: &'a Linker,
}

impl<'a> InstantiateContext<'a> {
    pub fn new(
        store: &'a mut Store,
        compiled: &'a CompiledState,
        instantiated: &'a mut ComponentInstantiated,
        linker: &'a Linker,
    ) -> Self {
        Self {
            current: None,
            store,
            compiled,
            instantiated,
            core_functions: vec![],
            core_memories: vec![],
            core_tables: vec![],
            resolved_imports: Default::default(),
            instances: Default::default(),
            linker,
        }
    }

    pub(crate) fn register_instantiated_core_instance(
        &mut self,
        idx: GlobalIdx<CoreInstance>,
        inst: CoreInstanceInstantiated,
    ) {
        self.instantiated.core_instances.insert(idx, inst);
    }

    pub(crate) fn get_instantiated_core_instance(
        &self,
        idx: &GlobalIdx<CoreInstance>,
    ) -> &CoreInstanceInstantiated {
        self.instantiated.core_instances.get(idx).unwrap()
    }

    pub(crate) fn get_instantiated_instance(
        &self,
        idx: &GlobalIdx<Instance>,
    ) -> &InstanceInstantiated {
        self.instantiated.instances.get(idx).unwrap()
    }

    pub(crate) fn get_core_module(
        &self,
        idx: &GlobalIdx<CoreModule>,
    ) -> InstantiateResult<&CoreModule> {
        match self.compiled.core_modules.get(idx).unwrap() {
            Relation::Defined(module) => Ok(module),
            Relation::Import(name) => {
                if let Some(state) = &self.current {
                    self.get_core_module(&state.get_import_core_module(name)?)
                } else {
                    // toplevel
                    todo!()
                }
            }
            Relation::FromCoreExport(_, _) => unreachable!("module export proposal"),
            Relation::FromExport(idx, name) => {
                let inst = self.get_instantiated_instance(idx);
                let export = inst.get_export_core_module(name)?;
                self.get_core_module(&export)
            }
        }
    }

    pub(crate) fn get_core_instance(
        &self,
        idx: &GlobalIdx<CoreInstance>,
    ) -> InstantiateResult<&CoreInstance> {
        match self.compiled.core_instances.get(&idx).unwrap() {
            Relation::Defined(inst) => Ok(inst),
            Relation::Import(name) => {
                if let Some(state) = &self.current {
                    self.get_core_instance(&state.get_import_core_instance(name)?)
                } else {
                    // toplevel
                    Err(InstantiateError::UnsupportedToplevelImportError(
                        "core instance".to_string(),
                    ))
                }
            }
            Relation::FromCoreExport(_, _) => unreachable!(),
            Relation::FromExport(idx, name) => {
                let inst = self.get_instantiated_instance(idx);
                let export = inst.get_export_core_instance(name)?;
                self.get_core_instance(&export)
            }
        }
    }

    pub(crate) fn get_instance(&self, idx: GlobalIdx<Instance>) -> InstantiateResult<&Instance> {
        match self.compiled.instances.get(&idx).unwrap() {
            Relation::Defined(inst) => Ok(inst),
            Relation::Import(name) => {
                if let Some(state) = &self.current {
                    self.get_instance(state.get_import_instance(name)?)
                } else {
                    todo!()
                }
            }
            Relation::FromCoreExport(_, _) => unreachable!(),
            Relation::FromExport(idx, name) => {
                let inst = self.get_instantiated_instance(idx);
                let export = inst.get_export_instance(name)?;
                self.get_instance(export)
            }
        }
    }
}

pub enum InstantiatedInstanceExport {
    Module(GlobalIdx<CoreModule>),
    Instance(GlobalIdx<Instance>),
}

pub struct InstantiatedInstance {
    pub exports: HashMap<String, InstantiatedInstanceExport>,
}

#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq)]
pub enum ResolvedImportKey {
    Toplevel,
    Child(usize),
}

pub struct ResolvedImportMap {
    pub core_modules: HashMap<GlobalIdx<CoreModule>, Module>,
}

impl ResolvedImportMap {
    pub fn new() -> Self {
        Self {
            core_modules: Default::default(),
        }
    }
}

pub enum ResolvedImport {
    CoreModule(GlobalIdx<CoreModule>),
    Instance(GlobalIdx<Instance>),
}
