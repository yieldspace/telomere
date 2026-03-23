use super::*;

// Required for direct function call threading.
// If unset, LLVM will not replace the end of op_call with a jump.
#[inline(never)]
/// WebAssembly call-dispatch helper for direct-threaded function invocation.
///
/// Spec:
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: internal runtime call dispatch.
/// Traps: propagates the trap behavior of the target instruction.
/// Notes: Resolves the callee, prepares the frame cache, and returns either a concrete instruction pointer or a pending async host call.
///
/// # Safety
/// - `return_addr` must remain valid for the duration of the helper and must point back into the active decoded instruction stream.
/// - `ctx` must reference a live execution context for the same store and validated frame layout.
/// - This helper must not keep borrows, locks, or guards alive across the tail-dispatch it initiates.
unsafe fn internal_op_call(
    return_addr: *const Instr,
    funcaddr: ObjectRef,
    ctx: &mut ExecuteContext,
    is_return_call: bool,
) -> VMResult<CallOutcome> {
    let funcinst = ctx.func_by_addr(funcaddr).clone();
    let instance = ctx.gc.instance(funcinst.instance);
    let memory0 = instance
        .memory_slots
        .first()
        .copied()
        .and_then(|slot| slot.handle());
    let frame = CallFrameCache::from_parts(funcaddr, &funcinst, memory0);
    trace!(
        "op_call_internal: {:?}({:?})  {funcaddr:?}",
        ctx.gc.object_ref_for_instance(funcinst.instance),
        instance.module_addr
    );
    let param_size = funcinst.execution.param_stack_bytes as usize;
    let is_host_func = funcinst.is_host_func();
    if is_host_func {
        if is_return_call {
            let local_reference = vm_try!(ctx.stack.function_return_call(
                &ctx.local_reference,
                param_size,
                funcinst.execution.param_shape,
                0,
                frame,
            ));
            ctx.set_local_reference(local_reference);
        } else {
            let local_reference = vm_try!(ctx.stack.function_call(
                param_size,
                0,
                frame,
                ctx.local_reference,
                return_addr,
                ctx.gc,
            ));
            ctx.set_local_reference(local_reference);
        }
        invoke_host_function(return_addr, ctx)
    } else {
        let wasm_metadata = funcinst
            .wasm_metadata()
            .expect("wasm function must expose execution metadata");
        let locals_size = wasm_metadata.locals_byte_size as usize;
        if is_return_call {
            let local_reference = vm_try!(ctx.stack.function_return_call(
                &ctx.local_reference,
                param_size,
                funcinst.execution.param_shape,
                locals_size,
                frame,
            ));
            ctx.set_local_reference(local_reference);
        } else {
            let local_reference = vm_try!(ctx.stack.function_call(
                param_size,
                locals_size,
                frame,
                ctx.local_reference,
                return_addr,
                ctx.gc,
            ));
            ctx.set_local_reference(local_reference);
        }

        let ptr = funcinst
            .canonical_code_pointer()
            .expect("wasm function must expose a code pointer");
        debug_assert!(!is_host_func);
        VMResult::Success(CallOutcome::Immediate(ptr))
    }
}

#[inline(always)]
unsafe fn direct_funcaddr_unchecked(ctx: &ExecuteContext, funcidx: u32) -> ObjectRef {
    *ctx.instance()
        .funcs
        .as_slice()
        .get_unchecked(funcidx as usize)
}

#[inline(always)]
unsafe fn op_direct_call(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    is_return_call: bool,
) -> VMResult<()> {
    if ctx.effect.get_pending_count() != 0 {
        trace!("waiting effect: {:?}", ctx.cont);
        return VMResult::Success(());
    }
    let funcidx = (*tail_code).operand.u32;
    let funcaddr = direct_funcaddr_unchecked(ctx, funcidx);
    match vm_try!(internal_op_call(
        tail_code.offset(1),
        funcaddr,
        ctx,
        is_return_call
    )) {
        CallOutcome::Immediate(ptr) => call_next(ptr, 0, ctx),
        CallOutcome::Pending => VMResult::Success(()),
    }
}

/// WebAssembly `call`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[params] -> [results]`.
/// Traps: traps if the target function cannot be invoked.
/// Notes: Transfers control in the direct-threaded interpreter and keeps tail-dispatch compatible with `call_code` or `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_call(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    op_direct_call(tail_code, ctx, false)
}

/// WebAssembly `call` for imported or otherwise generic direct callees.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[params] -> [results]`.
/// Traps: traps if the target function cannot be invoked.
/// Notes: The parser uses this slower path for imported direct call sites so local call hot paths
/// can stay on `op_call`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
pub unsafe fn op_call_import(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    op_direct_call(tail_code, ctx, false)
}

/// WebAssembly `return_call`.
///
/// Related spec:
/// - Tail-call: https://webassembly.github.io/tail-call/core/
///
/// Stack effect: `[params] -> [results]`.
/// Traps: traps if the target function cannot be invoked.
/// Notes: Transfers control in the direct-threaded interpreter and keeps tail-dispatch compatible with `call_code` or `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_return_call(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    op_direct_call(tail_code, ctx, true)
}

/// WebAssembly `return_call` for imported or otherwise generic direct callees.
///
/// Related spec:
/// - Tail-call: https://webassembly.github.io/tail-call/core/
///
/// Stack effect: `[params] -> [results]`.
/// Traps: traps if the target function cannot be invoked.
/// Notes: The parser uses this slower path for imported direct call sites so local call hot paths
/// can stay on `op_return_call`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
pub unsafe fn op_return_call_import(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_direct_call(tail_code, ctx, true)
}

#[inline(never)]
/// WebAssembly indirect call-dispatch helper for direct-threaded function invocation.
///
/// Spec:
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: internal runtime call dispatch.
/// Traps: traps on table index out of range, null function entries, or type mismatches.
/// Notes: Resolves the table entry, validates the callee type, and forwards to the direct call helper.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction stream for the current active frame.
/// - `ctx` must reference a live execution context for the same store and validated frame layout.
/// - This helper must not keep borrows, locks, or guards alive across the tail-dispatch it initiates.
unsafe fn internal_op_call_indirect(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    is_return_call: bool,
) -> VMResult<CallOutcome> {
    let i = ctx.stack.pop_u32();
    let tableidx = (*tail_code).operand.u32 as usize;
    let table_addr = *ctx.instance().tables.as_slice().get_unchecked(tableidx);
    let table = ctx.gc.get_table(table_addr);
    let func_addr = *vm_try!(VMResult::from_option(table.1.get(i as usize), || {
        VMResult::TableIndexOutOfRange
    }));
    trace!("internal_op_call_indirect: {tableidx} {table_addr:?} {func_addr} {table:?}");
    if func_addr == TABLE_UNINITIALIZED {
        return VMResult::TableUninitialized;
    }
    let func_addr = ObjectRef(func_addr);
    let funcinst = ctx.gc.get_func(func_addr);
    let expected_typeidx = (*tail_code.offset(1)).operand.u32;
    let expected_type_identity = ctx
        .module()
        .function_type_identities
        .get_unchecked(expected_typeidx as usize);
    trace!(
        "{:?} {:?}",
        funcinst.execution.type_identity,
        expected_type_identity
    );
    if &funcinst.execution.type_identity != expected_type_identity {
        return VMResult::CallIndirectInvalidType;
    }
    let outcome = vm_try!(internal_op_call(
        tail_code.offset(2),
        func_addr,
        ctx,
        is_return_call
    ));
    VMResult::Success(outcome)
}

/// WebAssembly `call_indirect`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[params, i32] -> [results]`.
/// Traps: traps on null or type-mismatched table entries.
/// Notes: Transfers control in the direct-threaded interpreter and keeps tail-dispatch compatible with `call_code` or `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_call_indirect(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    if ctx.effect.get_pending_count() != 0 {
        trace!("waiting effect: {:?}", ctx.cont);
        return VMResult::Success(());
    }
    match vm_try!(internal_op_call_indirect(tail_code, ctx, false)) {
        CallOutcome::Immediate(ptr) => call_next(ptr, 0, ctx),
        CallOutcome::Pending => VMResult::Success(()),
    }
}

/// WebAssembly `return_call_indirect`.
///
/// Related spec:
/// - Tail-call: https://webassembly.github.io/tail-call/core/
///
/// Stack effect: `[params, i32] -> [results]`.
/// Traps: traps on null or type-mismatched table entries.
/// Notes: Transfers control in the direct-threaded interpreter and keeps tail-dispatch compatible with `call_code` or `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_return_call_indirect(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    if ctx.effect.get_pending_count() != 0 {
        trace!("waiting effect: {:?}", ctx.cont);
        return VMResult::Success(());
    }
    match vm_try!(internal_op_call_indirect(tail_code, ctx, true)) {
        CallOutcome::Immediate(ptr) => call_next(ptr, 0, ctx),
        CallOutcome::Pending => VMResult::Success(()),
    }
}

/// Telomere internal `special_start_function_call` trampoline.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `internal runtime continuation`.
/// Traps: traps if the target function cannot be invoked.
/// Notes: Transfers control in the direct-threaded interpreter and keeps tail-dispatch compatible with `call_code` or `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn special_start_function_call(
    _tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    match vm_try!(invoke_host_function(&VM_END as *const Instr, ctx)) {
        CallOutcome::Immediate(ptr) => call_next(ptr, 0, ctx),
        CallOutcome::Pending => VMResult::Success(()),
    }
}
