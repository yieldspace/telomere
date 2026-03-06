use crate::component::ir::{CoreInstance, GlobalIdx, Instance};

pub type ImportNameString = String;
pub type ExportNameString = String;

pub enum CoreRelation<T> {
    Defined(T),
    ImportModule(ImportNameString),
    /// Only core module
    FromExport(GlobalIdx<Instance>, ExportNameString),
    FromCoreExport(GlobalIdx<CoreInstance>, ExportNameString),
}

#[derive(Clone, Debug)]
pub enum Relation<T> {
    Defined(T),
    Import(ImportNameString),
    FromExport(GlobalIdx<Instance>, String),
}

impl<T> Relation<T> {
    pub fn new_defined(value: T) -> Self {
        Relation::Defined(value)
    }

    pub fn new_import(placeholder: ImportNameString) -> Self {
        Relation::Import(placeholder)
    }

    pub fn new_from_export(global_idx: GlobalIdx<Instance>, placeholder: ExportNameString) -> Self {
        Relation::FromExport(global_idx, placeholder)
    }
}
