use indexmap::IndexMap;
use crate::name::ImportName;
use crate::parser::component::RawComponent;
use crate::parser::idx::RawComponentIdx;
use crate::types::resource::ResourceTableId;
use crate::types::ResourceDefId;

#[derive(Debug, Clone)]
pub struct InstanceType {
    pub component_index: RawComponentIdx,
    pub args: Vec<InstanceArg>, // kind/index は後述
    /// このインスタンスでの「ResourceKey -> 実テーブルID」を後で埋める
    pub resolved_tables: IndexMap<ResourceDefId, ResourceTableId>,
    pub table_map: Vec<ResourceTableId>,
}

#[derive(Debug, Clone)]
pub struct InstanceArg {
    pub name: ImportName,
    pub kind: wasmparser::ComponentExternalKind,
    pub index: u32,
}

pub trait ParentExportLookup {
    /// 親の "type export index" が resource であれば、その ResourceDefId を返す
    fn resource_by_export_type_index(&self, export_type_index: u32) -> Option<ResourceDefId>;

    /// （必要に応じて）親の "instance export の type" を名前で引く
    fn resource_by_instance_export_type(&self, instance_index: u32, name: &str) -> Option<ResourceDefId> {
        let _ = (instance_index, name);
        None
    }
}

/// 子側の import 名から、子の ResourceDefId を引くためのビュー
pub trait ChildImportLookup {
    /// 子の "type import 名" が resource であれば、その ResourceDefId を返す
    fn child_imported_resource_by_name(&self, child: &RawComponent, import_name: &str) -> Option<ResourceDefId>;
}
