use crate::common::{ExportDesc, ImportDesc};
use crate::component_model::types::{
    ComponentExportType, ComponentType, CoreFuncType, CoreGlobalType, CoreInstanceType,
    CoreMemoryType, CoreModuleType, CoreTableType, CoreType, Generic, Type,
};
use crate::component_model::{
    Component, CoreFunc, CoreGlobal, CoreInstance, CoreMemory, CoreModule, CoreTable, Func,
    GlobalIdx, Instance, LocalIdx, ParsedExportName, ParsedImportName, TypeId,
};
use crate::parser::component_model::ParseResult;
use std::collections::HashMap;
use std::marker::PhantomData;

pub struct TypeStore<T> {
    types: Vec<TypeId>,
    _phantom: PhantomData<T>,
}

impl<T> Default for TypeStore<T> {
    fn default() -> Self {
        Self {
            types: Vec::new(),
            _phantom: PhantomData,
        }
    }
}

pub struct CoreTypeStore<R, T> {
    types: Vec<T>,
    _phantom: PhantomData<R>,
}

impl<R, T> Default for CoreTypeStore<R, T> {
    fn default() -> Self {
        Self {
            types: Vec::new(),
            _phantom: PhantomData,
        }
    }
}

impl<R, T> CoreTypeStore<R, T> {
    pub fn add(&mut self, ty: T) -> LocalIdx<R> {
        let idx = self.types.len() as u32;
        self.types.push(ty);
        LocalIdx::new(idx)
    }

    pub fn get(&self, idx: LocalIdx<R>) -> ParseResult<&T> {
        Ok(self.types.get(idx.get() as usize).unwrap())
    }
}

#[derive(Default)]
pub struct ScopeGuard {
    // Types
    pub type_indexes: TypeStore<Type>,
    pub component_indexes: TypeStore<Component>,
    pub instance_indexes: TypeStore<Instance>,
    pub func_indexes: TypeStore<Func>,
    pub core_types: CoreTypeStore<CoreType, CoreType>,
    pub core_modules: CoreTypeStore<CoreModule, CoreModuleType>,
    pub core_instances: CoreTypeStore<CoreInstance, CoreInstanceType>,
    pub core_memories: CoreTypeStore<CoreMemory, CoreMemoryType>,
    pub core_tables: CoreTypeStore<CoreTable, CoreTableType>,
    pub core_globals: CoreTypeStore<CoreGlobal, CoreGlobalType>,
    pub core_funcs: CoreTypeStore<CoreFunc, CoreFuncType>,
    pub imports: HashMap<String, Generic>,
    pub exports: HashMap<String, ComponentExportType>,
    pub export_names: Vec<ParsedExportName>,
    pub import_names: Vec<ParsedImportName>,
}

impl<T> TypeStore<T> {
    pub fn add(&mut self, type_id: TypeId) -> LocalIdx<T> {
        let idx = self.types.len() as u32;
        self.types.push(type_id);
        LocalIdx::new(idx)
    }

    pub fn get(&self, idx: LocalIdx<T>) -> ParseResult<TypeId> {
        Ok(self.types.get(idx.get() as usize).cloned().unwrap())
    }
}

impl ScopeGuard {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn make_component(&self) -> ComponentType {
        ComponentType {
            imports: self.imports.clone(),
            exports: self.exports.clone(),
        } // TODO:
    }
}
