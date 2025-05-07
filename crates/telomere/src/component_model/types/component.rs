use crate::component_model::types::TypeId;
use crate::component_model::PlaceholderId;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentType {
    pub imports: HashMap<PlaceholderId, ComponentImportType>,
    pub exports: HashMap<PlaceholderId, ComponentExportType>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComponentExportType {
    Component(TypeId),
    Instance(TypeId),
    Type(TypeId),
    Sub(TypeId),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComponentImportType {
    Component(TypeId),
    Instance(TypeId),
    Type(TypeId),
    Sub(TypeId),
}
