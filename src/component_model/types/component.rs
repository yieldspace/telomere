use crate::component_model::{
    CoreModuleType, CoreType, ExternDesc, FuncType, GlobalIdx, ImportDecl, Instance, InstanceDecl,
    InstanceType, Type,
};
use crate::parser::component_model::ComponentParseError;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub struct ComponentType {
    pub(crate) imports: HashMap<String, ExternDesc>,
    pub(crate) exports: HashMap<String, ExternDesc>,
    pub(crate) core_types: Vec<GlobalIdx<CoreType>>,
    pub(crate) types: Vec<Type>,
    pub(crate) instances: Vec<GlobalIdx<Instance>>,
}

impl ComponentType {
    pub fn new() -> Self {
        Self {
            imports: HashMap::new(),
            exports: HashMap::new(),
            core_types: Vec::new(),
            types: Vec::new(),
            instances: Vec::new(),
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
