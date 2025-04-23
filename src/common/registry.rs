use std::collections::HashMap;

use super::InstanceHandle;

#[derive(Debug)]
pub struct Registry(HashMap<String, InstanceHandle>);
impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}

impl Registry {
    pub fn register(&mut self, name: impl Into<String>, inst: InstanceHandle) {
        self.0.insert(name.into(), inst);
    }
    pub fn new() -> Self {
        Self(HashMap::new())
    }
    pub fn get(&self, name: &str) -> Option<InstanceHandle> {
        self.0.get(name).cloned()
    }
}
