use crate::component_model::{Component, Func, GlobalIdx, PlaceholderId};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct Instance {
    pub component_idx: Option<GlobalIdx<Component>>,
    pub imports: HashMap<PlaceholderId, InstanceImport>,
    pub exports: HashMap<PlaceholderId, InstanceExport>,
}

#[derive(Debug, Clone)]
pub enum InstanceImport {
    // CoreModule,
    Func(GlobalIdx<Func>),
    Component(GlobalIdx<Component>),
    Instance(GlobalIdx<Instance>),
}

#[derive(Debug, Clone)]
pub struct InstanceExport {}
