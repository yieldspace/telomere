use crate::runtime::component_model::instantiate::ComponentInstance;
use crate::Module;
use std::collections::HashMap;

pub struct Linker<'a> {
    modules: HashMap<String, &'a Module>,
    instances: HashMap<String, &'a ComponentInstance>,
}

impl<'a> Default for Linker<'a> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> Linker<'a> {
    pub fn new() -> Self {
        Linker {
            modules: Default::default(),
            instances: Default::default(),
        }
    }

    pub fn register_module<IntoString: Into<String>>(
        &mut self,
        name: IntoString,
        module: &'a Module,
    ) {
        self.modules.insert(name.into(), module);
    }

    pub fn register_instance<IntoString: Into<String>>(
        &mut self,
        name: IntoString,
        instance: &'a ComponentInstance,
    ) {
        self.instances.insert(name.into(), instance);
    }
}
