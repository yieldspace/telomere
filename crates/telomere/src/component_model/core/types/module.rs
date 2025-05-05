use crate::common::ExportDesc;
use crate::component_model::{CoreInstanceInlineExportType, CoreSort, CoreType, ExternDesc};
use crate::parser::component_model::ComponentParseError;
use crate::Module;
use std::collections::HashMap;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CoreModuleType {
    pub(crate) imports: Vec<crate::common::Import>,
    pub(crate) exports: HashMap<String, CoreModuleExportType>,
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
                    let ty = module.fts.get(*func_type_idx).unwrap();
                    slf.exports
                        .insert(name.clone(), CoreModuleExportType::Func(ty.clone()));
                }
                ExportDesc::Table(idx) => {
                    let table_type = module.tables.get(idx.0 as usize).unwrap();
                    slf.exports
                        .insert(name.clone(), CoreModuleExportType::Table(*table_type));
                }
                ExportDesc::Mem(idx) => {
                    let mem_type = module.mems.get(idx.0 as usize).unwrap();
                    slf.exports
                        .insert(name.clone(), CoreModuleExportType::Memory(*mem_type));
                }
                ExportDesc::Global(idx) => {
                    let global_type = module.globals.get(idx.0 as usize).unwrap();
                    slf.exports
                        .insert(name.clone(), CoreModuleExportType::Global(*global_type));
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
    ) -> Result<CoreModuleExportType, ComponentParseError> {
        let target = self
            .exports
            .get(name)
            .ok_or_else(|| ComponentParseError::CoreExportNotFound(name.clone()))?;
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

#[derive(Debug, Clone, PartialEq)]
pub enum CoreModuleExportType {
    Memory(crate::common::MemType),
    Table(crate::common::TableType),
    Func(crate::common::FuncType),
    Global(crate::common::GlobalType),
}

impl TryFrom<CoreInstanceInlineExportType> for CoreModuleExportType {
    type Error = ComponentParseError;

    fn try_from(value: CoreInstanceInlineExportType) -> Result<Self, Self::Error> {
        match value {
            CoreInstanceInlineExportType::Func(ty) => Ok(CoreModuleExportType::Func(ty)),
            CoreInstanceInlineExportType::Table(ty) => Ok(CoreModuleExportType::Table(ty)),
            CoreInstanceInlineExportType::Memory(ty) => Ok(CoreModuleExportType::Memory(ty)),
            CoreInstanceInlineExportType::Global(ty) => Ok(CoreModuleExportType::Global(ty)),
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

impl PartialEq<CoreSort> for CoreModuleExportType {
    fn eq(&self, other: &CoreSort) -> bool {
        matches!(
            (self, other),
            (CoreModuleExportType::Memory(_), CoreSort::Memory)
                | (CoreModuleExportType::Table(_), CoreSort::Table)
                | (CoreModuleExportType::Func(_), CoreSort::Func)
                | (CoreModuleExportType::Global(_), CoreSort::Global)
        )
    }
}
