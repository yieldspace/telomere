use crate::common::{ExportDesc, ImportDesc};
pub use crate::common::{
    FuncType as CoreFuncType, GlobalType as CoreGlobalType, MemType as CoreMemoryType,
    TableType as CoreTableType,
};
use crate::component_model::types::CoreSortType;
use crate::parser::component_model::{ComponentParseError, ParseResult};
use crate::Module;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub enum CoreModuleExportType {
    Memory(CoreMemoryType),
    Table(CoreTableType),
    Func(CoreFuncType),
    Global(CoreGlobalType),
}

#[derive(Debug, Clone, PartialEq)]
pub enum CoreModuleImportType {
    Func(CoreFuncType),
    Memory(CoreMemoryType),
    Table(CoreTableType),
    Global(CoreGlobalType),
}

#[derive(Debug, Clone, PartialEq)]
pub struct CoreInstanceType {
    pub(crate) exports: HashMap<String, CoreModuleExportType>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CoreType {
    Module(CoreModuleType),
}

#[derive(Debug, Clone, PartialEq)]
pub struct CoreModuleType {
    pub imports: HashMap<String, HashMap<String, CoreModuleImportType>>,
    pub exports: HashMap<String, CoreModuleExportType>,
}

impl From<CoreModuleType> for CoreInstanceType {
    fn from(value: CoreModuleType) -> Self {
        Self {
            exports: value.exports,
        }
    }
}

impl From<&Module> for CoreModuleType {
    fn from(value: &Module) -> Self {
        let mut imports = HashMap::<String, HashMap<String, CoreModuleImportType>>::new();
        value.imports.0.iter().for_each(|x| {
            let value = match x.desc {
                ImportDesc::TypeIdx(ref idx) => {
                    let ty = value.fts.0.get(idx.0 as usize).unwrap();
                    CoreModuleImportType::Func(ty.clone())
                }
                ImportDesc::TableType(ref ty) => CoreModuleImportType::Table(*ty),
                ImportDesc::MemType(ref ty) => CoreModuleImportType::Memory(*ty),
                ImportDesc::GlobalType(ref ty) => CoreModuleImportType::Global(*ty),
            };
            if let Some(y) = imports.get_mut(&x.module) {
                y.insert(x.name.clone(), value);
            } else {
                let mut map = HashMap::new();
                map.insert(x.name.clone(), value);
            }
        });
        let exports = value
            .exs
            .0
            .iter()
            .map(|x| {
                (x.0.clone(), {
                    match x.1 {
                        ExportDesc::Func(ref idx) => {
                            let tidx = value.functions.get(idx.0 as usize).unwrap();
                            let ty = value.fts.0.get(tidx.0 as usize).unwrap();
                            CoreModuleExportType::Func(ty.clone())
                        }
                        ExportDesc::Table(ref idx) => {
                            let ty = value.tables.get(idx.0 as usize).unwrap();
                            CoreModuleExportType::Table(*ty)
                        }
                        ExportDesc::Mem(ref idx) => {
                            let ty = value.mems.get(idx.0 as usize).unwrap();
                            CoreModuleExportType::Memory(*ty)
                        }
                        ExportDesc::Global(ref idx) => {
                            let ty = value.globals.get(idx.0 as usize).unwrap();
                            CoreModuleExportType::Global(*ty)
                        }
                    }
                })
            })
            .collect::<HashMap<_, _>>();
        Self { imports, exports }
    }
}

impl CoreModuleType {
    pub fn get_export(&self, name: &String) -> ParseResult<&CoreModuleExportType> {
        self.exports
            .get(name)
            .ok_or_else(|| ComponentParseError::ExportNotFound(name.clone()))
    }
}

impl From<CoreModuleExportType> for CoreSortType {
    fn from(val: CoreModuleExportType) -> Self {
        match val {
            CoreModuleExportType::Memory(_) => CoreSortType::Memory,
            CoreModuleExportType::Table(_) => CoreSortType::Table,
            CoreModuleExportType::Func(_) => CoreSortType::Func,
            CoreModuleExportType::Global(_) => CoreSortType::Global,
        }
    }
}

impl From<CoreFuncType> for CoreModuleExportType {
    fn from(value: CoreFuncType) -> Self {
        CoreModuleExportType::Func(value)
    }
}

impl From<CoreMemoryType> for CoreModuleExportType {
    fn from(value: CoreMemoryType) -> Self {
        CoreModuleExportType::Memory(value)
    }
}

impl From<CoreTableType> for CoreModuleExportType {
    fn from(value: CoreTableType) -> Self {
        CoreModuleExportType::Table(value)
    }
}

impl From<CoreGlobalType> for CoreModuleExportType {
    fn from(value: CoreGlobalType) -> Self {
        CoreModuleExportType::Global(value)
    }
}

impl From<CoreType> for CoreModuleExportType {
    fn from(_value: CoreType) -> Self {
        unimplemented!("type export proposal")
    }
}

impl From<CoreModuleType> for CoreModuleExportType {
    fn from(_value: CoreModuleType) -> Self {
        unimplemented!("module export proposal")
    }
}

impl From<CoreInstanceType> for CoreModuleExportType {
    fn from(_value: CoreInstanceType) -> Self {
        unimplemented!("instance export proposal")
    }
}
