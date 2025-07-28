use crate::component_model::{
    Component, ComponentImport, CoreInstance, CoreModule, GlobalIdx, Instance, InstanceExport,
    InstanceImport,
};
use crate::runtime::component_model::instantiate::id::ResolveId;
use crate::runtime::component_model::instantiate::{
    ComponentInstance, Export, Import, InstantiateOp, InstantiateResult,
};
use crate::runtime::component_model::{ComponentVMError, Linker};
use std::collections::HashMap;
use std::rc::Rc;
use telomere_wasm::common::InstanceHandle;
use typed_arena::Arena;

pub enum InstantiateScope<'a> {
    Linker(&'a Linker<'a>, ScopeState),
    Instantiate(HashMap<String, Import>, ScopeState),
}

pub struct ScopeManager<'a> {
    pub scopes: Vec<InstantiateScope<'a>>,
}

pub struct ScopeState {
    idx: Option<GlobalIdx<Instance>>,
    exports: HashMap<String, InstanceExport>,
    instances: HashMap<GlobalIdx<Instance>, ComponentInstance>,
    core_instances: HashMap<GlobalIdx<CoreInstance>, InstanceHandle>,
    core_modules: HashMap<GlobalIdx<CoreModule>, GlobalIdx<CoreModule>>,
}

impl<'a> ScopeManager<'a> {
    pub fn new(linker: &'a Linker) -> Self {
        Self {
            scopes: vec![InstantiateScope::Linker(linker, ScopeState::new(None))],
        }
    }

    pub fn scope(&self) -> &InstantiateScope<'a> {
        self.scopes.last().unwrap()
    }

    pub fn scope_mut(&mut self) -> &mut InstantiateScope<'a> {
        self.scopes.last_mut().unwrap()
    }

    pub fn push(&mut self, idx: GlobalIdx<Instance>, imports: HashMap<String, Import>) {
        self.scopes.push(InstantiateScope::Instantiate(
            imports,
            ScopeState::new(Some(idx)),
        ));
    }

    pub fn pop(&mut self) -> InstantiateScope {
        self.scopes.pop().unwrap()
    }
}

impl<'a> InstantiateScope<'a> {
    pub fn state_mut(&mut self) -> &mut ScopeState {
        match self {
            InstantiateScope::Linker(_, state) => state,
            InstantiateScope::Instantiate(_, state) => state,
        }
    }

    pub fn state(&self) -> &ScopeState {
        match self {
            InstantiateScope::Linker(_, state) => state,
            InstantiateScope::Instantiate(_, state) => state,
        }
    }

    pub fn idx(&self) -> Option<GlobalIdx<Instance>> {
        match self {
            InstantiateScope::Linker(_, state) => state.idx,
            InstantiateScope::Instantiate(_, state) => state.idx,
        }
    }

    pub fn register_export(&mut self, name: String, export: InstanceExport) {
        self.state_mut().exports.insert(name, export);
    }

    pub fn register_instance(&mut self, idx: GlobalIdx<Instance>, inst: ComponentInstance) {
        self.state_mut().instances.insert(idx, inst);
    }

    pub fn register_core_instance(&mut self, idx: GlobalIdx<CoreInstance>, inst: InstanceHandle) {
        self.state_mut().core_instances.insert(idx, inst);
    }

    pub fn register_core_module(
        &mut self,
        idx: GlobalIdx<CoreModule>,
        target: GlobalIdx<CoreModule>,
    ) {
        self.state_mut().core_modules.insert(idx, target);
    }

    pub fn get_core_instance_instantiated(&self, idx: &GlobalIdx<CoreInstance>) -> &InstanceHandle {
        self.state().core_instances.get(idx).unwrap()
    }

    pub fn get_instance_instantiated(&self, idx: &GlobalIdx<Instance>) -> &ComponentInstance {
        self.state().instances.get(idx).unwrap()
    }

    pub fn make(self) -> ComponentInstance {
        let ScopeState {
            idx,
            exports,
            instances,
            core_instances,
            core_modules,
        } = match self {
            InstantiateScope::Linker(_, state) => state,
            InstantiateScope::Instantiate(_, state) => state,
        };
        ComponentInstance {
            exports,
            core_modules,
            instances,
            core_instances,
        }
    }
}

impl ScopeState {
    pub fn new(idx: Option<GlobalIdx<Instance>>) -> Self {
        Self {
            idx,
            exports: HashMap::new(),
            instances: Default::default(),
            core_instances: Default::default(),
            core_modules: Default::default(),
        }
    }
}
