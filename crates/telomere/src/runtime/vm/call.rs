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

#[cfg(feature = "jit")]
/// Telomere runtime helper for JIT direct `call` / `return_call` sites.
///
/// Stack effect: internal runtime call dispatch.
/// Traps: returns a JIT trap exit for the same trap cases as `op_call` and `op_return_call`.
/// Notes: This is the JIT-facing wrapper around the existing direct-threaded call helper, so the
/// tail-call frame replacement contract remains owned by `internal_op_call`.
///
/// # Safety
/// - `tail_code` must point to the call recipe operand in the active decoded instruction stream.
/// - `ctx` must reference a live execution context whose stack and current frame match that stream.
/// - The caller must return to the JIT/interpreter continuation indicated by the returned exit.
pub(crate) unsafe fn jit_call_direct(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    is_return_call: bool,
) -> crate::runtime::jit::JitNativeExit {
    if ctx.effect.get_pending_count() != 0 {
        return crate::runtime::jit::JitNativeExit::pending();
    }
    let recipe = unsafe { decode_direct_call_recipe(tail_code, ctx) };
    if let CallDispatchTarget::Wasm { local_size } = recipe.target {
        return match unsafe {
            prepare_jit_wasm_call(
                tail_code.offset(1),
                recipe,
                local_size as usize,
                ctx,
                is_return_call,
            )
        } {
            VMResult::Success(()) => unsafe {
                finish_jit_wasm_call(tail_code.offset(1), recipe, ctx, is_return_call)
            },
            other => crate::runtime::jit::JitNativeExit::trap(other),
        };
    }
    match unsafe { internal_op_call(tail_code.offset(1), recipe, ctx, is_return_call) } {
        VMResult::Success(CallOutcome::Immediate(ptr)) => {
            if !is_return_call && ptr == tail_code.offset(1) {
                crate::runtime::jit::JitNativeExit::done_with_value(pack_call_stack_sizes(recipe))
            } else {
                crate::runtime::jit::JitNativeExit::continue_ptr(ptr)
            }
        }
        VMResult::Success(CallOutcome::Pending) => crate::runtime::jit::JitNativeExit::pending(),
        VMResult::Unreachable => {
            crate::runtime::jit::JitNativeExit::trap(VMResult::<()>::Unreachable)
        }
        VMResult::StackOverflow => {
            crate::runtime::jit::JitNativeExit::trap(VMResult::<()>::StackOverflow)
        }
        VMResult::MemoryIndexOutOfRange => {
            crate::runtime::jit::JitNativeExit::trap(VMResult::<()>::MemoryIndexOutOfRange)
        }
        VMResult::TableIndexOutOfRange => {
            crate::runtime::jit::JitNativeExit::trap(VMResult::<()>::TableIndexOutOfRange)
        }
        VMResult::CallIndirectInvalidType => {
            crate::runtime::jit::JitNativeExit::trap(VMResult::<()>::CallIndirectInvalidType)
        }
        VMResult::TableUninitialized => {
            crate::runtime::jit::JitNativeExit::trap(VMResult::<()>::TableUninitialized)
        }
        VMResult::Unlinkable => {
            crate::runtime::jit::JitNativeExit::trap(VMResult::<()>::Unlinkable)
        }
        VMResult::InvalidOperand => {
            crate::runtime::jit::JitNativeExit::trap(VMResult::<()>::InvalidOperand)
        }
        VMResult::UnalignedAtomic => {
            crate::runtime::jit::JitNativeExit::trap(VMResult::<()>::UnalignedAtomic)
        }
        VMResult::Unimplemented => {
            crate::runtime::jit::JitNativeExit::trap(VMResult::<()>::Unimplemented)
        }
    }
}

#[cfg(feature = "jit")]
/// Telomere runtime helper for fast JIT direct calls whose callee is known to be a Wasm function.
///
/// Stack effect: prepares the WebAssembly `call` callee frame and returns a JIT continuation exit.
/// Traps: returns a JIT trap exit for the same frame preparation failures as the generic direct
/// call helper.
///
/// # Safety
/// - `continuation` must point to the instruction immediately after the active callsite.
/// - `ctx` must still describe the caller frame when this helper is entered.
/// - The caller must honor the returned JIT exit and resume through the indicated continuation.
pub(crate) unsafe fn jit_call_direct_wasm_fast(
    funcaddr: ObjectRef,
    continuation: *const Instr,
    ctx: &mut ExecuteContext,
) -> crate::runtime::jit::JitNativeExit {
    if ctx.effect.get_pending_count() != 0 {
        return crate::runtime::jit::JitNativeExit::pending();
    }
    let recipe = ensure_call_recipe(funcaddr, ctx);
    let CallDispatchTarget::Wasm { local_size } = recipe.target else {
        return crate::runtime::jit::JitNativeExit::trap(VMResult::<()>::InvalidOperand);
    };
    match unsafe { prepare_jit_wasm_call(continuation, recipe, local_size as usize, ctx, false) } {
        VMResult::Success(()) if ctx.gc.jit_rejected_func(funcaddr) => unsafe {
            finish_rejected_direct_wasm_call(continuation, recipe, ctx)
        },
        VMResult::Success(()) => unsafe { finish_jit_wasm_call(continuation, recipe, ctx, false) },
        other => crate::runtime::jit::JitNativeExit::trap(other),
    }
}

#[cfg(feature = "jit")]
/// Telomere runtime helper for JIT `call_indirect` / `return_call_indirect` sites.
///
/// Stack effect: pops the indirect table element index and dispatches to the resolved target.
/// Traps: returns a JIT trap exit for the same table, type, and call failures as
/// `op_call_indirect` and `op_return_call_indirect`.
/// Notes: The helper resolves the callee through the active table and then reuses the direct-call
/// JIT entry path for Wasm targets so nested JIT fallback remains tied to the callee frame.
///
/// # Safety
/// - `tail_code` must point to the decoded indirect call instruction in the active stream.
/// - `ctx` must reference a live execution context whose stack contains the validated table index.
/// - The caller must honor the returned JIT exit and resume through the indicated continuation.
pub(crate) unsafe fn jit_call_indirect(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    is_return_call: bool,
) -> crate::runtime::jit::JitNativeExit {
    if ctx.effect.get_pending_count() != 0 {
        return crate::runtime::jit::JitNativeExit::pending();
    }
    let i = ctx.stack.pop_u32();
    let tableidx = unsafe { (*tail_code).operand.u32 as usize };
    let table_addr = match ctx.instance().tables.as_slice().get(tableidx) {
        Some(addr) => *addr,
        None => {
            return crate::runtime::jit::JitNativeExit::trap(VMResult::<()>::TableIndexOutOfRange);
        }
    };
    let table = ctx.gc.get_table(table_addr);
    let Some(func_addr) = table.1.get(i as usize).copied() else {
        return crate::runtime::jit::JitNativeExit::trap(VMResult::<()>::TableIndexOutOfRange);
    };
    if func_addr == TABLE_UNINITIALIZED {
        return crate::runtime::jit::JitNativeExit::trap(VMResult::<()>::TableUninitialized);
    }
    let func_addr = ObjectRef(func_addr);
    let funcinst = ctx.gc.get_func(func_addr);
    let instance = ctx.gc.instance(funcinst.instance);
    let module = ctx.gc.get_module(instance.module_addr);
    let Some(actual_typeidx) = module.functions.get(funcinst.funcidx as usize) else {
        return crate::runtime::jit::JitNativeExit::trap(VMResult::<()>::InvalidOperand);
    };
    let Some(actual_ft) = module.function_types.get(actual_typeidx.0 as usize) else {
        return crate::runtime::jit::JitNativeExit::trap(VMResult::<()>::InvalidOperand);
    };
    let expected_typeidx = unsafe { (*tail_code.offset(1)).operand.u32 };
    let Some(expected_ft) = ctx.module().function_types.get(expected_typeidx as usize) else {
        return crate::runtime::jit::JitNativeExit::trap(VMResult::<()>::InvalidOperand);
    };
    if actual_ft != expected_ft {
        return crate::runtime::jit::JitNativeExit::trap(VMResult::<()>::CallIndirectInvalidType);
    }
    let recipe = ensure_indirect_call_recipe(func_addr, ctx);
    if let CallDispatchTarget::Wasm { local_size } = recipe.target {
        return match unsafe {
            prepare_jit_wasm_call(
                tail_code.offset(2),
                recipe,
                local_size as usize,
                ctx,
                is_return_call,
            )
        } {
            VMResult::Success(()) => unsafe {
                finish_jit_wasm_call(tail_code.offset(2), recipe, ctx, is_return_call)
            },
            other => crate::runtime::jit::JitNativeExit::trap(other),
        };
    }
    match unsafe { internal_op_call(tail_code.offset(2), recipe, ctx, is_return_call) } {
        VMResult::Success(CallOutcome::Immediate(ptr)) => {
            if !is_return_call && ptr == unsafe { tail_code.offset(2) } {
                crate::runtime::jit::JitNativeExit::done_with_value(pack_call_stack_sizes(recipe))
            } else {
                crate::runtime::jit::JitNativeExit::continue_ptr(ptr)
            }
        }
        VMResult::Success(CallOutcome::Pending) => crate::runtime::jit::JitNativeExit::pending(),
        VMResult::Unreachable => {
            crate::runtime::jit::JitNativeExit::trap(VMResult::<()>::Unreachable)
        }
        VMResult::StackOverflow => {
            crate::runtime::jit::JitNativeExit::trap(VMResult::<()>::StackOverflow)
        }
        VMResult::MemoryIndexOutOfRange => {
            crate::runtime::jit::JitNativeExit::trap(VMResult::<()>::MemoryIndexOutOfRange)
        }
        VMResult::TableIndexOutOfRange => {
            crate::runtime::jit::JitNativeExit::trap(VMResult::<()>::TableIndexOutOfRange)
        }
        VMResult::CallIndirectInvalidType => {
            crate::runtime::jit::JitNativeExit::trap(VMResult::<()>::CallIndirectInvalidType)
        }
        VMResult::TableUninitialized => {
            crate::runtime::jit::JitNativeExit::trap(VMResult::<()>::TableUninitialized)
        }
        VMResult::Unlinkable => {
            crate::runtime::jit::JitNativeExit::trap(VMResult::<()>::Unlinkable)
        }
        VMResult::InvalidOperand => {
            crate::runtime::jit::JitNativeExit::trap(VMResult::<()>::InvalidOperand)
        }
        VMResult::UnalignedAtomic => {
            crate::runtime::jit::JitNativeExit::trap(VMResult::<()>::UnalignedAtomic)
        }
        VMResult::Unimplemented => {
            crate::runtime::jit::JitNativeExit::trap(VMResult::<()>::Unimplemented)
        }
    }
}

#[cfg(feature = "jit")]
unsafe fn prepare_jit_wasm_call(
    return_addr: *const Instr,
    recipe: CallDispatchCache,
    local_size: usize,
    ctx: &mut ExecuteContext,
    is_return_call: bool,
) -> VMResult<()> {
    let param_size = recipe.param_size as usize;
    if is_return_call {
        let local_reference = vm_try!(ctx.stack.function_return_call_cached(
            &ctx.local_reference,
            param_size,
            local_size,
            recipe.frame,
        ));
        ctx.set_local_reference_with_frame(local_reference, recipe.frame);
    } else {
        let return_pc = cached_return_pc(return_addr, ctx);
        let local_reference = vm_try!(ctx.stack.function_call_cached(
            param_size,
            local_size,
            recipe.frame,
            ctx.local_reference,
            return_pc,
        ));
        ctx.set_local_reference_with_frame(local_reference, recipe.frame);
    }
    VMResult::Success(())
}

#[cfg(feature = "jit")]
unsafe fn finish_jit_wasm_call(
    continuation: *const Instr,
    recipe: CallDispatchCache,
    ctx: &mut ExecuteContext,
    is_return_call: bool,
) -> crate::runtime::jit::JitNativeExit {
    let exit = unsafe { crate::runtime::jit::enter_current_frame_from_jit_call(ctx) };
    crate::runtime::jit::profile::count_exit(exit.kind);
    let returned_to_continuation = exit.kind == crate::runtime::jit::JitNativeExit::DONE
        || (exit.kind == crate::runtime::jit::JitNativeExit::CONTINUE_PTR
            && exit.value == continuation as u64)
        || (exit.kind == crate::runtime::jit::JitNativeExit::PENDING && ctx.cont == continuation);
    if !is_return_call && returned_to_continuation {
        crate::runtime::jit::JitNativeExit::done_with_value(pack_call_stack_sizes(recipe))
    } else if !is_return_call
        && matches!(
            exit.kind,
            crate::runtime::jit::JitNativeExit::CONTINUE_PTR
                | crate::runtime::jit::JitNativeExit::FALLBACK_PTR
        )
    {
        let continue_exit = unsafe {
            crate::runtime::jit::run_interpreter_continue_from_jit_call(
                exit.value as *const Instr,
                continuation,
                ctx,
            )
        };
        if continue_exit.kind == crate::runtime::jit::JitNativeExit::DONE {
            crate::runtime::jit::JitNativeExit::done_with_value(pack_call_stack_sizes(recipe))
        } else {
            continue_exit
        }
    } else {
        exit
    }
}

#[cfg(feature = "jit")]
unsafe fn finish_rejected_direct_wasm_call(
    continuation: *const Instr,
    recipe: CallDispatchCache,
    ctx: &mut ExecuteContext,
) -> crate::runtime::jit::JitNativeExit {
    crate::runtime::jit::profile::count(
        crate::runtime::jit::profile::Counter::RuntimeRejectedWasmCallFast,
    );
    let exit = unsafe {
        crate::runtime::jit::run_interpreter_continue_from_jit_call(ctx.code(), continuation, ctx)
    };
    if exit.kind == crate::runtime::jit::JitNativeExit::DONE {
        crate::runtime::jit::JitNativeExit::done_with_value(pack_call_stack_sizes(recipe))
    } else {
        exit
    }
}

#[cfg(feature = "jit")]
#[inline(always)]
fn pack_call_stack_sizes(recipe: CallDispatchCache) -> u64 {
    (u64::from(recipe.param_size) << 32) | u64::from(recipe.return_size)
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

#[cfg(feature = "jit")]
/// WebAssembly `call` quickened to the lazy baseline JIT entry path.
///
/// Stack effect: `[params] -> [results]`.
/// Traps: propagates target traps or returns `Unimplemented` when the callee cannot be compiled.
/// Notes: This handler is installed only for direct local Wasm callees when runtime JIT is enabled.
///
/// # Safety
/// - `tail_code` must point to the direct-call recipe operand for the active instruction stream.
/// - The recipe must describe a Wasm target prepared by instantiation-time callsite quickening.
/// - This handler must not keep borrows, locks, or guards alive across JIT entry or tail-dispatch.
pub unsafe fn op_call_jit_lazy(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    unsafe { op_call_jit_lazy_inner(tail_code, ctx, false) }
}

#[cfg(feature = "jit")]
unsafe fn op_call_jit_lazy_inner(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    is_return_call: bool,
) -> VMResult<()> {
    if ctx.effect.get_pending_count() != 0 {
        trace!("waiting effect: {:?}", ctx.cont);
        return VMResult::Success(());
    }
    if is_return_call && crate::runtime::jit::interpreter_stop_active() {
        return unsafe { op_return_call(tail_code, ctx) };
    }
    let recipe = unsafe { decode_direct_call_recipe(tail_code, ctx) };
    if !matches!(recipe.target, CallDispatchTarget::Wasm { .. }) {
        return if is_return_call {
            unsafe { op_return_call(tail_code, ctx) }
        } else {
            unsafe { op_call(tail_code, ctx) }
        };
    }
    match vm_try!(unsafe { internal_op_call(tail_code.offset(1), recipe, ctx, is_return_call) }) {
        CallOutcome::Immediate(_ptr) => unsafe { crate::runtime::jit::enter_current_frame(ctx) },
        CallOutcome::Pending => VMResult::Success(()),
    }
}

enum NumericTransitionCallOutcome {
    Inlined,
    Fallback,
    Pending,
}

#[inline(always)]
unsafe fn call_target_starts_with_numeric_transition(recipe: CallDispatchCache) -> bool {
    let CallDispatchTarget::Wasm { .. } = recipe.target else {
        return false;
    };
    if recipe.param_size != 8 || recipe.return_arity != 1 || recipe.frame.code_base.is_null() {
        return false;
    }
    std::ptr::fn_addr_eq(
        unsafe { (*recipe.frame.code_base).op },
        super::memory::op_i32_numeric_token_state_transition as crate::common::Op,
    )
}

#[inline(always)]
unsafe fn call_target_starts_with_crc16_update16(recipe: CallDispatchCache) -> bool {
    let CallDispatchTarget::Wasm { .. } = recipe.target else {
        return false;
    };
    if recipe.param_size != 8 || recipe.return_arity != 1 || recipe.frame.code_base.is_null() {
        return false;
    }
    std::ptr::fn_addr_eq(
        unsafe { (*recipe.frame.code_base).op },
        super::numeric::op_i32_crc16_update16 as crate::common::Op,
    )
}

#[inline(always)]
unsafe fn call_target_starts_with_crc16_update16_masked(recipe: CallDispatchCache) -> bool {
    let CallDispatchTarget::Wasm { .. } = recipe.target else {
        return false;
    };
    if recipe.param_size != 8 || recipe.return_arity != 1 || recipe.frame.code_base.is_null() {
        return false;
    }
    std::ptr::fn_addr_eq(
        unsafe { (*recipe.frame.code_base).op },
        super::numeric::op_i32_crc16_update16_masked as crate::common::Op,
    )
}

#[inline(always)]
unsafe fn call_target_starts_with_cached_u16_low7_guard(recipe: CallDispatchCache) -> bool {
    let CallDispatchTarget::Wasm { .. } = recipe.target else {
        return false;
    };
    if recipe.param_size != 8 || recipe.return_arity != 1 || recipe.frame.code_base.is_null() {
        return false;
    }
    std::ptr::fn_addr_eq(
        unsafe { (*recipe.frame.code_base).op },
        super::memory::op_i32_load16_u_local_base_tee4 as crate::common::Op,
    )
}

#[allow(dead_code)]
#[inline(always)]
unsafe fn call_target_starts_with_list_crc_summary(recipe: CallDispatchCache) -> bool {
    let CallDispatchTarget::Wasm { .. } = recipe.target else {
        return false;
    };
    if recipe.param_size != 8 || recipe.return_arity != 1 || recipe.frame.code_base.is_null() {
        return false;
    }
    std::ptr::fn_addr_eq(
        unsafe { (*recipe.frame.code_base).op },
        super::memory::op_i32_list_crc_summary as crate::common::Op,
    )
}

#[inline(never)]
unsafe fn internal_op_call_i32_numeric_token_state_transition(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<NumericTransitionCallOutcome> {
    dispatch_profile_count("op_call_i32_numeric_token_state_transition");
    if ctx.effect.get_pending_count() != 0 {
        trace!("waiting effect: {:?}", ctx.cont);
        return VMResult::Success(NumericTransitionCallOutcome::Pending);
    }
    let recipe = decode_direct_call_recipe(tail_code, ctx);
    if !unsafe { call_target_starts_with_numeric_transition(recipe) } {
        return VMResult::Success(NumericTransitionCallOutcome::Fallback);
    }

    let counts = ctx.stack.pop_u32_fast();
    let instr_ref = ctx.stack.pop_u32_fast();
    let state = vm_try!(unsafe {
        super::memory::i32_numeric_token_state_transition_value(instr_ref, counts, ctx)
    });
    vm_try!(ctx.stack.push_u32_fast(state));
    VMResult::Success(NumericTransitionCallOutcome::Inlined)
}

/// WebAssembly `call` specialized for an optimizer-generated numeric state-transition callee.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[i32, i32] -> [i32]` when the target still matches the native transition entry.
/// Traps: propagates memory traps from the inlined callee or falls back to the generic call path.
/// Notes: Keeps the direct call recipe operand live so host relinking can safely fall back to `op_call`.
///
/// # Safety
/// - `tail_code` must point to the direct-call recipe operand for the active instruction stream.
/// - The optimizer/instantiator must only select this handler for a direct call whose target was
///   observed to start with `op_i32_numeric_token_state_transition`.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_call_i32_numeric_token_state_transition(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    match vm_try!(unsafe { internal_op_call_i32_numeric_token_state_transition(tail_code, ctx) }) {
        NumericTransitionCallOutcome::Inlined => call_next(tail_code, 1, ctx),
        NumericTransitionCallOutcome::Fallback => unsafe { op_call(tail_code, ctx) },
        NumericTransitionCallOutcome::Pending => VMResult::Success(()),
    }
}

#[inline(never)]
unsafe fn internal_op_call_i32_crc16_update16(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<NumericTransitionCallOutcome> {
    dispatch_profile_count("op_call_i32_crc16_update16");
    if ctx.effect.get_pending_count() != 0 {
        trace!("waiting effect: {:?}", ctx.cont);
        return VMResult::Success(NumericTransitionCallOutcome::Pending);
    }
    let recipe = decode_direct_call_recipe(tail_code, ctx);
    if !unsafe { call_target_starts_with_crc16_update16(recipe) } {
        return VMResult::Success(NumericTransitionCallOutcome::Fallback);
    }

    let crc = ctx.stack.pop_u32_fast();
    let data = ctx.stack.pop_u32_fast();
    vm_try!(ctx
        .stack
        .push_u32_fast(super::numeric::crc16_update16_bits(data, crc)));
    VMResult::Success(NumericTransitionCallOutcome::Inlined)
}

/// WebAssembly `call` specialized for an optimizer-generated CRC16 update callee.
///
/// Stack effect: `[i32, i32] -> [i32]` when the target still matches the native CRC entry.
/// Traps: falls back to the generic call path if the target has been relinked.
/// Notes: Keeps the direct call recipe operand live so host relinking remains valid.
///
/// # Safety
/// - `tail_code` must point to the direct-call recipe operand for the active instruction stream.
/// - The instantiator must only select this handler for a direct call whose target was observed to
///   start with `op_i32_crc16_update16`.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_call_i32_crc16_update16(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    match vm_try!(unsafe { internal_op_call_i32_crc16_update16(tail_code, ctx) }) {
        NumericTransitionCallOutcome::Inlined => call_next(tail_code, 1, ctx),
        NumericTransitionCallOutcome::Fallback => unsafe { op_call(tail_code, ctx) },
        NumericTransitionCallOutcome::Pending => VMResult::Success(()),
    }
}

#[inline(never)]
unsafe fn internal_op_call_i32_crc16_update16_masked(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<NumericTransitionCallOutcome> {
    dispatch_profile_count("op_call_i32_crc16_update16_masked");
    if ctx.effect.get_pending_count() != 0 {
        trace!("waiting effect: {:?}", ctx.cont);
        return VMResult::Success(NumericTransitionCallOutcome::Pending);
    }
    let recipe = decode_direct_call_recipe(tail_code, ctx);
    if !unsafe { call_target_starts_with_crc16_update16_masked(recipe) } {
        return VMResult::Success(NumericTransitionCallOutcome::Fallback);
    }

    let crc = ctx.stack.pop_u32_fast();
    let data = ctx.stack.pop_u32_fast() & 0xffff;
    vm_try!(ctx
        .stack
        .push_u32_fast(super::numeric::crc16_update16_bits(data, crc)));
    VMResult::Success(NumericTransitionCallOutcome::Inlined)
}

/// WebAssembly `call` specialized for an optimizer-generated masked CRC16 wrapper callee.
///
/// Stack effect: `[i32, i32] -> [i32]` when the target still matches the native wrapper entry.
/// Traps: falls back to the generic call path if the target has been relinked.
/// Notes: This is the same relink-safe direct-call shape as `op_call_i32_crc16_update16`,
/// with the wrapper's `data & 0xffff` operation folded into the call site.
///
/// # Safety
/// - `tail_code` must point to the direct-call recipe operand for the active instruction stream.
/// - The instantiator must only select this handler for a direct call whose target was observed to
///   materialize as `op_i32_crc16_update16_masked`.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_call_i32_crc16_update16_masked(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    match vm_try!(unsafe { internal_op_call_i32_crc16_update16_masked(tail_code, ctx) }) {
        NumericTransitionCallOutcome::Inlined => call_next(tail_code, 1, ctx),
        NumericTransitionCallOutcome::Fallback => unsafe { op_call(tail_code, ctx) },
        NumericTransitionCallOutcome::Pending => VMResult::Success(()),
    }
}

#[inline(never)]
unsafe fn internal_op_call_cached_u16_low7_guard(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<NumericTransitionCallOutcome> {
    dispatch_profile_count("op_call_cached_u16_low7_guard");
    if ctx.effect.get_pending_count() != 0 {
        trace!("waiting effect: {:?}", ctx.cont);
        return VMResult::Success(NumericTransitionCallOutcome::Pending);
    }
    let recipe = decode_direct_call_recipe(tail_code, ctx);
    if !unsafe { call_target_starts_with_cached_u16_low7_guard(recipe) } {
        return VMResult::Success(NumericTransitionCallOutcome::Fallback);
    }

    let data_ptr = ctx.stack.peek_u32_fast_from_top(4);
    let cached =
        vm_try!(unsafe { ctx.default_local_memory_unchecked() }.read_u16_at(data_ptr as usize));
    if cached & 0x80 == 0 {
        return VMResult::Success(NumericTransitionCallOutcome::Fallback);
    }

    let _context = ctx.stack.pop_u32_fast();
    let _data = ctx.stack.pop_u32_fast();
    vm_try!(ctx.stack.push_u32_fast(u32::from(cached & 0x7f)));
    VMResult::Success(NumericTransitionCallOutcome::Inlined)
}

/// WebAssembly `call` specialized for a memoized `u16` low-bit guard.
///
/// Stack effect: `[i32, i32] -> [i32]` only when the callee's first load observes the cached bit.
/// Traps: preserves the callee's first memory load trap; falls back to generic call when uncached.
/// Notes: This is a guarded partial inlining of a common memoized-field function entry, not a full
/// function replacement. If the guard does not match at runtime, the normal call path runs.
///
/// # Safety
/// - `tail_code` must point to the direct-call recipe operand for the active instruction stream.
/// - The instantiator must only select this handler for a direct call whose target starts with the
///   validated cached-u16 guard shape.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_call_cached_u16_low7_guard(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    match vm_try!(unsafe { internal_op_call_cached_u16_low7_guard(tail_code, ctx) }) {
        NumericTransitionCallOutcome::Inlined => call_next(tail_code, 1, ctx),
        NumericTransitionCallOutcome::Fallback => unsafe { op_call(tail_code, ctx) },
        NumericTransitionCallOutcome::Pending => VMResult::Success(()),
    }
}

#[allow(dead_code)]
#[inline(never)]
unsafe fn internal_op_call_i32_list_crc_summary(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<NumericTransitionCallOutcome> {
    dispatch_profile_count("op_call_i32_list_crc_summary");
    if ctx.effect.get_pending_count() != 0 {
        trace!("waiting effect: {:?}", ctx.cont);
        return VMResult::Success(NumericTransitionCallOutcome::Pending);
    }
    let recipe = decode_direct_call_recipe(tail_code, ctx);
    if !unsafe { call_target_starts_with_list_crc_summary(recipe) } {
        return VMResult::Success(NumericTransitionCallOutcome::Fallback);
    }

    let finder_idx = ctx.stack.pop_u32_fast();
    let res = ctx.stack.pop_u32_fast();
    let retval = vm_try!(unsafe { super::memory::list_crc_summary_value(ctx, res, finder_idx) });
    vm_try!(ctx.stack.push_u32_fast(retval));
    VMResult::Success(NumericTransitionCallOutcome::Inlined)
}

/// WebAssembly `call` specialized for a function already lowered to a verified list/CRC summary.
///
/// Stack effect: `[i32, i32] -> [i32]` when the target still starts with
/// `op_i32_list_crc_summary`.
/// Traps: propagates the same memory traps as the summary body or falls back to generic call after
/// relinking.
/// Notes: This removes only the direct-call frame around an already selected summary; it does not
/// alter tail-call threading or `call_code` identity.
///
/// # Safety
/// - `tail_code` must point to the direct-call recipe operand for the active instruction stream.
/// - The instantiator must only select this for call targets whose body was verified and rewritten
///   to `op_i32_list_crc_summary`.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
#[allow(dead_code)]
pub unsafe fn op_call_i32_list_crc_summary(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    match vm_try!(unsafe { internal_op_call_i32_list_crc_summary(tail_code, ctx) }) {
        NumericTransitionCallOutcome::Inlined => call_next(tail_code, 1, ctx),
        NumericTransitionCallOutcome::Fallback => unsafe { op_call(tail_code, ctx) },
        NumericTransitionCallOutcome::Pending => VMResult::Success(()),
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

#[cfg(feature = "jit")]
/// WebAssembly `return_call` quickened to the lazy baseline JIT entry path.
///
/// Stack effect: tail-calls the target with the active frame replaced by the callee frame.
/// Traps: propagates target traps or returns `Unimplemented` when the callee cannot be compiled.
/// Notes: This keeps the interpreter tail-call helper unchanged while allowing JIT-enabled
/// callsites to enter compiled code without a generic `call_code` JIT check.
///
/// # Safety
/// - `tail_code` must point to the direct-call recipe operand for the active instruction stream.
/// - The recipe must describe a Wasm target prepared by instantiation-time callsite quickening.
/// - This handler must not keep borrows, locks, or guards alive across JIT entry or tail-dispatch.
pub unsafe fn op_return_call_jit_lazy(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    unsafe { op_call_jit_lazy_inner(tail_code, ctx, true) }
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

#[cfg(feature = "jit")]
/// Telomere internal trampoline for starting a Wasm function through lazy baseline JIT.
///
/// Stack effect: internal runtime continuation.
/// Traps: propagates target traps or returns `Unimplemented` when the current function cannot be compiled.
/// Notes: Used for exported/start Wasm functions that do not have an interpreter caller opcode to quicken.
///
/// # Safety
/// - `ctx` must already contain the target Wasm frame as the current frame.
/// - The active store must have runtime JIT enabled and run on a supported target.
/// - This handler must not keep borrows, locks, or guards alive across JIT entry or tail-dispatch.
pub unsafe fn special_start_jit_function_call(
    _tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    unsafe { crate::runtime::jit::enter_current_frame(ctx) }
}
