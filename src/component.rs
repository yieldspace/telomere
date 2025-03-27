use crate::common::{Import, ImportDesc};

pub struct Component {}

pub enum CoreInstance {
    Instantiate(CoreInstantiate),
    InlineExport(Vec<CoreInstanceInlineExport>),
}

pub struct CoreInstantiate {
    pub module_idx: usize,
    pub args: Vec<CoreInstantiateArg>,
}

pub struct CoreInstantiateArg {
    pub name: String,
    pub instance_idx: usize,
}

pub struct CoreInstanceInlineExport {
    pub name: String,
    pub sort: CoreSort,
    pub sort_idx: usize,
}

#[repr(u8)]
pub enum CoreSort {
    Func = 0x00,
    Table = 0x01,
    Memory = 0x02,
    Global = 0x03,
    Type = 0x10,
    Module = 0x11,
    Instance = 0x12,
}

pub enum CoreType {
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

pub struct CoreAliasTarget {
    pub ct: u32,
    pub idx: u32,
}

pub enum Instance {
    Instantiate(Instantiate),
    InlineExport(Vec<InlineExport>),
}

pub struct Instantiate {
    pub component_idx: usize,
    pub args: Vec<InstantiateArg>,
}

pub struct InstantiateArg {
    pub name: String,
    pub sort: Sort,
    pub sort_idx: usize,
}

#[repr(u8)]
pub enum Sort {
    Core(CoreSort) = 0x00,
    Func = 0x01,
    Value = 0x02,
    Type = 0x03,
    Component = 0x04,
    Instance = 0x05,
}

pub struct InlineExport {
    pub name: String,
    pub sort: Sort,
    pub sort_idx: usize,
}
