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
        self_idx: CoreInstanceIdx,
        sort: CoreSort,
        name: String,
    ) -> Result<CoreExportSlot, ComponentParseError> {
        todo!()
    }
}
