pub trait Idx {
    fn new(local: usize, global: usize) -> Self;
    fn local(&self) -> usize;
    fn global(&self) -> usize;
}

macro_rules! impl_idx {
    ($name:ident) => {
        impl Idx for $name {
            fn new(local: usize, global: usize) -> Self {
                Self(local, global)
            }

            fn local(&self) -> usize {
                self.0
            }

            fn global(&self) -> usize {
                self.1
            }
        }
    };
}

#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct TypeIdx(usize, usize);

impl_idx!(TypeIdx);

#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct CoreFuncIdx(usize, usize);

impl_idx!(CoreFuncIdx);

#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct FuncIdx(usize, usize);

impl_idx!(FuncIdx);

#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct CoreMemoryIdx(usize, usize);

impl_idx!(CoreMemoryIdx);

#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct CoreTableIdx(usize, usize);
impl_idx!(CoreTableIdx);

#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct CoreGlobalIdx(usize, usize);
impl_idx!(CoreGlobalIdx);

#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct CoreTypeIdx(usize, usize);

impl_idx!(CoreTypeIdx);

#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct ComponentIdx(usize, usize);

impl_idx!(ComponentIdx);

#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct InstanceIdx(usize, usize);

impl_idx!(InstanceIdx);

#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct CoreModuleIdx(usize, usize);
impl_idx!(CoreModuleIdx);

#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct CoreInstanceIdx(usize, usize);
impl_idx!(CoreInstanceIdx);

#[cfg(feature = "component-gated-feature-value-imports-exports")]
#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct ValueIdx(usize, usize);
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
