use super::*;
use crate::common::Op;
use std::sync::OnceLock;

/// WebAssembly `drop`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> []`.
/// Traps: none.
/// Notes: Reads or writes validated locals/stack slots and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_drop(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let size = (*tail_code).operand.drop_size as usize;
    trace!("op_drop: {size}");

    ctx.stack.drop(size);
    call_next(tail_code, 1, ctx)
}

#[inline(never)]
/// WebAssembly `select` helper for validated stack values.
///
/// Spec:
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: internal `select` operand handling.
/// Traps: none.
/// Notes: Reads the validated operands and materializes the selected value before the tail-dispatch wrapper continues.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction stream for the current active frame.
/// - `ctx` must reference a live execution context whose validated operand stack matches this `select` instruction.
/// - This helper must not keep borrows or guards alive across the follow-up stack push.
unsafe fn internal_op_select(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let x = (*tail_code).operand.select as usize;
    let cond = ctx.stack.pop_u32();

    let a = ctx.stack.pop_u8_array_generic::<8>(x);
    let b = ctx.stack.pop_u8_array_generic::<8>(x);
    let value = if cond == 0 { a } else { b };
    trace!("op_select: {x} {cond} {a:?} {b:?} => {value:?}");
    vm_try!(ctx.stack.push_slice(&value[0..x]));
    VMResult::Success(())
}

#[inline(always)]
unsafe fn internal_op_select4(ctx: &mut ExecuteContext) -> VMResult<()> {
    let cond = ctx.stack.pop_u32_fast();
    let a = ctx.stack.pop_u32_fast();
    let b = ctx.stack.pop_u32_fast();
    vm_try!(ctx.stack.push_u32_fast(if cond == 0 { a } else { b }));
    VMResult::Success(())
}

#[inline(always)]
unsafe fn internal_op_select8(ctx: &mut ExecuteContext) -> VMResult<()> {
    let cond = ctx.stack.pop_u32_fast();
    let a = ctx.stack.pop_u64_fast();
    let b = ctx.stack.pop_u64_fast();
    vm_try!(ctx.stack.push_u64_fast(if cond == 0 { a } else { b }));
    VMResult::Success(())
}

#[inline(always)]
unsafe fn internal_op_select16(ctx: &mut ExecuteContext) -> VMResult<()> {
    let cond = ctx.stack.pop_u32_fast();
    let a = ctx.stack.pop_u128_fast();
    let b = ctx.stack.pop_u128_fast();
    vm_try!(ctx.stack.push_u128_fast(if cond == 0 { a } else { b }));
    VMResult::Success(())
}

/// WebAssembly `select`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs, i32] -> [value]`.
/// Traps: none.
/// Notes: Reads or writes validated locals/stack slots and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_select(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    dispatch_profile_count("op_select");
    vm_try!(internal_op_select(tail_code, ctx));
    call_next(tail_code, 1, ctx)
}

/// WebAssembly `select` typed fast path for 4-byte values.
///
/// Spec:
/// - Validation: result width must be 4 bytes.
///
/// Stack effect: `[lhs, rhs, i32] -> [value]`.
/// Traps: none.
/// Notes: Uses typed stack operations and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_select4(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    dispatch_profile_count("op_select4");
    vm_try!(internal_op_select4(ctx));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `select` typed fast path for 8-byte values.
///
/// Spec:
/// - Validation: result width must be 8 bytes.
///
/// Stack effect: `[lhs, rhs, i32] -> [value]`.
/// Traps: none.
/// Notes: Uses typed stack operations and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_select8(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    dispatch_profile_count("op_select8");
    vm_try!(internal_op_select8(ctx));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `select` typed fast path for 16-byte values.
///
/// Spec:
/// - Validation: result width must be 16 bytes.
///
/// Stack effect: `[lhs, rhs, i32] -> [value]`.
/// Traps: none.
/// Notes: Uses typed stack operations and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_select16(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    dispatch_profile_count("op_select16");
    vm_try!(internal_op_select16(ctx));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `local.get`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[] -> [local]`.
/// Traps: none.
/// Notes: Reads or writes validated locals/stack slots and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_local_get4(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let addr = (*tail_code).operand.local_addr as usize;
    vm_try!(ctx
        .stack
        .local_get4_from_base(ctx.local_base_ptr as *const u8, addr));
    trace!("op_local_get4: {addr}");
    call_next(tail_code, 1, ctx)
}

/// WebAssembly `local.get`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[] -> [local]`.
/// Traps: none.
/// Notes: Reads or writes validated locals/stack slots and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_local_get8(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let addr = (*tail_code).operand.local_addr as usize;
    vm_try!(ctx
        .stack
        .local_get8_from_base(ctx.local_base_ptr as *const u8, addr));
    trace!("op_local_get8: {addr}");
    call_next(tail_code, 1, ctx)
}

/// WebAssembly `local.get`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[] -> [local]`.
/// Traps: none.
/// Notes: Reads or writes validated locals/stack slots and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_local_get16(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let addr = (*tail_code).operand.local_addr as usize;
    vm_try!(ctx
        .stack
        .local_get16_from_base(ctx.local_base_ptr as *const u8, addr));
    trace!("op_local_get16: {addr}");
    call_next(tail_code, 1, ctx)
}

pub(crate) fn local_get_dispatch_op(size: u32) -> Op {
    static PROFILE_ENABLED: OnceLock<bool> = OnceLock::new();
    let profile_enabled = *PROFILE_ENABLED.get_or_init(|| {
        std::env::var("TELOMERE_VM_PROFILE")
            .ok()
            .is_some_and(|value| value != "0")
    });
    match (size, profile_enabled) {
        (4, true) => op_local_get4_profiled as Op,
        (8, true) => op_local_get8_profiled as Op,
        (16, true) => op_local_get16_profiled as Op,
        (4, false) => op_local_get4 as Op,
        (8, false) => op_local_get8 as Op,
        (16, false) => op_local_get16 as Op,
        (_, true) => op_local_get4_profiled as Op,
        (_, false) => op_local_get4 as Op,
    }
}

/// WebAssembly `local.get` profiled fast path for 4-byte values.
///
/// Telomere runtime helper: records dispatch profile counts, then forwards to `op_local_get4`.
///
/// # Safety
/// - `tail_code` must satisfy the same contract as [`op_local_get4`].
/// - `ctx` must satisfy the same contract as [`op_local_get4`].
pub unsafe fn op_local_get4_profiled(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    dispatch_profile_count("op_local_get4");
    op_local_get4(tail_code, ctx)
}

/// WebAssembly `local.get` profiled fast path for 8-byte values.
///
/// Telomere runtime helper: records dispatch profile counts, then forwards to `op_local_get8`.
///
/// # Safety
/// - `tail_code` must satisfy the same contract as [`op_local_get8`].
/// - `ctx` must satisfy the same contract as [`op_local_get8`].
pub unsafe fn op_local_get8_profiled(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    dispatch_profile_count("op_local_get8");
    op_local_get8(tail_code, ctx)
}

/// WebAssembly `local.get` profiled fast path for 16-byte values.
///
/// Telomere runtime helper: records dispatch profile counts, then forwards to `op_local_get16`.
///
/// # Safety
/// - `tail_code` must satisfy the same contract as [`op_local_get16`].
/// - `ctx` must satisfy the same contract as [`op_local_get16`].
pub unsafe fn op_local_get16_profiled(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    dispatch_profile_count("op_local_get16");
    op_local_get16(tail_code, ctx)
}

/// WebAssembly `local.set`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[local] -> []`.
/// Traps: none.
/// Notes: Reads or writes validated locals/stack slots and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_local_set4(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    dispatch_profile_count("op_local_set4");
    let addr = (*tail_code).operand.local_addr as usize;
    ctx.stack.local_set4_from_base(ctx.local_base_ptr, addr);
    call_next(tail_code, 1, ctx)
}

/// WebAssembly `local.set`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[local] -> []`.
/// Traps: none.
/// Notes: Reads or writes validated locals/stack slots and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_local_set8(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let addr = (*tail_code).operand.local_addr as usize;
    ctx.stack.local_set8_from_base(ctx.local_base_ptr, addr);

    call_next(tail_code, 1, ctx)
}

/// WebAssembly `local.set`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[local] -> []`.
/// Traps: none.
/// Notes: Reads or writes validated locals/stack slots and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_local_set16(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let addr = (*tail_code).operand.local_addr as usize;
    ctx.stack.local_set16_from_base(ctx.local_base_ptr, addr);

    call_next(tail_code, 1, ctx)
}

/// WebAssembly `local.tee`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[local] -> [local]`.
/// Traps: none.
/// Notes: Reads or writes validated locals/stack slots and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_local_tee4(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    dispatch_profile_count("op_local_tee4");
    let addr = (*tail_code).operand.local_addr as usize;
    ctx.stack.local_tee4_from_base(ctx.local_base_ptr, addr);
    call_next(tail_code, 1, ctx)
}

/// WebAssembly `local.tee`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[local] -> [local]`.
/// Traps: none.
/// Notes: Reads or writes validated locals/stack slots and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_local_tee8(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let addr = (*tail_code).operand.local_addr as usize;
    ctx.stack.local_tee8_from_base(ctx.local_base_ptr, addr);

    call_next(tail_code, 1, ctx)
}

/// WebAssembly `local.tee`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[local] -> [local]`.
/// Traps: none.
/// Notes: Reads or writes validated locals/stack slots and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_local_tee16(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let addr = (*tail_code).operand.local_addr as usize;
    ctx.stack.local_tee16_from_base(ctx.local_base_ptr, addr);

    call_next(tail_code, 1, ctx)
}
