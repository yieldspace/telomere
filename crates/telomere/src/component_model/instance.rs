use crate::component_model::{
    ExportName, ExternDesc, GlobalIdx, ImportName, InlineComponent, InstanceType, SortWithIdx,
};
use crate::parser::component_model::ComponentParseError;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct Instance {
    pub(crate) component_idx: Option<GlobalIdx<InlineComponent>>,
    pub(crate) imports: HashMap<ImportName, ExternDesc>,
    pub(crate) exports: HashMap<ExportName, ExternDesc>,
}

impl Instance {
    pub fn as_type(&self) -> InstanceType {
        InstanceType {
            imports: self.imports.clone(),
            exports: self.exports.clone(),
        }
    }
}

impl Instance {
    pub fn get_export(&self, name: &ExportName) -> Result<Option<ExternDesc>, ComponentParseError> {
        self.exports
            .get(name)
            .cloned()
            .map(Some)
            .ok_or_else(|| ComponentParseError::ExportNotFound(name.clone()))
    }
}

#[derive(Debug)]
pub struct InstantiateArg {
    pub name: ImportName,
    pub sort: SortWithIdx,
}
