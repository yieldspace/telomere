use crate::component::decoder::{ParseResult, Validator};
use crate::component::ir::AnyGlobalIdx;
use std::hash::Hash;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
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

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct TypeId(usize);

impl Default for TypeId {
    fn default() -> Self {
        Self::new()
    }
}

impl TypeId {
    pub fn new() -> Self {
        static TYPE_ID: AtomicUsize = AtomicUsize::new(0);
        Self(TYPE_ID.fetch_add(1, Ordering::Relaxed))
    }
    pub fn assert_subtype_of(self, parent: TypeId, validator: &Validator) -> ParseResult<()> {
        if self == parent {
            Ok(())
        } else {
            validator
                .get_type(self)?
                .assert_subtype_of(validator.get_type(parent)?, validator)
        }
    }
}
