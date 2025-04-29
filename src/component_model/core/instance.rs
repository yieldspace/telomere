use crate::component_model::{
    CoreExportType, CoreInstanceInlineExport, CoreModule, CoreModuleType, GlobalIdx, Instance,
};
use crate::parser::component_model::{ComponentParseError, ParseResult};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub enum CoreInstance {
    Real {
        module_idx: GlobalIdx<CoreModule>,
        imports: HashMap<String, GlobalIdx<Instance>>,
    },
    Alias {
        exports: HashMap<String, CoreInstanceInlineExport>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct CoreInstanceType {
    pub(crate) exports: HashMap<String, CoreExportType>,
}

impl CoreInstanceType {
    pub fn new(exports: HashMap<String, CoreExportType>) -> Self {
        CoreInstanceType { exports }
    }

    pub fn get_export_type(&self, name: &String) -> ParseResult<CoreExportType> {
        self.exports
            .get(name)
            .cloned()
            .ok_or_else(|| ComponentParseError::ExportNotFound(name.clone()))
    }
}

impl From<CoreModuleType> for CoreInstanceType {
    fn from(value: CoreModuleType) -> Self {
        Self {
            exports: value.exports,
        }
    }
}
