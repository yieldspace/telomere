mod func;
mod instance;
mod module;
mod sort;
mod types;

pub use crate::common::FuncType as CoreFuncType;
use crate::component_model::{
    Binding, CoreFuncIdx, CoreGlobalIdx, CoreInstanceIdx, CoreMemoryIdx, CoreModuleIdx,
    CoreTableIdx, CoreTypeIdx, GlobalIdx, Slot, Type,
};
pub use func::*;
pub use instance::*;
pub use module::*;
pub use sort::*;
pub use types::*;

#[derive(Debug, Clone)]
pub enum CoreInstanceInlineExport {
    Func(GlobalIdx<CoreFunc>),
    Table(GlobalIdx<CoreTableRef>),
    Memory(GlobalIdx<CoreMemoryRef>),
    Global(GlobalIdx<CoreGlobalRef>),
    Type(CoreType),
    Module(GlobalIdx<CoreModule>),
    Instance(GlobalIdx<CoreInstance>),
}

#[derive(Debug, Clone)]
pub enum CoreInstanceImport {
    Instance(GlobalIdx<CoreInstance>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct CoreMemoryRef(pub GlobalIdx<CoreInstance>, pub String);
#[derive(Debug, Clone, PartialEq)]
pub struct CoreTableRef(pub GlobalIdx<CoreInstance>, pub String);
#[derive(Debug, Clone, PartialEq)]
pub struct CoreGlobalRef(pub GlobalIdx<CoreInstance>, pub String);
#[derive(Debug, Clone, PartialEq)]
pub struct CoreFuncRef(pub GlobalIdx<CoreInstance>, pub String);
#[derive(Debug, Clone, PartialEq)]
pub struct CoreTypeRef(pub GlobalIdx<CoreInstance>, pub String);
