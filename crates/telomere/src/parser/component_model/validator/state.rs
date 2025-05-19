use crate::component_model::types::{TyRef, Type, TypeId};
use crate::component_model::GlobalIdx;
use crate::component_model::{Component, PlaceholderId};
use std::collections::HashMap;
use typed_arena::Arena;
use union_find::UnionFind;

pub struct ValueStore<T> {
    map: HashMap<GlobalIdx<T>, T>,
}

impl<T> Default for ValueStore<T> {
    fn default() -> Self {
        Self {
            map: HashMap::new(),
        }
    }
}

impl<T> ValueStore<T> {
    pub fn register(&mut self, value: T) -> GlobalIdx<T> {
        let idx = GlobalIdx::new();
        self.map.insert(idx, value);
        idx
    }

    pub fn get(&self, idx: GlobalIdx<T>) -> Option<&T> {
        self.map.get(&idx)
    }
}

#[derive(Default)]
pub struct ValidatorState {
    // pub core_modules: ValueStore<CoreModule>,
    pub components: ValueStore<Component>,
    // pub instances: ValueStore<Instance>,
}

impl ValidatorState {
    pub fn new() -> Self {
        Self::default()
    }
}
