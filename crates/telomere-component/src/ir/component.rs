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
    // `engine.rs` identifies callable imports by this variant but does not
    // currently need its type ID.
    #[allow(dead_code)]
    Func(TypeId),
    // Retained conservatively; the current decoder does not construct resource
    // imports.
    #[allow(dead_code)]
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
    // `runtime/env.rs` still matches resource exports during lookup.
    #[allow(dead_code)]
    Resource(ResourceId),
}
