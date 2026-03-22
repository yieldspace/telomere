use super::*;

#[inline(always)]
fn read_global_u32(bytes: &[u8]) -> u32 {
    debug_assert_eq!(bytes.len(), 4);
    u32::from_le_bytes(bytes.try_into().expect("validated 4-byte global"))
}

#[inline(always)]
fn read_global_u64(bytes: &[u8]) -> u64 {
    debug_assert_eq!(bytes.len(), 8);
    u64::from_le_bytes(bytes.try_into().expect("validated 8-byte global"))
}

/// WebAssembly `global.get`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[] -> [global]`.
/// Traps: none.
/// Notes: Accesses the instance global storage and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_global_get4(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let idx = (*tail_code).operand.u32 as usize;
    let addr = ctx.instance().globals.as_slice()[idx];
    vm_try!(ctx.stack.push_u32(read_global_u32(ctx.gc.get_global(addr))));
    call_next(tail_code, 1, ctx)
}

/// WebAssembly `global.get`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[] -> [global]`.
/// Traps: none.
/// Notes: Accesses the instance global storage and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_global_get8(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let idx = (*tail_code).operand.u32 as usize;
    let addr = ctx.instance().globals.as_slice()[idx];
    vm_try!(ctx.stack.push_u64(read_global_u64(ctx.gc.get_global(addr))));
    call_next(tail_code, 1, ctx)
}

/// WebAssembly `global.get`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[] -> [global]`.
/// Traps: none.
/// Notes: Accesses the instance global storage and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_global_get16(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let idx = (*tail_code).operand.u32 as usize;
    let addr = ctx.instance().globals.as_slice()[idx];
    vm_try!(ctx.stack.push_slice(ctx.gc.get_global(addr)));
    call_next(tail_code, 1, ctx)
}

/// WebAssembly `global.set`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[global] -> []`.
/// Traps: none.
/// Notes: Accesses the instance global storage and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_global_set4(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let idx = (*tail_code).operand.u32 as usize;
    let addr = ctx.instance().globals.as_slice()[idx];
    ctx.gc
        .get_global_mut(addr)
        .copy_from_slice(&ctx.stack.pop_u32_bytes());
    call_next(tail_code, 1, ctx)
}

/// WebAssembly `global.set`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[global] -> []`.
/// Traps: none.
/// Notes: Accesses the instance global storage and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_global_set8(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let idx = (*tail_code).operand.u32 as usize;
    let addr = ctx.instance().globals.as_slice()[idx];
    ctx.gc
        .get_global_mut(addr)
        .copy_from_slice(&ctx.stack.pop_u64_bytes());
    call_next(tail_code, 1, ctx)
}

/// WebAssembly `global.set`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[global] -> []`.
/// Traps: none.
/// Notes: Accesses the instance global storage and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_global_set16(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let idx = (*tail_code).operand.u32 as usize;
    let addr = ctx.instance().globals.as_slice()[idx];
    ctx.gc
        .get_global_mut(addr)
        .copy_from_slice(&ctx.stack.pop_u8_array::<16>());
    call_next(tail_code, 1, ctx)
}
