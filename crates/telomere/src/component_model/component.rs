use crate::component_model::{ExportNameString, ImportNameString};
use crate::runtime::component_model::instantiate::InstantiateOp;
use std::collections::HashMap;

#[allow(dead_code)]
#[derive(Clone)]
pub struct Component {
    pub(crate) imports: HashMap<ImportNameString, ComponentImport>,
    pub(crate) exports: HashMap<ExportNameString, ComponentExport>,
    pub(crate) ops: Vec<InstantiateOp>,
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
    CoreModule,
}
