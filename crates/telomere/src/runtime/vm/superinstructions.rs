use super::*;

#[inline(always)]
fn local_u32(stack: &Stack, local_reference: &LocalReference, local_addr: u32) -> u32 {
    stack.local_read_u32(local_reference, local_addr as usize)
}

#[inline(always)]
fn write_local_u32(
    stack: &mut Stack,
    local_reference: &LocalReference,
    local_addr: u32,
    value: u32,
) {
    stack.local_write_u32(local_reference, local_addr as usize, value);
}

#[inline(always)]
unsafe fn op_i32_local_add_sub_imm(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    subtract: bool,
    tee: bool,
) -> VMResult<()> {
    let src_local = (*tail_code).operand.local_addr;
    let imm = (*tail_code.add(1)).operand.i32;
    let dst_local = (*tail_code.add(2)).operand.local_addr;
    let lhs = local_u32(ctx.stack, &ctx.local_reference(), src_local) as i32;
    let value = if subtract {
        lhs.wrapping_sub(imm)
    } else {
        lhs.wrapping_add(imm)
    } as u32;
    write_local_u32(ctx.stack, &ctx.local_reference(), dst_local, value);
    if tee {
        vm_try!(ctx.stack.push_u32(value));
    }
    call_next(tail_code, 3, ctx)
}

enum LocalBitImmOp {
    And,
    Shl,
    ShrU,
}

#[inline(always)]
unsafe fn op_i32_local_bit_imm(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    tee: bool,
    op: LocalBitImmOp,
) -> VMResult<()> {
    let src_local = (*tail_code).operand.local_addr;
    let imm = (*tail_code.add(1)).operand.i32;
    let dst_local = (*tail_code.add(2)).operand.local_addr;
    let lhs = local_u32(ctx.stack, &ctx.local_reference(), src_local);
    let value = match op {
        LocalBitImmOp::And => lhs & imm as u32,
        LocalBitImmOp::Shl => wasm_i32_shl(lhs as i32, imm) as u32,
        LocalBitImmOp::ShrU => wasm_i32_shr_u(lhs, imm as u32),
    };
    write_local_u32(ctx.stack, &ctx.local_reference(), dst_local, value);
    if tee {
        vm_try!(ctx.stack.push_u32(value));
    }
    call_next(tail_code, 3, ctx)
}

pub unsafe fn op_i32_local_add_imm_set4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_i32_local_add_sub_imm(tail_code, ctx, false, false)
}

pub unsafe fn op_i32_local_add_imm_tee4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_i32_local_add_sub_imm(tail_code, ctx, false, true)
}

pub unsafe fn op_i32_local_sub_imm_set4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_i32_local_add_sub_imm(tail_code, ctx, true, false)
}

pub unsafe fn op_i32_local_sub_imm_tee4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_i32_local_add_sub_imm(tail_code, ctx, true, true)
}

pub unsafe fn op_i32_local_and_imm_set4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_i32_local_bit_imm(tail_code, ctx, false, LocalBitImmOp::And)
}

pub unsafe fn op_i32_local_and_imm_tee4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_i32_local_bit_imm(tail_code, ctx, true, LocalBitImmOp::And)
}

pub unsafe fn op_i32_local_shl_imm_set4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_i32_local_bit_imm(tail_code, ctx, false, LocalBitImmOp::Shl)
}

pub unsafe fn op_i32_local_shl_imm_tee4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_i32_local_bit_imm(tail_code, ctx, true, LocalBitImmOp::Shl)
}

pub unsafe fn op_i32_local_shr_u_imm_set4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_i32_local_bit_imm(tail_code, ctx, false, LocalBitImmOp::ShrU)
}

pub unsafe fn op_i32_local_shr_u_imm_tee4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_i32_local_bit_imm(tail_code, ctx, true, LocalBitImmOp::ShrU)
}

#[inline(always)]
unsafe fn op_i32_local_local_add(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    tee: bool,
) -> VMResult<()> {
    let lhs_local = (*tail_code).operand.local_addr;
    let rhs_local = (*tail_code.add(1)).operand.local_addr;
    let dst_local = (*tail_code.add(2)).operand.local_addr;
    let value = local_u32(ctx.stack, &ctx.local_reference(), lhs_local).wrapping_add(local_u32(
        ctx.stack,
        &ctx.local_reference(),
        rhs_local,
    ));
    write_local_u32(ctx.stack, &ctx.local_reference(), dst_local, value);
    if tee {
        vm_try!(ctx.stack.push_u32(value));
    }
    call_next(tail_code, 3, ctx)
}

pub unsafe fn op_i32_local_local_add_set4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_i32_local_local_add(tail_code, ctx, false)
}

pub unsafe fn op_i32_local_local_add_tee4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_i32_local_local_add(tail_code, ctx, true)
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

#[inline(always)]
unsafe fn local_addr_mem_start(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<usize> {
    let local_addr = (*tail_code).operand.local_addr;
    let memarg = (*tail_code.add(1)).operand.memarg;
    compute_memory_offset(
        memarg,
        local_u32(ctx.stack, &ctx.local_reference(), local_addr),
    )
}

pub unsafe fn op_i32_local_addr_load(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let start = vm_try!(local_addr_mem_start(tail_code, ctx));
    let value = vm_try!(ctx
        .gc
        .local_read_u32_at(ctx.default_local_memory_id_unchecked(), start));
    vm_try!(ctx.stack.push_u32(value));
    call_next(tail_code, 2, ctx)
}

pub unsafe fn op_i32_local_addr_load8_u(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let start = vm_try!(local_addr_mem_start(tail_code, ctx));
    let value = vm_try!(ctx
        .gc
        .local_read_u8_at(ctx.default_local_memory_id_unchecked(), start));
    vm_try!(ctx.stack.push_u32(u32::from(value)));
    call_next(tail_code, 2, ctx)
}

pub unsafe fn op_i32_local_addr_load16_s(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let start = vm_try!(local_addr_mem_start(tail_code, ctx));
    let value = vm_try!(ctx
        .gc
        .local_read_i16_at(ctx.default_local_memory_id_unchecked(), start));
    vm_try!(ctx.stack.push_i32(i32::from(value)));
    call_next(tail_code, 2, ctx)
}

pub unsafe fn op_i32_local_addr_load16_u(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let start = vm_try!(local_addr_mem_start(tail_code, ctx));
    let value = vm_try!(ctx
        .gc
        .local_read_u16_at(ctx.default_local_memory_id_unchecked(), start));
    vm_try!(ctx.stack.push_u32(u32::from(value)));
    call_next(tail_code, 2, ctx)
}

pub unsafe fn op_f32_local_addr_load(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let start = vm_try!(local_addr_mem_start(tail_code, ctx));
    let value = vm_try!(ctx
        .gc
        .local_read_u32_at(ctx.default_local_memory_id_unchecked(), start));
    vm_try!(ctx.stack.push_u32(value));
    call_next(tail_code, 2, ctx)
}

#[inline(always)]
unsafe fn local_local_store_start(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<usize> {
    let addr_local = (*tail_code).operand.local_addr;
    let memarg = (*tail_code.add(2)).operand.memarg;
    compute_memory_offset(
        memarg,
        local_u32(ctx.stack, &ctx.local_reference(), addr_local),
    )
}

pub unsafe fn op_i32_local_local_store(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let start = vm_try!(local_local_store_start(tail_code, ctx));
    let value_local = (*tail_code.add(1)).operand.local_addr;
    let bytes = local_u32(ctx.stack, &ctx.local_reference(), value_local).to_le_bytes();
    vm_try!(ctx
        .gc
        .local_write_bytes(ctx.default_local_memory_id_unchecked(), start, &bytes));
    call_next(tail_code, 3, ctx)
}

pub unsafe fn op_i32_local_local_store8(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let start = vm_try!(local_local_store_start(tail_code, ctx));
    let value_local = (*tail_code.add(1)).operand.local_addr;
    let bytes = [(local_u32(ctx.stack, &ctx.local_reference(), value_local) & 0xff) as u8];
    vm_try!(ctx
        .gc
        .local_write_bytes(ctx.default_local_memory_id_unchecked(), start, &bytes));
    call_next(tail_code, 3, ctx)
}

pub unsafe fn op_i32_local_local_store16(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let start = vm_try!(local_local_store_start(tail_code, ctx));
    let value_local = (*tail_code.add(1)).operand.local_addr;
    let value = local_u32(ctx.stack, &ctx.local_reference(), value_local);
    let bytes = [(value & 0xff) as u8, ((value >> 8) & 0xff) as u8];
    vm_try!(ctx
        .gc
        .local_write_bytes(ctx.default_local_memory_id_unchecked(), start, &bytes));
    call_next(tail_code, 3, ctx)
}
