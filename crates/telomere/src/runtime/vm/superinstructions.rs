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
fn local_u64(stack: &Stack, local_reference: &LocalReference, local_addr: u32) -> u64 {
    stack.local_read_u64(local_reference, local_addr as usize)
}

#[inline(always)]
fn write_local_u64(
    stack: &mut Stack,
    local_reference: &LocalReference,
    local_addr: u32,
    value: u64,
) {
    stack.local_write_u64(local_reference, local_addr as usize, value);
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProducerSeedKind {
    Local = 0,
    LocalImmScalar = 1,
    LocalLocalScalar = 2,
    LocalAddrLoad = 3,
    LocalImmAddrLoad = 4,
    ConstAddrLoad = 5,
}

impl ProducerSeedKind {
    #[inline(always)]
    fn from_raw(raw: u32) -> Self {
        match raw {
            0 => Self::Local,
            1 => Self::LocalImmScalar,
            2 => Self::LocalLocalScalar,
            3 => Self::LocalAddrLoad,
            4 => Self::LocalImmAddrLoad,
            5 => Self::ConstAddrLoad,
            _ => unreachable!("invalid ProducerSeedKind: {raw}"),
        }
    }
}

#[inline(always)]
fn bool_to_u32(value: bool) -> u32 {
    if value {
        1
    } else {
        0
    }
}

#[inline(always)]
fn i32_scalar_eval(lhs: u32, rhs: u32, kind: I32ScalarKind) -> VMResult<u32> {
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
fn i64_scalar_eval(lhs: u64, rhs: u64, kind: I64ScalarKind) -> VMResult<u64> {
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
fn f32_scalar_eval(lhs_bits: u32, rhs_bits: u32, kind: FloatScalarKind) -> u32 {
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
fn f64_scalar_eval(lhs_bits: u64, rhs_bits: u64, kind: FloatScalarKind) -> u64 {
    let lhs = f64::from_bits(lhs_bits);
    let rhs = f64::from_bits(rhs_bits);
    match kind {
        FloatScalarKind::Add => (lhs + rhs).to_bits(),
        FloatScalarKind::Sub => (lhs - rhs).to_bits(),
        FloatScalarKind::Mul => (lhs * rhs).to_bits(),
        FloatScalarKind::Div => (lhs / rhs).to_bits(),
    }
}

#[inline(always)]
fn i32_compare_eval(lhs: u32, rhs: u32, kind: IntCompareKind) -> u32 {
    bool_to_u32(match kind {
        IntCompareKind::Eq => lhs == rhs,
        IntCompareKind::Ne => lhs != rhs,
        IntCompareKind::LtS => (lhs as i32) < (rhs as i32),
        IntCompareKind::LtU => lhs < rhs,
        IntCompareKind::GtS => (lhs as i32) > (rhs as i32),
        IntCompareKind::GtU => lhs > rhs,
        IntCompareKind::LeS => (lhs as i32) <= (rhs as i32),
        IntCompareKind::LeU => lhs <= rhs,
        IntCompareKind::GeS => (lhs as i32) >= (rhs as i32),
        IntCompareKind::GeU => lhs >= rhs,
    })
}

#[inline(always)]
fn i64_compare_eval(lhs: u64, rhs: u64, kind: IntCompareKind) -> u32 {
    bool_to_u32(match kind {
        IntCompareKind::Eq => lhs == rhs,
        IntCompareKind::Ne => lhs != rhs,
        IntCompareKind::LtS => (lhs as i64) < (rhs as i64),
        IntCompareKind::LtU => lhs < rhs,
        IntCompareKind::GtS => (lhs as i64) > (rhs as i64),
        IntCompareKind::GtU => lhs > rhs,
        IntCompareKind::LeS => (lhs as i64) <= (rhs as i64),
        IntCompareKind::LeU => lhs <= rhs,
        IntCompareKind::GeS => (lhs as i64) >= (rhs as i64),
        IntCompareKind::GeU => lhs >= rhs,
    })
}

#[inline(always)]
fn f32_compare_eval(lhs_bits: u32, rhs_bits: u32, kind: FloatCompareKind) -> u32 {
    let lhs = f32::from_bits(lhs_bits);
    let rhs = f32::from_bits(rhs_bits);
    bool_to_u32(match kind {
        FloatCompareKind::Eq => lhs == rhs,
        FloatCompareKind::Ne => lhs != rhs,
        FloatCompareKind::Lt => lhs < rhs,
        FloatCompareKind::Gt => lhs > rhs,
        FloatCompareKind::Le => lhs <= rhs,
        FloatCompareKind::Ge => lhs >= rhs,
    })
}

#[inline(always)]
fn f64_compare_eval(lhs_bits: u64, rhs_bits: u64, kind: FloatCompareKind) -> u32 {
    let lhs = f64::from_bits(lhs_bits);
    let rhs = f64::from_bits(rhs_bits);
    bool_to_u32(match kind {
        FloatCompareKind::Eq => lhs == rhs,
        FloatCompareKind::Ne => lhs != rhs,
        FloatCompareKind::Lt => lhs < rhs,
        FloatCompareKind::Gt => lhs > rhs,
        FloatCompareKind::Le => lhs <= rhs,
        FloatCompareKind::Ge => lhs >= rhs,
    })
}

#[inline(always)]
fn select4_with_condition(ctx: &mut ExecuteContext, cond: u32) {
    ctx.stack.select_top_u32(cond);
}

#[inline(always)]
fn select8_with_condition(ctx: &mut ExecuteContext, cond: u32) {
    ctx.stack.select_top_u64(cond);
}

#[inline(always)]
fn store4_bytes(value: u32, kind: Store4Kind) -> StoreBytes {
    match kind {
        Store4Kind::I32 | Store4Kind::F32 => StoreBytes::Write4(value.to_le_bytes()),
        Store4Kind::I32Store8 => StoreBytes::Write1([(value & 0xff) as u8]),
        Store4Kind::I32Store16 => {
            StoreBytes::Write2([(value & 0xff) as u8, ((value >> 8) & 0xff) as u8])
        }
    }
}

#[inline(always)]
fn store8_bytes(value: u64, kind: Store8Kind) -> StoreBytes {
    match kind {
        Store8Kind::I64 | Store8Kind::F64 => StoreBytes::Write8(value.to_le_bytes()),
        Store8Kind::I64Store8 => StoreBytes::Write1([(value & 0xff) as u8]),
        Store8Kind::I64Store16 => {
            StoreBytes::Write2([(value & 0xff) as u8, ((value >> 8) & 0xff) as u8])
        }
        Store8Kind::I64Store32 => StoreBytes::Write4([
            (value & 0xff) as u8,
            ((value >> 8) & 0xff) as u8,
            ((value >> 16) & 0xff) as u8,
            ((value >> 24) & 0xff) as u8,
        ]),
    }
}

#[inline(always)]
unsafe fn read_local_load4_kind(
    ctx: &mut ExecuteContext,
    start: usize,
    kind: Load4Kind,
) -> VMResult<u32> {
    match kind {
        Load4Kind::I32 => ctx
            .gc
            .local_read_u32_at(ctx.default_local_memory_id_unchecked(), start),
        Load4Kind::I32Load8S => VMResult::Success(i32::from(vm_try!(ctx
            .gc
            .local_read_i8_at(ctx.default_local_memory_id_unchecked(), start,)))
            as u32),
        Load4Kind::I32Load8U => VMResult::Success(u32::from(vm_try!(ctx
            .gc
            .local_read_u8_at(ctx.default_local_memory_id_unchecked(), start)))),
        Load4Kind::I32Load16S => VMResult::Success(i32::from(vm_try!(ctx
            .gc
            .local_read_i16_at(ctx.default_local_memory_id_unchecked(), start,)))
            as u32),
        Load4Kind::I32Load16U => VMResult::Success(u32::from(vm_try!(ctx
            .gc
            .local_read_u16_at(ctx.default_local_memory_id_unchecked(), start)))),
        Load4Kind::F32 => ctx
            .gc
            .local_read_u32_at(ctx.default_local_memory_id_unchecked(), start),
    }
}

#[inline(always)]
unsafe fn read_local_load8_kind(
    ctx: &mut ExecuteContext,
    start: usize,
    kind: Load8Kind,
) -> VMResult<u64> {
    match kind {
        Load8Kind::I64 => ctx
            .gc
            .local_read_u64_at(ctx.default_local_memory_id_unchecked(), start),
        Load8Kind::I64Load8S => VMResult::Success(i64::from(vm_try!(ctx
            .gc
            .local_read_i8_at(ctx.default_local_memory_id_unchecked(), start,)))
            as u64),
        Load8Kind::I64Load8U => VMResult::Success(u64::from(vm_try!(ctx
            .gc
            .local_read_u8_at(ctx.default_local_memory_id_unchecked(), start)))),
        Load8Kind::I64Load16S => VMResult::Success(i64::from(vm_try!(ctx
            .gc
            .local_read_i16_at(ctx.default_local_memory_id_unchecked(), start,)))
            as u64),
        Load8Kind::I64Load16U => VMResult::Success(u64::from(vm_try!(ctx
            .gc
            .local_read_u16_at(ctx.default_local_memory_id_unchecked(), start)))),
        Load8Kind::I64Load32S => VMResult::Success(i64::from(vm_try!(ctx
            .gc
            .local_read_i32_at(ctx.default_local_memory_id_unchecked(), start,)))
            as u64),
        Load8Kind::I64Load32U => VMResult::Success(u64::from(vm_try!(ctx
            .gc
            .local_read_u32_at(ctx.default_local_memory_id_unchecked(), start)))),
        Load8Kind::F64 => ctx
            .gc
            .local_read_u64_at(ctx.default_local_memory_id_unchecked(), start),
    }
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

enum NarrowCopyKind {
    Load8Store8,
    Load16Store16,
}

enum ControlBranchKind {
    BrIf,
    If,
}

#[inline(always)]
unsafe fn branch_target_relative(
    tail_code: *const Instr,
    ctx: &ExecuteContext,
    jump_slot: usize,
    fallthrough_operands: isize,
    branch_kind: ControlBranchKind,
    taken: bool,
) -> *const Instr {
    let target = (*tail_code.add(jump_slot)).operand.jump_addr;
    match (branch_kind, taken) {
        (ControlBranchKind::BrIf, true) => ctx.code().offset(target as isize),
        (ControlBranchKind::BrIf, false) => tail_code.offset(fallthrough_operands),
        (ControlBranchKind::If, true) => tail_code.offset(fallthrough_operands),
        (ControlBranchKind::If, false) => ctx.code().offset(target as isize),
    }
}

#[inline(always)]
unsafe fn branch_target_ptr(
    tail_code: *const Instr,
    jump_slot: usize,
    fallthrough_operands: isize,
    branch_kind: ControlBranchKind,
    taken: bool,
) -> *const Instr {
    let target = (*tail_code.add(jump_slot)).operand.code_ptr as *const Instr;
    match (branch_kind, taken) {
        (ControlBranchKind::BrIf, true) => target,
        (ControlBranchKind::BrIf, false) => tail_code.offset(fallthrough_operands),
        (ControlBranchKind::If, true) => tail_code.offset(fallthrough_operands),
        (ControlBranchKind::If, false) => target,
    }
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
unsafe fn op_local_branch_u32(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    zero_test: bool,
    branch_kind: ControlBranchKind,
) -> VMResult<()> {
    let local_addr = (*tail_code).operand.local_addr;
    let cond = local_u32(ctx.stack, &ctx.local_reference(), local_addr) == 0;
    let taken = if zero_test { cond } else { !cond };
    let ptr = branch_target_relative(tail_code, ctx, 1, 2, branch_kind, taken);
    call_next(ptr, 0, ctx)
}

#[inline(always)]
unsafe fn op_local_branch_u32_ptr(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    zero_test: bool,
    branch_kind: ControlBranchKind,
) -> VMResult<()> {
    let local_addr = (*tail_code).operand.local_addr;
    let cond = local_u32(ctx.stack, &ctx.local_reference(), local_addr) == 0;
    let taken = if zero_test { cond } else { !cond };
    let ptr = branch_target_ptr(tail_code, 1, 2, branch_kind, taken);
    call_next(ptr, 0, ctx)
}

pub unsafe fn op_i32_local_br_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_local_branch_u32(tail_code, ctx, false, ControlBranchKind::BrIf)
}

pub unsafe fn op_i32_local_eqz_br_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_local_branch_u32(tail_code, ctx, true, ControlBranchKind::BrIf)
}

pub unsafe fn op_i32_local_if(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    op_local_branch_u32(tail_code, ctx, false, ControlBranchKind::If)
}

pub unsafe fn op_i32_local_eqz_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_local_branch_u32(tail_code, ctx, true, ControlBranchKind::If)
}

/// Telomere runtime helper `op_i32_local_br_if_ptr`.
///
/// Stack effect: `internal local branch dispatch`.
/// # Safety
/// - `tail_code` must point at the pointer-bearing operands for this specialized branch.
/// - `ctx` must reference a live execution context for the same validated frame and store.
pub unsafe fn op_i32_local_br_if_ptr(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_local_branch_u32_ptr(tail_code, ctx, false, ControlBranchKind::BrIf)
}

/// Telomere runtime helper `op_i32_local_eqz_br_if_ptr`.
///
/// Stack effect: `internal local branch dispatch`.
/// # Safety
/// - `tail_code` must point at the pointer-bearing operands for this specialized branch.
/// - `ctx` must reference a live execution context for the same validated frame and store.
pub unsafe fn op_i32_local_eqz_br_if_ptr(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_local_branch_u32_ptr(tail_code, ctx, true, ControlBranchKind::BrIf)
}

/// Telomere runtime helper `op_i32_local_if_ptr`.
///
/// Stack effect: `internal local branch dispatch`.
/// # Safety
/// - `tail_code` must point at the pointer-bearing operands for this specialized branch.
/// - `ctx` must reference a live execution context for the same validated frame and store.
pub unsafe fn op_i32_local_if_ptr(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_local_branch_u32_ptr(tail_code, ctx, false, ControlBranchKind::If)
}

/// Telomere runtime helper `op_i32_local_eqz_if_ptr`.
///
/// Stack effect: `internal local branch dispatch`.
/// # Safety
/// - `tail_code` must point at the pointer-bearing operands for this specialized branch.
/// - `ctx` must reference a live execution context for the same validated frame and store.
pub unsafe fn op_i32_local_eqz_if_ptr(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_local_branch_u32_ptr(tail_code, ctx, true, ControlBranchKind::If)
}

#[inline(always)]
unsafe fn op_local_branch_u64(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    zero_test: bool,
    branch_kind: ControlBranchKind,
) -> VMResult<()> {
    let local_addr = (*tail_code).operand.local_addr;
    let cond = local_u64(ctx.stack, &ctx.local_reference(), local_addr) == 0;
    let taken = if zero_test { cond } else { !cond };
    let ptr = branch_target_relative(tail_code, ctx, 1, 2, branch_kind, taken);
    call_next(ptr, 0, ctx)
}

#[inline(always)]
unsafe fn op_local_branch_u64_ptr(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    zero_test: bool,
    branch_kind: ControlBranchKind,
) -> VMResult<()> {
    let local_addr = (*tail_code).operand.local_addr;
    let cond = local_u64(ctx.stack, &ctx.local_reference(), local_addr) == 0;
    let taken = if zero_test { cond } else { !cond };
    let ptr = branch_target_ptr(tail_code, 1, 2, branch_kind, taken);
    call_next(ptr, 0, ctx)
}

pub unsafe fn op_i64_local_br_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_local_branch_u64(tail_code, ctx, false, ControlBranchKind::BrIf)
}

pub unsafe fn op_i64_local_eqz_br_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_local_branch_u64(tail_code, ctx, true, ControlBranchKind::BrIf)
}

pub unsafe fn op_i64_local_if(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    op_local_branch_u64(tail_code, ctx, false, ControlBranchKind::If)
}

pub unsafe fn op_i64_local_eqz_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_local_branch_u64(tail_code, ctx, true, ControlBranchKind::If)
}

/// Telomere runtime helper `op_i64_local_br_if_ptr`.
///
/// Stack effect: `internal local branch dispatch`.
/// # Safety
/// - `tail_code` must point at the pointer-bearing operands for this specialized branch.
/// - `ctx` must reference a live execution context for the same validated frame and store.
pub unsafe fn op_i64_local_br_if_ptr(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_local_branch_u64_ptr(tail_code, ctx, false, ControlBranchKind::BrIf)
}

/// Telomere runtime helper `op_i64_local_eqz_br_if_ptr`.
///
/// Stack effect: `internal local branch dispatch`.
/// # Safety
/// - `tail_code` must point at the pointer-bearing operands for this specialized branch.
/// - `ctx` must reference a live execution context for the same validated frame and store.
pub unsafe fn op_i64_local_eqz_br_if_ptr(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_local_branch_u64_ptr(tail_code, ctx, true, ControlBranchKind::BrIf)
}

/// Telomere runtime helper `op_i64_local_if_ptr`.
///
/// Stack effect: `internal local branch dispatch`.
/// # Safety
/// - `tail_code` must point at the pointer-bearing operands for this specialized branch.
/// - `ctx` must reference a live execution context for the same validated frame and store.
pub unsafe fn op_i64_local_if_ptr(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_local_branch_u64_ptr(tail_code, ctx, false, ControlBranchKind::If)
}

/// Telomere runtime helper `op_i64_local_eqz_if_ptr`.
///
/// Stack effect: `internal local branch dispatch`.
/// # Safety
/// - `tail_code` must point at the pointer-bearing operands for this specialized branch.
/// - `ctx` must reference a live execution context for the same validated frame and store.
pub unsafe fn op_i64_local_eqz_if_ptr(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_local_branch_u64_ptr(tail_code, ctx, true, ControlBranchKind::If)
}

#[inline(always)]
unsafe fn op_i32_local_and_imm_branch(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    zero_test: bool,
    branch_kind: ControlBranchKind,
) -> VMResult<()> {
    let local_addr = (*tail_code).operand.local_addr;
    let imm = (*tail_code.add(1)).operand.i32 as u32;
    let cond = (local_u32(ctx.stack, &ctx.local_reference(), local_addr) & imm) == 0;
    let taken = if zero_test { cond } else { !cond };
    let ptr = branch_target_relative(tail_code, ctx, 2, 3, branch_kind, taken);
    call_next(ptr, 0, ctx)
}

#[inline(always)]
unsafe fn op_i32_local_and_imm_branch_ptr(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    zero_test: bool,
    branch_kind: ControlBranchKind,
) -> VMResult<()> {
    let local_addr = (*tail_code).operand.local_addr;
    let imm = (*tail_code.add(1)).operand.i32 as u32;
    let cond = (local_u32(ctx.stack, &ctx.local_reference(), local_addr) & imm) == 0;
    let taken = if zero_test { cond } else { !cond };
    let ptr = branch_target_ptr(tail_code, 2, 3, branch_kind, taken);
    call_next(ptr, 0, ctx)
}

pub unsafe fn op_i32_local_and_imm_br_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_i32_local_and_imm_branch(tail_code, ctx, false, ControlBranchKind::BrIf)
}

pub unsafe fn op_i32_local_and_imm_eqz_br_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_i32_local_and_imm_branch(tail_code, ctx, true, ControlBranchKind::BrIf)
}

pub unsafe fn op_i32_local_and_imm_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_i32_local_and_imm_branch(tail_code, ctx, false, ControlBranchKind::If)
}

pub unsafe fn op_i32_local_and_imm_eqz_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_i32_local_and_imm_branch(tail_code, ctx, true, ControlBranchKind::If)
}

/// Telomere runtime helper `op_i32_local_and_imm_br_if_ptr`.
///
/// Stack effect: `internal local+imm branch dispatch`.
/// # Safety
/// - `tail_code` must point at the pointer-bearing operands for this specialized branch.
/// - `ctx` must reference a live execution context for the same validated frame and store.
pub unsafe fn op_i32_local_and_imm_br_if_ptr(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_i32_local_and_imm_branch_ptr(tail_code, ctx, false, ControlBranchKind::BrIf)
}

/// Telomere runtime helper `op_i32_local_and_imm_eqz_br_if_ptr`.
///
/// Stack effect: `internal local+imm branch dispatch`.
/// # Safety
/// - `tail_code` must point at the pointer-bearing operands for this specialized branch.
/// - `ctx` must reference a live execution context for the same validated frame and store.
pub unsafe fn op_i32_local_and_imm_eqz_br_if_ptr(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_i32_local_and_imm_branch_ptr(tail_code, ctx, true, ControlBranchKind::BrIf)
}

/// Telomere runtime helper `op_i32_local_and_imm_if_ptr`.
///
/// Stack effect: `internal local+imm branch dispatch`.
/// # Safety
/// - `tail_code` must point at the pointer-bearing operands for this specialized branch.
/// - `ctx` must reference a live execution context for the same validated frame and store.
pub unsafe fn op_i32_local_and_imm_if_ptr(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_i32_local_and_imm_branch_ptr(tail_code, ctx, false, ControlBranchKind::If)
}

/// Telomere runtime helper `op_i32_local_and_imm_eqz_if_ptr`.
///
/// Stack effect: `internal local+imm branch dispatch`.
/// # Safety
/// - `tail_code` must point at the pointer-bearing operands for this specialized branch.
/// - `ctx` must reference a live execution context for the same validated frame and store.
pub unsafe fn op_i32_local_and_imm_eqz_if_ptr(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_i32_local_and_imm_branch_ptr(tail_code, ctx, true, ControlBranchKind::If)
}

#[inline(always)]
unsafe fn op_i32_local_addr_load8_u_and_imm_eqz_branch(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    branch_kind: ControlBranchKind,
) -> VMResult<()> {
    let local_addr = (*tail_code).operand.local_addr;
    let memarg = (*tail_code.add(1)).operand.memarg;
    let imm = (*tail_code.add(2)).operand.i32 as u32;
    let start = vm_try!(local_mem_start_from_local(ctx, local_addr, memarg));
    let loaded = u32::from(vm_try!(ctx
        .gc
        .local_read_u8_at(ctx.default_local_memory_id_unchecked(), start)));
    let taken = (loaded & imm) == 0;
    let ptr = branch_target_relative(tail_code, ctx, 3, 4, branch_kind, taken);
    call_next(ptr, 0, ctx)
}

#[inline(always)]
unsafe fn op_i32_local_addr_load8_u_and_imm_eqz_branch_ptr(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    branch_kind: ControlBranchKind,
) -> VMResult<()> {
    let local_addr = (*tail_code).operand.local_addr;
    let memarg = (*tail_code.add(1)).operand.memarg;
    let imm = (*tail_code.add(2)).operand.i32 as u32;
    let start = vm_try!(local_mem_start_from_local(ctx, local_addr, memarg));
    let loaded = u32::from(vm_try!(ctx
        .gc
        .local_read_u8_at(ctx.default_local_memory_id_unchecked(), start)));
    let taken = (loaded & imm) == 0;
    let ptr = branch_target_ptr(tail_code, 3, 4, branch_kind, taken);
    call_next(ptr, 0, ctx)
}

#[cold]
#[inline(never)]
pub unsafe fn op_i32_local_addr_load8_u_and_imm_eqz_br_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_i32_local_addr_load8_u_and_imm_eqz_branch(tail_code, ctx, ControlBranchKind::BrIf)
}

#[cold]
#[inline(never)]
pub unsafe fn op_i32_local_addr_load8_u_and_imm_eqz_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_i32_local_addr_load8_u_and_imm_eqz_branch(tail_code, ctx, ControlBranchKind::If)
}

#[cold]
#[inline(never)]
/// Telomere runtime helper `op_i32_local_addr_load8_u_and_imm_eqz_br_if_ptr`.
///
/// Stack effect: `internal load+mask branch dispatch`.
/// # Safety
/// - `tail_code` must point at the pointer-bearing operands for this specialized branch.
/// - `ctx` must reference a live execution context for the same validated frame and store.
pub unsafe fn op_i32_local_addr_load8_u_and_imm_eqz_br_if_ptr(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_i32_local_addr_load8_u_and_imm_eqz_branch_ptr(tail_code, ctx, ControlBranchKind::BrIf)
}

#[cold]
#[inline(never)]
/// Telomere runtime helper `op_i32_local_addr_load8_u_and_imm_eqz_if_ptr`.
///
/// Stack effect: `internal load+mask branch dispatch`.
/// # Safety
/// - `tail_code` must point at the pointer-bearing operands for this specialized branch.
/// - `ctx` must reference a live execution context for the same validated frame and store.
pub unsafe fn op_i32_local_addr_load8_u_and_imm_eqz_if_ptr(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_i32_local_addr_load8_u_and_imm_eqz_branch_ptr(tail_code, ctx, ControlBranchKind::If)
}

#[inline(always)]
unsafe fn op_seed_tee_eqz_branch_u32<const PTR_TARGET: bool>(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    branch_kind: ControlBranchKind,
) -> VMResult<()> {
    let value = vm_try!(producer_seed_u32(tail_code, ctx));
    write_local_u32(
        ctx.stack,
        &ctx.local_reference(),
        (*tail_code.add(5)).operand.local_addr,
        value,
    );
    let ptr = if PTR_TARGET {
        branch_target_ptr(tail_code, 6, 7, branch_kind, value == 0)
    } else {
        branch_target_relative(tail_code, ctx, 6, 7, branch_kind, value == 0)
    };
    call_next(ptr, 0, ctx)
}

#[inline(always)]
unsafe fn op_seed_tee_eqz_branch_u64<const PTR_TARGET: bool>(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    branch_kind: ControlBranchKind,
) -> VMResult<()> {
    let value = vm_try!(producer_seed_u64(tail_code, ctx));
    write_local_u64(
        ctx.stack,
        &ctx.local_reference(),
        (*tail_code.add(5)).operand.local_addr,
        value,
    );
    let ptr = if PTR_TARGET {
        branch_target_ptr(tail_code, 6, 7, branch_kind, value == 0)
    } else {
        branch_target_relative(tail_code, ctx, 6, 7, branch_kind, value == 0)
    };
    call_next(ptr, 0, ctx)
}

#[inline(always)]
unsafe fn op_seed_tee_imm_compare_branch_u32<const PTR_TARGET: bool>(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    branch_kind: ControlBranchKind,
) -> VMResult<()> {
    let value = vm_try!(producer_seed_u32(tail_code, ctx));
    write_local_u32(
        ctx.stack,
        &ctx.local_reference(),
        (*tail_code.add(5)).operand.local_addr,
        value,
    );
    let taken = i32_compare_eval(
        value,
        (*tail_code.add(6)).operand.u32,
        IntCompareKind::from_raw((*tail_code.add(8)).operand.u32),
    ) != 0;
    let ptr = if PTR_TARGET {
        branch_target_ptr(tail_code, 7, 9, branch_kind, taken)
    } else {
        branch_target_relative(tail_code, ctx, 7, 9, branch_kind, taken)
    };
    call_next(ptr, 0, ctx)
}

#[inline(always)]
unsafe fn op_seed_tee_imm_compare_branch_u64<const PTR_TARGET: bool>(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    branch_kind: ControlBranchKind,
) -> VMResult<()> {
    let value = vm_try!(producer_seed_u64(tail_code, ctx));
    write_local_u64(
        ctx.stack,
        &ctx.local_reference(),
        (*tail_code.add(5)).operand.local_addr,
        value,
    );
    let taken = i64_compare_eval(
        value,
        (*tail_code.add(6)).operand.u64,
        IntCompareKind::from_raw((*tail_code.add(8)).operand.u32),
    ) != 0;
    let ptr = if PTR_TARGET {
        branch_target_ptr(tail_code, 7, 9, branch_kind, taken)
    } else {
        branch_target_relative(tail_code, ctx, 7, 9, branch_kind, taken)
    };
    call_next(ptr, 0, ctx)
}

pub unsafe fn op_i32_seed_tee_eqz_br_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_seed_tee_eqz_branch_u32::<false>(tail_code, ctx, ControlBranchKind::BrIf)
}

pub unsafe fn op_i32_seed_tee_eqz_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_seed_tee_eqz_branch_u32::<false>(tail_code, ctx, ControlBranchKind::If)
}

pub unsafe fn op_i32_seed_tee_eqz_br_if_ptr(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_seed_tee_eqz_branch_u32::<true>(tail_code, ctx, ControlBranchKind::BrIf)
}

pub unsafe fn op_i32_seed_tee_eqz_if_ptr(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_seed_tee_eqz_branch_u32::<true>(tail_code, ctx, ControlBranchKind::If)
}

pub unsafe fn op_i64_seed_tee_eqz_br_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_seed_tee_eqz_branch_u64::<false>(tail_code, ctx, ControlBranchKind::BrIf)
}

pub unsafe fn op_i64_seed_tee_eqz_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_seed_tee_eqz_branch_u64::<false>(tail_code, ctx, ControlBranchKind::If)
}

pub unsafe fn op_i64_seed_tee_eqz_br_if_ptr(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_seed_tee_eqz_branch_u64::<true>(tail_code, ctx, ControlBranchKind::BrIf)
}

pub unsafe fn op_i64_seed_tee_eqz_if_ptr(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_seed_tee_eqz_branch_u64::<true>(tail_code, ctx, ControlBranchKind::If)
}

pub unsafe fn op_i32_seed_tee_imm_compare_br_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_seed_tee_imm_compare_branch_u32::<false>(tail_code, ctx, ControlBranchKind::BrIf)
}

pub unsafe fn op_i32_seed_tee_imm_compare_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_seed_tee_imm_compare_branch_u32::<false>(tail_code, ctx, ControlBranchKind::If)
}

pub unsafe fn op_i32_seed_tee_imm_compare_br_if_ptr(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_seed_tee_imm_compare_branch_u32::<true>(tail_code, ctx, ControlBranchKind::BrIf)
}

pub unsafe fn op_i32_seed_tee_imm_compare_if_ptr(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_seed_tee_imm_compare_branch_u32::<true>(tail_code, ctx, ControlBranchKind::If)
}

pub unsafe fn op_i64_seed_tee_imm_compare_br_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_seed_tee_imm_compare_branch_u64::<false>(tail_code, ctx, ControlBranchKind::BrIf)
}

pub unsafe fn op_i64_seed_tee_imm_compare_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_seed_tee_imm_compare_branch_u64::<false>(tail_code, ctx, ControlBranchKind::If)
}

pub unsafe fn op_i64_seed_tee_imm_compare_br_if_ptr(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_seed_tee_imm_compare_branch_u64::<true>(tail_code, ctx, ControlBranchKind::BrIf)
}

pub unsafe fn op_i64_seed_tee_imm_compare_if_ptr(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_seed_tee_imm_compare_branch_u64::<true>(tail_code, ctx, ControlBranchKind::If)
}

#[inline(always)]
unsafe fn op_seed_imm_and_branch_u32<const ZERO_TEST: bool, const PTR_TARGET: bool>(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    branch_kind: ControlBranchKind,
) -> VMResult<()> {
    let value = vm_try!(producer_seed_u32(tail_code, ctx));
    let imm = (*tail_code.add(5)).operand.u32;
    let cond = (value & imm) == 0;
    let taken = if ZERO_TEST { cond } else { !cond };
    let ptr = if PTR_TARGET {
        branch_target_ptr(tail_code, 6, 7, branch_kind, taken)
    } else {
        branch_target_relative(tail_code, ctx, 6, 7, branch_kind, taken)
    };
    call_next(ptr, 0, ctx)
}

#[inline(always)]
unsafe fn op_seed_imm_and_branch_u64<const ZERO_TEST: bool, const PTR_TARGET: bool>(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    branch_kind: ControlBranchKind,
) -> VMResult<()> {
    let value = vm_try!(producer_seed_u64(tail_code, ctx));
    let imm = (*tail_code.add(5)).operand.u64;
    let cond = (value & imm) == 0;
    let taken = if ZERO_TEST { cond } else { !cond };
    let ptr = if PTR_TARGET {
        branch_target_ptr(tail_code, 6, 7, branch_kind, taken)
    } else {
        branch_target_relative(tail_code, ctx, 6, 7, branch_kind, taken)
    };
    call_next(ptr, 0, ctx)
}

pub unsafe fn op_i32_seed_imm_and_br_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_seed_imm_and_branch_u32::<false, false>(tail_code, ctx, ControlBranchKind::BrIf)
}

pub unsafe fn op_i32_seed_imm_and_eqz_br_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_seed_imm_and_branch_u32::<true, false>(tail_code, ctx, ControlBranchKind::BrIf)
}

pub unsafe fn op_i32_seed_imm_and_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_seed_imm_and_branch_u32::<false, false>(tail_code, ctx, ControlBranchKind::If)
}

pub unsafe fn op_i32_seed_imm_and_eqz_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_seed_imm_and_branch_u32::<true, false>(tail_code, ctx, ControlBranchKind::If)
}

/// Telomere runtime helper `op_i32_seed_imm_and_br_if_ptr`.
///
/// Stack effect: `internal producer+mask branch dispatch`.
/// # Safety
/// - `tail_code` must point at the pointer-bearing operands for this specialized branch.
/// - `ctx` must reference a live execution context for the same validated frame and store.
pub unsafe fn op_i32_seed_imm_and_br_if_ptr(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_seed_imm_and_branch_u32::<false, true>(tail_code, ctx, ControlBranchKind::BrIf)
}

/// Telomere runtime helper `op_i32_seed_imm_and_eqz_br_if_ptr`.
///
/// Stack effect: `internal producer+mask branch dispatch`.
/// # Safety
/// - `tail_code` must point at the pointer-bearing operands for this specialized branch.
/// - `ctx` must reference a live execution context for the same validated frame and store.
pub unsafe fn op_i32_seed_imm_and_eqz_br_if_ptr(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_seed_imm_and_branch_u32::<true, true>(tail_code, ctx, ControlBranchKind::BrIf)
}

/// Telomere runtime helper `op_i32_seed_imm_and_if_ptr`.
///
/// Stack effect: `internal producer+mask branch dispatch`.
/// # Safety
/// - `tail_code` must point at the pointer-bearing operands for this specialized branch.
/// - `ctx` must reference a live execution context for the same validated frame and store.
pub unsafe fn op_i32_seed_imm_and_if_ptr(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_seed_imm_and_branch_u32::<false, true>(tail_code, ctx, ControlBranchKind::If)
}

/// Telomere runtime helper `op_i32_seed_imm_and_eqz_if_ptr`.
///
/// Stack effect: `internal producer+mask branch dispatch`.
/// # Safety
/// - `tail_code` must point at the pointer-bearing operands for this specialized branch.
/// - `ctx` must reference a live execution context for the same validated frame and store.
pub unsafe fn op_i32_seed_imm_and_eqz_if_ptr(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_seed_imm_and_branch_u32::<true, true>(tail_code, ctx, ControlBranchKind::If)
}

pub unsafe fn op_i64_seed_imm_and_br_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_seed_imm_and_branch_u64::<false, false>(tail_code, ctx, ControlBranchKind::BrIf)
}

pub unsafe fn op_i64_seed_imm_and_eqz_br_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_seed_imm_and_branch_u64::<true, false>(tail_code, ctx, ControlBranchKind::BrIf)
}

pub unsafe fn op_i64_seed_imm_and_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_seed_imm_and_branch_u64::<false, false>(tail_code, ctx, ControlBranchKind::If)
}

pub unsafe fn op_i64_seed_imm_and_eqz_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_seed_imm_and_branch_u64::<true, false>(tail_code, ctx, ControlBranchKind::If)
}

/// Telomere runtime helper `op_i64_seed_imm_and_br_if_ptr`.
///
/// Stack effect: `internal producer+mask branch dispatch`.
/// # Safety
/// - `tail_code` must point at the pointer-bearing operands for this specialized branch.
/// - `ctx` must reference a live execution context for the same validated frame and store.
pub unsafe fn op_i64_seed_imm_and_br_if_ptr(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_seed_imm_and_branch_u64::<false, true>(tail_code, ctx, ControlBranchKind::BrIf)
}

/// Telomere runtime helper `op_i64_seed_imm_and_eqz_br_if_ptr`.
///
/// Stack effect: `internal producer+mask branch dispatch`.
/// # Safety
/// - `tail_code` must point at the pointer-bearing operands for this specialized branch.
/// - `ctx` must reference a live execution context for the same validated frame and store.
pub unsafe fn op_i64_seed_imm_and_eqz_br_if_ptr(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_seed_imm_and_branch_u64::<true, true>(tail_code, ctx, ControlBranchKind::BrIf)
}

/// Telomere runtime helper `op_i64_seed_imm_and_if_ptr`.
///
/// Stack effect: `internal producer+mask branch dispatch`.
/// # Safety
/// - `tail_code` must point at the pointer-bearing operands for this specialized branch.
/// - `ctx` must reference a live execution context for the same validated frame and store.
pub unsafe fn op_i64_seed_imm_and_if_ptr(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_seed_imm_and_branch_u64::<false, true>(tail_code, ctx, ControlBranchKind::If)
}

/// Telomere runtime helper `op_i64_seed_imm_and_eqz_if_ptr`.
///
/// Stack effect: `internal producer+mask branch dispatch`.
/// # Safety
/// - `tail_code` must point at the pointer-bearing operands for this specialized branch.
/// - `ctx` must reference a live execution context for the same validated frame and store.
pub unsafe fn op_i64_seed_imm_and_eqz_if_ptr(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_seed_imm_and_branch_u64::<true, true>(tail_code, ctx, ControlBranchKind::If)
}

pub unsafe fn op_i32_local_local_ge_u_br_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lhs_local_addr = (*tail_code).operand.local_addr;
    let rhs_local_addr = (*tail_code.add(1)).operand.local_addr;
    let ptr = if local_u32(ctx.stack, &ctx.local_reference(), lhs_local_addr)
        >= local_u32(ctx.stack, &ctx.local_reference(), rhs_local_addr)
    {
        branch_target_relative(tail_code, ctx, 2, 3, ControlBranchKind::BrIf, true)
    } else {
        tail_code.offset(3)
    };
    call_next(ptr, 0, ctx)
}

/// Telomere runtime helper `op_i32_local_local_ge_u_br_if_ptr`.
///
/// Stack effect: `internal compare branch dispatch`.
/// # Safety
/// - `tail_code` must point at the pointer-bearing operands for this specialized branch.
/// - `ctx` must reference a live execution context for the same validated frame and store.
pub unsafe fn op_i32_local_local_ge_u_br_if_ptr(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lhs_local_addr = (*tail_code).operand.local_addr;
    let rhs_local_addr = (*tail_code.add(1)).operand.local_addr;
    let ptr = if local_u32(ctx.stack, &ctx.local_reference(), lhs_local_addr)
        >= local_u32(ctx.stack, &ctx.local_reference(), rhs_local_addr)
    {
        branch_target_ptr(tail_code, 2, 3, ControlBranchKind::BrIf, true)
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
    local_mem_start_from_local(ctx, local_addr, memarg)
}

#[inline(always)]
unsafe fn local_imm_addr_mem_start(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<usize> {
    let local_addr = (*tail_code).operand.local_addr;
    let imm = (*tail_code.add(1)).operand.i32 as u32;
    let memarg = (*tail_code.add(2)).operand.memarg;
    local_imm_addr_mem_start_from_parts(ctx, local_addr, imm, memarg)
}

#[inline(always)]
unsafe fn local_imm_addr_mem_start_from_parts(
    ctx: &mut ExecuteContext,
    local_addr: u32,
    imm: u32,
    memarg: MemArg,
) -> VMResult<usize> {
    compute_memory_offset(
        memarg,
        local_u32(ctx.stack, &ctx.local_reference(), local_addr).wrapping_add(imm),
    )
}

#[inline(always)]
unsafe fn local_mem_start_from_local(
    ctx: &mut ExecuteContext,
    local_addr: u32,
    memarg: MemArg,
) -> VMResult<usize> {
    compute_memory_offset(
        memarg,
        local_u32(ctx.stack, &ctx.local_reference(), local_addr),
    )
}

#[inline(always)]
unsafe fn producer_seed_u32(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<u32> {
    match ProducerSeedKind::from_raw((*tail_code).operand.u32) {
        ProducerSeedKind::Local => VMResult::Success(local_u32(
            ctx.stack,
            &ctx.local_reference(),
            (*tail_code.add(1)).operand.local_addr,
        )),
        ProducerSeedKind::LocalImmScalar => i32_scalar_eval(
            local_u32(
                ctx.stack,
                &ctx.local_reference(),
                (*tail_code.add(1)).operand.local_addr,
            ),
            (*tail_code.add(2)).operand.u32,
            I32ScalarKind::from_raw((*tail_code.add(3)).operand.u32),
        ),
        ProducerSeedKind::LocalLocalScalar => i32_scalar_eval(
            local_u32(
                ctx.stack,
                &ctx.local_reference(),
                (*tail_code.add(1)).operand.local_addr,
            ),
            local_u32(
                ctx.stack,
                &ctx.local_reference(),
                (*tail_code.add(2)).operand.local_addr,
            ),
            I32ScalarKind::from_raw((*tail_code.add(3)).operand.u32),
        ),
        ProducerSeedKind::LocalAddrLoad => {
            let start = vm_try!(local_mem_start_from_local(
                ctx,
                (*tail_code.add(1)).operand.local_addr,
                (*tail_code.add(2)).operand.memarg,
            ));
            read_local_load4_kind(
                ctx,
                start,
                Load4Kind::from_raw((*tail_code.add(3)).operand.u32),
            )
        }
        ProducerSeedKind::LocalImmAddrLoad => {
            let start = vm_try!(local_imm_addr_mem_start_from_parts(
                ctx,
                (*tail_code.add(1)).operand.local_addr,
                (*tail_code.add(2)).operand.i32 as u32,
                (*tail_code.add(3)).operand.memarg,
            ));
            read_local_load4_kind(
                ctx,
                start,
                Load4Kind::from_raw((*tail_code.add(4)).operand.u32),
            )
        }
        ProducerSeedKind::ConstAddrLoad => read_local_load4_kind(
            ctx,
            (*tail_code.add(1)).operand.u32 as usize,
            Load4Kind::from_raw((*tail_code.add(2)).operand.u32),
        ),
    }
}

#[inline(always)]
unsafe fn producer_seed_u64(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<u64> {
    match ProducerSeedKind::from_raw((*tail_code).operand.u32) {
        ProducerSeedKind::Local => VMResult::Success(local_u64(
            ctx.stack,
            &ctx.local_reference(),
            (*tail_code.add(1)).operand.local_addr,
        )),
        ProducerSeedKind::LocalImmScalar => i64_scalar_eval(
            local_u64(
                ctx.stack,
                &ctx.local_reference(),
                (*tail_code.add(1)).operand.local_addr,
            ),
            (*tail_code.add(2)).operand.u64,
            I64ScalarKind::from_raw((*tail_code.add(3)).operand.u32),
        ),
        ProducerSeedKind::LocalLocalScalar => i64_scalar_eval(
            local_u64(
                ctx.stack,
                &ctx.local_reference(),
                (*tail_code.add(1)).operand.local_addr,
            ),
            local_u64(
                ctx.stack,
                &ctx.local_reference(),
                (*tail_code.add(2)).operand.local_addr,
            ),
            I64ScalarKind::from_raw((*tail_code.add(3)).operand.u32),
        ),
        ProducerSeedKind::LocalAddrLoad => {
            let start = vm_try!(local_mem_start_from_local(
                ctx,
                (*tail_code.add(1)).operand.local_addr,
                (*tail_code.add(2)).operand.memarg,
            ));
            read_local_load8_kind(
                ctx,
                start,
                Load8Kind::from_raw((*tail_code.add(3)).operand.u32),
            )
        }
        ProducerSeedKind::LocalImmAddrLoad => {
            let start = vm_try!(local_imm_addr_mem_start_from_parts(
                ctx,
                (*tail_code.add(1)).operand.local_addr,
                (*tail_code.add(2)).operand.i32 as u32,
                (*tail_code.add(3)).operand.memarg,
            ));
            read_local_load8_kind(
                ctx,
                start,
                Load8Kind::from_raw((*tail_code.add(4)).operand.u32),
            )
        }
        ProducerSeedKind::ConstAddrLoad => read_local_load8_kind(
            ctx,
            (*tail_code.add(1)).operand.u32 as usize,
            Load8Kind::from_raw((*tail_code.add(2)).operand.u32),
        ),
    }
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

pub unsafe fn op_i32_local_imm_addr_load(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let start = vm_try!(local_imm_addr_mem_start(tail_code, ctx));
    let value = vm_try!(ctx
        .gc
        .local_read_u32_at(ctx.default_local_memory_id_unchecked(), start));
    vm_try!(ctx.stack.push_u32(value));
    call_next(tail_code, 3, ctx)
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

pub unsafe fn op_i32_local_imm_addr_load8_u(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let start = vm_try!(local_imm_addr_mem_start(tail_code, ctx));
    let value = vm_try!(ctx
        .gc
        .local_read_u8_at(ctx.default_local_memory_id_unchecked(), start));
    vm_try!(ctx.stack.push_u32(u32::from(value)));
    call_next(tail_code, 3, ctx)
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

pub unsafe fn op_i32_local_imm_addr_load16_s(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let start = vm_try!(local_imm_addr_mem_start(tail_code, ctx));
    let value = vm_try!(ctx
        .gc
        .local_read_i16_at(ctx.default_local_memory_id_unchecked(), start));
    vm_try!(ctx.stack.push_i32(i32::from(value)));
    call_next(tail_code, 3, ctx)
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

pub unsafe fn op_i32_local_imm_addr_load16_u(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let start = vm_try!(local_imm_addr_mem_start(tail_code, ctx));
    let value = vm_try!(ctx
        .gc
        .local_read_u16_at(ctx.default_local_memory_id_unchecked(), start));
    vm_try!(ctx.stack.push_u32(u32::from(value)));
    call_next(tail_code, 3, ctx)
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

pub unsafe fn op_f32_local_imm_addr_load(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let start = vm_try!(local_imm_addr_mem_start(tail_code, ctx));
    let value = vm_try!(ctx
        .gc
        .local_read_u32_at(ctx.default_local_memory_id_unchecked(), start));
    vm_try!(ctx.stack.push_u32(value));
    call_next(tail_code, 3, ctx)
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

#[inline(always)]
unsafe fn local_imm_local_store_start(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<usize> {
    let addr_local = (*tail_code).operand.local_addr;
    let imm = (*tail_code.add(1)).operand.i32 as u32;
    let memarg = (*tail_code.add(3)).operand.memarg;
    compute_memory_offset(
        memarg,
        local_u32(ctx.stack, &ctx.local_reference(), addr_local).wrapping_add(imm),
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

pub unsafe fn op_i32_local_imm_local_store(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let start = vm_try!(local_imm_local_store_start(tail_code, ctx));
    let value_local = (*tail_code.add(2)).operand.local_addr;
    let bytes = local_u32(ctx.stack, &ctx.local_reference(), value_local).to_le_bytes();
    vm_try!(ctx
        .gc
        .local_write_bytes(ctx.default_local_memory_id_unchecked(), start, &bytes));
    call_next(tail_code, 4, ctx)
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

pub unsafe fn op_i32_local_imm_local_store8(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let start = vm_try!(local_imm_local_store_start(tail_code, ctx));
    let value_local = (*tail_code.add(2)).operand.local_addr;
    let bytes = [(local_u32(ctx.stack, &ctx.local_reference(), value_local) & 0xff) as u8];
    vm_try!(ctx
        .gc
        .local_write_bytes(ctx.default_local_memory_id_unchecked(), start, &bytes));
    call_next(tail_code, 4, ctx)
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

pub unsafe fn op_i32_local_imm_local_store16(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let start = vm_try!(local_imm_local_store_start(tail_code, ctx));
    let value_local = (*tail_code.add(2)).operand.local_addr;
    let value = local_u32(ctx.stack, &ctx.local_reference(), value_local);
    let bytes = [(value & 0xff) as u8, ((value >> 8) & 0xff) as u8];
    vm_try!(ctx
        .gc
        .local_write_bytes(ctx.default_local_memory_id_unchecked(), start, &bytes));
    call_next(tail_code, 4, ctx)
}

#[cold]
#[inline(never)]
pub unsafe fn op_i32_local_local_load_tee_add_imm_store(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let store_addr_local = (*tail_code).operand.local_addr;
    let load_addr_local = (*tail_code.add(1)).operand.local_addr;
    let tee_local = (*tail_code.add(2)).operand.local_addr;
    let imm = (*tail_code.add(3)).operand.i32 as u32;
    let load_memarg = (*tail_code.add(4)).operand.memarg;
    let store_memarg = (*tail_code.add(5)).operand.memarg;
    let load_start = vm_try!(local_mem_start_from_local(
        ctx,
        load_addr_local,
        load_memarg
    ));
    let loaded = vm_try!(ctx
        .gc
        .local_read_u32_at(ctx.default_local_memory_id_unchecked(), load_start));
    write_local_u32(ctx.stack, &ctx.local_reference(), tee_local, loaded);
    let value = loaded.wrapping_add(imm);
    let store_start = vm_try!(local_mem_start_from_local(
        ctx,
        store_addr_local,
        store_memarg
    ));
    let bytes = value.to_le_bytes();
    vm_try!(ctx
        .gc
        .local_write_bytes(ctx.default_local_memory_id_unchecked(), store_start, &bytes));
    call_next(tail_code, 6, ctx)
}

#[cold]
#[inline(never)]
unsafe fn op_i32_local_local_narrow_copy(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    kind: NarrowCopyKind,
) -> VMResult<()> {
    let dst_local = (*tail_code).operand.local_addr;
    let src_local = (*tail_code.add(1)).operand.local_addr;
    let load_memarg = (*tail_code.add(2)).operand.memarg;
    let store_memarg = (*tail_code.add(3)).operand.memarg;
    let load_start = vm_try!(local_mem_start_from_local(ctx, src_local, load_memarg));
    let store_start = vm_try!(local_mem_start_from_local(ctx, dst_local, store_memarg));
    let bytes = match kind {
        NarrowCopyKind::Load8Store8 => StoreBytes::Write1([vm_try!(ctx
            .gc
            .local_read_u8_at(ctx.default_local_memory_id_unchecked(), load_start))]),
        NarrowCopyKind::Load16Store16 => StoreBytes::Write2(
            vm_try!(ctx
                .gc
                .local_read_u16_at(ctx.default_local_memory_id_unchecked(), load_start))
            .to_le_bytes(),
        ),
    };
    vm_try!(ctx.gc.local_write_bytes(
        ctx.default_local_memory_id_unchecked(),
        store_start,
        bytes.as_slice(),
    ));
    call_next(tail_code, 4, ctx)
}

#[cold]
#[inline(never)]
pub unsafe fn op_i32_local_local_load8_u_store8(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_i32_local_local_narrow_copy(tail_code, ctx, NarrowCopyKind::Load8Store8)
}

#[cold]
#[inline(never)]
pub unsafe fn op_i32_local_local_load16_u_store16(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_i32_local_local_narrow_copy(tail_code, ctx, NarrowCopyKind::Load16Store16)
}

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

pub unsafe fn op_i32_local_local_compare_set4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lhs_local = (*tail_code).operand.local_addr;
    let rhs_local = (*tail_code.add(1)).operand.local_addr;
    let dst_local = (*tail_code.add(2)).operand.local_addr;
    let kind = IntCompareKind::from_raw((*tail_code.add(3)).operand.u32);
    let value = i32_compare_eval(
        local_u32(ctx.stack, &ctx.local_reference(), lhs_local),
        local_u32(ctx.stack, &ctx.local_reference(), rhs_local),
        kind,
    );
    write_local_u32(ctx.stack, &ctx.local_reference(), dst_local, value);
    call_next(tail_code, 4, ctx)
}

pub unsafe fn op_i32_local_local_compare_tee4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lhs_local = (*tail_code).operand.local_addr;
    let rhs_local = (*tail_code.add(1)).operand.local_addr;
    let dst_local = (*tail_code.add(2)).operand.local_addr;
    let kind = IntCompareKind::from_raw((*tail_code.add(3)).operand.u32);
    let value = i32_compare_eval(
        local_u32(ctx.stack, &ctx.local_reference(), lhs_local),
        local_u32(ctx.stack, &ctx.local_reference(), rhs_local),
        kind,
    );
    write_local_u32(ctx.stack, &ctx.local_reference(), dst_local, value);
    vm_try!(ctx.stack.push_u32(value));
    call_next(tail_code, 4, ctx)
}

pub unsafe fn op_i32_local_const_compare_set4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lhs_local = (*tail_code).operand.local_addr;
    let rhs = (*tail_code.add(1)).operand.i32 as u32;
    let dst_local = (*tail_code.add(2)).operand.local_addr;
    let kind = IntCompareKind::from_raw((*tail_code.add(3)).operand.u32);
    let value = i32_compare_eval(
        local_u32(ctx.stack, &ctx.local_reference(), lhs_local),
        rhs,
        kind,
    );
    write_local_u32(ctx.stack, &ctx.local_reference(), dst_local, value);
    call_next(tail_code, 4, ctx)
}

pub unsafe fn op_i32_local_const_compare_tee4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lhs_local = (*tail_code).operand.local_addr;
    let rhs = (*tail_code.add(1)).operand.i32 as u32;
    let dst_local = (*tail_code.add(2)).operand.local_addr;
    let kind = IntCompareKind::from_raw((*tail_code.add(3)).operand.u32);
    let value = i32_compare_eval(
        local_u32(ctx.stack, &ctx.local_reference(), lhs_local),
        rhs,
        kind,
    );
    write_local_u32(ctx.stack, &ctx.local_reference(), dst_local, value);
    vm_try!(ctx.stack.push_u32(value));
    call_next(tail_code, 4, ctx)
}

pub unsafe fn op_i64_local_local_compare_set4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lhs_local = (*tail_code).operand.local_addr;
    let rhs_local = (*tail_code.add(1)).operand.local_addr;
    let dst_local = (*tail_code.add(2)).operand.local_addr;
    let kind = IntCompareKind::from_raw((*tail_code.add(3)).operand.u32);
    let value = i64_compare_eval(
        local_u64(ctx.stack, &ctx.local_reference(), lhs_local),
        local_u64(ctx.stack, &ctx.local_reference(), rhs_local),
        kind,
    );
    write_local_u32(ctx.stack, &ctx.local_reference(), dst_local, value);
    call_next(tail_code, 4, ctx)
}

pub unsafe fn op_i64_local_local_compare_tee4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lhs_local = (*tail_code).operand.local_addr;
    let rhs_local = (*tail_code.add(1)).operand.local_addr;
    let dst_local = (*tail_code.add(2)).operand.local_addr;
    let kind = IntCompareKind::from_raw((*tail_code.add(3)).operand.u32);
    let value = i64_compare_eval(
        local_u64(ctx.stack, &ctx.local_reference(), lhs_local),
        local_u64(ctx.stack, &ctx.local_reference(), rhs_local),
        kind,
    );
    write_local_u32(ctx.stack, &ctx.local_reference(), dst_local, value);
    vm_try!(ctx.stack.push_u32(value));
    call_next(tail_code, 4, ctx)
}

pub unsafe fn op_i64_local_const_compare_set4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lhs_local = (*tail_code).operand.local_addr;
    let rhs = (*tail_code.add(1)).operand.u64;
    let dst_local = (*tail_code.add(2)).operand.local_addr;
    let kind = IntCompareKind::from_raw((*tail_code.add(3)).operand.u32);
    let value = i64_compare_eval(
        local_u64(ctx.stack, &ctx.local_reference(), lhs_local),
        rhs,
        kind,
    );
    write_local_u32(ctx.stack, &ctx.local_reference(), dst_local, value);
    call_next(tail_code, 4, ctx)
}

pub unsafe fn op_i64_local_const_compare_tee4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lhs_local = (*tail_code).operand.local_addr;
    let rhs = (*tail_code.add(1)).operand.u64;
    let dst_local = (*tail_code.add(2)).operand.local_addr;
    let kind = IntCompareKind::from_raw((*tail_code.add(3)).operand.u32);
    let value = i64_compare_eval(
        local_u64(ctx.stack, &ctx.local_reference(), lhs_local),
        rhs,
        kind,
    );
    write_local_u32(ctx.stack, &ctx.local_reference(), dst_local, value);
    vm_try!(ctx.stack.push_u32(value));
    call_next(tail_code, 4, ctx)
}

pub unsafe fn op_f32_local_local_compare_set4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lhs_local = (*tail_code).operand.local_addr;
    let rhs_local = (*tail_code.add(1)).operand.local_addr;
    let dst_local = (*tail_code.add(2)).operand.local_addr;
    let kind = FloatCompareKind::from_raw((*tail_code.add(3)).operand.u32);
    let value = f32_compare_eval(
        local_u32(ctx.stack, &ctx.local_reference(), lhs_local),
        local_u32(ctx.stack, &ctx.local_reference(), rhs_local),
        kind,
    );
    write_local_u32(ctx.stack, &ctx.local_reference(), dst_local, value);
    call_next(tail_code, 4, ctx)
}

pub unsafe fn op_f32_local_local_compare_tee4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lhs_local = (*tail_code).operand.local_addr;
    let rhs_local = (*tail_code.add(1)).operand.local_addr;
    let dst_local = (*tail_code.add(2)).operand.local_addr;
    let kind = FloatCompareKind::from_raw((*tail_code.add(3)).operand.u32);
    let value = f32_compare_eval(
        local_u32(ctx.stack, &ctx.local_reference(), lhs_local),
        local_u32(ctx.stack, &ctx.local_reference(), rhs_local),
        kind,
    );
    write_local_u32(ctx.stack, &ctx.local_reference(), dst_local, value);
    vm_try!(ctx.stack.push_u32(value));
    call_next(tail_code, 4, ctx)
}

pub unsafe fn op_f32_local_const_compare_set4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lhs_local = (*tail_code).operand.local_addr;
    let rhs = (*tail_code.add(1)).operand.f32.to_bits();
    let dst_local = (*tail_code.add(2)).operand.local_addr;
    let kind = FloatCompareKind::from_raw((*tail_code.add(3)).operand.u32);
    let value = f32_compare_eval(
        local_u32(ctx.stack, &ctx.local_reference(), lhs_local),
        rhs,
        kind,
    );
    write_local_u32(ctx.stack, &ctx.local_reference(), dst_local, value);
    call_next(tail_code, 4, ctx)
}

pub unsafe fn op_f32_local_const_compare_tee4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lhs_local = (*tail_code).operand.local_addr;
    let rhs = (*tail_code.add(1)).operand.f32.to_bits();
    let dst_local = (*tail_code.add(2)).operand.local_addr;
    let kind = FloatCompareKind::from_raw((*tail_code.add(3)).operand.u32);
    let value = f32_compare_eval(
        local_u32(ctx.stack, &ctx.local_reference(), lhs_local),
        rhs,
        kind,
    );
    write_local_u32(ctx.stack, &ctx.local_reference(), dst_local, value);
    vm_try!(ctx.stack.push_u32(value));
    call_next(tail_code, 4, ctx)
}

pub unsafe fn op_f64_local_local_compare_set4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lhs_local = (*tail_code).operand.local_addr;
    let rhs_local = (*tail_code.add(1)).operand.local_addr;
    let dst_local = (*tail_code.add(2)).operand.local_addr;
    let kind = FloatCompareKind::from_raw((*tail_code.add(3)).operand.u32);
    let value = f64_compare_eval(
        local_u64(ctx.stack, &ctx.local_reference(), lhs_local),
        local_u64(ctx.stack, &ctx.local_reference(), rhs_local),
        kind,
    );
    write_local_u32(ctx.stack, &ctx.local_reference(), dst_local, value);
    call_next(tail_code, 4, ctx)
}

pub unsafe fn op_f64_local_local_compare_tee4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lhs_local = (*tail_code).operand.local_addr;
    let rhs_local = (*tail_code.add(1)).operand.local_addr;
    let dst_local = (*tail_code.add(2)).operand.local_addr;
    let kind = FloatCompareKind::from_raw((*tail_code.add(3)).operand.u32);
    let value = f64_compare_eval(
        local_u64(ctx.stack, &ctx.local_reference(), lhs_local),
        local_u64(ctx.stack, &ctx.local_reference(), rhs_local),
        kind,
    );
    write_local_u32(ctx.stack, &ctx.local_reference(), dst_local, value);
    vm_try!(ctx.stack.push_u32(value));
    call_next(tail_code, 4, ctx)
}

pub unsafe fn op_f64_local_const_compare_set4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lhs_local = (*tail_code).operand.local_addr;
    let rhs = (*tail_code.add(1)).operand.f64.to_bits();
    let dst_local = (*tail_code.add(2)).operand.local_addr;
    let kind = FloatCompareKind::from_raw((*tail_code.add(3)).operand.u32);
    let value = f64_compare_eval(
        local_u64(ctx.stack, &ctx.local_reference(), lhs_local),
        rhs,
        kind,
    );
    write_local_u32(ctx.stack, &ctx.local_reference(), dst_local, value);
    call_next(tail_code, 4, ctx)
}

pub unsafe fn op_f64_local_const_compare_tee4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lhs_local = (*tail_code).operand.local_addr;
    let rhs = (*tail_code.add(1)).operand.f64.to_bits();
    let dst_local = (*tail_code.add(2)).operand.local_addr;
    let kind = FloatCompareKind::from_raw((*tail_code.add(3)).operand.u32);
    let value = f64_compare_eval(
        local_u64(ctx.stack, &ctx.local_reference(), lhs_local),
        rhs,
        kind,
    );
    write_local_u32(ctx.stack, &ctx.local_reference(), dst_local, value);
    vm_try!(ctx.stack.push_u32(value));
    call_next(tail_code, 4, ctx)
}

pub unsafe fn op_i32_local_local_compare_br_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lhs_local = (*tail_code).operand.local_addr;
    let rhs_local = (*tail_code.add(1)).operand.local_addr;
    let kind = IntCompareKind::from_raw((*tail_code.add(3)).operand.u32);
    let ptr = if i32_compare_eval(
        local_u32(ctx.stack, &ctx.local_reference(), lhs_local),
        local_u32(ctx.stack, &ctx.local_reference(), rhs_local),
        kind,
    ) != 0
    {
        branch_target_relative(tail_code, ctx, 2, 4, ControlBranchKind::BrIf, true)
    } else {
        tail_code.offset(4)
    };
    call_next(ptr, 0, ctx)
}

pub unsafe fn op_i32_local_local_compare_br_if_ptr(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lhs_local = (*tail_code).operand.local_addr;
    let rhs_local = (*tail_code.add(1)).operand.local_addr;
    let kind = IntCompareKind::from_raw((*tail_code.add(3)).operand.u32);
    let ptr = if i32_compare_eval(
        local_u32(ctx.stack, &ctx.local_reference(), lhs_local),
        local_u32(ctx.stack, &ctx.local_reference(), rhs_local),
        kind,
    ) != 0
    {
        branch_target_ptr(tail_code, 2, 4, ControlBranchKind::BrIf, true)
    } else {
        tail_code.offset(4)
    };
    call_next(ptr, 0, ctx)
}

/// Telomere runtime helper `op_i32_local_const_compare_br_if`.
///
/// Stack effect: `internal compare branch dispatch`.
/// # Safety
/// - `tail_code` must point at the pointer-bearing operands for this specialized branch.
/// - `ctx` must reference a live execution context for the same validated frame and store.
pub unsafe fn op_i32_local_const_compare_br_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lhs_local = (*tail_code).operand.local_addr;
    let rhs = (*tail_code.add(1)).operand.i32 as u32;
    let kind = IntCompareKind::from_raw((*tail_code.add(3)).operand.u32);
    let ptr = if i32_compare_eval(
        local_u32(ctx.stack, &ctx.local_reference(), lhs_local),
        rhs,
        kind,
    ) != 0
    {
        branch_target_relative(tail_code, ctx, 2, 4, ControlBranchKind::BrIf, true)
    } else {
        tail_code.offset(4)
    };
    call_next(ptr, 0, ctx)
}

/// Telomere runtime helper `op_i32_local_const_compare_br_if_ptr`.
///
/// Stack effect: `internal compare branch dispatch`.
/// # Safety
/// - `tail_code` must point at the pointer-bearing operands for this specialized branch.
/// - `ctx` must reference a live execution context for the same validated frame and store.
pub unsafe fn op_i32_local_const_compare_br_if_ptr(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lhs_local = (*tail_code).operand.local_addr;
    let rhs = (*tail_code.add(1)).operand.i32 as u32;
    let kind = IntCompareKind::from_raw((*tail_code.add(3)).operand.u32);
    let ptr = if i32_compare_eval(
        local_u32(ctx.stack, &ctx.local_reference(), lhs_local),
        rhs,
        kind,
    ) != 0
    {
        branch_target_ptr(tail_code, 2, 4, ControlBranchKind::BrIf, true)
    } else {
        tail_code.offset(4)
    };
    call_next(ptr, 0, ctx)
}

pub unsafe fn op_i64_local_local_compare_br_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lhs_local = (*tail_code).operand.local_addr;
    let rhs_local = (*tail_code.add(1)).operand.local_addr;
    let kind = IntCompareKind::from_raw((*tail_code.add(3)).operand.u32);
    let ptr = if i64_compare_eval(
        local_u64(ctx.stack, &ctx.local_reference(), lhs_local),
        local_u64(ctx.stack, &ctx.local_reference(), rhs_local),
        kind,
    ) != 0
    {
        branch_target_relative(tail_code, ctx, 2, 4, ControlBranchKind::BrIf, true)
    } else {
        tail_code.offset(4)
    };
    call_next(ptr, 0, ctx)
}

/// Telomere runtime helper `op_i64_local_local_compare_br_if_ptr`.
///
/// Stack effect: `internal compare branch dispatch`.
/// # Safety
/// - `tail_code` must point at the pointer-bearing operands for this specialized branch.
/// - `ctx` must reference a live execution context for the same validated frame and store.
pub unsafe fn op_i64_local_local_compare_br_if_ptr(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lhs_local = (*tail_code).operand.local_addr;
    let rhs_local = (*tail_code.add(1)).operand.local_addr;
    let kind = IntCompareKind::from_raw((*tail_code.add(3)).operand.u32);
    let ptr = if i64_compare_eval(
        local_u64(ctx.stack, &ctx.local_reference(), lhs_local),
        local_u64(ctx.stack, &ctx.local_reference(), rhs_local),
        kind,
    ) != 0
    {
        branch_target_ptr(tail_code, 2, 4, ControlBranchKind::BrIf, true)
    } else {
        tail_code.offset(4)
    };
    call_next(ptr, 0, ctx)
}

pub unsafe fn op_i64_local_const_compare_br_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lhs_local = (*tail_code).operand.local_addr;
    let rhs = (*tail_code.add(1)).operand.u64;
    let kind = IntCompareKind::from_raw((*tail_code.add(3)).operand.u32);
    let ptr = if i64_compare_eval(
        local_u64(ctx.stack, &ctx.local_reference(), lhs_local),
        rhs,
        kind,
    ) != 0
    {
        branch_target_relative(tail_code, ctx, 2, 4, ControlBranchKind::BrIf, true)
    } else {
        tail_code.offset(4)
    };
    call_next(ptr, 0, ctx)
}

/// Telomere runtime helper `op_i64_local_const_compare_br_if_ptr`.
///
/// Stack effect: `internal compare branch dispatch`.
/// # Safety
/// - `tail_code` must point at the pointer-bearing operands for this specialized branch.
/// - `ctx` must reference a live execution context for the same validated frame and store.
pub unsafe fn op_i64_local_const_compare_br_if_ptr(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lhs_local = (*tail_code).operand.local_addr;
    let rhs = (*tail_code.add(1)).operand.u64;
    let kind = IntCompareKind::from_raw((*tail_code.add(3)).operand.u32);
    let ptr = if i64_compare_eval(
        local_u64(ctx.stack, &ctx.local_reference(), lhs_local),
        rhs,
        kind,
    ) != 0
    {
        branch_target_ptr(tail_code, 2, 4, ControlBranchKind::BrIf, true)
    } else {
        tail_code.offset(4)
    };
    call_next(ptr, 0, ctx)
}

pub unsafe fn op_f32_local_local_compare_br_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lhs_local = (*tail_code).operand.local_addr;
    let rhs_local = (*tail_code.add(1)).operand.local_addr;
    let kind = FloatCompareKind::from_raw((*tail_code.add(3)).operand.u32);
    let ptr = if f32_compare_eval(
        local_u32(ctx.stack, &ctx.local_reference(), lhs_local),
        local_u32(ctx.stack, &ctx.local_reference(), rhs_local),
        kind,
    ) != 0
    {
        branch_target_relative(tail_code, ctx, 2, 4, ControlBranchKind::BrIf, true)
    } else {
        tail_code.offset(4)
    };
    call_next(ptr, 0, ctx)
}

/// Telomere runtime helper `op_f32_local_local_compare_br_if_ptr`.
///
/// Stack effect: `internal compare branch dispatch`.
/// # Safety
/// - `tail_code` must point at the pointer-bearing operands for this specialized branch.
/// - `ctx` must reference a live execution context for the same validated frame and store.
pub unsafe fn op_f32_local_local_compare_br_if_ptr(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lhs_local = (*tail_code).operand.local_addr;
    let rhs_local = (*tail_code.add(1)).operand.local_addr;
    let kind = FloatCompareKind::from_raw((*tail_code.add(3)).operand.u32);
    let ptr = if f32_compare_eval(
        local_u32(ctx.stack, &ctx.local_reference(), lhs_local),
        local_u32(ctx.stack, &ctx.local_reference(), rhs_local),
        kind,
    ) != 0
    {
        branch_target_ptr(tail_code, 2, 4, ControlBranchKind::BrIf, true)
    } else {
        tail_code.offset(4)
    };
    call_next(ptr, 0, ctx)
}

pub unsafe fn op_f32_local_const_compare_br_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lhs_local = (*tail_code).operand.local_addr;
    let rhs = (*tail_code.add(1)).operand.f32.to_bits();
    let kind = FloatCompareKind::from_raw((*tail_code.add(3)).operand.u32);
    let ptr = if f32_compare_eval(
        local_u32(ctx.stack, &ctx.local_reference(), lhs_local),
        rhs,
        kind,
    ) != 0
    {
        branch_target_relative(tail_code, ctx, 2, 4, ControlBranchKind::BrIf, true)
    } else {
        tail_code.offset(4)
    };
    call_next(ptr, 0, ctx)
}

/// Telomere runtime helper `op_f32_local_const_compare_br_if_ptr`.
///
/// Stack effect: `internal compare branch dispatch`.
/// # Safety
/// - `tail_code` must point at the pointer-bearing operands for this specialized branch.
/// - `ctx` must reference a live execution context for the same validated frame and store.
pub unsafe fn op_f32_local_const_compare_br_if_ptr(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lhs_local = (*tail_code).operand.local_addr;
    let rhs = (*tail_code.add(1)).operand.f32.to_bits();
    let kind = FloatCompareKind::from_raw((*tail_code.add(3)).operand.u32);
    let ptr = if f32_compare_eval(
        local_u32(ctx.stack, &ctx.local_reference(), lhs_local),
        rhs,
        kind,
    ) != 0
    {
        branch_target_ptr(tail_code, 2, 4, ControlBranchKind::BrIf, true)
    } else {
        tail_code.offset(4)
    };
    call_next(ptr, 0, ctx)
}

pub unsafe fn op_f64_local_local_compare_br_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lhs_local = (*tail_code).operand.local_addr;
    let rhs_local = (*tail_code.add(1)).operand.local_addr;
    let kind = FloatCompareKind::from_raw((*tail_code.add(3)).operand.u32);
    let ptr = if f64_compare_eval(
        local_u64(ctx.stack, &ctx.local_reference(), lhs_local),
        local_u64(ctx.stack, &ctx.local_reference(), rhs_local),
        kind,
    ) != 0
    {
        branch_target_relative(tail_code, ctx, 2, 4, ControlBranchKind::BrIf, true)
    } else {
        tail_code.offset(4)
    };
    call_next(ptr, 0, ctx)
}

/// Telomere runtime helper `op_f64_local_local_compare_br_if_ptr`.
///
/// Stack effect: `internal compare branch dispatch`.
/// # Safety
/// - `tail_code` must point at the pointer-bearing operands for this specialized branch.
/// - `ctx` must reference a live execution context for the same validated frame and store.
pub unsafe fn op_f64_local_local_compare_br_if_ptr(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lhs_local = (*tail_code).operand.local_addr;
    let rhs_local = (*tail_code.add(1)).operand.local_addr;
    let kind = FloatCompareKind::from_raw((*tail_code.add(3)).operand.u32);
    let ptr = if f64_compare_eval(
        local_u64(ctx.stack, &ctx.local_reference(), lhs_local),
        local_u64(ctx.stack, &ctx.local_reference(), rhs_local),
        kind,
    ) != 0
    {
        branch_target_ptr(tail_code, 2, 4, ControlBranchKind::BrIf, true)
    } else {
        tail_code.offset(4)
    };
    call_next(ptr, 0, ctx)
}

pub unsafe fn op_f64_local_const_compare_br_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lhs_local = (*tail_code).operand.local_addr;
    let rhs = (*tail_code.add(1)).operand.f64.to_bits();
    let kind = FloatCompareKind::from_raw((*tail_code.add(3)).operand.u32);
    let ptr = if f64_compare_eval(
        local_u64(ctx.stack, &ctx.local_reference(), lhs_local),
        rhs,
        kind,
    ) != 0
    {
        branch_target_relative(tail_code, ctx, 2, 4, ControlBranchKind::BrIf, true)
    } else {
        tail_code.offset(4)
    };
    call_next(ptr, 0, ctx)
}

/// Telomere runtime helper `op_f64_local_const_compare_br_if_ptr`.
///
/// Stack effect: `internal compare branch dispatch`.
/// # Safety
/// - `tail_code` must point at the pointer-bearing operands for this specialized branch.
/// - `ctx` must reference a live execution context for the same validated frame and store.
pub unsafe fn op_f64_local_const_compare_br_if_ptr(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lhs_local = (*tail_code).operand.local_addr;
    let rhs = (*tail_code.add(1)).operand.f64.to_bits();
    let kind = FloatCompareKind::from_raw((*tail_code.add(3)).operand.u32);
    let ptr = if f64_compare_eval(
        local_u64(ctx.stack, &ctx.local_reference(), lhs_local),
        rhs,
        kind,
    ) != 0
    {
        branch_target_ptr(tail_code, 2, 4, ControlBranchKind::BrIf, true)
    } else {
        tail_code.offset(4)
    };
    call_next(ptr, 0, ctx)
}

pub unsafe fn op_i32_seed_imm_scalar_set4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let value = vm_try!(i32_scalar_eval(
        vm_try!(producer_seed_u32(tail_code, ctx)),
        (*tail_code.add(5)).operand.u32,
        I32ScalarKind::from_raw((*tail_code.add(7)).operand.u32),
    ));
    write_local_u32(
        ctx.stack,
        &ctx.local_reference(),
        (*tail_code.add(6)).operand.local_addr,
        value,
    );
    call_next(tail_code, 8, ctx)
}

pub unsafe fn op_i32_seed_imm_scalar_tee4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let value = vm_try!(i32_scalar_eval(
        vm_try!(producer_seed_u32(tail_code, ctx)),
        (*tail_code.add(5)).operand.u32,
        I32ScalarKind::from_raw((*tail_code.add(7)).operand.u32),
    ));
    write_local_u32(
        ctx.stack,
        &ctx.local_reference(),
        (*tail_code.add(6)).operand.local_addr,
        value,
    );
    vm_try!(ctx.stack.push_u32(value));
    call_next(tail_code, 8, ctx)
}

pub unsafe fn op_i64_seed_imm_scalar_set8(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let value = vm_try!(i64_scalar_eval(
        vm_try!(producer_seed_u64(tail_code, ctx)),
        (*tail_code.add(5)).operand.u64,
        I64ScalarKind::from_raw((*tail_code.add(7)).operand.u32),
    ));
    write_local_u64(
        ctx.stack,
        &ctx.local_reference(),
        (*tail_code.add(6)).operand.local_addr,
        value,
    );
    call_next(tail_code, 8, ctx)
}

pub unsafe fn op_i64_seed_imm_scalar_tee8(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let value = vm_try!(i64_scalar_eval(
        vm_try!(producer_seed_u64(tail_code, ctx)),
        (*tail_code.add(5)).operand.u64,
        I64ScalarKind::from_raw((*tail_code.add(7)).operand.u32),
    ));
    write_local_u64(
        ctx.stack,
        &ctx.local_reference(),
        (*tail_code.add(6)).operand.local_addr,
        value,
    );
    vm_try!(ctx.stack.push_u64(value));
    call_next(tail_code, 8, ctx)
}

pub unsafe fn op_i32_seed_tee_imm_scalar_set4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let seed = vm_try!(producer_seed_u32(tail_code, ctx));
    write_local_u32(
        ctx.stack,
        &ctx.local_reference(),
        (*tail_code.add(5)).operand.local_addr,
        seed,
    );
    let value = vm_try!(i32_scalar_eval(
        seed,
        (*tail_code.add(6)).operand.u32,
        I32ScalarKind::from_raw((*tail_code.add(8)).operand.u32),
    ));
    write_local_u32(
        ctx.stack,
        &ctx.local_reference(),
        (*tail_code.add(7)).operand.local_addr,
        value,
    );
    call_next(tail_code, 9, ctx)
}

pub unsafe fn op_i32_seed_tee_imm_scalar_tee4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let seed = vm_try!(producer_seed_u32(tail_code, ctx));
    write_local_u32(
        ctx.stack,
        &ctx.local_reference(),
        (*tail_code.add(5)).operand.local_addr,
        seed,
    );
    let value = vm_try!(i32_scalar_eval(
        seed,
        (*tail_code.add(6)).operand.u32,
        I32ScalarKind::from_raw((*tail_code.add(8)).operand.u32),
    ));
    write_local_u32(
        ctx.stack,
        &ctx.local_reference(),
        (*tail_code.add(7)).operand.local_addr,
        value,
    );
    vm_try!(ctx.stack.push_u32(value));
    call_next(tail_code, 9, ctx)
}

pub unsafe fn op_i64_seed_tee_imm_scalar_set8(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let seed = vm_try!(producer_seed_u64(tail_code, ctx));
    write_local_u64(
        ctx.stack,
        &ctx.local_reference(),
        (*tail_code.add(5)).operand.local_addr,
        seed,
    );
    let value = vm_try!(i64_scalar_eval(
        seed,
        (*tail_code.add(6)).operand.u64,
        I64ScalarKind::from_raw((*tail_code.add(8)).operand.u32),
    ));
    write_local_u64(
        ctx.stack,
        &ctx.local_reference(),
        (*tail_code.add(7)).operand.local_addr,
        value,
    );
    call_next(tail_code, 9, ctx)
}

pub unsafe fn op_i64_seed_tee_imm_scalar_tee8(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let seed = vm_try!(producer_seed_u64(tail_code, ctx));
    write_local_u64(
        ctx.stack,
        &ctx.local_reference(),
        (*tail_code.add(5)).operand.local_addr,
        seed,
    );
    let value = vm_try!(i64_scalar_eval(
        seed,
        (*tail_code.add(6)).operand.u64,
        I64ScalarKind::from_raw((*tail_code.add(8)).operand.u32),
    ));
    write_local_u64(
        ctx.stack,
        &ctx.local_reference(),
        (*tail_code.add(7)).operand.local_addr,
        value,
    );
    vm_try!(ctx.stack.push_u64(value));
    call_next(tail_code, 9, ctx)
}

pub unsafe fn op_i32_seed_tee_const_self_select4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let seed = vm_try!(producer_seed_u32(tail_code, ctx));
    write_local_u32(
        ctx.stack,
        &ctx.local_reference(),
        (*tail_code.add(5)).operand.local_addr,
        seed,
    );
    vm_try!(ctx.stack.push_u32(if seed == 0 {
        (*tail_code.add(6)).operand.u32
    } else {
        seed
    }));
    call_next(tail_code, 7, ctx)
}

pub unsafe fn op_i64_seed_tee_const_self_select8(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let seed = vm_try!(producer_seed_u64(tail_code, ctx));
    write_local_u64(
        ctx.stack,
        &ctx.local_reference(),
        (*tail_code.add(5)).operand.local_addr,
        seed,
    );
    vm_try!(ctx.stack.push_u64(if seed == 0 {
        (*tail_code.add(6)).operand.u64
    } else {
        seed
    }));
    call_next(tail_code, 7, ctx)
}

#[inline(always)]
unsafe fn seed_compare_select_i32_local(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    select8: bool,
) -> VMResult<()> {
    let cond = i32_compare_eval(
        vm_try!(producer_seed_u32(tail_code, ctx)),
        local_u32(
            ctx.stack,
            &ctx.local_reference(),
            (*tail_code.add(5)).operand.local_addr,
        ),
        IntCompareKind::from_raw((*tail_code.add(6)).operand.u32),
    );
    if select8 {
        select8_with_condition(ctx, cond);
    } else {
        select4_with_condition(ctx, cond);
    }
    call_next(tail_code, 7, ctx)
}

#[inline(always)]
unsafe fn seed_compare_select_i32_const(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    select8: bool,
) -> VMResult<()> {
    let cond = i32_compare_eval(
        vm_try!(producer_seed_u32(tail_code, ctx)),
        (*tail_code.add(5)).operand.u32,
        IntCompareKind::from_raw((*tail_code.add(6)).operand.u32),
    );
    if select8 {
        select8_with_condition(ctx, cond);
    } else {
        select4_with_condition(ctx, cond);
    }
    call_next(tail_code, 7, ctx)
}

#[inline(always)]
unsafe fn seed_compare_select_i64_local(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    select8: bool,
) -> VMResult<()> {
    let cond = i64_compare_eval(
        vm_try!(producer_seed_u64(tail_code, ctx)),
        local_u64(
            ctx.stack,
            &ctx.local_reference(),
            (*tail_code.add(5)).operand.local_addr,
        ),
        IntCompareKind::from_raw((*tail_code.add(6)).operand.u32),
    );
    if select8 {
        select8_with_condition(ctx, cond);
    } else {
        select4_with_condition(ctx, cond);
    }
    call_next(tail_code, 7, ctx)
}

#[inline(always)]
unsafe fn seed_compare_select_i64_const(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    select8: bool,
) -> VMResult<()> {
    let cond = i64_compare_eval(
        vm_try!(producer_seed_u64(tail_code, ctx)),
        (*tail_code.add(5)).operand.u64,
        IntCompareKind::from_raw((*tail_code.add(6)).operand.u32),
    );
    if select8 {
        select8_with_condition(ctx, cond);
    } else {
        select4_with_condition(ctx, cond);
    }
    call_next(tail_code, 7, ctx)
}

#[inline(always)]
unsafe fn seed_compare_select_f32_local(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    select8: bool,
) -> VMResult<()> {
    let cond = f32_compare_eval(
        vm_try!(producer_seed_u32(tail_code, ctx)),
        local_u32(
            ctx.stack,
            &ctx.local_reference(),
            (*tail_code.add(5)).operand.local_addr,
        ),
        FloatCompareKind::from_raw((*tail_code.add(6)).operand.u32),
    );
    if select8 {
        select8_with_condition(ctx, cond);
    } else {
        select4_with_condition(ctx, cond);
    }
    call_next(tail_code, 7, ctx)
}

#[inline(always)]
unsafe fn seed_compare_select_f32_const(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    select8: bool,
) -> VMResult<()> {
    let cond = f32_compare_eval(
        vm_try!(producer_seed_u32(tail_code, ctx)),
        (*tail_code.add(5)).operand.f32.to_bits(),
        FloatCompareKind::from_raw((*tail_code.add(6)).operand.u32),
    );
    if select8 {
        select8_with_condition(ctx, cond);
    } else {
        select4_with_condition(ctx, cond);
    }
    call_next(tail_code, 7, ctx)
}

#[inline(always)]
unsafe fn seed_compare_select_f64_local(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    select8: bool,
) -> VMResult<()> {
    let cond = f64_compare_eval(
        vm_try!(producer_seed_u64(tail_code, ctx)),
        local_u64(
            ctx.stack,
            &ctx.local_reference(),
            (*tail_code.add(5)).operand.local_addr,
        ),
        FloatCompareKind::from_raw((*tail_code.add(6)).operand.u32),
    );
    if select8 {
        select8_with_condition(ctx, cond);
    } else {
        select4_with_condition(ctx, cond);
    }
    call_next(tail_code, 7, ctx)
}

#[inline(always)]
unsafe fn seed_compare_select_f64_const(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    select8: bool,
) -> VMResult<()> {
    let cond = f64_compare_eval(
        vm_try!(producer_seed_u64(tail_code, ctx)),
        (*tail_code.add(5)).operand.f64.to_bits(),
        FloatCompareKind::from_raw((*tail_code.add(6)).operand.u32),
    );
    if select8 {
        select8_with_condition(ctx, cond);
    } else {
        select4_with_condition(ctx, cond);
    }
    call_next(tail_code, 7, ctx)
}

pub unsafe fn op_i32_seed_local_compare_select4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    seed_compare_select_i32_local(tail_code, ctx, false)
}

pub unsafe fn op_i32_seed_local_compare_select8(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    seed_compare_select_i32_local(tail_code, ctx, true)
}

pub unsafe fn op_i32_seed_const_compare_select4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    seed_compare_select_i32_const(tail_code, ctx, false)
}

pub unsafe fn op_i32_seed_const_compare_select8(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    seed_compare_select_i32_const(tail_code, ctx, true)
}

pub unsafe fn op_i64_seed_local_compare_select4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    seed_compare_select_i64_local(tail_code, ctx, false)
}

pub unsafe fn op_i64_seed_local_compare_select8(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    seed_compare_select_i64_local(tail_code, ctx, true)
}

pub unsafe fn op_i64_seed_const_compare_select4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    seed_compare_select_i64_const(tail_code, ctx, false)
}

pub unsafe fn op_i64_seed_const_compare_select8(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    seed_compare_select_i64_const(tail_code, ctx, true)
}

pub unsafe fn op_f32_seed_local_compare_select4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    seed_compare_select_f32_local(tail_code, ctx, false)
}

pub unsafe fn op_f32_seed_local_compare_select8(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    seed_compare_select_f32_local(tail_code, ctx, true)
}

pub unsafe fn op_f32_seed_const_compare_select4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    seed_compare_select_f32_const(tail_code, ctx, false)
}

pub unsafe fn op_f32_seed_const_compare_select8(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    seed_compare_select_f32_const(tail_code, ctx, true)
}

pub unsafe fn op_f64_seed_local_compare_select4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    seed_compare_select_f64_local(tail_code, ctx, false)
}

pub unsafe fn op_f64_seed_local_compare_select8(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    seed_compare_select_f64_local(tail_code, ctx, true)
}

pub unsafe fn op_f64_seed_const_compare_select4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    seed_compare_select_f64_const(tail_code, ctx, false)
}

pub unsafe fn op_f64_seed_const_compare_select8(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    seed_compare_select_f64_const(tail_code, ctx, true)
}

pub unsafe fn op_i32_local_local_compare_tee_select4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let cond = i32_compare_eval(
        local_u32(
            ctx.stack,
            &ctx.local_reference(),
            (*tail_code).operand.local_addr,
        ),
        local_u32(
            ctx.stack,
            &ctx.local_reference(),
            (*tail_code.add(1)).operand.local_addr,
        ),
        IntCompareKind::from_raw((*tail_code.add(3)).operand.u32),
    );
    write_local_u32(
        ctx.stack,
        &ctx.local_reference(),
        (*tail_code.add(2)).operand.local_addr,
        cond,
    );
    select4_with_condition(ctx, cond);
    call_next(tail_code, 4, ctx)
}

pub unsafe fn op_i32_local_local_compare_tee_select8(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let cond = i32_compare_eval(
        local_u32(
            ctx.stack,
            &ctx.local_reference(),
            (*tail_code).operand.local_addr,
        ),
        local_u32(
            ctx.stack,
            &ctx.local_reference(),
            (*tail_code.add(1)).operand.local_addr,
        ),
        IntCompareKind::from_raw((*tail_code.add(3)).operand.u32),
    );
    write_local_u32(
        ctx.stack,
        &ctx.local_reference(),
        (*tail_code.add(2)).operand.local_addr,
        cond,
    );
    select8_with_condition(ctx, cond);
    call_next(tail_code, 4, ctx)
}

pub unsafe fn op_i32_local_const_compare_tee_select4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let cond = i32_compare_eval(
        local_u32(
            ctx.stack,
            &ctx.local_reference(),
            (*tail_code).operand.local_addr,
        ),
        (*tail_code.add(1)).operand.u32,
        IntCompareKind::from_raw((*tail_code.add(3)).operand.u32),
    );
    write_local_u32(
        ctx.stack,
        &ctx.local_reference(),
        (*tail_code.add(2)).operand.local_addr,
        cond,
    );
    select4_with_condition(ctx, cond);
    call_next(tail_code, 4, ctx)
}

pub unsafe fn op_i32_local_const_compare_tee_select8(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let cond = i32_compare_eval(
        local_u32(
            ctx.stack,
            &ctx.local_reference(),
            (*tail_code).operand.local_addr,
        ),
        (*tail_code.add(1)).operand.u32,
        IntCompareKind::from_raw((*tail_code.add(3)).operand.u32),
    );
    write_local_u32(
        ctx.stack,
        &ctx.local_reference(),
        (*tail_code.add(2)).operand.local_addr,
        cond,
    );
    select8_with_condition(ctx, cond);
    call_next(tail_code, 4, ctx)
}

pub unsafe fn op_i64_local_local_compare_tee_select4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let cond = i64_compare_eval(
        local_u64(
            ctx.stack,
            &ctx.local_reference(),
            (*tail_code).operand.local_addr,
        ),
        local_u64(
            ctx.stack,
            &ctx.local_reference(),
            (*tail_code.add(1)).operand.local_addr,
        ),
        IntCompareKind::from_raw((*tail_code.add(3)).operand.u32),
    );
    write_local_u32(
        ctx.stack,
        &ctx.local_reference(),
        (*tail_code.add(2)).operand.local_addr,
        cond,
    );
    select4_with_condition(ctx, cond);
    call_next(tail_code, 4, ctx)
}

pub unsafe fn op_i64_local_local_compare_tee_select8(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let cond = i64_compare_eval(
        local_u64(
            ctx.stack,
            &ctx.local_reference(),
            (*tail_code).operand.local_addr,
        ),
        local_u64(
            ctx.stack,
            &ctx.local_reference(),
            (*tail_code.add(1)).operand.local_addr,
        ),
        IntCompareKind::from_raw((*tail_code.add(3)).operand.u32),
    );
    write_local_u32(
        ctx.stack,
        &ctx.local_reference(),
        (*tail_code.add(2)).operand.local_addr,
        cond,
    );
    select8_with_condition(ctx, cond);
    call_next(tail_code, 4, ctx)
}

pub unsafe fn op_i64_local_const_compare_tee_select4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let cond = i64_compare_eval(
        local_u64(
            ctx.stack,
            &ctx.local_reference(),
            (*tail_code).operand.local_addr,
        ),
        (*tail_code.add(1)).operand.u64,
        IntCompareKind::from_raw((*tail_code.add(3)).operand.u32),
    );
    write_local_u32(
        ctx.stack,
        &ctx.local_reference(),
        (*tail_code.add(2)).operand.local_addr,
        cond,
    );
    select4_with_condition(ctx, cond);
    call_next(tail_code, 4, ctx)
}

pub unsafe fn op_i64_local_const_compare_tee_select8(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let cond = i64_compare_eval(
        local_u64(
            ctx.stack,
            &ctx.local_reference(),
            (*tail_code).operand.local_addr,
        ),
        (*tail_code.add(1)).operand.u64,
        IntCompareKind::from_raw((*tail_code.add(3)).operand.u32),
    );
    write_local_u32(
        ctx.stack,
        &ctx.local_reference(),
        (*tail_code.add(2)).operand.local_addr,
        cond,
    );
    select8_with_condition(ctx, cond);
    call_next(tail_code, 4, ctx)
}

pub unsafe fn op_i32_local_local_compare_select4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lhs_local = (*tail_code).operand.local_addr;
    let rhs_local = (*tail_code.add(1)).operand.local_addr;
    let kind = IntCompareKind::from_raw((*tail_code.add(2)).operand.u32);
    select4_with_condition(
        ctx,
        i32_compare_eval(
            local_u32(ctx.stack, &ctx.local_reference(), lhs_local),
            local_u32(ctx.stack, &ctx.local_reference(), rhs_local),
            kind,
        ),
    );
    call_next(tail_code, 3, ctx)
}

pub unsafe fn op_i32_local_local_compare_select8(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lhs_local = (*tail_code).operand.local_addr;
    let rhs_local = (*tail_code.add(1)).operand.local_addr;
    let kind = IntCompareKind::from_raw((*tail_code.add(2)).operand.u32);
    select8_with_condition(
        ctx,
        i32_compare_eval(
            local_u32(ctx.stack, &ctx.local_reference(), lhs_local),
            local_u32(ctx.stack, &ctx.local_reference(), rhs_local),
            kind,
        ),
    );
    call_next(tail_code, 3, ctx)
}

pub unsafe fn op_i32_local_const_compare_select4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lhs_local = (*tail_code).operand.local_addr;
    let rhs = (*tail_code.add(1)).operand.i32 as u32;
    let kind = IntCompareKind::from_raw((*tail_code.add(2)).operand.u32);
    select4_with_condition(
        ctx,
        i32_compare_eval(
            local_u32(ctx.stack, &ctx.local_reference(), lhs_local),
            rhs,
            kind,
        ),
    );
    call_next(tail_code, 3, ctx)
}

pub unsafe fn op_i32_local_const_compare_select8(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lhs_local = (*tail_code).operand.local_addr;
    let rhs = (*tail_code.add(1)).operand.i32 as u32;
    let kind = IntCompareKind::from_raw((*tail_code.add(2)).operand.u32);
    select8_with_condition(
        ctx,
        i32_compare_eval(
            local_u32(ctx.stack, &ctx.local_reference(), lhs_local),
            rhs,
            kind,
        ),
    );
    call_next(tail_code, 3, ctx)
}

pub unsafe fn op_i64_local_local_compare_select4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lhs_local = (*tail_code).operand.local_addr;
    let rhs_local = (*tail_code.add(1)).operand.local_addr;
    let kind = IntCompareKind::from_raw((*tail_code.add(2)).operand.u32);
    select4_with_condition(
        ctx,
        i64_compare_eval(
            local_u64(ctx.stack, &ctx.local_reference(), lhs_local),
            local_u64(ctx.stack, &ctx.local_reference(), rhs_local),
            kind,
        ),
    );
    call_next(tail_code, 3, ctx)
}

pub unsafe fn op_i64_local_local_compare_select8(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lhs_local = (*tail_code).operand.local_addr;
    let rhs_local = (*tail_code.add(1)).operand.local_addr;
    let kind = IntCompareKind::from_raw((*tail_code.add(2)).operand.u32);
    select8_with_condition(
        ctx,
        i64_compare_eval(
            local_u64(ctx.stack, &ctx.local_reference(), lhs_local),
            local_u64(ctx.stack, &ctx.local_reference(), rhs_local),
            kind,
        ),
    );
    call_next(tail_code, 3, ctx)
}

pub unsafe fn op_i64_local_const_compare_select4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lhs_local = (*tail_code).operand.local_addr;
    let rhs = (*tail_code.add(1)).operand.u64;
    let kind = IntCompareKind::from_raw((*tail_code.add(2)).operand.u32);
    select4_with_condition(
        ctx,
        i64_compare_eval(
            local_u64(ctx.stack, &ctx.local_reference(), lhs_local),
            rhs,
            kind,
        ),
    );
    call_next(tail_code, 3, ctx)
}

pub unsafe fn op_i64_local_const_compare_select8(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lhs_local = (*tail_code).operand.local_addr;
    let rhs = (*tail_code.add(1)).operand.u64;
    let kind = IntCompareKind::from_raw((*tail_code.add(2)).operand.u32);
    select8_with_condition(
        ctx,
        i64_compare_eval(
            local_u64(ctx.stack, &ctx.local_reference(), lhs_local),
            rhs,
            kind,
        ),
    );
    call_next(tail_code, 3, ctx)
}

pub unsafe fn op_f32_local_local_compare_select4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lhs_local = (*tail_code).operand.local_addr;
    let rhs_local = (*tail_code.add(1)).operand.local_addr;
    let kind = FloatCompareKind::from_raw((*tail_code.add(2)).operand.u32);
    select4_with_condition(
        ctx,
        f32_compare_eval(
            local_u32(ctx.stack, &ctx.local_reference(), lhs_local),
            local_u32(ctx.stack, &ctx.local_reference(), rhs_local),
            kind,
        ),
    );
    call_next(tail_code, 3, ctx)
}

pub unsafe fn op_f32_local_local_compare_select8(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lhs_local = (*tail_code).operand.local_addr;
    let rhs_local = (*tail_code.add(1)).operand.local_addr;
    let kind = FloatCompareKind::from_raw((*tail_code.add(2)).operand.u32);
    select8_with_condition(
        ctx,
        f32_compare_eval(
            local_u32(ctx.stack, &ctx.local_reference(), lhs_local),
            local_u32(ctx.stack, &ctx.local_reference(), rhs_local),
            kind,
        ),
    );
    call_next(tail_code, 3, ctx)
}

pub unsafe fn op_f32_local_const_compare_select4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lhs_local = (*tail_code).operand.local_addr;
    let rhs = (*tail_code.add(1)).operand.f32.to_bits();
    let kind = FloatCompareKind::from_raw((*tail_code.add(2)).operand.u32);
    select4_with_condition(
        ctx,
        f32_compare_eval(
            local_u32(ctx.stack, &ctx.local_reference(), lhs_local),
            rhs,
            kind,
        ),
    );
    call_next(tail_code, 3, ctx)
}

pub unsafe fn op_f32_local_const_compare_select8(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lhs_local = (*tail_code).operand.local_addr;
    let rhs = (*tail_code.add(1)).operand.f32.to_bits();
    let kind = FloatCompareKind::from_raw((*tail_code.add(2)).operand.u32);
    select8_with_condition(
        ctx,
        f32_compare_eval(
            local_u32(ctx.stack, &ctx.local_reference(), lhs_local),
            rhs,
            kind,
        ),
    );
    call_next(tail_code, 3, ctx)
}

pub unsafe fn op_f64_local_local_compare_select4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lhs_local = (*tail_code).operand.local_addr;
    let rhs_local = (*tail_code.add(1)).operand.local_addr;
    let kind = FloatCompareKind::from_raw((*tail_code.add(2)).operand.u32);
    select4_with_condition(
        ctx,
        f64_compare_eval(
            local_u64(ctx.stack, &ctx.local_reference(), lhs_local),
            local_u64(ctx.stack, &ctx.local_reference(), rhs_local),
            kind,
        ),
    );
    call_next(tail_code, 3, ctx)
}

pub unsafe fn op_f64_local_local_compare_select8(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lhs_local = (*tail_code).operand.local_addr;
    let rhs_local = (*tail_code.add(1)).operand.local_addr;
    let kind = FloatCompareKind::from_raw((*tail_code.add(2)).operand.u32);
    select8_with_condition(
        ctx,
        f64_compare_eval(
            local_u64(ctx.stack, &ctx.local_reference(), lhs_local),
            local_u64(ctx.stack, &ctx.local_reference(), rhs_local),
            kind,
        ),
    );
    call_next(tail_code, 3, ctx)
}

pub unsafe fn op_f64_local_const_compare_select4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lhs_local = (*tail_code).operand.local_addr;
    let rhs = (*tail_code.add(1)).operand.f64.to_bits();
    let kind = FloatCompareKind::from_raw((*tail_code.add(2)).operand.u32);
    select4_with_condition(
        ctx,
        f64_compare_eval(
            local_u64(ctx.stack, &ctx.local_reference(), lhs_local),
            rhs,
            kind,
        ),
    );
    call_next(tail_code, 3, ctx)
}

pub unsafe fn op_f64_local_const_compare_select8(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lhs_local = (*tail_code).operand.local_addr;
    let rhs = (*tail_code.add(1)).operand.f64.to_bits();
    let kind = FloatCompareKind::from_raw((*tail_code.add(2)).operand.u32);
    select8_with_condition(
        ctx,
        f64_compare_eval(
            local_u64(ctx.stack, &ctx.local_reference(), lhs_local),
            rhs,
            kind,
        ),
    );
    call_next(tail_code, 3, ctx)
}

pub unsafe fn op_load_const_local4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let start = (*tail_code).operand.u32 as usize;
    let kind = Load4Kind::from_raw((*tail_code.add(1)).operand.u32);
    match kind {
        Load4Kind::I32 => vm_try!(ctx.stack.push_u32(vm_try!(ctx
            .gc
            .local_read_u32_at(ctx.default_local_memory_id_unchecked(), start)))),
        Load4Kind::I32Load8S => vm_try!(ctx.stack.push_i32(i32::from(vm_try!(ctx
            .gc
            .local_read_i8_at(ctx.default_local_memory_id_unchecked(), start))))),
        Load4Kind::I32Load8U => vm_try!(ctx.stack.push_u32(u32::from(vm_try!(ctx
            .gc
            .local_read_u8_at(ctx.default_local_memory_id_unchecked(), start))))),
        Load4Kind::I32Load16S => vm_try!(ctx.stack.push_i32(i32::from(vm_try!(ctx
            .gc
            .local_read_i16_at(ctx.default_local_memory_id_unchecked(), start))))),
        Load4Kind::I32Load16U => vm_try!(ctx.stack.push_u32(u32::from(vm_try!(ctx
            .gc
            .local_read_u16_at(ctx.default_local_memory_id_unchecked(), start))))),
        Load4Kind::F32 => vm_try!(ctx.stack.push_u32(vm_try!(ctx
            .gc
            .local_read_u32_at(ctx.default_local_memory_id_unchecked(), start)))),
    }
    call_next(tail_code, 2, ctx)
}

pub unsafe fn op_load_const_local8(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let start = (*tail_code).operand.u32 as usize;
    let kind = Load8Kind::from_raw((*tail_code.add(1)).operand.u32);
    match kind {
        Load8Kind::I64 => vm_try!(ctx.stack.push_u64(vm_try!(ctx
            .gc
            .local_read_u64_at(ctx.default_local_memory_id_unchecked(), start)))),
        Load8Kind::I64Load8S => vm_try!(ctx.stack.push_i64(i64::from(vm_try!(ctx
            .gc
            .local_read_i8_at(ctx.default_local_memory_id_unchecked(), start))))),
        Load8Kind::I64Load8U => vm_try!(ctx.stack.push_u64(u64::from(vm_try!(ctx
            .gc
            .local_read_u8_at(ctx.default_local_memory_id_unchecked(), start))))),
        Load8Kind::I64Load16S => vm_try!(ctx.stack.push_i64(i64::from(vm_try!(ctx
            .gc
            .local_read_i16_at(ctx.default_local_memory_id_unchecked(), start))))),
        Load8Kind::I64Load16U => vm_try!(ctx.stack.push_u64(u64::from(vm_try!(ctx
            .gc
            .local_read_u16_at(ctx.default_local_memory_id_unchecked(), start))))),
        Load8Kind::I64Load32S => vm_try!(ctx.stack.push_i64(i64::from(vm_try!(ctx
            .gc
            .local_read_i32_at(ctx.default_local_memory_id_unchecked(), start))))),
        Load8Kind::I64Load32U => vm_try!(ctx.stack.push_u64(u64::from(vm_try!(ctx
            .gc
            .local_read_u32_at(ctx.default_local_memory_id_unchecked(), start))))),
        Load8Kind::F64 => vm_try!(ctx.stack.push_u64(vm_try!(ctx
            .gc
            .local_read_u64_at(ctx.default_local_memory_id_unchecked(), start)))),
    }
    call_next(tail_code, 2, ctx)
}

pub unsafe fn op_local_addr_load4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let start = vm_try!(local_addr_mem_start(tail_code, ctx));
    let kind = Load4Kind::from_raw((*tail_code.add(2)).operand.u32);
    match kind {
        Load4Kind::I32 => vm_try!(ctx.stack.push_u32(vm_try!(ctx
            .gc
            .local_read_u32_at(ctx.default_local_memory_id_unchecked(), start)))),
        Load4Kind::I32Load8S => vm_try!(ctx.stack.push_i32(i32::from(vm_try!(ctx
            .gc
            .local_read_i8_at(ctx.default_local_memory_id_unchecked(), start))))),
        Load4Kind::I32Load8U => vm_try!(ctx.stack.push_u32(u32::from(vm_try!(ctx
            .gc
            .local_read_u8_at(ctx.default_local_memory_id_unchecked(), start))))),
        Load4Kind::I32Load16S => vm_try!(ctx.stack.push_i32(i32::from(vm_try!(ctx
            .gc
            .local_read_i16_at(ctx.default_local_memory_id_unchecked(), start))))),
        Load4Kind::I32Load16U => vm_try!(ctx.stack.push_u32(u32::from(vm_try!(ctx
            .gc
            .local_read_u16_at(ctx.default_local_memory_id_unchecked(), start))))),
        Load4Kind::F32 => vm_try!(ctx.stack.push_u32(vm_try!(ctx
            .gc
            .local_read_u32_at(ctx.default_local_memory_id_unchecked(), start)))),
    }
    call_next(tail_code, 3, ctx)
}

pub unsafe fn op_local_addr_load8(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let start = vm_try!(local_addr_mem_start(tail_code, ctx));
    let kind = Load8Kind::from_raw((*tail_code.add(2)).operand.u32);
    match kind {
        Load8Kind::I64 => vm_try!(ctx.stack.push_u64(vm_try!(ctx
            .gc
            .local_read_u64_at(ctx.default_local_memory_id_unchecked(), start)))),
        Load8Kind::I64Load8S => vm_try!(ctx.stack.push_i64(i64::from(vm_try!(ctx
            .gc
            .local_read_i8_at(ctx.default_local_memory_id_unchecked(), start))))),
        Load8Kind::I64Load8U => vm_try!(ctx.stack.push_u64(u64::from(vm_try!(ctx
            .gc
            .local_read_u8_at(ctx.default_local_memory_id_unchecked(), start))))),
        Load8Kind::I64Load16S => vm_try!(ctx.stack.push_i64(i64::from(vm_try!(ctx
            .gc
            .local_read_i16_at(ctx.default_local_memory_id_unchecked(), start))))),
        Load8Kind::I64Load16U => vm_try!(ctx.stack.push_u64(u64::from(vm_try!(ctx
            .gc
            .local_read_u16_at(ctx.default_local_memory_id_unchecked(), start))))),
        Load8Kind::I64Load32S => vm_try!(ctx.stack.push_i64(i64::from(vm_try!(ctx
            .gc
            .local_read_i32_at(ctx.default_local_memory_id_unchecked(), start))))),
        Load8Kind::I64Load32U => vm_try!(ctx.stack.push_u64(u64::from(vm_try!(ctx
            .gc
            .local_read_u32_at(ctx.default_local_memory_id_unchecked(), start))))),
        Load8Kind::F64 => vm_try!(ctx.stack.push_u64(vm_try!(ctx
            .gc
            .local_read_u64_at(ctx.default_local_memory_id_unchecked(), start)))),
    }
    call_next(tail_code, 3, ctx)
}

pub unsafe fn op_local_store_const_local4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let start = (*tail_code).operand.u32 as usize;
    let value_local = (*tail_code.add(1)).operand.local_addr;
    let kind = Store4Kind::from_raw((*tail_code.add(2)).operand.u32);
    let bytes = store4_bytes(
        local_u32(ctx.stack, &ctx.local_reference(), value_local),
        kind,
    );
    vm_try!(ctx.gc.local_write_bytes(
        ctx.default_local_memory_id_unchecked(),
        start,
        bytes.as_slice()
    ));
    call_next(tail_code, 3, ctx)
}

pub unsafe fn op_local_store_const_local8(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let start = (*tail_code).operand.u32 as usize;
    let value_local = (*tail_code.add(1)).operand.local_addr;
    let kind = Store8Kind::from_raw((*tail_code.add(2)).operand.u32);
    let bytes = store8_bytes(
        local_u64(ctx.stack, &ctx.local_reference(), value_local),
        kind,
    );
    vm_try!(ctx.gc.local_write_bytes(
        ctx.default_local_memory_id_unchecked(),
        start,
        bytes.as_slice()
    ));
    call_next(tail_code, 3, ctx)
}

pub unsafe fn op_local_local_store4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let start = vm_try!(local_local_store_start(tail_code, ctx));
    let value_local = (*tail_code.add(1)).operand.local_addr;
    let kind = Store4Kind::from_raw((*tail_code.add(3)).operand.u32);
    let bytes = store4_bytes(
        local_u32(ctx.stack, &ctx.local_reference(), value_local),
        kind,
    );
    vm_try!(ctx.gc.local_write_bytes(
        ctx.default_local_memory_id_unchecked(),
        start,
        bytes.as_slice()
    ));
    call_next(tail_code, 4, ctx)
}

pub unsafe fn op_local_local_store8(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let start = vm_try!(local_local_store_start(tail_code, ctx));
    let value_local = (*tail_code.add(1)).operand.local_addr;
    let kind = Store8Kind::from_raw((*tail_code.add(3)).operand.u32);
    let bytes = store8_bytes(
        local_u64(ctx.stack, &ctx.local_reference(), value_local),
        kind,
    );
    vm_try!(ctx.gc.local_write_bytes(
        ctx.default_local_memory_id_unchecked(),
        start,
        bytes.as_slice()
    ));
    call_next(tail_code, 4, ctx)
}
