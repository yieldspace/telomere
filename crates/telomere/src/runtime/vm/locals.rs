use super::*;
use vstd::prelude::*;

verus! {

pub open spec fn spec_drop_result(
    view: crate::common::formal::StackView,
    size: nat,
) -> crate::common::formal::StackView {
    crate::common::formal::stack_drop_values(view, size)
}

pub open spec fn spec_select_result(
    view: crate::common::formal::StackView,
    size: nat,
    cond: u32,
) -> crate::common::formal::StackView {
    crate::common::formal::stack_select_bytes(view, size, cond)
}

#[inline(always)]
fn select_uses_top_value(cond: u32) -> (result: bool)
    ensures
        result == (cond == 0),
{
    cond == 0
}

} // verus!

#[inline(always)]
unsafe fn local_get<const SIZE: usize>(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let addr = (*tail_code).operand.local_addr as usize;
    vm_try!(ctx.local_get(addr, SIZE));
    trace!("op_local_get{SIZE}: {addr}");
    call_next(tail_code, 1, ctx)
}

#[inline(always)]
unsafe fn internal_op_drop(size: usize, ctx: &mut ExecuteContext) {
    ctx.stack_mut().drop(size);
}

#[inline(always)]
unsafe fn local_set<const SIZE: usize>(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let addr = (*tail_code).operand.local_addr as usize;
    ctx.local_set(addr, SIZE);
    call_next(tail_code, 1, ctx)
}

#[inline(always)]
unsafe fn local_tee<const SIZE: usize>(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let addr = (*tail_code).operand.local_addr as usize;
    ctx.local_tee(addr, SIZE);
    call_next(tail_code, 1, ctx)
}

/// WebAssembly `drop`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> []`.
/// Traps: none.
/// Notes: Reads or writes validated locals/stack slots and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_drop(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let size = (*tail_code).operand.drop_size as usize;
    trace!("op_drop: {size}");
    internal_op_drop(size, ctx);
    call_next(tail_code, 1, ctx)
}

#[inline(never)]
/// WebAssembly `select` helper for validated stack values.
///
/// Spec:
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: internal `select` operand handling.
/// Traps: none.
/// Notes: Reads the validated operands and materializes the selected value before the tail-dispatch wrapper continues.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction stream for the current active frame.
/// - `ctx` must reference a live execution context whose validated operand stack matches this `select` instruction.
/// - This helper must not keep borrows or guards alive across the follow-up stack push.
unsafe fn internal_op_select(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let x = (*tail_code).operand.select as usize;
    let cond = ctx.stack_mut().pop_u32();

    let a = ctx.stack_mut().pop_u8_array_generic::<8>(x);
    let b = ctx.stack_mut().pop_u8_array_generic::<8>(x);
    let value = if select_uses_top_value(cond) { a } else { b };
    trace!("op_select: {x} {cond} {a:?} {b:?} => {value:?}");
    vm_try!(ctx.stack_mut().push_slice(&value[0..x]));
    VMResult::Success(())
}

/// WebAssembly `select`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs, i32] -> [value]`.
/// Traps: none.
/// Notes: Reads or writes validated locals/stack slots and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_select(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    vm_try!(internal_op_select(tail_code, ctx));
    call_next(tail_code, 1, ctx)
}

/// WebAssembly `local.get`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[] -> [local]`.
/// Traps: none.
/// Notes: Reads or writes validated locals/stack slots and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_local_get4(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    local_get::<4>(tail_code, ctx)
}

/// WebAssembly `local.get`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[] -> [local]`.
/// Traps: none.
/// Notes: Reads or writes validated locals/stack slots and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_local_get8(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    local_get::<8>(tail_code, ctx)
}

/// WebAssembly `local.get`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[] -> [local]`.
/// Traps: none.
/// Notes: Reads or writes validated locals/stack slots and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_local_get16(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    local_get::<16>(tail_code, ctx)
}

/// WebAssembly `local.set`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[local] -> []`.
/// Traps: none.
/// Notes: Reads or writes validated locals/stack slots and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_local_set4(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    local_set::<4>(tail_code, ctx)
}

/// WebAssembly `local.set`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[local] -> []`.
/// Traps: none.
/// Notes: Reads or writes validated locals/stack slots and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_local_set8(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    local_set::<8>(tail_code, ctx)
}

/// WebAssembly `local.set`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[local] -> []`.
/// Traps: none.
/// Notes: Reads or writes validated locals/stack slots and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_local_set16(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    local_set::<16>(tail_code, ctx)
}

/// WebAssembly `local.tee`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[local] -> [local]`.
/// Traps: none.
/// Notes: Reads or writes validated locals/stack slots and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_local_tee4(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    local_tee::<4>(tail_code, ctx)
}

/// WebAssembly `local.tee`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[local] -> [local]`.
/// Traps: none.
/// Notes: Reads or writes validated locals/stack slots and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_local_tee8(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    local_tee::<8>(tail_code, ctx)
}

/// WebAssembly `local.tee`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[local] -> [local]`.
/// Traps: none.
/// Notes: Reads or writes validated locals/stack slots and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_local_tee16(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    local_tee::<16>(tail_code, ctx)
}
