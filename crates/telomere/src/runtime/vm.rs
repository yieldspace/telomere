#[macro_use]
pub(crate) mod traps;
mod bulk_memory;
mod call;
mod control;
mod locals;
mod memory;
#[cfg(feature = "simd")]
pub(crate) mod simd;
use std::ops::BitXor;

use crate::{
    common::{
        execute_elem_init_const_expr, CallFrameCache, ElemInit, ExecuteContext, ExportDesc, GcRef,
        InstanceHandle, Instr, LocalReference, MemArg, ResultType, ResultValue, StablePc, Stack,
        VMResult, ValType, WasmValue, TABLE_UNINITIALIZED,
    },
    runtime::scheduler::{ReadyFlag, Scheduler, SyncRunError, Task},
    Store,
};

use super::memory_effect::{AsyncCompletion, AsyncResult};
#[inline(always)]
fn wait_effect(ctx: &mut ExecuteContext, cont: *const Instr) -> bool {
    if ctx.effect.get_pending_count() != 0 {
        trace!("waiting effect: {:?}", cont);
        ctx.cont = cont;
        true
    } else {
        false
    }
}

#[inline(always)]
fn wasm_shift_mask32(rhs: u32) -> u32 {
    rhs & 31
}

#[inline(always)]
fn wasm_shift_mask64(rhs: u32) -> u32 {
    rhs & 63
}

#[inline(always)]
fn wasm_i32_shl(lhs: i32, rhs: i32) -> i32 {
    lhs.wrapping_shl(wasm_shift_mask32(rhs as u32))
}

#[inline(always)]
fn wasm_i32_shr_s(lhs: i32, rhs: i32) -> i32 {
    lhs >> wasm_shift_mask32(rhs as u32)
}

#[inline(always)]
fn wasm_i32_shr_u(lhs: u32, rhs: u32) -> u32 {
    lhs.wrapping_shr(wasm_shift_mask32(rhs))
}

#[inline(always)]
fn wasm_i64_shl(lhs: i64, rhs: i64) -> i64 {
    lhs.wrapping_shl(wasm_shift_mask64(rhs as u32))
}

#[inline(always)]
fn wasm_i64_shr_s(lhs: i64, rhs: i64) -> i64 {
    lhs >> wasm_shift_mask64(rhs as u32)
}

#[inline(always)]
fn wasm_i64_shr_u(lhs: u64, rhs: u64) -> u64 {
    lhs.wrapping_shr(wasm_shift_mask64(rhs as u32))
}

pub(crate) enum StoreBytes {
    Write1([u8; 1]),
    Write2([u8; 2]),
    Write4([u8; 4]),
    Write8([u8; 8]),
    Write16([u8; 16]),
}

impl StoreBytes {
    #[inline(always)]
    fn as_slice(&self) -> &[u8] {
        match self {
            Self::Write1(bytes) => bytes,
            Self::Write2(bytes) => bytes,
            Self::Write4(bytes) => bytes,
            Self::Write8(bytes) => bytes,
            Self::Write16(bytes) => bytes,
        }
    }
}

#[inline(always)]
pub(crate) fn compute_memory_offset(memarg: MemArg, offset: u32) -> VMResult<usize> {
    VMResult::from_option(
        memarg
            .offset
            .checked_add(offset)
            .map(|value| value as usize),
        || VMResult::MemoryIndexOutOfRange,
    )
}

enum CallOutcome {
    Immediate(*const Instr),
    Pending,
}

#[inline(always)]
pub(crate) unsafe fn call_code(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    ctx.cont = tail_code;
    ((*tail_code).op)(tail_code.offset(1), ctx)
}
#[inline(always)]
pub(crate) unsafe fn call_next(
    tail_code: *const Instr,
    consumed: isize,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    call_code(tail_code.offset(consumed), ctx)
}

fn result_type_size(ty: &ResultType) -> usize {
    ty.iter().map(|value| value.stack_size().usize()).sum()
}

fn push_typed_value(stack: &mut Stack, ty: ValType, value: &WasmValue) -> VMResult<()> {
    match (ty, value) {
        (ValType::I32, WasmValue::I32(value)) => stack.push_i32(*value),
        (ValType::I64, WasmValue::I64(value)) => stack.push_i64(*value),
        (ValType::F32, WasmValue::F32(value)) => stack.push_f32(*value),
        (ValType::F64, WasmValue::F64(value)) => stack.push_f64(*value),
        (ValType::V128, WasmValue::V128(value)) => stack.push_u128(*value),
        (ValType::FuncRef, WasmValue::FuncRef(value)) => stack.push_u32(*value),
        (ValType::ExternRef, WasmValue::ExternRef(value)) => stack.push_u32(*value),
        _ => VMResult::InvalidOperand,
    }
}

fn push_result_values(stack: &mut Stack, types: &ResultType, values: &ResultValue) -> VMResult<()> {
    if types.0.len() != values.len() {
        return VMResult::InvalidOperand;
    }
    for (ty, value) in types.iter().zip(values.iter()) {
        vm_try!(push_typed_value(stack, *ty, value));
    }
    VMResult::Success(())
}

fn pop_result_values(stack: &mut Stack, ty: &ResultType) -> ResultValue {
    let mut result = ty
        .stack_pop_iter()
        .map(|t| match t {
            ValType::I32 => WasmValue::I32(stack.pop_i32()),
            ValType::I64 => WasmValue::I64(stack.pop_i64()),
            ValType::F32 => WasmValue::F32(stack.pop_f32()),
            ValType::F64 => WasmValue::F64(stack.pop_f64()),
            ValType::FuncRef => WasmValue::FuncRef(stack.pop_u32()),
            ValType::ExternRef => WasmValue::ExternRef(stack.pop_u32()),
            ValType::V128 => WasmValue::V128(stack.pop_u128()),
        })
        .collect::<Vec<_>>();
    result.reverse();
    ResultValue::new(result)
}

fn start_async_host_call(
    return_addr: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<CallOutcome> {
    let async_host = ctx.func().async_host_code_pointer();
    let task_id = ctx.task_id;
    let future = async_host(ctx);
    ctx.effect.push_async_effect(Box::pin(async move {
        AsyncResult {
            task_id,
            completion: AsyncCompletion::HostCall {
                result: future.await,
            },
        }
    }));
    ctx.cont = return_addr;
    VMResult::Success(CallOutcome::Pending)
}

fn invoke_host_function(
    return_addr: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<CallOutcome> {
    if ctx.func().is_async_host_func() {
        start_async_host_call(return_addr, ctx)
    } else {
        let fp = ctx.func().host_code_pointer();
        let return_addr = vm_try!(fp(ctx));
        VMResult::Success(CallOutcome::Immediate(return_addr))
    }
}

pub(crate) use bulk_memory::{op_data_drop, op_mem_copy, op_mem_fill, op_mem_init};
pub(crate) use call::{
    op_call, op_call_indirect, op_return_call, op_return_call_indirect, special_start_function_call,
};
pub use control::special_function_return;
pub(crate) use control::{
    op_br, op_br_if, op_br_table, op_else, op_end, op_if, op_loop, op_return, special_block_return,
    special_function_vm_end,
};
pub(crate) use locals::{
    op_drop, op_local_get16, op_local_get4, op_local_get8, op_local_set16, op_local_set4,
    op_local_set8, op_local_tee16, op_local_tee4, op_local_tee8, op_select,
};
pub(crate) use memory::{
    op_f32_load, op_f32_store, op_f64_load, op_f64_store, op_i32_load, op_i32_load16_s,
    op_i32_load16_u, op_i32_load8_s, op_i32_load8_u, op_i32_store, op_i32_store16, op_i32_store8,
    op_i64_load, op_i64_load16_s, op_i64_load16_u, op_i64_load32_s, op_i64_load32_u,
    op_i64_load8_s, op_i64_load8_u, op_i64_store, op_i64_store16, op_i64_store32, op_i64_store8,
    op_mem_grow, op_mem_size,
};

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
pub unsafe fn op_global_get4(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let idx = (*tail_code).operand.u32 as usize;
    let addr = ctx.instance().globals.as_slice()[idx];
    vm_try!(ctx.stack.push_slice(ctx.gc.get_global(addr)));
    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_global_get8(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let idx = (*tail_code).operand.u32 as usize;
    let addr = ctx.instance().globals.as_slice()[idx];
    vm_try!(ctx.stack.push_slice(ctx.gc.get_global(addr)));
    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_global_get16(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let idx = (*tail_code).operand.u32 as usize;
    let addr = ctx.instance().globals.as_slice()[idx];
    vm_try!(ctx.stack.push_slice(ctx.gc.get_global(addr)));
    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_global_set4(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let idx = (*tail_code).operand.u32 as usize;
    let addr = ctx.instance().globals.as_slice()[idx];
    ctx.gc
        .get_global_mut(addr)
        .copy_from_slice(&ctx.stack.pop_u8_array::<4>());
    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_global_set8(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let idx = (*tail_code).operand.u32 as usize;
    let addr = ctx.instance().globals.as_slice()[idx];
    ctx.gc
        .get_global_mut(addr)
        .copy_from_slice(&ctx.stack.pop_u8_array::<8>());
    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_global_set16(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let idx = (*tail_code).operand.u32 as usize;
    let addr = ctx.instance().globals.as_slice()[idx];
    ctx.gc
        .get_global_mut(addr)
        .copy_from_slice(&ctx.stack.pop_u8_array::<16>());
    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_table_get(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let idx = (*tail_code).operand.u32 as usize;
    let addr = ctx.instance().tables.as_slice()[idx];
    let inst = ctx.gc.get_table(addr);
    let i = ctx.stack.pop_u32();
    if i as usize >= inst.1.len() {
        return VMResult::TableIndexOutOfRange;
    }
    let val = inst.1[i as usize];
    trace!("op_table_get: {idx} {addr:?} {i} {val}");

    vm_try!(ctx.stack.push_u32(val));
    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_table_set(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let idx = (*tail_code).operand.u32 as usize;
    let addr = ctx.instance().tables.as_slice()[idx];
    let inst = &mut ctx.gc.get_table(addr);
    let val = ctx.stack.pop_u32();
    let i = ctx.stack.pop_u32();
    trace!("op_table_set: {idx} {addr:?} {i} {val}");

    if i as usize >= inst.1.len() {
        return VMResult::TableIndexOutOfRange;
    }
    inst.1[i as usize] = val;
    call_next(tail_code, 1, ctx)
}

#[inline(never)]
unsafe fn table_init_impl(
    ctx: &mut ExecuteContext,
    src_elem_idx: u32,
    dst_table_idx: usize,
    dst_pos: usize,
    src: usize,
    len: usize,
) -> VMResult<()> {
    let instance_addr = ctx.instance_addr();
    let ExecuteContext { store, gc, .. } = ctx;
    let instance = unsafe { &*gc.get_instance_unchecked(instance_addr) };
    let dst_table_addr = instance.tables.as_slice()[dst_table_idx];
    let segments = store.lock_segments();
    let dst_table_len = {
        let dst_table = gc.get_table(dst_table_addr);
        dst_table.1.len()
    };
    if dst_pos.checked_add(len).is_none() || dst_pos + len > dst_table_len {
        return VMResult::TableIndexOutOfRange;
    }
    let Some(elem) = segments.elems.get(&(instance.instance_id, src_elem_idx)) else {
        return if len == 0 && src == 0 {
            VMResult::Success(())
        } else {
            VMResult::TableIndexOutOfRange
        };
    };
    let reftype = {
        let dst_table = gc.get_table(dst_table_addr);
        dst_table.0.reftype
    };
    match &elem.init {
        ElemInit::FuncIdx(idxs) => {
            let slice = vm_try!(VMResult::from_option(idxs.get(src..(src + len)), || {
                VMResult::TableIndexOutOfRange
            }));
            let func_addrs = instance
                .funcs
                .as_slice()
                .iter()
                .map(|it| it.get())
                .collect::<Vec<_>>();
            let dst_table = gc.get_table(dst_table_addr);
            let dst = vm_try!(VMResult::from_option(
                dst_table.1.get_mut(dst_pos..dst_pos + len),
                || { VMResult::TableIndexOutOfRange }
            ));
            for (i, funcidx) in slice.iter().enumerate() {
                dst[i] = func_addrs[*funcidx as usize];
            }
        }
        ElemInit::ConstExpr(exprs) => {
            let slice = vm_try!(VMResult::from_option(exprs.get(src..(src + len)), || {
                VMResult::TableIndexOutOfRange
            }));
            for (i, expr) in slice.iter().enumerate() {
                let res = vm_try!(execute_elem_init_const_expr(
                    gc,
                    instance.globals.as_slice(),
                    instance.funcs.as_slice(),
                    expr,
                    reftype,
                ));
                let dst_table = gc.get_table(dst_table_addr);
                let dst = vm_try!(VMResult::from_option(
                    dst_table.1.get_mut(dst_pos..dst_pos + len),
                    || { VMResult::TableIndexOutOfRange }
                ));
                dst[i] = res.get();
            }
        }
    }
    VMResult::Success(())
}

pub unsafe fn op_table_init(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let len = ctx.stack.pop_u32() as usize;
    let src = ctx.stack.pop_u32() as usize;
    let dst_pos = ctx.stack.pop_u32() as usize;
    let src_elem_idx = (*tail_code).operand.u32;
    let dst_table_idx = (*tail_code.offset(1)).operand.u32 as usize;
    vm_try!(table_init_impl(
        ctx,
        src_elem_idx,
        dst_table_idx,
        dst_pos,
        src,
        len,
    ));
    call_next(tail_code, 2, ctx)
}

#[inline(never)]
fn elem_drop_impl(ctx: &mut ExecuteContext, instance_id: u32, elem_idx: u32) {
    let _ = ctx
        .store
        .lock_segments()
        .elems
        .remove(&(instance_id, elem_idx));
}

pub unsafe fn op_elem_drop(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let elem_idx = (*tail_code).operand.u32;
    let instance_id = ctx.instance_id();
    elem_drop_impl(ctx, instance_id, elem_idx);
    call_next(tail_code, 1, ctx)
}

pub unsafe fn op_table_copy(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let len = ctx.stack.pop_u32() as usize;
    let src = ctx.stack.pop_u32() as usize;
    let dst = ctx.stack.pop_u32() as usize;
    let dst_table_idx = (*tail_code).operand.u32 as usize;
    let src_table_idx = (*tail_code.offset(1)).operand.u32 as usize;

    let src_table_addr = ctx.instance().tables.as_slice()[src_table_idx];
    let dst_table_addr = ctx.instance().tables.as_slice()[dst_table_idx];
    let src_table = &ctx.gc.get_table(src_table_addr).1;
    let src_ptr = vm_try!(VMResult::from_option(src_table.get(src..src + len), || {
        VMResult::TableIndexOutOfRange
    }))
    .as_ptr();
    let dst_table = &mut ctx.gc.get_table(dst_table_addr).1;
    let dst_ptr = vm_try!(VMResult::from_option(
        dst_table.get_mut(dst..dst + len),
        || { VMResult::TableIndexOutOfRange }
    ))
    .as_mut_ptr();
    std::ptr::copy(src_ptr, dst_ptr, len);
    call_next(tail_code, 2, ctx)
}
pub unsafe fn op_table_grow(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let table_idx = (*tail_code).operand.u32 as usize;
    let table_addr = ctx.instance().tables.as_slice()[table_idx];
    let table_inst = &mut ctx.gc.get_table(table_addr);
    let n = ctx.stack.pop_i32();
    let val = ctx.stack.pop_u32();
    let sz = table_inst.1.len();
    if n < 0 {
        vm_try!(ctx.stack.push_i32(-1));
    } else {
        let new_len = sz + n as usize;
        match table_inst.0.limits.max {
            Some(max) if max as usize >= new_len => {
                table_inst.1.resize(new_len, val);
                vm_try!(ctx.stack.push_u32(sz as u32));
            }
            None => {
                table_inst.1.resize(new_len, val);
                vm_try!(ctx.stack.push_u32(sz as u32));
            }
            Some(_) => {
                vm_try!(ctx.stack.push_i32(-1));
            }
        }
    }
    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_table_size(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let table_idx = (*tail_code).operand.u32 as usize;
    let table_addr = ctx.instance().tables.as_slice()[table_idx];
    let table_inst = &mut ctx.gc.get_table(table_addr);
    let val = table_inst.1.len() as u32;
    trace!("op_table_size: {table_idx} {table_addr:?} {table_inst:?} => {val}");
    vm_try!(ctx.stack.push_u32(val));
    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_table_fill(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let n = ctx.stack.pop_u32() as usize;
    let val = ctx.stack.pop_u32();
    let i = ctx.stack.pop_u32() as usize;
    let table_idx = (*tail_code).operand.u32 as usize;

    let table_addr = ctx.instance().tables.as_slice()[table_idx];
    let table = &mut ctx.gc.get_table(table_addr).1;
    let slice = vm_try!(VMResult::from_option(table.get_mut(i..i + n), || {
        VMResult::TableIndexOutOfRange
    }));
    slice.fill(val);
    call_next(tail_code, 1, ctx)
}

pub(crate) unsafe fn store_internal(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    make_operation: impl FnOnce(&mut ExecuteContext) -> StoreBytes,
) -> VMResult<()> {
    let memarg = (*tail_code).operand.memarg;
    let operation = make_operation(ctx);
    let offset = ctx.stack.pop_u32();
    trace!("op_store: {:?} {}", memarg, offset);
    let bytes = operation.as_slice();
    let start = vm_try!(compute_memory_offset(memarg, offset));
    vm_try!(ctx.write_memory_bytes(start, bytes));
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

    vm_try!(ctx.stack.push_i32(wasm_i32_shl(a, b)));

    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i32_shr_s(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_i32();
    let a = ctx.stack.pop_i32();

    vm_try!(ctx.stack.push_i32(wasm_i32_shr_s(a, b)));

    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i32_shr_u(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_u32();
    let a = ctx.stack.pop_u32();

    vm_try!(ctx.stack.push_u32(wasm_i32_shr_u(a, b)));

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

    vm_try!(ctx.stack.push_i64(wasm_i64_shl(a, b)));

    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i64_shr_s(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_i64();
    let a = ctx.stack.pop_i64();

    vm_try!(ctx.stack.push_i64(wasm_i64_shr_s(a, b)));

    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i64_shr_u(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_u64();
    let a = ctx.stack.pop_u64();

    vm_try!(ctx.stack.push_u64(wasm_i64_shr_u(a, b)));

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
    vm_try!(ctx
        .stack
        .push_u32(ctx.instance().funcs.as_slice()[funcidx as usize].get()));
    call_next(tail_code, 1, ctx)
}
pub(crate) const VM_END: Instr = Instr {
    op: special_function_vm_end,
};
pub(crate) const START_HOST_FUNCTION_PROGRAM: [Instr; 1] = [Instr {
    op: special_start_function_call,
}];
pub async fn run_module_function(
    instance: &InstanceHandle,
    store: &Store,
    name: &str,
    args: &ResultValue,
) -> VMResult<ResultValue> {
    if store.has_active_gc_on_current_thread() {
        tracing::error!(
            "run_module_function is unsupported while the same store GC is already active"
        );
        return VMResult::Unlinkable;
    }
    let mut scheduler: Scheduler<'_> = Scheduler::new(store);

    let ft = {
        let gc = store.lock_gc();
        let instance = gc.get_instance(vm_try!(VMResult::from_option(
            instance.get_gc_ref_with_pool(store, &gc),
            || { VMResult::Unlinkable }
        )));
        let module_inst = gc.get_module(instance.module_addr);
        trace!("{:?}", module_inst.exports);
        let ft = if let Some(ExportDesc::Func(idx)) = module_inst.exports.find(name) {
            let code_addr = instance.funcs[idx.0 as usize];
            let funcinst = gc.get_func(code_addr);
            let func_instance = gc.instance(funcinst.instance);
            let frame = CallFrameCache::from_parts(code_addr, funcinst, &func_instance.mems);
            let mut stack = Stack::new(128 * 1024);
            let tidx = module_inst.functions.get(idx.0 as usize).unwrap();
            let ft = module_inst
                .function_types
                .get(tidx.0 as usize)
                .unwrap()
                .clone();
            let param_size = result_type_size(&ft.0);

            let locals_data = funcinst.locals();
            let local_size = locals_data.byte_size();
            vm_try!(push_result_values(&mut stack, &ft.0, args));

            tracing::trace!("run_module_function: {name} {local_size}");
            let local_reference = vm_try!(stack.function_call(
                param_size,
                local_size,
                frame,
                LocalReference {
                    local_size: 0,
                    local_top: 0
                },
                &VM_END as *const Instr,
                &gc,
            ));

            scheduler.push(Task {
                fp: StablePc::from_relative_index(0),
                task_id: 0,
                stack,
                local_reference,
                ready_flag: ReadyFlag::Ready,
                pending_effects: 0,
                terminal_result: None,
            });
            ft
        } else {
            unimplemented!()
        };
        ft
    };
    scheduler.run().await;
    let ct = scheduler.completed_tasks.pop().unwrap();
    vm_try!(ct.result);
    let mut stack = ct.stack;
    VMResult::Success(pop_result_values(&mut stack, &ft.1))
}

pub(crate) fn run_module_function_sync_with_gc(
    instance: &InstanceHandle,
    store: &Store,
    gc: &mut crate::common::StoreInner,
    name: &str,
    args: &ResultValue,
) -> Result<VMResult<ResultValue>, SyncRunError> {
    let mut scheduler: Scheduler<'_> = Scheduler::new(store);

    let ft = {
        let instance = gc.get_instance(match instance.get_gc_ref_with_pool(store, gc) {
            Some(gc_ref) => gc_ref,
            None => return Ok(VMResult::Unlinkable),
        });
        let module_inst = gc.get_module(instance.module_addr);
        trace!("{:?}", module_inst.exports);
        let ft = if let Some(ExportDesc::Func(idx)) = module_inst.exports.find(name) {
            let code_addr = instance.funcs[idx.0 as usize];
            let funcinst = gc.get_func(code_addr);
            let func_instance = gc.instance(funcinst.instance);
            let frame = CallFrameCache::from_parts(code_addr, funcinst, &func_instance.mems);
            let mut stack = Stack::new(128 * 1024);
            let tidx = module_inst.functions.get(idx.0 as usize).unwrap();
            let ft = module_inst
                .function_types
                .get(tidx.0 as usize)
                .unwrap()
                .clone();
            let param_size = result_type_size(&ft.0);

            let locals_data = funcinst.locals();
            let local_size = locals_data.byte_size();
            let push_result = push_result_values(&mut stack, &ft.0, args);
            if !matches!(push_result, VMResult::Success(())) {
                return Ok(vm_result_err_into_result_value(push_result));
            }

            tracing::trace!("run_module_function: {name} {local_size}");
            let local_reference = match stack.function_call(
                param_size,
                local_size,
                frame,
                LocalReference {
                    local_size: 0,
                    local_top: 0,
                },
                &VM_END as *const Instr,
                gc,
            ) {
                VMResult::Success(local_reference) => local_reference,
                other => return Ok(vm_result_err_into_result_value(other)),
            };

            scheduler.push(Task {
                fp: StablePc::from_relative_index(0),
                task_id: 0,
                stack,
                local_reference,
                ready_flag: ReadyFlag::Ready,
                pending_effects: 0,
                terminal_result: None,
            });
            ft
        } else {
            unimplemented!()
        };
        ft
    };

    scheduler.run_sync_with_gc(gc)?;
    let ct = scheduler.completed_tasks.pop().unwrap();
    match ct.result {
        VMResult::Success(()) => {
            let mut stack = ct.stack;
            Ok(VMResult::Success(pop_result_values(&mut stack, &ft.1)))
        }
        VMResult::Unreachable => Ok(VMResult::Unreachable),
        VMResult::StackOverflow => Ok(VMResult::StackOverflow),
        VMResult::MemoryIndexOutOfRange => Ok(VMResult::MemoryIndexOutOfRange),
        VMResult::TableIndexOutOfRange => Ok(VMResult::TableIndexOutOfRange),
        VMResult::CallIndirectInvalidType => Ok(VMResult::CallIndirectInvalidType),
        VMResult::TableUninitialized => Ok(VMResult::TableUninitialized),
        VMResult::Unlinkable => Ok(VMResult::Unlinkable),
        VMResult::InvalidOperand => Ok(VMResult::InvalidOperand),
    }
}

fn vm_result_err_into_result_value<T>(result: VMResult<T>) -> VMResult<ResultValue> {
    match result {
        VMResult::Success(_) => unreachable!(),
        VMResult::Unreachable => VMResult::Unreachable,
        VMResult::StackOverflow => VMResult::StackOverflow,
        VMResult::MemoryIndexOutOfRange => VMResult::MemoryIndexOutOfRange,
        VMResult::TableIndexOutOfRange => VMResult::TableIndexOutOfRange,
        VMResult::CallIndirectInvalidType => VMResult::CallIndirectInvalidType,
        VMResult::TableUninitialized => VMResult::TableUninitialized,
        VMResult::Unlinkable => VMResult::Unlinkable,
        VMResult::InvalidOperand => VMResult::InvalidOperand,
    }
}

pub fn get_global(instance: &InstanceHandle, store: &Store, name: &str) -> VMResult<WasmValue> {
    if store.has_active_gc_on_current_thread() {
        tracing::error!("get_global is unsupported while the same store GC is already active");
        return VMResult::Unlinkable;
    }
    let gc = store.lock_gc();

    let instance = unsafe {
        &*gc.get_instance_unchecked(vm_try!(VMResult::from_option(
            instance.get_gc_ref_with_pool(store, &gc),
            || { VMResult::Unlinkable }
        )))
    };
    let module_inst = gc.get_module(instance.module_addr);
    if let Some(ExportDesc::Global(idx)) = module_inst.exports.find(name) {
        let addr = instance.globals.as_slice()[idx.0 as usize];
        let gt = module_inst.globals[idx.0 as usize];
        VMResult::Success(match gt.0 {
            ValType::I32 => {
                let mut buf = [0u8; 4];
                buf.copy_from_slice(gc.get_global(addr));
                WasmValue::I32(i32::from_le_bytes(buf))
            }
            ValType::I64 => {
                let mut buf = [0u8; 8];
                buf.copy_from_slice(gc.get_global(addr));
                WasmValue::I64(i64::from_le_bytes(buf))
            }
            _ => todo!(),
        })
    } else {
        unimplemented!()
    }
}
