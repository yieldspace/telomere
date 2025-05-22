use crate::parser::component_model::{ParseResult, Validator};
use std::hash::Hash;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct ScopeId(u32, u32);

impl ScopeId {
    pub fn new(depth: u32) -> Self {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        Self(depth, COUNTER.fetch_add(1, Ordering::Relaxed))
    }

    pub fn depth(&self) -> u32 {
        self.0
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ResourceId(u32);

impl ResourceId {
    pub fn new() -> Self {
        static RESOURCE_HANDLE: AtomicU32 = AtomicU32::new(0);
        Self(RESOURCE_HANDLE.fetch_add(1, Ordering::Relaxed))
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct TypeId(usize);

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
