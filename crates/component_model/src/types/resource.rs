use std::sync::atomic::AtomicU32;
use indexmap::IndexMap;
use crate::parser::idx::{RawFuncIdx, RawTypeIdx};
use crate::types::{ResourceDefId, TypeId, TypeResourceTableIndex};
use crate::vec::{Idx, IndexVec};


/// 実インスタンス側の表ID（実体: ランタイムのテーブル配列index）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ResourceTableId(pub u32);


#[derive(Default)]
pub struct ResourceTypes {
    /// componentのtype index -> ResourceDefId（resourceのみ）
    pub resource_by_type_idx: IndexMap<RawTypeIdx, ResourceDefId>,
    /// ResourceDefId -> 代表情報
    pub defs: IndexMap<ResourceDefId, ResourceDef>,
}

pub struct ResourceDef {
    // pub rep: todo: now i32
    pub dtor: Option<RawFuncIdx>,
}

impl ResourceTypes {
    pub fn push_from_type_section(
        &mut self,
        type_index: RawTypeIdx,
        dtor: Option<RawFuncIdx>,
    ) -> ResourceDefId {
        let id = ResourceDefId::new();
        self.resource_by_type_idx.insert(type_index, id);
        self.defs.insert(id, ResourceDef { dtor });
        id
    }

    pub fn get_by_type_index(&self, ty: RawTypeIdx) -> Option<ResourceDefId> {
        self.resource_by_type_idx.get(&ty).copied()
    }
}

#[derive(Default)]
pub struct ResourcePlan {
    pub table_index_of_key: IndexMap<ResourceDefId, TypeResourceTableIndex>,
}

