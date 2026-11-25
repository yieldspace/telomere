use crate::decoder::{ComponentParseError, ParseResult};
use crate::ir::types::CoreType;
use crate::ir::{
    Component, CoreFunc, CoreGlobal, CoreInstance, CoreMemory, CoreModule, CoreRelation, CoreTable,
};
use crate::ir::{
    ComponentExport, ComponentImport, ExportName, Func, GlobalIdx, ImportName, Instance, LocalIdx,
    Relation, Value,
};
use std::collections::HashMap;
use typed_arena::Arena;

#[derive(Clone, Debug)]
pub struct ValueLocalStore<T> {
    values: Vec<GlobalIdx<T>>,
}

#[derive(Clone, Debug)]
pub struct ValueStore<T, R = Relation<T>> {
    map: HashMap<GlobalIdx<T>, R>,
}

#[derive(Default, Clone, Debug)]
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
    pub values: ValueLocalStore<Value>,
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
    pub(crate) core_type_store: ValueStore<CoreType, CoreRelation<CoreType>>,
    pub(crate) core_instance_store: ValueStore<CoreInstance, CoreRelation<CoreInstance>>,
    pub(crate) core_func_store: ValueStore<CoreFunc, CoreRelation<CoreFunc>>,
    pub(crate) core_memory_store: ValueStore<CoreMemory, CoreRelation<CoreMemory>>,
    pub(crate) core_global_store: ValueStore<CoreGlobal, CoreRelation<CoreGlobal>>,
    pub(crate) core_table_store: ValueStore<CoreTable, CoreRelation<CoreTable>>,
    pub(crate) value_store: ValueStore<Value>,
    /// Bundle decode session (#201): when a component is loaded from a
    /// component bundle, this holds the opened AoT artifacts the
    /// decoder substitutes for per-section core module parsing. Always
    /// `None` for ordinary compilation.
    #[cfg(feature = "aot-bundle")]
    pub(crate) core_module_substitution: Option<crate::bundle::CoreModuleSubstitution>,
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
        self.values
            .get(idx.get() as usize)
            .cloned()
            .ok_or_else(|| ComponentParseError::TypeIdxNotFound(idx.get()))
    }
}

impl<T, R> ValueStore<T, R> {
    pub fn register(&mut self, value: R) -> GlobalIdx<T> {
        let idx = GlobalIdx::new();
        self.map.insert(idx, value);
        idx
    }

    #[allow(dead_code)]
    pub fn get(&self, idx: GlobalIdx<T>) -> Option<&R> {
        self.map.get(&idx)
    }

    pub fn snapshot(&self) -> HashMap<GlobalIdx<T>, R>
    where
        R: Clone,
    {
        let mut map = self.map.clone();
        map.shrink_to_fit();
        map
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
            core_type_store: Default::default(),
            core_instance_store: Default::default(),
            core_func_store: Default::default(),
            core_memory_store: Default::default(),
            core_global_store: Default::default(),
            core_table_store: Default::default(),
            value_store: Default::default(),
            #[cfg(feature = "aot-bundle")]
            core_module_substitution: None,
        }
    }

    /// Installs a bundle substitution session (#201) for the decode
    /// that follows. Bundle loads only; ordinary compilation never
    /// calls this.
    #[cfg(feature = "aot-bundle")]
    pub(crate) fn set_core_module_substitution(
        &mut self,
        substitution: crate::bundle::CoreModuleSubstitution,
    ) {
        self.core_module_substitution = Some(substitution);
    }

    /// Removes and returns the bundle substitution session, if any.
    #[cfg(feature = "aot-bundle")]
    pub(crate) fn take_core_module_substitution(
        &mut self,
    ) -> Option<crate::bundle::CoreModuleSubstitution> {
        self.core_module_substitution.take()
    }

    /// If a bundle substitution session is active, consumes the
    /// current CoreModule section from `reader` without parsing it
    /// and returns the substituted module. Returns `Ok(None)` when no
    /// session is active (the caller parses as usual).
    #[cfg(feature = "aot-bundle")]
    pub(crate) fn substitute_core_module(
        &mut self,
        reader: &mut impl crate::support::binary::BinaryReader,
    ) -> ParseResult<Option<crate::support::Module>> {
        match self.core_module_substitution.as_mut() {
            Some(substitution) => substitution.take_for_section(reader),
            None => Ok(None),
        }
    }

    /// Records the artifact consumed for the most recent CoreModule
    /// section against the global index the decoder registered the
    /// substituted module under. No-op without a substitution session.
    #[cfg(feature = "aot-bundle")]
    pub(crate) fn bind_core_module_substitution(&mut self, idx: GlobalIdx<CoreModule>) {
        if let Some(substitution) = self.core_module_substitution.as_mut() {
            substitution.bind(idx);
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
        let index = self
            .scopes
            .len()
            .checked_sub(1 + ct as usize)
            .ok_or(ComponentParseError::InvalidScope)?;
        self.scopes
            .get(index)
            .map(|scope| &**scope)
            .ok_or(ComponentParseError::InvalidScope)
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
