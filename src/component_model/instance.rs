use crate::component_model::{GlobalIdx, InlineComponent, InstanceType, SortWithIdx};
use crate::parser::component_model::ComponentParseError;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct Instance {
    pub(crate) component_idx: Option<GlobalIdx<InlineComponent>>,
    pub(crate) args: HashMap<String, SortWithIdx>,
    pub(crate) exports: HashMap<String, SortWithIdx>,
}

impl Instance {
    pub fn as_type(&self) -> InstanceType {
        InstanceType {
            core_types: vec![],
            types: vec![],
            instances: vec![],
            exports: Default::default(),
        }
    }
}

impl Instance {
    pub fn get_export(&self, name: &String) -> Result<Option<SortWithIdx>, ComponentParseError> {
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
