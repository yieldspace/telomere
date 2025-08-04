use crate::parser::idx::{
    RawCoreInstanceIdx, RawCoreModuleIdx, RawExportIdx, RawImportIdx, RawInstanceIdx,
};
use std::collections::HashMap;

pub enum RawData<T> {
    Defined(T),
    Imported(RawImportIdx),
    ReExported(RawInstanceIdx),
}

pub enum RawCoreData<T> {
    Defined(T),
    Imported(RawImportIdx),
    ReExported(RawCoreInstanceIdx),
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
