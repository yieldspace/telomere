use crate::{
    common::stack::LaneType,
    runtime::{memory_effect::WriteOperation, vm::load_internal},
};
use telomere_macros::define_simd_operation;
#[allow(unused_imports)]
use wide::{f32x4, f64x2, i16x8, i32x4, i64x2, i8x16, u32x4, u8x16};
use wide::{u16x8, u64x2};

use crate::{
    common::{stack::StackOperation, ExecuteContext, Instr},
    runtime::vm::call_next,
    Stack, VMResult,
};

use super::store_internal;

pub unsafe fn op_v128_load(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    load_internal::<16>(tail_code, ctx, |stack, data, next| {
        trap_func!(stack.push_slice(data));
        next
    })
}
pub unsafe fn v128_load8x8_s(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    load_internal::<8>(tail_code, ctx, |stack, data, next| {
        let extended = [
            data[0] as i16,
            data[1] as i16,
            data[2] as i16,
            data[3] as i16,
            data[4] as i16,
            data[5] as i16,
            data[6] as i16,
            data[7] as i16,
        ];
        trap_func!(stack.push(i16x8::from(extended)));
        next
    })
}
pub unsafe fn v128_load8x8_u(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    load_internal::<8>(tail_code, ctx, |stack, data, next| {
        let extended = [
            data[0] as u16,
            data[1] as u16,
            data[2] as u16,
            data[3] as u16,
            data[4] as u16,
            data[5] as u16,
            data[6] as u16,
            data[7] as u16,
        ];
        trap_func!(stack.push(u16x8::from(extended)));
        next
    })
}

pub unsafe fn v128_load16x4_s(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    load_internal::<8>(tail_code, ctx, |stack, data, next| {
        let i16s = [
            i16::from_le_bytes([data[0], data[1]]),
            i16::from_le_bytes([data[2], data[3]]),
            i16::from_le_bytes([data[4], data[5]]),
            i16::from_le_bytes([data[6], data[7]]),
        ];

        let extended = [
            i16s[0] as i32,
            i16s[1] as i32,
            i16s[2] as i32,
            i16s[3] as i32,
        ];
        trap_func!(stack.push(i32x4::from(extended)));
        next
    })
}
pub unsafe fn v128_load16x4_u(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    load_internal::<8>(tail_code, ctx, |stack, data, next| {
        let u16s = [
            i16::from_le_bytes([data[0], data[1]]),
            i16::from_le_bytes([data[2], data[3]]),
            i16::from_le_bytes([data[4], data[5]]),
            i16::from_le_bytes([data[6], data[7]]),
        ];

        let extended = [
            u16s[0] as u32,
            u16s[1] as u32,
            u16s[2] as u32,
            u16s[3] as u32,
        ];
        trap_func!(stack.push(u32x4::from(extended)));
        next
    })
}

pub unsafe fn v128_store(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    store_internal(tail_code, ctx, |ctx| {
        WriteOperation::Write16(ctx.stack.pop_u128().to_le_bytes())
    })
}

pub unsafe fn v128_load32x2_s(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    load_internal::<8>(tail_code, ctx, |stack, bytes, next| {
        let i32s = [
            i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
            i32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
        ];

        let extended = [i32s[0] as i64, i32s[1] as i64];

        let v = i64x2::from(extended);
        trap_func!(stack.push(v));
        next
    })
}
pub unsafe fn v128_load32x2_u(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    load_internal::<8>(tail_code, ctx, |stack, bytes, next| {
        let u32s = [
            u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
            u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
        ];

        let extended = [u32s[0] as u64, u32s[1] as u64];

        let v = u64x2::from(extended);
        trap_func!(stack.push(v));
        next
    })
}

pub unsafe fn v128_load8_splat(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    load_internal::<1>(tail_code, ctx, |stack, bytes, next| {
        let v = i8x16::from(bytes[0] as i8);
        trap_func!(stack.push(v));
        next
    })
}

pub unsafe fn v128_load16_splat(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    load_internal::<2>(tail_code, ctx, |stack, bytes, next| {
        let v = i16x8::from(i16::from_le_bytes([bytes[0], bytes[1]]));
        trap_func!(stack.push(v));
        next
    })
}

pub unsafe fn v128_load32_splat(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    load_internal::<4>(tail_code, ctx, |stack, bytes, next| {
        let v = i32x4::from(i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]));
        trap_func!(stack.push(v));
        next
    })
}

pub unsafe fn v128_load64_splat(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    load_internal::<4>(tail_code, ctx, |stack, bytes, next| {
        let v = i64x2::from(i64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]));
        trap_func!(stack.push(v));
        next
    })
}

pub unsafe fn v128_const(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let left_buf = &(*tail_code).operand.encoded;
    let right_buf = &(*tail_code.add(1)).operand.encoded;
    let mut buf = [0u8; 16];
    buf[0..8].copy_from_slice(left_buf);
    buf[8..16].copy_from_slice(right_buf);

    vm_try!(ctx.stack.push_u128(u128::from_le_bytes(buf)));
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
pub unsafe fn v128_not(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_u128();
    vm_try!(ctx.stack.push_u128(!b));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn v128_and(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_u128();
    let a = ctx.stack.pop_u128();
    vm_try!(ctx.stack.push_u128(a & b));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn v128_andnot(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_u128();
    let a = ctx.stack.pop_u128();
    vm_try!(ctx.stack.push_u128(a & !b));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn v128_or(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_u128();
    let a = ctx.stack.pop_u128();
    vm_try!(ctx.stack.push_u128(a | b));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn v128_xor(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_u128();
    let a = ctx.stack.pop_u128();
    vm_try!(ctx.stack.push_u128(a ^ b));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn v128_any_true(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let v = ctx.stack.pop_u128();
    let result = if v == 0 { 0 } else { 1 };
    vm_try!(ctx.stack.push_i32(result));
    call_next(tail_code, 0, ctx)
}

macro_rules! all_true_instruction {
    ($name: ident,$target: ident) => {
        pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            let v: $target = ctx.stack.pop();
            let mut all_true = 0x01;
            for v in v.to_array() {
                all_true &= (v != 0) as i32;
            }
            vm_try!(ctx.stack.push_i32(all_true));
            call_next(tail_code, 0, ctx)
        }
    };
}

all_true_instruction!(i8x16_all_true, i8x16);
all_true_instruction!(i16x8_all_true, i16x8);
all_true_instruction!(i32x4_all_true, i32x4);
all_true_instruction!(i64x2_all_true, i64x2);
macro_rules! bitmask_instruction {
    ($name: ident,$target: ident) => {
        pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            let v: $target = ctx.stack.pop();
            let result = v.move_mask();
            vm_try!(ctx.stack.push_i32(result));
            call_next(tail_code, 0, ctx)
        }
    };
}
bitmask_instruction!(i8x16_bitmask, i8x16);
bitmask_instruction!(i16x8_bitmask, i16x8);
bitmask_instruction!(i32x4_bitmask, i32x4);
bitmask_instruction!(i64x2_bitmask, i64x2);

pub unsafe fn op_v128_bitselect(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let mask = ctx.stack.pop_u128();
    let b = ctx.stack.pop_u128();
    let a = ctx.stack.pop_u128();

    let result = a & mask | b & !mask;

    vm_try!(ctx.stack.push_u128(result));
    call_next(tail_code, 0, ctx)
}
macro_rules! shl_instruction {
    ($name: ident,$target: ident) => {
        pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            let shift = ctx.stack.pop_u32();
            let v: $target = ctx.stack.pop();

            let shift = shift as u32;

            let mut v = v.to_array();
            for i in 0..v.len() {
                v[i] = v[i].wrapping_shl(shift);
            }

            vm_try!(ctx.stack.push($target::from(v)));
            call_next(tail_code, 0, ctx)
        }
    };
}

macro_rules! shr_instruction {
    ($name: ident,$target: ident) => {
        pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            let shift = ctx.stack.pop_u32();
            let v: $target = ctx.stack.pop();

            let shift = shift as u32;

            let mut v = v.to_array();
            for i in 0..v.len() {
                v[i] = v[i].wrapping_shr(shift);
            }

            vm_try!(ctx.stack.push($target::from(v)));
            call_next(tail_code, 0, ctx)
        }
    };
}

shl_instruction!(i8x16_shl, i8x16);
shl_instruction!(i16x8_shl, i16x8);
shl_instruction!(i32x4_shl, i32x4);
shl_instruction!(i64x2_shl, i64x2);

shr_instruction!(i8x16_shr, i8x16);
shr_instruction!(i16x8_shr, i16x8);
shr_instruction!(i32x4_shr, i32x4);
shr_instruction!(i64x2_shr, i64x2);
shr_instruction!(u8x16_shr, u8x16);
shr_instruction!(u16x8_shr, u16x8);
shr_instruction!(u32x4_shr, u32x4);
shr_instruction!(u64x2_shr, u64x2);
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
pub unsafe fn f32x4_convert_i32x4_s(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    use crate::common::stack::StackOperation;
    let a: i32x4 = ctx.stack.pop();
    let [a, b, c, d] = a.to_array();
    let result = f32x4::from([a as f32, b as f32, c as f32, d as f32]);
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

pub unsafe fn f64x2_convert_low_i32x4_s(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    use crate::common::stack::StackOperation;
    let v: i32x4 = ctx.stack.pop();
    let result = f64x2::from_i32x4_lower2(v);
    vm_try!(ctx.stack.push(result));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn f64x2_convert_low_i32x4_u(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    use crate::common::stack::StackOperation;
    let a: u32x4 = ctx.stack.pop();
    let [a, b, _c, _d] = a.to_array();
    let result = f64x2::from([a as f64, b as f64]);
    vm_try!(ctx.stack.push(result));
    call_next(tail_code, 0, ctx)
}
macro_rules! narrow_instruction {
    ($name: ident,$from: ident,$to: ident) => {
        pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            use crate::common::stack::StackOperation;

            let b: $from = ctx.stack.pop();
            let a: $from = ctx.stack.pop();
            let mut result: [<$to as LaneType>::BaseType; <$to as LaneType>::LANE_SIZE] =
                [0; <$to as LaneType>::LANE_SIZE];
            let a_arr = a.to_array();
            let b_arr = b.to_array();

            for i in 0..<$from as LaneType>::LANE_SIZE {
                result[i] = a_arr[i].clamp(
                    <$to as LaneType>::BaseType::MIN as <$from as LaneType>::BaseType,
                    <$to as LaneType>::BaseType::MAX as <$from as LaneType>::BaseType,
                ) as <$to as LaneType>::BaseType;
                result[i + <$from as LaneType>::LANE_SIZE] = b_arr[i].clamp(
                    <$to as LaneType>::BaseType::MIN as <$from as LaneType>::BaseType,
                    <$to as LaneType>::BaseType::MAX as <$from as LaneType>::BaseType,
                )
                    as <$to as LaneType>::BaseType;
            }

            vm_try!(ctx.stack.push($to::from(result)));
            call_next(tail_code, 0, ctx)
        }
    };
}

narrow_instruction!(i8x16_narrow_i16x8_s, i16x8, i8x16);
narrow_instruction!(i8x16_narrow_i16x8_u, i16x8, u8x16);
narrow_instruction!(i16x8_narrow_i32x4_s, i32x4, i16x8);
narrow_instruction!(i16x8_narrow_i32x4_u, i32x4, u16x8);

pub unsafe fn f32x4_demote_f64x2_zero(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let v: f64x2 = ctx.stack.pop();
    let [a, b] = v.to_array();
    vm_try!(ctx
        .stack
        .push(f32x4::from([a as f32, b as f32, 0.0f32, 0.0f32])));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn f64x2_promote_low_f32x4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let v: f32x4 = ctx.stack.pop();
    let [a, b, _c, _d] = v.to_array();
    vm_try!(ctx.stack.push(f64x2::from([a as f64, b as f64])));
    call_next(tail_code, 0, ctx)
}
macro_rules! extend_instruction {
    ($name: ident,$from: ident,$to: ident,$($index: expr),*)=> {
        pub unsafe fn $name(    tail_code: *const Instr,
            ctx: &mut ExecuteContext)->VMResult<()>{
                let v: $from = ctx.stack.pop();
                let v = v.to_array();
                vm_try!(ctx.stack.push($to::from([$(v[$index] as <$to as LaneType>::BaseType),*])));

                call_next(tail_code, 0, ctx)

            }
    }
}

extend_instruction!(
    i16x8_extend_low_i8x16_s,
    i8x16,
    i16x8,
    0,
    1,
    2,
    3,
    4,
    5,
    6,
    7
);
extend_instruction!(
    i16x8_extend_high_i8x16_s,
    i8x16,
    i16x8,
    8,
    9,
    10,
    11,
    12,
    13,
    14,
    15
);
extend_instruction!(
    i16x8_extend_low_i8x16_u,
    u8x16,
    u16x8,
    0,
    1,
    2,
    3,
    4,
    5,
    6,
    7
);
extend_instruction!(
    i16x8_extend_high_i8x16_u,
    u8x16,
    u16x8,
    8,
    9,
    10,
    11,
    12,
    13,
    14,
    15
);

extend_instruction!(i32x4_extend_low_i16x8_s, i16x8, i32x4, 0, 1, 2, 3);
extend_instruction!(i32x4_extend_high_i16x8_s, i16x8, i32x4, 4, 5, 6, 7);
extend_instruction!(i32x4_extend_low_i16x8_u, u16x8, u32x4, 0, 1, 2, 3);
extend_instruction!(i32x4_extend_high_i16x8_u, u16x8, u32x4, 4, 5, 6, 7);

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

macro_rules! define_unary_simd_operation {
    ($op: ident,[$($target: ident),*],$expr: expr) => {
        define_simd_operation!(handle_unary_op,$op,[$($target),*],$expr);
    };
}
macro_rules! define_binary_simd_operation {
    ($op: ident,[$($target: ident),*],$expr: expr) => {
        define_simd_operation!(handle_binary_op,$op,[$($target),*],$expr);
    };
}
define_unary_simd_operation!(add, [i8x16, i32x4, i64x2, f32x4], |a, b| a + b);
define_unary_simd_operation!(sub, [i8x16, i32x4, f32x4], |a, b| a - b);
define_unary_simd_operation!(mul, [f32x4, i32x4], |a, b| a * b);
define_unary_simd_operation!(div, [f32x4], |a, b| a / b);
define_unary_simd_operation!(swizzle, [i8x16], |a, b| a.swizzle(b));
define_unary_simd_operation!(min, [i8x16, u8x16, f32x4], |a, b| a.min(b)); // FIXME: nan behaviour
define_unary_simd_operation!(max, [i8x16, u8x16, f32x4], |a, b| a.max(b)); // FIXME: nan behaviour
define_unary_simd_operation!(pmin, [f32x4], |a, b| a.max(b));
define_unary_simd_operation!(pmax, [f32x4], |a, b| a.max(b));
define_binary_simd_operation!(abs, [f32x4, i32x4], |a| a.abs());
pub unsafe fn f32x4_neg(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let v: f32x4 = ctx.stack.pop();
    let [a, b, c, d] = v.to_array();
    vm_try!(ctx.stack.push(f32x4::from([-a, -b, -c, -d])));
    call_next(tail_code, 0, ctx)
}
define_binary_simd_operation!(sqrt, [f32x4], |a| a.sqrt());
use wide::CmpEq;
use wide::CmpNe;
use wide::CmpLt;
use wide::CmpGt;
use wide::CmpLe;
use wide::CmpGe;
define_unary_simd_operation!(eq, [f32x4], |a, b| a.cmp_eq(b));
define_unary_simd_operation!(ne, [f32x4], |a, b| a.cmp_ne(b));
define_unary_simd_operation!(lt, [f32x4], |a, b| a.cmp_lt(b));
define_unary_simd_operation!(gt, [f32x4], |a, b| a.cmp_gt(b));
define_unary_simd_operation!(le, [f32x4], |a, b| a.cmp_le(b));
define_unary_simd_operation!(ge, [f32x4], |a, b| a.cmp_ge(b));
