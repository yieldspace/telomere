use super::*;

#[cold]
fn ensure_call_recipe(funcaddr: ObjectRef, ctx: &mut ExecuteContext) -> CallDispatchCache {
    ctx.gc.ensure_call_recipe_for_func(funcaddr)
}

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
    recipe: CallDispatchCache,
    ctx: &mut ExecuteContext,
    is_return_call: bool,
) -> VMResult<CallOutcome> {
    dispatch_profile_count(if is_return_call {
        "op_return_call"
    } else {
        "op_call"
    });
    trace!(
        "op_call_internal: {:?}({:?})  {:?}",
        ctx.gc.object_ref_for_instance(recipe.frame.instance),
        ctx.gc.instance(recipe.frame.instance).module_addr,
        recipe.frame.code_addr
    );
    let param_size = recipe.param_size as usize;
    let return_pc = cached_return_pc(return_addr, ctx);
    match recipe.target {
        CallDispatchTarget::Host(fp) => {
            if is_return_call {
                let local_reference = vm_try!(ctx.stack.function_return_call_cached(
                    &ctx.local_reference,
                    param_size,
                    0,
                    recipe.frame,
                ));
                ctx.set_local_reference_with_frame(local_reference, recipe.frame);
            } else {
                let local_reference = vm_try!(ctx.stack.function_call_cached(
                    param_size,
                    0,
                    recipe.frame,
                    ctx.local_reference,
                    return_pc,
                ));
                ctx.set_local_reference_with_frame(local_reference, recipe.frame);
            }
            invoke_sync_host_function_with(return_addr, ctx, fp)
        }
        CallDispatchTarget::AsyncHost(fp) => {
            if is_return_call {
                let local_reference = vm_try!(ctx.stack.function_return_call_cached(
                    &ctx.local_reference,
                    param_size,
                    0,
                    recipe.frame,
                ));
                ctx.set_local_reference_with_frame(local_reference, recipe.frame);
            } else {
                let local_reference = vm_try!(ctx.stack.function_call_cached(
                    param_size,
                    0,
                    recipe.frame,
                    ctx.local_reference,
                    return_pc,
                ));
                ctx.set_local_reference_with_frame(local_reference, recipe.frame);
            }
            start_async_host_call_with(return_addr, ctx, fp)
        }
        CallDispatchTarget::Wasm { local_size } => {
            let local_size = local_size as usize;
            if is_return_call {
                let local_reference = vm_try!(ctx.stack.function_return_call_cached(
                    &ctx.local_reference,
                    param_size,
                    local_size,
                    recipe.frame,
                ));
                ctx.set_local_reference_with_frame(local_reference, recipe.frame);
            } else {
                let local_reference = vm_try!(ctx.stack.function_call_cached(
                    param_size,
                    local_size,
                    recipe.frame,
                    ctx.local_reference,
                    return_pc,
                ));
                ctx.set_local_reference_with_frame(local_reference, recipe.frame);
            }
            VMResult::Success(CallOutcome::Immediate(recipe.frame.code_base))
        }
    }
}

#[inline(always)]
fn cached_return_pc(return_addr: *const Instr, ctx: &ExecuteContext) -> StablePc {
    let code_base = ctx.code();
    if code_base.is_null() {
        return StablePc::from_stable_ptr(return_addr);
    }
    let instr_size = std::mem::size_of::<Instr>();
    let delta = (return_addr as usize).wrapping_sub(code_base as usize);
    debug_assert_eq!(delta % instr_size, 0);
    StablePc::from_relative_index(delta / instr_size)
}

#[inline(always)]
unsafe fn decode_direct_call_recipe(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> CallDispatchCache {
    let recipe_ref = (*tail_code).operand.call_recipe_ref;
    if let Some(recipe_slot) = recipe_ref.resolved_recipe_slot() {
        if let Some(recipe) = ctx.gc.call_recipe(recipe_slot) {
            return recipe;
        }
    }
    let funcaddr = ctx.instance().funcs.as_slice()[recipe_ref.funcidx as usize];
    ensure_call_recipe(funcaddr, ctx)
}

#[inline(always)]
unsafe fn ensure_indirect_call_recipe(
    funcaddr: ObjectRef,
    ctx: &mut ExecuteContext,
) -> CallDispatchCache {
    if let Some(recipe) = ctx
        .gc
        .call_recipe(ctx.gc.call_recipe_slot_for_func(funcaddr))
    {
        recipe
    } else {
        ensure_call_recipe(funcaddr, ctx)
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
    if ctx.effect.get_pending_count() != 0 {
        trace!("waiting effect: {:?}", ctx.cont);
        return VMResult::Success(());
    }
    let recipe = decode_direct_call_recipe(tail_code, ctx);
    match vm_try!(internal_op_call(tail_code.offset(1), recipe, ctx, false)) {
        CallOutcome::Immediate(ptr) => call_next(ptr, 0, ctx),
        CallOutcome::Pending => VMResult::Success(()),
    }
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
    std::hint::spin_loop();
    if ctx.effect.get_pending_count() != 0 {
        trace!("waiting effect: {:?}", ctx.cont);
        return VMResult::Success(());
    }
    let recipe = decode_direct_call_recipe(tail_code, ctx);
    match vm_try!(internal_op_call(tail_code.offset(1), recipe, ctx, false)) {
        CallOutcome::Immediate(ptr) => call_next(ptr, 0, ctx),
        CallOutcome::Pending => VMResult::Success(()),
    }
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
    if ctx.effect.get_pending_count() != 0 {
        trace!("waiting effect: {:?}", ctx.cont);
        return VMResult::Success(());
    }
    let recipe = decode_direct_call_recipe(tail_code, ctx);
    match vm_try!(internal_op_call(tail_code.offset(1), recipe, ctx, true)) {
        CallOutcome::Immediate(ptr) => call_next(ptr, 0, ctx),
        CallOutcome::Pending => VMResult::Success(()),
    }
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
    std::hint::spin_loop();
    if ctx.effect.get_pending_count() != 0 {
        trace!("waiting effect: {:?}", ctx.cont);
        return VMResult::Success(());
    }
    let recipe = decode_direct_call_recipe(tail_code, ctx);
    match vm_try!(internal_op_call(tail_code.offset(1), recipe, ctx, true)) {
        CallOutcome::Immediate(ptr) => call_next(ptr, 0, ctx),
        CallOutcome::Pending => VMResult::Success(()),
    }
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
    dispatch_profile_count(if is_return_call {
        "op_return_call_indirect"
    } else {
        "op_call_indirect"
    });
    let i = ctx.stack.pop_u32();
    let tableidx = (*tail_code).operand.u32 as usize;
    let table_addr = *vm_try!(VMResult::from_option(
        ctx.instance().tables.as_slice().get(tableidx),
        || { VMResult::TableIndexOutOfRange }
    ));
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
    let instance = ctx.gc.instance(funcinst.instance);
    let module = ctx.gc.get_module(instance.module_addr);
    let actual_typeidx = module.functions.get(funcinst.funcidx as usize).unwrap();
    let actual_ft = &module.function_types[actual_typeidx.0 as usize];
    let expected_typeidx = (*tail_code.offset(1)).operand.u32;
    let expected_ft = ctx
        .module()
        .function_types
        .get(expected_typeidx as usize)
        .unwrap();
    trace!("{:?} {:?}", actual_ft, expected_ft);
    if actual_ft != expected_ft {
        return VMResult::CallIndirectInvalidType;
    }
    let recipe = ensure_indirect_call_recipe(func_addr, ctx);
    let outcome = vm_try!(internal_op_call(
        tail_code.offset(2),
        recipe,
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
