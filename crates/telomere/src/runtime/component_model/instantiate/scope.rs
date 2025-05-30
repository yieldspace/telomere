use std::collections::HashMap;
use std::rc::Rc;
use typed_arena::Arena;
use crate::common::InstanceHandle;
use crate::component_model::{Component, ComponentImport, CoreInstance, CoreModule, GlobalIdx, Instance, InstanceImport};
use crate::runtime::component_model::instantiate::{ComponentInstance, Export, Import, InstantiateOp, InstantiateResult};
use crate::runtime::component_model::{ComponentVMError, Linker};
use crate::runtime::component_model::instantiate::id::ResolveId;

pub enum InstantiateScope<'a, 'b, 'o> {
    Linker(
        &'a Linker<'a>,
        ScopeState<'b, 'a, 'o>,
        &'o [InstantiateOp],
    ),
    Instantiate(
        HashMap<String, Import>,
        ScopeState<'b, 'a, 'o>,
        &'o [InstantiateOp],
    ),
}

pub struct ScopeManager<'a, 'b, 'o> {
    arena: &'a Arena<InstantiateScope<'b, 'a, 'o>>,
    pub current: &'a InstantiateScope<'b, 'a, 'o>,
}

pub struct ScopeState<'a, 'b, 'o> {
    instantiated: ComponentInstance,
    parent: Option<&'b InstantiateScope<'a, 'b, 'o>>,
    idx: Option<GlobalIdx<Instance>>,
    instances: HashMap<GlobalIdx<Instance>, Rc<ComponentInstance>>,
    core_instances: HashMap<GlobalIdx<CoreInstance>, InstanceHandle>,
}

impl<'a, 'b, 'o> ScopeManager<'a, 'b, 'o> {
    pub fn new(
        arena: &'a Arena<InstantiateScope<'b, 'a, 'o>>,
        linker: &'b Linker,
        ops: &'o [InstantiateOp],
    ) -> Self {
        let current = arena.alloc(InstantiateScope::Linker(linker, ScopeState::new(None, None), ops));
        Self { arena, current }
    }

    pub fn push(
        &mut self,
        imports: HashMap<String, Import>,
        idx: GlobalIdx<Instance>,
        ops: &'o [InstantiateOp],
    ) -> &InstantiateScope<'b, 'a, 'o> {
        let new_scope = self.arena.alloc(InstantiateScope::Instantiate(
            imports,
            ScopeState::new(Some(self.current), Some(idx)),
            ops,
        ));
        self.current = new_scope;
        new_scope
    }

    pub fn pop(&mut self) {
        self.current = self.current.parent().unwrap();
    }
}

impl<'a, 'b, 'o> InstantiateScope<'a, 'b, 'o> {
    fn parent(&self) -> Option<&InstantiateScope<'a, 'b, 'o>> {
        match self {
            InstantiateScope::Linker(_, state, _) => state.parent,
            InstantiateScope::Instantiate(_, state, _) => state.parent,
        }
    }
    
    fn state_mut(&mut self) -> &mut ScopeState<'a, 'b, 'o> {
        match self {
            InstantiateScope::Linker(_, state, _) => state,
            InstantiateScope::Instantiate(_, state, _) => state,
        }
    }
    
    pub fn state(&self) -> &ScopeState<'a, 'b, 'o> {
        match self {
            InstantiateScope::Linker(_, state, _) => state,
            InstantiateScope::Instantiate(_, state, _) => state,
        }
    }

    pub(crate) fn ops(&self) -> &'o [InstantiateOp] {
        match self {
            InstantiateScope::Linker(_, _, ops) => ops,
            InstantiateScope::Instantiate(_, _, ops) => ops,
        }
    }
    
    pub fn idx(&self) -> Option<GlobalIdx<Instance>> {
        match self {
            InstantiateScope::Linker(_, state, _) => state.idx,
            InstantiateScope::Instantiate(_, state, _) => state.idx,
        }
    }
    
    pub fn register_import(
        &mut self,
        name: Box<String>,
        import: InstanceImport,
    ) {
        match import {
            InstanceImport::CoreModule(idx) => {
                todo!()
            }
            InstanceImport::Func(idx) => {
                todo!()
            }
            InstanceImport::Component(idx) => {
                todo!()
            }
            InstanceImport::Instance(idx) => {
                let Import::Instance(inst) = self.get_import(name.as_ref()) else {
                    panic!("Expected Instance import for {}", name);
                };
                let inst = inst.clone();
                self.state_mut().instances.insert(idx, inst);
            }
        }
    }

    pub fn register_instance(&mut self, idx: GlobalIdx<Instance>, inst: Rc<ComponentInstance>) {
        self.state_mut().instances.insert(
            idx,
            inst
        );
    }
}

impl<'a, 'b, 'o> ScopeState<'a, 'b, 'o> {
    pub fn new(
        parent: Option<&'b InstantiateScope<'a, 'b, 'o>>,
        idx: Option<GlobalIdx<Instance>>,
    ) -> Self {
        Self {
            instantiated: ComponentInstance::default(),
            parent,
            idx,
            instances: Default::default(),
            core_instances: Default::default(),
        }
    }
}
