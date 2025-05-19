use crate::component_model::{
    ComponentExport, ComponentImport, CoreFunc, CoreFuncType, CoreGlobalRef,
    CoreInstance, CoreInstanceType, CoreMemoryRef, CoreModule, CoreModuleType, CoreTableRef,
    CoreType, ExportName, Func, GlobalIdx, ImportName, InlineComponent, Instance,
};
use std::collections::HashMap;
use crate::component_model::types::{Type, TypeId};

#[derive(Default)]
pub struct LocalStore {
    pub type_count: usize,
    pub core_modules: Vec<CoreModuleType>,
    pub core_instances: Vec<CoreInstanceType>,
    pub core_funcs: Vec<CoreFuncType>,
    pub components: Vec<TypeId>,
    pub instances: Vec<TypeId>,
    pub core_memories: Vec<crate::common::MemType>,
    pub core_tables: Vec<crate::common::TableType>,
    pub core_globals: Vec<crate::common::GlobalType>,
    pub core_types: Vec<CoreType>,
    pub functions: Vec<TypeId>,
    pub type_indexes: HashMap<LocalIdx<Type>, TypeId>,
    pub imports: HashMap<ImportName, ComponentImport>,
    pub exports: HashMap<ExportName, ComponentExport>,
}

#[derive(Default)]
pub struct GlobalStore {
    pub core_modules: HashMap<TypeId, GlobalIdx<CoreModule>>,
    pub core_instances: HashMap<TypeId, GlobalIdx<CoreInstance>>,
    pub core_funcs: HashMap<TypeId, GlobalIdx<CoreFunc>>,
    pub components: HashMap<TypeId, GlobalIdx<InlineComponent>>,
    pub instances: HashMap<TypeId, GlobalIdx<Instance>>,
    pub core_memories: HashMap<TypeId, GlobalIdx<CoreMemoryRef>>,
    pub core_tables: HashMap<TypeId, GlobalIdx<CoreTableRef>>,
    pub core_globals: HashMap<TypeId, GlobalIdx<CoreGlobalRef>>,
    pub core_types: HashMap<TypeId, GlobalIdx<CoreType>>,
    pub funcs: HashMap<TypeId, GlobalIdx<Func>>,
}
