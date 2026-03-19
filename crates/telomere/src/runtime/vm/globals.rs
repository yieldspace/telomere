#![allow(clippy::missing_safety_doc)]

use super::*;
use vstd::prelude::*;

verus! {

#[inline(always)]
fn global_index(idx: usize) -> (result: usize)
    ensures
        result == idx,
{
    idx
}

} // verus!

pub unsafe fn op_global_get4(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let idx = global_index((*tail_code).operand.u32 as usize);
    let addr = ctx.instance().globals.as_slice()[idx];
    vm_try!(ctx.stack.push_slice(ctx.gc.get_global(addr)));
    call_next(tail_code, 1, ctx)
}

pub unsafe fn op_global_get8(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let idx = global_index((*tail_code).operand.u32 as usize);
    let addr = ctx.instance().globals.as_slice()[idx];
    vm_try!(ctx.stack.push_slice(ctx.gc.get_global(addr)));
    call_next(tail_code, 1, ctx)
}

pub unsafe fn op_global_get16(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let idx = global_index((*tail_code).operand.u32 as usize);
    let addr = ctx.instance().globals.as_slice()[idx];
    vm_try!(ctx.stack.push_slice(ctx.gc.get_global(addr)));
    call_next(tail_code, 1, ctx)
}

pub unsafe fn op_global_set4(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let idx = global_index((*tail_code).operand.u32 as usize);
    let addr = ctx.instance().globals.as_slice()[idx];
    ctx.gc
        .get_global_mut(addr)
        .copy_from_slice(&ctx.stack.pop_u8_array::<4>());
    call_next(tail_code, 1, ctx)
}

pub unsafe fn op_global_set8(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let idx = global_index((*tail_code).operand.u32 as usize);
    let addr = ctx.instance().globals.as_slice()[idx];
    ctx.gc
        .get_global_mut(addr)
        .copy_from_slice(&ctx.stack.pop_u8_array::<8>());
    call_next(tail_code, 1, ctx)
}

pub unsafe fn op_global_set16(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let idx = global_index((*tail_code).operand.u32 as usize);
    let addr = ctx.instance().globals.as_slice()[idx];
    ctx.gc
        .get_global_mut(addr)
        .copy_from_slice(&ctx.stack.pop_u8_array::<16>());
    call_next(tail_code, 1, ctx)
}
