use crate::parser::idx::{RawCoreModuleIdx, RawExportIdx, RawImportIdx};
use std::collections::HashMap;

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
