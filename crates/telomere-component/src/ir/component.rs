use crate::ir::{
    CoreModule, ExportNameString, Func, GlobalIdx, ImportNameString, Instance, ResourceId, TypeId,
};
use std::collections::HashMap;

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct Component {
    pub(crate) imports: HashMap<ImportNameString, ComponentImport>,
    pub(crate) exports: HashMap<ExportNameString, ComponentExport>,
}

#[derive(Debug, Clone)]
pub enum ComponentImport {
    Module,
    Component,
    Instance,
    Func(TypeId),
    Resource,
}

#[derive(Debug, Clone)]
pub enum ComponentExport {
    Module(GlobalIdx<CoreModule>),
    Component(GlobalIdx<Component>),
    Instance(GlobalIdx<Instance>),
    Func {
        idx: GlobalIdx<Func>,
        type_id: TypeId,
    },
    Resource(ResourceId),
}
