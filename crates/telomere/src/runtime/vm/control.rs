#![allow(clippy::missing_safety_doc)]

use super::*;
use vstd::prelude::*;

verus! {

#[inline(always)]
fn branch_taken(cond: u32) -> (taken: bool)
    ensures
        taken == (cond != 0),
{
    cond != 0
}

} // verus!

pub unsafe fn op_return(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let addr = (*tail_code).operand.jump_addr;
    trace!("op_return: {addr}");
    let code = ctx.code();
    let tail_code = code.offset(addr as isize);
    call_next(tail_code, 0, ctx)
}

pub unsafe fn op_end(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    trace!("op_end");
    call_next(tail_code, 0, ctx)
}

pub unsafe fn op_br(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let addr = (*tail_code).operand.jump_addr;
    trace!("op_br: {addr}");

    let tail_code = ctx.code().offset(addr as isize);
    call_next(tail_code, 0, ctx)
}

pub unsafe fn op_else(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    trace!("op_else");

    let addr = (*tail_code).operand.jump_addr;
    let tail_code = ctx.code().offset(addr as isize);
    call_next(tail_code, 1, ctx)
}

pub unsafe fn op_br_if(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let cond = ctx.stack.pop_u32();
    trace!("op_br_if: {cond}");

    let ptr = if branch_taken(cond) {
        let addr = (*tail_code).operand.jump_addr;
        ctx.code().offset(addr as isize)
    } else {
        tail_code.offset(1)
    };
    call_next(ptr, 0, ctx)
}

pub unsafe fn op_br_table(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let index = ctx.stack.pop_u32();
    let table_size = (*tail_code).operand.u32;

    let addr = if index < table_size {
        (*tail_code.offset((index + 1) as isize)).operand.jump_addr
    } else {
        (*tail_code.offset((table_size + 1) as isize))
            .operand
            .jump_addr
    };
    trace!(
        "op_br_table: index={} table_size={} => addr={}",
        index,
        table_size,
        addr
    );
    let tail_code = ctx.code().offset(addr as isize);
    call_next(tail_code, 0, ctx)
}

pub unsafe fn op_loop(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    trace!("op_loop: {}", (*tail_code).operand.jump_addr);

    let loop_param = (*tail_code).operand.loop_param;
    ctx.stack.block_return(
        &ctx.local_reference(),
        loop_param.stack_top as usize,
        loop_param.param_size as usize,
    );

    call_next(tail_code, 1, ctx)
}

pub unsafe fn op_if(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let else_addr = (*tail_code).operand.jump_addr;
    let value = ctx.stack.pop_u32();
    trace!("op_if: {else_addr} {value}");

    let ptr = if branch_taken(value) {
        tail_code.offset(1)
    } else {
        ctx.code().offset(else_addr as isize)
    };
    call_next(ptr, 0, ctx)
}

pub unsafe fn special_function_return(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    trace!("function return");
    let (prev_local_ref, tail_code) = ctx.stack.function_return(
        &ctx.local_reference(),
        (*tail_code).operand.drop_size as usize,
        ctx.gc,
    );
    ctx.set_local_reference(prev_local_ref);
    call_next(tail_code, 0, ctx)
}

pub unsafe fn special_block_return(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let block_return = &(*tail_code).operand.block_return;
    trace!(
        "block return: {:?} {:?} {:?}",
        ctx.local_reference(),
        block_return,
        ctx.stack
    );
    ctx.stack.block_return(
        &ctx.local_reference(),
        block_return.stack_top as usize,
        block_return.return_size as usize,
    );
    trace!("stack: {:?}", ctx.stack);

    call_next(tail_code, 1, ctx)
}

pub unsafe fn special_function_vm_end(
    _tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    if wait_effect(ctx, ctx.cont) {
        return VMResult::Success(());
    }
    ctx.cont = std::ptr::null();
    VMResult::Success(())
}
