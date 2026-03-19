#![allow(clippy::missing_safety_doc)]

use super::*;

// Required for direct function call threading.
// If unset, LLVM will not replace the end of op_call with a jump.
#[inline(never)]
unsafe fn internal_op_call(
    return_addr: *const Instr,
    funcaddr: GcRef,
    ctx: &mut ExecuteContext,
    is_return_call: bool,
) -> VMResult<CallOutcome> {
    let funcinst = ctx.func_by_addr(funcaddr).clone();
    let instance = ctx.gc.instance(funcinst.instance);
    let frame = CallFrameCache::from_parts(funcaddr, &funcinst, &instance.mems);
    let module_addr = instance.module_addr;
    let module = ctx.gc.get_module(module_addr);
    let typeidx = module
        .functions
        .get(funcinst.funcidx as usize)
        .unwrap_unchecked();
    let ft = &module.function_types[typeidx.0 as usize];
    trace!(
        "op_call_internal: {:?}({module_addr:?})  {funcaddr:?}",
        ctx.gc.gc_ref_for_instance(funcinst.instance)
    );
    let mut param_size = 0usize;
    for param in ft.0.iter() {
        param_size += param.stack_size().usize();
    }
    let is_host_func = funcinst.is_host_func();
    if funcinst.is_host_func() {
        if is_return_call {
            let local_reference =
                vm_try!(ctx
                    .stack
                    .function_return_call(&ctx.local_reference, param_size, 0, frame));
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
        let (locals, code_offset) = funcinst.locals_and_code_offset(ctx.gc);
        if is_return_call {
            let local_reference = vm_try!(ctx.stack.function_return_call(
                &ctx.local_reference,
                param_size,
                locals.byte_size(),
                frame
            ));
            ctx.set_local_reference(local_reference);
        } else {
            let local_reference = vm_try!(ctx.stack.function_call(
                param_size,
                locals.byte_size(),
                frame,
                ctx.local_reference,
                return_addr,
                ctx.gc,
            ));
            ctx.set_local_reference(local_reference);
        }

        let ptr = funcinst
            .code_pointer()
            .expect("wasm function must expose a code pointer")
            .wrapping_add(code_offset);
        debug_assert!(!is_host_func);
        VMResult::Success(CallOutcome::Immediate(ptr))
    }
}

pub unsafe fn op_call(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    if wait_effect(ctx, ctx.cont) {
        return VMResult::Success(());
    }
    let funcidx = (*tail_code).operand.u32;
    let funcaddr = ctx.instance().funcs.as_slice()[funcidx as usize];
    match vm_try!(internal_op_call(tail_code.offset(1), funcaddr, ctx, false)) {
        CallOutcome::Immediate(ptr) => call_next(ptr, 0, ctx),
        CallOutcome::Pending => VMResult::Success(()),
    }
}

pub unsafe fn op_return_call(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    if wait_effect(ctx, ctx.cont) {
        return VMResult::Success(());
    }
    let funcidx = (*tail_code).operand.u32;
    let funcaddr = ctx.instance().funcs.as_slice()[funcidx as usize];
    match vm_try!(internal_op_call(tail_code.offset(1), funcaddr, ctx, true)) {
        CallOutcome::Immediate(ptr) => call_next(ptr, 0, ctx),
        CallOutcome::Pending => VMResult::Success(()),
    }
}

#[inline(never)]
unsafe fn internal_op_call_indirect(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    is_return_call: bool,
) -> VMResult<CallOutcome> {
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
    let func_addr = GcRef(func_addr);
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

pub unsafe fn op_call_indirect(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    if wait_effect(ctx, ctx.cont) {
        return VMResult::Success(());
    }
    match vm_try!(internal_op_call_indirect(tail_code, ctx, false)) {
        CallOutcome::Immediate(ptr) => call_next(ptr, 0, ctx),
        CallOutcome::Pending => VMResult::Success(()),
    }
}

pub unsafe fn op_return_call_indirect(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    if wait_effect(ctx, ctx.cont) {
        return VMResult::Success(());
    }
    match vm_try!(internal_op_call_indirect(tail_code, ctx, true)) {
        CallOutcome::Immediate(ptr) => call_next(ptr, 0, ctx),
        CallOutcome::Pending => VMResult::Success(()),
    }
}

pub unsafe fn special_start_function_call(
    _tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    match vm_try!(invoke_host_function(&VM_END as *const Instr, ctx)) {
        CallOutcome::Immediate(ptr) => call_next(ptr, 0, ctx),
        CallOutcome::Pending => VMResult::Success(()),
    }
}
