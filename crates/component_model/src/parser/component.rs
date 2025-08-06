use crate::name::ExportName;
use crate::parser::idx::{
    RawCoreInstanceIdx, RawCoreModuleIdx, RawExportId, RawImportId, RawInstanceIdx,
};
use std::collections::HashMap;

pub enum RawData<T> {
    Defined(T),
    Imported(RawImportId),
    ReExported(ExportName, RawInstanceIdx),
}

pub enum RawCoreData<T> {
    Defined(T),
    Imported(RawImportId),
    ReExported(ExportName, RawCoreInstanceIdx),
    /// Only used for core modules
    ReExportedModule(ExportName, RawInstanceIdx),
}

pub struct RawComponent {
    pub imports: HashMap<RawImportId, RawComponentImport>,
    pub exports: HashMap<RawExportId, RawComponentExport>,
}

pub enum RawComponentImport {
    CoreModule(RawCoreModuleIdx),
}

pub enum RawComponentExport {
    CoreModule(RawCoreModuleIdx),
}
