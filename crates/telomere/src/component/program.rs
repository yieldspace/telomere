use crate::component::ir::types::{CoreType, Type};
use crate::component::ir::{
    Component, CoreFunc, CoreGlobal, CoreInstance, CoreMemory, CoreModule, CoreRelation, CoreTable,
    Func, GlobalIdx, Instance, Relation, TypeId,
};
use std::collections::HashMap;

#[derive(Clone, Debug)]
pub struct ComponentTypeInfo {
    pub id: u32,
}

#[derive(Clone, Debug)]
pub enum ComponentOp {
    Instantiate { component_idx: u32 },
    Alias { source_idx: u32, target_idx: u32 },
    CanonLower { func_idx: u32 },
    CanonLift { func_idx: u32 },
    Export { name: String },
}

#[derive(Clone, Debug)]
pub struct ComponentProgram {
    pub types: Vec<ComponentTypeInfo>,
    pub imports: Vec<String>,
    pub callable_imports: Vec<String>,
    pub exports: Vec<String>,
    pub callable_exports: Vec<String>,
    pub ops: Vec<ComponentOp>,
    pub bytes: Vec<u8>,
    pub root: Component,
    pub type_map: HashMap<TypeId, Type>,
    pub component_store: HashMap<GlobalIdx<Component>, Relation<Component>>,
    pub instance_store: HashMap<GlobalIdx<Instance>, Relation<Instance>>,
    pub func_store: HashMap<GlobalIdx<Func>, Relation<Func>>,
    pub core_module_store: HashMap<GlobalIdx<CoreModule>, CoreRelation<CoreModule>>,
    pub core_type_store: HashMap<GlobalIdx<CoreType>, CoreRelation<CoreType>>,
    pub core_instance_store: HashMap<GlobalIdx<CoreInstance>, CoreRelation<CoreInstance>>,
    pub core_func_store: HashMap<GlobalIdx<CoreFunc>, CoreRelation<CoreFunc>>,
    pub core_memory_store: HashMap<GlobalIdx<CoreMemory>, CoreRelation<CoreMemory>>,
    pub core_global_store: HashMap<GlobalIdx<CoreGlobal>, CoreRelation<CoreGlobal>>,
    pub core_table_store: HashMap<GlobalIdx<CoreTable>, CoreRelation<CoreTable>>,
}
