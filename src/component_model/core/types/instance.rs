use crate::component_model::{CoreModuleExportType, CoreModuleType};
use crate::parser::component_model::{ComponentParseError, ParseResult};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub struct CoreInstanceType {
    pub(crate) exports: HashMap<String, CoreModuleExportType>,
}

impl CoreInstanceType {
    pub fn new(exports: HashMap<String, CoreModuleExportType>) -> Self {
        CoreInstanceType { exports }
    }

    pub fn get_export_type(&self, name: &String) -> ParseResult<CoreModuleExportType> {
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
