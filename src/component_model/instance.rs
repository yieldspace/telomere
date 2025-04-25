use crate::component_model::{
    ComponentIdx,
    InstanceExportType, InstanceType,
    SortWithIdx,
};
use crate::parser::component_model::ComponentParseError;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct Instance {
    pub(crate) value: Option<InstanceValue>,
    pub(crate) ty: InstanceType,
}

#[derive(Debug, Clone)]
pub struct InstanceValue {
    pub(crate) component_idx: Option<ComponentIdx>,
    pub(crate) args: HashMap<String, SortWithIdx>,
    pub(crate) exports: HashMap<String, SortWithIdx>,
}
impl InstanceValue {
    pub fn get_type(&self) -> InstanceType {
        InstanceType {
            core_types: vec![],
            types: vec![],
            instances: vec![],
            exports: Default::default(),
        }
    }
}

impl Instance {
    pub fn new(value: Option<InstanceValue>, ty: InstanceType) -> Self {
        Self { value, ty }
    }
    pub fn get_export(&self, name: &String) -> Result<Option<SortWithIdx>, ComponentParseError> {
        match self.value {
            Some(ref value) => value
                .exports
                .get(name)
                .cloned()
                .map(|x| Some(x))
                .ok_or_else(|| ComponentParseError::ExportNotFound(name.clone())),
            _ => Ok(None),
        }
    }

    pub fn get_export_type(
        &self,
        name: &String,
    ) -> Result<InstanceExportType, ComponentParseError> {
        todo!()
    }
}

#[derive(Debug)]
pub struct InstantiateArg {
    pub name: String,
    pub sort: SortWithIdx,
}
