use crate::component_model::id::SortId;
use crate::component_model::types::ExternDesc;

pub struct ComponentImport {
    pub name: String,
    pub ed: ExternDesc,
}

pub struct ComponentExport {
    pub name: String,
    pub si: SortId,
    pub ed: Option<ExternDesc>,
}
