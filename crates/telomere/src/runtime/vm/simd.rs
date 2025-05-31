use crate::{
    common::stack::LaneType,
    runtime::{memory_effect::WriteOperation, vm::load_internal},
};
use telomere_macros::define_simd_operation;
use wide::{f32x4, f64x2, i16x8, i32x4, i64x2, i8x16, u16x8, u32x4, u64x2, u8x16};

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
pub unsafe fn i32x4_trunc_sat_f32x4_u(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    use crate::common::stack::StackOperation;
    let a: f32x4 = ctx.stack.pop();
    let a = a.to_array();
    vm_try!(ctx.stack.push(u32x4::from([
        a[0].trunc() as u32,
        a[1].trunc() as u32,
        a[2].trunc() as u32,
        a[3].trunc() as u32
    ])));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn i32x4_trunc_sat_f64x2_s(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    use crate::common::stack::StackOperation;
    let a: f64x2 = ctx.stack.pop();
    let a = a.to_array();
    vm_try!(ctx.stack.push(i32x4::from([
        a[0].trunc() as i32,
        a[1].trunc() as i32,
        0,
        0
    ])));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn i32x4_trunc_sat_f64x2_u(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    use crate::common::stack::StackOperation;
    let a: f64x2 = ctx.stack.pop();
    let a = a.to_array();
    vm_try!(ctx.stack.push(u32x4::from([
        a[0].trunc() as u32,
        a[1].trunc() as u32,
        0,
        0
    ])));
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
unsafe fn handle_binary_op<T>(
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
unsafe fn handle_unary_op<T>(
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

macro_rules! define_binary_simd_operation {
    ($op: ident,[$($target: ident),*],$expr: expr) => {
        define_simd_operation!(handle_binary_op,$op,[$($target),*],$expr);
    };
}
macro_rules! define_unary_simd_operation {
    ($op: ident,[$($target: ident),*],$expr: expr) => {
        define_simd_operation!(handle_unary_op,$op,[$($target),*],$expr);
    };
}
define_binary_simd_operation!(avgr, [u8x16], |a, b| {
    let mut res = [0u8; 16];
    let a = a.to_array();
    let b = b.to_array();
    for i in 0..16 {
        res[i] = (a[i] as u16 + b[i] as u16).div_ceil(2) as u8;
    }
    res.into()
});

define_binary_simd_operation!(avgr, [u16x8], |a, b| {
    let mut res = [0u16; 8];
    let a = a.to_array();
    let b = b.to_array();
    for i in 0..8 {
        res[i] = (a[i] as u32 + b[i] as u32).div_ceil(2) as u16;
    }
    res.into()
});
define_unary_simd_operation!(popcnt, [u8x16], |a| {
    let mut res = [0u8; 16];
    let a = a.to_array();
    for i in 0..16 {
        res[i] = a[i].count_ones() as u8;
    }
    res.into()
});

define_binary_simd_operation!(add, [f32x4, f64x2, i8x16, i16x8, i32x4, i64x2], |a, b| a
    + b);
define_binary_simd_operation!(add_sat, [i8x16, u8x16, i16x8, u16x8], |a, b| a
    .saturating_add(b));
define_binary_simd_operation!(sub, [f32x4, f64x2, i8x16, i16x8, i32x4, i64x2], |a, b| a
    - b);
define_binary_simd_operation!(sub_sat, [i8x16, u8x16, i16x8, u16x8], |a, b| a
    .saturating_sub(b));
define_binary_simd_operation!(mul, [f32x4, f64x2, i16x8, i32x4, i64x2], |a, b| a * b);
define_binary_simd_operation!(div, [f32x4, f64x2], |a, b| a / b);
define_binary_simd_operation!(swizzle, [i8x16], |a, b| a.swizzle(b));
define_binary_simd_operation!(min, [i8x16, u8x16, i16x8, u16x8, i32x4, u32x4], |a, b| a
    .min(b));
define_binary_simd_operation!(min, [f32x4], |a, b| {
    let aa = a.to_array();
    let bb = b.to_array();
    let mut result = [0.0f32; 4];

    for i in 0..4 {
        let (x, y) = (aa[i], bb[i]);
        result[i] = if x.is_nan() || y.is_nan() {
            f32::NAN
        } else if x == y {
            if x == 0.0 && y == 0.0 {
                if x.to_bits() == 0x8000_0000 || y.to_bits() == 0x8000_0000 {
                    -0.0
                } else {
                    0.0
                }
            } else {
                x
            }
        } else {
            x.min(y)
        };
    }

    f32x4::from(result)
});
define_binary_simd_operation!(min, [f64x2], |a, b| {
    let aa = a.to_array();
    let bb = b.to_array();
    let mut result = [0.0f64; 2];

    for i in 0..2 {
        let (x, y) = (aa[i], bb[i]);
        result[i] = if x.is_nan() || y.is_nan() {
            f64::NAN
        } else if x == y {
            if x == 0.0 && y == 0.0 {
                if x.to_bits() == 0x8000_0000_0000_0000 || y.to_bits() == 0x8000_0000_0000_0000 {
                    -0.0
                } else {
                    0.0
                }
            } else {
                x
            }
        } else {
            x.min(y)
        };
    }

    f64x2::from(result)
});

define_binary_simd_operation!(max, [i8x16, u8x16, i16x8, u16x8, i32x4, u32x4], |a, b| a
    .max(b));
define_binary_simd_operation!(max, [f32x4], |a, b| {
    let aa = a.to_array();
    let bb = b.to_array();
    let mut result = [0.0f32; 4];

    for i in 0..4 {
        let (x, y) = (aa[i], bb[i]);
        result[i] = if x.is_nan() || y.is_nan() {
            f32::NAN
        } else if x == y {
            if x == 0.0 && y == 0.0 {
                if x.to_bits() == 0x0000_0000 || y.to_bits() == 0x0000_0000 {
                    0.0
                } else {
                    -0.0
                }
            } else {
                x
            }
        } else {
            x.max(y)
        };
    }

    f32x4::from(result)
});
define_binary_simd_operation!(max, [f64x2], |a, b| {
    let aa = a.to_array();
    let bb = b.to_array();
    let mut result = [0.0f64; 2];

    for i in 0..2 {
        let (x, y) = (aa[i], bb[i]);
        result[i] = if x.is_nan() || y.is_nan() {
            f64::NAN
        } else if x == y {
            if x == 0.0 && y == 0.0 {
                if x.to_bits() == 0x0000_0000_0000_0000 || y.to_bits() == 0x0000_0000_0000_0000 {
                    0.0
                } else {
                    -0.0
                }
            } else {
                x
            }
        } else {
            x.max(y)
        };
    }

    f64x2::from(result)
});
define_binary_simd_operation!(pmin, [f32x4], |a, b| {
    let a_arr = a.to_array();
    let b_arr = b.to_array();
    let mut result = [0.0f32; 4];

    for i in 0..4 {
        let (va, vb) = (a_arr[i], b_arr[i]);
        match (va.is_nan(), vb.is_nan()) {
            (true, _) => result[i] = va,
            (_, true) => result[i] = va,
            (false, false) => {
                if va == vb && va == 0.0 {
                    result[i] = va;
                } else {
                    result[i] = va.min(vb);
                }
            }
        }
    }

    f32x4::from(result)
});
define_binary_simd_operation!(pmax, [f32x4], |a, b| {
    let a_arr = a.to_array();
    let b_arr = b.to_array();
    let mut result = [0.0f32; 4];

    for i in 0..4 {
        let (va, vb) = (a_arr[i], b_arr[i]);
        match (va.is_nan(), vb.is_nan()) {
            (true, _) => result[i] = va,
            (_, true) => result[i] = va,
            (false, false) => {
                if va == vb && va == 0.0 {
                    result[i] = va;
                } else {
                    result[i] = va.max(vb);
                }
            }
        }
    }

    f32x4::from(result)
});

define_binary_simd_operation!(pmin, [f64x2], |a, b| {
    let a_arr = a.to_array();
    let b_arr = b.to_array();
    let mut result = [0.0f64; 2];

    for i in 0..2 {
        let (va, vb) = (a_arr[i], b_arr[i]);
        match (va.is_nan(), vb.is_nan()) {
            (true, _) => result[i] = va,
            (_, true) => result[i] = va,
            (false, false) => {
                if va == vb && va == 0.0 {
                    result[i] = va;
                } else {
                    result[i] = va.min(vb);
                }
            }
        }
    }

    f64x2::from(result)
});
define_binary_simd_operation!(pmax, [f64x2], |a, b| {
    let a_arr = a.to_array();
    let b_arr = b.to_array();
    let mut result = [0.0f64; 2];

    for i in 0..2 {
        let (va, vb) = (a_arr[i], b_arr[i]);
        match (va.is_nan(), vb.is_nan()) {
            (true, _) => result[i] = va,
            (_, true) => result[i] = va,
            (false, false) => {
                if va == vb && va == 0.0 {
                    result[i] = va;
                } else {
                    result[i] = va.max(vb);
                }
            }
        }
    }

    f64x2::from(result)
});

define_unary_simd_operation!(abs, [f64x2, f32x4, i32x4, i8x16, i16x8, i64x2], |a| a.abs());
define_unary_simd_operation!(ceil, [f64x2, f32x4], |a| a.ceil());
define_unary_simd_operation!(floor, [f64x2, f32x4], |a| a.floor());
define_unary_simd_operation!(trunc, [f32x4], |a| {
    let arr = a.to_array();
    f32x4::from([
        arr[0].trunc(),
        arr[1].trunc(),
        arr[2].trunc(),
        arr[3].trunc(),
    ])
});
define_unary_simd_operation!(trunc, [f64x2], |a| {
    let arr = a.to_array();
    f64x2::from([arr[0].trunc(), arr[1].trunc()])
});
define_unary_simd_operation!(nearest, [f32x4], |a| {
    let arr = a.to_array();
    f32x4::from([
        arr[0].round_ties_even(),
        arr[1].round_ties_even(),
        arr[2].round_ties_even(),
        arr[3].round_ties_even(),
    ])
});
define_unary_simd_operation!(nearest, [f64x2], |a| {
    let arr = a.to_array();
    f64x2::from([arr[0].round_ties_even(), arr[1].round_ties_even()])
});
pub unsafe fn f32x4_neg(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let v: f32x4 = ctx.stack.pop();
    let [a, b, c, d] = v.to_array();
    vm_try!(ctx.stack.push(f32x4::from([-a, -b, -c, -d])));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn f64x2_neg(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let v: f64x2 = ctx.stack.pop();
    let [a, b] = v.to_array();
    vm_try!(ctx.stack.push(f64x2::from([-a, -b])));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn i8x16_neg(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    use std::ops::BitXor;
    let a: i8x16 = ctx.stack.pop();

    vm_try!(ctx.stack.push(a.bitxor(-i8x16::ONE) + i8x16::ONE));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn i16x8_neg(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    use std::ops::BitXor;
    let a: i16x8 = ctx.stack.pop();

    vm_try!(ctx.stack.push(a.bitxor(-i16x8::ONE) + i16x8::ONE));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn i32x4_neg(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    use std::ops::BitXor;
    let a: i32x4 = ctx.stack.pop();

    vm_try!(ctx.stack.push(a.bitxor(-i32x4::ONE) + i32x4::ONE));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn i64x2_neg(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    use std::ops::BitXor;
    let a: i64x2 = ctx.stack.pop();

    vm_try!(ctx.stack.push(a.bitxor(-i64x2::ONE) + i64x2::ONE));
    call_next(tail_code, 0, ctx)
}
define_unary_simd_operation!(sqrt, [f64x2, f32x4], |a| a.sqrt());
use std::ops::Not;
use wide::CmpEq;
use wide::CmpGe;
use wide::CmpGt;
use wide::CmpLe;
use wide::CmpLt;
use wide::CmpNe;
macro_rules! define_simd_cmp_operation {
    ($op_name: ident,[$($ty: ident),*],$op: expr) => {
        $(define_simd_operation!(handle_binary_op, $op_name, [$ty], |a, b| {
            let mut res = [0; $ty::LANE_SIZE];
            let a: [<$ty as LaneType>::BaseType; $ty::LANE_SIZE] = a.to_array();
            let b: [<$ty as LaneType>::BaseType; $ty::LANE_SIZE] = b.to_array();

            for i in 0..$ty::LANE_SIZE {
                let a: <$ty as LaneType>::BaseType = a[i];
                let b: <$ty as LaneType>::BaseType = b[i];

                res[i] = if ($op)(a, b) {
                    !(0 as <$ty as LaneType>::BaseType)
                } else {
                    0 as <$ty as LaneType>::BaseType
                };
            }
            res.into()
        });)*
    };
}

define_binary_simd_operation!(eq, [f32x4, f64x2, i8x16, i16x8, i32x4], |a, b| a.cmp_eq(b));
define_binary_simd_operation!(ne, [f32x4, f64x2], |a, b| a.cmp_ne(b));
define_binary_simd_operation!(ne, [i8x16, i16x8, i32x4], |a, b| a.cmp_eq(b).not());
define_binary_simd_operation!(lt, [f32x4, f64x2], |a, b| a.cmp_lt(b));
define_simd_cmp_operation!(lt, [i8x16, u8x16, i16x8, u16x8, i32x4, u32x4], |a, b| a < b);

define_binary_simd_operation!(gt, [f32x4, f64x2], |a, b| a.cmp_gt(b));
define_simd_cmp_operation!(gt, [i8x16, u8x16, i16x8, u16x8, i32x4, u32x4], |a, b| a > b);
define_binary_simd_operation!(le, [f32x4, f64x2], |a, b| a.cmp_le(b));
define_simd_cmp_operation!(le, [i8x16, u8x16, i16x8, u16x8, i32x4, u32x4], |a, b| a
    <= b);
define_binary_simd_operation!(ge, [f32x4, f64x2], |a, b| a.cmp_ge(b));
define_simd_cmp_operation!(ge, [i8x16, u8x16, i16x8, u16x8, i32x4, u32x4], |a, b| a
    >= b);
pub unsafe fn i16x8_extadd_pairwise_i8x16(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let mut res = [0i16; 8];
    let a: i8x16 = ctx.stack.pop();
    let a = a.to_array();
    for i in 0..8 {
        res[i] = a[i * 2] as i16 + a[i * 2 + 1] as i16;
    }
    vm_try!(ctx.stack.push(i16x8::from(res)));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn u16x8_extadd_pairwise_i8x16(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let mut res = [0u16; 8];
    let a: u8x16 = ctx.stack.pop();
    let a = a.to_array();
    for i in 0..8 {
        res[i] = a[i * 2] as u16 + a[i * 2 + 1] as u16;
    }
    vm_try!(ctx.stack.push(u16x8::from(res)));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn i32x4_extadd_pairwise_i16x8(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let mut res = [0i32; 4];
    let a: i16x8 = ctx.stack.pop();
    let a = a.to_array();
    for i in 0..4 {
        res[i] = a[i * 2] as i32 + a[i * 2 + 1] as i32;
    }
    vm_try!(ctx.stack.push(i32x4::from(res)));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn u32x4_extadd_pairwise_i16x8(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let mut res = [0u32; 4];
    let a: u16x8 = ctx.stack.pop();
    let a = a.to_array();
    for i in 0..4 {
        res[i] = a[i * 2] as u32 + a[i * 2 + 1] as u32;
    }
    vm_try!(ctx.stack.push(u32x4::from(res)));
    call_next(tail_code, 0, ctx)
}

define_binary_simd_operation!(q15mulr_sat_s, [i16x8], |a, b| a.mul_scale_round(b));
fn extend_low_i8x16_to_i16x8(input: i8x16) -> i16x8 {
    let arr = input.to_array();
    let mut extended = [0i16; 8];
    for i in 0..8 {
        extended[i] = arr[i] as i16;
    }
    i16x8::from(extended)
}
fn extend_high_i8x16_to_i16x8(input: i8x16) -> i16x8 {
    let arr = input.to_array();
    let mut extended = [0i16; 8];
    for i in 0..8 {
        extended[i] = arr[i + 8] as i16;
    }
    i16x8::from(extended)
}
fn extend_low_i16x8_to_i32x4(input: i16x8) -> i32x4 {
    let arr = input.to_array();
    let mut extended = [0i32; 4];
    for i in 0..4 {
        extended[i] = arr[i] as i32;
    }
    i32x4::from(extended)
}
fn extend_high_i16x8_to_i32x4(input: i16x8) -> i32x4 {
    let arr = input.to_array();
    let mut extended = [0i32; 4];
    for i in 0..4 {
        extended[i] = arr[i + 4] as i32;
    }
    i32x4::from(extended)
}
fn extend_low_u16x8_to_u32x4(input: u16x8) -> u32x4 {
    let arr = input.to_array();
    let mut extended = [0u32; 4];
    for i in 0..4 {
        extended[i] = arr[i] as u32;
    }
    u32x4::from(extended)
}
fn extend_high_u16x8_to_u32x4(input: u16x8) -> u32x4 {
    let arr = input.to_array();
    let mut extended = [0u32; 4];
    for i in 0..4 {
        extended[i] = arr[i + 4] as u32;
    }
    u32x4::from(extended)
}

pub unsafe fn i16x8_extmul_low(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let a: i8x16 = ctx.stack.pop();
    let b: i8x16 = ctx.stack.pop();

    let a = extend_low_i8x16_to_i16x8(a);
    let b = extend_low_i8x16_to_i16x8(b);
    vm_try!(ctx.stack.push(a * b));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn i16x8_extmul_high(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let a: i8x16 = ctx.stack.pop();
    let b: i8x16 = ctx.stack.pop();

    let a = extend_high_i8x16_to_i16x8(a);
    let b = extend_high_i8x16_to_i16x8(b);
    vm_try!(ctx.stack.push(a * b));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn u16x8_extmul_low(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let a: u8x16 = ctx.stack.pop();
    let b: u8x16 = ctx.stack.pop();

    let a = u16x8::from_u8x16_low(a);
    let b = u16x8::from_u8x16_low(b);
    vm_try!(ctx.stack.push(a * b));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn u16x8_extmul_high(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let a: u8x16 = ctx.stack.pop();
    let b: u8x16 = ctx.stack.pop();

    let a = u16x8::from_u8x16_high(a);
    let b = u16x8::from_u8x16_high(b);
    vm_try!(ctx.stack.push(a * b));
    call_next(tail_code, 0, ctx)
}

pub unsafe fn i32x4_extmul_low(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let a: i16x8 = ctx.stack.pop();
    let b: i16x8 = ctx.stack.pop();

    let a = extend_low_i16x8_to_i32x4(a);
    let b = extend_low_i16x8_to_i32x4(b);
    vm_try!(ctx.stack.push(a * b));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn i32x4_extmul_high(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let a: i16x8 = ctx.stack.pop();
    let b: i16x8 = ctx.stack.pop();

    let a = extend_high_i16x8_to_i32x4(a);
    let b = extend_high_i16x8_to_i32x4(b);
    vm_try!(ctx.stack.push(a * b));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn u32x4_extmul_low(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let a: u16x8 = ctx.stack.pop();
    let b: u16x8 = ctx.stack.pop();

    let a = extend_low_u16x8_to_u32x4(a);
    let b = extend_low_u16x8_to_u32x4(b);
    vm_try!(ctx.stack.push(a * b));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn u32x4_extmul_high(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let a: u16x8 = ctx.stack.pop();
    let b: u16x8 = ctx.stack.pop();

    let a = extend_high_u16x8_to_u32x4(a);
    let b = extend_high_u16x8_to_u32x4(b);
    vm_try!(ctx.stack.push(a * b));
    call_next(tail_code, 0, ctx)
}

pub unsafe fn i32x4_dot_i16x8(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let a: i16x8 = ctx.stack.pop();
    let b: i16x8 = ctx.stack.pop();

    let a = a.to_array();
    let b = b.to_array();
    vm_try!(ctx.stack.push(i32x4::from([
        i32::wrapping_add(a[0] as i32 * b[0] as i32, a[1] as i32 * b[1] as i32),
        i32::wrapping_add(a[2] as i32 * b[2] as i32, a[3] as i32 * b[3] as i32),
        i32::wrapping_add(a[4] as i32 * b[4] as i32, a[5] as i32 * b[5] as i32),
        i32::wrapping_add(a[6] as i32 * b[6] as i32, a[7] as i32 * b[7] as i32)
    ])));
    call_next(tail_code, 0, ctx)
}
