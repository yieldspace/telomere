use std::collections::HashMap;

use super::{Instance, Module};

pub struct Registry(HashMap<String, (Module, Instance)>);
impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}

impl Registry {
    pub fn register(&mut self, name: impl Into<String>, m: Module, inst: Instance) {
        self.0.insert(name.into(), (m, inst));
    }
    pub fn new() -> Self {
        Self(HashMap::new())
    }
    pub fn get(&self, name: &str) -> Option<&(Module, Instance)> {
        self.0.get(name)
    }
}
