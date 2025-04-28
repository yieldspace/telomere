use crate::component_model::{
    ComponentFunction, CoreFunction, CoreGlobalRef, CoreInstance, CoreMemoryRef, CoreModule,
    CoreTableRef, CoreType, InlineComponent, Instance, InstanceType, Type,
};
use std::ops::Deref;

pub trait Idx: Clone + Deref<Target = usize> {
    fn new(global: usize) -> Self;
    fn global(&self) -> usize;
}

pub trait Resolvable<V>: Idx {
    fn resolve<'a, T: Resolver<V>>(&self, resolver: &'a T) -> Result<&'a V, T::Error> {
        resolver.resolve(self)
    }
}

pub trait Resolver<O> {
    type Error;

    fn resolve<I>(&self, idx: &I) -> Result<&O, Self::Error>
    where
        I: Idx + Resolvable<O>,
        Self: Sized;
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

macro_rules! impl_resolve {
    ($name:ident, $target:ident) => {
        impl Resolvable<$target> for $name {}
    };
}

#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct TypeIdx(usize);

impl_idx!(TypeIdx);
impl_resolve!(TypeIdx, Type);

#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct CoreFuncIdx(usize);

impl_idx!(CoreFuncIdx);
impl_resolve!(CoreFuncIdx, CoreFunction);

#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct FuncIdx(usize);

impl_idx!(FuncIdx);
impl_resolve!(FuncIdx, ComponentFunction);

#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct CoreMemoryIdx(usize);

impl_idx!(CoreMemoryIdx);
impl_resolve!(CoreMemoryIdx, CoreMemoryRef);

#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct CoreTableIdx(usize);
impl_idx!(CoreTableIdx);
impl_resolve!(CoreTableIdx, CoreTableRef);

#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct CoreGlobalIdx(usize);
impl_idx!(CoreGlobalIdx);
impl_resolve!(CoreGlobalIdx, CoreGlobalRef);

#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct CoreTypeIdx(usize);

impl_idx!(CoreTypeIdx);
impl_resolve!(CoreTypeIdx, CoreType);

#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct ComponentIdx(usize);

impl_idx!(ComponentIdx);
impl_resolve!(ComponentIdx, InlineComponent);

#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct InstanceIdx(usize);

impl_idx!(InstanceIdx);
impl_resolve!(InstanceIdx, Instance);
impl_resolve!(InstanceIdx, InstanceType);

#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct CoreModuleIdx(usize);
impl_idx!(CoreModuleIdx);
impl_resolve!(CoreModuleIdx, CoreModule);

#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct CoreInstanceIdx(usize);
impl_idx!(CoreInstanceIdx);
impl_resolve!(CoreInstanceIdx, CoreInstance);

#[cfg(feature = "component-gated-feature-value-imports-exports")]
#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct ValueIdx(usize);
#[cfg(feature = "component-gated-feature-value-imports-exports")]
impl_idx!(ValueIdx);

#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub enum AliasIdx {
    CoreFunc(CoreFuncIdx),
    CoreTable(CoreTableIdx),
    CoreMemory(CoreMemoryIdx),
    CoreGlobal(CoreGlobalIdx),
    CoreType(CoreTypeIdx),
    CoreModule(CoreModuleIdx),
    CoreInstance(CoreInstanceIdx),
    Func(FuncIdx),
    #[cfg(feature = "component-gated-feature-value-imports-exports")]
    Value(ValueIdx),
    Type(TypeIdx),
    Component(ComponentIdx),
    Instance(InstanceIdx),
}
