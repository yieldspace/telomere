use crate::common::InstanceHandle;
use crate::component_model::{
    Component, CoreInstance, CoreModule, GlobalIdx, Instance, InstanceExport, InstanceImport,
    Relation,
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
    Import(String, InstanceImport),
    Export(String, InstanceExport),
    Alias(AliasOp),
    InstantiateEnd,
    InstantiateInlineExport(GlobalIdx<Instance>),
}

#[derive(Debug, Clone)]
pub enum AliasOp {
    Instance(GlobalIdx<Instance>),
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

#[derive(Default, Clone, Debug)]
pub struct ComponentInstance {
    pub(crate) exports: HashMap<String, InstanceExport>,
    pub(crate) core_modules: HashMap<GlobalIdx<CoreModule>, GlobalIdx<CoreModule>>,
    pub(crate) instances: HashMap<GlobalIdx<Instance>, ComponentInstance>,
    pub(crate) core_instances: HashMap<GlobalIdx<CoreInstance>, InstanceHandle>,
}

#[derive(Clone)]
pub enum Import {
    CoreModule(GlobalIdx<CoreModule>),
    Instance(ComponentInstance),
    Func,
}

#[derive(Clone)]
pub enum Export {
    Instance(ComponentInstance),
    Func,
}

pub async fn instantiate(
    component: ParsedComponent,
    store: &mut Store,
    linker: &Linker<'_>,
) -> Result<(), ComponentVMError> {
    let mut manager = ScopeManager::new(&linker);
    let iter = InstantiateOpIterator::new(&component.ops, |idx| {
        let Instance::Defined { component_idx, .. } = component.resolve_instance(idx).unwrap()
        else {
            unreachable!();
        };
        let component = component.resolve_component(*component_idx).unwrap();
        component.ops.iter()
    });
    for op in iter {
        match op {
            InstantiateOp::CoreInstantiate(idx) => {
                let CoreInstance::Defined {
                    module_idx,
                    imports,
                } = component.resolve_core_instance(*idx)?
                else {
                    unreachable!();
                };
                let module = component.resolve_core_module(*module_idx)?;
                let mut registry = Registry::new();
                for (name, import) in imports {
                    registry.register(
                        name,
                        manager
                            .scope()
                            .get_core_instance_instantiated(import)
                            .clone(),
                    );
                }
                let r = core_instantiate(module.module.clone(), store, &registry)
                    .await
                    .unwrap();
                manager.scope_mut().register_core_instance(*idx, r);
            }
            InstantiateOp::CoreInstanceInlineExport(_) => {}
            InstantiateOp::Instantiate(idx) => {
                // scopeをセットする
                let Instance::Defined {
                    component_idx,
                    imports,
                } = component.resolve_instance(*idx)?
                else {
                    unreachable!();
                };
                let data = {
                    let scope = manager.scope();
                    let mut data = HashMap::new();
                    for (name, import) in imports {
                        match import {
                            InstanceImport::CoreModule(idx) => {
                                data.insert(name.clone(), Import::CoreModule(*idx));
                            }
                            InstanceImport::Func(_) => {}
                            InstanceImport::Component(_) => {}
                            InstanceImport::Instance(idx) => {
                                let inst = scope.get_instance_instantiated(idx);
                                data.insert(name.clone(), Import::Instance(inst.clone()));
                            }
                        }
                    }
                    data
                };
                manager.push(*idx, data);
            }
            InstantiateOp::InstantiateEnd => {
                let scope = manager.pop();
                let idx = scope.idx().clone().unwrap();
                let inst = scope.make();
                manager.scope_mut().register_instance(idx, inst);
            }
            InstantiateOp::InstantiateInlineExport(idx) => {
                let Instance::InlineExport { exports } = component.resolve_instance(*idx)? else {
                    unreachable!();
                };
                let inst = ComponentInstance {
                    exports: exports.clone(),
                    core_modules: Default::default(),
                    instances: exports
                        .iter()
                        .filter_map(|(_, v)| match v {
                            InstanceExport::Instance(x) => {
                                Some((*x, manager.scope().get_instance_instantiated(x).clone()))
                            }
                            _ => None,
                        })
                        .collect(),
                    core_instances: Default::default(),
                };
                manager.scope_mut().register_instance(*idx, inst);
            }
            InstantiateOp::Export(name, target) => {
                manager
                    .scope_mut()
                    .register_export(name.clone(), target.clone());
            }
            InstantiateOp::Import(_, _) => {}
            InstantiateOp::Alias(op) => match op {
                AliasOp::Instance(base_idx) => match component.instances.get(base_idx).unwrap() {
                    Relation::FromExport(idx, name) => {
                        let inst = manager.scope().get_instance_instantiated(idx);
                        let InstanceExport::Instance(idx) = inst.exports.get(name).unwrap() else {
                            unreachable!();
                        };
                        let export_inst = inst.instances.get(idx).unwrap().clone();
                        manager
                            .scope_mut()
                            .register_instance(*base_idx, export_inst);
                    }
                    _ => unreachable!(),
                },
            },
        }
    }
    println!("{:?}", manager.pop().make());
    Ok(())
}
