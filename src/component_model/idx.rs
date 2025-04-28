use std::fmt::{Debug, Formatter};
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;
use std::ops::Deref;
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

impl<T> GlobalIdx<T> {
    pub fn new() -> Self {
        Self(
            GLOBAL_IDX_COUNTER.fetch_add(1, Ordering::Relaxed),
            PhantomData,
        )
    }
}

pub trait Idx: Clone {
    fn new(global: usize) -> Self;
    fn global(&self) -> usize;
}

macro_rules! impl_idx {
    ($name:ident) => {
        impl Idx for $name {
            fn new(global: usize) -> Self {
                Self(global)
            }

            fn global(&self) -> usize {
                self.0
            }
        }

        impl Deref for $name {
            type Target = usize;
            fn deref(&self) -> &Self::Target {
                &self.0
            }
        }

        impl From<usize> for $name {
            fn from(global: usize) -> Self {
                Self::new(global)
            }
        }
    };
}

#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct TypeIdx(usize);

impl_idx!(TypeIdx);

#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct CoreFuncIdx(usize);

impl_idx!(CoreFuncIdx);

#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct FuncIdx(usize);

impl_idx!(FuncIdx);

#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct CoreMemoryIdx(usize);

impl_idx!(CoreMemoryIdx);

#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct CoreTableIdx(usize);
impl_idx!(CoreTableIdx);

#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct CoreGlobalIdx(usize);
impl_idx!(CoreGlobalIdx);

#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct CoreTypeIdx(usize);

impl_idx!(CoreTypeIdx);

#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct ComponentIdx(usize);

impl_idx!(ComponentIdx);

#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct InstanceIdx(usize);

impl_idx!(InstanceIdx);

#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct CoreModuleIdx(usize);
impl_idx!(CoreModuleIdx);

#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct CoreInstanceIdx(usize);
impl_idx!(CoreInstanceIdx);

#[cfg(feature = "component-gated-feature-value-imports-exports")]
#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct ValueIdx(usize);
#[cfg(feature = "component-gated-feature-value-imports-exports")]
impl_idx!(ValueIdx);

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
