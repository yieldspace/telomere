use crate::component_model::types::{CoreType, Type};
use crate::component_model::{
    Component, CoreFunc, CoreGlobal, CoreInstance, CoreMemory, CoreModule, CoreRelation, CoreTable,
};
use crate::component_model::{
    ComponentExport, ComponentImport, ExportName, Func, GlobalIdx, ImportName, Instance, LocalIdx,
    Relation, TypeId,
};
use crate::parser::component_model::{ComponentParseError, ParseResult};
use std::collections::HashMap;
use typed_arena::Arena;
use union_find::UnionFind;

pub struct ValueLocalStore<T> {
    values: Vec<GlobalIdx<T>>,
}

pub struct ValueStore<T, R = Relation<T>> {
    map: HashMap<GlobalIdx<T>, R>,
}

#[derive(Default)]
pub struct Scope {
    pub components: ValueLocalStore<Component>,
    pub instances: ValueLocalStore<Instance>,
    pub funcs: ValueLocalStore<Func>,
    pub core_modules: ValueLocalStore<CoreModule>,
    pub core_instances: ValueLocalStore<CoreInstance>,
    pub core_funcs: ValueLocalStore<CoreFunc>,
    pub core_memories: ValueLocalStore<CoreMemory>,
    pub core_globals: ValueLocalStore<CoreGlobal>,
    pub core_tables: ValueLocalStore<CoreTable>,
    pub core_types: ValueLocalStore<CoreType>,
    pub imports: HashMap<String, ComponentImport>,
    pub exports: HashMap<String, ComponentExport>,
}

pub struct ParseState<'a> {
    arena: &'a Arena<Scope>,
    scopes: Vec<&'a mut Scope>,
    // pub core_modules: ValueStore<CoreModule>,
    pub(crate) component_store: ValueStore<Component>,
    pub(crate) instance_store: ValueStore<Instance>,
    pub(crate) func_store: ValueStore<Func>,
    pub(crate) core_module_store: ValueStore<CoreModule, CoreRelation<CoreModule>>,
    pub(crate) core_instance_store: ValueStore<CoreInstance, CoreRelation<CoreInstance>>,
    pub(crate) core_func_store: ValueStore<CoreFunc, CoreRelation<CoreFunc>>,
    pub(crate) core_memory_store: ValueStore<CoreMemory, CoreRelation<CoreMemory>>,
    pub(crate) core_global_store: ValueStore<CoreGlobal, CoreRelation<CoreGlobal>>,
    pub(crate) core_table_store: ValueStore<CoreTable, CoreRelation<CoreTable>>,
}

impl<T> Default for ValueLocalStore<T> {
    fn default() -> Self {
        Self { values: Vec::new() }
    }
}

impl<T, R> Default for ValueStore<T, R> {
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

impl<T, R> ValueStore<T, R> {
    pub fn register(&mut self, value: R) -> GlobalIdx<T> {
        let idx = GlobalIdx::new();
        self.map.insert(idx, value);
        idx
    }

    pub fn get(&self, idx: GlobalIdx<T>) -> Option<&R> {
        self.map.get(&idx)
    }
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
            core_module_store: Default::default(),
            core_instance_store: Default::default(),
            core_func_store: Default::default(),
            core_memory_store: Default::default(),
            core_global_store: Default::default(),
            core_table_store: Default::default(),
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

    pub fn outer_scope(&self, ct: u32) -> ParseResult<&Scope> {
        let length = self.scopes.len();
        let k = self
            .scopes
            .get(length - 1 - ct as usize)
            .ok_or(ComponentParseError::InvalidScope)?;
        Ok(k)
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
        self.exports.insert(name.original.clone(), export);
    }

    pub fn add_import(&mut self, name: &ImportName, import: ComponentImport) {
        self.imports.insert(name.original.clone(), import);
    }
}
