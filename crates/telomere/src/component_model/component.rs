use crate::component_model::sort::Sort;
use crate::component_model::{GlobalIdx, ImportName, Instance, PlaceholderId};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone)]
pub struct Component {
    pub(crate) imports: HashMap<PlaceholderId, ComponentImport>,
    pub(crate) exports: HashMap<PlaceholderId, ComponentExport>,
}

#[derive(Debug, Clone)]
pub enum ComponentImport {
    Component(GlobalIdx<Component>),
    Instance(GlobalIdx<Instance>),
}

#[derive(Debug, Clone)]
pub enum ComponentExport {
    Component(GlobalIdx<Component>),
    Instance(GlobalIdx<Instance>),
}
