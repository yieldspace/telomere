use super::*;

macro_rules! replicated_br_if {
    ($name:ident) => {
        #[inline(never)]
        pub(crate) unsafe fn $name(
            tail_code: *const Instr,
            ctx: &mut ExecuteContext,
        ) -> VMResult<()> {
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
    };
}

macro_rules! replicated_br_if_ptr {
    ($name:ident) => {
        #[inline(never)]
        pub(crate) unsafe fn $name(
            tail_code: *const Instr,
            ctx: &mut ExecuteContext,
        ) -> VMResult<()> {
            let cond = ctx.stack.pop_u32();
            trace!("op_br_if_ptr: {cond}");

            let ptr = if cond != 0 {
                (*tail_code).operand.code_ptr as *const Instr
            } else {
                tail_code.offset(1)
            };
            call_next(ptr, 0, ctx)
        }
    };
}

macro_rules! define_loop_handler {
    ($name:ident, $method:ident $(, size = $size:ident)?) => {
        pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            trace!("op_loop: {}", (*tail_code).operand.jump_addr);
            let loop_param = (*tail_code).operand.loop_param;
            ctx.stack.$method(
                &ctx.local_reference(),
                loop_param.stack_top as usize
                $(, loop_param.$size() as usize)?,
            );
            call_next(tail_code, 1, ctx)
        }
    };
}

macro_rules! define_precomputed_loop_handler {
    ($name:ident, $method:ident $(, size = $size:ident)?) => {
        pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            let loop_param = (*tail_code).operand.precomputed_loop_param;
            ctx.stack.$method(
                &ctx.local_reference(),
                loop_param.dst_from_local_top as usize
                $(, loop_param.$size() as usize)?,
            );
            call_next(tail_code, 1, ctx)
        }
    };
}

macro_rules! define_special_block_return_handler {
    ($name:ident, $method:ident $(, size = $size:ident)?) => {
        pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            let block_return = &(*tail_code).operand.block_return;
            trace!(
                "block return: {:?} {:?} {:?}",
                ctx.local_reference(),
                block_return,
                ctx.stack
            );
            ctx.stack.$method(
                &ctx.local_reference(),
                block_return.stack_top as usize
                $(, block_return.$size() as usize)?,
            );
            trace!("stack: {:?}", ctx.stack);
            call_next(tail_code, 1, ctx)
        }
    };
}

macro_rules! define_precomputed_special_block_return_handler {
    ($name:ident, $method:ident $(, size = $size:ident)?) => {
        pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            let block_return = (*tail_code).operand.precomputed_block_return;
            ctx.stack.$method(
                &ctx.local_reference(),
                block_return.dst_from_local_top as usize
                $(, block_return.$size() as usize)?,
            );
            call_next(tail_code, 1, ctx)
        }
    };
}

macro_rules! define_special_function_return_handler {
    ($name:ident, $method:ident) => {
        pub unsafe fn $name(_tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            trace!("function return");
            let (prev_local_ref, tail_code) = ctx.stack.$method(&ctx.local_reference(), ctx.gc);
            ctx.set_local_reference(prev_local_ref);
            call_next(tail_code, 0, ctx)
        }
    };
    ($name:ident, $method:ident, size = $size:ident) => {
        pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            trace!("function return");
            let (prev_local_ref, tail_code) = ctx.stack.$method(
                &ctx.local_reference(),
                (*tail_code).operand.$size as usize,
                ctx.gc,
            );
            ctx.set_local_reference(prev_local_ref);
            call_next(tail_code, 0, ctx)
        }
    };
}

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

/// Telomere runtime helper `op_br_ptr`.
///
/// Stack effect: `[] -> []`.
/// # Safety
/// - `tail_code` must point at the pointer-bearing branch operand in the active instruction stream.
/// - `ctx` must reference a live execution context for the same validated frame and store.
pub unsafe fn op_br_ptr(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    call_next((*tail_code).operand.code_ptr as *const Instr, 0, ctx)
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

/// Telomere runtime helper `op_else_ptr`.
///
/// Stack effect: `[] -> []`.
/// # Safety
/// - `tail_code` must point at the pointer-bearing else operand in the active instruction stream.
/// - `ctx` must reference a live execution context for the same validated frame and store.
pub unsafe fn op_else_ptr(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    call_next((*tail_code).operand.code_ptr as *const Instr, 1, ctx)
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

/// Telomere runtime helper `op_br_if_ptr`.
///
/// Stack effect: `[..., i32] -> [...]`.
/// # Safety
/// - `tail_code` must point at the pointer-bearing branch operand in the active instruction stream.
/// - `ctx` must reference a live execution context for the same validated frame and store.
pub unsafe fn op_br_if_ptr(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let cond = ctx.stack.pop_u32();
    trace!("op_br_if_ptr: {cond}");

    let ptr = if cond != 0 {
        (*tail_code).operand.code_ptr as *const Instr
    } else {
        tail_code.offset(1)
    };
    call_next(ptr, 0, ctx)
}

replicated_br_if!(op_br_if_r0);
replicated_br_if!(op_br_if_r1);
replicated_br_if!(op_br_if_r2);
replicated_br_if!(op_br_if_r3);
replicated_br_if_ptr!(op_br_if_ptr_r0);
replicated_br_if_ptr!(op_br_if_ptr_r1);
replicated_br_if_ptr!(op_br_if_ptr_r2);
replicated_br_if_ptr!(op_br_if_ptr_r3);

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

/// Telomere runtime helper `op_br_table_ptr`.
///
/// Stack effect: `[..., i32] -> [...]`.
/// # Safety
/// - `tail_code` must point at the pointer-bearing branch table payload in the active instruction stream.
/// - `ctx` must reference a live execution context for the same validated frame and store.
pub unsafe fn op_br_table_ptr(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let index = ctx.stack.pop_u32();
    let table_size = (*tail_code).operand.u32;
    let ptr = if index < table_size {
        (*tail_code.offset((index + 1) as isize)).operand.code_ptr as *const Instr
    } else {
        (*tail_code.offset((table_size + 1) as isize))
            .operand
            .code_ptr as *const Instr
    };
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
#[allow(dead_code)]
pub unsafe fn op_loop(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    trace!("op_loop: {}", (*tail_code).operand.jump_addr);

    let loop_param = (*tail_code).operand.loop_param;
    ctx.stack.block_return_shaped(
        &ctx.local_reference(),
        loop_param.stack_top as usize,
        loop_param.param_size() as usize,
        loop_param.param_shape(),
    );

    call_next(tail_code, 1, ctx)
}

define_loop_handler!(op_loop_empty, block_return_empty);
define_loop_handler!(op_loop4, block_return4);
define_loop_handler!(op_loop8, block_return8);
define_loop_handler!(op_loop_generic, block_return_generic, size = param_size);
define_precomputed_loop_handler!(op_loop_empty_precomputed, block_return_empty_precomputed);
define_precomputed_loop_handler!(op_loop4_precomputed, block_return4_precomputed);
define_precomputed_loop_handler!(op_loop8_precomputed, block_return8_precomputed);
define_precomputed_loop_handler!(
    op_loop_generic_precomputed,
    block_return_generic_precomputed,
    size = param_size
);

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

/// Telomere runtime helper `op_if_ptr`.
///
/// Stack effect: `[i32] -> []`.
/// # Safety
/// - `tail_code` must point at the pointer-bearing if operand in the active instruction stream.
/// - `ctx` must reference a live execution context for the same validated frame and store.
pub unsafe fn op_if_ptr(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let value = ctx.stack.pop_u32();
    let ptr = if value != 0 {
        tail_code.offset(1)
    } else {
        (*tail_code).operand.code_ptr as *const Instr
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
    let (prev_local_ref, tail_code) = ctx.stack.function_return(
        &ctx.local_reference(),
        (*tail_code).operand.drop_size as usize,
        ctx.gc,
    );
    ctx.set_local_reference(prev_local_ref);
    call_next(tail_code, 0, ctx)
}

define_special_function_return_handler!(special_function_return_empty, function_return_empty);
define_special_function_return_handler!(special_function_return4, function_return4);
define_special_function_return_handler!(special_function_return8, function_return8);
define_special_function_return_handler!(
    special_function_return_generic,
    function_return,
    size = drop_size
);

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
#[allow(dead_code)]
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
    ctx.stack.block_return_shaped(
        &ctx.local_reference(),
        block_return.stack_top as usize,
        block_return.return_size() as usize,
        block_return.return_shape(),
    );
    trace!("stack: {:?}", ctx.stack);

    call_next(tail_code, 1, ctx)
}

define_special_block_return_handler!(special_block_return_empty, block_return_empty);
define_special_block_return_handler!(special_block_return4, block_return4);
define_special_block_return_handler!(special_block_return8, block_return8);
define_special_block_return_handler!(
    special_block_return_generic,
    block_return_generic,
    size = return_size
);
define_precomputed_special_block_return_handler!(
    special_block_return_empty_precomputed,
    block_return_empty_precomputed
);
define_precomputed_special_block_return_handler!(
    special_block_return4_precomputed,
    block_return4_precomputed
);
define_precomputed_special_block_return_handler!(
    special_block_return8_precomputed,
    block_return8_precomputed
);
define_precomputed_special_block_return_handler!(
    special_block_return_generic_precomputed,
    block_return_generic_precomputed,
    size = return_size
);

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
pub unsafe fn op_unreachable(_tail_code: *const Instr, _ctx: &mut ExecuteContext) -> VMResult<()> {
    VMResult::Unreachable
}
