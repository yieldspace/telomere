use std::collections::HashMap;

use super::InstanceAddr;

pub struct Registry(HashMap<String, InstanceAddr>);
impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}

impl Registry {
    pub fn register(&mut self, name: impl Into<String>, inst: InstanceAddr) {
        self.0.insert(name.into(), inst);
    }
    pub fn new() -> Self {
        Self(HashMap::new())
    }
    pub fn get(&self, name: &str) -> Option<InstanceAddr> {
        self.0.get(name).copied()
    }
}
