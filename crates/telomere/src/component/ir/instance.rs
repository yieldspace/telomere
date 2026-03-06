use crate::component::ir::{Component, Func, GlobalIdx, ImportNameString};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct Instance {
    pub component_idx: Option<GlobalIdx<Component>>,
    pub imports: HashMap<ImportNameString, InstanceImport>,
}

#[derive(Debug, Clone)]
pub enum InstanceImport {
    // CoreModule,
    Func(GlobalIdx<Func>),
    Component(GlobalIdx<Component>),
    Instance(GlobalIdx<Instance>),
}
