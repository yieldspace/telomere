use crate::component_model::{CoreInstance, GlobalIdx, Instance, PlaceholderId};

pub enum CoreRelation<T> {
    Defined(T),
    ImportModule(PlaceholderId),
    /// Only core module
    FromExport(GlobalIdx<Instance>, PlaceholderId),
    FromCoreExport(GlobalIdx<CoreInstance>, PlaceholderId),
}

#[derive(Clone, Debug)]
pub enum Relation<T> {
    Defined(T),
    Import(PlaceholderId),
    FromExport(GlobalIdx<Instance>, PlaceholderId),
}

impl<T> Relation<T> {
    pub fn new_defined(value: T) -> Self {
        Relation::Defined(value)
    }

    pub fn new_import(placeholder: PlaceholderId) -> Self {
        Relation::Import(placeholder)
    }

    pub fn new_from_export(global_idx: GlobalIdx<Instance>, placeholder: PlaceholderId) -> Self {
        Relation::FromExport(global_idx, placeholder)
    }
}
