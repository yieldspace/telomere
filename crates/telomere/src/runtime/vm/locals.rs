#![allow(clippy::missing_safety_doc)]

use super::*;

pub unsafe fn op_drop(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let size = (*tail_code).operand.drop_size as usize;
    trace!("op_drop: {size}");

    ctx.stack.drop(size);
    call_next(tail_code, 1, ctx)
}

#[inline(never)]
unsafe fn internal_op_select(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let x = (*tail_code).operand.select as usize;
    let cond = ctx.stack.pop_u32();

    let a = ctx.stack.pop_u8_array_generic::<8>(x);
    let b = ctx.stack.pop_u8_array_generic::<8>(x);
    let value = if cond == 0 { a } else { b };
    trace!("op_select: {x} {cond} {a:?} {b:?} => {value:?}");
    vm_try!(ctx.stack.push_slice(&value[0..x]));
    VMResult::Success(())
}

pub unsafe fn op_select(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    vm_try!(internal_op_select(tail_code, ctx));
    call_next(tail_code, 1, ctx)
}

pub unsafe fn op_local_get4(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let addr = (*tail_code).operand.local_addr as usize;
    vm_try!(ctx.stack.local_get4(&ctx.local_reference(), addr));
    trace!("op_local_get4: {addr}");

    call_next(tail_code, 1, ctx)
}

pub unsafe fn op_local_get8(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let addr = (*tail_code).operand.local_addr as usize;
    vm_try!(ctx.stack.local_get8(&ctx.local_reference(), addr));
    trace!("op_local_get8: {addr}");

    call_next(tail_code, 1, ctx)
}

pub unsafe fn op_local_get16(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let addr = (*tail_code).operand.local_addr as usize;
    vm_try!(ctx.stack.local_get16(&ctx.local_reference(), addr));
    trace!("op_local_get16: {addr}");

    call_next(tail_code, 1, ctx)
}

pub unsafe fn op_local_set4(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let addr = (*tail_code).operand.local_addr as usize;
    ctx.stack.local_set4(&ctx.local_reference(), addr);
    call_next(tail_code, 1, ctx)
}

pub unsafe fn op_local_set8(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let addr = (*tail_code).operand.local_addr as usize;
    ctx.stack.local_set8(&ctx.local_reference(), addr);

    call_next(tail_code, 1, ctx)
}

pub unsafe fn op_local_set16(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let addr = (*tail_code).operand.local_addr as usize;
    ctx.stack.local_set16(&ctx.local_reference(), addr);

    call_next(tail_code, 1, ctx)
}

pub unsafe fn op_local_tee4(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let addr = (*tail_code).operand.local_addr as usize;
    ctx.stack.local_tee4(&ctx.local_reference(), addr);
    call_next(tail_code, 1, ctx)
}

pub unsafe fn op_local_tee8(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let addr = (*tail_code).operand.local_addr as usize;
    ctx.stack.local_tee8(&ctx.local_reference(), addr);

    call_next(tail_code, 1, ctx)
}

pub unsafe fn op_local_tee16(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let addr = (*tail_code).operand.local_addr as usize;
    ctx.stack.local_tee16(&ctx.local_reference(), addr);

    call_next(tail_code, 1, ctx)
}
