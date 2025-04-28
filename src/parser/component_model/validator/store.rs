use crate::component_model::{
    ComponentExport, ComponentImport, ComponentType, CoreFunc, CoreFuncType, CoreGlobalRef,
    CoreInstance, CoreInstanceType, CoreMemoryRef, CoreModule, CoreModuleType, CoreTableRef,
    CoreType, Func, FuncType, GlobalIdx, InlineComponent, Instance, InstanceType, Type,
};
use crate::parser::component_model::validator::LocalIdx;
use std::collections::HashMap;

#[derive(Default)]
pub struct LocalStore {
    pub core_modules: Vec<CoreModuleType>,
    pub core_instances: Vec<CoreInstanceType>,
    pub core_funcs: Vec<CoreFuncType>,
    pub components: Vec<ComponentType>,
    pub instances: Vec<InstanceType>,
    pub core_memories: Vec<crate::common::MemType>,
    pub core_tables: Vec<crate::common::TableType>,
    pub core_globals: Vec<crate::common::GlobalType>,
    pub core_types: Vec<CoreType>,
    pub functions: Vec<FuncType>,
    pub types: Vec<Type>,
    #[cfg(feature = "component-gated-feature-value-imports-exports")]
    pub values: Vec<ValueIdx>,
    pub imports: HashMap<String, ComponentImport>,
    pub exports: HashMap<String, ComponentExport>,
}

#[derive(Default)]
pub struct GlobalStore {
    pub core_modules: HashMap<LocalIdx, GlobalIdx<CoreModule>>,
    pub core_instances: HashMap<LocalIdx, GlobalIdx<CoreInstance>>,
    pub core_funcs: HashMap<LocalIdx, GlobalIdx<CoreFunc>>,
    pub components: HashMap<LocalIdx, GlobalIdx<InlineComponent>>,
    pub instances: HashMap<LocalIdx, GlobalIdx<Instance>>,
    pub core_memories: HashMap<LocalIdx, GlobalIdx<CoreMemoryRef>>,
    pub core_tables: HashMap<LocalIdx, GlobalIdx<CoreTableRef>>,
    pub core_globals: HashMap<LocalIdx, GlobalIdx<CoreGlobalRef>>,
    pub core_types: HashMap<LocalIdx, GlobalIdx<CoreType>>,
    pub funcs: HashMap<LocalIdx, GlobalIdx<Func>>,
}
