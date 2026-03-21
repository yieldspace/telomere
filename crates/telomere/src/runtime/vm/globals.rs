use super::*;
use vstd::prelude::*;

verus! {

pub open spec fn spec_global_get_result(global: crate::common::formal::GlobalView) -> Seq<u8> {
    crate::common::formal::global_get_bytes(global)
}

pub open spec fn spec_global_set_result(
    global: crate::common::formal::GlobalView,
    bytes: Seq<u8>,
) -> crate::common::formal::GlobalView {
    crate::common::formal::global_set_bytes(global, bytes)
}

#[inline(always)]
fn global_index(idx: usize) -> (result: usize)
    ensures
        result == idx,
{
    idx
}

} // verus!

#[inline(always)]
unsafe fn global_addr(tail_code: *const Instr, ctx: &ExecuteContext) -> GcRef {
    let idx = global_index((*tail_code).operand.u32 as usize);
    ctx.instance().globals.as_slice()[idx]
}

#[inline(always)]
unsafe fn global_get<const SIZE: usize>(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let addr = global_addr(tail_code, ctx);
    let bytes = ctx.gc_ref().get_global(addr).to_vec();
    debug_assert_eq!(bytes.len(), SIZE);
    vm_try!(ctx.push_slice(&bytes));
    call_next(tail_code, 1, ctx)
}

#[inline(always)]
unsafe fn global_set<const SIZE: usize>(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let addr = global_addr(tail_code, ctx);
    let value = ctx.pop_u8_array::<SIZE>();
    ctx.gc_mut().get_global_mut(addr).copy_from_slice(&value);
    call_next(tail_code, 1, ctx)
}

/// WebAssembly `global.get`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[] -> [global]`.
/// Traps: none.
/// Notes: Accesses the instance global storage and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_global_get4(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    global_get::<4>(tail_code, ctx)
}

/// WebAssembly `global.get`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[] -> [global]`.
/// Traps: none.
/// Notes: Accesses the instance global storage and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_global_get8(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    global_get::<8>(tail_code, ctx)
}

/// WebAssembly `global.get`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[] -> [global]`.
/// Traps: none.
/// Notes: Accesses the instance global storage and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_global_get16(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    global_get::<16>(tail_code, ctx)
}

/// WebAssembly `global.set`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[global] -> []`.
/// Traps: none.
/// Notes: Accesses the instance global storage and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_global_set4(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    global_set::<4>(tail_code, ctx)
}

/// WebAssembly `global.set`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[global] -> []`.
/// Traps: none.
/// Notes: Accesses the instance global storage and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_global_set8(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    global_set::<8>(tail_code, ctx)
}

/// WebAssembly `global.set`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[global] -> []`.
/// Traps: none.
/// Notes: Accesses the instance global storage and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_global_set16(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    global_set::<16>(tail_code, ctx)
}
