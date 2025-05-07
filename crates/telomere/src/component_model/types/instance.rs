use crate::component_model::types::TypeId;
use crate::component_model::{ExportName, PlaceholderId};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstanceType {
    exports: HashMap<PlaceholderId, InstanceExportType>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstanceExportType {
    Component(TypeId),
    Instance(TypeId),
    Type(TypeId),
    Sub(TypeId),
}

impl InstanceType {
    pub fn new() -> Self {
        Self {
            exports: HashMap::new(),
        }
    }

    pub fn get_export(&self, name: &ExportName) -> Option<(&PlaceholderId, &InstanceExportType)> {
        let hash = {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            name.hash(&mut hasher);
            hasher.finish()
        };
        self.exports
            .iter()
            .find(|(pid, data)| pid.name_hash() == hash)
    }
}
