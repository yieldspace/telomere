use super::*;

#[inline(always)]
fn local_i32(ctx: &mut ExecuteContext, addr: usize) -> i32 {
    i32::from_le_bytes(
        ctx.stack
            .local_bytes(&ctx.local_reference(), addr, 4)
            .try_into()
            .expect("validated i32 local access"),
    )
}

pub unsafe fn op_local_get4_i32_const_add(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let addr = (*tail_code).operand.local_addr as usize;
    let imm = (*tail_code.add(1)).operand.i32;
    let result = local_i32(ctx, addr).wrapping_add(imm);
    vm_try!(ctx.stack.push_i32(result));
    call_next(tail_code, 2, ctx)
}

pub unsafe fn op_local_get4_i32_const_add_set4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let src = (*tail_code).operand.local_addr as usize;
    let imm = (*tail_code.add(1)).operand.i32;
    let dst = (*tail_code.add(2)).operand.local_addr as usize;
    let result = local_i32(ctx, src).wrapping_add(imm);
    vm_try!(ctx.stack.push_i32(result));
    ctx.stack.local_set4(&ctx.local_reference(), dst);
    call_next(tail_code, 3, ctx)
}

pub unsafe fn op_local_get4_i32_const_add_tee4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let src = (*tail_code).operand.local_addr as usize;
    let imm = (*tail_code.add(1)).operand.i32;
    let dst = (*tail_code.add(2)).operand.local_addr as usize;
    let result = local_i32(ctx, src).wrapping_add(imm);
    vm_try!(ctx.stack.push_i32(result));
    ctx.stack.local_tee4(&ctx.local_reference(), dst);
    call_next(tail_code, 3, ctx)
}

pub unsafe fn op_local_get4_local_get4_i32_add(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lhs = (*tail_code).operand.local_addr as usize;
    let rhs = (*tail_code.add(1)).operand.local_addr as usize;
    let result = local_i32(ctx, lhs).wrapping_add(local_i32(ctx, rhs));
    vm_try!(ctx.stack.push_i32(result));
    call_next(tail_code, 2, ctx)
}

pub unsafe fn op_local_get4_local_get4_i32_add_set4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lhs = (*tail_code).operand.local_addr as usize;
    let rhs = (*tail_code.add(1)).operand.local_addr as usize;
    let dst = (*tail_code.add(2)).operand.local_addr as usize;
    let result = local_i32(ctx, lhs).wrapping_add(local_i32(ctx, rhs));
    vm_try!(ctx.stack.push_i32(result));
    ctx.stack.local_set4(&ctx.local_reference(), dst);
    call_next(tail_code, 3, ctx)
}

pub unsafe fn op_local_get4_local_get4_i32_add_tee4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lhs = (*tail_code).operand.local_addr as usize;
    let rhs = (*tail_code.add(1)).operand.local_addr as usize;
    let dst = (*tail_code.add(2)).operand.local_addr as usize;
    let result = local_i32(ctx, lhs).wrapping_add(local_i32(ctx, rhs));
    vm_try!(ctx.stack.push_i32(result));
    ctx.stack.local_tee4(&ctx.local_reference(), dst);
    call_next(tail_code, 3, ctx)
}
