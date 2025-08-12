use crate::name::{ExportName, ImportName};
use crate::parser::idx::RawFuncIdx;
use crate::types::{ResourceDefId, TypeId, TypeResourceTableIndex};
use indexmap::IndexMap;

#[derive(Default, Debug)]
pub struct ResourcePlan {
    pub table_index_of_key: IndexMap<ResourceDefId, TypeResourceTableIndex>,
}

#[derive(Debug)]
pub enum ResourceDef {
    Defined { dtor: Option<RawFuncIdx> },
    ImportSubResource { import_name: ImportName },
    ExportSubResource { export_name: ExportName },
}
