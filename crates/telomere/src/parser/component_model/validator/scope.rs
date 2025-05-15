use crate::component_model::types::Type;
use crate::component_model::{Component, Func, GlobalIdx, Instance, LocalIdx, TypeId};
use crate::parser::component_model::ParseResult;
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

#[derive(Default)]
pub struct ScopeGuard {
    // Types
    pub type_indexes: TypeStore<Type>,
    pub component_indexes: TypeStore<Component>,
    pub instance_indexes: TypeStore<Instance>,
    pub func_indexes: TypeStore<Func>,
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
}
