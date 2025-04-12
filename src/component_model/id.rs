use crate::component_model::{Component, CoreInstance, CoreType, Instance};
use crate::Module;
use std::sync::Weak;

#[derive(Debug)]
pub struct CoreModuleIdx(pub usize);

#[derive(Debug)]
pub struct CoreInstanceIdx(pub usize);

#[derive(Debug)]
pub struct InstanceIdx(pub Weak<Instance>);

#[derive(Debug)]
pub struct CoreTypeIdx(pub Weak<CoreType>);

#[derive(Debug)]
pub enum CoreSortId {
    Func(usize),
    Table(usize),
    Memory(usize),
    Global(usize),
    Type(usize),
    Module(usize),
    Instance(usize),
}

#[derive(Debug)]
pub struct CoreFuncId(pub u32);

#[derive(Debug)]
pub struct CoreTableId(pub u32);

#[derive(Debug)]
pub struct CoreMemoryId(pub u32);

#[derive(Debug)]
pub struct CoreGlobalId(pub u32);

#[derive(Debug)]
pub enum SortId {
    CoreSort(CoreSortId),
    Func(usize),
    #[cfg(feature = "import_export")]
    Value(usize),
    Type(usize),
    Component(usize),
    Instance(usize),
}

#[derive(Debug)]
pub struct ComponentIdx(pub Weak<Component>);

#[derive(Debug)]
pub struct TypeId(pub i32);

#[derive(Debug)]
pub struct FuncId {
    pub value: usize,
}
