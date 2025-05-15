use crate::component_model::types::Type;
use crate::component_model::{Component, PlaceholderId};
use crate::component_model::{
    ComponentExport, ComponentImport, ExportName, Func, GlobalIdx, ImportName, Instance, LocalIdx,
    Relation, TypeId,
};
use crate::parser::component_model::ParseResult;
use std::collections::HashMap;
use typed_arena::Arena;
use union_find::UnionFind;

pub struct ValueLocalStore<T> {
    values: Vec<GlobalIdx<T>>,
}

pub struct ValueStore<T> {
    map: HashMap<GlobalIdx<T>, Relation<T>>,
}

#[derive(Default)]
pub struct Scope {
    pub components: ValueLocalStore<Component>,
    pub instances: ValueLocalStore<Instance>,
    pub funcs: ValueLocalStore<Func>,
    pub imports: HashMap<PlaceholderId, ComponentImport>,
    pub exports: HashMap<PlaceholderId, ComponentExport>,
}

impl<T> Default for ValueLocalStore<T> {
    fn default() -> Self {
        Self { values: Vec::new() }
    }
}

impl<T> Default for ValueStore<T> {
    fn default() -> Self {
        Self {
            map: HashMap::new(),
        }
    }
}

impl<T> ValueLocalStore<T> {
    pub fn register(&mut self, value: GlobalIdx<T>) {
        self.values.push(value);
    }

    pub fn get(&self, idx: LocalIdx<T>) -> ParseResult<GlobalIdx<T>> {
        Ok(self.values.get(idx.get() as usize).cloned().unwrap())
    }
}

impl<T> ValueStore<T> {
    pub fn register(&mut self, value: Relation<T>) -> GlobalIdx<T> {
        let idx = GlobalIdx::new();
        self.map.insert(idx, value);
        idx
    }

    pub fn get(&self, idx: GlobalIdx<T>) -> Option<&Relation<T>> {
        self.map.get(&idx)
    }
}

pub struct ParseState<'a> {
    arena: &'a Arena<Scope>,
    scopes: Vec<&'a mut Scope>,
    // pub core_modules: ValueStore<CoreModule>,
    pub(crate) component_store: ValueStore<Component>,
    pub(crate) instance_store: ValueStore<Instance>,
    pub(crate) func_store: ValueStore<Func>,
}

impl<'a> ParseState<'a> {
    pub fn new(arena: &'a Arena<Scope>) -> Self {
        let scope = arena.alloc(Scope::default());
        Self {
            arena,
            scopes: vec![scope],
            component_store: Default::default(),
            instance_store: Default::default(),
            func_store: Default::default(),
        }
    }

    pub fn push_scope(&mut self) {
        let scope = self.arena.alloc(Scope::default());
        self.scopes.push(scope);
    }

    pub fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    pub fn scope(&self) -> &Scope {
        self.scopes.last().unwrap()
    }

    pub fn scope_mut(&mut self) -> &mut Scope {
        self.scopes.last_mut().unwrap()
    }
}

impl Scope {
    pub fn make_component(&self) -> Component {
        Component {
            imports: self.imports.clone(),
            exports: self.exports.clone(),
        }
    }

    pub fn add_export(&mut self, name: &ExportName, export: ComponentExport) {
        self.exports.insert(PlaceholderId::new(name), export);
    }

    pub fn add_import(&mut self, name: &ImportName, import: ComponentImport) {
        self.imports.insert(PlaceholderId::new(name), import);
    }
}
