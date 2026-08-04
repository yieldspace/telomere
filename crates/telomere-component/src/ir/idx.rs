use std::fmt::{Debug, Formatter};
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;
use std::sync::atomic::{AtomicUsize, Ordering};

macro_rules! impl_idx {
    ($name:ident, $value:ty) => {
        pub struct $name<T>($value, PhantomData<T>);

        impl<T> Clone for $name<T> {
            fn clone(&self) -> Self {
                *self
            }
        }

        impl<T> Copy for $name<T> {}

        impl<T> Debug for $name<T> {
            fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}({})", stringify!($name), self.0)
            }
        }

        impl<T> Hash for $name<T> {
            fn hash<H: Hasher>(&self, state: &mut H) {
                self.0.hash(state);
            }
        }

        impl<T> PartialEq for $name<T> {
            fn eq(&self, other: &Self) -> bool {
                self.0 == other.0
            }
        }

        impl<T> Eq for $name<T> {}
    };
}

impl_idx!(LocalIdx, u32);
impl_idx!(GlobalIdx, usize);

impl<T> LocalIdx<T> {
    pub fn new(value: u32) -> Self {
        Self(value, PhantomData)
    }

    pub fn get(&self) -> u32 {
        self.0
    }
}

impl<T> From<usize> for LocalIdx<T> {
    fn from(value: usize) -> Self {
        Self::new(value as u32)
    }
}

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
// Retained conservatively; no current crate path references this alias-index IR.
#[allow(dead_code)]
pub enum AliasIdx {
    CoreFunc,
    CoreTable,
    CoreMemory,
    CoreGlobal,
    CoreType,
    CoreModule,
    CoreInstance,
    Func,
    Type,
    Component,
    Instance,
}

#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct AnyGlobalIdx(usize);

impl<T> From<AnyGlobalIdx> for GlobalIdx<T> {
    fn from(value: AnyGlobalIdx) -> Self {
        Self(value.0, PhantomData)
    }
}

impl<T> From<GlobalIdx<T>> for AnyGlobalIdx {
    fn from(value: GlobalIdx<T>) -> Self {
        Self(value.0)
    }
}

impl AnyGlobalIdx {
    pub fn raw(self) -> usize {
        self.0
    }
}
