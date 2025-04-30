mod instance;
mod module;

use crate::common::ExportDesc;
use crate::component_model::{CoreInstanceInlineExportType, CoreSort, CoreTypeRef, ExternDesc};
use crate::parser::component_model::ComponentParseError;
use crate::Module;
pub use instance::*;
pub use module::*;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub enum CoreType {
    Ref(CoreTypeRef),
    ModuleType(CoreModuleType),
}
