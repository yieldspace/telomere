use crate::component_model::types::TypeId;
use crate::component_model::ScopeId;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Debug, Clone, PartialEq)]
pub enum ExternDesc {
    Component(TypeId),
    Instance(TypeId),
    Eq(TypeId),
    Sub,
    Func(TypeId),
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum PlaceholderType {
    Import,
    Export,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PlaceholderId(ScopeId, u64, PlaceholderType);

impl PlaceholderId {
    pub fn new(scope_id: ScopeId, name: &impl Hash, ty: PlaceholderType) -> Self {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        name.hash(&mut hasher);
        Self(scope_id, hasher.finish(), ty)
    }

    pub fn name_hash(&self) -> u64 {
        self.1
    }
}
