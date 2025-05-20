use crate::component_model::sort::Sort;
use crate::component_model::{
    ExportName, ExportNameString, GlobalIdx, ImportName, ImportNameString, Instance, TypeId,
};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone)]
pub struct Component {
    pub(crate) imports: HashMap<ImportNameString, ComponentImport>,
    pub(crate) exports: HashMap<ExportNameString, ComponentExport>,
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
