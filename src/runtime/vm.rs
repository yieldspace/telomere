use std::ops::BitXor;

use crate::{
    common::{
        ElemInit, ElemMode, ExecuteContext, ExportDesc, Instance, InstanceAddr, Instr, JumpTable,
        LocalState, Stack, VMResult, ValType, WasmValue,
    },
    Store,
};

use super::{instantiate::execute_elem_init_const_expr, TABLE_UNINITIALIZED};
pub struct ResultValue(Vec<WasmValue>);
impl ResultValue {
    pub fn new(args: Vec<WasmValue>) -> Self {
        Self(args)
    }
    pub fn iter(&self) -> impl Iterator<Item = &WasmValue> + use<'_> {
        self.0.iter()
    }
}

#[inline(always)]
pub(crate) unsafe fn call_next(
    tail_code: *const Instr,
    consumed: isize,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    ((*tail_code.offset(consumed)).op)(tail_code.offset(consumed + 1), ctx)
}
pub unsafe fn op_i32_const(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let v = (*tail_code).operand.i32;
    trace!("op_i32_const: {v}");
    vm_try!(ctx.stack.push_i32(v));
    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_i32_add(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_i32();

    let a = ctx.stack.pop_i32();
    let r = a.wrapping_add(b);
    trace!("op_i32_add: {a} + {b} => {r}");

    vm_try!(ctx.stack.push_i32(r));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i32_sub(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let a = ctx.stack.pop_i32();
    let b = ctx.stack.pop_i32();
    let r = b.wrapping_sub(a);
    vm_try!(ctx.stack.push_i32(r));

    trace!("op_i32_sub: {a} {b} {r}");

    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i64_clz(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let a = ctx.stack.pop_i64();
    vm_try!(ctx.stack.push_i64(a.leading_zeros().into()));

    trace!("op_i64_ctz");

    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i64_ctz(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let a = ctx.stack.pop_i64();
    vm_try!(ctx.stack.push_i64(a.trailing_zeros().into()));

    trace!("op_i64_ctz");

    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i64_popcnt(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let a = ctx.stack.pop_i64();
    vm_try!(ctx.stack.push_i64(a.count_ones().into()));

    trace!("op_i64_ctz");

    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i64_sub(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let a = ctx.stack.pop_i64();
    let b = ctx.stack.pop_i64();
    let r = b.wrapping_sub(a);
    vm_try!(ctx.stack.push_i64(r));

    trace!("op_i64_sub: {a} {b} {r}");

    call_next(tail_code, 0, ctx)
}

pub unsafe fn op_i64_const(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    trace!("op_i64_const");
    vm_try!(ctx.stack.push_i64((*tail_code).operand.i64));
    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_f32_const(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    trace!("op_f32_const");
    vm_try!(ctx.stack.push_f32((*tail_code).operand.f32));
    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_f64_const(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    trace!("op_f64_const");
    vm_try!(ctx.stack.push_f64((*tail_code).operand.f64));
    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_f32_lt(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    trace!("op_f32_lt");
    let b = ctx.stack.pop_f32();
    let a = ctx.stack.pop_f32();
    vm_try!(ctx.stack.push_u32(if a < b { 1 } else { 0 }));

    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_f32_gt(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    trace!("op_f32_gt");
    let b = ctx.stack.pop_f32();

    let a = ctx.stack.pop_f32();
    vm_try!(ctx.stack.push_u32(if a > b { 1 } else { 0 }));

    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_f32_sqrt(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    trace!("op_f32_sqrt");
    let a = ctx.stack.pop_f32();
    vm_try!(ctx.stack.push_f32(a.sqrt()));

    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_f32_add(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    trace!("op_f32_add");
    let b = ctx.stack.pop_f32();
    let a = ctx.stack.pop_f32();

    vm_try!(ctx.stack.push_f32(a + b));

    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_f32_sub(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    trace!("op_f32_sub");
    let a = ctx.stack.pop_f32();

    let b = ctx.stack.pop_f32();

    vm_try!(ctx.stack.push_f32(b - a));

    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_f32_mul(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    trace!("op_f32_mul");
    let b = ctx.stack.pop_f32();
    let a = ctx.stack.pop_f32();

    vm_try!(ctx.stack.push_f32(a * b));

    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_f32_div(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    trace!("op_f32_div");
    let b = ctx.stack.pop_f32();
    let a = ctx.stack.pop_f32();

    vm_try!(ctx.stack.push_f32(a / b));

    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_f32_min(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    trace!("op_f32_min");
    let b = ctx.stack.pop_f32();
    let a = ctx.stack.pop_f32();
    let r = if a.is_nan() || b.is_nan() {
        f32::NAN
    } else if a == 0. && b == 0. && (a.is_sign_negative() || b.is_sign_negative()) {
        -0.0
    } else {
        f32::min(a, b)
    };
    vm_try!(ctx.stack.push_f32(r));

    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_f32_max(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    trace!("op_f32_max");
    let b = ctx.stack.pop_f32();
    let a = ctx.stack.pop_f32();

    let r = if a.is_nan() || b.is_nan() {
        f32::NAN
    } else if a == 0. && b == 0. && (a.is_sign_positive() || b.is_sign_positive()) {
        0.0
    } else {
        f32::max(a, b)
    };
    vm_try!(ctx.stack.push_f32(r));

    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_f32_copysign(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    trace!("op_f32_copysign");
    let b = ctx.stack.pop_f32();
    let a = ctx.stack.pop_f32();

    vm_try!(ctx.stack.push_f32(a.copysign(b)));

    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_f64_add(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    trace!("op_f64_add");
    let b = ctx.stack.pop_f64();
    let a = ctx.stack.pop_f64();

    vm_try!(ctx.stack.push_f64(a + b));

    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_f64_sub(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    trace!("op_f64_sub");
    let a = ctx.stack.pop_f64();

    let b = ctx.stack.pop_f64();

    vm_try!(ctx.stack.push_f64(b - a));

    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_f64_mul(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    trace!("op_f64_mul");
    let b = ctx.stack.pop_f64();
    let a = ctx.stack.pop_f64();

    vm_try!(ctx.stack.push_f64(a * b));

    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_f64_div(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    trace!("op_f64_div");
    let b = ctx.stack.pop_f64();
    let a = ctx.stack.pop_f64();

    vm_try!(ctx.stack.push_f64(a / b));

    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_f64_min(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    trace!("op_f64_min");
    let b = ctx.stack.pop_f64();
    let a = ctx.stack.pop_f64();
    let r = if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if a == 0. && b == 0. && (a.is_sign_negative() || b.is_sign_negative()) {
        -0.0
    } else {
        f64::min(a, b)
    };
    vm_try!(ctx.stack.push_f64(r));

    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_f64_max(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    trace!("op_f64_max");
    let b = ctx.stack.pop_f64();
    let a = ctx.stack.pop_f64();

    let r = if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if a == 0. && b == 0. && (a.is_sign_positive() || b.is_sign_positive()) {
        0.0
    } else {
        f64::max(a, b)
    };
    vm_try!(ctx.stack.push_f64(r));

    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_f64_copysign(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    trace!("op_f64_copysign");
    let b = ctx.stack.pop_f64();
    let a = ctx.stack.pop_f64();

    vm_try!(ctx.stack.push_f64(a.copysign(b)));

    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i32_wrap_i64(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    trace!("op_i32_wrap_i64");
    let a = ctx.stack.pop_i64();
    vm_try!(ctx.stack.push_i32(a as i32));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i32_trunc_f32_s(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let a = ctx.stack.pop_f32();
    if (i32::MIN as f32) <= a && a < (i32::MAX as f32) {
        vm_try!(ctx.stack.push_i32(a as i32));
    } else {
        return VMResult::InvalidOperand;
    }
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i32_trunc_f32_u(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let a = ctx.stack.pop_f32();
    if -1.0 < a && a < (u32::MAX as f32) {
        vm_try!(ctx.stack.push_u32(a.trunc() as u32));
    } else {
        return VMResult::InvalidOperand;
    }
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i32_trunc_f64_s(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let a = ctx.stack.pop_f64().trunc();
    if (i32::MIN as f64) <= a && a <= (i32::MAX as f64) {
        vm_try!(ctx.stack.push_i32(a as i32));
    } else {
        return VMResult::InvalidOperand;
    }
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i32_trunc_f64_u(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let a = ctx.stack.pop_f64().trunc();
    if -1.0 < a && a <= (u32::MAX as f64) {
        vm_try!(ctx.stack.push_u32(a as u32));
    } else {
        return VMResult::InvalidOperand;
    }
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i64_trunc_f32_s(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let a = ctx.stack.pop_f32();
    if (i64::MIN as f32) <= a && a < (i64::MAX as f32) {
        vm_try!(ctx.stack.push_i64(a as i64));
    } else {
        return VMResult::InvalidOperand;
    }
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i64_trunc_f32_u(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let a = ctx.stack.pop_f32();
    if -1.0 < a && a < (u64::MAX as f32) {
        vm_try!(ctx.stack.push_u64(a.trunc() as u64));
    } else {
        return VMResult::InvalidOperand;
    }
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i64_trunc_f64_s(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let a = ctx.stack.pop_f64();
    if (i64::MIN as f64) <= a && a < (i64::MAX as f64) {
        vm_try!(ctx.stack.push_i64(a as i64));
    } else {
        return VMResult::InvalidOperand;
    }
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i64_trunc_f64_u(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let a = ctx.stack.pop_f64();
    if -1. < a && a < (u64::MAX as f64) {
        vm_try!(ctx.stack.push_u64(a as u64));
    } else {
        return VMResult::InvalidOperand;
    }
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i32_trunc_sat_f32_s(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let a = ctx.stack.pop_f32();
    vm_try!(ctx.stack.push_i32(a.trunc() as i32));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i32_trunc_sat_f32_u(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let a = ctx.stack.pop_f32();
    vm_try!(ctx.stack.push_u32(a.trunc() as u32));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i32_trunc_sat_f64_s(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let a = ctx.stack.pop_f64();
    vm_try!(ctx.stack.push_i32(a.trunc() as i32));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i32_trunc_sat_f64_u(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let a = ctx.stack.pop_f64();
    vm_try!(ctx.stack.push_u32(a.trunc() as u32));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i64_trunc_sat_f32_s(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let a = ctx.stack.pop_f32();
    vm_try!(ctx.stack.push_i64(a.trunc() as i64));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i64_trunc_sat_f32_u(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let a = ctx.stack.pop_f32();
    vm_try!(ctx.stack.push_u64(a.trunc() as u64));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i64_trunc_sat_f64_s(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let a = ctx.stack.pop_f64();
    vm_try!(ctx.stack.push_i64(a.trunc() as i64));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i64_trunc_sat_f64_u(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let a = ctx.stack.pop_f64();
    vm_try!(ctx.stack.push_u64(a.trunc() as u64));
    call_next(tail_code, 0, ctx)
}

pub unsafe fn op_i64_add(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let a = ctx.stack.pop_i64();
    let b = ctx.stack.pop_i64();
    vm_try!(ctx.stack.push_i64(a.wrapping_add(b)));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i64_extend_i32_s(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let a = ctx.stack.pop_i32();
    vm_try!(ctx.stack.push_i64(a.into()));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i64_extend_i32_u(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let a = ctx.stack.pop_u32();
    vm_try!(ctx.stack.push_i64(a.into()));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_f32_convert_i32_s(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let a = ctx.stack.pop_i32();
    vm_try!(ctx.stack.push_f32(a as f32));
    call_next(tail_code, 0, ctx)
}

pub unsafe fn op_f32_convert_i32_u(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let a = ctx.stack.pop_u32();
    vm_try!(ctx.stack.push_f32(a as f32));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_f32_convert_i64_s(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let a = ctx.stack.pop_i64();
    vm_try!(ctx.stack.push_f32(a as f32));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_f32_convert_i64_u(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let a = ctx.stack.pop_u64();
    vm_try!(ctx.stack.push_f32(a as f32));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_f32_demote_f64(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let a = ctx.stack.pop_f64();
    vm_try!(ctx.stack.push_f32(a as f32));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_f64_convert_i32_s(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let a = ctx.stack.pop_i32();
    vm_try!(ctx.stack.push_f64(a as f64));
    call_next(tail_code, 0, ctx)
}

pub unsafe fn op_f64_convert_i32_u(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let a = ctx.stack.pop_u32();
    vm_try!(ctx.stack.push_f64(a as f64));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_f64_convert_i64_s(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let a = ctx.stack.pop_i64();
    vm_try!(ctx.stack.push_f64(a as f64));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_f64_convert_i64_u(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let a = ctx.stack.pop_u64();
    vm_try!(ctx.stack.push_f64(a as f64));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_f64_promote_f32(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let a = ctx.stack.pop_f32();
    vm_try!(ctx.stack.push_f64(a.into()));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_return(_tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let addr = ctx.jump_table().ret();
    trace!("op_return: {addr}");
    let code = ctx.code();
    let tail_code = code.offset(addr as isize);
    call_next(tail_code, 0, ctx)
}

pub unsafe fn op_end(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    trace!("op_end");

    ctx.jump_table().end();
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_br(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let addr = ctx
        .jump_table()
        .br((*tail_code).operand.u32 as usize)
        .unwrap_unchecked();
    trace!("op_br: {addr}");

    let tail_code = ctx.code().offset(addr as isize);
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_else(_tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    trace!("op_else");

    let addr = ctx.jump_table().br(0).unwrap_unchecked();
    let tail_code = ctx.code().offset(addr as isize);
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_br_if(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let cond = ctx.stack.pop_u32();
    trace!("op_br_if: {cond}");

    let ptr = if cond != 0 {
        let addr = ctx
            .jump_table()
            .br((*tail_code).operand.u32 as usize)
            .unwrap_unchecked();

        ctx.code().offset(addr as isize)
    } else {
        tail_code.offset(1)
    };
    call_next(ptr, 0, ctx)
}
pub unsafe fn op_br_table(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let index = ctx.stack.pop_u32();
    let table_size = (*tail_code).operand.u32;

    let idx = if index < table_size {
        (*tail_code.offset((index + 1) as isize)).operand.u32
    } else {
        (*tail_code.offset((table_size + 1) as isize)).operand.u32
    };
    trace!(
        "op_br_table: index={} table_size={} => jump_idx={}",
        index,
        table_size,
        idx
    );
    let addr = ctx.jump_table().br(idx as usize).unwrap_unchecked();

    let tail_code = ctx.code().offset(addr as isize);
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_block(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    trace!("op_block: {}", (*tail_code).operand.jump_addr);
    ctx.jump_table().push((*tail_code).operand.jump_addr);
    call_next(tail_code, 1, ctx)
}

pub unsafe fn op_loop(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    ctx.jump_table().push((*tail_code).operand.jump_addr);
    trace!(
        "op_loop: {} {:?}",
        (*tail_code).operand.jump_addr,
        ctx.jump_table()
    );

    let loop_param = (*tail_code.offset(1)).operand.loop_param;
    ctx.stack.block_return(
        &ctx.local_reference(),
        loop_param.stack_top as usize,
        loop_param.param_size as usize,
    );

    call_next(tail_code, 2, ctx)
}
pub unsafe fn op_if(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let (end_addr, else_addr) = (*tail_code).operand.jump_addr2;
    ctx.jump_table().push(end_addr);
    let v = ctx.stack.pop_u32();
    trace!("op_if: {end_addr} {else_addr} {v} {:?}", ctx.jump_table());

    let ptr = if v == 0 {
        ctx.code().offset(else_addr as isize)
    } else {
        tail_code.offset(1)
    };
    call_next(ptr, 0, ctx)
}
const MAX_CALL_STACK: usize = 10000;
// Required for direct function call threading.
// If unset, LLVM will not replace the end of op_call with a jump.
#[inline(never)]
pub(crate) unsafe fn internal_op_call(
    return_addr: *const Instr,
    funcaddr: u32,
    ctx: &mut ExecuteContext,
) -> VMResult<*const Instr> {
    if ctx.local_state.len() > MAX_CALL_STACK {
        return VMResult::StackOverflow;
    }
    //FIXME: unwrap

    let funcinst = &ctx.store.funcs.0[funcaddr as usize];
    let instance_addr = funcinst.instance_addr;
    let instance = &ctx.store.instances[instance_addr as usize];
    let module_addr = instance.module_addr;
    let module = &ctx.store.modules[module_addr as usize];
    let typeidx = module
        .functions
        .get(funcinst.funcidx as usize)
        .unwrap_unchecked();
    let ft = &module.function_types[typeidx.0 as usize];
    let code = &funcinst.body;
    let mut jump_table = JumpTable::new();
    jump_table.push(code.expr.len() as u32 - 2);

    let mut param_size = 0usize;
    for t in ft.0.iter() {
        param_size += t.stack_size().usize();
    }
    let mut local_size = 0usize;
    for local in &code.locals {
        local_size += local.n as usize * local.t.stack_size().usize();
    }
    let local_reference = vm_try!(ctx.stack.function_call(param_size, local_size, return_addr));
    trace!(
        "op_call_internal: {} @ {instance_addr}({module_addr})  {funcaddr} {local_size} {:?} {:?} {:?}",
        funcinst.funcidx,
        ft,
        code.locals,
        local_reference
    );
    ctx.local_state.push(LocalState {
        local_reference,
        jump_table,
        code_addr: funcaddr,
        instance_addr: funcinst.instance_addr,
    });
    VMResult::Success(code.expr.as_ptr())
}

pub unsafe fn op_call(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let funcidx = (*tail_code).operand.u32;
    let funcaddr = ctx.instance().funcs[funcidx as usize];
    let ptr = vm_try!(internal_op_call(tail_code.offset(1), funcaddr, ctx));
    call_next(ptr, 0, ctx)
}

#[inline(never)]
unsafe fn internal_op_call_indirect(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<*const Instr> {
    let i = ctx.stack.pop_u32();
    let tableidx = (*tail_code).operand.u32 as usize;
    let table_addr = *vm_try!(VMResult::from_option(
        ctx.instance().tables.get(tableidx),
        || { VMResult::TableIndexOutOfRange }
    ));
    let table = &mut ctx.store.tables[table_addr as usize];
    let func_addr = *vm_try!(VMResult::from_option(table.1.get(i as usize), || {
        VMResult::TableIndexOutOfRange
    }));
    trace!("internal_op_call_indirect: {tableidx} {table_addr} {func_addr} {table:?}");
    if func_addr == TABLE_UNINITIALIZED {
        return VMResult::TableUninitialized;
    }

    let funcinst = &ctx.store.funcs.0[func_addr as usize];
    let instance = &ctx.store.instances[funcinst.instance_addr as usize];
    let module = &ctx.store.modules[instance.module_addr as usize];
    let actual_typeidx = module.functions.get(funcinst.funcidx as usize).unwrap();
    let actual_ft = &module.function_types[actual_typeidx.0 as usize];
    let expected_typeidx = (*tail_code.offset(1)).operand.u32;
    let expected_ft = ctx
        .module()
        .function_types
        .get(expected_typeidx as usize)
        .unwrap();
    trace!("{:?} {:?}", actual_ft, expected_ft);
    if actual_ft != expected_ft {
        return VMResult::CallIndirectInvalidType;
    }
    let ptr = vm_try!(internal_op_call(tail_code.offset(2), func_addr, ctx));
    VMResult::Success(ptr)
}
pub unsafe fn op_call_indirect(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let ptr = vm_try!(internal_op_call_indirect(tail_code, ctx));
    call_next(ptr, 0, ctx)
}
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
    let v = if cond == 0 { a } else { b };
    trace!("op_select: {x} {cond} {a:?} {b:?} => {v:?}");
    vm_try!(ctx.stack.push_slice(&v[0..x]));
    VMResult::Success(())
}
pub unsafe fn op_select(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    vm_try!(internal_op_select(tail_code, ctx));
    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_local_get4(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let addr = (*tail_code).operand.local_addr as usize;
    vm_try!(ctx.stack.local_get(&ctx.local_reference(), addr, 4));
    trace!("op_local_get4: {addr}");

    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_local_get8(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let addr = (*tail_code).operand.local_addr as usize;
    vm_try!(ctx.stack.local_get(&ctx.local_reference(), addr, 8));
    trace!("op_local_get8: {addr}");

    call_next(tail_code, 1, ctx)
}

pub unsafe fn op_local_set4(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let addr = (*tail_code).operand.local_addr as usize;
    ctx.stack.local_set(&ctx.local_reference(), addr, 4);
    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_local_set8(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let addr = (*tail_code).operand.local_addr as usize;
    ctx.stack.local_set(&ctx.local_reference(), addr, 8);

    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_local_tee4(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let addr = (*tail_code).operand.local_addr as usize;
    ctx.stack.local_tee(&ctx.local_reference(), addr, 4);
    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_local_tee8(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let addr = (*tail_code).operand.local_addr as usize;
    ctx.stack.local_tee(&ctx.local_reference(), addr, 8);

    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_global_get4(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let idx = (*tail_code).operand.u32 as usize;
    let addr = ctx.instance().globals[idx] as usize;
    vm_try!(ctx.stack.push_slice(&ctx.store.globals.0[addr..addr + 4]));
    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_global_get8(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let idx = (*tail_code).operand.u32 as usize;
    let addr = ctx.instance().globals[idx] as usize;
    vm_try!(ctx.stack.push_slice(&ctx.store.globals.0[addr..addr + 8]));
    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_global_set4(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let idx = (*tail_code).operand.u32 as usize;
    let addr = ctx.instance().globals[idx] as usize;
    ctx.store.globals.0[addr..addr + 4].copy_from_slice(&ctx.stack.pop_u8_array::<4>());
    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_global_set8(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let idx = (*tail_code).operand.u32 as usize;
    let addr = ctx.instance().globals[idx] as usize;
    ctx.store.globals.0[addr..addr + 8].copy_from_slice(&ctx.stack.pop_u8_array::<8>());
    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_table_get(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let idx = (*tail_code).operand.u32 as usize;
    let addr = ctx.instance().tables[idx] as usize;
    let inst = &mut ctx.store.tables[addr];
    let i = ctx.stack.pop_u32();
    if i as usize >= inst.1.len() {
        return VMResult::TableIndexOutOfRange;
    }
    let val = inst.1[i as usize];
    trace!("op_table_get: {idx} {addr} {i} {val}");

    vm_try!(ctx.stack.push_u32(val));
    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_table_set(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let idx = (*tail_code).operand.u32 as usize;
    let addr = ctx.instance().tables[idx] as usize;
    let inst = &mut ctx.store.tables[addr];
    let val = ctx.stack.pop_u32();
    let i = ctx.stack.pop_u32();
    trace!("op_table_set: {idx} {addr} {i} {val}");

    if i as usize >= inst.1.len() {
        return VMResult::TableIndexOutOfRange;
    }
    inst.1[i as usize] = val;
    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_table_init(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let len = ctx.stack.pop_u32() as usize;
    let src = ctx.stack.pop_u32() as usize;
    let dst = ctx.stack.pop_u32() as usize;
    let src_elem_idx = (*tail_code).operand.u32;
    let dst_table_idx = (*tail_code.offset(1)).operand.u32 as usize;

    let ExecuteContext {
        local_state, store, ..
    } = ctx;
    let ls = local_state.last().unwrap_unchecked();
    let instance_addr = ls.instance_addr;
    let Store {
        instances,
        tables,
        globals: global_store,
        elems,
        ..
    } = store;
    let instance = &mut instances[instance_addr as usize];
    let dst_table_addr = instance.tables[dst_table_idx] as usize;

    let elem = if let Some(elem) = elems.get(&(instance_addr, src_elem_idx)) {
        elem
    } else {
        return VMResult::TableIndexOutOfRange;
    };

    let dst_table = &mut tables[dst_table_addr];
    let dst = vm_try!(VMResult::from_option(
        dst_table.1.get_mut(dst..dst + len),
        || { VMResult::TableIndexOutOfRange }
    ));
    match &elem.init {
        ElemInit::FuncIdx(idxs) => {
            let slice = vm_try!(VMResult::from_option(idxs.get(src..(src + len)), || {
                VMResult::TableIndexOutOfRange
            }));
            for (i, funcidx) in slice.iter().enumerate() {
                dst[i] = instance.funcs[*funcidx as usize];
            }
        }
        ElemInit::ConstExpr(exprs) => {
            let slice = vm_try!(VMResult::from_option(exprs.get(src..(src + len)), || {
                VMResult::TableIndexOutOfRange
            }));
            for (i, expr) in slice.iter().enumerate() {
                dst[i] = vm_try!(execute_elem_init_const_expr(
                    global_store,
                    &instance.globals,
                    &instance.funcs,
                    expr,
                    dst_table.0.reftype,
                ));
            }
        }
    }

    call_next(tail_code, 2, ctx)
}
pub unsafe fn op_elem_drop(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let elem_idx = (*tail_code).operand.u32;
    let ls = ctx.local_state.last().unwrap_unchecked();
    let instance_addr = ls.instance_addr;
    ctx.store.elems.remove(&(instance_addr, elem_idx));
    call_next(tail_code, 1, ctx)
}

pub unsafe fn op_table_copy(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let len = ctx.stack.pop_u32() as usize;
    let src = ctx.stack.pop_u32() as usize;
    let dst = ctx.stack.pop_u32() as usize;
    let dst_table_idx = (*tail_code).operand.u32 as usize;
    let src_table_idx = (*tail_code.offset(1)).operand.u32 as usize;

    let src_table_addr = ctx.instance().tables[src_table_idx] as usize;
    let dst_table_addr = ctx.instance().tables[dst_table_idx] as usize;
    let src_table = &ctx.store.tables[src_table_addr].1;
    let src_ptr = vm_try!(VMResult::from_option(src_table.get(src..src + len), || {
        VMResult::TableIndexOutOfRange
    }))
    .as_ptr();
    let dst_table = &mut ctx.store.tables[dst_table_addr].1;
    let dst_ptr = vm_try!(VMResult::from_option(
        dst_table.get_mut(dst..dst + len),
        || { VMResult::TableIndexOutOfRange }
    ))
    .as_mut_ptr();
    std::ptr::copy(src_ptr, dst_ptr, len);
    call_next(tail_code, 2, ctx)
}
macro_rules! memory_try {
    ($ctx: expr) => {
        if let Some(v) = $ctx.memory() {
            v
        } else {
            return VMResult::MemoryIndexOutOfRange;
        }
    };
}
pub unsafe fn op_i32_load(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let memarg = (*tail_code).operand.memarg;
    let offset = ctx.stack.pop_u32();
    let memory = memory_try!(ctx);
    let v = vm_try!(memory.read_u32(memarg, offset));
    vm_try!(ctx.stack.push_u32(v));
    trace!("op_i32_load: {:?} {} => {v}", memarg, offset);
    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_i64_load(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let memarg = (*tail_code).operand.memarg;
    let offset = ctx.stack.pop_u32();
    let memory = memory_try!(ctx);
    let v = vm_try!(memory.read_u64(memarg, offset));
    vm_try!(ctx.stack.push_u64(v));
    trace!("op_i64_load: {:?} {} => {v}", memarg, offset);
    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_f32_load(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let memarg = (*tail_code).operand.memarg;
    let offset = ctx.stack.pop_u32();
    let memory = memory_try!(ctx);
    let v = vm_try!(memory.read_f32(memarg, offset));
    vm_try!(ctx.stack.push_f32(v));
    trace!("op_f32_load: {:?} {} => {v}", memarg, offset);
    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_f64_load(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let memarg = (*tail_code).operand.memarg;
    let offset = ctx.stack.pop_u32();
    let memory = memory_try!(ctx);
    let v = vm_try!(memory.read_f64(memarg, offset));
    vm_try!(ctx.stack.push_f64(v));
    trace!("op_f64_load: {:?} {} => {v}", memarg, offset);
    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_i32_load8_u(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let memarg = (*tail_code).operand.memarg;
    let offset = ctx.stack.pop_u32();
    let memory = memory_try!(ctx);
    let v = vm_try!(memory.read_u8(memarg, offset)) as u32;
    vm_try!(ctx.stack.push_u32(v));
    trace!("op_i32_load8_u: {:?} {} => {v}", memarg, offset);
    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_i32_load8_s(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let memarg = (*tail_code).operand.memarg;
    let offset = ctx.stack.pop_u32();
    let memory = memory_try!(ctx);
    let v = vm_try!(memory.read_i8(memarg, offset)) as i32;
    vm_try!(ctx.stack.push_i32(v));
    trace!("op_i32_load8_s: {:?} {} => {v}", memarg, offset);
    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_i32_load16_s(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let memarg = (*tail_code).operand.memarg;
    let offset = ctx.stack.pop_u32();
    let memory = memory_try!(ctx);
    let v = vm_try!(memory.read_i16(memarg, offset)) as i32;
    vm_try!(ctx.stack.push_i32(v));
    trace!("op_i32_load8_s: {:?} {} => {v}", memarg, offset);
    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_i32_load16_u(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let memarg = (*tail_code).operand.memarg;
    let offset = ctx.stack.pop_u32();
    let memory = memory_try!(ctx);
    let v = vm_try!(memory.read_u16(memarg, offset)) as u32;
    vm_try!(ctx.stack.push_u32(v));
    trace!("op_i32_load8_s: {:?} {} => {v}", memarg, offset);
    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_i64_load8_s(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let memarg = (*tail_code).operand.memarg;
    let offset = ctx.stack.pop_u32();
    let memory = memory_try!(ctx);
    let v = vm_try!(memory.read_i8(memarg, offset)) as i64;
    vm_try!(ctx.stack.push_i64(v));
    trace!("op_i64_load8_s: {:?} {} => {v}", memarg, offset);
    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_i64_load8_u(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let memarg = (*tail_code).operand.memarg;
    let offset = ctx.stack.pop_u32();
    let memory = memory_try!(ctx);
    let v = vm_try!(memory.read_u8(memarg, offset)) as u64;
    vm_try!(ctx.stack.push_u64(v));
    trace!("op_i64_load8_u: {:?} {} => {v}", memarg, offset);
    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_i64_load16_s(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let memarg = (*tail_code).operand.memarg;
    let offset = ctx.stack.pop_u32();
    let memory = memory_try!(ctx);
    let v = vm_try!(memory.read_i16(memarg, offset)) as i64;
    vm_try!(ctx.stack.push_i64(v));
    trace!("op_i64_load16_s: {:?} {} => {v}", memarg, offset);
    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_i64_load16_u(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let memarg = (*tail_code).operand.memarg;
    let offset = ctx.stack.pop_u32();
    let memory = memory_try!(ctx);
    let v = vm_try!(memory.read_u16(memarg, offset)) as u64;
    vm_try!(ctx.stack.push_u64(v));
    trace!("op_i64_load16_u: {:?} {} => {v}", memarg, offset);
    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_i64_load32_s(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let memarg = (*tail_code).operand.memarg;
    let offset = ctx.stack.pop_u32();
    let memory = memory_try!(ctx);
    let v = vm_try!(memory.read_i32(memarg, offset)) as i64;
    vm_try!(ctx.stack.push_i64(v));
    trace!("op_i64_load32_s: {:?} {} => {v}", memarg, offset);
    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_i64_load32_u(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let memarg = (*tail_code).operand.memarg;
    let offset = ctx.stack.pop_u32();
    let memory = memory_try!(ctx);

    let v = vm_try!(memory.read_u32(memarg, offset)) as u64;
    vm_try!(ctx.stack.push_u64(v));
    trace!("op_i64_load32_u: {:?} {} => {v}", memarg, offset);
    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_i32_store(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let memarg = (*tail_code).operand.memarg;
    let v = ctx.stack.pop_u32();
    let offset = ctx.stack.pop_u32();
    trace!("op_i32_store: {:?} offset={} value={v}", memarg, offset);
    let memory = memory_try!(ctx);
    vm_try!(memory.write_u32(memarg, offset, v));
    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_i64_store(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let memarg = (*tail_code).operand.memarg;
    let v = ctx.stack.pop_u64();
    let offset = ctx.stack.pop_u32();
    trace!("op_i64_store: {:?} offset={} value={v}", memarg, offset);
    let memory = memory_try!(ctx);
    vm_try!(memory.write_u64(memarg, offset, v));
    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_f32_store(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let memarg = (*tail_code).operand.memarg;
    let v = ctx.stack.pop_f32();
    let offset = ctx.stack.pop_u32();
    trace!("op_i32_store: {:?} offset={} value={v}", memarg, offset);
    let memory = memory_try!(ctx);
    vm_try!(memory.write_f32(memarg, offset, v));
    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_f64_store(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let memarg = (*tail_code).operand.memarg;
    let v = ctx.stack.pop_f64();
    let offset = ctx.stack.pop_u32();
    trace!("op_i32_store: {:?} offset={} value={v}", memarg, offset);
    let memory = memory_try!(ctx);
    vm_try!(memory.write_f64(memarg, offset, v));
    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_i32_store8(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let memarg = (*tail_code).operand.memarg;
    let v = ctx.stack.pop_u32();
    let offset = ctx.stack.pop_u32();
    trace!("op_i32_store: {:?} offset={} value={v}", memarg, offset);
    let memory = memory_try!(ctx);
    vm_try!(memory.write_u8(memarg, offset, v as u8));
    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_i32_store16(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let memarg = (*tail_code).operand.memarg;
    let v = ctx.stack.pop_u32();
    let offset = ctx.stack.pop_u32();
    trace!("op_i32_store: {:?} offset={} value={v}", memarg, offset);
    let memory = memory_try!(ctx);
    vm_try!(memory.write_u16(memarg, offset, v as u16));
    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_i64_store8(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let memarg = (*tail_code).operand.memarg;
    let v = ctx.stack.pop_u64();
    let offset = ctx.stack.pop_u32();
    trace!("op_i64_store8: {:?} offset={} value={v}", memarg, offset);
    let memory = memory_try!(ctx);
    vm_try!(memory.write_u8(memarg, offset, v as u8));
    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_i64_store16(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let memarg = (*tail_code).operand.memarg;
    let v = ctx.stack.pop_u64();
    let offset = ctx.stack.pop_u32();
    trace!("op_i64_store16: {:?} offset={} value={v}", memarg, offset);
    let memory = memory_try!(ctx);
    vm_try!(memory.write_u16(memarg, offset, v as u16));
    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_i64_store32(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let memarg = (*tail_code).operand.memarg;
    let v = ctx.stack.pop_u64();
    let offset = ctx.stack.pop_u32();
    trace!("op_i64_store32: {:?} offset={} value={v}", memarg, offset);
    let memory = memory_try!(ctx);
    vm_try!(memory.write_u32(memarg, offset, v as u32));
    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_f32_abs(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let a = ctx.stack.pop_f32();
    vm_try!(ctx.stack.push_f32(a.abs()));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_f32_neg(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let a = ctx.stack.pop_f32();
    vm_try!(ctx.stack.push_f32(-a));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_f32_ceil(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let a = ctx.stack.pop_f32();
    vm_try!(ctx.stack.push_f32(a.ceil()));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_f32_floor(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let a = ctx.stack.pop_f32();
    vm_try!(ctx.stack.push_f32(a.floor()));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_f32_trunc(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let a = ctx.stack.pop_f32();
    vm_try!(ctx.stack.push_f32(a.trunc()));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_f32_nearest(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let a = ctx.stack.pop_f32();
    vm_try!(ctx.stack.push_f32(a.round_ties_even()));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_f64_ceil(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let a = ctx.stack.pop_f64();
    vm_try!(ctx.stack.push_f64(a.ceil()));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_f64_floor(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let a = ctx.stack.pop_f64();
    vm_try!(ctx.stack.push_f64(a.floor()));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_f64_trunc(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let a = ctx.stack.pop_f64();
    vm_try!(ctx.stack.push_f64(a.trunc()));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_f64_nearest(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let a = ctx.stack.pop_f64();
    vm_try!(ctx.stack.push_f64(a.round_ties_even()));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_f64_abs(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let a = ctx.stack.pop_f64();
    vm_try!(ctx.stack.push_f64(a.abs()));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_f64_neg(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let a = ctx.stack.pop_f64();
    vm_try!(ctx.stack.push_f64(-a));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_f64_sqrt(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let a = ctx.stack.pop_f64();
    vm_try!(ctx.stack.push_f64(a.sqrt()));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_f32_eq(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let a = ctx.stack.pop_f32();
    let b = ctx.stack.pop_f32();
    vm_try!(ctx.stack.push_u32(if a == b { 1 } else { 0 }));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_f32_ne(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let a = ctx.stack.pop_f32();
    let b = ctx.stack.pop_f32();
    vm_try!(ctx.stack.push_u32(if a != b { 1 } else { 0 }));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_f32_le(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_f32();
    let a = ctx.stack.pop_f32();
    vm_try!(ctx.stack.push_u32(if a <= b { 1 } else { 0 }));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_f32_ge(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_f32();
    let a = ctx.stack.pop_f32();
    vm_try!(ctx.stack.push_u32(if a >= b { 1 } else { 0 }));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_f64_eq(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_f64();
    let a = ctx.stack.pop_f64();
    vm_try!(ctx.stack.push_u32(if a == b { 1 } else { 0 }));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_f64_ne(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_f64();
    let a = ctx.stack.pop_f64();
    vm_try!(ctx.stack.push_u32(if a != b { 1 } else { 0 }));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_f64_lt(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_f64();
    let a = ctx.stack.pop_f64();
    vm_try!(ctx.stack.push_u32(if a < b { 1 } else { 0 }));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_f64_gt(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_f64();
    let a = ctx.stack.pop_f64();
    vm_try!(ctx.stack.push_u32(if a > b { 1 } else { 0 }));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_f64_le(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_f64();
    let a = ctx.stack.pop_f64();
    vm_try!(ctx.stack.push_u32(if a <= b { 1 } else { 0 }));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_f64_ge(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_f64();
    let a = ctx.stack.pop_f64();
    vm_try!(ctx.stack.push_u32(if a >= b { 1 } else { 0 }));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i32_ctz(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let v = ctx.stack.pop_u32().trailing_zeros();
    vm_try!(ctx.stack.push_u32(v));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i32_clz(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let v = ctx.stack.pop_u32().leading_zeros();
    vm_try!(ctx.stack.push_u32(v));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i32_popcnt(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let v = ctx.stack.pop_u32().count_ones();
    vm_try!(ctx.stack.push_u32(v));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i32_mul(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let a = ctx.stack.pop_i32();
    let b = ctx.stack.pop_i32();
    let r = a.wrapping_mul(b);
    vm_try!(ctx.stack.push_i32(r));
    trace!("op_i32_mul: {a} {b} => {r}");
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i32_div_s(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let a = ctx.stack.pop_i32();
    let b = ctx.stack.pop_i32();

    let r = vm_try!(VMResult::from_option(b.checked_div(a), || {
        VMResult::InvalidOperand
    }));
    vm_try!(ctx.stack.push_i32(r));
    trace!("op_i32_div_s: {a} {b} => {r}");
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i32_div_u(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let a = ctx.stack.pop_u32();
    let b = ctx.stack.pop_u32();
    let r = vm_try!(VMResult::from_option(b.checked_div(a), || {
        VMResult::InvalidOperand
    }));
    vm_try!(ctx.stack.push_u32(r));
    trace!("op_i32_div_u: {a} {b} => {r}");
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i64_div_s(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let a = ctx.stack.pop_i64();
    let b = ctx.stack.pop_i64();

    let r = vm_try!(VMResult::from_option(b.checked_div(a), || {
        VMResult::InvalidOperand
    }));
    vm_try!(ctx.stack.push_i64(r));
    trace!("op_i64_div_s: {a} {b} => {r}");
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i64_div_u(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let a = ctx.stack.pop_u64();
    let b = ctx.stack.pop_u64();
    let r = vm_try!(VMResult::from_option(b.checked_div(a), || {
        VMResult::InvalidOperand
    }));
    vm_try!(ctx.stack.push_u64(r));
    trace!("op_i64_div_u: {a} {b} => {r}");
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i64_and(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_u64();
    let a = ctx.stack.pop_u64();
    vm_try!(ctx.stack.push_u64(a & b));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i64_or(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_u64();
    let a = ctx.stack.pop_u64();
    vm_try!(ctx.stack.push_u64(a | b));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i64_xor(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_u64();
    let a = ctx.stack.pop_u64();
    vm_try!(ctx.stack.push_u64(a.bitxor(b)));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i64_mul(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let a = ctx.stack.pop_i64();
    let b = ctx.stack.pop_i64();
    let r = a.wrapping_mul(b);
    vm_try!(ctx.stack.push_i64(r));
    trace!("op_i64_mul: {a} {b} => {r}");
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i32_rem_s(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_i32();
    let a = ctx.stack.pop_i32();
    if b == 0 {
        return VMResult::InvalidOperand;
    }
    vm_try!(ctx.stack.push_i32(a.wrapping_rem(b)));

    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i32_rem_u(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_u32();
    let a = ctx.stack.pop_u32();
    let r = vm_try!(VMResult::from_option(a.checked_rem(b), || {
        VMResult::InvalidOperand
    }));
    vm_try!(ctx.stack.push_u32(r));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i64_rem_s(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_i64();
    let a = ctx.stack.pop_i64();
    if b == 0 {
        return VMResult::InvalidOperand;
    }
    vm_try!(ctx.stack.push_i64(a.wrapping_rem(b)));

    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i64_rem_u(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_u64();
    let a = ctx.stack.pop_u64();
    let r = vm_try!(VMResult::from_option(a.checked_rem(b), || {
        VMResult::InvalidOperand
    }));
    vm_try!(ctx.stack.push_u64(r));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i32_and(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_u32();
    let a = ctx.stack.pop_u32();
    vm_try!(ctx.stack.push_u32(a & b));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i32_or(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_u32();
    let a = ctx.stack.pop_u32();
    vm_try!(ctx.stack.push_u32(a | b));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i32_xor(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_u32();
    let a = ctx.stack.pop_u32();
    vm_try!(ctx.stack.push_u32(a.bitxor(b)));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i32_shl(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_i32();
    let a = ctx.stack.pop_i32();

    vm_try!(ctx.stack.push_i32(a << b));

    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i32_shr_s(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_i32();
    let a = ctx.stack.pop_i32();

    vm_try!(ctx.stack.push_i32(a >> b));

    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i32_shr_u(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_u32();
    let a = ctx.stack.pop_u32();

    vm_try!(ctx.stack.push_u32(a >> b));

    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i32_rotl(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_u32();
    let a = ctx.stack.pop_u32();

    vm_try!(ctx.stack.push_u32(a.rotate_left(b)));

    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i32_rotr(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_u32();
    let a = ctx.stack.pop_u32();

    vm_try!(ctx.stack.push_u32(a.rotate_right(b)));

    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i64_rotl(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_u64();
    let a = ctx.stack.pop_u64();

    vm_try!(ctx.stack.push_u64(a.rotate_left(b as u32)));

    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i64_rotr(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_u64();
    let a = ctx.stack.pop_u64();

    vm_try!(ctx.stack.push_u64(a.rotate_right(b as u32)));

    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i64_shl(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_i64();
    let a = ctx.stack.pop_i64();

    vm_try!(ctx.stack.push_i64(a << b));

    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i64_shr_s(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_i64();
    let a = ctx.stack.pop_i64();

    vm_try!(ctx.stack.push_i64(a >> b));

    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i64_shr_u(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_u64();
    let a = ctx.stack.pop_u64();

    vm_try!(ctx.stack.push_u64(a >> b));

    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i32_eqz(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let a = ctx.stack.pop_u32();
    vm_try!(ctx.stack.push_u32(if a == 0 { 1 } else { 0 }));

    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i64_eqz(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let a = ctx.stack.pop_u64();
    let r = if a == 0 { 1 } else { 0 };
    trace!("op_i64_eqz: {a} => {r}");
    vm_try!(ctx.stack.push_u32(r));

    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i64_eq(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_u64();
    let a = ctx.stack.pop_u64();

    vm_try!(ctx.stack.push_u32(if a == b { 1 } else { 0 }));

    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i64_ne(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_u64();
    let a = ctx.stack.pop_u64();

    vm_try!(ctx.stack.push_u32(if a != b { 1 } else { 0 }));

    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i64_lt_s(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_i64();
    let a = ctx.stack.pop_i64();

    vm_try!(ctx.stack.push_u32(if a < b { 1 } else { 0 }));

    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i64_lt_u(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_u64();
    let a = ctx.stack.pop_u64();

    vm_try!(ctx.stack.push_u32(if a < b { 1 } else { 0 }));

    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i64_gt_s(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_i64();
    let a = ctx.stack.pop_i64();

    vm_try!(ctx.stack.push_u32(if a > b { 1 } else { 0 }));

    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i64_gt_u(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_u64();
    let a = ctx.stack.pop_u64();

    vm_try!(ctx.stack.push_u32(if a > b { 1 } else { 0 }));

    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i64_le_s(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_i64();
    let a = ctx.stack.pop_i64();

    vm_try!(ctx.stack.push_u32(if a <= b { 1 } else { 0 }));

    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i64_le_u(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_u64();
    let a = ctx.stack.pop_u64();

    vm_try!(ctx.stack.push_u32(if a <= b { 1 } else { 0 }));

    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i64_ge_s(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_i64();
    let a = ctx.stack.pop_i64();

    vm_try!(ctx.stack.push_u32(if a >= b { 1 } else { 0 }));

    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i64_ge_u(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_u64();
    let a = ctx.stack.pop_u64();

    vm_try!(ctx.stack.push_u32(if a >= b { 1 } else { 0 }));

    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i32_eq(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let a = ctx.stack.pop_u32();
    let b = ctx.stack.pop_u32();
    let r = if a == b { 1 } else { 0 };
    trace!("op_i32_eq: {a} {b} => {r}");

    vm_try!(ctx.stack.push_u32(r));

    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i32_ne(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let a = ctx.stack.pop_u32();
    let b = ctx.stack.pop_u32();
    let r = if a != b { 1 } else { 0 };
    trace!("op_i32_ne: {a} {b} => {r}");

    vm_try!(ctx.stack.push_u32(r));

    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i32_le_s(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_i32();
    let a = ctx.stack.pop_i32();
    let r = if a <= b { 1 } else { 0 };
    trace!("op_i32_le_s: {a} {b} => {r}");

    vm_try!(ctx.stack.push_u32(r));

    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i32_le_u(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_u32();
    let a = ctx.stack.pop_u32();
    let r = if a <= b { 1 } else { 0 };
    trace!("op_i32_le_u: {a} {b} => {r}");

    vm_try!(ctx.stack.push_u32(r));

    call_next(tail_code, 0, ctx)
}

pub unsafe fn op_i32_lt_s(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_i32();
    let a = ctx.stack.pop_i32();
    let r = if a < b { 1 } else { 0 };
    trace!("op_i32_lt_s: {a} {b} => {r}");

    vm_try!(ctx.stack.push_u32(r));

    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i32_lt_u(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_u32();
    let a = ctx.stack.pop_u32();
    let r = if a < b { 1 } else { 0 };
    trace!("op_i32_lt_u: {a} {b} => {r}");

    vm_try!(ctx.stack.push_u32(r));

    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i32_gt_s(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_i32();
    let a = ctx.stack.pop_i32();
    let r = if a > b { 1 } else { 0 };
    trace!("op_i32_gt_s: {a} {b} => {r}");

    vm_try!(ctx.stack.push_u32(r));

    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i32_gt_u(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_u32();
    let a = ctx.stack.pop_u32();
    let r = if a > b { 1 } else { 0 };
    trace!("op_i32_gt_u: {a} {b} => {r}");

    vm_try!(ctx.stack.push_u32(r));

    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i32_ge_s(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_i32();
    let a = ctx.stack.pop_i32();
    let r = if a >= b { 1 } else { 0 };
    trace!("op_i32_ge_s: {a} {b} => {r}");

    vm_try!(ctx.stack.push_u32(r));

    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i32_ge_u(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_u32();
    let a = ctx.stack.pop_u32();
    let r = if a >= b { 1 } else { 0 };
    trace!("op_i32_ge_u: {a} {b} => {r}");

    vm_try!(ctx.stack.push_u32(r));

    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_mem_size(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    if let Some(addr) = ctx.instance().memory {
        let memory = &mut ctx.store.memory[addr as usize];
        vm_try!(ctx.stack.push_u32(memory.page_size()));
        call_next(tail_code, 0, ctx)
    } else {
        VMResult::MemoryIndexOutOfRange
    }
}
pub unsafe fn op_mem_grow(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let page_size_delta = ctx.stack.pop_u32();
    let memory = memory_try!(ctx);
    let res = vm_try!(memory.grow(page_size_delta));
    vm_try!(ctx.stack.push_i32(res));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_mem_init(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let idx = (*tail_code).operand.u32;
    let n = ctx.stack.pop_u32();
    let s = ctx.stack.pop_u32();
    let d = ctx.stack.pop_u32();
    let module_addr = ctx.instance().module_addr;

    let m = &ctx.store.modules[module_addr as usize];
    let data = vm_try!(VMResult::from_option(m.data.get(idx as usize), || {
        VMResult::MemoryIndexOutOfRange
    }));
    let last = vm_try!(VMResult::from_option(s.checked_add(n), || {
        VMResult::MemoryIndexOutOfRange
    }));
    let data = vm_try!(VMResult::from_option(
        data.init.get(s as usize..last as usize),
        || { VMResult::MemoryIndexOutOfRange }
    ));
    let memory = if let Some(v) = ctx.instance().memory {
        &mut ctx.store.memory[v as usize]
    } else {
        return VMResult::MemoryIndexOutOfRange;
    };

    vm_try!(memory.init(d, data));
    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_mem_copy(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let len = ctx.stack.pop_u32();
    let src = ctx.stack.pop_u32();
    let dst = ctx.stack.pop_u32();
    let memory = memory_try!(ctx);

    vm_try!(memory.copy(dst, src, len));
    call_next(tail_code, 0, ctx)
}

pub unsafe fn op_mem_fill(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let len = ctx.stack.pop_u32();
    let data = ctx.stack.pop_u32();
    let ptr = ctx.stack.pop_u32();
    let memory = memory_try!(ctx);

    vm_try!(memory.fill(ptr, len, data));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_unreachable(_tail_code: *const Instr, _ctx: &mut ExecuteContext) -> VMResult<()> {
    VMResult::Unreachable
}
pub unsafe fn op_i32_extend8_s(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let v = ctx.stack.pop_u32();
    let v = i8::from_le_bytes([v as u8]);
    vm_try!(ctx.stack.push_i32(v.into()));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i32_extend16_s(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let v = ctx.stack.pop_u32();
    let v = i16::from_le_bytes([v as u8, (v >> 8) as u8]);
    vm_try!(ctx.stack.push_i32(v.into()));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i64_extend8_s(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let v = ctx.stack.pop_u64();
    let v = i8::from_le_bytes([v as u8]);
    vm_try!(ctx.stack.push_i64(v.into()));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i64_extend16_s(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let v = ctx.stack.pop_u64();
    let v = i16::from_le_bytes([v as u8, (v >> 8) as u8]);
    vm_try!(ctx.stack.push_i64(v.into()));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i64_extend32_s(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let v = ctx.stack.pop_u64();
    let v = i32::from_le_bytes([v as u8, (v >> 8) as u8, (v >> 16) as u8, (v >> 24) as u8]);
    vm_try!(ctx.stack.push_i64(v.into()));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_ref_null(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    vm_try!(ctx.stack.push_u32(0));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_ref_is_null(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    if ctx.stack.pop_u32() == 0 {
        vm_try!(ctx.stack.push_u32(1));
    } else {
        vm_try!(ctx.stack.push_u32(0));
    }
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_ref_func(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let funcidx = (*tail_code).operand.u32;
    vm_try!(ctx.stack.push_u32(ctx.instance().funcs[funcidx as usize]));
    call_next(tail_code, 1, ctx)
}
pub unsafe fn special_function_return(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    trace!("function return");
    let tail_code = ctx.stack.function_return(
        &ctx.local_reference(),
        (*tail_code).operand.drop_size as usize,
    );

    ctx.local_state.pop();
    call_next(tail_code, 0, ctx)
}
pub unsafe fn special_block_return(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let block_return = &(*tail_code).operand.block_return;
    trace!(
        "block return: {:?} {:?} {:?}",
        ctx.local_reference(),
        block_return,
        ctx.stack
    );
    ctx.stack.block_return(
        &ctx.local_reference(),
        block_return.stack_top as usize,
        block_return.return_size as usize,
    );
    trace!("stack: {:?}", ctx.stack);

    call_next(tail_code, 1, ctx)
}
pub unsafe fn special_function_vm_end(
    _tail_code: *const Instr,
    _ctx: &mut ExecuteContext,
) -> VMResult<()> {
    VMResult::Success(())
}

pub(crate) const VM_END: Instr = Instr {
    op: special_function_vm_end,
};
pub fn run_module_function(
    instance: InstanceAddr,
    store: &mut Store,
    name: &str,
    args: &ResultValue,
) -> VMResult<ResultValue> {
    let Instance {
        module_addr,
        memory: _,
        tables: _,
        globals: _,
        funcs,
    } = &store.instances[instance.0 as usize];
    let module_inst = &store.modules[*module_addr as usize];
    if let Some(ExportDesc::Func(idx)) = module_inst.exports.find(name) {
        let code_addr = funcs[idx.0 as usize];
        let funcinst = &store.funcs.0[code_addr as usize];
        let mut stack = Stack::new(128 * 1024);
        let tidx = module_inst.functions.get(idx.0 as usize).unwrap();
        let ft = module_inst
            .function_types
            .get(tidx.0 as usize)
            .unwrap()
            .clone();

        let mut param_size = 0usize;
        for t in ft.0.iter() {
            param_size += t.stack_size().usize();
        }
        let mut local_size = 0usize;
        let code = &funcinst.body;
        for local in &code.locals {
            local_size += local.n as usize * local.t.stack_size().usize();
        }
        for arg in args.iter() {
            vm_try!(match arg {
                WasmValue::I32(i32) => stack.push_i32(*i32),
                WasmValue::I64(i64) => stack.push_i64(*i64),
                WasmValue::F32(v) => stack.push_f32(*v),
                WasmValue::F64(v) => stack.push_f64(*v),
                WasmValue::ExternRef(v) => stack.push_u32(*v),
                WasmValue::FuncRef(v) => stack.push_u32(*v),
            });
        }

        tracing::trace!("run_module_function: {name} {local_size} {:?}", code.locals,);
        let local_reference =
            vm_try!(stack.function_call(param_size, local_size, &VM_END as *const Instr));
        let mut jump_table = JumpTable::new();
        jump_table.push((code.expr.len() - 2) as u32);

        let ptr = code.expr.as_ptr();
        let mut ctx = ExecuteContext {
            stack: &mut stack,
            local_state: vec![LocalState {
                jump_table,
                local_reference,
                code_addr,
                instance_addr: funcinst.instance_addr,
            }],
            store,
        };
        vm_try!(unsafe { call_next(ptr, 0, &mut ctx) });

        let mut result =
            ft.1.stack_pop_iter()
                .map(|t| match t {
                    ValType::I32 => WasmValue::I32(stack.pop_i32()),
                    ValType::I64 => WasmValue::I64(stack.pop_i64()),
                    ValType::F32 => WasmValue::F32(stack.pop_f32()),
                    ValType::F64 => WasmValue::F64(stack.pop_f64()),
                    ValType::FuncRef => WasmValue::FuncRef(stack.pop_u32()),
                    ValType::ExternRef => WasmValue::ExternRef(stack.pop_u32()),
                    ValType::V128 => todo!(),
                })
                .collect::<Vec<_>>();
        result.reverse();
        VMResult::Success(ResultValue(result))
    } else {
        unimplemented!()
    }
}
pub fn get_global(instance: InstanceAddr, store: &mut Store, name: &str) -> VMResult<WasmValue> {
    let instance = &store.instances[instance.0 as usize];
    let module_inst = &store.modules[instance.module_addr as usize];
    if let Some(ExportDesc::Global(idx)) = module_inst.exports.find(name) {
        let addr = instance.globals[idx.0 as usize] as usize;
        let gt = module_inst.globals[idx.0 as usize];
        VMResult::Success(match gt.0 {
            ValType::I32 => {
                let mut buf = [0u8; 4];
                buf.copy_from_slice(&store.globals.0[addr..addr + 4]);
                WasmValue::I32(i32::from_le_bytes(buf))
            }
            ValType::I64 => {
                let mut buf = [0u8; 8];
                buf.copy_from_slice(&store.globals.0[addr..addr + 8]);
                WasmValue::I64(i64::from_le_bytes(buf))
            }
            _ => todo!(),
        })
    } else {
        unimplemented!()
    }
}
