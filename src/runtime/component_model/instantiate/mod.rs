use crate::component_model::{
    CoreFuncRef, CoreFunction, CoreInstance, CoreInstanceImport, CoreInstanceInlineExport,
    CoreModule, CoreModuleReference, CoreSortWithIdx, Idx, Reference, SortWithIdx,
};
use crate::instantiate as core_instantiate;
pub use crate::runtime::component_model::instantiate::context::InstantiateContext;
use crate::runtime::component_model::instantiate::context::{
    InstantiatedInstance, InstantiatedInstanceExport, ResolvedImportKey, ResolvedImportMap,
};
use crate::runtime::component_model::ComponentVMError;
use crate::Registry;
use crate::{aliasing as core_aliasing, Module};

mod context;
mod instance;

pub type InstantiateResult<T> = Result<T, ComponentVMError>;

pub type InstantiateOp =
    unsafe fn(*const InstantiateInstr, &mut InstantiateContext) -> InstantiateResult<()>;

#[derive(Copy, Clone)]
pub union InstantiateInstr {
    pub(crate) op: InstantiateOp,
    pub(crate) operand: InstantiateOperand,
}

#[derive(Clone, Copy)]
pub union InstantiateOperand {
    #[allow(dead_code)]
    idx: usize,
    pub core_module_idx: usize,
    pub core_instance_idx: usize,
    pub core_func_idx: usize,
    pub instance_idx: usize,
    pub module_idx: usize,
    pub func_idx: usize,
    pub type_idx: usize,
}

#[inline(always)]
pub(crate) unsafe fn instantiate_next(
    tail_code: *const InstantiateInstr,
    consumed: isize,
    ctx: &mut InstantiateContext,
) -> InstantiateResult<()> {
    ((*tail_code.offset(consumed)).op)(tail_code.offset(consumed + 1), ctx)
}

fn instantiate_core_module_rec(
    ctx: &mut InstantiateContext,
    registry: Registry,
    module: CoreModule,
) -> InstantiateResult<()> {
    let CoreModule { value, .. } = module;
    match value {
        None => {
            unreachable!()
            // match reference.clone().unwrap() {
            // CoreModuleReference::Imported(name) => match ctx.current {
            //     None => {
            //         let module = ctx.linker.get_module(&name).unwrap();
            //         let instance = core_instantiate(module.clone(), ctx.store, &registry).unwrap();
            //         ctx.push_core_module_instance(instance, registry);
            //         Ok(())
            //     }
            //     Some(idx) => {
            //         let inst = ctx.component.get_instance(idx);
            //         todo!("instanceにreferenceをつけてやる")
            //     }
            // },
            // CoreModuleReference::Instance(idx, name) => {
            //     let inst = ctx.instances.get(&idx.global()).unwrap();
            //     if let InstantiatedInstanceExport::Module(idx) = inst.exports.get(&name).unwrap() {
            //         let core_module = ctx.component.get_core_module(idx.global()).clone();
            //         return instantiate_core_module_rec(ctx, registry, core_module);
            //     };
            //     panic!("Invalid instance export");
            // }
            // CoreModuleReference::TypeOverwritten(idx) => {
            //     let core_module = ctx.component.get_core_module(idx.global()).clone();
            //     instantiate_core_module_rec(ctx, registry, core_module)
            // }
            // CoreModuleReference::Exported(_) => unreachable!(),
        }
        Some(module) => {
            let instance = core_instantiate(module.clone(), ctx.store, &registry).unwrap();
            ctx.push_core_module_instance(instance, registry);
            Ok(())
        }
    }
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
                        let addr = ctx.instantiated.core_instances[idx.global()].id.clone();
                        registry.register(name, addr);
                    }
                }
            }
            let module = ctx.component.get_core_module(module_idx.global()).clone();
            instantiate_core_module_rec(ctx, registry, module)?;
        }
        CoreInstance::Alias { exports } => {
            let triplets = exports
                .iter()
                .enumerate()
                .map(|(nth, (export_name, export))| match export {
                    CoreInstanceInlineExport::Func(idx) => {
                        let func = ctx.component.get_core_function(idx.global());
                        match func {
                            CoreFunction::Export(CoreFuncRef(inst_idx, _idx, name)) => {
                                let inst = ctx
                                    .instantiated
                                    .core_instances
                                    .get(inst_idx.global())
                                    .unwrap();
                                registry.register(nth.to_string(), inst.id.clone());
                                (nth.to_string(), name.clone(), export_name.clone())
                            }
                            _ => {
                                let (instance_addr, name) =
                                    ctx.core_functions.get(idx.global()).unwrap();
                                registry.register(nth.to_string(), instance_addr.clone());
                                (nth.to_string(), name.clone(), export_name.clone())
                            }
                        }
                    }
                    CoreInstanceInlineExport::Memory(idx) => {
                        let (instance_addr, name) = ctx.core_memories.get(idx.global()).unwrap();
                        registry.register(nth.to_string(), instance_addr.clone());
                        (nth.to_string(), name.clone(), export_name.clone())
                    }
                    CoreInstanceInlineExport::Table(idx) => {
                        let (instance_addr, name) = ctx.core_tables.get(idx.global()).unwrap();
                        registry.register(nth.to_string(), instance_addr.clone());
                        (nth.to_string(), name.clone(), export_name.clone())
                    }
                    _ => todo!(),
                })
                .collect::<Vec<_>>();
            let inst = core_aliasing(&registry, triplets.as_slice(), ctx.store).unwrap();
            ctx.push_core_module_instance(inst, registry)
        }
    }

    instantiate_next(tail_code, 1, ctx)
}

pub unsafe fn instantiate_core_type(
    _tail_code: *const InstantiateInstr,
    _ctx: &mut InstantiateContext,
) -> InstantiateResult<()> {
    todo!()
}

pub unsafe fn instantiate_import_core_module(
    tail_code: *const InstantiateInstr,
    ctx: &mut InstantiateContext,
) -> InstantiateResult<()> {
    let idx = (*tail_code).operand.core_module_idx;
    let module = ctx.component.get_core_module(idx);
    // assert!(module.value.is_none());
    // if let Some(CoreModuleReference::Imported(name)) = &module.reference {
    //     let imported_module = ctx.component.get_instance(idx);
    //     ctx.resolved_imports.get_mut(&ResolvedImportKey::Child(ctx.current.unwrap())).unwrap()
    //         .core_modules.insert(idx, )
    // } else {
    //     unreachable!()
    // }
    unreachable!()
}

pub unsafe fn instantiate_instance_start(
    tail_code: *const InstantiateInstr,
    ctx: &mut InstantiateContext,
) -> InstantiateResult<()> {
    let idx = (*tail_code).operand.instance_idx;

    ctx.resolved_imports
        .insert(ResolvedImportKey::Child(idx), ResolvedImportMap::new());

    ctx.current = Some(idx);

    instantiate_next(tail_code, 1, ctx)
}

pub unsafe fn instantiate_instance_end(
    tail_code: *const InstantiateInstr,
    ctx: &mut InstantiateContext,
) -> InstantiateResult<()> {
    let target = ctx.current.unwrap();
    let inst = ctx.component.get_instance(target);
    let value = inst.value.clone().unwrap();
    let instantiated = InstantiatedInstance {
        exports: value
            .exports
            .into_iter()
            .map(|(name, sort)| {
                let value = match sort {
                    SortWithIdx::Core(CoreSortWithIdx::Module(idx)) => {
                        InstantiatedInstanceExport::Module(idx)
                    }
                    SortWithIdx::Func(idx) => todo!(),
                    #[cfg(feature = "component-gated-feature-value-imports-exports")]
                    SortWithIdx::Value(idx) => {}
                    SortWithIdx::Type(idx) => todo!(),
                    SortWithIdx::Component(idx) => todo!(),
                    SortWithIdx::Instance(idx) => InstantiatedInstanceExport::Instance(idx),
                    _ => unreachable!(),
                };
                (name, value)
            })
            .collect(),
    };
    instantiate_next(tail_code, 0, ctx)
}

pub unsafe fn instantiate_inline_instance(
    _tail_code: *const InstantiateInstr,
    _ctx: &mut InstantiateContext,
) -> InstantiateResult<()> {
    todo!();
}

pub unsafe fn instantiate_type(
    _tail_code: *const InstantiateInstr,
    _ctx: &mut InstantiateContext,
) -> InstantiateResult<()> {
    todo!();
}

pub unsafe fn instantiate_core_function(
    tail_code: *const InstantiateInstr,
    _ctx: &mut InstantiateContext,
) -> InstantiateResult<()> {
    let _idx = (*tail_code).operand.core_func_idx;

    todo!();
}
#[allow(clippy::result_unit_err)]
pub unsafe fn instantiate_function(
    tail_code: *const InstantiateInstr,
    _ctx: &mut InstantiateContext,
) -> InstantiateResult<()> {
    let _idx = (*tail_code).operand.func_idx;
    todo!()
}

#[allow(clippy::result_unit_err)]
pub unsafe fn instantiate_special_end(
    _tail_code: *const InstantiateInstr,
    _ctx: &mut InstantiateContext,
) -> InstantiateResult<()> {
    Ok(())
}
