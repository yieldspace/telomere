use crate::component_model::{
    CoreInstanceInlineExport, CoreModule, CoreModuleExportType, CoreModuleType, GlobalIdx, Instance,
};
use crate::parser::component_model::{ComponentParseError, ParseResult};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub enum CoreInstance {
    Real {
        module_idx: GlobalIdx<CoreModule>,
        imports: HashMap<String, GlobalIdx<Instance>>,
    },
    Alias {
        exports: HashMap<String, CoreInstanceInlineExport>,
    },
}
