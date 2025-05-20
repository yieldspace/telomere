use crate::component_model::{Component, ExportNameString, Func, GlobalIdx, ImportNameString};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct Instance {
    pub component_idx: Option<GlobalIdx<Component>>,
    pub imports: HashMap<ImportNameString, InstanceImport>,
    pub exports: HashMap<ExportNameString, InstanceExport>,
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
