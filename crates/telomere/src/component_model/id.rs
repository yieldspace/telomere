use crate::component_model::{ExportName, ImportName};
use std::hash::{DefaultHasher, Hash, Hasher};
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
}
