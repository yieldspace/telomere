use crate::component_model::id::SortId;
use crate::component_model::types::ExternDesc;

#[derive(Debug)]
pub struct ComponentImport {
    pub name: String,
    pub ed: ExternDesc,
}

#[derive(Debug)]
pub struct ComponentExport {
    pub name: String,
    pub si: SortId,
    pub ed: Option<ExternDesc>,
}
