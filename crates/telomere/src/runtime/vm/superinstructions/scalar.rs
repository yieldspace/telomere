use super::*;

#[inline(always)]
pub(super) fn i32_scalar_eval(lhs: u32, rhs: u32, kind: I32ScalarKind) -> VMResult<u32> {
    match kind {
        I32ScalarKind::Add => VMResult::Success((lhs as i32).wrapping_add(rhs as i32) as u32),
        I32ScalarKind::Sub => VMResult::Success((lhs as i32).wrapping_sub(rhs as i32) as u32),
        I32ScalarKind::Mul => VMResult::Success((lhs as i32).wrapping_mul(rhs as i32) as u32),
        I32ScalarKind::And => VMResult::Success(lhs & rhs),
        I32ScalarKind::Or => VMResult::Success(lhs | rhs),
        I32ScalarKind::Xor => VMResult::Success(lhs ^ rhs),
        I32ScalarKind::Shl => VMResult::Success(wasm_i32_shl(lhs as i32, rhs as i32) as u32),
        I32ScalarKind::ShrS => VMResult::Success(wasm_i32_shr_s(lhs as i32, rhs as i32) as u32),
        I32ScalarKind::ShrU => VMResult::Success(wasm_i32_shr_u(lhs, rhs)),
        I32ScalarKind::DivS => match (lhs as i32).checked_div(rhs as i32) {
            Some(value) => VMResult::Success(value as u32),
            None => VMResult::InvalidOperand,
        },
        I32ScalarKind::DivU => match lhs.checked_div(rhs) {
            Some(value) => VMResult::Success(value),
            None => VMResult::InvalidOperand,
        },
        I32ScalarKind::RemS => {
            let rhs = rhs as i32;
            if rhs == 0 {
                VMResult::InvalidOperand
            } else {
                VMResult::Success((lhs as i32).wrapping_rem(rhs) as u32)
            }
        }
        I32ScalarKind::RemU => match lhs.checked_rem(rhs) {
            Some(value) => VMResult::Success(value),
            None => VMResult::InvalidOperand,
        },
    }
}

#[inline(always)]
pub(super) fn i64_scalar_eval(lhs: u64, rhs: u64, kind: I64ScalarKind) -> VMResult<u64> {
    match kind {
        I64ScalarKind::Add => VMResult::Success((lhs as i64).wrapping_add(rhs as i64) as u64),
        I64ScalarKind::Sub => VMResult::Success((lhs as i64).wrapping_sub(rhs as i64) as u64),
        I64ScalarKind::Mul => VMResult::Success((lhs as i64).wrapping_mul(rhs as i64) as u64),
        I64ScalarKind::And => VMResult::Success(lhs & rhs),
        I64ScalarKind::Or => VMResult::Success(lhs | rhs),
        I64ScalarKind::Xor => VMResult::Success(lhs ^ rhs),
        I64ScalarKind::Shl => VMResult::Success(wasm_i64_shl(lhs as i64, rhs as i64) as u64),
        I64ScalarKind::ShrS => VMResult::Success(wasm_i64_shr_s(lhs as i64, rhs as i64) as u64),
        I64ScalarKind::ShrU => VMResult::Success(wasm_i64_shr_u(lhs, rhs)),
        I64ScalarKind::DivS => match (lhs as i64).checked_div(rhs as i64) {
            Some(value) => VMResult::Success(value as u64),
            None => VMResult::InvalidOperand,
        },
        I64ScalarKind::DivU => match lhs.checked_div(rhs) {
            Some(value) => VMResult::Success(value),
            None => VMResult::InvalidOperand,
        },
        I64ScalarKind::RemS => {
            let rhs = rhs as i64;
            if rhs == 0 {
                VMResult::InvalidOperand
            } else {
                VMResult::Success((lhs as i64).wrapping_rem(rhs) as u64)
            }
        }
        I64ScalarKind::RemU => match lhs.checked_rem(rhs) {
            Some(value) => VMResult::Success(value),
            None => VMResult::InvalidOperand,
        },
    }
}

#[inline(always)]
pub(super) fn f32_scalar_eval(lhs_bits: u32, rhs_bits: u32, kind: FloatScalarKind) -> u32 {
    let lhs = f32::from_bits(lhs_bits);
    let rhs = f32::from_bits(rhs_bits);
    match kind {
        FloatScalarKind::Add => (lhs + rhs).to_bits(),
        FloatScalarKind::Sub => (lhs - rhs).to_bits(),
        FloatScalarKind::Mul => (lhs * rhs).to_bits(),
        FloatScalarKind::Div => (lhs / rhs).to_bits(),
    }
}

#[inline(always)]
pub(super) fn f64_scalar_eval(lhs_bits: u64, rhs_bits: u64, kind: FloatScalarKind) -> u64 {
    let lhs = f64::from_bits(lhs_bits);
    let rhs = f64::from_bits(rhs_bits);
    match kind {
        FloatScalarKind::Add => (lhs + rhs).to_bits(),
        FloatScalarKind::Sub => (lhs - rhs).to_bits(),
        FloatScalarKind::Mul => (lhs * rhs).to_bits(),
        FloatScalarKind::Div => (lhs / rhs).to_bits(),
    }
}

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

#[inline(always)]
unsafe fn op_local_copy4_impl(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    tee: bool,
) -> VMResult<()> {
    let src_local = (*tail_code).operand.local_addr;
    let dst_local = (*tail_code.add(1)).operand.local_addr;
    let value = local_u32(ctx.stack, &ctx.local_reference(), src_local);
    write_local_u32(ctx.stack, &ctx.local_reference(), dst_local, value);
    if tee {
        vm_try!(ctx.stack.push_u32(value));
    }
    call_next(tail_code, 2, ctx)
}

pub unsafe fn op_local_copy4(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    op_local_copy4_impl(tail_code, ctx, false)
}

pub unsafe fn op_local_copy_tee4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_local_copy4_impl(tail_code, ctx, true)
}

#[inline(always)]
unsafe fn op_local_copy8_impl(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    tee: bool,
) -> VMResult<()> {
    let src_local = (*tail_code).operand.local_addr;
    let dst_local = (*tail_code.add(1)).operand.local_addr;
    let value = local_u64(ctx.stack, &ctx.local_reference(), src_local);
    write_local_u64(ctx.stack, &ctx.local_reference(), dst_local, value);
    if tee {
        vm_try!(ctx.stack.push_u64(value));
    }
    call_next(tail_code, 2, ctx)
}

pub unsafe fn op_local_copy8(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    op_local_copy8_impl(tail_code, ctx, false)
}

pub unsafe fn op_local_copy_tee8(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_local_copy8_impl(tail_code, ctx, true)
}

#[inline(always)]
unsafe fn op_i32_const_set_impl(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    tee: bool,
) -> VMResult<()> {
    let value = (*tail_code).operand.i32 as u32;
    let dst_local = (*tail_code.add(1)).operand.local_addr;
    write_local_u32(ctx.stack, &ctx.local_reference(), dst_local, value);
    if tee {
        vm_try!(ctx.stack.push_u32(value));
    }
    call_next(tail_code, 2, ctx)
}

pub unsafe fn op_i32_const_set4(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    op_i32_const_set_impl(tail_code, ctx, false)
}

pub unsafe fn op_i32_const_tee4(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    op_i32_const_set_impl(tail_code, ctx, true)
}

#[inline(always)]
unsafe fn op_i64_const_set_impl(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    tee: bool,
) -> VMResult<()> {
    let value = (*tail_code).operand.u64;
    let dst_local = (*tail_code.add(1)).operand.local_addr;
    write_local_u64(ctx.stack, &ctx.local_reference(), dst_local, value);
    if tee {
        vm_try!(ctx.stack.push_u64(value));
    }
    call_next(tail_code, 2, ctx)
}

pub unsafe fn op_i64_const_set8(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    op_i64_const_set_impl(tail_code, ctx, false)
}

pub unsafe fn op_i64_const_tee8(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    op_i64_const_set_impl(tail_code, ctx, true)
}

#[inline(always)]
unsafe fn op_f32_const_set_impl(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    tee: bool,
) -> VMResult<()> {
    let value = (*tail_code).operand.f32.to_bits();
    let dst_local = (*tail_code.add(1)).operand.local_addr;
    write_local_u32(ctx.stack, &ctx.local_reference(), dst_local, value);
    if tee {
        vm_try!(ctx.stack.push_u32(value));
    }
    call_next(tail_code, 2, ctx)
}

pub unsafe fn op_f32_const_set4(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    op_f32_const_set_impl(tail_code, ctx, false)
}

pub unsafe fn op_f32_const_tee4(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    op_f32_const_set_impl(tail_code, ctx, true)
}

#[inline(always)]
unsafe fn op_f64_const_set_impl(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    tee: bool,
) -> VMResult<()> {
    let value = (*tail_code).operand.f64.to_bits();
    let dst_local = (*tail_code.add(1)).operand.local_addr;
    write_local_u64(ctx.stack, &ctx.local_reference(), dst_local, value);
    if tee {
        vm_try!(ctx.stack.push_u64(value));
    }
    call_next(tail_code, 2, ctx)
}

pub unsafe fn op_f64_const_set8(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    op_f64_const_set_impl(tail_code, ctx, false)
}

pub unsafe fn op_f64_const_tee8(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    op_f64_const_set_impl(tail_code, ctx, true)
}

#[inline(always)]
pub unsafe fn op_i32_local_scalar_imm_push4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let src_local = (*tail_code).operand.local_addr;
    let imm = (*tail_code.add(1)).operand.i32 as u32;
    let kind = I32ScalarKind::from_raw((*tail_code.add(2)).operand.u32);
    let value = vm_try!(i32_scalar_eval(
        local_u32(ctx.stack, &ctx.local_reference(), src_local),
        imm,
        kind,
    ));
    vm_try!(ctx.stack.push_u32(value));
    call_next(tail_code, 3, ctx)
}

pub unsafe fn op_i32_local_scalar_imm_set4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let src_local = (*tail_code).operand.local_addr;
    let imm = (*tail_code.add(1)).operand.i32 as u32;
    let dst_local = (*tail_code.add(2)).operand.local_addr;
    let kind = I32ScalarKind::from_raw((*tail_code.add(3)).operand.u32);
    let value = vm_try!(i32_scalar_eval(
        local_u32(ctx.stack, &ctx.local_reference(), src_local),
        imm,
        kind,
    ));
    write_local_u32(ctx.stack, &ctx.local_reference(), dst_local, value);
    call_next(tail_code, 4, ctx)
}

pub unsafe fn op_i32_local_scalar_imm_tee4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let src_local = (*tail_code).operand.local_addr;
    let imm = (*tail_code.add(1)).operand.i32 as u32;
    let dst_local = (*tail_code.add(2)).operand.local_addr;
    let kind = I32ScalarKind::from_raw((*tail_code.add(3)).operand.u32);
    let value = vm_try!(i32_scalar_eval(
        local_u32(ctx.stack, &ctx.local_reference(), src_local),
        imm,
        kind,
    ));
    write_local_u32(ctx.stack, &ctx.local_reference(), dst_local, value);
    vm_try!(ctx.stack.push_u32(value));
    call_next(tail_code, 4, ctx)
}

pub unsafe fn op_i64_local_scalar_imm_set8(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let src_local = (*tail_code).operand.local_addr;
    let imm = (*tail_code.add(1)).operand.u64;
    let dst_local = (*tail_code.add(2)).operand.local_addr;
    let kind = I64ScalarKind::from_raw((*tail_code.add(3)).operand.u32);
    let value = vm_try!(i64_scalar_eval(
        local_u64(ctx.stack, &ctx.local_reference(), src_local),
        imm,
        kind,
    ));
    write_local_u64(ctx.stack, &ctx.local_reference(), dst_local, value);
    call_next(tail_code, 4, ctx)
}

pub unsafe fn op_i64_local_scalar_imm_tee8(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let src_local = (*tail_code).operand.local_addr;
    let imm = (*tail_code.add(1)).operand.u64;
    let dst_local = (*tail_code.add(2)).operand.local_addr;
    let kind = I64ScalarKind::from_raw((*tail_code.add(3)).operand.u32);
    let value = vm_try!(i64_scalar_eval(
        local_u64(ctx.stack, &ctx.local_reference(), src_local),
        imm,
        kind,
    ));
    write_local_u64(ctx.stack, &ctx.local_reference(), dst_local, value);
    vm_try!(ctx.stack.push_u64(value));
    call_next(tail_code, 4, ctx)
}

pub unsafe fn op_f32_local_scalar_imm_set4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let src_local = (*tail_code).operand.local_addr;
    let imm = (*tail_code.add(1)).operand.f32.to_bits();
    let dst_local = (*tail_code.add(2)).operand.local_addr;
    let kind = FloatScalarKind::from_raw((*tail_code.add(3)).operand.u32);
    let value = f32_scalar_eval(
        local_u32(ctx.stack, &ctx.local_reference(), src_local),
        imm,
        kind,
    );
    write_local_u32(ctx.stack, &ctx.local_reference(), dst_local, value);
    call_next(tail_code, 4, ctx)
}

pub unsafe fn op_f32_local_scalar_imm_tee4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let src_local = (*tail_code).operand.local_addr;
    let imm = (*tail_code.add(1)).operand.f32.to_bits();
    let dst_local = (*tail_code.add(2)).operand.local_addr;
    let kind = FloatScalarKind::from_raw((*tail_code.add(3)).operand.u32);
    let value = f32_scalar_eval(
        local_u32(ctx.stack, &ctx.local_reference(), src_local),
        imm,
        kind,
    );
    write_local_u32(ctx.stack, &ctx.local_reference(), dst_local, value);
    vm_try!(ctx.stack.push_u32(value));
    call_next(tail_code, 4, ctx)
}

pub unsafe fn op_f64_local_scalar_imm_set8(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let src_local = (*tail_code).operand.local_addr;
    let imm = (*tail_code.add(1)).operand.f64.to_bits();
    let dst_local = (*tail_code.add(2)).operand.local_addr;
    let kind = FloatScalarKind::from_raw((*tail_code.add(3)).operand.u32);
    let value = f64_scalar_eval(
        local_u64(ctx.stack, &ctx.local_reference(), src_local),
        imm,
        kind,
    );
    write_local_u64(ctx.stack, &ctx.local_reference(), dst_local, value);
    call_next(tail_code, 4, ctx)
}

pub unsafe fn op_f64_local_scalar_imm_tee8(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let src_local = (*tail_code).operand.local_addr;
    let imm = (*tail_code.add(1)).operand.f64.to_bits();
    let dst_local = (*tail_code.add(2)).operand.local_addr;
    let kind = FloatScalarKind::from_raw((*tail_code.add(3)).operand.u32);
    let value = f64_scalar_eval(
        local_u64(ctx.stack, &ctx.local_reference(), src_local),
        imm,
        kind,
    );
    write_local_u64(ctx.stack, &ctx.local_reference(), dst_local, value);
    vm_try!(ctx.stack.push_u64(value));
    call_next(tail_code, 4, ctx)
}

pub unsafe fn op_i32_local_local_scalar_push4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lhs_local = (*tail_code).operand.local_addr;
    let rhs_local = (*tail_code.add(1)).operand.local_addr;
    let kind = I32ScalarKind::from_raw((*tail_code.add(2)).operand.u32);
    let value = vm_try!(i32_scalar_eval(
        local_u32(ctx.stack, &ctx.local_reference(), lhs_local),
        local_u32(ctx.stack, &ctx.local_reference(), rhs_local),
        kind,
    ));
    vm_try!(ctx.stack.push_u32(value));
    call_next(tail_code, 3, ctx)
}

pub unsafe fn op_i32_local_local_scalar_set4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lhs_local = (*tail_code).operand.local_addr;
    let rhs_local = (*tail_code.add(1)).operand.local_addr;
    let dst_local = (*tail_code.add(2)).operand.local_addr;
    let kind = I32ScalarKind::from_raw((*tail_code.add(3)).operand.u32);
    let value = vm_try!(i32_scalar_eval(
        local_u32(ctx.stack, &ctx.local_reference(), lhs_local),
        local_u32(ctx.stack, &ctx.local_reference(), rhs_local),
        kind,
    ));
    write_local_u32(ctx.stack, &ctx.local_reference(), dst_local, value);
    call_next(tail_code, 4, ctx)
}

pub unsafe fn op_i32_local_local_scalar_tee4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lhs_local = (*tail_code).operand.local_addr;
    let rhs_local = (*tail_code.add(1)).operand.local_addr;
    let dst_local = (*tail_code.add(2)).operand.local_addr;
    let kind = I32ScalarKind::from_raw((*tail_code.add(3)).operand.u32);
    let value = vm_try!(i32_scalar_eval(
        local_u32(ctx.stack, &ctx.local_reference(), lhs_local),
        local_u32(ctx.stack, &ctx.local_reference(), rhs_local),
        kind,
    ));
    write_local_u32(ctx.stack, &ctx.local_reference(), dst_local, value);
    vm_try!(ctx.stack.push_u32(value));
    call_next(tail_code, 4, ctx)
}

pub unsafe fn op_i64_local_local_scalar_set8(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lhs_local = (*tail_code).operand.local_addr;
    let rhs_local = (*tail_code.add(1)).operand.local_addr;
    let dst_local = (*tail_code.add(2)).operand.local_addr;
    let kind = I64ScalarKind::from_raw((*tail_code.add(3)).operand.u32);
    let value = vm_try!(i64_scalar_eval(
        local_u64(ctx.stack, &ctx.local_reference(), lhs_local),
        local_u64(ctx.stack, &ctx.local_reference(), rhs_local),
        kind,
    ));
    write_local_u64(ctx.stack, &ctx.local_reference(), dst_local, value);
    call_next(tail_code, 4, ctx)
}

pub unsafe fn op_i64_local_local_scalar_tee8(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lhs_local = (*tail_code).operand.local_addr;
    let rhs_local = (*tail_code.add(1)).operand.local_addr;
    let dst_local = (*tail_code.add(2)).operand.local_addr;
    let kind = I64ScalarKind::from_raw((*tail_code.add(3)).operand.u32);
    let value = vm_try!(i64_scalar_eval(
        local_u64(ctx.stack, &ctx.local_reference(), lhs_local),
        local_u64(ctx.stack, &ctx.local_reference(), rhs_local),
        kind,
    ));
    write_local_u64(ctx.stack, &ctx.local_reference(), dst_local, value);
    vm_try!(ctx.stack.push_u64(value));
    call_next(tail_code, 4, ctx)
}

pub unsafe fn op_f32_local_local_scalar_set4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lhs_local = (*tail_code).operand.local_addr;
    let rhs_local = (*tail_code.add(1)).operand.local_addr;
    let dst_local = (*tail_code.add(2)).operand.local_addr;
    let kind = FloatScalarKind::from_raw((*tail_code.add(3)).operand.u32);
    let value = f32_scalar_eval(
        local_u32(ctx.stack, &ctx.local_reference(), lhs_local),
        local_u32(ctx.stack, &ctx.local_reference(), rhs_local),
        kind,
    );
    write_local_u32(ctx.stack, &ctx.local_reference(), dst_local, value);
    call_next(tail_code, 4, ctx)
}

pub unsafe fn op_f32_local_local_scalar_tee4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lhs_local = (*tail_code).operand.local_addr;
    let rhs_local = (*tail_code.add(1)).operand.local_addr;
    let dst_local = (*tail_code.add(2)).operand.local_addr;
    let kind = FloatScalarKind::from_raw((*tail_code.add(3)).operand.u32);
    let value = f32_scalar_eval(
        local_u32(ctx.stack, &ctx.local_reference(), lhs_local),
        local_u32(ctx.stack, &ctx.local_reference(), rhs_local),
        kind,
    );
    write_local_u32(ctx.stack, &ctx.local_reference(), dst_local, value);
    vm_try!(ctx.stack.push_u32(value));
    call_next(tail_code, 4, ctx)
}

pub unsafe fn op_f64_local_local_scalar_set8(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lhs_local = (*tail_code).operand.local_addr;
    let rhs_local = (*tail_code.add(1)).operand.local_addr;
    let dst_local = (*tail_code.add(2)).operand.local_addr;
    let kind = FloatScalarKind::from_raw((*tail_code.add(3)).operand.u32);
    let value = f64_scalar_eval(
        local_u64(ctx.stack, &ctx.local_reference(), lhs_local),
        local_u64(ctx.stack, &ctx.local_reference(), rhs_local),
        kind,
    );
    write_local_u64(ctx.stack, &ctx.local_reference(), dst_local, value);
    call_next(tail_code, 4, ctx)
}

pub unsafe fn op_f64_local_local_scalar_tee8(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lhs_local = (*tail_code).operand.local_addr;
    let rhs_local = (*tail_code.add(1)).operand.local_addr;
    let dst_local = (*tail_code.add(2)).operand.local_addr;
    let kind = FloatScalarKind::from_raw((*tail_code.add(3)).operand.u32);
    let value = f64_scalar_eval(
        local_u64(ctx.stack, &ctx.local_reference(), lhs_local),
        local_u64(ctx.stack, &ctx.local_reference(), rhs_local),
        kind,
    );
    write_local_u64(ctx.stack, &ctx.local_reference(), dst_local, value);
    vm_try!(ctx.stack.push_u64(value));
    call_next(tail_code, 4, ctx)
}
