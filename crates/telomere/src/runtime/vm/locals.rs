use super::*;

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
    let cond = ctx.stack.pop_u32();
    let a = ctx.stack.pop_u8_array::<4>();
    let b = ctx.stack.pop_u8_array::<4>();
    let value = if cond == 0 { a } else { b };
    vm_try!(ctx.stack.push_slice(&value));
    VMResult::Success(())
}

#[inline(always)]
unsafe fn internal_op_select8(ctx: &mut ExecuteContext) -> VMResult<()> {
    let cond = ctx.stack.pop_u32();
    let a = ctx.stack.pop_u8_array::<8>();
    let b = ctx.stack.pop_u8_array::<8>();
    let value = if cond == 0 { a } else { b };
    vm_try!(ctx.stack.push_slice(&value));
    VMResult::Success(())
}

#[inline(always)]
unsafe fn internal_op_select16(ctx: &mut ExecuteContext) -> VMResult<()> {
    let cond = ctx.stack.pop_u32();
    let a = ctx.stack.pop_u8_array::<16>();
    let b = ctx.stack.pop_u8_array::<16>();
    let value = if cond == 0 { a } else { b };
    vm_try!(ctx.stack.push_slice(&value));
    VMResult::Success(())
}

#[cfg(feature = "vm-profile")]
#[cold]
#[inline(never)]
fn profile_local_get(label: &'static str) {
    dispatch_profile_count(label);
}

#[inline(always)]
fn maybe_profile_local_get(_label: &'static str) {
    #[cfg(feature = "vm-profile")]
    if dispatch_profile_enabled() {
        profile_local_get(_label);
    }
}

#[inline(always)]
unsafe fn op_local_get4_impl(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let addr = (*tail_code).operand.local_addr as usize;
    vm_try!(ctx
        .stack
        .local_get4_from_base(ctx.local_base_ptr as *const u8, addr));
    trace!("op_local_get4: {addr}");
    call_next(tail_code, 1, ctx)
}

#[inline(always)]
unsafe fn op_local_get8_impl(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let addr = (*tail_code).operand.local_addr as usize;
    vm_try!(ctx
        .stack
        .local_get8_from_base(ctx.local_base_ptr as *const u8, addr));
    trace!("op_local_get8: {addr}");
    call_next(tail_code, 1, ctx)
}

#[inline(always)]
unsafe fn op_local_get16_impl(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let addr = (*tail_code).operand.local_addr as usize;
    vm_try!(ctx
        .stack
        .local_get16_from_base(ctx.local_base_ptr as *const u8, addr));
    trace!("op_local_get16: {addr}");
    call_next(tail_code, 1, ctx)
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
#[inline(never)]
#[unsafe(no_mangle)]
pub unsafe fn op_select4(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    vm_try!(internal_op_select4(ctx));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `select` followed by `local.set` for 4-byte values.
///
/// Spec:
/// - Validation: result width must be 4 bytes and the following local must be 4 bytes wide.
///
/// Stack effect: `[lhs, rhs, i32] -> []`.
/// Traps: none.
/// Notes: Fuses the typed select and local write without materializing an intermediate stack value.
///
/// # Safety
/// - `tail_code` must point to the local address operand for this fused handler.
/// - `ctx` must reference a live execution context whose validated operand stack and local layout match this fused instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next`.
pub unsafe fn op_select4_set4(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let dst = (*tail_code).operand.local_addr as usize;
    let cond = ctx.stack.pop_u32_fast();
    let rhs = ctx.stack.pop_u32_fast();
    let lhs = ctx.stack.pop_u32_fast();
    let value = if cond == 0 { rhs } else { lhs };
    ctx.stack
        .local_set4_from_base_value(ctx.local_base_ptr, dst, value);
    call_next(tail_code, 1, ctx)
}

/// WebAssembly `select` followed by `local.tee` for 4-byte values.
///
/// Spec:
/// - Validation: result width must be 4 bytes and the following local must be 4 bytes wide.
///
/// Stack effect: `[lhs, rhs, i32] -> [value]`.
/// Traps: none.
/// Notes: Fuses the typed select and local tee using typed stack operations.
///
/// # Safety
/// - `tail_code` must point to the local address operand for this fused handler.
/// - `ctx` must reference a live execution context whose validated operand stack and local layout match this fused instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next`.
pub unsafe fn op_select4_tee4(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let dst = (*tail_code).operand.local_addr as usize;
    let cond = ctx.stack.pop_u32_fast();
    let rhs = ctx.stack.pop_u32_fast();
    let lhs = ctx.stack.pop_u32_fast();
    let value = if cond == 0 { rhs } else { lhs };
    vm_try!(ctx.stack.push_u32_fast(value));
    ctx.stack
        .local_set4_from_base_value(ctx.local_base_ptr, dst, value);
    call_next(tail_code, 1, ctx)
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
#[inline(never)]
#[unsafe(no_mangle)]
pub unsafe fn op_select8(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
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
#[inline(never)]
#[unsafe(no_mangle)]
pub unsafe fn op_select16(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
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
    maybe_profile_local_get("op_local_get4");
    op_local_get4_impl(tail_code, ctx)
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
    maybe_profile_local_get("op_local_get8");
    op_local_get8_impl(tail_code, ctx)
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
    maybe_profile_local_get("op_local_get16");
    op_local_get16_impl(tail_code, ctx)
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
