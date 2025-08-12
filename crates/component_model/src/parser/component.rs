use crate::Result;
use crate::name::ExportName;
use crate::parser::canon::{RawCoreFunction, RawFunction};
use crate::parser::core::CoreInstanceDef;
use crate::parser::export::RawExport;
use crate::parser::idx::{
    RawComponentIdx, RawCoreFuncIdx, RawCoreGlobalIdx, RawCoreInstanceIdx, RawCoreMemoryIdx,
    RawCoreModuleIdx, RawCoreTableIdx, RawCoreTypeIdx, RawExportId, RawFuncIdx, RawImportId,
    RawInstanceIdx,
};
use crate::parser::import::RawImport;
use crate::parser::instance::RawInstanceDef;
use crate::parser::vec::{RawIndexVec, Relation};
use std::collections::HashMap;

#[derive(Clone)]
pub enum RawData<T> {
    Defined(T),
    Imported(RawImportId),
    ReExported(ExportName, RawInstanceIdx),
}

pub enum RawCoreData<T> {
    Defined(T),
    Imported(RawImportId),
    ReExported(String, RawCoreInstanceIdx),
    /// Only used for core modules
    ReExportedModule(ExportName, RawInstanceIdx),
}

pub struct RawComponent {
    pub imports: HashMap<RawImportId, RawImport>,
    pub exports: HashMap<RawExportId, RawExport>,
    pub ops: Vec<ComponentOp>,
    pub(crate) components: RawIndexVec<RawComponentIdx, RawData<RawComponent>>,
    pub(crate) instances: RawIndexVec<RawInstanceIdx, RawData<RawInstanceDef>>,
    pub(crate) funcs: RawIndexVec<RawFuncIdx, RawData<RawFunction>>,
    pub(crate) core_modules: RawIndexVec<RawCoreModuleIdx, RawCoreData<telomere_wasm::Module>>,
    pub(crate) core_instances: RawIndexVec<RawCoreInstanceIdx, RawCoreData<CoreInstanceDef>>,
    pub(crate) core_memories: RawIndexVec<RawCoreMemoryIdx, RawCoreData<()>>,
    pub(crate) core_globals: RawIndexVec<RawCoreGlobalIdx, RawCoreData<()>>,
    pub(crate) core_tables: RawIndexVec<RawCoreTableIdx, RawCoreData<()>>,
    pub(crate) core_types: RawIndexVec<RawCoreTypeIdx, RawCoreData<()>>,
    pub(crate) core_funcs: RawIndexVec<RawCoreFuncIdx, RawCoreData<RawCoreFunction>>,
}

pub enum ComponentOp {
    Instantiate(RawInstanceIdx),
    CoreInstantiate(RawCoreInstanceIdx),
    DefineCoreModule(RawCoreModuleIdx),
    DefineComponent(RawComponentIdx),
}

pub enum RawComponentImport {
    CoreModule(RawCoreModuleIdx),
}

pub enum RawComponentExport {
    CoreModule(RawCoreModuleIdx),
}

impl RawComponent {
    pub fn get_instance(&self, idx: &RawInstanceIdx) -> Result<&RawData<RawInstanceDef>> {
        self.instances.get(idx)
    }

    pub fn get_component(&self, idx: &RawComponentIdx) -> Result<&RawData<RawComponent>> {
        self.components.get(idx)
    }

    pub fn get_func(&self, idx: &RawFuncIdx) -> Result<&RawData<RawFunction>> {
        self.funcs.get(idx)
    }

    pub fn get_core_module(
        &self,
        idx: &RawCoreModuleIdx,
    ) -> Result<&RawCoreData<telomere_wasm::Module>> {
        self.core_modules.get(idx)
    }

    pub fn get_core_instance(
        &self,
        idx: &RawCoreInstanceIdx,
    ) -> Result<&RawCoreData<CoreInstanceDef>> {
        self.core_instances.get(idx)
    }
}
