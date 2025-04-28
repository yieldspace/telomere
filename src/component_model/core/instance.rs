use crate::common::ExportDesc;
use crate::component_model::{Binding, CoreBinding, CoreExportSlot, CoreFuncRef, CoreFunction, CoreGlobalRef, CoreInstanceIdx, CoreInstanceImport, CoreInstanceInlineExport, CoreMemoryRef, CoreModule, CoreModuleIdx, CoreModuleType, CoreReference, CoreSort, CoreTableRef, Idx, Resolvable, Resolver, Slot};
use crate::parser::component_model::{ComponentParseError, ParseResult, Validator};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct CoreInstance {
    pub value: Option<CoreInstanceValue>,
    pub ty: CoreInstanceType,
}

#[derive(Debug, Clone)]
pub enum CoreInstanceValue {
    Real {
        module_idx: CoreModuleIdx,
        imports: HashMap<String, CoreInstanceImport>,
    },
    Alias {
        exports: HashMap<String, CoreInstanceInlineExport>,
    },
}

impl CoreInstance {
    pub fn new(value: Option<CoreInstanceValue>, ty: CoreInstanceType) -> Self {
        CoreInstance { value, ty }
    }
}

#[derive(Debug, Clone)]
pub struct CoreInstanceType {
    
}

pub enum CoreInstanceExportType {
    Memory(String),
    Table(String),
    Func(String),
    Global(String),
}

impl CoreInstanceType {
    pub fn new() -> Self {
        CoreInstanceType {}
    }
    
    pub fn get_export_type(&self, sort: &CoreSort, name: &String) -> ParseResult<CoreInstanceExportType> {
        todo!()
    }
}
