use super::*;
use std::ops::BitXor;

const I32_SELECT_BIT_STEP_MASK_SHIFTED: u32 = 1 << 0;
const I32_SELECT_BIT_STEP_EQ_CONDITION: u32 = 1 << 1;
const I32_SELECT_BIT_STEP_TEE_DST: u32 = 1 << 2;

#[cfg(feature = "vm-profile")]
#[cold]
#[inline(never)]
fn profile_numeric_family_enabled(label: &'static str) {
    dispatch_profile_count(label);
}

#[inline(always)]
fn profile_numeric_family(_label: &'static str) {
    #[cfg(feature = "vm-profile")]
    if dispatch_profile_enabled() {
        profile_numeric_family_enabled(_label);
    }
}

#[inline(always)]
pub(crate) fn crc16_update16_bits(data: u32, mut crc: u32) -> u32 {
    for bit in 0..16 {
        let shifted = (crc >> 1) & 0x7fff;
        crc = if ((crc ^ (data >> bit)) & 1) != 0 {
            shifted ^ 0xa001
        } else {
            shifted
        };
    }
    crc
}

/// Telomere internal inlined 16-bit CRC bit-step update recognized from a pure select-bit-step function.
///
/// Stack effect: `[] -> [i32]`.
/// Traps: none.
/// Notes: The optimizer selects this only for a verified local-only bit-step function body and
/// tail-dispatches to the materialized function return.
///
/// # Safety
/// - `tail_code` must point to the data local, crc local, and function-return target operands.
/// - The active frame must match the validated lowered shape selected by the optimizer.
pub unsafe fn op_i32_crc16_update16(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    profile_numeric_family("op_i32_crc16_update16");
    let data_local = (*tail_code).operand.local_addr as usize;
    let crc_local = (*tail_code.add(1)).operand.local_addr as usize;
    let local_base = ctx.local_base_ptr as *const u8;
    let data = ctx.stack.local_u32_from_base(local_base, data_local);
    let crc = ctx.stack.local_u32_from_base(local_base, crc_local);
    vm_try!(ctx.stack.push_u32_fast(crc16_update16_bits(data, crc)));
    let return_addr = (*tail_code.add(2)).operand.jump_addr;
    call_next(ctx.code().offset(return_addr as isize), 0, ctx)
}

/// Telomere internal inlined 16-bit CRC update wrapper that masks the data argument before updating.
///
/// Stack effect: `[] -> [i32]`.
/// Traps: none.
/// Notes: Selected for a materialized wrapper that only masks its first local and tail-calls the
/// CRC update native entry.
///
/// # Safety
/// - `tail_code` must point to the data local, crc local, and function-return target operands.
/// - The active frame must match the wrapper shape selected by the instantiator.
pub unsafe fn op_i32_crc16_update16_masked(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    profile_numeric_family("op_i32_crc16_update16_masked");
    let data_local = (*tail_code).operand.local_addr as usize;
    let crc_local = (*tail_code.add(1)).operand.local_addr as usize;
    let local_base = ctx.local_base_ptr as *const u8;
    let data = ctx.stack.local_u32_from_base(local_base, data_local) & 0xffff;
    let crc = ctx.stack.local_u32_from_base(local_base, crc_local);
    vm_try!(ctx.stack.push_u32_fast(crc16_update16_bits(data, crc)));
    let return_addr = (*tail_code.add(2)).operand.jump_addr;
    call_next(ctx.code().offset(return_addr as isize), 0, ctx)
}

/// WebAssembly `i32.const`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[] -> [value]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_const(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let v = (*tail_code).operand.i32;
    trace!("op_i32_const: {v}");
    vm_try!(ctx.stack.push_i32_fast(v));
    call_next(tail_code, 1, ctx)
}

/// WebAssembly `i32.add`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_add(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let _ = ctx.stack.reduce_top_i32_add();
    trace!("op_i32_add");
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `i32.sub`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_sub(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let _ = ctx.stack.reduce_top_i32_sub();
    trace!("op_i32_sub");
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `i64.clz`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_clz(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let a = ctx.stack.pop_i64();
    vm_try!(ctx.stack.push_i64(a.leading_zeros().into()));
    trace!("op_i64_ctz");
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `i64.ctz`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_ctz(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let a = ctx.stack.pop_i64();
    vm_try!(ctx.stack.push_i64(a.trailing_zeros().into()));
    trace!("op_i64_ctz");
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `i64.popcnt`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_popcnt(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let a = ctx.stack.pop_i64();
    vm_try!(ctx.stack.push_i64(a.count_ones().into()));
    trace!("op_i64_ctz");
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `i64.sub`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_sub(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let a = ctx.stack.pop_i64();
    let b = ctx.stack.pop_i64();
    let r = b.wrapping_sub(a);
    vm_try!(ctx.stack.push_i64(r));
    trace!("op_i64_sub: {a} {b} {r}");
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `i64.const`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[] -> [value]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_const(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    trace!("op_i64_const");
    vm_try!(ctx.stack.push_i64((*tail_code).operand.i64));
    call_next(tail_code, 1, ctx)
}

/// WebAssembly `f32.const`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[] -> [value]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f32_const(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    trace!("op_f32_const");
    vm_try!(ctx.stack.push_f32((*tail_code).operand.f32));
    call_next(tail_code, 1, ctx)
}

/// WebAssembly `f64.const`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[] -> [value]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f64_const(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    trace!("op_f64_const");
    vm_try!(ctx.stack.push_f64((*tail_code).operand.f64));
    call_next(tail_code, 1, ctx)
}

/// WebAssembly `f32.lt`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [i32]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f32_lt(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    trace!("op_f32_lt");
    let b = ctx.stack.pop_f32();
    let a = ctx.stack.pop_f32();
    vm_try!(ctx.stack.push_u32(u32::from(a < b)));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `f32.gt`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [i32]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f32_gt(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    trace!("op_f32_gt");
    let b = ctx.stack.pop_f32();
    let a = ctx.stack.pop_f32();
    vm_try!(ctx.stack.push_u32(u32::from(a > b)));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `f32.sqrt`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f32_sqrt(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    trace!("op_f32_sqrt");
    let a = ctx.stack.pop_f32();
    vm_try!(ctx.stack.push_f32(a.sqrt()));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `f32.add`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f32_add(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    trace!("op_f32_add");
    let b = ctx.stack.pop_f32();
    let a = ctx.stack.pop_f32();
    vm_try!(ctx.stack.push_f32(a + b));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `f32.sub`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f32_sub(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    trace!("op_f32_sub");
    let a = ctx.stack.pop_f32();
    let b = ctx.stack.pop_f32();
    vm_try!(ctx.stack.push_f32(b - a));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `f32.mul`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f32_mul(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    trace!("op_f32_mul");
    let b = ctx.stack.pop_f32();
    let a = ctx.stack.pop_f32();
    vm_try!(ctx.stack.push_f32(a * b));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `f32.div`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f32_div(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    trace!("op_f32_div");
    let b = ctx.stack.pop_f32();
    let a = ctx.stack.pop_f32();
    vm_try!(ctx.stack.push_f32(a / b));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `f32.min`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
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

/// WebAssembly `f32.max`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
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

/// WebAssembly `f32.copysign`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f32_copysign(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    trace!("op_f32_copysign");
    let b = ctx.stack.pop_f32();
    let a = ctx.stack.pop_f32();
    vm_try!(ctx.stack.push_f32(a.copysign(b)));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `f64.add`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f64_add(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    trace!("op_f64_add");
    let b = ctx.stack.pop_f64();
    let a = ctx.stack.pop_f64();
    vm_try!(ctx.stack.push_f64(a + b));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `f64.sub`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f64_sub(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    trace!("op_f64_sub");
    let a = ctx.stack.pop_f64();
    let b = ctx.stack.pop_f64();
    vm_try!(ctx.stack.push_f64(b - a));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `f64.mul`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f64_mul(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    trace!("op_f64_mul");
    let b = ctx.stack.pop_f64();
    let a = ctx.stack.pop_f64();
    vm_try!(ctx.stack.push_f64(a * b));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `f64.div`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f64_div(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    trace!("op_f64_div");
    let b = ctx.stack.pop_f64();
    let a = ctx.stack.pop_f64();
    vm_try!(ctx.stack.push_f64(a / b));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `f64.min`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
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

/// WebAssembly `f64.max`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
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

/// WebAssembly `f64.copysign`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f64_copysign(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    trace!("op_f64_copysign");
    let b = ctx.stack.pop_f64();
    let a = ctx.stack.pop_f64();
    vm_try!(ctx.stack.push_f64(a.copysign(b)));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `i32.wrap_i64`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_wrap_i64(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    trace!("op_i32_wrap_i64");
    let a = ctx.stack.pop_i64();
    vm_try!(ctx.stack.push_i32(a as i32));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `i32.trunc_f32_s`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: traps on NaN or out-of-range conversion.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
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

/// WebAssembly `i32.trunc_f32_u`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: traps on NaN or out-of-range conversion.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
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

/// WebAssembly `i32.trunc_f64_s`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: traps on NaN or out-of-range conversion.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
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

/// WebAssembly `i32.trunc_f64_u`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: traps on NaN or out-of-range conversion.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
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

/// WebAssembly `i64.trunc_f32_s`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: traps on NaN or out-of-range conversion.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
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

/// WebAssembly `i64.trunc_f32_u`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: traps on NaN or out-of-range conversion.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
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

/// WebAssembly `i64.trunc_f64_s`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: traps on NaN or out-of-range conversion.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
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

/// WebAssembly `i64.trunc_f64_u`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: traps on NaN or out-of-range conversion.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
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

/// WebAssembly `i32.trunc_sat_f32_s`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_trunc_sat_f32_s(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let a = ctx.stack.pop_f32();
    vm_try!(ctx.stack.push_i32(a.trunc() as i32));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `i32.trunc_sat_f32_u`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_trunc_sat_f32_u(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let a = ctx.stack.pop_f32();
    vm_try!(ctx.stack.push_u32(a.trunc() as u32));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `i32.trunc_sat_f64_s`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_trunc_sat_f64_s(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let a = ctx.stack.pop_f64();
    vm_try!(ctx.stack.push_i32(a.trunc() as i32));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `i32.trunc_sat_f64_u`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_trunc_sat_f64_u(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let a = ctx.stack.pop_f64();
    vm_try!(ctx.stack.push_u32(a.trunc() as u32));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `i64.trunc_sat_f32_s`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_trunc_sat_f32_s(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let a = ctx.stack.pop_f32();
    vm_try!(ctx.stack.push_i64(a.trunc() as i64));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `i64.trunc_sat_f32_u`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_trunc_sat_f32_u(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let a = ctx.stack.pop_f32();
    vm_try!(ctx.stack.push_u64(a.trunc() as u64));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `i64.trunc_sat_f64_s`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_trunc_sat_f64_s(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let a = ctx.stack.pop_f64();
    vm_try!(ctx.stack.push_i64(a.trunc() as i64));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `i64.trunc_sat_f64_u`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_trunc_sat_f64_u(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let a = ctx.stack.pop_f64();
    vm_try!(ctx.stack.push_u64(a.trunc() as u64));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `i64.add`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_add(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let a = ctx.stack.pop_i64();
    let b = ctx.stack.pop_i64();
    vm_try!(ctx.stack.push_i64(a.wrapping_add(b)));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `i64.extend_i32_s`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_extend_i32_s(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let a = ctx.stack.pop_i32();
    vm_try!(ctx.stack.push_i64(a.into()));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `i64.extend_i32_u`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_extend_i32_u(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let a = ctx.stack.pop_u32();
    vm_try!(ctx.stack.push_i64(a.into()));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `f32.convert_i32_s`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f32_convert_i32_s(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let a = ctx.stack.pop_i32();
    vm_try!(ctx.stack.push_f32(a as f32));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `f32.convert_i32_u`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f32_convert_i32_u(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let a = ctx.stack.pop_u32();
    vm_try!(ctx.stack.push_f32(a as f32));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `f32.convert_i64_s`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f32_convert_i64_s(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let a = ctx.stack.pop_i64();
    vm_try!(ctx.stack.push_f32(a as f32));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `f32.convert_i64_u`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f32_convert_i64_u(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let a = ctx.stack.pop_u64();
    vm_try!(ctx.stack.push_f32(a as f32));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `f32.demote_f64`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f32_demote_f64(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let a = ctx.stack.pop_f64();
    vm_try!(ctx.stack.push_f32(a as f32));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `f64.convert_i32_s`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f64_convert_i32_s(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let a = ctx.stack.pop_i32();
    vm_try!(ctx.stack.push_f64(a as f64));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `f64.convert_i32_u`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f64_convert_i32_u(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let a = ctx.stack.pop_u32();
    vm_try!(ctx.stack.push_f64(a as f64));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `f64.convert_i64_s`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f64_convert_i64_s(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let a = ctx.stack.pop_i64();
    vm_try!(ctx.stack.push_f64(a as f64));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `f64.convert_i64_u`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f64_convert_i64_u(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let a = ctx.stack.pop_u64();
    vm_try!(ctx.stack.push_f64(a as f64));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `f64.promote_f32`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f64_promote_f32(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let a = ctx.stack.pop_f32();
    vm_try!(ctx.stack.push_f64(a.into()));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `f32.abs`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f32_abs(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let a = ctx.stack.pop_f32();
    vm_try!(ctx.stack.push_f32(a.abs()));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `f32.neg`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f32_neg(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let a = ctx.stack.pop_f32();
    vm_try!(ctx.stack.push_f32(-a));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `f32.ceil`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f32_ceil(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let a = ctx.stack.pop_f32();
    vm_try!(ctx.stack.push_f32(a.ceil()));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `f32.floor`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f32_floor(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let a = ctx.stack.pop_f32();
    vm_try!(ctx.stack.push_f32(a.floor()));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `f32.trunc`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f32_trunc(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let a = ctx.stack.pop_f32();
    vm_try!(ctx.stack.push_f32(a.trunc()));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `f32.nearest`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f32_nearest(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let a = ctx.stack.pop_f32();
    vm_try!(ctx.stack.push_f32(a.round_ties_even()));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `f64.ceil`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f64_ceil(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let a = ctx.stack.pop_f64();
    vm_try!(ctx.stack.push_f64(a.ceil()));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `f64.floor`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f64_floor(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let a = ctx.stack.pop_f64();
    vm_try!(ctx.stack.push_f64(a.floor()));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `f64.trunc`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f64_trunc(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let a = ctx.stack.pop_f64();
    vm_try!(ctx.stack.push_f64(a.trunc()));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `f64.nearest`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f64_nearest(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let a = ctx.stack.pop_f64();
    vm_try!(ctx.stack.push_f64(a.round_ties_even()));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `f64.abs`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f64_abs(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let a = ctx.stack.pop_f64();
    vm_try!(ctx.stack.push_f64(a.abs()));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `f64.neg`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f64_neg(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let a = ctx.stack.pop_f64();
    vm_try!(ctx.stack.push_f64(-a));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `f64.sqrt`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f64_sqrt(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let a = ctx.stack.pop_f64();
    vm_try!(ctx.stack.push_f64(a.sqrt()));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `f32.eq`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [i32]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f32_eq(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let a = ctx.stack.pop_f32();
    let b = ctx.stack.pop_f32();
    vm_try!(ctx.stack.push_u32(u32::from(a == b)));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `f32.ne`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [i32]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f32_ne(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let a = ctx.stack.pop_f32();
    let b = ctx.stack.pop_f32();
    vm_try!(ctx.stack.push_u32(u32::from(a != b)));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `f32.le`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [i32]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f32_le(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_f32();
    let a = ctx.stack.pop_f32();
    vm_try!(ctx.stack.push_u32(u32::from(a <= b)));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `f32.ge`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [i32]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f32_ge(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_f32();
    let a = ctx.stack.pop_f32();
    vm_try!(ctx.stack.push_u32(u32::from(a >= b)));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `f64.eq`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [i32]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f64_eq(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_f64();
    let a = ctx.stack.pop_f64();
    vm_try!(ctx.stack.push_u32(u32::from(a == b)));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `f64.ne`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [i32]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f64_ne(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_f64();
    let a = ctx.stack.pop_f64();
    vm_try!(ctx.stack.push_u32(u32::from(a != b)));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `f64.lt`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [i32]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f64_lt(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_f64();
    let a = ctx.stack.pop_f64();
    vm_try!(ctx.stack.push_u32(u32::from(a < b)));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `f64.gt`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [i32]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f64_gt(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_f64();
    let a = ctx.stack.pop_f64();
    vm_try!(ctx.stack.push_u32(u32::from(a > b)));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `f64.le`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [i32]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f64_le(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_f64();
    let a = ctx.stack.pop_f64();
    vm_try!(ctx.stack.push_u32(u32::from(a <= b)));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `f64.ge`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [i32]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f64_ge(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_f64();
    let a = ctx.stack.pop_f64();
    vm_try!(ctx.stack.push_u32(u32::from(a >= b)));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `i32.ctz`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_ctz(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let v = ctx.stack.pop_u32().trailing_zeros();
    vm_try!(ctx.stack.push_u32(v));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `i32.clz`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_clz(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let v = ctx.stack.pop_u32().leading_zeros();
    vm_try!(ctx.stack.push_u32(v));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `i32.popcnt`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_popcnt(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let v = ctx.stack.pop_u32().count_ones();
    vm_try!(ctx.stack.push_u32(v));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `i32.mul`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_mul(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let a = ctx.stack.pop_i32();
    let b = ctx.stack.pop_i32();
    let r = a.wrapping_mul(b);
    vm_try!(ctx.stack.push_i32(r));
    trace!("op_i32_mul: {a} {b} => {r}");
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `i32.div_s`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: traps on division by zero and signed overflow.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
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

/// WebAssembly `i32.div_u`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: traps on division by zero.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
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

/// WebAssembly `i64.div_s`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: traps on division by zero and signed overflow.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
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

/// WebAssembly `i64.div_u`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: traps on division by zero.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
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

/// WebAssembly `i64.and`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_and(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_u64();
    let a = ctx.stack.pop_u64();
    vm_try!(ctx.stack.push_u64(a & b));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `i64.or`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_or(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_u64();
    let a = ctx.stack.pop_u64();
    vm_try!(ctx.stack.push_u64(a | b));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `i64.xor`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_xor(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_u64();
    let a = ctx.stack.pop_u64();
    vm_try!(ctx.stack.push_u64(a.bitxor(b)));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `i64.mul`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_mul(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let a = ctx.stack.pop_i64();
    let b = ctx.stack.pop_i64();
    let r = a.wrapping_mul(b);
    vm_try!(ctx.stack.push_i64(r));
    trace!("op_i64_mul: {a} {b} => {r}");
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `i32.rem_s`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [result]`.
/// Traps: traps on division by zero and signed overflow.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_rem_s(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_i32();
    let a = ctx.stack.pop_i32();
    if b == 0 {
        return VMResult::InvalidOperand;
    }
    vm_try!(ctx.stack.push_i32(a.wrapping_rem(b)));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `i32.rem_u`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [result]`.
/// Traps: traps on division by zero.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_rem_u(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_u32();
    let a = ctx.stack.pop_u32();
    let r = vm_try!(VMResult::from_option(a.checked_rem(b), || {
        VMResult::InvalidOperand
    }));
    vm_try!(ctx.stack.push_u32(r));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `i64.rem_s`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [result]`.
/// Traps: traps on division by zero and signed overflow.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_rem_s(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_i64();
    let a = ctx.stack.pop_i64();
    if b == 0 {
        return VMResult::InvalidOperand;
    }
    vm_try!(ctx.stack.push_i64(a.wrapping_rem(b)));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `i64.rem_u`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [result]`.
/// Traps: traps on division by zero.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_rem_u(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_u64();
    let a = ctx.stack.pop_u64();
    let r = vm_try!(VMResult::from_option(a.checked_rem(b), || {
        VMResult::InvalidOperand
    }));
    vm_try!(ctx.stack.push_u64(r));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `i32.and`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_and(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_u32();
    let a = ctx.stack.pop_u32();
    vm_try!(ctx.stack.push_u32(a & b));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `i32.or`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_or(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_u32();
    let a = ctx.stack.pop_u32();
    vm_try!(ctx.stack.push_u32(a | b));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `i32.xor`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_xor(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_u32();
    let a = ctx.stack.pop_u32();
    vm_try!(ctx.stack.push_u32(a.bitxor(b)));
    call_next(tail_code, 0, ctx)
}

/// Telomere internal fused i32 bit-update step ending in a typed `select`.
///
/// This covers stack shapes such as
/// `i32.const 1; i32.shr_u; i32.const 32767; i32.and; local.tee; ...; select`
/// where the selected value is either the shifted state or that state XORed with
/// a polynomial/bitmask constant. It is intentionally expressed as a generic
/// select-step family rather than as a benchmark-specific CRC replacement.
///
/// Stack effect: `[state] -> [selected]`.
/// Traps: none.
///
/// # Safety
/// - `tail_code` must point to operands decoded for this fused bit-step handler.
/// - `ctx` must hold a valid frame and local base for the active module.
/// - The decoded operands must describe locals validated as 4-byte values.
/// - This handler must not keep borrows, locks, or guards alive across
///   `call_next`.
pub unsafe fn op_i32_select_bit_step4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    profile_numeric_family("op_i32_select_bit_step4");
    vm_try!(i32_select_bit_step4_at(tail_code, ctx));
    call_next(tail_code, 7, ctx)
}

/// Telomere internal fused run of consecutive i32 bit-update select steps.
///
/// Stack effect: `[state] -> [selected]`.
/// Traps: none.
///
/// # Safety
/// - `tail_code` must point to a run count followed by consecutive encoded
///   `op_i32_select_bit_step4` operand groups.
/// - `ctx` must hold a valid frame and local base for the active module.
/// - Each encoded step must satisfy `op_i32_select_bit_step4`'s safety
///   requirements.
/// - This handler must not keep borrows, locks, or guards alive across
///   `call_next`.
pub unsafe fn op_i32_select_bit_step4_run(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    profile_numeric_family("op_i32_select_bit_step4_run");
    let count = (*tail_code).operand.u32 as usize;
    let mut cursor = tail_code.add(1);
    for _ in 0..count {
        vm_try!(i32_select_bit_step4_at(cursor, ctx));
        cursor = cursor.add(7);
    }
    call_next(tail_code, (1 + count * 7) as isize, ctx)
}

#[inline(always)]
unsafe fn i32_select_bit_step4_at(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let tmp_local = (*tail_code).operand.local_addr as usize;
    let poly = (*tail_code.add(1)).operand.i32 as u32;
    let source_local = (*tail_code.add(2)).operand.local_addr as usize;
    let source_shift = (*tail_code.add(3)).operand.u32;
    let prev_local = (*tail_code.add(4)).operand.local_addr as usize;
    let flags = (*tail_code.add(5)).operand.u32;
    let dst_local = (*tail_code.add(6)).operand.local_addr as usize;

    let local_base = ctx.local_base_ptr as *const u8;
    let local_base_mut = ctx.local_base_ptr;
    let mut shifted = ctx.stack.pop_u32_fast().wrapping_shr(1);
    if flags & I32_SELECT_BIT_STEP_MASK_SHIFTED != 0 {
        shifted &= 0x7fff;
    }
    ctx.stack
        .local_set4_from_base_value(local_base_mut, tmp_local, shifted);

    let source = ctx
        .stack
        .local_u32_from_base(local_base, source_local)
        .wrapping_shr(source_shift);
    let prev = ctx.stack.local_u32_from_base(local_base, prev_local);
    let xored = shifted ^ poly;
    let selected = if flags & I32_SELECT_BIT_STEP_EQ_CONDITION != 0 {
        if (prev & 1) == source {
            shifted
        } else {
            xored
        }
    } else if ((source ^ prev) & 1) != 0 {
        xored
    } else {
        shifted
    };

    vm_try!(ctx.stack.push_u32_fast(selected));
    if flags & I32_SELECT_BIT_STEP_TEE_DST != 0 {
        ctx.stack
            .local_set4_from_base_value(local_base_mut, dst_local, selected);
    }
    VMResult::Success(())
}

/// WebAssembly `i32.shl`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_shl(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_i32();
    let a = ctx.stack.pop_i32();
    vm_try!(ctx.stack.push_i32(wasm_i32_shl(a, b)));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `i32.shr_s`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_shr_s(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_i32();
    let a = ctx.stack.pop_i32();
    vm_try!(ctx.stack.push_i32(wasm_i32_shr_s(a, b)));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `i32.shr_u`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_shr_u(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_u32();
    let a = ctx.stack.pop_u32();
    vm_try!(ctx.stack.push_u32(wasm_i32_shr_u(a, b)));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `i32.rotl`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_rotl(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_u32();
    let a = ctx.stack.pop_u32();
    vm_try!(ctx.stack.push_u32(a.rotate_left(b)));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `i32.rotr`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_rotr(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_u32();
    let a = ctx.stack.pop_u32();
    vm_try!(ctx.stack.push_u32(a.rotate_right(b)));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `i64.rotl`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_rotl(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_u64();
    let a = ctx.stack.pop_u64();
    vm_try!(ctx.stack.push_u64(a.rotate_left(b as u32)));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `i64.rotr`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_rotr(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_u64();
    let a = ctx.stack.pop_u64();
    vm_try!(ctx.stack.push_u64(a.rotate_right(b as u32)));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `i64.shl`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_shl(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_i64();
    let a = ctx.stack.pop_i64();
    vm_try!(ctx.stack.push_i64(wasm_i64_shl(a, b)));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `i64.shr_s`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_shr_s(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_i64();
    let a = ctx.stack.pop_i64();
    vm_try!(ctx.stack.push_i64(wasm_i64_shr_s(a, b)));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `i64.shr_u`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_shr_u(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_u64();
    let a = ctx.stack.pop_u64();
    vm_try!(ctx.stack.push_u64(wasm_i64_shr_u(a, b)));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `i32.eqz`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [i32]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_eqz(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let a = ctx.stack.pop_u32();
    vm_try!(ctx.stack.push_u32(u32::from(a == 0)));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `i64.eqz`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [i32]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_eqz(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let a = ctx.stack.pop_u64();
    let r = u32::from(a == 0);
    trace!("op_i64_eqz: {a} => {r}");
    vm_try!(ctx.stack.push_u32(r));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `i64.eq`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [i32]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_eq(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_u64();
    let a = ctx.stack.pop_u64();
    vm_try!(ctx.stack.push_u32(u32::from(a == b)));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `i64.ne`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [i32]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_ne(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_u64();
    let a = ctx.stack.pop_u64();
    vm_try!(ctx.stack.push_u32(u32::from(a != b)));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `i64.lt_s`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [i32]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_lt_s(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_i64();
    let a = ctx.stack.pop_i64();
    vm_try!(ctx.stack.push_u32(u32::from(a < b)));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `i64.lt_u`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [i32]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_lt_u(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_u64();
    let a = ctx.stack.pop_u64();
    vm_try!(ctx.stack.push_u32(u32::from(a < b)));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `i64.gt_s`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [i32]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_gt_s(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_i64();
    let a = ctx.stack.pop_i64();
    vm_try!(ctx.stack.push_u32(u32::from(a > b)));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `i64.gt_u`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [i32]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_gt_u(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_u64();
    let a = ctx.stack.pop_u64();
    vm_try!(ctx.stack.push_u32(u32::from(a > b)));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `i64.le_s`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [i32]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_le_s(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_i64();
    let a = ctx.stack.pop_i64();
    vm_try!(ctx.stack.push_u32(u32::from(a <= b)));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `i64.le_u`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [i32]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_le_u(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_u64();
    let a = ctx.stack.pop_u64();
    vm_try!(ctx.stack.push_u32(u32::from(a <= b)));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `i64.ge_s`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [i32]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_ge_s(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_i64();
    let a = ctx.stack.pop_i64();
    vm_try!(ctx.stack.push_u32(u32::from(a >= b)));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `i64.ge_u`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [i32]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_ge_u(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_u64();
    let a = ctx.stack.pop_u64();
    vm_try!(ctx.stack.push_u32(u32::from(a >= b)));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `i32.eq`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [i32]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_eq(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let a = ctx.stack.pop_u32();
    let b = ctx.stack.pop_u32();
    let r = u32::from(a == b);
    trace!("op_i32_eq: {a} {b} => {r}");
    vm_try!(ctx.stack.push_u32(r));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `i32.ne`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [i32]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_ne(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let a = ctx.stack.pop_u32();
    let b = ctx.stack.pop_u32();
    let r = u32::from(a != b);
    trace!("op_i32_ne: {a} {b} => {r}");
    vm_try!(ctx.stack.push_u32(r));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `i32.le_s`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [i32]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_le_s(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_i32();
    let a = ctx.stack.pop_i32();
    let r = u32::from(a <= b);
    trace!("op_i32_le_s: {a} {b} => {r}");
    vm_try!(ctx.stack.push_u32(r));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `i32.le_u`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [i32]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_le_u(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_u32();
    let a = ctx.stack.pop_u32();
    let r = u32::from(a <= b);
    trace!("op_i32_le_u: {a} {b} => {r}");
    vm_try!(ctx.stack.push_u32(r));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `i32.lt_s`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [i32]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_lt_s(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_i32();
    let a = ctx.stack.pop_i32();
    let r = u32::from(a < b);
    trace!("op_i32_lt_s: {a} {b} => {r}");
    vm_try!(ctx.stack.push_u32(r));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `i32.lt_u`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [i32]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_lt_u(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_u32();
    let a = ctx.stack.pop_u32();
    let r = u32::from(a < b);
    trace!("op_i32_lt_u: {a} {b} => {r}");
    vm_try!(ctx.stack.push_u32(r));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `i32.gt_s`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [i32]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_gt_s(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_i32();
    let a = ctx.stack.pop_i32();
    let r = u32::from(a > b);
    trace!("op_i32_gt_s: {a} {b} => {r}");
    vm_try!(ctx.stack.push_u32(r));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `i32.gt_u`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [i32]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_gt_u(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_u32();
    let a = ctx.stack.pop_u32();
    let r = u32::from(a > b);
    trace!("op_i32_gt_u: {a} {b} => {r}");
    vm_try!(ctx.stack.push_u32(r));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `i32.ge_s`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [i32]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_ge_s(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_i32();
    let a = ctx.stack.pop_i32();
    let r = u32::from(a >= b);
    trace!("op_i32_ge_s: {a} {b} => {r}");
    vm_try!(ctx.stack.push_u32(r));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `i32.ge_u`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [i32]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_ge_u(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_u32();
    let a = ctx.stack.pop_u32();
    let r = u32::from(a >= b);
    trace!("op_i32_ge_u: {a} {b} => {r}");
    vm_try!(ctx.stack.push_u32(r));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `i32.extend8_s`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_extend8_s(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let v = ctx.stack.pop_u32();
    let v = i8::from_le_bytes([v as u8]);
    vm_try!(ctx.stack.push_i32(v.into()));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `i32.extend16_s`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_extend16_s(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let v = ctx.stack.pop_u32();
    let v = i16::from_le_bytes([v as u8, (v >> 8) as u8]);
    vm_try!(ctx.stack.push_i32(v.into()));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `i64.extend8_s`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_extend8_s(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let v = ctx.stack.pop_u64();
    let v = i8::from_le_bytes([v as u8]);
    vm_try!(ctx.stack.push_i64(v.into()));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `i64.extend16_s`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_extend16_s(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let v = ctx.stack.pop_u64();
    let v = i16::from_le_bytes([v as u8, (v >> 8) as u8]);
    vm_try!(ctx.stack.push_i64(v.into()));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `i64.extend32_s`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_extend32_s(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let v = ctx.stack.pop_u64();
    let v = i32::from_le_bytes([v as u8, (v >> 8) as u8, (v >> 16) as u8, (v >> 24) as u8]);
    vm_try!(ctx.stack.push_i64(v.into()));
    call_next(tail_code, 0, ctx)
}
