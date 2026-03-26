use super::*;

#[inline(always)]
fn local_i32(ctx: &mut ExecuteContext, addr: usize) -> i32 {
    unsafe {
        ctx.stack
            .local_u32_from_base(ctx.local_base_ptr as *const u8, addr) as i32
    }
}

pub unsafe fn op_local_get4_i32_const_add(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    dispatch_profile_count("op_local_get4_i32_const_add");
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
    dispatch_profile_count("op_local_get4_i32_const_add_set4");
    let src = (*tail_code).operand.local_addr as usize;
    let imm = (*tail_code.add(1)).operand.i32;
    let dst = (*tail_code.add(2)).operand.local_addr as usize;
    let result = local_i32(ctx, src).wrapping_add(imm);
    vm_try!(ctx.stack.push_i32(result));
    ctx.stack.local_set4_from_base(ctx.local_base_ptr, dst);
    call_next(tail_code, 3, ctx)
}

pub unsafe fn op_local_get4_i32_const_add_tee4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    dispatch_profile_count("op_local_get4_i32_const_add_tee4");
    let src = (*tail_code).operand.local_addr as usize;
    let imm = (*tail_code.add(1)).operand.i32;
    let dst = (*tail_code.add(2)).operand.local_addr as usize;
    let result = local_i32(ctx, src).wrapping_add(imm);
    vm_try!(ctx.stack.push_i32(result));
    ctx.stack.local_tee4_from_base(ctx.local_base_ptr, dst);
    call_next(tail_code, 3, ctx)
}

pub unsafe fn op_local_get4_local_get4_i32_add(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    dispatch_profile_count("op_local_get4_local_get4_i32_add");
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
    dispatch_profile_count("op_local_get4_local_get4_i32_add_set4");
    let lhs = (*tail_code).operand.local_addr as usize;
    let rhs = (*tail_code.add(1)).operand.local_addr as usize;
    let dst = (*tail_code.add(2)).operand.local_addr as usize;
    let result = local_i32(ctx, lhs).wrapping_add(local_i32(ctx, rhs));
    vm_try!(ctx.stack.push_i32(result));
    ctx.stack.local_set4_from_base(ctx.local_base_ptr, dst);
    call_next(tail_code, 3, ctx)
}

pub unsafe fn op_local_get4_local_get4_i32_add_tee4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    dispatch_profile_count("op_local_get4_local_get4_i32_add_tee4");
    let lhs = (*tail_code).operand.local_addr as usize;
    let rhs = (*tail_code.add(1)).operand.local_addr as usize;
    let dst = (*tail_code.add(2)).operand.local_addr as usize;
    let result = local_i32(ctx, lhs).wrapping_add(local_i32(ctx, rhs));
    vm_try!(ctx.stack.push_i32(result));
    ctx.stack.local_tee4_from_base(ctx.local_base_ptr, dst);
    call_next(tail_code, 3, ctx)
}

pub unsafe fn op_local_get4_br_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    dispatch_profile_count("op_local_get4_br_if");
    let addr = (*tail_code).operand.local_addr as usize;
    let cond = ctx
        .stack
        .local_u32_from_base(ctx.local_base_ptr as *const u8, addr);
    let ptr = if cond != 0 {
        let jump_addr = (*tail_code.add(1)).operand.jump_addr;
        ctx.code().offset(jump_addr as isize)
    } else {
        tail_code.add(2)
    };
    call_next(ptr, 0, ctx)
}

pub unsafe fn op_local_get4_i32_const_add_br_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    dispatch_profile_count("op_local_get4_i32_const_add_br_if");
    let addr = (*tail_code).operand.local_addr as usize;
    let imm = (*tail_code.add(1)).operand.i32;
    let cond = local_i32(ctx, addr).wrapping_add(imm) as u32;
    let ptr = if cond != 0 {
        let jump_addr = (*tail_code.add(2)).operand.jump_addr;
        ctx.code().offset(jump_addr as isize)
    } else {
        tail_code.add(3)
    };
    call_next(ptr, 0, ctx)
}
