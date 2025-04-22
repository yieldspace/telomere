use std::ops::Deref;

pub trait Idx: Clone + Deref<Target = usize> {
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
