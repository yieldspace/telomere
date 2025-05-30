use crate::component_model::{
    Component, CoreModule, ExportNameString, Func, GlobalIdx, ImportNameString,
};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub enum Instance {
    Defined {
        component_idx: GlobalIdx<Component>,
        imports: HashMap<ImportNameString, InstanceImport>,
    },
    InlineExport {
        exports: HashMap<ExportNameString, InstanceExport>,
    },
}

#[derive(Debug, Clone)]
pub enum InstanceImport {
    CoreModule(GlobalIdx<CoreModule>),
    Func(GlobalIdx<Func>),
    Component(GlobalIdx<Component>),
    Instance(GlobalIdx<Instance>),
}

#[derive(Debug, Clone)]
pub enum InstanceExport {
    CoreModule(GlobalIdx<CoreModule>),
    Func(GlobalIdx<Func>),
    Component(GlobalIdx<Component>),
    Instance(GlobalIdx<Instance>),
}
