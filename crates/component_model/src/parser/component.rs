use crate::name::ExportName;
use crate::parser::idx::{
    RawCoreInstanceIdx, RawCoreModuleIdx, RawExportIdx, RawImportIdx, RawInstanceIdx,
};
use std::collections::HashMap;

pub enum RawData<T> {
    Defined(T),
    Imported(RawImportIdx),
    ReExported(ExportName, RawInstanceIdx),
}

pub enum RawCoreData<T> {
    Defined(T),
    Imported(RawImportIdx),
    ReExported(ExportName, RawCoreInstanceIdx),
    /// Only used for core modules
    ReExportedModule(ExportName, RawInstanceIdx),
}

pub struct RawComponent {
    pub imports: HashMap<RawImportIdx, RawComponentImport>,
    pub exports: HashMap<RawExportIdx, RawComponentExport>,
}

pub enum RawComponentImport {
    CoreModule(RawCoreModuleIdx),
}

pub enum RawComponentExport {
    CoreModule(RawCoreModuleIdx),
}
