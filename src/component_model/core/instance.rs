use crate::common::ExportDesc;
use crate::component_model::{Binding, CoreBinding, CoreExportSlot, CoreFuncRef, CoreFunction, CoreGlobalRef, CoreInstanceIdx, CoreInstanceImport, CoreInstanceInlineExport, CoreMemoryRef, CoreModule, CoreModuleIdx, CoreModuleType, CoreReference, CoreSort, CoreTableRef, GlobalIdx, Idx, Slot};
use crate::parser::component_model::{ComponentParseError, ParseResult, Validator};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub enum CoreInstance {
    Real {
        module_idx: GlobalIdx<CoreModule>,
        imports: HashMap<String, CoreInstanceImport>,
    },
    Alias {
        exports: HashMap<String, CoreInstanceInlineExport>,
    },
}

#[derive(Debug, Clone)]
pub struct CoreInstanceType {
    
}

pub enum CoreInstanceExportType {
    Memory(String, crate::common::MemType),
    Table(String, crate::common::TableType),
    Func(String, crate::common::FuncType),
    Global(String, crate::common::GlobalType),
}

impl CoreInstanceType {
    pub fn new() -> Self {
        CoreInstanceType {}
    }
    
    pub fn get_export_type(&self, sort: &CoreSort, name: &String) -> ParseResult<CoreInstanceExportType> {
        todo!()
    }
}

impl From<&CoreInstance> for CoreInstanceType {
    fn from(value: &CoreInstance) -> Self {
        todo!()
    }
}