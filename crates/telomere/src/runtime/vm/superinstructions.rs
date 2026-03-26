use super::*;

#[inline(always)]
fn local_i32(ctx: &mut ExecuteContext, addr: usize) -> i32 {
    unsafe {
        ctx.stack
            .local_u32_from_base(ctx.local_base_ptr as *const u8, addr) as i32
    }
}

#[inline(always)]
fn i32_compare(kind: u32, lhs: i32, rhs: i32) -> bool {
    match kind {
        0 => lhs == rhs,
        1 => lhs != rhs,
        2 => lhs < rhs,
        3 => (lhs as u32) < (rhs as u32),
        4 => lhs > rhs,
        5 => (lhs as u32) > (rhs as u32),
        6 => lhs <= rhs,
        7 => (lhs as u32) <= (rhs as u32),
        8 => lhs >= rhs,
        9 => (lhs as u32) >= (rhs as u32),
        _ => false,
    }
}

#[inline(always)]
unsafe fn br_if_ptr(
    tail_code: *const Instr,
    target_offset: usize,
    taken_advance: usize,
    cond: u32,
    ctx: &mut ExecuteContext,
) -> *const Instr {
    if cond != 0 {
        let jump_addr = (*tail_code.add(target_offset)).operand.jump_addr;
        ctx.code().offset(jump_addr as isize)
    } else {
        tail_code.add(taken_advance)
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
    let result = local_i32(ctx, src).wrapping_add(imm) as u32;
    ctx.stack
        .local_set4_from_base_value(ctx.local_base_ptr, dst, result);
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
    vm_try!(ctx.stack.push_i32_fast(result));
    ctx.stack
        .local_set4_from_base_value(ctx.local_base_ptr, dst, result as u32);
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
    let result = local_i32(ctx, lhs).wrapping_add(local_i32(ctx, rhs)) as u32;
    ctx.stack
        .local_set4_from_base_value(ctx.local_base_ptr, dst, result);
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
    vm_try!(ctx.stack.push_i32_fast(result));
    ctx.stack
        .local_set4_from_base_value(ctx.local_base_ptr, dst, result as u32);
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
    let ptr = br_if_ptr(tail_code, 1, 2, cond, ctx);
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
    let ptr = br_if_ptr(tail_code, 2, 3, cond, ctx);
    call_next(ptr, 0, ctx)
}

pub unsafe fn op_local_get4_local_get4_i32_add_br_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    dispatch_profile_count("op_local_get4_local_get4_i32_add_br_if");
    let lhs = (*tail_code).operand.local_addr as usize;
    let rhs = (*tail_code.add(1)).operand.local_addr as usize;
    let cond = local_i32(ctx, lhs).wrapping_add(local_i32(ctx, rhs)) as u32;
    let ptr = br_if_ptr(tail_code, 2, 3, cond, ctx);
    call_next(ptr, 0, ctx)
}

pub unsafe fn op_local_get4_i32_eqz_br_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    dispatch_profile_count("op_local_get4_i32_eqz_br_if");
    let addr = (*tail_code).operand.local_addr as usize;
    let cond = (local_i32(ctx, addr) == 0) as u32;
    let ptr = br_if_ptr(tail_code, 1, 2, cond, ctx);
    call_next(ptr, 0, ctx)
}

pub unsafe fn op_local_get4_i32_const_compare_br_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    dispatch_profile_count("op_local_get4_i32_const_compare_br_if");
    let lhs = (*tail_code).operand.local_addr as usize;
    let kind = (*tail_code.add(1)).operand.u32;
    let rhs = (*tail_code.add(2)).operand.i32;
    let cond = i32_compare(kind, local_i32(ctx, lhs), rhs) as u32;
    let ptr = br_if_ptr(tail_code, 3, 4, cond, ctx);
    call_next(ptr, 0, ctx)
}

pub unsafe fn op_local_get4_local_get4_compare_br_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    dispatch_profile_count("op_local_get4_local_get4_compare_br_if");
    let lhs = (*tail_code).operand.local_addr as usize;
    let rhs = (*tail_code.add(1)).operand.local_addr as usize;
    let kind = (*tail_code.add(2)).operand.u32;
    let cond = i32_compare(kind, local_i32(ctx, lhs), local_i32(ctx, rhs)) as u32;
    let ptr = br_if_ptr(tail_code, 3, 4, cond, ctx);
    call_next(ptr, 0, ctx)
}

pub unsafe fn op_local_get4_i32_const_add_tee4_br_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    dispatch_profile_count("op_local_get4_i32_const_add_tee4_br_if");
    let src = (*tail_code).operand.local_addr as usize;
    let imm = (*tail_code.add(1)).operand.i32;
    let dst = (*tail_code.add(2)).operand.local_addr as usize;
    let result = local_i32(ctx, src).wrapping_add(imm);
    let cond = result as u32;
    ctx.stack
        .local_set4_from_base_value(ctx.local_base_ptr, dst, cond);
    let ptr = br_if_ptr(tail_code, 3, 4, cond, ctx);
    call_next(ptr, 0, ctx)
}
