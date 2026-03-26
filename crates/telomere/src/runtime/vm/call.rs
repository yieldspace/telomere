use super::*;

#[cold]
fn build_call_dispatch_cache(funcaddr: ObjectRef, ctx: &mut ExecuteContext) -> CallDispatchCache {
    let (instance, funcidx, target, code_base) = {
        let funcinst = ctx.gc.get_func(funcaddr);
        let target = match &funcinst.body {
            crate::common::store::FunctionBody::Wasm { locals, code } => (
                CallDispatchTarget::Wasm {
                    local_size: locals.byte_size() as u32,
                },
                code.as_ptr(),
            ),
            crate::common::store::FunctionBody::Host(fp) => {
                (CallDispatchTarget::Host(*fp), std::ptr::null())
            }
            crate::common::store::FunctionBody::AsyncHost(fp) => {
                (CallDispatchTarget::AsyncHost(*fp), std::ptr::null())
            }
        };
        (funcinst.instance, funcinst.funcidx, target.0, target.1)
    };
    let instance_data = ctx.gc.instance(instance);
    let memory0 = instance_data
        .memory_slots
        .first()
        .copied()
        .unwrap_or(crate::common::store::InstanceMemorySlot::None);
    let module = ctx.gc.get_module(instance_data.module_addr);
    let typeidx = module.functions[funcidx as usize];
    let param_size = result_type_size(&module.function_types[typeidx.0 as usize].0) as u32;
    CallDispatchCache {
        frame: CallFrameCache::from_cached_parts(funcaddr, instance, code_base, memory0.handle()),
        param_size,
        target,
    }
}

#[inline(always)]
fn ensure_call_dispatch_cache(funcaddr: ObjectRef, ctx: &mut ExecuteContext) -> CallDispatchCache {
    if let Some(cache) = ctx.gc.get_func(funcaddr).call_cache {
        return cache;
    }
    let cache = build_call_dispatch_cache(funcaddr, ctx);
    ctx.gc.get_func_mut(funcaddr).call_cache = Some(cache);
    cache
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
    funcaddr: ObjectRef,
    ctx: &mut ExecuteContext,
    is_return_call: bool,
) -> VMResult<CallOutcome> {
    dispatch_profile_count(if is_return_call {
        "op_return_call"
    } else {
        "op_call"
    });
    let cache = ensure_call_dispatch_cache(funcaddr, ctx);
    trace!(
        "op_call_internal: {:?}({:?})  {funcaddr:?}",
        ctx.gc.object_ref_for_instance(cache.frame.instance),
        ctx.gc.instance(cache.frame.instance).module_addr
    );
    let param_size = cache.param_size as usize;
    let return_pc = cached_return_pc(return_addr, ctx);
    match cache.target {
        CallDispatchTarget::Host(fp) => {
            if is_return_call {
                let local_reference = vm_try!(ctx.stack.function_return_call_cached(
                    &ctx.local_reference,
                    param_size,
                    0,
                    cache.frame,
                ));
                ctx.set_local_reference_with_frame(local_reference, cache.frame);
            } else {
                let local_reference = vm_try!(ctx.stack.function_call_cached(
                    param_size,
                    0,
                    cache.frame,
                    ctx.local_reference,
                    return_pc,
                ));
                ctx.set_local_reference_with_frame(local_reference, cache.frame);
            }
            invoke_sync_host_function_with(return_addr, ctx, fp)
        }
        CallDispatchTarget::AsyncHost(fp) => {
            if is_return_call {
                let local_reference = vm_try!(ctx.stack.function_return_call_cached(
                    &ctx.local_reference,
                    param_size,
                    0,
                    cache.frame,
                ));
                ctx.set_local_reference_with_frame(local_reference, cache.frame);
            } else {
                let local_reference = vm_try!(ctx.stack.function_call_cached(
                    param_size,
                    0,
                    cache.frame,
                    ctx.local_reference,
                    return_pc,
                ));
                ctx.set_local_reference_with_frame(local_reference, cache.frame);
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
                    cache.frame,
                ));
                ctx.set_local_reference_with_frame(local_reference, cache.frame);
            } else {
                let local_reference = vm_try!(ctx.stack.function_call_cached(
                    param_size,
                    local_size,
                    cache.frame,
                    ctx.local_reference,
                    return_pc,
                ));
                ctx.set_local_reference_with_frame(local_reference, cache.frame);
            }
            VMResult::Success(CallOutcome::Immediate(cache.frame.code_base))
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
    let funcidx = (*tail_code).operand.u32;
    let funcaddr = ctx.instance().funcs.as_slice()[funcidx as usize];
    let outcome = internal_op_call(tail_code.offset(1), funcaddr, ctx, false);
    match vm_try!(outcome) {
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
    unsafe { op_call(tail_code, ctx) }
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
    let funcidx = (*tail_code).operand.u32;
    let funcaddr = ctx.instance().funcs.as_slice()[funcidx as usize];
    let outcome = internal_op_call(tail_code.offset(1), funcaddr, ctx, true);
    match vm_try!(outcome) {
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
    unsafe { op_return_call(tail_code, ctx) }
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
    let outcome = internal_op_call_indirect(tail_code, ctx, false);
    match vm_try!(outcome) {
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
    let outcome = internal_op_call_indirect(tail_code, ctx, true);
    match vm_try!(outcome) {
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
