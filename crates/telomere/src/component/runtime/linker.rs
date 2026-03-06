use crate::Module;
use std::collections::HashMap;

pub struct Linker {
    modules: HashMap<String, Module>,
}

impl Default for Linker {
    fn default() -> Self {
        Self::new()
    }
}

impl Linker {
    pub fn new() -> Self {
        Linker {
            modules: Default::default(),
        }
    }

    pub fn register_module<IntoString: Into<String>>(&mut self, name: IntoString, module: Module) {
        self.modules.insert(name.into(), module);
    }
}
