mod func;
mod instance;
mod sort;
mod types;

pub use crate::common::FuncType as CoreFuncType;
use crate::component_model::{
    Binding, CoreFuncIdx, CoreGlobalIdx, CoreInstanceIdx, CoreMemoryIdx, CoreModuleIdx,
    CoreTableIdx, CoreTypeIdx,
};
pub use func::*;
pub use instance::*;
pub use sort::*;
pub use types::*;

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
pub struct CoreMemoryRef(pub CoreInstanceIdx, pub usize);
#[derive(Debug)]
pub struct CoreTableRef(pub CoreInstanceIdx, pub usize);
#[derive(Debug)]
pub struct CoreGlobalRef(pub CoreInstanceIdx, pub usize);
#[derive(Debug)]
pub struct CoreFuncRef(pub CoreInstanceIdx, pub usize, pub CoreFuncType, pub String);
#[derive(Debug)]
pub struct CoreTypeRef(pub CoreInstanceIdx, pub usize);
