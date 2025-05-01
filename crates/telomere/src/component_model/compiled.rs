use crate::component_model::{
    CoreFunc, CoreGlobalRef, CoreInstance, CoreMemoryRef, CoreModule, CoreTableRef, Func,
    GlobalIdx, InlineComponent, Instance,
};
use std::collections::HashMap;

pub enum Relation<T> {
    Defined(T),
    Alias(GlobalIdx<T>),
    Import(String),
    FromCoreExport(GlobalIdx<CoreInstance>, String),
    FromExport(GlobalIdx<Instance>, String),
}

impl<T> Relation<T> {
    pub fn new(data: T) -> Self {
        Relation::Defined(data)
    }

    pub fn alias(idx: GlobalIdx<T>) -> Self {
        Relation::Alias(idx)
    }

    pub fn import(name: String) -> Self {
        Relation::Import(name)
    }

    pub fn from_core_export(idx: GlobalIdx<CoreInstance>, name: String) -> Self {
        Relation::FromCoreExport(idx, name)
    }

    pub fn from_export(idx: GlobalIdx<Instance>, name: String) -> Self {
        Relation::FromExport(idx, name)
    }
}

#[derive(Default)]
pub struct CompiledState {
    core_modules: HashMap<GlobalIdx<CoreModule>, Relation<CoreModule>>,
    core_instances: HashMap<GlobalIdx<CoreInstance>, Relation<CoreInstance>>,
    core_funcs: HashMap<GlobalIdx<CoreFunc>, Relation<CoreFunc>>,
    core_memories: HashMap<GlobalIdx<CoreMemoryRef>, CoreMemoryRef>,
    core_tables: HashMap<GlobalIdx<CoreTableRef>, CoreTableRef>,
    core_globals: HashMap<GlobalIdx<CoreGlobalRef>, CoreGlobalRef>,
    components: HashMap<GlobalIdx<InlineComponent>, Relation<InlineComponent>>,
    instances: HashMap<GlobalIdx<Instance>, Relation<Instance>>,
    funcs: HashMap<GlobalIdx<Func>, Relation<Func>>,
}

impl CompiledState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register_core_module(&mut self, idx: GlobalIdx<CoreModule>, data: Relation<CoreModule>) {
        self.core_modules.insert(idx, data);
    }

    pub fn register_core_instance(
        &mut self,
        idx: GlobalIdx<CoreInstance>,
        data: Relation<CoreInstance>,
    ) {
        self.core_instances.insert(idx, data);
    }

    pub fn register_core_func(&mut self, idx: GlobalIdx<CoreFunc>, data: Relation<CoreFunc>) {
        self.core_funcs.insert(idx, data);
    }

    pub fn register_core_memory(&mut self, idx: GlobalIdx<CoreMemoryRef>, data: CoreMemoryRef) {
        self.core_memories.insert(idx, data);
    }

    pub fn register_core_table(&mut self, idx: GlobalIdx<CoreTableRef>, data: CoreTableRef) {
        self.core_tables.insert(idx, data);
    }

    pub fn register_core_global(&mut self, idx: GlobalIdx<CoreGlobalRef>, data: CoreGlobalRef) {
        self.core_globals.insert(idx, data);
    }

    pub fn register_component(
        &mut self,
        idx: GlobalIdx<InlineComponent>,
        data: Relation<InlineComponent>,
    ) {
        self.components.insert(idx, data);
    }

    pub fn register_instance(&mut self, idx: GlobalIdx<Instance>, data: Relation<Instance>) {
        self.instances.insert(idx, data);
    }

    pub fn register_func(&mut self, idx: GlobalIdx<Func>, data: Relation<Func>) {
        self.funcs.insert(idx, data);
    }
}
