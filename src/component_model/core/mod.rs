mod func;
mod types;
mod sort;

use crate::component_model::{CanonOpt, CanonicalFuncKind, Component, CoreFuncIdx, Idx, TypeIdx};
use crate::runtime::component_model::{ComponentInstantiated, CoreInstantiated, Linker};
use crate::{Registry, Store};
use std::collections::HashMap;
pub use func::*;
pub use types::*;
pub use sort::*;


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
        linker: &Linker,
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
                let instance =
                    crate::runtime::instantiate::instantiate(module, store, &registry).unwrap();
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
                                let k = idx.get(component);
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
