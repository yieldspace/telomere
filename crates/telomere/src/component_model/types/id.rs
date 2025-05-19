use crate::component_model::types::Type;
use std::cell::Cell;
use std::sync::atomic::{AtomicUsize, Ordering};

static TYPE_COUNTER: AtomicUsize = AtomicUsize::new(0);
static CORE_MODULE_COUNTER: AtomicUsize = AtomicUsize::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Hash, Eq)]
pub struct TypeId(usize);

impl TypeId {
    pub fn new() -> Self {
        Self(TYPE_COUNTER.fetch_add(1, Ordering::Relaxed))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Hash, Eq)]
pub struct CoreModuleId(usize);

impl CoreModuleId {
    pub fn new() -> Self {
        Self(CORE_MODULE_COUNTER.fetch_add(1, Ordering::Relaxed))
    }
}
