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
