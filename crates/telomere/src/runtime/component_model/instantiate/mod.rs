use crate::common::InstanceHandle;
use crate::component_model::{
    Component, CoreInstance, CoreModule, GlobalIdx, Instance, InstanceImport,
};
use crate::parser::component_model::ParsedComponent;
pub use crate::runtime::component_model::instantiate::context::InstantiateContext;
use crate::runtime::component_model::{ComponentModelInstance, ComponentVMError, Linker};
use crate::runtime::instantiate as core_instantiate;
use crate::{Registry, Store};
pub use state::InstantiateState;
use std::collections::HashMap;
use typed_arena::Arena;

mod context;
mod state;

pub type InstantiateResult<T> = Result<T, ComponentVMError>;

#[derive(Debug, Clone)]
pub enum InstantiateOp {
    CoreInstantiate(GlobalIdx<CoreInstance>),
    CoreInstanceInlineExport(GlobalIdx<CoreInstance>),
    Instantiate(GlobalIdx<Instance>),
    InstantiateEnd,
    InstantiateInlineExport(GlobalIdx<Instance>),
}

pub enum InstantiateScope<'a, 'b, 'o> {
    Linker(
        &'a Linker,
        Option<&'b InstantiateScope<'a, 'b, 'o>>,
        &'o [InstantiateOp],
    ),
    Instantiate(
        HashMap<String, InstanceImport>,
        Option<&'b InstantiateScope<'a, 'b, 'o>>,
        &'o [InstantiateOp],
    ),
}

pub struct ScopeManager<'a, 'b, 'o> {
    arena: &'a Arena<InstantiateScope<'b, 'a, 'o>>,
    pub current: &'a InstantiateScope<'b, 'a, 'o>,
}

impl<'a, 'b, 'o> ScopeManager<'a, 'b, 'o> {
    pub fn new(
        arena: &'a Arena<InstantiateScope<'b, 'a, 'o>>,
        linker: &'b Linker,
        ops: &'o [InstantiateOp],
    ) -> Self {
        let current = arena.alloc(InstantiateScope::Linker(linker, None, ops));
        Self { arena, current }
    }

    pub fn push(
        &mut self,
        imports: HashMap<String, InstanceImport>,
        ops: &'o [InstantiateOp],
    ) -> &InstantiateScope<'b, 'a, 'o> {
        let new_scope = self.arena.alloc(InstantiateScope::Instantiate(
            imports,
            Some(self.current),
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
            InstantiateScope::Linker(_, parent, _) => *parent,
            InstantiateScope::Instantiate(_, parent, _) => *parent,
        }
    }

    fn ops(&self) -> &'o [InstantiateOp] {
        match self {
            InstantiateScope::Linker(_, _, ops) => ops,
            InstantiateScope::Instantiate(_, _, ops) => ops,
        }
    }

    pub fn get_core_module(&self, name: &String) -> InstantiateResult<GlobalIdx<CoreModule>> {
        match self {
            InstantiateScope::Linker(_, _, _) => todo!(),
            InstantiateScope::Instantiate(imports, _, _) => {
                if let Some(InstanceImport::CoreModule(module_idx)) = imports.get(name) {
                    Ok(*module_idx)
                } else {
                    Err(ComponentVMError::LinkError(name.clone()))
                }
            }
        }
    }

    pub fn get_component(&self, name: &String) -> InstantiateResult<GlobalIdx<Component>> {
        match self {
            InstantiateScope::Linker(_, _, _) => todo!(),
            InstantiateScope::Instantiate(imports, _, _) => {
                if let Some(InstanceImport::Component(component_idx)) = imports.get(name) {
                    Ok(*component_idx)
                } else {
                    Err(ComponentVMError::LinkError(name.clone()))
                }
            }
        }
    }

    pub fn get_instance(&self, name: &String) -> InstantiateResult<GlobalIdx<Instance>> {
        match self {
            InstantiateScope::Linker(_, _, _) => todo!(),
            InstantiateScope::Instantiate(imports, _, _) => {
                if let Some(InstanceImport::Instance(instance_idx)) = imports.get(name) {
                    Ok(*instance_idx)
                } else {
                    Err(ComponentVMError::LinkError(name.clone()))
                }
            }
        }
    }
}

pub async fn instantiate(
    component: ParsedComponent,
    store: &mut Store,
    linker: &Linker,
) -> Result<(), ComponentVMError> {
    let arena = Arena::new();
    let mut manager = ScopeManager::new(&arena, linker, &component.ops);
    let mut state = InstantiateState::new();
    'outer: loop {
        for op in manager.current.ops() {
            match op {
                InstantiateOp::CoreInstantiate(idx) => {
                    let CoreInstance::Defined {
                        module_idx,
                        imports,
                    } = component.resolve_core_instance(*idx)?
                    else {
                        unreachable!();
                    };
                    let module =
                        component.resolve_core_module(*module_idx, manager.current, &state)?;
                    let mut registry = Registry::new();
                    for (name, import) in imports {
                        registry.register(name, state.get_core_instance(import).unwrap().clone());
                    }
                    let r = core_instantiate(module.module.clone(), store, &registry)
                        .await
                        .unwrap();
                    state.insert_core_instance(*idx, r);
                }
                InstantiateOp::CoreInstanceInlineExport(idx) => {}
                InstantiateOp::Instantiate(idx) => {
                    let Instance::Defined {
                        imports,
                        component_idx,
                    } = component.resolve_instance(*idx, manager.current, &state)?
                    else {
                        unreachable!();
                    };
                    let component =
                        component.resolve_component(*component_idx, manager.current, &state)?;
                    manager.push(imports.clone(), &component.ops);
                }
                InstantiateOp::InstantiateEnd => {
                    // special end
                    break 'outer;
                }
                InstantiateOp::InstantiateInlineExport(idx) => {}
            }
        }
        manager.pop();
    }
    Ok(())
}
