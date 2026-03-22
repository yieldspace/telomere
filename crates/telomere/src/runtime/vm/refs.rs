use super::*;

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
    vm_try!(ctx.stack.push_u32(0));
    call_next(tail_code, 0, ctx)
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
    let value = ctx.stack.peek_top_u32();
    ctx.stack.replace_top_u32(u32::from(value == 0));
    call_next(tail_code, 0, ctx)
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
    vm_try!(ctx
        .stack
        .push_u32(ctx.instance().funcs.as_slice()[funcidx as usize].get()));
    call_next(tail_code, 1, ctx)
}
