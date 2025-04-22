use crate::component_model::{CoreExportSlot, CoreInstanceIdx, CoreSort, CoreTypeRef};
use crate::Module;
use crate::parser::component_model::ComponentParseError;

#[derive(Debug, Clone)]
pub enum CoreType {
    Ref(CoreTypeRef),
    ModuleType(CoreModuleType),
}

#[derive(Debug, Clone)]
pub struct CoreModuleType {}

impl TryFrom<CoreType> for CoreModuleType {
    type Error = ComponentParseError;

    fn try_from(value: CoreType) -> Result<Self, Self::Error> {
        if let CoreType::ModuleType(module_type) = value {
            Ok(module_type)
        } else {
            Err(ComponentParseError::InvalidType(
                "ModuleType".to_string(),
            ))
        }
    }
}

impl CoreModuleType {
    pub fn from_module(module: &Module) -> Self {
        todo!()
    }

    pub fn get_export(
        &self,
        _self_idx: CoreInstanceIdx,
        _sort: CoreSort,
        _name: String,
    ) -> Result<CoreExportSlot, ComponentParseError> {
        todo!()
    }
}
