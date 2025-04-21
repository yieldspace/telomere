mod func;
mod instance;
mod module;
mod sort;
mod types;

pub use crate::common::FuncType as CoreFuncType;
use crate::component_model::{
    Binding, CoreFuncIdx, CoreGlobalIdx, CoreInstanceIdx, CoreMemoryIdx, CoreModuleIdx,
    CoreTableIdx, CoreTypeIdx, Slot,
};
pub use func::*;
pub use instance::*;
pub use module::CoreModule;
pub use sort::*;
pub use types::*;

#[derive(Debug)]
pub enum CoreReference {
    Module(CoreModuleIdx, String),
    Instance(CoreInstanceIdx, String),
}

pub enum CoreExportSlot {
    Func(Slot<CoreFunction, CoreFuncIdx>, CoreReference),
    Table(Slot<CoreTableRef, CoreTableIdx>, CoreReference),
    Memory(Slot<CoreMemoryRef, CoreMemoryIdx>, CoreReference),
    Global(Slot<CoreGlobalRef, CoreGlobalIdx>, CoreReference),
    Type(Slot<CoreType, CoreTypeIdx>, CoreReference),
}

pub enum CoreBinding<T, R> {
    Real(R),
    Binding(Binding<T>),
}

pub enum CoreInstanceInlineExport {
    Func(CoreFuncIdx),
    Table(CoreTableIdx),
    Memory(CoreMemoryIdx),
    Global(CoreGlobalIdx),
    Type(CoreTypeIdx),
    Module(CoreModuleIdx),
    Instance(CoreInstanceIdx),
}

pub enum CoreInstanceImport {
    Instance(CoreInstanceIdx),
}

#[derive(Debug)]
pub struct CoreMemoryRef(pub CoreInstanceIdx, pub crate::common::MemIdx, pub String);
#[derive(Debug)]
pub struct CoreTableRef(pub CoreInstanceIdx, pub crate::common::TableIdx, pub String);
#[derive(Debug)]
pub struct CoreGlobalRef(
    pub CoreInstanceIdx,
    pub crate::common::GlobalIdx,
    pub String,
);
#[derive(Debug)]
pub struct CoreFuncRef(pub CoreInstanceIdx, pub crate::common::FuncIdx, pub String);
#[derive(Debug)]
pub struct CoreTypeRef(pub CoreInstanceIdx, pub crate::common::TypeIdx, pub String);
