mod func;
mod instance;
mod module;
mod sort;
mod types;

pub use crate::common::FuncType as CoreFuncType;
use crate::component_model::GlobalIdx;
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

pub enum CoreInstanceInlineExportType {
    Func(CoreFuncType),
    Table(crate::common::TableType),
    Memory(crate::common::MemType),
    Global(crate::common::GlobalType),
    Type(CoreType),
    Module(CoreModuleType),
    Instance(CoreInstanceType),
}

impl From<CoreFuncType> for CoreInstanceInlineExportType {
    fn from(value: CoreFuncType) -> Self {
        CoreInstanceInlineExportType::Func(value)
    }
}

impl From<crate::common::TableType> for CoreInstanceInlineExportType {
    fn from(value: crate::common::TableType) -> Self {
        CoreInstanceInlineExportType::Table(value)
    }
}

impl From<crate::common::MemType> for CoreInstanceInlineExportType {
    fn from(value: crate::common::MemType) -> Self {
        CoreInstanceInlineExportType::Memory(value)
    }
}

impl From<crate::common::GlobalType> for CoreInstanceInlineExportType {
    fn from(value: crate::common::GlobalType) -> Self {
        CoreInstanceInlineExportType::Global(value)
    }
}

impl From<CoreType> for CoreInstanceInlineExportType {
    fn from(value: CoreType) -> Self {
        CoreInstanceInlineExportType::Type(value)
    }
}

impl From<CoreModuleType> for CoreInstanceInlineExportType {
    fn from(value: CoreModuleType) -> Self {
        CoreInstanceInlineExportType::Module(value)
    }
}

impl From<CoreInstanceType> for CoreInstanceInlineExportType {
    fn from(value: CoreInstanceType) -> Self {
        CoreInstanceInlineExportType::Instance(value)
    }
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
