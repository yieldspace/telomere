use std::collections::HashMap;
use telomere_wasm::common::InstanceHandle;
use telomere_wasm::{instantiate as core_instantiate, Module, Registry, Store};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CoreModuleIndex(pub usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CoreInstanceIndex(pub usize);

pub struct CoreInstance {
    pub module_index: CoreModuleIndex,
}

pub struct Component {
    pub core_modules: HashMap<CoreModuleIndex, Module>,
    pub core_instances: HashMap<CoreInstanceIndex, CoreInstance>,
    pub dependencies: Vec<Dependency>,
}

pub struct Instance {
    pub exports: HashMap<String, InstanceExport>,
}

pub struct InstanceExport {
    pub instances: HashMap<CoreInstanceIndex, InstanceHandle>,
}

impl Component {
    pub async fn instantiate(self, store: &mut Store) {
        let mut instantiated = HashMap::new();
        let Self {
            core_modules,
            dependencies,
            core_instances,
        } = self;
        for dependency in dependencies {
            match dependency {
                Dependency::CoreInstantiate(idx) => {
                    let core_instance = core_instances.get(&idx).expect("Core instance not found");
                    let module = core_modules
                        .get(&core_instance.module_index)
                        .expect("Core module not found");

                    let registry = Registry::new();
                    let instance = core_instantiate(module.clone(), store, &registry)
                        .await
                        .unwrap();
                    println!("Instantiated core instance: {:?}", instance);
                    instantiated.insert(idx, instance);
                }
                Dependency::LowerAdaptor(_) => {}
            }
        }
    }
}

pub enum Dependency {
    CoreInstantiate(CoreInstanceIndex),
    LowerAdaptor(LowerAdaptor),
}

/// Component Function -> Core Function
pub struct LowerAdaptor {
    pub core_instance_index: CoreInstanceIndex,
}
