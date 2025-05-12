use std::collections::HashMap;
use crate::component_model::{Component, GlobalIdx, PlaceholderId};

#[derive(Debug, Clone)]
pub struct Instance {
    pub component_idx: Option<GlobalIdx<Component>>,
    pub imports: HashMap<PlaceholderId, InstanceImport>,
    pub exports: HashMap<PlaceholderId, InstanceExport>,
}

#[derive(Debug, Clone)]
pub enum InstanceImport {
    // CoreModule,
    // Func,
    Component(GlobalIdx<Component>),
    Instance(GlobalIdx<Instance>),
}

#[derive(Debug, Clone)]
pub struct InstanceExport {

}
