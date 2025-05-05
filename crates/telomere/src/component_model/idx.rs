use std::fmt::{Debug, Formatter};
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;
use std::sync::atomic::{AtomicUsize, Ordering};

pub struct GlobalIdx<T>(usize, PhantomData<T>);

impl<T> Clone for GlobalIdx<T> {
    fn clone(&self) -> Self {
        Self(self.0, PhantomData)
    }
}

impl<T> Copy for GlobalIdx<T> {}

impl<T> Debug for GlobalIdx<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "GlobalIdx({})", self.0)
    }
}

impl<T> Hash for GlobalIdx<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

impl<T> PartialEq for GlobalIdx<T> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl<T> Eq for GlobalIdx<T> {}

static GLOBAL_IDX_COUNTER: AtomicUsize = AtomicUsize::new(0);

impl<T> Default for GlobalIdx<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> GlobalIdx<T> {
    pub fn new() -> Self {
        Self(
            GLOBAL_IDX_COUNTER.fetch_add(1, Ordering::Relaxed),
            PhantomData,
        )
    }
}

#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub enum AliasIdx {
    CoreFunc,
    CoreTable,
    CoreMemory,
    CoreGlobal,
    CoreType,
    CoreModule,
    CoreInstance,
    Func,
    #[cfg(feature = "component-gated-feature-value-imports-exports")]
    Value(ValueIdx),
    Type,
    Component,
    Instance,
}
