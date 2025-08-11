use indexmap::{IndexMap, IndexSet};
use crate::parser::idx::RawTypeIdx;
use crate::parser::vec::RawIdx;
use crate::types::resource::{ResourcePlan, ResourceTypes};
use crate::types::{ResourceDefId, TypeId, TypeInterner, TypeResourceTableIndex};
use crate::vec::IndexVec;

/// パース時に"resource使用"を集計するヘルパ
#[derive(Default)]
struct ResourceUseCollector {
    /// 観測した（表が必要な）ResourceKey
    used: IndexSet<ResourceDefId>,
}
impl ResourceUseCollector {
    pub(crate) fn note_own(&mut self, res_id: ResourceDefId) {
        self.used.insert(res_id);
    }
    pub(crate) fn note_borrow(&mut self, res_id: ResourceDefId) {
        // borrow も表参照は同じ。必要なら区別しても良い。
        self.used.insert(res_id);
    }
    pub(crate) fn note_canon_resource(&mut self, res_id: ResourceDefId) {
        self.used.insert(res_id);
    }
    pub(crate) fn finalize_plan(self, plan: &mut ResourcePlan) {
        for (i, key) in self.used.into_iter().enumerate() {
            plan.table_index_of_key.insert(key, TypeResourceTableIndex(i as u32));
        }
    }
}

pub struct TypeValidator<'a> {
    pub types: ResourceTypes,
    pub plan: ResourcePlan,
    pub usec: ResourceUseCollector,
    pub interner: &'a mut TypeInterner,
    type_vec: Vec<TypeId>,
}

impl<'a> TypeValidator<'a> {
    pub fn new(interner: &'a mut TypeInterner) -> Self {
        Self {
            types: ResourceTypes::default(),
            plan: ResourcePlan::default(),
            usec: ResourceUseCollector::default(),
            interner,
            type_vec: Vec::new(),
        }
    }

    pub fn new_raw_type_idx(&mut self) -> RawTypeIdx {
        let idx = RawTypeIdx::new(self.type_vec.len() as u32);
        idx
    }

    pub fn insert_type(&mut self, ty: TypeId) -> RawTypeIdx {
        let idx = self.new_raw_type_idx();
        self.type_vec.push(ty);
        idx
    }
}
