use crate::component_model::{
    CoreFunc, CoreInstance, CoreInstanceInlineExport, CoreModule, Func, GlobalIdx, Instance,
};
pub use crate::instantiate as core_instantiate;
pub use crate::runtime::component_model::instantiate::context::InstantiateContext;
use crate::runtime::component_model::instantiate::context::{ResolvedImportKey, ResolvedImportMap};
pub use crate::runtime::component_model::instantiate::error::InstantiateError;
use crate::runtime::component_model::CoreInstanceInstantiated;
use crate::Registry;

pub type InstantiateResult<T> = Result<T, InstantiateError>;

mod context;
mod error;

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
    pub core_module_idx: GlobalIdx<CoreModule>,
    pub core_instance_idx: GlobalIdx<CoreInstance>,
    pub core_func_idx: GlobalIdx<CoreFunc>,
    pub instance_idx: GlobalIdx<Instance>,
    pub module_idx: GlobalIdx<CoreModule>,
    pub func_idx: GlobalIdx<Func>,
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
    let instance = ctx.get_core_instance(&idx)?;
    match instance {
        CoreInstance::Real {
            module_idx,
            imports,
        } => {
            let module = ctx.get_core_module(&module_idx)?;
            let mut registry = Registry::new();
            for (name, idx) in imports {
                let inst = ctx.get_instantiated_core_instance(idx);
                registry.register(name, inst.handle.clone());
            }
            let result = core_instantiate(module.value.clone(), &mut ctx.store, &registry);
            if result.is_err() {
                return Err(InstantiateError::CoreVMError(format!("{result:?}")));
            }
            ctx.register_instantiated_core_instance(
                idx,
                CoreInstanceInstantiated {
                    handle: result.unwrap(),
                    registry,
                },
            );
        }
        CoreInstance::Alias { exports } => {
            for (name, export) in exports {
                match export {
                    CoreInstanceInlineExport::Func(idx) => {
                        let (handle, name) = ctx.get_instantiated_core_function(idx);
                    }
                    CoreInstanceInlineExport::Table(idx) => {
                    }
                    CoreInstanceInlineExport::Memory(_) => {}
                    CoreInstanceInlineExport::Global(_) => {}
                    CoreInstanceInlineExport::Type(_) => {}
                    CoreInstanceInlineExport::Module(_) => {}
                    CoreInstanceInlineExport::Instance(_) => {}
                }
            }
            todo!()
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
    // let module = ctx.component.get_core_module(idx);
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

    instantiate_next(tail_code, 1, ctx)
}

pub unsafe fn instantiate_instance_end(
    tail_code: *const InstantiateInstr,
    ctx: &mut InstantiateContext,
) -> InstantiateResult<()> {
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

pub unsafe fn instantiate_canon_lower(
    tail_code: *const InstantiateInstr,
    ctx: &mut InstantiateContext,
) -> InstantiateResult<()> {
    let idx = (*tail_code).operand.core_func_idx;
    
    let func = ctx.get_core_func(&idx)?;
    let CoreFunc::CanonLower(func_idx, ft, i) = func else {
        unreachable!()
    };
    let func = ctx.get_func(func_idx)?;

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
