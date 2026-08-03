use std::collections::HashMap;

use super::InstanceHandle;

#[derive(Debug)]
/// Maps module names in imports to instances that satisfy those imports.
///
/// Populate a registry before calling [`crate::instantiate`]. The registry owns
/// a clone of each handle, so the associated [`crate::Store`] must outlive the
/// instantiation and subsequent calls that use it.
pub struct Registry(HashMap<String, InstanceHandle>);
impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}

impl Registry {
    /// Makes an instance available to imports that name `name`.
    ///
    /// Re-registering the same name replaces the previous instance.
    pub fn register(&mut self, name: impl Into<String>, inst: InstanceHandle) {
        self.0.insert(name.into(), inst);
    }

    /// Creates an empty import registry.
    pub fn new() -> Self {
        Self(HashMap::new())
    }

    /// Returns the instance registered for an import module name, if any.
    pub fn get(&self, name: &str) -> Option<InstanceHandle> {
        self.0.get(name).cloned()
    }
}
