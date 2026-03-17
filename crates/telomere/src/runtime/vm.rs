#[macro_use]
pub(crate) mod traps;
#[cfg(feature = "simd")]
pub(crate) mod simd;
use std::ops::BitXor;

use crate::{
    common::{
        execute_elem_init_const_expr,
        gc::{GcRef, InstanceData},
        ElemInit, ExecuteContext, ExportDesc, InstanceHandle, Instr, LocalReference, ResultType,
        ResultValue, StablePc, Stack, VMResult, ValType, WasmValue, TABLE_UNINITIALIZED,
    },
    runtime::scheduler::{ReadyFlag, Scheduler, SyncRunError, Task},
    Store,
};

use super::memory_effect::{AsyncCompletion, AsyncResult, ReadOperationHandler, WriteOperation};
macro_rules! wait_effect {
    ($ctx: expr, $cont: expr) => {
        if $ctx.effect.get_pending_count() != 0 {
            trace!("waiting effect: {:?}", $cont);
            $ctx.cont = $cont;
            return VMResult::Success(());
        }
    };
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
    let async_host = ctx.func().async_host_code_pointer(ctx.gc);
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
        let fp = ctx.func().host_code_pointer(ctx.gc);
        let return_addr = vm_try!(fp(ctx));
        VMResult::Success(CallOutcome::Immediate(return_addr))
    }
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
pub unsafe fn op_return(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let addr = (*tail_code).operand.jump_addr;
    trace!("op_return: {addr}");
    let code = ctx.code();
    let tail_code = code.offset(addr as isize);
    call_next(tail_code, 0, ctx)
}

pub unsafe fn op_end(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    trace!("op_end");
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_br(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let addr = (*tail_code).operand.jump_addr;
    trace!("op_br: {addr}");

    let tail_code = ctx.code().offset(addr as isize);
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_else(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    trace!("op_else");

    let addr = (*tail_code).operand.jump_addr;
    let tail_code = ctx.code().offset(addr as isize);
    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_br_if(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let cond = ctx.stack.pop_u32();
    trace!("op_br_if: {cond}");

    let ptr = if cond != 0 {
        let addr = (*tail_code).operand.jump_addr;

        ctx.code().offset(addr as isize)
    } else {
        tail_code.offset(1)
    };
    call_next(ptr, 0, ctx)
}
pub unsafe fn op_br_table(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let index = ctx.stack.pop_u32();
    let table_size = (*tail_code).operand.u32;

    let addr = if index < table_size {
        (*tail_code.offset((index + 1) as isize)).operand.jump_addr
    } else {
        (*tail_code.offset((table_size + 1) as isize))
            .operand
            .jump_addr
    };
    trace!(
        "op_br_table: index={} table_size={} => addr={}",
        index,
        table_size,
        addr
    );
    let tail_code = ctx.code().offset(addr as isize);
    call_next(tail_code, 0, ctx)
}

pub unsafe fn op_loop(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    trace!("op_loop: {}", (*tail_code).operand.jump_addr);

    let loop_param = (*tail_code).operand.loop_param;
    ctx.stack.block_return(
        &ctx.local_reference(),
        loop_param.stack_top as usize,
        loop_param.param_size as usize,
    );

    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_if(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let else_addr = (*tail_code).operand.jump_addr;
    let v = ctx.stack.pop_u32();
    trace!("op_if: {else_addr} {v}");

    let ptr = if v == 0 {
        ctx.code().offset(else_addr as isize)
    } else {
        tail_code.offset(1)
    };
    call_next(ptr, 0, ctx)
}
// Required for direct function call threading.
// If unset, LLVM will not replace the end of op_call with a jump.
#[inline(never)]
unsafe fn internal_op_call(
    return_addr: *const Instr,
    funcaddr: GcRef,
    ctx: &mut ExecuteContext,
    is_return_call: bool,
) -> VMResult<CallOutcome> {
    let funcinst = ctx.func_by_addr(funcaddr);
    let instance_addr = funcinst.instance_addr;
    let instance = &*ctx.gc.get_instance_unchecked(instance_addr);
    let module_addr = instance.module_addr;
    let module = ctx.gc.get_module(module_addr);
    let typeidx = module
        .functions
        .get(funcinst.funcidx as usize)
        .unwrap_unchecked();
    let ft = &module.function_types[typeidx.0 as usize];
    trace!("op_call_internal: {instance_addr:?}({module_addr:?})  {funcaddr:?}");
    let mut param_size = 0usize;
    for param in ft.0.iter() {
        param_size += param.stack_size().usize();
    }
    let is_host_func = funcinst.is_host_func();
    if funcinst.is_host_func() {
        if is_return_call {
            ctx.local_reference = vm_try!(ctx.stack.function_return_call(
                &ctx.local_reference,
                param_size,
                0,
                funcaddr
            ));
        } else {
            ctx.local_reference = vm_try!(ctx.stack.function_call(
                param_size,
                0,
                funcaddr,
                ctx.local_reference,
                return_addr,
                ctx.gc,
            ));
        }
        invoke_host_function(return_addr, ctx)
    } else {
        let (locals, code_offset) = funcinst.locals_and_code_offset(ctx.gc);
        let addr = funcinst.body;
        if is_return_call {
            ctx.local_reference = vm_try!(ctx.stack.function_return_call(
                &ctx.local_reference,
                param_size,
                locals.byte_size(),
                funcaddr
            ));
        } else {
            ctx.local_reference = vm_try!(ctx.stack.function_call(
                param_size,
                locals.byte_size(),
                funcaddr,
                ctx.local_reference,
                return_addr,
                ctx.gc,
            ));
        }

        let ptr = ctx.gc.get_value::<Instr>(addr, code_offset);
        debug_assert!(!is_host_func);
        VMResult::Success(CallOutcome::Immediate(ptr))
    }
}

pub unsafe fn op_call(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let funcidx = (*tail_code).operand.u32;
    let funcaddr = ctx.instance().funcs.as_slice(ctx.gc)[funcidx as usize];
    match vm_try!(internal_op_call(tail_code.offset(1), funcaddr, ctx, false)) {
        CallOutcome::Immediate(ptr) => call_next(ptr, 0, ctx),
        CallOutcome::Pending => VMResult::Success(()),
    }
}
pub unsafe fn op_return_call(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let funcidx = (*tail_code).operand.u32;
    let funcaddr = ctx.instance().funcs.as_slice(ctx.gc)[funcidx as usize];
    match vm_try!(internal_op_call(tail_code.offset(1), funcaddr, ctx, true)) {
        CallOutcome::Immediate(ptr) => call_next(ptr, 0, ctx),
        CallOutcome::Pending => VMResult::Success(()),
    }
}
#[inline(never)]
unsafe fn internal_op_call_indirect(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    is_return_call: bool,
) -> VMResult<CallOutcome> {
    let i = ctx.stack.pop_u32();
    let tableidx = (*tail_code).operand.u32 as usize;
    let table_addr = *vm_try!(VMResult::from_option(
        ctx.instance().tables.as_slice(ctx.gc).get(tableidx),
        || { VMResult::TableIndexOutOfRange }
    ));
    let table = ctx.gc.get_table(table_addr);
    let func_addr = *vm_try!(VMResult::from_option(table.1.get(i as usize), || {
        VMResult::TableIndexOutOfRange
    }));
    trace!("internal_op_call_indirect: {tableidx} {table_addr:?} {func_addr} {table:?}");
    if func_addr == TABLE_UNINITIALIZED {
        return VMResult::TableUninitialized;
    }
    let func_addr = GcRef(func_addr);
    let funcinst = ctx.gc.get_func(func_addr);
    let instance = &*ctx.gc.get_instance_unchecked(funcinst.instance_addr);
    let module = ctx.gc.get_module(instance.module_addr);
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
    let outcome = vm_try!(internal_op_call(
        tail_code.offset(2),
        func_addr,
        ctx,
        is_return_call
    ));
    VMResult::Success(outcome)
}
pub unsafe fn op_call_indirect(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    match vm_try!(internal_op_call_indirect(tail_code, ctx, false)) {
        CallOutcome::Immediate(ptr) => call_next(ptr, 0, ctx),
        CallOutcome::Pending => VMResult::Success(()),
    }
}
pub unsafe fn op_return_call_indirect(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    match vm_try!(internal_op_call_indirect(tail_code, ctx, true)) {
        CallOutcome::Immediate(ptr) => call_next(ptr, 0, ctx),
        CallOutcome::Pending => VMResult::Success(()),
    }
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
pub unsafe fn op_local_get16(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let addr = (*tail_code).operand.local_addr as usize;
    vm_try!(ctx.stack.local_get(&ctx.local_reference(), addr, 16));
    trace!("op_local_get16: {addr}");

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
pub unsafe fn op_local_set16(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let addr = (*tail_code).operand.local_addr as usize;
    ctx.stack.local_set(&ctx.local_reference(), addr, 16);

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
pub unsafe fn op_local_tee16(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let addr = (*tail_code).operand.local_addr as usize;
    ctx.stack.local_tee(&ctx.local_reference(), addr, 16);

    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_global_get4(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let idx = (*tail_code).operand.u32 as usize;
    let addr = ctx.instance().globals.as_slice(ctx.gc)[idx];
    vm_try!(ctx.stack.push_slice(ctx.gc.get_global(addr)));
    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_global_get8(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let idx = (*tail_code).operand.u32 as usize;
    let addr = ctx.instance().globals.as_slice(ctx.gc)[idx];
    vm_try!(ctx.stack.push_slice(ctx.gc.get_global(addr)));
    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_global_get16(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let idx = (*tail_code).operand.u32 as usize;
    let addr = ctx.instance().globals.as_slice(ctx.gc)[idx];
    vm_try!(ctx.stack.push_slice(ctx.gc.get_global(addr)));
    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_global_set4(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let idx = (*tail_code).operand.u32 as usize;
    let addr = ctx.instance().globals.as_slice(ctx.gc)[idx];
    ctx.gc
        .get_global_mut(addr)
        .copy_from_slice(&ctx.stack.pop_u8_array::<4>());
    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_global_set8(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let idx = (*tail_code).operand.u32 as usize;
    let addr = ctx.instance().globals.as_slice(ctx.gc)[idx];
    ctx.gc
        .get_global_mut(addr)
        .copy_from_slice(&ctx.stack.pop_u8_array::<8>());
    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_global_set16(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let idx = (*tail_code).operand.u32 as usize;
    let addr = ctx.instance().globals.as_slice(ctx.gc)[idx];
    ctx.gc
        .get_global_mut(addr)
        .copy_from_slice(&ctx.stack.pop_u8_array::<16>());
    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_table_get(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let idx = (*tail_code).operand.u32 as usize;
    let addr = ctx.instance().tables.as_slice(ctx.gc)[idx];
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
    let addr = ctx.instance().tables.as_slice(ctx.gc)[idx];
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
pub unsafe fn op_table_init(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let len = ctx.stack.pop_u32() as usize;
    let src = ctx.stack.pop_u32() as usize;
    let dst_pos = ctx.stack.pop_u32() as usize;
    let src_elem_idx = (*tail_code).operand.u32;
    let dst_table_idx = (*tail_code.offset(1)).operand.u32 as usize;
    let instance_addr = ctx.instance_addr();

    let ExecuteContext { store, gc, .. } = ctx;
    let instance = *gc.get_instance_unchecked(instance_addr);
    let dst_table_addr = instance.tables.as_slice(gc)[dst_table_idx];
    // `call_next` may recurse into `elem.drop`, so the segments guard must be dropped first.
    let init_result = {
        let segments = store.lock_segments();
        let dst_table_len = {
            let dst_table = gc.get_table(dst_table_addr);
            dst_table.1.len()
        };
        if dst_pos.checked_add(len).is_none() || dst_pos + len > dst_table_len {
            VMResult::TableIndexOutOfRange
        } else if let Some(elem) = segments.elems.get(&(instance.instance_id, src_elem_idx)) {
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
                        .as_slice(gc)
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
                            instance.globals.as_slice(gc),
                            instance.funcs.as_slice(gc),
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
        } else if len == 0 && src == 0 {
            VMResult::Success(())
        } else {
            VMResult::TableIndexOutOfRange
        }
    };
    vm_try!(init_result);
    call_next(tail_code, 2, ctx)
}
pub unsafe fn op_elem_drop(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let elem_idx = (*tail_code).operand.u32;
    let instance_id = ctx.instance_id();
    ctx.store
        .lock_segments()
        .elems
        .remove(&(instance_id, elem_idx));
    call_next(tail_code, 1, ctx)
}

pub unsafe fn op_table_copy(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let len = ctx.stack.pop_u32() as usize;
    let src = ctx.stack.pop_u32() as usize;
    let dst = ctx.stack.pop_u32() as usize;
    let dst_table_idx = (*tail_code).operand.u32 as usize;
    let src_table_idx = (*tail_code.offset(1)).operand.u32 as usize;

    let src_table_addr = ctx.instance().tables.as_slice(ctx.gc)[src_table_idx];
    let dst_table_addr = ctx.instance().tables.as_slice(ctx.gc)[dst_table_idx];
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
    let table_addr = ctx.instance().tables.as_slice(ctx.gc)[table_idx];
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
    let table_addr = ctx.instance().tables.as_slice(ctx.gc)[table_idx];
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

    let table_addr = ctx.instance().tables.as_slice(ctx.gc)[table_idx];
    let table = &mut ctx.gc.get_table(table_addr).1;
    let slice = vm_try!(VMResult::from_option(table.get_mut(i..i + n), || {
        VMResult::TableIndexOutOfRange
    }));
    slice.fill(val);
    call_next(tail_code, 1, ctx)
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
unsafe fn load_internal<const N: u32>(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    handler: ReadOperationHandler,
) -> VMResult<()> {
    let memarg = (*tail_code).operand.memarg;
    let offset = ctx.stack.pop_u32();
    let mem_addr = vm_try!(VMResult::from_option(ctx.memory_addr(), || {
        VMResult::MemoryIndexOutOfRange
    }));
    trace!("op_load_{N}: {:?} {}", memarg, offset);
    vm_try!(ctx.effect.push_non_atomic_memory_read_effect(
        ctx.task_id,
        mem_addr,
        memarg,
        offset,
        N,
        handler
    ));
    ctx.cont = tail_code.add(1);
    VMResult::Success(())
}
unsafe fn store_internal(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    make_operation: impl FnOnce(&mut ExecuteContext) -> WriteOperation,
) -> VMResult<()> {
    let memarg = (*tail_code).operand.memarg;
    let operation = make_operation(ctx);
    let offset = ctx.stack.pop_u32();
    let mem_addr = vm_try!(VMResult::from_option(ctx.memory_addr(), || {
        VMResult::MemoryIndexOutOfRange
    }));
    trace!("op_store: {:?} {}", memarg, offset);
    vm_try!(ctx.effect.push_non_atomic_memory_write_effect(
        ctx.task_id,
        mem_addr,
        memarg,
        offset,
        ctx.gc,
        operation
    ));
    call_next(tail_code, 1, ctx)
}
unsafe fn read_operation_handler_push_stack(
    stack: &mut Stack,
    data: &[u8],
    next: *const Instr,
) -> *const Instr {
    trap_func!(stack.push_slice(data));
    next
}
pub unsafe fn op_i32_load(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    load_internal::<4>(tail_code, ctx, read_operation_handler_push_stack)
}
pub unsafe fn op_i64_load(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    load_internal::<8>(tail_code, ctx, read_operation_handler_push_stack)
}
pub unsafe fn op_f32_load(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    load_internal::<4>(tail_code, ctx, read_operation_handler_push_stack)
}
pub unsafe fn op_f64_load(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    load_internal::<8>(tail_code, ctx, read_operation_handler_push_stack)
}
pub unsafe fn op_i32_load8_u(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    load_internal::<1>(tail_code, ctx, |stack, data, code| {
        let v = u8::from_le_bytes([data[0]]);
        trap_func!(stack.push_u32(v as u32));
        code
    })
}
pub unsafe fn op_i32_load8_s(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    load_internal::<1>(tail_code, ctx, |stack, data, code| {
        let v = i8::from_le_bytes([data[0]]);
        trap_func!(stack.push_i32(v as i32));
        code
    })
}
pub unsafe fn op_i32_load16_s(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    load_internal::<2>(tail_code, ctx, |stack, data, code| {
        let v = i16::from_le_bytes([data[0], data[1]]);
        trap_func!(stack.push_i32(v as i32));
        code
    })
}
pub unsafe fn op_i32_load16_u(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    load_internal::<2>(tail_code, ctx, |stack, data, code| {
        let v = u16::from_le_bytes([data[0], data[1]]);
        trap_func!(stack.push_u32(v as u32));
        code
    })
}
pub unsafe fn op_i64_load8_s(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    load_internal::<1>(tail_code, ctx, |stack, data, code| {
        let v = i8::from_le_bytes([data[0]]);
        trap_func!(stack.push_i64(v as i64));
        code
    })
}
pub unsafe fn op_i64_load8_u(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    load_internal::<1>(tail_code, ctx, |stack, data, code| {
        let v = u8::from_le_bytes([data[0]]);
        trap_func!(stack.push_u64(v as u64));
        code
    })
}
pub unsafe fn op_i64_load16_s(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    load_internal::<2>(tail_code, ctx, |stack, data, code| {
        let v = i16::from_le_bytes([data[0], data[1]]);
        trap_func!(stack.push_i64(v as i64));
        code
    })
}
pub unsafe fn op_i64_load16_u(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    load_internal::<2>(tail_code, ctx, |stack, data, code| {
        let v = u16::from_le_bytes([data[0], data[1]]);
        trap_func!(stack.push_u64(v as u64));
        code
    })
}
pub unsafe fn op_i64_load32_s(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    load_internal::<4>(tail_code, ctx, |stack, data, code| {
        let v = i32::from_le_bytes([data[0], data[1], data[2], data[3]]);
        trap_func!(stack.push_i64(v as i64));
        code
    })
}
pub unsafe fn op_i64_load32_u(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    load_internal::<4>(tail_code, ctx, |stack, data, code| {
        let v = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
        trap_func!(stack.push_u64(v as u64));
        code
    })
}
pub unsafe fn op_i32_store(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    store_internal(tail_code, ctx, |ctx| {
        WriteOperation::Write4(ctx.stack.pop_u8_array::<4>())
    })
}
pub unsafe fn op_i64_store(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    store_internal(tail_code, ctx, |ctx| {
        WriteOperation::Write8(ctx.stack.pop_u8_array::<8>())
    })
}
pub unsafe fn op_f32_store(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    store_internal(tail_code, ctx, |ctx| {
        WriteOperation::Write4(ctx.stack.pop_u8_array::<4>())
    })
}
pub unsafe fn op_f64_store(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    store_internal(tail_code, ctx, |ctx| {
        WriteOperation::Write8(ctx.stack.pop_u8_array::<8>())
    })
}
pub unsafe fn op_i32_store8(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    store_internal(tail_code, ctx, |ctx| {
        WriteOperation::Write1([ctx.stack.pop_u32().to_le_bytes()[0]])
    })
}
pub unsafe fn op_i32_store16(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    store_internal(tail_code, ctx, |ctx| {
        let v = ctx.stack.pop_u32().to_le_bytes();
        WriteOperation::Write2([v[0], v[1]])
    })
}
pub unsafe fn op_i64_store8(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    store_internal(tail_code, ctx, |ctx| {
        let v = ctx.stack.pop_u64().to_le_bytes();
        WriteOperation::Write1([v[0]])
    })
}
pub unsafe fn op_i64_store16(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    store_internal(tail_code, ctx, |ctx| {
        let v = ctx.stack.pop_u64().to_le_bytes();
        WriteOperation::Write2([v[0], v[1]])
    })
}
pub unsafe fn op_i64_store32(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    store_internal(tail_code, ctx, |ctx| {
        let v = ctx.stack.pop_u64().to_le_bytes();
        WriteOperation::Write4([v[0], v[1], v[2], v[3]])
    })
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
    let page_size = if let Some(mem) = ctx.memory() {
        mem.page_size()
    } else {
        return VMResult::MemoryIndexOutOfRange;
    };
    vm_try!(ctx.stack.push_u32(page_size));
    call_next(tail_code, 0, ctx)
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
    {
        let instance_id = ctx.instance_id();
        let memory = if let Some(v) = ctx.instance().mems.as_slice(ctx.gc).first().copied() {
            ctx.gc.get_memory(v)
        } else {
            return VMResult::MemoryIndexOutOfRange;
        };
        let dst_last = vm_try!(VMResult::from_option(d.checked_add(n), || {
            VMResult::MemoryIndexOutOfRange
        })) as usize;
        let d = d as usize;
        let dst = vm_try!(VMResult::from_option(memory.get_mut(d..dst_last), || {
            VMResult::MemoryIndexOutOfRange
        }));

        let src_last = vm_try!(VMResult::from_option(s.checked_add(n), || {
            VMResult::MemoryIndexOutOfRange
        })) as usize;
        let segments = ctx.store.lock_segments();
        let data = segments.data.get(&(instance_id, idx));
        if data.is_none() && n == 0 && s == 0 {
            // it is ok
        } else {
            let data = vm_try!(VMResult::from_option(data, || {
                VMResult::MemoryIndexOutOfRange
            }));
            let data = vm_try!(VMResult::from_option(
                data.init.get(s as usize..src_last),
                || { VMResult::MemoryIndexOutOfRange }
            ));
            dst.copy_from_slice(data);
        }
    }
    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_data_drop(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    wait_effect!(ctx, ctx.cont);
    let idx = (*tail_code).operand.u32;
    let instance_id = ctx.instance_id();
    ctx.store.lock_segments().data.remove(&(instance_id, idx));
    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_mem_copy(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    wait_effect!(ctx, ctx.cont);
    let len = ctx.stack.pop_u32();
    let src = ctx.stack.pop_u32();
    let dst = ctx.stack.pop_u32();
    trace!("op_mem_copy src: {src},dst: {dst},len: {len}");
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
    vm_try!(ctx
        .stack
        .push_u32(ctx.instance().funcs.as_slice(ctx.gc)[funcidx as usize].get()));
    call_next(tail_code, 1, ctx)
}
pub unsafe fn special_start_function_call(
    _tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    match vm_try!(invoke_host_function(&VM_END as *const Instr, ctx)) {
        CallOutcome::Immediate(ptr) => call_next(ptr, 0, ctx),
        CallOutcome::Pending => VMResult::Success(()),
    }
}
pub unsafe fn special_function_return(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    trace!("function return");
    let (prev_local_ref, tail_code) = ctx.stack.function_return(
        &ctx.local_reference(),
        (*tail_code).operand.drop_size as usize,
        ctx.gc,
    );
    ctx.local_reference = prev_local_ref;
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
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    wait_effect!(ctx, ctx.cont);
    ctx.cont = std::ptr::null();
    VMResult::Success(())
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
        let InstanceData {
            module_addr, funcs, ..
        } = unsafe {
            *gc.get_instance_unchecked(vm_try!(VMResult::from_option(
                instance.get_gc_ref_with_pool(store, &gc),
                || { VMResult::Unlinkable }
            )))
        };
        let module_inst = unsafe { gc.get_module(module_addr) };
        trace!("{:?}", module_inst.exports);
        let ft = if let Some(ExportDesc::Func(idx)) = module_inst.exports.find(name) {
            let code_addr = funcs.as_slice(&gc)[idx.0 as usize];
            let funcinst = unsafe { gc.get_func(code_addr) };
            let mut stack = Stack::new(128 * 1024);
            let tidx = module_inst.functions.get(idx.0 as usize).unwrap();
            let ft = module_inst
                .function_types
                .get(tidx.0 as usize)
                .unwrap()
                .clone();
            let param_size = result_type_size(&ft.0);

            let (locals_data, _code_offset) = funcinst.locals_and_code_offset(&gc);
            let local_size = locals_data.byte_size();
            vm_try!(push_result_values(&mut stack, &ft.0, args));

            tracing::trace!("run_module_function: {name} {local_size}");
            let local_reference = vm_try!(stack.function_call(
                param_size,
                local_size,
                code_addr,
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
    gc: &mut crate::common::gc::MemoryPool,
    name: &str,
    args: &ResultValue,
) -> Result<VMResult<ResultValue>, SyncRunError> {
    let mut scheduler: Scheduler<'_> = Scheduler::new(store);

    let ft = {
        let InstanceData {
            module_addr, funcs, ..
        } = unsafe {
            *gc.get_instance_unchecked(match instance.get_gc_ref_with_pool(store, gc) {
                Some(gc_ref) => gc_ref,
                None => return Ok(VMResult::Unlinkable),
            })
        };
        let module_inst = unsafe { gc.get_module(module_addr) };
        trace!("{:?}", module_inst.exports);
        let ft = if let Some(ExportDesc::Func(idx)) = module_inst.exports.find(name) {
            let code_addr = funcs.as_slice(gc)[idx.0 as usize];
            let funcinst = unsafe { gc.get_func(code_addr) };
            let mut stack = Stack::new(128 * 1024);
            let tidx = module_inst.functions.get(idx.0 as usize).unwrap();
            let ft = module_inst
                .function_types
                .get(tidx.0 as usize)
                .unwrap()
                .clone();
            let param_size = result_type_size(&ft.0);

            let (locals_data, _code_offset) = funcinst.locals_and_code_offset(gc);
            let local_size = locals_data.byte_size();
            let push_result = push_result_values(&mut stack, &ft.0, args);
            if !matches!(push_result, VMResult::Success(())) {
                return Ok(vm_result_err_into_result_value(push_result));
            }

            tracing::trace!("run_module_function: {name} {local_size}");
            let local_reference = match stack.function_call(
                param_size,
                local_size,
                code_addr,
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
    let module_inst = unsafe { gc.get_module(instance.module_addr) };
    if let Some(ExportDesc::Global(idx)) = module_inst.exports.find(name) {
        let addr = instance.globals.as_slice(&gc)[idx.0 as usize];
        let gt = module_inst.globals[idx.0 as usize];
        VMResult::Success(match gt.0 {
            ValType::I32 => {
                let mut buf = [0u8; 4];
                buf.copy_from_slice(unsafe { gc.get_global(addr) });
                WasmValue::I32(i32::from_le_bytes(buf))
            }
            ValType::I64 => {
                let mut buf = [0u8; 8];
                buf.copy_from_slice(unsafe { gc.get_global(addr) });
                WasmValue::I64(i64::from_le_bytes(buf))
            }
            _ => todo!(),
        })
    } else {
        unimplemented!()
    }
}
