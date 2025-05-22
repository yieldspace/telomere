use crate::common::InstanceHandle;
use crate::runtime::component_model::{ComponentInstantiated, CoreInstantiated, Linker};
use crate::{Registry, Store};
use std::collections::HashMap;

pub struct InstantiateContext<'a> {
    pub current: Option<usize>,
    pub(crate) store: &'a mut Store,
    pub instantiated: &'a mut ComponentInstantiated,
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
        instantiated: &'a mut ComponentInstantiated,
        linker: &'a Linker,
    ) -> Self {
        Self {
            current: None,
            store,
            instantiated,
            core_functions: vec![],
            core_memories: vec![],
            core_tables: vec![],
            resolved_imports: Default::default(),
            instances: Default::default(),
            linker,
        }
    }

    pub fn push_core_module_instance(&mut self, instance: InstanceHandle, registry: Registry) {
        self.instantiated.core_instances.push(CoreInstantiated {
            id: instance,
            registry,
        });
    }
}

pub enum InstantiatedInstanceExport {
    // Module(GlobalIdx<CoreModule>),
    // Instance(GlobalIdx<Instance>),
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
    // pub core_modules: HashMap<GlobalIdx<CoreModule>, Module>,
}

impl Default for ResolvedImportMap {
    fn default() -> Self {
        Self::new()
    }
}

impl ResolvedImportMap {
    pub fn new() -> Self {
        Self {
            // core_modules: Default::default(),
        }
    }
}

pub enum ResolvedImport {
    // CoreModule(GlobalIdx<CoreModule>),
    // Instance(GlobalIdx<Instance>),
}
