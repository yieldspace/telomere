use crate::component::decoder::{ParseResult, Validator};
use crate::component::ir::AnyGlobalIdx;
use std::hash::Hash;
use std::sync::atomic::{AtomicU32, Ordering};

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ScopeId(u32, u32);

impl ScopeId {
    pub fn new(depth: u32) -> Self {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        Self(depth, COUNTER.fetch_add(1, Ordering::Relaxed))
    }

    pub fn synthetic() -> Self {
        Self::new(u32::MAX)
    }

    pub fn depth(&self) -> u32 {
        self.0
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ResourceId {
    id: u32,
    owner: ScopeId,
    dtor: Option<AnyGlobalIdx>,
}

impl Default for ResourceId {
    fn default() -> Self {
        Self::new(ScopeId::synthetic())
    }
}

impl ResourceId {
    pub fn new(owner: ScopeId) -> Self {
        Self::with_dtor(owner, None)
    }

    pub fn with_dtor(owner: ScopeId, dtor: Option<AnyGlobalIdx>) -> Self {
        static RESOURCE_HANDLE: AtomicU32 = AtomicU32::new(0);
        Self {
            id: RESOURCE_HANDLE.fetch_add(1, Ordering::Relaxed),
            owner,
            dtor,
        }
    }

    pub fn synthetic() -> Self {
        Self::new(ScopeId::synthetic())
    }

    pub fn owner(self) -> ScopeId {
        self.owner
    }

    pub fn dtor(self) -> Option<AnyGlobalIdx> {
        self.dtor
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord, Default)]
pub struct TypeId(u32);

impl TypeId {
    pub(crate) fn from_index(index: u32) -> Self {
        Self(index)
    }

    pub fn index(self) -> u32 {
        self.0
    }

    pub fn assert_subtype_of(self, parent: TypeId, validator: &Validator) -> ParseResult<()> {
        if self == parent {
            Ok(())
        } else {
            validator.assert_type_ids_subtype_of(self, parent)
        }
    }
}
