#[allow(unused_imports)]
use wide::{f32x4, f64x2, i16x8, i32x4, i64x2, i8x16, u32x4, u8x16};

use crate::{
    common::{stack::StackOperation, ExecuteContext, Instr},
    runtime::vm::call_next,
    Stack, VMResult,
};

pub unsafe fn op_v128_load(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let memarg = (*tail_code).operand.memarg;
    let offset = ctx.stack.pop_u32();

    let memory = vm_try!(VMResult::from_option(ctx.memory(), || {
        VMResult::MemoryIndexOutOfRange
    }));
    let v = vm_try!(memory.read_u128(memarg, offset));
    vm_try!(ctx.stack.push_u128(v));
    call_next(tail_code, 1, ctx)
}

pub unsafe fn v128_const(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let left_buf = &(*tail_code).operand.encoded;
    let right_buf = &(*tail_code.add(1)).operand.encoded;
    let mut buf = [0u8; 16];
    buf[0..8].copy_from_slice(left_buf);
    buf[8..16].copy_from_slice(right_buf);

    vm_try!(ctx.stack.push_i128(i128::from_le_bytes(buf)));
    call_next(tail_code, 2, ctx)
}

pub unsafe fn op_i8x16_extract_lane_s(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lane = (*tail_code).operand.u32;
    let v = ctx.stack.pop_u128();
    let bytes = v.to_le_bytes();
    let value = bytes[lane as usize] as i8 as i32;
    vm_try!(ctx.stack.push_i32(value));
    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_i8x16_eq(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_u128();
    let a = ctx.stack.pop_u128();
    let a_bytes = a.to_le_bytes();
    let b_bytes = b.to_le_bytes();
    let mut result = [0u8; 16];
    for i in 0..16 {
        result[i] = ((a_bytes[i] == b_bytes[i]) as u8) * 0xFF;
    }
    vm_try!(ctx.stack.push_u128(u128::from_le_bytes(result)));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_v128_not(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_u128();
    vm_try!(ctx.stack.push_u128(!b));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i8x16_all_true(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let v = ctx.stack.pop_u128();
    let bytes = v.to_le_bytes();

    let mut all_true = 0xff;

    for &byte in &bytes {
        all_true &= byte;
    }

    let result = if all_true != 0 { 1 } else { 0 };

    vm_try!(ctx.stack.push_i32(result));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_v128_bitselect(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let mask = ctx.stack.pop_u128();
    let b = ctx.stack.pop_u128();
    let a = ctx.stack.pop_u128();

    let result = a & mask | b & !mask;

    vm_try!(ctx.stack.push_u128(result));
    call_next(tail_code, 0, ctx)
}

pub unsafe fn op_i8x16_shl(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let shift = ctx.stack.pop_i32();
    let v = ctx.stack.pop_u128();

    let shift = shift as u32 & 7;

    let mut result = [0u8; 16];
    #[allow(clippy::needless_range_loop)]
    for i in 0..16 {
        result[i] = (((v >> (8 * i)) & 0xff) << shift) as u8;
    }

    vm_try!(ctx.stack.push_u128(u128::from_le_bytes(result)));

    call_next(tail_code, 0, ctx)
}

pub unsafe fn i32x4_trunc_sat_f32x4_s(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    use crate::common::stack::StackOperation;
    let a: f32x4 = ctx.stack.pop();
    let result = a.trunc_int();
    vm_try!(ctx.stack.push(result));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn f32x4_convert_i32x4_u(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    use crate::common::stack::StackOperation;
    let a: u32x4 = ctx.stack.pop();
    let [a, b, c, d] = a.to_array();
    let result = f32x4::from([a as f32, b as f32, c as f32, d as f32]);
    vm_try!(ctx.stack.push(result));
    call_next(tail_code, 0, ctx)
}

#[inline]
unsafe fn handle_unary_op<T>(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    op: impl FnOnce(T, T) -> T,
) -> VMResult<()>
where
    Stack: StackOperation<T>,
{
    use crate::common::stack::StackOperation;
    let v2: T = ctx.stack.pop();
    let v1: T = ctx.stack.pop();
    let result = op(v1, v2);
    vm_try!(ctx.stack.push(result));
    call_next(tail_code, 0, ctx)
}

macro_rules! impl_unary_op {
    ([$(($name:ident, $target: ty)),*],$closure: expr) => {
        $(pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            handle_unary_op::<$target>(tail_code, ctx, $closure)
        })*
    };
}
#[inline]
unsafe fn handle_binary_op<T>(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    op: impl FnOnce(T) -> T,
) -> VMResult<()>
where
    Stack: StackOperation<T>,
{
    use crate::common::stack::StackOperation;
    let a: T = ctx.stack.pop();
    let result = op(a);
    vm_try!(ctx.stack.push(result));
    call_next(tail_code, 0, ctx)
}
macro_rules! impl_binary_op {
    ([$(($name:ident, $target: ty)),*], $closure: expr) => {
        $(pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            handle_binary_op::<$target>(tail_code, ctx, $closure)
        })*
    };
}
include!("simd_generated.rs");
