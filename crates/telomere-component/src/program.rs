use crate::ir::types::{CoreType, Type};
use crate::ir::{
    Component, ComponentExport, CoreFunc, CoreGlobal, CoreInstance, CoreMemory, CoreModule,
    CoreRelation, CoreTable, Func, GlobalIdx, Instance, Relation, TypeId,
};
use std::collections::HashMap;

#[derive(Clone, Debug)]
pub struct ComponentTypeInfo {
    pub id: u32,
    pub flat_len: usize,
    pub indirect_size: u32,
    pub indirect_align: u32,
    pub fixed_length: Option<u32>,
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
    pub type_infos: Vec<ComponentTypeInfo>,
    pub imports: Vec<String>,
    pub callable_imports: Vec<String>,
    pub exports: Vec<String>,
    pub callable_exports: Vec<String>,
    pub ops: Vec<ComponentOp>,
    pub bytes: Vec<u8>,
    pub root: Component,
    pub types: Box<[Type]>,
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

impl ComponentProgram {
    pub fn get_type(&self, id: TypeId) -> Option<&Type> {
        self.types.get(id.index() as usize)
    }

    pub fn get_type_info(&self, id: TypeId) -> Option<&ComponentTypeInfo> {
        self.type_infos.get(id.index() as usize)
    }

    pub fn get_root_func_type_id(&self, name: &str) -> Option<TypeId> {
        match self.root.exports.get(name) {
            Some(ComponentExport::Func { type_id, .. }) => Some(*type_id),
            _ => None,
        }
    }

    pub fn get_root_func(&self, name: &str) -> Option<(GlobalIdx<Func>, TypeId)> {
        match self.root.exports.get(name) {
            Some(ComponentExport::Func { idx, type_id }) => Some((*idx, *type_id)),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{Component, ResourceId};

    #[test]
    fn get_type_reads_dense_type_storage_by_index() {
        let first = Type::Resource(ResourceId::synthetic());
        let second = Type::Resource(ResourceId::synthetic());
        let program = ComponentProgram {
            type_infos: vec![
                ComponentTypeInfo {
                    id: 0,
                    flat_len: 0,
                    indirect_size: 0,
                    indirect_align: 1,
                    fixed_length: None,
                },
                ComponentTypeInfo {
                    id: 1,
                    flat_len: 0,
                    indirect_size: 0,
                    indirect_align: 1,
                    fixed_length: None,
                },
            ],
            imports: Vec::new(),
            callable_imports: Vec::new(),
            exports: Vec::new(),
            callable_exports: Vec::new(),
            ops: Vec::new(),
            bytes: Vec::new(),
            root: Component {
                imports: HashMap::new(),
                exports: HashMap::new(),
            },
            types: vec![first.clone(), second.clone()].into_boxed_slice(),
            component_store: HashMap::new(),
            instance_store: HashMap::new(),
            func_store: HashMap::new(),
            core_module_store: HashMap::new(),
            core_type_store: HashMap::new(),
            core_instance_store: HashMap::new(),
            core_func_store: HashMap::new(),
            core_memory_store: HashMap::new(),
            core_global_store: HashMap::new(),
            core_table_store: HashMap::new(),
        };

        assert!(matches!(
            program.get_type(TypeId::from_index(0)),
            Some(Type::Resource(_))
        ));
        assert_eq!(program.get_type_info(TypeId::from_index(0)).unwrap().id, 0);
        assert!(matches!(
            program.get_type(TypeId::from_index(1)),
            Some(Type::Resource(_))
        ));
        assert!(program.get_type(TypeId::from_index(2)).is_none());
    }
}
