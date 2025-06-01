use crate::common::InstanceHandle;
use crate::component_model::{
    Component, CoreInstance, CoreModule, GlobalIdx, Instance, InstanceExport, InstanceImport,
};
use crate::parser::component_model::ParsedComponent;
pub use crate::runtime::component_model::instantiate::context::InstantiateContext;
use crate::runtime::component_model::instantiate::id::ResolveId;
use crate::runtime::component_model::{ComponentModelInstance, ComponentVMError, Linker};
use crate::runtime::instantiate as core_instantiate;
use crate::{Registry, Store};
pub use scope::{InstantiateScope, ScopeManager};
pub use state::InstantiateState;
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;
use typed_arena::Arena;

mod context;
mod id;
mod scope;
mod state;

pub type InstantiateResult<T> = Result<T, ComponentVMError>;

#[derive(Debug, Clone)]
pub enum InstantiateOp {
    CoreInstantiate(GlobalIdx<CoreInstance>),
    CoreInstanceInlineExport(GlobalIdx<CoreInstance>),
    Instantiate(GlobalIdx<Instance>),
    InstantiateEnd,
    MapExport(Box<String>, InstanceExport),
    MapImport(Box<String>, InstanceImport),
    InstantiateInlineExport(GlobalIdx<Instance>),
}
type InnerTy<'a> = std::slice::Iter<'a, InstantiateOp>;
pub struct InstantiateOpIterator<'a, F>
where
    F: FnMut(GlobalIdx<Instance>) -> InnerTy<'a>,
{
    iter: InnerTy<'a>,
    stack: VecDeque<InnerTy<'a>>,
    resolver: F,
}
impl<'a, F> InstantiateOpIterator<'a, F>
where
    F: FnMut(GlobalIdx<Instance>) -> InnerTy<'a>,
{
    pub fn new(slice: &'a [InstantiateOp], resolver: F) -> Self {
        Self {
            iter: slice.iter(),
            stack: VecDeque::new(),
            resolver,
        }
    }
}
impl<'a, F> Iterator for InstantiateOpIterator<'a, F>
where
    F: FnMut(GlobalIdx<Instance>) -> InnerTy<'a>,
{
    type Item = &'a InstantiateOp;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(v) = self.iter.next() {
            match v {
                op @ InstantiateOp::Instantiate(v) => {
                    let mut iter = (self.resolver)(*v);
                    std::mem::swap(&mut self.iter, &mut iter);
                    self.stack.push_back(iter);
                    return Some(op);
                }
                other => return Some(other),
            }
        } else if let Some(iter) = self.stack.pop_back() {
            self.iter = iter;
            self.next()
        } else {
            return None;
        }
    }
}

#[derive(Default)]
pub struct ComponentInstance {
    pub(crate) exports: HashMap<String, Export>,
}

pub enum Import {
    Instance(Rc<ComponentInstance>),
    Func,
}

pub enum Export {
    Instance(Rc<ComponentInstance>),
    Func,
}

pub async fn instantiate(
    component: ParsedComponent,
    store: &mut Store,
    linker: &Linker<'_>,
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
                    manager.push(imports.clone(), *idx, &component.ops);
                }
                InstantiateOp::InstantiateEnd => {
                    // special end
                    break 'outer;
                }
                InstantiateOp::InstantiateInlineExport(idx) => {}
                InstantiateOp::MapExport(name, exp) => {}
                InstantiateOp::MapImport(_, _) => {}
            }
        }
        let idx = manager.current.idx();
        manager.pop();
    }
    Ok(())
}
