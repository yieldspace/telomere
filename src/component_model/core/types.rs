use crate::common::ExportDesc;
use crate::component_model::{CoreExportSlot, CoreInstanceIdx, CoreSort, CoreTypeRef};
use crate::parser::component_model::ComponentParseError;
use crate::Module;

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
            Err(ComponentParseError::InvalidType("ModuleType".to_string()))
        }
    }
}

impl CoreModuleType {
    pub fn from_module(module: &Module) -> Self {
        for export in module.exs.0.iter() {
            let name = export.0.clone();
            match export.1 {
                ExportDesc::Func(_) => {}
                ExportDesc::Table(_) => {}
                ExportDesc::Mem(_) => {}
                ExportDesc::Global(_) => {}
            }
        }
        Self {}
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
