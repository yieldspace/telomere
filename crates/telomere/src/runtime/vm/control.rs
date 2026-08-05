use super::*;
use crate::common::Op;

/// WebAssembly `return`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[results] -> []`.
/// Traps: unreachable always traps; other handlers follow validated control-flow invariants.
/// Notes: Computes the next continuation in the direct-threaded interpreter and tail-dispatches without keeping temporary guards alive.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_return(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let addr = (*tail_code).operand.jump_addr;
    trace!("op_return: {addr}");
    let code = ctx.code();
    let tail_code = code.offset(addr as isize);
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `end`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[] -> []`.
/// Traps: unreachable always traps; other handlers follow validated control-flow invariants.
/// Notes: Computes the next continuation in the direct-threaded interpreter and tail-dispatches without keeping temporary guards alive.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_end(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    trace!("op_end");
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `br`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[] -> []`.
/// Traps: unreachable always traps; other handlers follow validated control-flow invariants.
/// Notes: Computes the next continuation in the direct-threaded interpreter and tail-dispatches without keeping temporary guards alive.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_br(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let addr = (*tail_code).operand.jump_addr;
    trace!("op_br: {addr}");

    let tail_code = ctx.code().offset(addr as isize);
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `else`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[] -> []`.
/// Traps: unreachable always traps; other handlers follow validated control-flow invariants.
/// Notes: Computes the next continuation in the direct-threaded interpreter and tail-dispatches without keeping temporary guards alive.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_else(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    trace!("op_else");

    let addr = (*tail_code).operand.jump_addr;
    let tail_code = ctx.code().offset(addr as isize);
    call_next(tail_code, 1, ctx)
}

/// WebAssembly `br_if`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[..., i32] -> [...]`.
/// Traps: unreachable always traps; other handlers follow validated control-flow invariants.
/// Notes: Computes the next continuation in the direct-threaded interpreter and tail-dispatches without keeping temporary guards alive.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_br_if(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let cond = ctx.stack.pop_u32_fast();
    trace!("op_br_if: {cond}");

    let ptr = if cond != 0 {
        let addr = (*tail_code).operand.jump_addr;
        ctx.code().offset(addr as isize)
    } else {
        tail_code.offset(1)
    };
    call_next(ptr, 0, ctx)
}

/// WebAssembly `br_table`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[..., i32] -> [...]`.
/// Traps: unreachable always traps; other handlers follow validated control-flow invariants.
/// Notes: Computes the next continuation in the direct-threaded interpreter and tail-dispatches without keeping temporary guards alive.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
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

#[inline(always)]
unsafe fn br_table_target_ptr(
    tail_code: *const Instr,
    table_size_offset: isize,
    first_target_offset: isize,
    index: u32,
    ctx: &mut ExecuteContext,
) -> *const Instr {
    let table_size = (*tail_code.offset(table_size_offset)).operand.u32;
    let target_offset = if index < table_size {
        first_target_offset + index as isize
    } else {
        first_target_offset + table_size as isize
    };
    let addr = (*tail_code.offset(target_offset)).operand.jump_addr;
    unsafe { skip_end_ops(ctx.code().offset(addr as isize)) }
}

#[inline(always)]
unsafe fn skip_end_ops(mut ptr: *const Instr) -> *const Instr {
    while std::ptr::fn_addr_eq((*ptr).op, op_end as Op) {
        ptr = ptr.add(1);
    }
    ptr
}

#[allow(dead_code)]
/// WebAssembly `local.get; br_table` fusion for 4-byte locals.
///
/// # Safety
/// - `tail_code` must point to a local address followed by a materialized br_table operand group.
/// - The branch targets must be valid for the current active function body.
pub unsafe fn op_local_get4_br_table(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    dispatch_profile_count("op_local_get4_br_table");
    let local = (*tail_code).operand.local_addr as usize;
    let index = ctx
        .stack
        .local_u32_from_base(ctx.local_base_ptr as *const u8, local);
    let ptr = unsafe { br_table_target_ptr(tail_code, 1, 2, index, ctx) };
    call_next(ptr, 0, ctx)
}

#[allow(dead_code)]
/// WebAssembly `local.get; i32.const; i32.add; br_table` fusion for 4-byte locals.
///
/// # Safety
/// - `tail_code` must point to local/constant operands followed by a materialized br_table operand group.
/// - The branch targets must be valid for the current active function body.
pub unsafe fn op_local_get4_i32_const_add_br_table(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    dispatch_profile_count("op_local_get4_i32_const_add_br_table");
    let local = (*tail_code).operand.local_addr as usize;
    let rhs = (*tail_code.add(1)).operand.i32 as u32;
    let index = ctx
        .stack
        .local_u32_from_base(ctx.local_base_ptr as *const u8, local)
        .wrapping_add(rhs);
    let ptr = unsafe { br_table_target_ptr(tail_code, 2, 3, index, ctx) };
    call_next(ptr, 0, ctx)
}

/// WebAssembly `loop`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[params] -> [params]`.
/// Traps: unreachable always traps; other handlers follow validated control-flow invariants.
/// Notes: Computes the next continuation in the direct-threaded interpreter and tail-dispatches without keeping temporary guards alive.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_loop(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    vm_checkpoint!(ctx);
    trace!("op_loop: {}", (*tail_code).operand.jump_addr);

    let loop_param = (*tail_code).operand.loop_param;
    ctx.stack.block_return(
        &ctx.local_reference(),
        loop_param.stack_top as usize,
        loop_param.param_size as usize,
    );

    call_next(tail_code, 1, ctx)
}

/// WebAssembly `if`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[i32] -> []`.
/// Traps: unreachable always traps; other handlers follow validated control-flow invariants.
/// Notes: Computes the next continuation in the direct-threaded interpreter and tail-dispatches without keeping temporary guards alive.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_if(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let else_addr = (*tail_code).operand.jump_addr;
    let value = ctx.stack.pop_u32();
    trace!("op_if: {else_addr} {value}");

    let ptr = if value != 0 {
        tail_code.offset(1)
    } else {
        ctx.code().offset(else_addr as isize)
    };
    call_next(ptr, 0, ctx)
}

/// Telomere internal `special_function_return` trampoline.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `internal runtime continuation`.
/// Traps: unreachable always traps; other handlers follow validated control-flow invariants.
/// Notes: Computes the next continuation in the direct-threaded interpreter and tail-dispatches without keeping temporary guards alive.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn special_function_return(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    trace!("function return");
    let (prev_local_ref, tail_code) = ctx.stack.function_return_optional_continuation(
        &ctx.local_reference(),
        (*tail_code).operand.drop_size as usize,
        ctx.gc,
    );
    ctx.set_local_reference(prev_local_ref);
    match tail_code {
        Some(tail_code) => {
            #[cfg(feature = "jit")]
            if crate::runtime::jit::interpreter_stop_active() {
                ctx.cont = tail_code;
                return VMResult::Success(());
            }
            #[cfg(feature = "jit")]
            if crate::runtime::jit::should_stop_interpreter_at(tail_code) {
                ctx.cont = tail_code;
                return VMResult::Success(());
            }
            call_next(tail_code, 0, ctx)
        }
        None => {
            ctx.cont = std::ptr::null();
            VMResult::Success(())
        }
    }
}

/// Telomere internal `special_block_return` trampoline.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `internal runtime continuation`.
/// Traps: unreachable always traps; other handlers follow validated control-flow invariants.
/// Notes: Computes the next continuation in the direct-threaded interpreter and tail-dispatches without keeping temporary guards alive.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
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

/// Telomere internal `special_function_vm_end` trampoline.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `internal runtime continuation`.
/// Traps: unreachable always traps; other handlers follow validated control-flow invariants.
/// Notes: Computes the next continuation in the direct-threaded interpreter and tail-dispatches without keeping temporary guards alive.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn special_function_vm_end(
    _tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    if ctx.effect.get_pending_count() != 0 {
        trace!("waiting effect: {:?}", ctx.cont);
        return VMResult::Success(());
    }
    ctx.cont = std::ptr::null();
    VMResult::Success(())
}

/// WebAssembly `unreachable`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[] -> trap`.
/// Traps: unreachable always traps; other handlers follow validated control-flow invariants.
/// Notes: Computes the next continuation in the direct-threaded interpreter and tail-dispatches without keeping temporary guards alive.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
// #240 instance fix: In the measured Rust 1.96.0 release/aarch64-darwin build,
// this one-line constant return was duplicated across CGUs, breaking `fn_addr_eq`
// identity. That observed compiler behavior is not a Rust language guarantee.
#[inline(never)]
pub unsafe fn op_unreachable(_tail_code: *const Instr, _ctx: &mut ExecuteContext) -> VMResult<()> {
    VMResult::Unreachable
}
