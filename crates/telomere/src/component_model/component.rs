use crate::component_model::sort::Sort;
use crate::component_model::{ExportName, GlobalIdx, ImportName, Instance, PlaceholderId, TypeId};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone)]
pub struct Component {
    pub(crate) imports: HashMap<PlaceholderId, ComponentImport>,
    pub(crate) exports: HashMap<PlaceholderId, ComponentExport>,
}

#[derive(Debug, Clone)]
pub enum ComponentImport {
    Component,
    Instance,
    Func,
    Resource,
}

#[derive(Debug, Clone)]
pub enum ComponentExport {
    Component,
    Instance,
    Func,
    Type,
    Resource,
}
