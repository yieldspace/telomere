use super::producer_seed::{producer_seed_u32, producer_seed_u64};
use super::*;

#[inline(always)]
fn bool_to_u32(value: bool) -> u32 {
    if value {
        1
    } else {
        0
    }
}

pub(super) fn i32_compare_eval(lhs: u32, rhs: u32, kind: IntCompareKind) -> u32 {
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
pub(super) fn i64_compare_eval(lhs: u64, rhs: u64, kind: IntCompareKind) -> u32 {
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
pub(super) fn f32_compare_eval(lhs_bits: u32, rhs_bits: u32, kind: FloatCompareKind) -> u32 {
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
pub(super) fn f64_compare_eval(lhs_bits: u64, rhs_bits: u64, kind: FloatCompareKind) -> u32 {
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
pub(super) fn select4_with_condition(ctx: &mut ExecuteContext, cond: u32) {
    ctx.stack.select_top_u32(cond);
}

#[inline(always)]
pub(super) fn select8_with_condition(ctx: &mut ExecuteContext, cond: u32) {
    ctx.stack.select_top_u64(cond);
}

#[inline(always)]
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
