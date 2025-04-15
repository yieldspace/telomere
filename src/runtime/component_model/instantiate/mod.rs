use crate::aliasing as core_aliasing;
use crate::component_model::{
    CoreInstance, CoreInstanceImport, CoreInstanceInlineExport, Idx,
};
use crate::instantiate as core_instantiate;
use crate::runtime::component_model::instantiate::context::InstantiateContext;
use crate::Registry;

mod context;

pub type InstantiateResult<T> = Result<T, ()>;

pub type InstantiateOp =
    unsafe fn(*const InstantiateInstr, &mut InstantiateContext) -> InstantiateResult<()>;

pub union InstantiateInstr {
    op: InstantiateOp,
    operand: InstantiateOperand,
}

#[derive(Clone, Copy)]
pub union InstantiateOperand {
    idx: usize,
}

#[inline(always)]
pub(crate) unsafe fn instantiate_next(
    tail_code: *const InstantiateInstr,
    consumed: isize,
    ctx: &mut InstantiateContext,
) -> InstantiateResult<()> {
    ((*tail_code.offset(consumed)).op)(tail_code.offset(consumed + 1), ctx)
}

pub unsafe fn instantiate_core_module(
    tail_code: *const InstantiateInstr,
    ctx: &mut InstantiateContext,
) -> InstantiateResult<()> {
    let idx = (*tail_code).operand.idx;
    let core_instance = &ctx.component.core_instances[idx];
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
            let instance = core_instantiate(
                ctx.component
                    .core_modules
                    .get(module_idx.global())
                    .unwrap()
                    .clone(),
                &mut ctx.store,
                &registry,
            )
            .unwrap();
            ctx.push_core_module_instance(instance, registry);
        }
        CoreInstance::Alias { exports } => {
            let triplets = exports
                .iter()
                .enumerate()
                .map(|(nth, (export_name, export))| match export {
                    CoreInstanceInlineExport::Func(idx) => {
                        let (instance_addr, name) = ctx.core_functions.get(idx.global()).unwrap();
                        registry.register(nth.to_string(), *instance_addr);
                        (nth.to_string(), name.clone(), export_name.clone())
                    }
                    CoreInstanceInlineExport::Memory(idx) => {
                        let (instance_addr, name) = ctx.core_memories.get(*idx).unwrap();
                        registry.register(nth.to_string(), *instance_addr);
                        (nth.to_string(), name.clone(), export_name.clone())
                    }
                    CoreInstanceInlineExport::Table(idx) => {
                        let (instance_addr, name) = ctx.core_tables.get(*idx).unwrap();
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
