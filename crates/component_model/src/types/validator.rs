use crate::parser::idx::{RawComponentIdx, RawFuncIdx, RawInstanceIdx};
use crate::parser::vec::RawIdx;
use crate::types::component::{ComponentSurface, ComponentType};
use crate::types::resource::ResourcePlan;
use crate::types::{
    AliasResolvable, AliasTarget, ComponentDefId, ComponentTypeId, FuncTypeId, InstanceTypeId,
    Relation, ResourceDefId, TypeId, TypeIdx, TypeResourceTableIndex, TypeStore,
};
use crate::vec::{Idx, IndexVec};
use crate::{ComponentParseError, Result};
use indexmap::{IndexMap, IndexSet};

#[derive(Default, Clone)]
pub struct AliasContext<'a> {
    pub toplevel: Option<&'a TypeValidator>,
    pub outers: Vec<ComponentDefId>,
}

impl<'a> AliasContext<'a> {
    pub fn resolve_component_type(
        &self,
        store: &'a TypeStore,
        value: &'a Relation<ComponentType>,
    ) -> Result<&'a ComponentType> {
        match value {
            Relation::Direct(ty) => Ok(ty),
            Relation::Alias(alias_id) => {
                let alias = store.alias.get(alias_id)?;
                match alias {
                    AliasTarget::Current { index } => self.resolve_component_type(
                        store,
                        store.get_component(&ComponentTypeId::new(*index))?,
                    ),
                    AliasTarget::OuterType { levels, index } => {
                        let mut current = {
                            let id = self
                                .toplevel
                                .unwrap()
                                .store
                                .component_defs
                                .get(&levels[0])
                                .unwrap();
                            self.resolve_component_type(
                                &self.toplevel.unwrap().store,
                                self.toplevel.unwrap().store.get_component(id)?,
                            )
                        }?;
                        for i in 1..levels.len() {
                            let level = &levels[i];
                            let id = current.store.component_defs.get(level).unwrap();
                            current = self.resolve_component_type(
                                &current.store,
                                current.store.get_component(id)?,
                            )?;
                        }
                        self.resolve_component_type(
                            &current.store,
                            current.store.get_component(&ComponentTypeId::new(*index))?,
                        )
                    }
                    AliasTarget::InstanceExportType {
                        instance_type_id,
                        name,
                    } => {
                        // ここではまだインスタンスの型は解決しない
                        unimplemented!()
                    }
                }
            }
        }
    }
}

/// パース時に"resource使用"を集計するヘルパ
#[derive(Default)]
pub struct ResourceUseCollector {
    /// 観測した（表が必要な）ResourceKey
    pub(crate) used: IndexSet<ResourceDefId>,
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
            plan.table_index_of_key
                .insert(key, TypeResourceTableIndex(i as u32));
        }
    }
}

pub struct TypeValidator {
    pub id: ComponentDefId,
    pub usec: ResourceUseCollector,
    pub store: TypeStore,
    pub locals: LocalTypeMap,
    pub surface: ComponentSurface,
}

#[derive(Default, Debug)]
pub struct LocalTypeMap {
    pub(crate) types: IndexVec<TypeIdx, TypeId>,
    pub(crate) components: IndexMap<RawComponentIdx, ComponentTypeId>,
    pub(crate) instances: IndexMap<RawInstanceIdx, InstanceTypeId>,
    pub(crate) funcs: IndexMap<RawFuncIdx, FuncTypeId>,
}

impl TypeValidator {
    pub fn new() -> Self {
        Self {
            id: ComponentDefId::new(),
            usec: ResourceUseCollector::default(),
            store: TypeStore::default(),
            locals: LocalTypeMap::default(),
            surface: ComponentSurface::default(),
        }
    }
}

impl LocalTypeMap {
    pub fn register_type_idx(&mut self, ty: TypeId) -> TypeIdx {
        self.types.push(ty)
    }

    pub fn get_type(&self, idx: &TypeIdx) -> Result<&TypeId> {
        self.types.get(idx)
    }

    pub fn push_component(&mut self, idx: RawComponentIdx, ty: ComponentTypeId) {
        self.components.insert(idx, ty);
    }

    pub fn push_instance(&mut self, idx: RawInstanceIdx, ty: InstanceTypeId) {
        self.instances.insert(idx, ty);
    }

    pub fn push_func(&mut self, idx: RawFuncIdx, ty: FuncTypeId) {
        self.funcs.insert(idx, ty);
    }

    pub fn get_component_type(&self, idx: &RawComponentIdx) -> Result<ComponentTypeId> {
        self.components.get(idx).cloned().ok_or_else(|| {
            ComponentParseError::IndexError(format!("component {:?} not found", idx))
        })
    }

    pub fn get_instance_type(&self, idx: &RawInstanceIdx) -> Result<InstanceTypeId> {
        self.instances
            .get(idx)
            .cloned()
            .ok_or_else(|| ComponentParseError::IndexError(format!("instance {:?} not found", idx)))
    }

    pub fn get_func_type(&self, idx: &RawFuncIdx) -> Result<FuncTypeId> {
        self.funcs
            .get(idx)
            .cloned()
            .ok_or_else(|| ComponentParseError::IndexError(format!("function {:?} not found", idx)))
    }
}
