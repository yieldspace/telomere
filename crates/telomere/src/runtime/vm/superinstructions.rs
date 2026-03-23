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
pub(crate) enum I32ScalarKind {
    Add = 0,
    Sub = 1,
    Mul = 2,
    And = 3,
    Or = 4,
    Xor = 5,
    Shl = 6,
    ShrS = 7,
    ShrU = 8,
    DivS = 9,
    DivU = 10,
    RemS = 11,
    RemU = 12,
}

impl I32ScalarKind {
    #[inline(always)]
    pub(crate) fn from_raw(raw: u32) -> Self {
        match raw {
            0 => Self::Add,
            1 => Self::Sub,
            2 => Self::Mul,
            3 => Self::And,
            4 => Self::Or,
            5 => Self::Xor,
            6 => Self::Shl,
            7 => Self::ShrS,
            8 => Self::ShrU,
            9 => Self::DivS,
            10 => Self::DivU,
            11 => Self::RemS,
            12 => Self::RemU,
            _ => unreachable!("invalid I32ScalarKind: {raw}"),
        }
    }
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum I64ScalarKind {
    Add = 0,
    Sub = 1,
    Mul = 2,
    And = 3,
    Or = 4,
    Xor = 5,
    Shl = 6,
    ShrS = 7,
    ShrU = 8,
    DivS = 9,
    DivU = 10,
    RemS = 11,
    RemU = 12,
}

impl I64ScalarKind {
    #[inline(always)]
    pub(crate) fn from_raw(raw: u32) -> Self {
        match raw {
            0 => Self::Add,
            1 => Self::Sub,
            2 => Self::Mul,
            3 => Self::And,
            4 => Self::Or,
            5 => Self::Xor,
            6 => Self::Shl,
            7 => Self::ShrS,
            8 => Self::ShrU,
            9 => Self::DivS,
            10 => Self::DivU,
            11 => Self::RemS,
            12 => Self::RemU,
            _ => unreachable!("invalid I64ScalarKind: {raw}"),
        }
    }
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FloatScalarKind {
    Add = 0,
    Sub = 1,
    Mul = 2,
    Div = 3,
}

impl FloatScalarKind {
    #[inline(always)]
    pub(crate) fn from_raw(raw: u32) -> Self {
        match raw {
            0 => Self::Add,
            1 => Self::Sub,
            2 => Self::Mul,
            3 => Self::Div,
            _ => unreachable!("invalid FloatScalarKind: {raw}"),
        }
    }
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum IntCompareKind {
    Eq = 0,
    Ne = 1,
    LtS = 2,
    LtU = 3,
    GtS = 4,
    GtU = 5,
    LeS = 6,
    LeU = 7,
    GeS = 8,
    GeU = 9,
}

impl IntCompareKind {
    #[inline(always)]
    pub(crate) fn from_raw(raw: u32) -> Self {
        match raw {
            0 => Self::Eq,
            1 => Self::Ne,
            2 => Self::LtS,
            3 => Self::LtU,
            4 => Self::GtS,
            5 => Self::GtU,
            6 => Self::LeS,
            7 => Self::LeU,
            8 => Self::GeS,
            9 => Self::GeU,
            _ => unreachable!("invalid IntCompareKind: {raw}"),
        }
    }
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FloatCompareKind {
    Eq = 0,
    Ne = 1,
    Lt = 2,
    Gt = 3,
    Le = 4,
    Ge = 5,
}

impl FloatCompareKind {
    #[inline(always)]
    pub(crate) fn from_raw(raw: u32) -> Self {
        match raw {
            0 => Self::Eq,
            1 => Self::Ne,
            2 => Self::Lt,
            3 => Self::Gt,
            4 => Self::Le,
            5 => Self::Ge,
            _ => unreachable!("invalid FloatCompareKind: {raw}"),
        }
    }
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Load4Kind {
    I32 = 0,
    I32Load8S = 1,
    I32Load8U = 2,
    I32Load16S = 3,
    I32Load16U = 4,
    F32 = 5,
}

impl Load4Kind {
    #[inline(always)]
    pub(crate) fn from_raw(raw: u32) -> Self {
        match raw {
            0 => Self::I32,
            1 => Self::I32Load8S,
            2 => Self::I32Load8U,
            3 => Self::I32Load16S,
            4 => Self::I32Load16U,
            5 => Self::F32,
            _ => unreachable!("invalid Load4Kind: {raw}"),
        }
    }
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Load8Kind {
    I64 = 0,
    I64Load8S = 1,
    I64Load8U = 2,
    I64Load16S = 3,
    I64Load16U = 4,
    I64Load32S = 5,
    I64Load32U = 6,
    F64 = 7,
}

impl Load8Kind {
    #[inline(always)]
    pub(crate) fn from_raw(raw: u32) -> Self {
        match raw {
            0 => Self::I64,
            1 => Self::I64Load8S,
            2 => Self::I64Load8U,
            3 => Self::I64Load16S,
            4 => Self::I64Load16U,
            5 => Self::I64Load32S,
            6 => Self::I64Load32U,
            7 => Self::F64,
            _ => unreachable!("invalid Load8Kind: {raw}"),
        }
    }
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Store4Kind {
    I32 = 0,
    I32Store8 = 1,
    I32Store16 = 2,
    F32 = 3,
}

impl Store4Kind {
    #[inline(always)]
    pub(crate) fn from_raw(raw: u32) -> Self {
        match raw {
            0 => Self::I32,
            1 => Self::I32Store8,
            2 => Self::I32Store16,
            3 => Self::F32,
            _ => unreachable!("invalid Store4Kind: {raw}"),
        }
    }
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Store8Kind {
    I64 = 0,
    I64Store8 = 1,
    I64Store16 = 2,
    I64Store32 = 3,
    F64 = 4,
}

impl Store8Kind {
    #[inline(always)]
    pub(crate) fn from_raw(raw: u32) -> Self {
        match raw {
            0 => Self::I64,
            1 => Self::I64Store8,
            2 => Self::I64Store16,
            3 => Self::I64Store32,
            4 => Self::F64,
            _ => unreachable!("invalid Store8Kind: {raw}"),
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

enum ControlBranchKind {
    BrIf,
    If,
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
    let target = (*tail_code.add(1)).operand.jump_addr;
    let cond = local_u32(ctx.stack, &ctx.local_reference(), local_addr) == 0;
    let taken = if zero_test { cond } else { !cond };
    let ptr = match (branch_kind, taken) {
        (ControlBranchKind::BrIf, true) => ctx.code().offset(target as isize),
        (ControlBranchKind::BrIf, false) => tail_code.offset(2),
        (ControlBranchKind::If, true) => tail_code.offset(2),
        (ControlBranchKind::If, false) => ctx.code().offset(target as isize),
    };
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

#[inline(always)]
unsafe fn op_local_branch_u64(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    zero_test: bool,
    branch_kind: ControlBranchKind,
) -> VMResult<()> {
    let local_addr = (*tail_code).operand.local_addr;
    let target = (*tail_code.add(1)).operand.jump_addr;
    let cond = local_u64(ctx.stack, &ctx.local_reference(), local_addr) == 0;
    let taken = if zero_test { cond } else { !cond };
    let ptr = match (branch_kind, taken) {
        (ControlBranchKind::BrIf, true) => ctx.code().offset(target as isize),
        (ControlBranchKind::BrIf, false) => tail_code.offset(2),
        (ControlBranchKind::If, true) => tail_code.offset(2),
        (ControlBranchKind::If, false) => ctx.code().offset(target as isize),
    };
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

#[inline(always)]
unsafe fn op_i32_local_and_imm_branch(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    zero_test: bool,
    branch_kind: ControlBranchKind,
) -> VMResult<()> {
    let local_addr = (*tail_code).operand.local_addr;
    let imm = (*tail_code.add(1)).operand.i32 as u32;
    let target = (*tail_code.add(2)).operand.jump_addr;
    let cond = (local_u32(ctx.stack, &ctx.local_reference(), local_addr) & imm) == 0;
    let taken = if zero_test { cond } else { !cond };
    let ptr = match (branch_kind, taken) {
        (ControlBranchKind::BrIf, true) => ctx.code().offset(target as isize),
        (ControlBranchKind::BrIf, false) => tail_code.offset(3),
        (ControlBranchKind::If, true) => tail_code.offset(3),
        (ControlBranchKind::If, false) => ctx.code().offset(target as isize),
    };
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

#[inline(always)]
unsafe fn local_imm_addr_mem_start(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<usize> {
    let local_addr = (*tail_code).operand.local_addr;
    let imm = (*tail_code.add(1)).operand.i32 as u32;
    let memarg = (*tail_code.add(2)).operand.memarg;
    compute_memory_offset(
        memarg,
        local_u32(ctx.stack, &ctx.local_reference(), local_addr).wrapping_add(imm),
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
    let target = (*tail_code.add(2)).operand.jump_addr;
    let kind = IntCompareKind::from_raw((*tail_code.add(3)).operand.u32);
    let ptr = if i32_compare_eval(
        local_u32(ctx.stack, &ctx.local_reference(), lhs_local),
        local_u32(ctx.stack, &ctx.local_reference(), rhs_local),
        kind,
    ) != 0
    {
        ctx.code().offset(target as isize)
    } else {
        tail_code.offset(4)
    };
    call_next(ptr, 0, ctx)
}

pub unsafe fn op_i32_local_const_compare_br_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lhs_local = (*tail_code).operand.local_addr;
    let rhs = (*tail_code.add(1)).operand.i32 as u32;
    let target = (*tail_code.add(2)).operand.jump_addr;
    let kind = IntCompareKind::from_raw((*tail_code.add(3)).operand.u32);
    let ptr = if i32_compare_eval(
        local_u32(ctx.stack, &ctx.local_reference(), lhs_local),
        rhs,
        kind,
    ) != 0
    {
        ctx.code().offset(target as isize)
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
    let target = (*tail_code.add(2)).operand.jump_addr;
    let kind = IntCompareKind::from_raw((*tail_code.add(3)).operand.u32);
    let ptr = if i64_compare_eval(
        local_u64(ctx.stack, &ctx.local_reference(), lhs_local),
        local_u64(ctx.stack, &ctx.local_reference(), rhs_local),
        kind,
    ) != 0
    {
        ctx.code().offset(target as isize)
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
    let target = (*tail_code.add(2)).operand.jump_addr;
    let kind = IntCompareKind::from_raw((*tail_code.add(3)).operand.u32);
    let ptr = if i64_compare_eval(
        local_u64(ctx.stack, &ctx.local_reference(), lhs_local),
        rhs,
        kind,
    ) != 0
    {
        ctx.code().offset(target as isize)
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
    let target = (*tail_code.add(2)).operand.jump_addr;
    let kind = FloatCompareKind::from_raw((*tail_code.add(3)).operand.u32);
    let ptr = if f32_compare_eval(
        local_u32(ctx.stack, &ctx.local_reference(), lhs_local),
        local_u32(ctx.stack, &ctx.local_reference(), rhs_local),
        kind,
    ) != 0
    {
        ctx.code().offset(target as isize)
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
    let target = (*tail_code.add(2)).operand.jump_addr;
    let kind = FloatCompareKind::from_raw((*tail_code.add(3)).operand.u32);
    let ptr = if f32_compare_eval(
        local_u32(ctx.stack, &ctx.local_reference(), lhs_local),
        rhs,
        kind,
    ) != 0
    {
        ctx.code().offset(target as isize)
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
    let target = (*tail_code.add(2)).operand.jump_addr;
    let kind = FloatCompareKind::from_raw((*tail_code.add(3)).operand.u32);
    let ptr = if f64_compare_eval(
        local_u64(ctx.stack, &ctx.local_reference(), lhs_local),
        local_u64(ctx.stack, &ctx.local_reference(), rhs_local),
        kind,
    ) != 0
    {
        ctx.code().offset(target as isize)
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
    let target = (*tail_code.add(2)).operand.jump_addr;
    let kind = FloatCompareKind::from_raw((*tail_code.add(3)).operand.u32);
    let ptr = if f64_compare_eval(
        local_u64(ctx.stack, &ctx.local_reference(), lhs_local),
        rhs,
        kind,
    ) != 0
    {
        ctx.code().offset(target as isize)
    } else {
        tail_code.offset(4)
    };
    call_next(ptr, 0, ctx)
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
