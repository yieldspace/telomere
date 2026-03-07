use crate::ir::types::{
    ComponentExportType, ComponentImportType, CoreModuleType, InstanceExportType, Type,
};
use crate::ir::{ScopeId, TypeId};
use std::cell::{Ref, RefCell};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct NameId(u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct SubstEnvId(u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum TypeTransformKind {
    Instantiate,
    FreshenImport,
    SubResource,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ResourceOwnerSummary {
    owners: Vec<ScopeId>,
    contains_abstract_resource: bool,
}

impl ResourceOwnerSummary {
    pub(crate) fn from_owner(owner: ScopeId) -> Self {
        Self {
            owners: vec![owner],
            contains_abstract_resource: false,
        }
    }

    pub(crate) fn refs_foreign_resource(&self, owner: ScopeId) -> bool {
        self.contains_abstract_resource || self.owners.iter().any(|candidate| *candidate != owner)
    }

    pub(crate) fn merge(&mut self, other: &Self) {
        self.contains_abstract_resource |= other.contains_abstract_resource;
        for owner in &other.owners {
            if self.owners.binary_search(owner).is_err() {
                let position = self
                    .owners
                    .binary_search(owner)
                    .unwrap_or_else(|position| position);
                self.owners.insert(position, *owner);
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum ValidationState {
    #[default]
    Unknown,
    InProgress,
    Validated,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct ComponentSurfaceSummary {
    pub(crate) imports: Box<[ComponentImportEntry]>,
    pub(crate) exports: Box<[ComponentExportEntry]>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct InstanceSurfaceSummary {
    pub(crate) exports: Box<[InstanceExportEntry]>,
}

#[derive(Clone, Debug)]
pub(crate) struct ComponentImportEntry {
    pub(crate) name: NameId,
    pub(crate) kind: ComponentImportKind,
}

#[derive(Clone, Debug)]
pub(crate) struct ComponentExportEntry {
    pub(crate) name: NameId,
    pub(crate) kind: ComponentExportType,
}

#[derive(Clone, Debug)]
pub(crate) struct InstanceExportEntry {
    pub(crate) name: NameId,
    pub(crate) kind: InstanceExportType,
}

#[derive(Clone, Debug)]
pub(crate) enum ComponentImportKind {
    Type(ComponentImportType),
    CoreModule(CoreModuleType),
}

#[derive(Clone, Debug, Default)]
pub(crate) struct TypeMeta {
    effective_size: Option<u64>,
    contains_resource_handle: Option<bool>,
    resource_owner_summary: Option<ResourceOwnerSummary>,
    visible_closure: Option<Box<[TypeId]>>,
    surface_validated: ValidationState,
    component_surface_summary: Option<ComponentSurfaceSummary>,
    instance_surface_summary: Option<InstanceSurfaceSummary>,
}

struct TypeSlot {
    ty: Type,
    meta: RefCell<TypeMeta>,
}

#[derive(Default)]
struct NameInterner {
    inline: [Option<(String, NameId)>; 8],
    overflow: Vec<(String, NameId)>,
    next_id: u32,
}

impl NameInterner {
    fn intern(&mut self, value: &str) -> NameId {
        for entry in self.inline.iter().flatten() {
            if entry.0 == value {
                return entry.1;
            }
        }
        for (name, id) in &self.overflow {
            if name == value {
                return *id;
            }
        }
        let id = NameId(self.next_id);
        self.next_id += 1;
        if let Some(slot) = self.inline.iter_mut().find(|slot| slot.is_none()) {
            *slot = Some((value.to_owned(), id));
            return id;
        }
        self.overflow.push((value.to_owned(), id));
        id
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum SubstEnvKey {
    Empty,
    One((TypeId, TypeId)),
    Two([(TypeId, TypeId); 2]),
    Many(Box<[(TypeId, TypeId)]>),
}

#[derive(Default)]
struct SubstEnvInterner {
    ids: HashMap<SubstEnvKey, SubstEnvId>,
    next_id: u32,
}

impl SubstEnvInterner {
    fn intern(&mut self, env: &TransformContext) -> SubstEnvId {
        let key = match &env.map {
            TransformMap::Empty => SubstEnvKey::Empty,
            TransformMap::One(pair) => SubstEnvKey::One(*pair),
            TransformMap::Two(pairs) => SubstEnvKey::Two(*pairs),
            TransformMap::Many(pairs) => SubstEnvKey::Many(pairs.clone().into_boxed_slice()),
        };
        if matches!(key, SubstEnvKey::Empty) {
            return SubstEnvId(0);
        }
        if let Some(id) = self.ids.get(&key).copied() {
            return id;
        }
        let id = SubstEnvId(self.next_id.max(1));
        self.next_id = id.0.saturating_add(1);
        self.ids.insert(key, id);
        id
    }
}

#[derive(Debug, Default)]
enum TransformMap {
    #[default]
    Empty,
    One((TypeId, TypeId)),
    Two([(TypeId, TypeId); 2]),
    Many(Vec<(TypeId, TypeId)>),
}

impl TransformMap {
    fn get(&self, type_id: TypeId) -> Option<TypeId> {
        match self {
            TransformMap::Empty => None,
            TransformMap::One((lhs, rhs)) => (*lhs == type_id).then_some(*rhs),
            TransformMap::Two(pairs) => pairs
                .iter()
                .find_map(|(lhs, rhs)| (*lhs == type_id).then_some(*rhs)),
            TransformMap::Many(pairs) => pairs
                .binary_search_by_key(&type_id, |(lhs, _)| *lhs)
                .ok()
                .map(|index| pairs[index].1),
        }
    }

    fn insert(&mut self, type_id: TypeId, mapped: TypeId) {
        match self {
            TransformMap::Empty => {
                *self = TransformMap::One((type_id, mapped));
            }
            TransformMap::One(pair) => {
                if pair.0 == type_id {
                    pair.1 = mapped;
                    return;
                }
                let mut pairs = [*pair, (type_id, mapped)];
                if pairs[1].0 < pairs[0].0 {
                    pairs.swap(0, 1);
                }
                *self = TransformMap::Two(pairs);
            }
            TransformMap::Two(pairs) => {
                if let Some(existing) = pairs.iter_mut().find(|(lhs, _)| *lhs == type_id) {
                    existing.1 = mapped;
                    return;
                }
                let mut entries = Vec::with_capacity(3);
                entries.extend_from_slice(pairs);
                insert_sorted_pair(&mut entries, (type_id, mapped));
                *self = TransformMap::Many(entries);
            }
            TransformMap::Many(pairs) => {
                insert_sorted_pair(pairs, (type_id, mapped));
            }
        }
    }
}

fn insert_sorted_pair(entries: &mut Vec<(TypeId, TypeId)>, pair: (TypeId, TypeId)) {
    match entries.binary_search_by_key(&pair.0, |(lhs, _)| *lhs) {
        Ok(index) => entries[index].1 = pair.1,
        Err(index) => entries.insert(index, pair),
    }
}

#[derive(Debug, Default)]
pub(crate) struct TransformContext {
    generation: u32,
    map: TransformMap,
}

impl TransformContext {
    pub(crate) fn new(generation: u32) -> Self {
        Self {
            generation,
            map: TransformMap::Empty,
        }
    }

    pub(crate) fn generation(&self) -> u32 {
        self.generation
    }

    pub(crate) fn get(&self, type_id: TypeId) -> Option<TypeId> {
        self.map.get(type_id)
    }

    pub(crate) fn insert(&mut self, type_id: TypeId, mapped: TypeId) {
        self.map.insert(type_id, mapped);
    }
}

pub(crate) struct TypeArena {
    slots: Vec<TypeSlot>,
    names: RefCell<NameInterner>,
    subst_envs: RefCell<SubstEnvInterner>,
    transform_cache: RefCell<HashMap<(TypeId, TypeTransformKind, u32, SubstEnvId), TypeId>>,
}

impl Default for TypeArena {
    fn default() -> Self {
        Self {
            slots: Vec::new(),
            names: RefCell::new(NameInterner::default()),
            subst_envs: RefCell::new(SubstEnvInterner {
                ids: HashMap::new(),
                next_id: 1,
            }),
            transform_cache: RefCell::new(HashMap::new()),
        }
    }
}

impl TypeArena {
    pub(crate) fn add(&mut self, ty: Type) -> TypeId {
        let id = TypeId::from_index(self.slots.len() as u32);
        self.slots.push(TypeSlot {
            ty,
            meta: RefCell::new(TypeMeta::default()),
        });
        id
    }

    pub(crate) fn get(&self, id: TypeId) -> Option<&Type> {
        self.slots.get(id.index() as usize).map(|slot| &slot.ty)
    }

    pub(crate) fn len(&self) -> usize {
        self.slots.len()
    }

    pub(crate) fn snapshot_types(&self) -> Box<[Type]> {
        self.slots
            .iter()
            .map(|slot| slot.ty.clone())
            .collect::<Vec<_>>()
            .into_boxed_slice()
    }

    pub(crate) fn validation_state(&self, id: TypeId) -> Option<ValidationState> {
        self.slots
            .get(id.index() as usize)
            .map(|slot| slot.meta.borrow().surface_validated)
    }

    pub(crate) fn set_validation_state(&self, id: TypeId, state: ValidationState) {
        if let Some(slot) = self.slots.get(id.index() as usize) {
            slot.meta.borrow_mut().surface_validated = state;
        }
    }

    pub(crate) fn component_surface_summary(
        &self,
        id: TypeId,
    ) -> Option<Ref<'_, ComponentSurfaceSummary>> {
        let slot = self.slots.get(id.index() as usize)?;
        Ref::filter_map(slot.meta.borrow(), |meta| {
            meta.component_surface_summary.as_ref()
        })
        .ok()
    }

    pub(crate) fn set_component_surface_summary(
        &self,
        id: TypeId,
        summary: ComponentSurfaceSummary,
    ) {
        if let Some(slot) = self.slots.get(id.index() as usize) {
            slot.meta.borrow_mut().component_surface_summary = Some(summary);
        }
    }

    pub(crate) fn instance_surface_summary(
        &self,
        id: TypeId,
    ) -> Option<Ref<'_, InstanceSurfaceSummary>> {
        let slot = self.slots.get(id.index() as usize)?;
        Ref::filter_map(slot.meta.borrow(), |meta| {
            meta.instance_surface_summary.as_ref()
        })
        .ok()
    }

    pub(crate) fn set_instance_surface_summary(&self, id: TypeId, summary: InstanceSurfaceSummary) {
        if let Some(slot) = self.slots.get(id.index() as usize) {
            slot.meta.borrow_mut().instance_surface_summary = Some(summary);
        }
    }

    pub(crate) fn visible_closure(&self, id: TypeId) -> Option<Ref<'_, [TypeId]>> {
        let slot = self.slots.get(id.index() as usize)?;
        Ref::filter_map(slot.meta.borrow(), |meta| meta.visible_closure.as_deref()).ok()
    }

    pub(crate) fn set_visible_closure(&self, id: TypeId, closure: Vec<TypeId>) {
        if let Some(slot) = self.slots.get(id.index() as usize) {
            slot.meta.borrow_mut().visible_closure = Some(closure.into_boxed_slice());
        }
    }

    pub(crate) fn contains_resource_handle(&self, id: TypeId) -> Option<bool> {
        self.slots
            .get(id.index() as usize)
            .and_then(|slot| slot.meta.borrow().contains_resource_handle)
    }

    pub(crate) fn set_contains_resource_handle(&self, id: TypeId, found: bool) {
        if let Some(slot) = self.slots.get(id.index() as usize) {
            slot.meta.borrow_mut().contains_resource_handle = Some(found);
        }
    }

    pub(crate) fn resource_owner_summary(&self, id: TypeId) -> Option<ResourceOwnerSummary> {
        self.slots
            .get(id.index() as usize)
            .and_then(|slot| slot.meta.borrow().resource_owner_summary.as_ref().cloned())
    }

    pub(crate) fn set_resource_owner_summary(&self, id: TypeId, summary: ResourceOwnerSummary) {
        if let Some(slot) = self.slots.get(id.index() as usize) {
            slot.meta.borrow_mut().resource_owner_summary = Some(summary);
        }
    }

    pub(crate) fn effective_size(&self, id: TypeId) -> Option<u64> {
        self.slots
            .get(id.index() as usize)
            .and_then(|slot| slot.meta.borrow().effective_size)
    }

    pub(crate) fn set_effective_size(&self, id: TypeId, size: u64) {
        if let Some(slot) = self.slots.get(id.index() as usize) {
            slot.meta.borrow_mut().effective_size = Some(size);
        }
    }

    pub(crate) fn intern_name(&self, value: &str) -> NameId {
        self.names.borrow_mut().intern(value)
    }

    pub(crate) fn subst_env_id(&self, env: &TransformContext) -> SubstEnvId {
        self.subst_envs.borrow_mut().intern(env)
    }

    pub(crate) fn lookup_transform(
        &self,
        source: TypeId,
        kind: TypeTransformKind,
        generation: u32,
        env: SubstEnvId,
    ) -> Option<TypeId> {
        self.transform_cache
            .borrow()
            .get(&(source, kind, generation, env))
            .copied()
    }

    pub(crate) fn record_transform(
        &self,
        source: TypeId,
        kind: TypeTransformKind,
        generation: u32,
        env: SubstEnvId,
        target: TypeId,
    ) {
        self.transform_cache
            .borrow_mut()
            .insert((source, kind, generation, env), target);
    }

    #[cfg(test)]
    pub(crate) fn transform_cache_len(&self) -> usize {
        self.transform_cache.borrow().len()
    }

    #[cfg(test)]
    pub(crate) fn interned_name_count(&self) -> usize {
        let names = self.names.borrow();
        names.inline.iter().flatten().count() + names.overflow.len()
    }
}
