use std::collections::HashMap;
use crate::{Module, Registry, Store};
use crate::common::InstanceAddr;
use crate::runtime::instantiate as runtime_instantiate;

pub trait CoreInstantiate {
    fn compile(module: Option<Module>, imports: &HashMap<String, impl CoreInstantiate>) -> Self;
}

pub struct CoreModuleInstantiate {
    addr: InstanceAddr,
    store: Store,
    registry: Registry,
}

pub struct CoreWrappedInstantiate {

}

impl CoreInstantiate for CoreModuleInstantiate {
    fn compile(module: Option<Module>, imports: &HashMap<String, impl CoreInstantiate>) -> Self {
        let module = module.unwrap();
        let mut store = Store::new();
        let mut registry = Registry::new();
        imports.iter().for_each(|(name, import)| {
            registry.register(name, import)
        });
        let addr = runtime_instantiate(module, &mut store, &registry).unwrap();

        Self {
            addr,
            store,
            registry,
        }
    }
}

impl CoreInstantiate for CoreWrappedInstantiate {
    fn compile(module: Option<Module>, imports: HashMap<String, impl CoreInstantiate>) -> Self {
        Self {}
    }
}