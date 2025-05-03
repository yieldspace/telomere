use crate::component_model::{
    ExportName, ExternDesc, ImportDecl, InstanceDecl,
};
use crate::parser::component_model::ComponentParseError;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub struct ComponentType {
    pub(crate) imports: HashMap<String, ExternDesc>,
    pub(crate) exports: HashMap<ExportName, ExternDesc>,
}

impl Default for ComponentType {
    fn default() -> Self {
        Self::new()
    }
}

impl ComponentType {
    pub fn new() -> Self {
        Self {
            imports: HashMap::new(),
            exports: HashMap::new(),
        }
    }
}

impl TryFrom<ExternDesc> for ComponentType {
    type Error = ComponentParseError;

    fn try_from(value: ExternDesc) -> Result<Self, Self::Error> {
        if let ExternDesc::Component(component_type) = value {
            Ok(component_type)
        } else {
            Err(ComponentParseError::InvalidType(
                "ComponentType".to_string(),
            ))
        }
    }
}

#[derive(Debug, Clone)]
pub enum ComponentDecl {
    Import(ImportDecl),
    Instance(InstanceDecl),
}
