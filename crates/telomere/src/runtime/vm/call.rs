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
/// Notes: Resolves the callee, prepares the frame cache, and returns either a concrete instruction pointer or a suspended async host call.
///
/// # Safety
/// - `return_addr` must remain valid for the duration of the helper and must point back into the active decoded instruction stream.
/// - `ctx` must reference a live execution context for the same store and validated frame layout.
/// - This helper must not keep borrows, locks, or guards alive across the tail-dispatch it initiates.
pub(crate) unsafe fn internal_op_call(
    return_addr: *const Instr,
    funcaddr: GcRef,
    ctx: &mut ExecuteContext,
    is_return_call: bool,
) -> VMResult<Option<*const Instr>> {
    let funcinst = ctx.func_by_addr(funcaddr).clone();
    let (frame, ft) = {
        let gc = ctx.gc_ref();
        let instance = gc.instance(funcinst.instance);
        let memory0 = instance
            .memory_slots
            .first()
            .copied()
            .and_then(|slot| slot.handle());
        let frame = CallFrameCache::from_parts(funcaddr, &funcinst, memory0);
        let module_addr = instance.module_addr;
        let module = gc.get_module(module_addr);
        let typeidx = module
            .functions
            .get(funcinst.funcidx as usize)
            .unwrap_unchecked();
        let ft = module.function_types[typeidx.0 as usize].clone();
        (frame, ft)
    };
    trace!(
        "op_call_internal: {:?}({:?}) {funcaddr:?}",
        funcinst.funcidx,
        ctx.gc_ref().gc_ref_for_instance(funcinst.instance)
    );
    let mut param_size = 0usize;
    for param in ft.0.iter() {
        param_size += param.stack_size().usize();
    }
    let is_host_func = funcinst.is_host_func();
    if funcinst.is_host_func() {
        if is_return_call {
            vm_try!(ctx.enter_function_return_call(param_size, 0, frame));
        } else {
            vm_try!(ctx.enter_function_call(param_size, 0, frame, return_addr));
        }
        invoke_host_function(return_addr, ctx, &ft)
    } else {
        let (locals, code_offset) = funcinst.locals_and_code_offset(ctx.gc_ref());
        if is_return_call {
            vm_try!(ctx.enter_function_return_call(param_size, locals.byte_size(), frame,));
        } else {
            vm_try!(ctx.enter_function_call(param_size, locals.byte_size(), frame, return_addr,));
        }

        let ptr = funcinst
            .code_pointer()
            .expect("wasm function must expose a code pointer")
            .wrapping_add(code_offset);
        debug_assert!(!is_host_func);
        VMResult::Success(Some(ptr))
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
    let funcidx = (*tail_code).operand.u32;
    let funcaddr = ctx.instance().funcs.as_slice()[funcidx as usize];
    match vm_try!(internal_op_call(tail_code.offset(1), funcaddr, ctx, false)) {
        Some(ptr) => call_next(ptr, 0, ctx),
        None => VMResult::Success(()),
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
    let funcidx = (*tail_code).operand.u32;
    let funcaddr = ctx.instance().funcs.as_slice()[funcidx as usize];
    match vm_try!(internal_op_call(tail_code.offset(1), funcaddr, ctx, true)) {
        Some(ptr) => call_next(ptr, 0, ctx),
        None => VMResult::Success(()),
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
) -> VMResult<Option<*const Instr>> {
    let i = ctx.stack_mut().pop_u32();
    let tableidx = (*tail_code).operand.u32 as usize;
    let table_addr = *vm_try!(VMResult::from_option(
        ctx.instance().tables.as_slice().get(tableidx),
        || { VMResult::TableIndexOutOfRange }
    ));
    let func_addr = {
        let table = ctx.gc_mut().get_table(table_addr);
        let func_addr = *vm_try!(VMResult::from_option(table.1.get(i as usize), || {
            VMResult::TableIndexOutOfRange
        }));
        trace!("internal_op_call_indirect: {tableidx} {table_addr:?} {func_addr} {table:?}");
        func_addr
    };
    if func_addr == TABLE_UNINITIALIZED {
        return VMResult::TableUninitialized;
    }
    let func_addr = GcRef(func_addr);
    let (actual_ft, expected_ft) = {
        let gc = ctx.gc_ref();
        let funcinst = gc.get_func(func_addr);
        let instance = gc.instance(funcinst.instance);
        let module = gc.get_module(instance.module_addr);
        let actual_typeidx = module.functions.get(funcinst.funcidx as usize).unwrap();
        let actual_ft = module.function_types[actual_typeidx.0 as usize].clone();
        let expected_typeidx = (*tail_code.offset(1)).operand.u32;
        let expected_ft = ctx
            .module()
            .function_types
            .get(expected_typeidx as usize)
            .unwrap()
            .clone();
        (actual_ft, expected_ft)
    };
    trace!("{:?} {:?}", actual_ft, expected_ft);
    if actual_ft != expected_ft {
        return VMResult::CallIndirectInvalidType;
    }
    internal_op_call(tail_code.offset(2), func_addr, ctx, is_return_call)
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
    match vm_try!(internal_op_call_indirect(tail_code, ctx, false)) {
        Some(ptr) => call_next(ptr, 0, ctx),
        None => VMResult::Success(()),
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
    match vm_try!(internal_op_call_indirect(tail_code, ctx, true)) {
        Some(ptr) => call_next(ptr, 0, ctx),
        None => VMResult::Success(()),
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
    let function_type = current_function_type(ctx);
    match vm_try!(invoke_host_function(
        &VM_END as *const Instr,
        ctx,
        &function_type
    )) {
        Some(ptr) => call_next(ptr, 0, ctx),
        None => VMResult::Success(()),
    }
}
