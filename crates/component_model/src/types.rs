mod externdesc;
mod primitive;
mod validator;
mod resource;
mod instance;

pub use primitive::PrimValType;
pub use instance::*;
use std::sync::atomic::{AtomicU32, Ordering};

use std::sync::Arc;
use fxhash::FxHashMap;
use smallvec::SmallVec;
pub use externdesc::*;
pub use validator::*;
use crate::name::ImportName;
use crate::vec::{IndexVec, Idx};

macro_rules! index {
    ($name:ident) => {
        #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
        pub struct $name(u32);

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

index!(TypeId);
index!(ComponentTypeId);
index!(InstanceTypeId);
index!(TypeSchemaId);
index!(ResourceDefId);
index!(ResourceInstId);
index!(TypeParamId);
index!(ComponentDefId);
index!(ComponentInstanceId);
index!(TableClassId);
index!(TypeResourceTableIndex);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TypeIndex {
    Mono(TypeId),
    Component(ComponentTypeId),
    Instance(InstanceTypeId),
}


pub enum PrimitiveType {
    Bool, S8, U8, S16, U16, S32, U32, S64, U64,
    F32, F64, Char,
    String,
    #[cfg(feature = "async")]
    ErrorContext,
}

pub enum MonoType {
    Prim(PrimitiveType),
    List(TypeId), Option(TypeId),
    Result { ok: Option<TypeId>, err: Option<TypeId> },
    Tuple(SmallVec<[TypeId; 4]>),
    Record(SmallVec<[(String, TypeId); 4]>),
    Variant(SmallVec<[(String, Option<TypeId>); 4]>),

    Resource(ResourceDefId),       // WIT の resource 定義
    HandleOwn(ResourceDefId),
    HandleBorrow(ResourceDefId),
}

pub struct TypeInterner { map: FxHashMap<MonoType, TypeId>, arena: Vec<MonoType> }
impl TypeInterner {
    pub fn new() -> Self {
        Self { map: FxHashMap::default(), arena: Vec::new() }
    }

    pub fn intern(&mut self, t: MonoType) -> TypeId {
        todo!()
    }
}

pub struct ImportResolver { /* package -> (types/resources) -> DefId */ }
impl ImportResolver {
    pub fn resolve_resource(&self, path: &ImportName) -> ResourceDefId {
        todo!()
    }
    pub fn resolve_type(&self, path: &ImportName) -> TypeId {
        todo!()
    } // record等
}

pub struct ComponentInstance {
    resource_tables: IndexVec<TypeResourceTableIndex, ResourceTable>,
    // dtorや「このtyの所有はどのruntime instanceか」等のメタへアクセスできるhandles
}

impl ComponentInstance {
    pub fn instantiate() -> Self {
        let num = 0;
        let mut tables = IndexVec::with_capacity(num);
        for _ in 0..num { tables.push(ResourceTable::default()); } // 0..N-1 を確保
        Self { resource_tables: tables }
    }
}

pub struct ResourceEntry { rep: u32, borrows: u32 /* + 世代番号など */ }
#[derive(Default)]
pub struct ResourceTable { entries: slab::Slab<ResourceEntry> /* + 親子追跡は任意 */ }

impl ResourceTable {
    pub fn new_own(&mut self, rep: u32) -> u32 {
        todo!()
    }
    pub fn rep_of (&self, idx: u32) -> Result<u32, ()> {
        todo!()
    }
    pub fn drop_own(&mut self, idx: u32) -> Result<(), ()> {
        todo!()
    }
}