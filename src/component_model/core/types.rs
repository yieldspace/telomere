use crate::component_model::{CoreExportSlot, CoreInstanceIdx, CoreSort, CoreTypeRef};
use crate::parser::component_model::ComponentParseError;

#[derive(Debug)]
pub enum CoreType {
    Ref(CoreTypeRef),
    ModuleType(CoreModuleType),
}

#[derive(Debug, Clone)]
pub struct CoreModuleType {}

impl CoreModuleType {
    pub fn get_export(
        &self,
        _self_idx: CoreInstanceIdx,
        _sort: CoreSort,
        _name: String,
    ) -> Result<CoreExportSlot, ComponentParseError> {
        todo!()
    }
}
