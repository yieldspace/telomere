mod check;
pub mod component;
pub mod func;
pub mod instance;
pub mod primitive;
pub mod resource;
pub mod val;
pub mod validator;

use crate::{ComponentParseError, Result};
pub use primitive::PrimValType;
use std::collections::HashMap;
use std::hash::Hash;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::name::{ExportName, ImportName};
use crate::parser::vec::RawIdx;
use crate::types::component::ComponentType;
use crate::types::func::FuncType;
use crate::types::instance::InstanceType;
use crate::types::resource::ResourceDef;
use crate::types::val::ValType;
use crate::vec::{Idx, IndexVec};
use fxhash::FxHashMap;
use indexmap::IndexMap;
use smallvec::SmallVec;
use std::sync::Arc;
pub use validator::*;

macro_rules! index {
    ($name:ident) => {
        #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
        pub struct $name(pub u32);

        impl Idx for $name {
            fn new(value: u32) -> Self {
                Self(value)
            }

            fn index(&self) -> usize {
                self.0 as usize
            }
        }

        impl From<u32> for $name {
            fn from(value: u32) -> Self {
                Self(value)
            }
        }
    };
}

index!(ValTypeId);
impl From<ValTypeId> for TypeId {
    fn from(value: ValTypeId) -> Self {
        Self::Val(value)
    }
}
index!(ComponentTypeId);
impl From<ComponentTypeId> for TypeId {
    fn from(value: ComponentTypeId) -> Self {
        Self::Component(value)
    }
}

index!(InstanceTypeId);
impl From<InstanceTypeId> for TypeId {
    fn from(value: InstanceTypeId) -> Self {
        Self::Instance(value)
    }
}
index!(FuncTypeId);
impl From<FuncTypeId> for TypeId {
    fn from(value: FuncTypeId) -> Self {
        Self::Func(value)
    }
}
index!(AliasTypeId);
impl From<AliasTypeId> for TypeId {
    fn from(value: AliasTypeId) -> Self {
        Self::Alias(value)
    }
}
index!(ResourceDefId);
impl From<ResourceDefId> for TypeId {
    fn from(value: ResourceDefId) -> Self {
        Self::Resource(value)
    }
}
index!(TypeIdx);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TypeId {
    Val(ValTypeId),
    Func(FuncTypeId),
    Resource(ResourceDefId),
    Component(ComponentTypeId),
    Instance(InstanceTypeId),
    Alias(AliasTypeId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct TypeResourceTableIndex(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ResourceTableId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ComponentDefId(pub u32);

impl ComponentDefId {
    pub fn new() -> Self {
        static NEXT_ID: AtomicU32 = AtomicU32::new(0);
        Self(NEXT_ID.fetch_add(1, Ordering::Relaxed))
    }
}

#[derive(Debug, Clone)]
pub enum ExportTyRef {
    Func(TypeId),
    Instance(TypeId),
    Component(TypeId),
    TypeEq(TypeId),
    TypeSubResource(ResourceDefId),
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub enum Relation<T> {
    Direct(T),
    Alias(AliasTypeId),
}

pub trait AliasResolvable<T> {
    fn resolve(&self, current_validator: &TypeValidator, context: &AliasContext<'_>) -> Result<&T>;
}

#[derive(Default, Debug)]
pub struct TypeStore {
    pub(crate) component_defs: IndexMap<ComponentDefId, ComponentTypeId>,

    pub(crate) val_types: Interner<ValTypeId, Relation<ValType>>,
    pub(crate) alias: IndexVec<AliasTypeId, AliasTarget>,
    pub(crate) funcs: IndexVec<FuncTypeId, Relation<FuncType>>,
    pub(crate) components: IndexVec<ComponentTypeId, Relation<ComponentType>>,
    pub(crate) instances: IndexVec<InstanceTypeId, Relation<InstanceType>>,
    pub(crate) resources: IndexVec<ResourceDefId, ResourceDef>,
}

#[derive(Debug, Clone, Eq, Hash, PartialEq)]
pub enum AliasTarget {
    /// alias outerのうち，ctが0のもの
    Current { index: u32 },
    OuterType {
        levels: Box<[ComponentDefId]>,
        index: u32,
    },
    InstanceExportType {
        instance_type_id: InstanceTypeId,
        name: ExportName,
    },
}

#[derive(Debug)]
pub struct Interner<K: Idx, V> {
    map: HashMap<V, K>,
    keys: IndexVec<K, V>,
}
impl<K: Idx, V: Hash + Eq + Clone> Interner<K, V> {
    pub fn intern(&mut self, value: V) -> K {
        *self
            .map
            .entry(value.clone())
            .or_insert_with(|| self.keys.push(value))
    }

    pub fn get(&self, key: &K) -> Result<&V> {
        self.keys.get(key)
    }

    pub fn iter(&self) -> impl Iterator<Item = &V> {
        self.keys.raw.iter()
    }
}

impl<K: Idx, V> Default for Interner<K, V> {
    fn default() -> Self {
        Self {
            map: HashMap::new(),
            keys: IndexVec::new(),
        }
    }
}

impl TypeId {
    pub fn ensure_val_type(&self) -> Result<ValTypeId> {
        match self {
            TypeId::Val(id) => Ok(*id),
            _ => Err(ComponentParseError::TypeError("Expected ValType".into())),
        }
    }

    pub fn ensure_func_type(&self) -> Result<FuncTypeId> {
        match self {
            TypeId::Func(id) => Ok(*id),
            _ => Err(ComponentParseError::TypeError("Expected FuncType".into())),
        }
    }

    pub fn ensure_resource(&self) -> Result<ResourceDefId> {
        match self {
            TypeId::Resource(id) => Ok(*id),
            _ => Err(ComponentParseError::TypeError(
                "Expected ResourceDef".into(),
            )),
        }
    }

    pub fn ensure_component_type(&self) -> Result<ComponentTypeId> {
        match self {
            TypeId::Component(id) => Ok(*id),
            _ => Err(ComponentParseError::TypeError(
                "Expected ComponentType".into(),
            )),
        }
    }

    pub fn ensure_instance_type(&self) -> Result<InstanceTypeId> {
        match self {
            TypeId::Instance(id) => Ok(*id),
            _ => Err(ComponentParseError::TypeError(
                "Expected InstanceType".into(),
            )),
        }
    }

    pub fn ensure_alias_type(&self) -> Result<AliasTypeId> {
        match self {
            TypeId::Alias(id) => Ok(*id),
            _ => Err(ComponentParseError::TypeError("Expected AliasType".into())),
        }
    }
}

impl TypeStore {
    pub fn push_val_type_in_type(&mut self, val_type: Relation<ValType>) -> ValTypeId {
        self.val_types.intern(val_type)
    }

    pub fn push_alias_in_type(&mut self, alias: AliasTarget) -> AliasTypeId {
        self.alias.push(alias)
    }

    pub fn push_func_in_type(&mut self, func: Relation<FuncType>) -> FuncTypeId {
        self.funcs.push(func)
    }

    pub fn push_component_in_type(
        &mut self,
        component: Relation<ComponentType>,
    ) -> ComponentTypeId {
        self.components.push(component)
    }

    pub fn push_instance_in_type(&mut self, instance: Relation<InstanceType>) -> InstanceTypeId {
        self.instances.push(instance)
    }

    pub fn push_resource_in_type(&mut self, resource: ResourceDef) -> ResourceDefId {
        self.resources.push(resource)
    }

    pub fn get_val_type(&self, idx: &ValTypeId) -> Result<&Relation<ValType>> {
        self.val_types.get(idx)
    }

    pub fn get_alias(&self, idx: &AliasTypeId) -> Result<&AliasTarget> {
        self.alias.get(idx)
    }

    pub fn get_func(&self, idx: &FuncTypeId) -> Result<&Relation<FuncType>> {
        self.funcs.get(idx)
    }

    pub fn get_component(&self, idx: &ComponentTypeId) -> Result<&Relation<ComponentType>> {
        self.components.get(idx)
    }

    pub fn get_instance(&self, idx: &InstanceTypeId) -> Result<&Relation<InstanceType>> {
        self.instances.get(idx)
    }

    pub fn get_resource(&self, idx: &ResourceDefId) -> Result<&ResourceDef> {
        self.resources.get(idx)
    }
}
