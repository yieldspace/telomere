use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub enum CoreModuleExportType {
    Memory(crate::common::MemType),
    Table(crate::common::TableType),
    Func(crate::common::FuncType),
    Global(crate::common::GlobalType),
}

#[derive(Debug, Clone, PartialEq)]
pub struct CoreInstanceType {
    pub(crate) exports: HashMap<String, CoreModuleExportType>,
}

pub enum CoreType {
    Module(CoreModuleType),
}

pub struct CoreModuleType {
    pub exports: HashMap<String, CoreModuleExportType>,
}
