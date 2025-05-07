use crate::component_model::{GlobalIdx, PlaceholderId};

#[derive(Clone, Debug)]
pub enum Relation<T> {
    Defined(T),
    Import(PlaceholderId),
    FromExport(GlobalIdx<T>, PlaceholderId),
}

impl<T> Relation<T> {
    pub fn new_defined(value: T) -> Self {
        Relation::Defined(value)
    }

    pub fn new_import(placeholder: PlaceholderId) -> Self {
        Relation::Import(placeholder)
    }

    pub fn new_from_export(global_idx: GlobalIdx<T>, placeholder: PlaceholderId) -> Self {
        Relation::FromExport(global_idx, placeholder)
    }
}
