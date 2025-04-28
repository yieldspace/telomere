use crate::component_model::{
    CoreInstanceImport,
    CoreInstanceInlineExport, CoreModule, CoreModuleType, CoreSort, GlobalIdx,
};
use crate::parser::component_model::ParseResult;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub enum CoreInstance {
    Real {
        module_idx: GlobalIdx<CoreModule>,
        imports: HashMap<String, CoreInstanceImport>,
    },
    Alias {
        exports: HashMap<String, CoreInstanceInlineExport>,
    },
}

#[derive(Debug, Clone)]
pub struct CoreInstanceType {}

pub enum CoreInstanceExportType {
    Memory(String, crate::common::MemType),
    Table(String, crate::common::TableType),
    Func(String, crate::common::FuncType),
    Global(String, crate::common::GlobalType),
}

impl CoreInstanceType {
    pub fn new() -> Self {
        CoreInstanceType {}
    }

    pub fn get_export_type(
        &self,
        sort: &CoreSort,
        name: &String,
    ) -> ParseResult<CoreInstanceExportType> {
        todo!()
    }
}

impl From<(&CoreInstance, Option<&CoreModuleType>)> for CoreInstanceType {
    fn from(value: (&CoreInstance, Option<&CoreModuleType>)) -> Self {
        todo!()
    }
}
