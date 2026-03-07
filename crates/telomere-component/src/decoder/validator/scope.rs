use crate::decoder::ParseResult;
use crate::ir::types::{
    ComponentImportType, CoreFuncType, CoreGlobalType, CoreInstanceType, CoreMemoryType,
    CoreModuleType, CoreTableType, CoreType, GenericsReplaceDSL, Type,
};
use crate::ir::{
    Component, CoreFunc, CoreGlobal, CoreInstance, CoreMemory, CoreModule, CoreTable, ExportName,
    Func, ImportName, Instance, LocalIdx, ScopeId, TypeId,
};
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

impl<T> Clone for TypeStore<T> {
    fn clone(&self) -> Self {
        Self {
            types: self.types.clone(),
            _phantom: PhantomData,
        }
    }
}

impl<R, T: Clone> Clone for CoreTypeStore<R, T> {
    fn clone(&self) -> Self {
        Self {
            types: self.types.clone(),
            _phantom: PhantomData,
        }
    }
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
        self.types
            .get(idx.get() as usize)
            .ok_or_else(|| crate::decoder::ComponentParseError::TypeIdxNotFound(idx.get()))
    }
}
#[derive(Debug)]
pub enum ExportInfo {
    CoreModule(CoreModuleType),
    Component(TypeId),
    Instance(TypeId),
    Func(TypeId),
    TypeEq(TypeId),
    TypeSub(TypeId),
}

pub struct ScopeGuard {
    pub scope_id: ScopeId,
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
    pub imports: HashMap<String, ComponentImportType>,
    pub exports: HashMap<String, ExportInfo>,
    pub generics_replace_program: Vec<GenericsReplaceDSL>,
    pub export_names: Vec<ExportName>,
    pub import_names: Vec<ImportName>,
}

impl<T> TypeStore<T> {
    pub fn add(&mut self, type_id: TypeId) -> LocalIdx<T> {
        let idx = self.types.len() as u32;
        self.types.push(type_id);
        LocalIdx::new(idx)
    }

    pub fn get(&self, idx: LocalIdx<T>) -> ParseResult<TypeId> {
        self.types
            .get(idx.get() as usize)
            .copied()
            .ok_or_else(|| crate::decoder::ComponentParseError::TypeIdxNotFound(idx.get()))
    }
}

impl ScopeGuard {
    pub fn new(scope_id: ScopeId) -> Self {
        Self {
            scope_id,
            type_indexes: Default::default(),
            component_indexes: Default::default(),
            instance_indexes: Default::default(),
            func_indexes: Default::default(),
            core_types: Default::default(),
            core_modules: Default::default(),
            core_instances: Default::default(),
            core_memories: Default::default(),
            core_tables: Default::default(),
            core_globals: Default::default(),
            core_funcs: Default::default(),
            imports: Default::default(),
            exports: Default::default(),
            generics_replace_program: Default::default(),
            export_names: Default::default(),
            import_names: Default::default(),
        }
    }

    pub fn inherit_type_scope_from(&mut self, parent: &Self) {
        self.type_indexes = parent.type_indexes.clone();
        self.component_indexes = parent.component_indexes.clone();
        self.instance_indexes = parent.instance_indexes.clone();
        self.func_indexes = parent.func_indexes.clone();
        self.core_types = parent.core_types.clone();
        self.core_modules = parent.core_modules.clone();
        self.core_instances = parent.core_instances.clone();
        self.core_memories = parent.core_memories.clone();
        self.core_tables = parent.core_tables.clone();
        self.core_globals = parent.core_globals.clone();
        self.core_funcs = parent.core_funcs.clone();
    }
}
