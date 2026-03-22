use super::*;

#[inline(always)]
fn local_i32(stack: &Stack, local_reference: &LocalReference, local_addr: u32) -> i32 {
    stack.local_read_u32(local_reference, local_addr as usize) as i32
}

#[inline(always)]
fn local_u32(stack: &Stack, local_reference: &LocalReference, local_addr: u32) -> u32 {
    stack.local_read_u32(local_reference, local_addr as usize)
}

#[inline(always)]
fn write_local_i32(
    stack: &mut Stack,
    local_reference: &LocalReference,
    local_addr: u32,
    value: i32,
) {
    stack.local_write_u32(local_reference, local_addr as usize, value as u32);
}

#[inline(always)]
unsafe fn op_i32_local_add_imm(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    subtract: bool,
    tee: bool,
) -> VMResult<()> {
    let src_local = (*tail_code).operand.local_addr;
    let imm = (*tail_code.add(1)).operand.i32;
    let dst_local = (*tail_code.add(2)).operand.local_addr;
    let lhs = local_i32(ctx.stack, &ctx.local_reference(), src_local);
    let value = if subtract {
        lhs.wrapping_sub(imm)
    } else {
        lhs.wrapping_add(imm)
    };
    write_local_i32(ctx.stack, &ctx.local_reference(), dst_local, value);
    if tee {
        vm_try!(ctx.stack.push_i32(value));
    }
    call_next(tail_code, 3, ctx)
}

pub unsafe fn op_i32_local_add_imm_set4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_i32_local_add_imm(tail_code, ctx, false, false)
}

pub unsafe fn op_i32_local_add_imm_tee4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_i32_local_add_imm(tail_code, ctx, false, true)
}

pub unsafe fn op_i32_local_sub_imm_set4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_i32_local_add_imm(tail_code, ctx, true, false)
}

pub unsafe fn op_i32_local_sub_imm_tee4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_i32_local_add_imm(tail_code, ctx, true, true)
}

pub unsafe fn op_i32_local_eqz_br_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let local_addr = (*tail_code).operand.local_addr;
    let target = (*tail_code.add(1)).operand.jump_addr;
    let ptr = if local_u32(ctx.stack, &ctx.local_reference(), local_addr) == 0 {
        ctx.code().offset(target as isize)
    } else {
        tail_code.offset(2)
    };
    call_next(ptr, 0, ctx)
}

pub unsafe fn op_i32_local_local_ge_u_br_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lhs_local_addr = (*tail_code).operand.local_addr;
    let rhs_local_addr = (*tail_code.add(1)).operand.local_addr;
    let target = (*tail_code.add(2)).operand.jump_addr;
    let ptr = if local_u32(ctx.stack, &ctx.local_reference(), lhs_local_addr)
        >= local_u32(ctx.stack, &ctx.local_reference(), rhs_local_addr)
    {
        ctx.code().offset(target as isize)
    } else {
        tail_code.offset(3)
    };
    call_next(ptr, 0, ctx)
}

pub unsafe fn op_i32_load_const_local(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let start = (*tail_code).operand.u32 as usize;
    vm_try!(ctx.gc.local_push_memory_to_stack::<4>(
        ctx.default_local_memory_id_unchecked(),
        ctx.stack,
        start,
    ));
    call_next(tail_code, 1, ctx)
}

pub unsafe fn op_i32_local_get4_store_const_local(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let start = (*tail_code).operand.u32 as usize;
    let local_addr = (*tail_code.add(1)).operand.local_addr;
    let bytes = local_u32(ctx.stack, &ctx.local_reference(), local_addr).to_le_bytes();
    vm_try!(ctx
        .gc
        .local_write_bytes(ctx.default_local_memory_id_unchecked(), start, &bytes,));
    call_next(tail_code, 2, ctx)
}
