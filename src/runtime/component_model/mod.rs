use crate::common::InstanceAddr;
use crate::component_model::{CanonicalFuncKind, CoreFuncIdx, CoreType, Idx};
use crate::runtime::instantiate::aliasing as core_aliasing;
use crate::runtime::instantiate::instantiate as core_instantiate;
use crate::{Module, Registry, Store};
use std::collections::HashMap;

pub struct ComponentInstantiated {
    children: Vec<ComponentInstantiated>,
    core_instances: Vec<CoreInstantiated>,
    export: HashMap<String, InstanceExport>,
}

impl ComponentInstantiated {
    fn new() -> Self {
        Self {
            children: vec![],
            core_instances: vec![],
            export: HashMap::new(),
        }
    }

    fn get_core_instance(&self, idx: usize) -> Option<&CoreInstantiated> {
        self.core_instances.get(idx)
    }
}

pub enum InstanceExport {
    Instance,
}

pub struct CoreInstantiated {
    id: InstanceAddr,
    registry: Registry,
}

pub struct Component {
    children: Vec<Component>,
    pub imports: HashMap<String, ComponentImport>,
    pub exports: HashMap<String, ComponentExport>,

    pub core_modules: Vec<Module>,
    pub core_instances: Vec<CoreInstance>,
    pub core_functions: Vec<CoreFunction>,
    pub core_types: Vec<CoreType>,
}

impl Component {
    pub fn get_core_function(&self, idx: usize) -> &CoreFunction {
        self.core_functions
            .get(idx)
            .expect("Core function not found")
    }
}

pub enum CoreFunction {
    Canon(CanonicalFuncKind),
}

pub enum CoreInstance {
    Real {
        module_idx: usize,
        imports: HashMap<String, CoreInstanceImport>,
    },
    Alias {
        exports: HashMap<String, CoreInstanceAliasExport>,
    },
}

pub enum CoreInstanceAliasExport {
    Func(CoreFuncIdx),
}

impl CoreInstance {
    pub(crate) fn instantiate(
        &self,
        store: &mut Store,
        component: &Component,
        present_instance: &ComponentInstantiated,
    ) -> CoreInstantiated {
        match self {
            CoreInstance::Real {
                module_idx,
                imports,
            } => {
                let module = component
                    .core_modules
                    .get(*module_idx)
                    .expect("Module not found")
                    .clone();
                let mut registry = Registry::new();
                for (name, import) in imports {
                    match import {
                        CoreInstanceImport::Instance(idx) => {
                            let instance = present_instance
                                .get_core_instance(*idx)
                                .expect("Instance not found");
                            registry.register(name, instance.id);
                        }
                    }
                }
                let instance = core_instantiate(module, store, &registry).unwrap();
                CoreInstantiated {
                    id: instance,
                    registry,
                }
            }
            CoreInstance::Alias { exports } => {
                let registry = Registry::new();
                let triplets = exports
                    .iter()
                    .map(|(name, alias)| {
                        let instance = registry.get(name).expect("Instance not found");
                        match alias {
                            CoreInstanceAliasExport::Func(idx) => {
                                let k = idx.get(&component);
                            }
                        }
                        (name.clone(), instance)
                    })
                    .collect::<Vec<_>>();
                // todo: host function implを待って，canonical abiを実装する
                // core_aliasing(&registry, )
                todo!()
            }
        }
    }
}

pub enum CoreInstanceImport {
    Instance(usize),
}

pub enum ComponentImport {
    Instance(usize),
}

pub enum ComponentExport {
    Instance,
}

pub struct Linker {}

impl Linker {}

pub fn instantiate(
    component: Component,
    store: &mut Store,
    linker: Linker,
) -> ComponentInstantiated {
    let component_instance = ComponentInstantiated::new();

    let mut compiled_core_instances = vec![];

    for core_instance in &component.core_instances {
        let compiled = core_instance.instantiate(store, &component, &component_instance);
        compiled_core_instances.push(compiled);
    }

    todo!()
}
