use crate::name::ImportName;
use crate::parser::sort::SortType;
use crate::types::component::ComponentType;
use crate::types::{ComponentTypeId, ResourceDefId, ResourceTableId, TypeId};
use indexmap::IndexMap;

#[derive(Debug)]
pub struct InstanceType {
    pub component_type_id: ComponentTypeId,
    pub resolved_tables: IndexMap<ResourceDefId, ResourceTableId>,
    pub table_map: Vec<ResourceTableId>, // TypeResourceTableIndex → 実表ID
}

pub struct InstanceArg {
    pub ty: TypeId,
    pub sort: SortType,
    pub name: ImportName,
}

pub trait ParentExportLookup {
    /// 親の "type export index" が resource であれば、その ResourceDefId を返す
    fn resource_by_export_type_index(&self, export_type_index: u32) -> Option<ResourceDefId>;

    /// （必要に応じて）親の "instance export の type" を名前で引く
    fn resource_by_instance_export_type(
        &self,
        instance_index: u32,
        name: &str,
    ) -> Option<ResourceDefId> {
        let _ = (instance_index, name);
        None
    }
}

/// 子側の import 名から、子の ResourceDefId を引くためのビュー
pub trait ChildImportLookup {
    /// 子の "type import 名" が resource であれば、その ResourceDefId を返す
    fn child_imported_resource_by_name(
        &self,
        child: &ComponentType,
        import_name: &str,
    ) -> Option<ResourceDefId>;
}
