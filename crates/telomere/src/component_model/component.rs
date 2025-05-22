use crate::component_model::{ExportNameString, ImportNameString};
use std::collections::HashMap;

#[allow(dead_code)]
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
