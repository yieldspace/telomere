use crate::aliasing as core_aliasing;
use crate::component_model::{
    CoreFuncRef, CoreFunction, CoreInstance, CoreInstanceImport, CoreInstanceInlineExport, Idx,
};
use crate::instantiate as core_instantiate;
pub use crate::runtime::component_model::instantiate::context::InstantiateContext;
use crate::Registry;

mod context;

pub type InstantiateResult<T> = Result<T, ()>;

pub type InstantiateOp =
    unsafe fn(*const InstantiateInstr, &mut InstantiateContext) -> InstantiateResult<()>;

#[derive(Copy, Clone)]
pub union InstantiateInstr {
    pub(crate) op: InstantiateOp,
    pub(crate) operand: InstantiateOperand,
}

#[derive(Clone, Copy)]
pub union InstantiateOperand {
    idx: usize,
    pub core_module_idx: usize,
    pub instance_idx: usize,
    pub module_idx: usize,
    pub core_instance_idx: usize,
}

#[inline(always)]
pub(crate) unsafe fn instantiate_next(
    tail_code: *const InstantiateInstr,
    consumed: isize,
    ctx: &mut InstantiateContext,
) -> InstantiateResult<()> {
    ((*tail_code.offset(consumed)).op)(tail_code.offset(consumed + 1), ctx)
}

pub unsafe fn instantiate_core_instance(
    tail_code: *const InstantiateInstr,
    ctx: &mut InstantiateContext,
) -> InstantiateResult<()> {
    let idx = (*tail_code).operand.core_instance_idx;
    let core_instance = ctx.component.get_core_instance(idx);
    let mut registry = Registry::new();
    match core_instance {
        CoreInstance::Real {
            module_idx,
            imports,
        } => {
            for (name, import) in imports {
                match import {
                    CoreInstanceImport::Instance(idx) => {
                        let addr = ctx.instantiated.core_instances[idx.global()].id;
                        registry.register(name, addr);
                    }
                }
            }
            let module = ctx.component.get_core_module(module_idx.global());
            let instance = core_instantiate(module.clone(), &mut ctx.store, &registry).unwrap();
            ctx.push_core_module_instance(instance, registry);
        }
        CoreInstance::Alias { exports } => {
            let triplets = exports
                .iter()
                .enumerate()
                .map(|(nth, (export_name, export))| match export {
                    CoreInstanceInlineExport::Func(idx) => {
                        let func = ctx.component.get_core_function(idx.global());
                        match func {
                            CoreFunction::Export(CoreFuncRef(inst_idx, idx, _, name)) => {
                                let inst = ctx
                                    .instantiated
                                    .core_instances
                                    .get(inst_idx.global())
                                    .unwrap();
                                registry.register(nth.to_string(), inst.id);
                                (nth.to_string(), name.clone(), export_name.clone())
                            }
                            _ => {
                                let (instance_addr, name) =
                                    ctx.core_functions.get(idx.global()).unwrap();
                                registry.register(nth.to_string(), *instance_addr);
                                (nth.to_string(), name.clone(), export_name.clone())
                            }
                        }
                    }
                    CoreInstanceInlineExport::Memory(idx) => {
                        let (instance_addr, name) = ctx.core_memories.get(idx.global()).unwrap();
                        registry.register(nth.to_string(), *instance_addr);
                        (nth.to_string(), name.clone(), export_name.clone())
                    }
                    CoreInstanceInlineExport::Table(idx) => {
                        let (instance_addr, name) = ctx.core_tables.get(idx.global()).unwrap();
                        registry.register(nth.to_string(), *instance_addr);
                        (nth.to_string(), name.clone(), export_name.clone())
                    }
                    _ => todo!(),
                })
                .collect::<Vec<_>>();
            let inst = core_aliasing(&registry, triplets.as_slice(), &mut ctx.store).unwrap();
            ctx.push_core_module_instance(inst, registry)
        }
    }

    instantiate_next(tail_code, 1, ctx)
}

pub unsafe fn instantiate_core_type(
    tail_code: *const InstantiateInstr,
    ctx: &mut InstantiateContext,
) -> InstantiateResult<()> {
    todo!()
}

pub unsafe fn instantiate_instance_start(
    tail_code: *const InstantiateInstr,
    ctx: &mut InstantiateContext,
) -> InstantiateResult<()> {
    todo!();
}

pub unsafe fn instantiate_instance_end(
    tail_code: *const InstantiateInstr,
    ctx: &mut InstantiateContext,
) -> InstantiateResult<()> {
    todo!();
}

pub unsafe fn instantiate_inline_instance(
    tail_code: *const InstantiateInstr,
    ctx: &mut InstantiateContext,
) -> InstantiateResult<()> {
    todo!();
}

pub unsafe fn instantiate_type(
    tail_code: *const InstantiateInstr,
    ctx: &mut InstantiateContext,
) -> InstantiateResult<()> {
    todo!();
}

pub unsafe fn instantiate_canon_lower(
    tail_code: *const InstantiateInstr,
    ctx: &mut InstantiateContext,
) -> InstantiateResult<()> {
    let idx = (*tail_code).operand.idx;

    todo!();
}

pub unsafe fn instantiate_special_end(
    tail_code: *const InstantiateInstr,
    ctx: &mut InstantiateContext,
) -> InstantiateResult<()> {
    Ok(())
}
