mod canon;
pub mod id;
pub mod types;

use crate::common::{Import, ImportDesc};
use crate::component_model::id::{
    ComponentId, CoreFuncId, CoreGlobalId, CoreInstanceId, CoreMemoryId, CoreModuleId, CoreTableId,
    CoreTypeId, FuncId, InstanceId, ModuleId, SortId, TypeId,
};
use crate::component_model::types::Type;
use crate::Module;
pub use canon::*;

pub struct Component {
    pub modules: Vec<Module>,
    pub core_instances: Vec<CoreInstance>,
    pub core_types: Vec<CoreType>,
    pub components: Vec<Component>,
    pub instances: Vec<Instance>,
    pub aliases: Vec<Alias>,
    pub types: Vec<Type>,
    // canons
    // start
    // import
    // export
    // value
}

pub enum CoreInstance {
    Instantiate(CoreInstantiate),
    InlineExport(Vec<CoreInstanceInlineExport>),
}

pub struct CoreInstantiate {
    pub module_idx: ModuleId,
    pub args: Vec<CoreInstantiateArg>,
}

pub struct CoreInstantiateArg {
    pub name: String,
    pub instance_idx: InstanceId,
}

pub struct CoreInstanceInlineExport {
    pub name: String,
    pub sort: CoreSort,
    pub sort_idx: SortId,
}

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

pub enum CoreSort {
    Func(CoreFuncId),
    Table(CoreTableId),
    Memory(CoreMemoryId),
    Global(CoreGlobalId),
    Type(CoreTypeId),
    Module(CoreModuleId),
    Instance(CoreInstanceId),
}

pub enum CoreType {
    #[cfg(feature = "wasm3")]
    CoreRecType,
    #[cfg(feature = "wasm3")]
    Sub(Vec<CoreTypeId>, todo!("comptype")),
    CoreModuleType(Vec<CoreModuleDecl>),
}

pub enum CoreModuleDecl {
    Import(Import),
    Type(CoreType),
    Alias(CoreAlias),
    ExportDecl(CoreExportDecl),
}

pub struct CoreExportDecl {
    name: String,
    import_desc: ImportDesc,
}

pub struct CoreAlias {
    pub sort: CoreSort,
    pub target: CoreAliasTarget,
}

pub enum CoreAliasTarget {
    Outer(u32, usize),
}

pub enum Instance {
    Instantiate(Instantiate),
    InlineExport(Vec<InlineExport>),
}

pub struct Instantiate {
    pub component_idx: ComponentId,
    pub args: Vec<InstantiateArg>,
}

pub struct InstantiateArg {
    pub name: String,
    pub sort: Sort,
}

#[repr(u8)]
pub enum SortType {
    Core(CoreSort) = 0x00,
    Func = 0x01,
    Value = 0x02,
    Type = 0x03,
    Component = 0x04,
    Instance = 0x05,
}

pub enum Sort {
    Core(CoreSort, usize),
    Func(FuncId, usize),
    Value,
    Type(TypeId, usize),
    Component(ComponentId, usize),
    Instance(InstanceId, usize),
}

pub struct InlineExport {
    pub name: String,
    pub sort: Sort,
}

pub struct Alias {
    // pub sort: Sort,
    pub target: AliasTarget,
}

#[repr(u8)]
pub enum AliasTarget {
    Export(Sort, String) = 0x00,
    CoreExport(Sort, String) = 0x01,
    Outer(u32, Sort) = 0x02,
}
