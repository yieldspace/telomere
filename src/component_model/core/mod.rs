mod func;
mod instance;
mod sort;
mod types;

use crate::binary::BinaryReader;
pub use crate::common::FuncType as CoreFuncType;
use crate::component_model::{
    Binding, CoreFuncIdx, CoreGlobalIdx, CoreInstanceIdx, CoreMemoryIdx, CoreModuleIdx,
    CoreTableIdx, CoreTypeIdx, Idx, Sort,
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

pub struct CoreMemoryRef(pub CoreInstanceIdx, pub usize);
pub struct CoreTableRef(pub CoreInstanceIdx, pub usize);
pub struct CoreGlobalRef(pub CoreInstanceIdx, pub usize);
pub struct CoreFuncRef(pub CoreInstanceIdx, pub usize, pub CoreFuncType);
