use crate::common::ExportDesc;
use crate::component_model::{CoreInstanceInlineExportType, CoreSort, CoreTypeRef, ExternDesc};
use crate::parser::component_model::ComponentParseError;
use crate::Module;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub enum CoreType {
    Ref(CoreTypeRef),
    ModuleType(CoreModuleType),
}

#[derive(Debug, Clone)]
pub enum CoreAlias {}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CoreModuleType {
    pub(crate) imports: Vec<crate::common::Import>,
    pub(crate) exports: HashMap<String, CoreExportType>,
}

impl TryFrom<ExternDesc> for CoreModuleType {
    type Error = ComponentParseError;

    fn try_from(value: ExternDesc) -> Result<Self, Self::Error> {
        if let ExternDesc::CoreModule(module_type) = value {
            Ok(module_type)
        } else {
            Err(ComponentParseError::InvalidType(
                "CoreModuleType".to_string(),
            ))
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum CoreExportType {
    Memory(crate::common::MemType),
    Table(crate::common::TableType),
    Func(crate::common::FuncType),
    Global(crate::common::GlobalType),
}

impl TryFrom<CoreInstanceInlineExportType> for CoreExportType {
    type Error = ComponentParseError;

    fn try_from(value: CoreInstanceInlineExportType) -> Result<Self, Self::Error> {
        match value {
            CoreInstanceInlineExportType::Func(ty) => Ok(CoreExportType::Func(ty)),
            CoreInstanceInlineExportType::Table(ty) => Ok(CoreExportType::Table(ty)),
            CoreInstanceInlineExportType::Memory(ty) => Ok(CoreExportType::Memory(ty)),
            CoreInstanceInlineExportType::Global(ty) => Ok(CoreExportType::Global(ty)),
            CoreInstanceInlineExportType::Type(_) => Err(ComponentParseError::Unsupported(
                "exporting core type is not supported".to_string(),
            )),
            CoreInstanceInlineExportType::Module(_) => Err(ComponentParseError::Unsupported(
                "exporting core module is not supported".to_string(),
            )),
            CoreInstanceInlineExportType::Instance(_) => Err(ComponentParseError::Unsupported(
                "exporting core instance is not supported".to_string(),
            )),
        }
    }
}

impl PartialEq<CoreSort> for CoreExportType {
    fn eq(&self, other: &CoreSort) -> bool {
        match (self, other) {
            (CoreExportType::Memory(_), CoreSort::Memory) => true,
            (CoreExportType::Table(_), CoreSort::Table) => true,
            (CoreExportType::Func(_), CoreSort::Func) => true,
            (CoreExportType::Global(_), CoreSort::Global) => true,
            _ => false,
        }
    }
}

impl TryFrom<CoreType> for CoreModuleType {
    type Error = ComponentParseError;

    fn try_from(value: CoreType) -> Result<Self, Self::Error> {
        if let CoreType::ModuleType(module_type) = value {
            Ok(module_type)
        } else {
            Err(ComponentParseError::InvalidType("ModuleType".to_string()))
        }
    }
}

impl CoreModuleType {
    pub fn from_module(module: &Module) -> Self {
        let mut slf = Self::default();
        for crate::common::Export(name, desc) in module.exs.0.iter() {
            match desc {
                ExportDesc::Func(idx) => {
                    let func_type_idx = module.functions.get(idx.0 as usize).unwrap();
                    let ty = module.fts.get(func_type_idx.clone()).unwrap();
                    slf.exports
                        .insert(name.clone(), CoreExportType::Func(ty.clone()));
                }
                ExportDesc::Table(idx) => {
                    let table_type = module.tables.get(idx.0 as usize).unwrap();
                    slf.exports
                        .insert(name.clone(), CoreExportType::Table(table_type.clone()));
                }
                ExportDesc::Mem(idx) => {
                    let mem_type = module.mems.get(idx.0 as usize).unwrap();
                    slf.exports
                        .insert(name.clone(), CoreExportType::Memory(mem_type.clone()));
                }
                ExportDesc::Global(idx) => {
                    let global_type = module.globals.get(idx.0 as usize).unwrap();
                    slf.exports
                        .insert(name.clone(), CoreExportType::Global(global_type.clone()));
                }
            }
        }
        slf.imports = module.imports.0.clone();
        slf
    }

    pub fn get_export_type(
        &self,
        sort: &CoreSort,
        name: &String,
    ) -> Result<CoreExportType, ComponentParseError> {
        let target = self
            .exports
            .get(name)
            .ok_or_else(|| ComponentParseError::ExportNotFound(name.clone()))?;
        if target == sort {
            Ok(target.clone())
        } else {
            Err(ComponentParseError::InvalidExportType(
                name.clone(),
                target.clone(),
                sort.clone(),
            ))
        }
    }
}
