use crate::common::ExportDesc;
use crate::component_model::{
    CoreGlobalIdx, CoreGlobalRef, CoreInstanceIdx, CoreInstanceInlineExport, CoreMemoryIdx,
    CoreMemoryRef, CoreSort, CoreTableIdx, CoreTableRef, CoreTypeIdx, CoreTypeRef,
};
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
    pub(crate) types: Vec<CoreType>,
    pub(crate) globals: Vec<CoreGlobalRef>,
    pub(crate) tables: Vec<CoreTableRef>,
    pub(crate) memories: Vec<CoreMemoryRef>,
    pub(crate) exports: HashMap<String, crate::common::ImportDesc>,
}

pub enum CoreExportType {
    Memory(crate::common::MemIdx),
    Table(crate::common::TableIdx),
    Func(crate::common::FuncIdx),
    Global(crate::common::GlobalIdx),
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
        todo!();
        // for export in module.exs.0.iter() {
        //     let name = export.0.clone();
        //     match export.1 {
        //         ExportDesc::Func(_) => {}
        //         ExportDesc::Table(_) => {}
        //         ExportDesc::Mem(_) => {}
        //         ExportDesc::Global(_) => {}
        //     }
        // }
        // Self {}
    }

    pub fn get_export_type(
        &self,
        sort: &CoreSort,
        name: &String,
    ) -> Result<CoreExportType, ComponentParseError> {
        todo!()
    }
}
