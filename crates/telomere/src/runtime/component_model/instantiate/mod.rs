pub use crate::runtime::component_model::instantiate::context::InstantiateContext;
use crate::runtime::component_model::instantiate::context::{ResolvedImportKey, ResolvedImportMap};
use crate::runtime::component_model::ComponentVMError;

mod context;

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

pub unsafe fn instantiate_core_instance(
    tail_code: *const InstantiateInstr,
    ctx: &mut InstantiateContext,
) -> InstantiateResult<()> {
    instantiate_next(tail_code, 1, ctx)
}

pub unsafe fn instantiate_core_type(
    _tail_code: *const InstantiateInstr,
    _ctx: &mut InstantiateContext,
) -> InstantiateResult<()> {
    todo!()
}

pub unsafe fn instantiate_import_core_module(
    _tail_code: *const InstantiateInstr,
    _ctx: &mut InstantiateContext,
) -> InstantiateResult<()> {
    // let idx = (*tail_code).operand.core_module_idx;
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

    ctx.resolved_imports
        .insert(ResolvedImportKey::Child(idx), ResolvedImportMap::new());

    ctx.current = Some(idx);

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
