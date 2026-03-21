use super::*;
use vstd::prelude::*;

verus! {

pub open spec fn spec_ref_null_result() -> crate::common::formal::RefView {
    crate::common::formal::ref_null()
}

pub open spec fn spec_ref_is_null_result(value: u32) -> u32 {
    crate::common::formal::ref_is_null_result(value)
}

#[inline(always)]
fn null_ref_value() -> (result: u32)
    ensures
        result == 0u32,
{
    0u32
}

#[inline(always)]
fn null_result(value: u32) -> (result: u32)
    ensures
        result == if value == 0u32 { 1u32 } else { 0u32 },
{
    if value == 0u32 { 1u32 } else { 0u32 }
}

} // verus!

#[inline(always)]
unsafe fn push_ref_value(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    skip: isize,
    value: u32,
) -> VMResult<()> {
    vm_try!(ctx.stack_mut().push_u32(value));
    call_next(tail_code, skip, ctx)
}

#[inline(always)]
unsafe fn ref_func_value(ctx: &ExecuteContext, funcidx: u32) -> u32 {
    ctx.instance().funcs.as_slice()[funcidx as usize].get()
}

#[inline(always)]
unsafe fn pop_ref_value(ctx: &mut ExecuteContext) -> u32 {
    ctx.stack_mut().pop_u32()
}

/// WebAssembly `ref.null`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[] -> [ref]`.
/// Traps: none.
/// Notes: Implements the validated reference semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_ref_null(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    push_ref_value(tail_code, ctx, 0, null_ref_value())
}

/// WebAssembly `ref.is.null`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[ref] -> [i32]`.
/// Traps: none.
/// Notes: Implements the validated reference semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_ref_is_null(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let value = pop_ref_value(ctx);
    push_ref_value(tail_code, ctx, 0, null_result(value))
}

/// WebAssembly `ref.func`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[] -> [funcref]`.
/// Traps: none.
/// Notes: Implements the validated reference semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_ref_func(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let funcidx = (*tail_code).operand.u32;
    let value = ref_func_value(ctx, funcidx);
    push_ref_value(tail_code, ctx, 1, value)
}
