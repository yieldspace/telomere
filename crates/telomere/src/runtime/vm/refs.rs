#![allow(clippy::missing_safety_doc)]

use super::*;
use vstd::prelude::*;

verus! {

#[inline(always)]
fn null_result(value: u32) -> (result: u32)
    ensures
        result == if value == 0u32 { 1u32 } else { 0u32 },
{
    if value == 0u32 { 1u32 } else { 0u32 }
}

} // verus!

pub unsafe fn op_ref_null(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    vm_try!(ctx.stack.push_u32(0));
    call_next(tail_code, 0, ctx)
}

pub unsafe fn op_ref_is_null(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let value = ctx.stack.pop_u32();
    vm_try!(ctx.stack.push_u32(null_result(value)));
    call_next(tail_code, 0, ctx)
}

pub unsafe fn op_ref_func(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let funcidx = (*tail_code).operand.u32;
    vm_try!(ctx
        .stack
        .push_u32(ctx.instance().funcs.as_slice()[funcidx as usize].get()));
    call_next(tail_code, 1, ctx)
}
