use crate::component_model::{
    ExternDesc, GlobalIdx, InlineComponent, InstanceType, SortWithIdx,
};
use crate::parser::component_model::ComponentParseError;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct Instance {
    pub(crate) component_idx: Option<GlobalIdx<InlineComponent>>,
    pub(crate) imports: HashMap<String, ExternDesc>,
    pub(crate) exports: HashMap<String, ExternDesc>,
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
    pub fn get_export(&self, name: &String) -> Result<Option<ExternDesc>, ComponentParseError> {
        self.exports
            .get(name)
            .cloned()
            .map(|x| Some(x))
            .ok_or_else(|| ComponentParseError::ExportNotFound(name.clone()))
    }
}

#[derive(Debug)]
pub struct InstantiateArg {
    pub name: String,
    pub sort: SortWithIdx,
}
