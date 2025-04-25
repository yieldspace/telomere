use crate::{
    common::{ExecuteContext, Instr},
    runtime::vm::call_next,
    VMResult,
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
    for i in 0..16 {
        result[i] = ((v >> (8 * i) & 0xff) << shift) as u8;
    }

    vm_try!(ctx.stack.push_u128(u128::from_le_bytes(result)));

    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i8x16_add(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let v2 = ctx.stack.pop_u128();
    let v1 = ctx.stack.pop_u128();

    let mut result = [0u8; 16];
    for i in 0..16 {
        let v1_byte = ((v1 >> (i * 8)) & 0xff) as u8;
        let v2_byte = ((v2 >> (i * 8)) & 0xff) as u8;

        result[i] = v1_byte.wrapping_add(v2_byte);
    }
    let result_u128 = u128::from_le_bytes(result);
    vm_try!(ctx.stack.push_u128(result_u128));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i8x16_sub(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let v2 = ctx.stack.pop_u128();
    let v1 = ctx.stack.pop_u128();

    let mut result = [0u8; 16];
    for i in 0..16 {
        let v1_byte = ((v1 >> (i * 8)) & 0xff) as i8;
        let v2_byte = ((v2 >> (i * 8)) & 0xff) as i8;

        result[i] = v1_byte.wrapping_sub(v2_byte) as u8;
    }
    let result_u128 = u128::from_le_bytes(result);
    vm_try!(ctx.stack.push_u128(result_u128));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_f32x4_mul(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let v2 = ctx.stack.pop_f32x4();
    let v1 = ctx.stack.pop_f32x4();
    trace!("{v2:?} {v1:?}");
    let result = v1 * v2;
    trace!("{result:?}");
    vm_try!(ctx.stack.push_f32x4(result));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_f32x4_abs(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let v1 = ctx.stack.pop_f32x4();
    vm_try!(ctx.stack.push_f32x4(v1.abs()));
    call_next(tail_code, 0, ctx)
}