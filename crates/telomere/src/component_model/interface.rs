use crate::component_model::{ImportName, ScopeId, TypeId};
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
pub struct PlaceholderId(u64);

impl PlaceholderId {
    pub fn new(name: &impl Hash) -> Self {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        name.hash(&mut hasher);
        Self(hasher.finish())
    }

    pub fn name_hash(&self) -> u64 {
        self.0
    }
}
