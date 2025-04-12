mod canon;
mod component;
pub mod id;
mod import_export;
pub mod types;
mod core;

use std::collections::HashMap;
use crate::common::{Import, ImportDesc};
use crate::component_model::id::{ComponentIdx, CoreFuncId, CoreInstanceIdx, CoreModuleIdx, CoreTypeIdx, FuncId, InstanceIdx, TypeId};
use crate::Module;
pub use canon::*;
pub use component::*;
pub use import_export::*;
pub use core::*;
use std::fmt::{Debug, Formatter};

impl Debug for Module {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str("Module {core module}")
    }
}

#[derive(Debug)]
pub enum CoreFunc {

}

#[derive(Debug)]
pub struct CoreInstantiate {
    // pub module_idx: CoreModuleIdx,
    pub args: Vec<CoreInstantiateArg>,
}

#[derive(Debug)]
pub struct CoreInstantiateArg {
    pub name: String,
    pub instance_idx: CoreInstanceIdx,
}

#[derive(Debug)]
pub struct CoreInstanceInlineExport {
    pub name: String,
    pub sort: CoreSort,
}

#[derive(Debug)]
#[repr(u8)]
pub enum CoreSortType {
    Func = 0x00,
    Table = 0x01,
    Memory = 0x02,
    Global = 0x03,
    Type = 0x10,
    Module = 0x11,
    Instance = 0x12,
}

#[derive(Debug)]
pub enum CoreSort {
    Func(u32),
    Table(u32),
    Memory(u32),
    Global(u32),
    Type(u32),
    Module(u32),
    Instance(u32),
}

#[derive(Debug)]
pub enum CoreType {
    #[cfg(feature = "wasm3")]
    CoreRecType,
    #[cfg(feature = "wasm3")]
    Sub(Vec<CoreTypeIdx>, todo!("comptype")),
    CoreModuleType(Vec<CoreModuleDecl>),
}

#[derive(Debug)]
pub enum CoreModuleDecl {
    Import(Import),
    Type(CoreType),
    Alias(CoreAlias),
    ExportDecl(CoreExportDecl),
}

#[derive(Debug)]
pub struct CoreExportDecl {
    name: String,
    import_desc: ImportDesc,
}

#[derive(Debug)]
pub struct CoreAlias {
    pub sort: CoreSort,
    pub target: CoreAliasTarget,
}

#[derive(Debug)]
pub enum CoreAliasTarget {
    Outer(u32, usize),
}

#[derive(Debug)]
pub enum Instance {
    Instantiate(Instantiate),
    InlineExport(Vec<InlineExport>),
}

pub struct CInstance {
    component: Option<ComponentIdx>,

}

#[derive(Debug)]
pub struct Instantiate {
    pub component_idx: ComponentIdx,
    pub args: Vec<InstantiateArg>,
}

#[derive(Debug)]
pub struct InstantiateArg {
    pub name: String,
    pub sort: Sort,
}

#[derive(Debug)]
#[repr(u8)]
pub enum SortType {
    Core(CoreSort) = 0x00,
    Func = 0x01,
    Value = 0x02,
    Type = 0x03,
    Component = 0x04,
    Instance = 0x05,
}

#[derive(Debug)]
pub enum Sort {
    Core(CoreSort, usize),
    Func(FuncId, usize),
    Value,
    Type(TypeId, usize),
    Component(ComponentIdx, usize),
    Instance(InstanceIdx, usize),
}

#[derive(Debug)]
pub struct InlineExport {
    pub name: String,
    pub sort: Sort,
}

#[derive(Debug)]
pub struct Alias {
    // pub sort: Sort,
    pub target: AliasTarget,
}

#[derive(Debug)]
#[repr(u8)]
pub enum AliasTarget {
    Export(Sort, InstanceIdx, String) = 0x00,
    CoreExport(Sort, CoreInstanceIdx, String) = 0x01,
    Outer(u32, Sort, u32) = 0x02,
}
