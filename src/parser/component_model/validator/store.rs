use crate::component_model::{
    ComponentExport, ComponentIdx, ComponentImport, ComponentType, CoreFuncIdx, CoreGlobalIdx,
    CoreInstanceIdx, CoreMemoryIdx, CoreModuleIdx, CoreTableIdx, CoreTypeIdx, FuncIdx, InstanceIdx,
    TypeIdx,
};
use std::collections::HashMap;

#[derive(Default)]
pub struct LocalStore {
    pub core_modules: Vec<CoreModuleIdx>,
    pub core_instances: Vec<CoreInstanceIdx>,
    pub core_funcs: Vec<CoreFuncIdx>,
    pub components: Vec<ComponentIdx>,
    pub instances: Vec<InstanceIdx>,
    pub core_memories: Vec<CoreMemoryIdx>,
    pub core_tables: Vec<CoreTableIdx>,
    pub core_globals: Vec<CoreGlobalIdx>,
    pub core_types: Vec<CoreTypeIdx>,
    pub functions: Vec<FuncIdx>,
    pub types: Vec<TypeIdx>,
    #[cfg(feature = "component-gated-feature-value-imports-exports")]
    pub values: Vec<ValueIdx>,
    pub imports: HashMap<String, ComponentImport>,
    pub exports: HashMap<String, ComponentExport>,
}

impl LocalStore {
    pub fn make_component_type(&self) -> ComponentType {
        ComponentType {
            imports: Default::default(),
            exports: Default::default(),
            core_types: self.core_types.clone(),
            types: self.types.clone(),
            instances: self.instances.clone(),
        }
    }
}
