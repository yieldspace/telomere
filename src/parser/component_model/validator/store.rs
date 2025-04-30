use crate::component_model::{
    ComponentExport, ComponentImport, ComponentType, CoreFunc, CoreFuncType, CoreGlobalRef,
    CoreInstance, CoreInstanceType, CoreMemoryRef, CoreModule, CoreModuleType, CoreTableRef,
    CoreType, Func, FuncType, GlobalIdx, InlineComponent, Instance, InstanceType, Type,
};
use crate::parser::component_model::validator::LocalIdx;
use std::collections::HashMap;
use std::hash::Hash;

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
    pub core_modules: StoreHashMap<LocalIdx, GlobalIdx<CoreModule>>,
    pub core_instances: StoreHashMap<LocalIdx, GlobalIdx<CoreInstance>>,
    pub core_funcs: StoreHashMap<LocalIdx, GlobalIdx<CoreFunc>>,
    pub components: StoreHashMap<LocalIdx, GlobalIdx<InlineComponent>>,
    pub instances: StoreHashMap<LocalIdx, GlobalIdx<Instance>>,
    pub core_memories: StoreHashMap<LocalIdx, GlobalIdx<CoreMemoryRef>>,
    pub core_tables: StoreHashMap<LocalIdx, GlobalIdx<CoreTableRef>>,
    pub core_globals: StoreHashMap<LocalIdx, GlobalIdx<CoreGlobalRef>>,
    pub core_types: StoreHashMap<LocalIdx, GlobalIdx<CoreType>>,
    pub funcs: StoreHashMap<LocalIdx, GlobalIdx<Func>>,
}

pub struct StoreHashMap<K, V>
where
    K: Hash + Eq + Clone,
    V: Hash + Eq + Clone,
{
    map: HashMap<K, V>,
    rev_map: HashMap<V, K>,
}

impl<K, V> Default for StoreHashMap<K, V>
where
    K: Hash + Eq + Clone,
    V: Hash + Eq + Clone,
{
    fn default() -> Self {
        Self {
            map: HashMap::default(),
            rev_map: HashMap::default(),
        }
    }
}

impl<K, V> StoreHashMap<K, V>
where
    K: Hash + Eq + Clone,
    V: Hash + Eq + Clone,
{
    pub fn get(&self, key: &K) -> Option<&V> {
        self.map.get(key)
    }

    pub fn get_global(&self, value: &V) -> Option<&K> {
        self.rev_map.get(value)
    }

    pub fn insert(&mut self, key: K, value: V) {
        self.map.insert(key.clone(), value.clone());
        self.rev_map.insert(value, key);
    }
}
