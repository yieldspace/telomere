use super::*;
use vstd::prelude::*;

verus! {

#[allow(dead_code)]
pub(crate) enum CallStepWitnessParts {
    Call { function_id: u32, return_addr: nat, is_return_call: bool },
    CallIndirect {
        table_id: u32,
        elem_index: nat,
        expected_type_id: u32,
        return_addr: nat,
        is_return_call: bool,
    },
}

#[allow(dead_code)]
pub(crate) open spec fn call_witness_for_handler(
    function_id: u32,
    return_addr: nat,
    is_return_call: bool,
) -> CallStepWitnessParts {
    CallStepWitnessParts::Call {
        function_id,
        return_addr,
        is_return_call,
    }
}

#[allow(dead_code)]
pub(crate) open spec fn call_indirect_witness_for_handler(
    table_id: u32,
    elem_index: nat,
    expected_type_id: u32,
    return_addr: nat,
    is_return_call: bool,
) -> CallStepWitnessParts {
    CallStepWitnessParts::CallIndirect {
        table_id,
        elem_index,
        expected_type_id,
        return_addr,
        is_return_call,
    }
}

pub(crate) open spec fn call_step_from_witness_parts(
    witness: CallStepWitnessParts,
) -> crate::common::formal::CallStep {
    match witness {
        CallStepWitnessParts::Call {
            function_id,
            return_addr,
            is_return_call,
        } => crate::common::formal::CallStep::Call {
            function_id: function_id as nat,
            return_addr,
            is_return_call,
        },
        CallStepWitnessParts::CallIndirect {
            table_id,
            elem_index,
            expected_type_id,
            return_addr,
            is_return_call,
        } => crate::common::formal::CallStep::CallIndirect {
            table_id: table_id as nat,
            elem_index,
            expected_type_id: expected_type_id as nat,
            return_addr,
            is_return_call,
        },
    }
}

pub open spec fn call_continue_cont(
    before: crate::common::formal::CoreStepState,
    step: crate::common::formal::CallStep,
) -> nat {
    crate::common::formal::spec_step_call(before, step).0.context.cont_addr
}

pub(crate) open spec fn call_observation_refines_spec_step(
    before: crate::common::CoreStepStateProjectionParts,
    step: crate::common::formal::CallStep,
    after: crate::common::CoreStepStateProjectionParts,
    outcome: crate::common::formal::CoreOutcome,
) -> bool {
    crate::common::runtime_observation_refines_instr(
        before,
        crate::common::formal::CoreStepInstr::Call(step),
        after,
        outcome,
    ) && crate::common::observation_task_id_preserved(before, after)
        && crate::common::core_step_state_from_projection_parts(after).context.cont_addr
            == call_continue_cont(
                crate::common::core_step_state_from_projection_parts(before),
                step,
            )
}

proof fn lemma_call_family_state_refines_spec_step(
    before: crate::common::formal::CoreStepState,
    step: crate::common::formal::CallStep,
)
    ensures
        crate::common::formal::spec_step(
            before,
            crate::common::formal::CoreStepInstr::Call(step),
        ) == crate::common::formal::spec_step_call(before, step),
        crate::common::formal::task_id_preserved(
            before,
            crate::common::formal::spec_step_call(before, step).0,
        ),
        crate::common::formal::spec_step_call(before, step).0.context.cont_addr
            == call_continue_cont(before, step),
{
    assert(
        crate::common::formal::spec_step(
            before,
            crate::common::formal::CoreStepInstr::Call(step),
        ) == crate::common::formal::spec_step_call(before, step)
    );
    assert(crate::common::formal::task_id_preserved(
        before,
        crate::common::formal::spec_step_call(before, step).0,
    ));
}

pub(crate) proof fn lemma_call_family_refines_spec_step(
    before: crate::common::formal::CoreStepState,
    step: crate::common::formal::CallStep,
)
    ensures
        crate::common::formal::spec_step(
            before,
            crate::common::formal::CoreStepInstr::Call(step),
        ) == crate::common::formal::spec_step_call(before, step),
        crate::common::formal::task_id_preserved(
            before,
            crate::common::formal::spec_step_call(before, step).0,
        ),
        crate::common::formal::spec_step_call(before, step).0.context.cont_addr
            == call_continue_cont(before, step),
{
    lemma_call_family_state_refines_spec_step(before, step);
}

pub(crate) proof fn lemma_call_observation_refines_spec_step(
    before: crate::common::CoreStepStateProjectionParts,
    step: crate::common::formal::CallStep,
    after: crate::common::CoreStepStateProjectionParts,
    outcome: crate::common::formal::CoreOutcome,
)
    requires
        crate::common::runtime_observation_refines_instr(
            before,
            crate::common::formal::CoreStepInstr::Call(step),
            after,
            outcome,
        ),
    ensures
        call_observation_refines_spec_step(before, step, after, outcome),
{
    lemma_call_family_refines_spec_step(
        crate::common::core_step_state_from_projection_parts(before),
        step,
    );
}

pub(crate) open spec fn call_witness_observation_refines_spec_step(
    before: crate::common::CoreStepStateProjectionParts,
    witness: CallStepWitnessParts,
    after: crate::common::CoreStepStateProjectionParts,
    outcome: crate::common::formal::CoreOutcome,
) -> bool {
    call_observation_refines_spec_step(
        before,
        call_step_from_witness_parts(witness),
        after,
        outcome,
    )
}

pub(crate) proof fn lemma_call_witness_observation_refines_spec_step(
    before: crate::common::CoreStepStateProjectionParts,
    witness: CallStepWitnessParts,
    after: crate::common::CoreStepStateProjectionParts,
    outcome: crate::common::formal::CoreOutcome,
)
    requires
        crate::common::runtime_observation_refines_instr(
            before,
            crate::common::formal::CoreStepInstr::Call(call_step_from_witness_parts(witness)),
            after,
            outcome,
        ),
    ensures
        call_witness_observation_refines_spec_step(before, witness, after, outcome),
{
    lemma_call_observation_refines_spec_step(
        before,
        call_step_from_witness_parts(witness),
        after,
        outcome,
    );
}

pub(crate) proof fn lemma_call_handler_refines_spec_step(
    before: crate::common::CoreStepStateProjectionParts,
    witness: CallStepWitnessParts,
    after: crate::common::CoreStepStateProjectionParts,
    outcome: crate::common::formal::CoreOutcome,
)
    requires
        crate::common::runtime_observation_refines_instr(
            before,
            crate::common::formal::CoreStepInstr::Call(call_step_from_witness_parts(witness)),
            after,
            outcome,
        ),
    ensures
        call_witness_observation_refines_spec_step(before, witness, after, outcome),
{
    lemma_call_witness_observation_refines_spec_step(before, witness, after, outcome);
}

} // verus!

#[inline(always)]
/// Decode the direct callee function index immediate.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for the current handler.
unsafe fn decode_direct_call_funcidx(tail_code: *const Instr) -> u32 {
    (*tail_code).operand.u32
}

#[inline(always)]
/// Decode the table index immediate for `call_indirect`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for the current handler.
unsafe fn decode_indirect_call_tableidx(tail_code: *const Instr) -> usize {
    (*tail_code).operand.u32 as usize
}

#[inline(always)]
/// Decode the expected function-type index immediate for `call_indirect`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for the current handler.
unsafe fn decode_indirect_call_expected_typeidx(tail_code: *const Instr) -> u32 {
    (*tail_code.offset(1)).operand.u32
}

#[inline(always)]
/// Compute the return address for direct-call handlers.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for the current handler.
unsafe fn direct_call_return_addr(tail_code: *const Instr) -> *const Instr {
    tail_code.offset(1)
}

#[inline(always)]
/// Compute the return address for indirect-call handlers.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for the current handler.
unsafe fn indirect_call_return_addr(tail_code: *const Instr) -> *const Instr {
    tail_code.offset(2)
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
) -> VMResult<*const Instr> {
    let mut facade = ExecuteContextFacade::new(ctx);
    let funcinst = facade.func_by_addr(funcaddr);
    let (frame, param_types, result_types) = {
        let gc = facade.gc_ref();
        let instance = gc.instance(funcinst.instance);
        let memory0 = instance
            .memory_slots
            .first()
            .copied()
            .and_then(|slot| slot.handle());
        let frame = CallFrameCache::from_parts(funcaddr, funcinst, memory0);
        let module_addr = instance.module_addr;
        let module = gc.get_module(module_addr);
        let ft = module
            .function_types
            .get_unchecked(funcinst.typeidx.0 as usize);
        (
            frame,
            &ft.0 as *const ResultType,
            &ft.1 as *const ResultType,
        )
    };
    trace!(
        "op_call_internal: {:?}({:?}) {funcaddr:?}",
        funcinst.funcidx,
        facade.gc_ref_for_instance(funcinst.instance)
    );
    let param_size = funcinst.param_size();
    if funcinst.is_host_func() {
        if is_return_call {
            vm_try!(facade.enter_function_return_call(param_size, 0, frame));
        } else {
            vm_try!(facade.enter_function_call(param_size, 0, frame, return_addr));
        }
        vm_try!(unsafe {
            invoke_host_function(return_addr, &mut facade, param_types, result_types)
        });
        VMResult::Success(std::ptr::null())
    } else {
        let code_ptr = funcinst
            .code_pointer()
            .expect("wasm function must expose a code pointer");
        let local_size = funcinst.local_size();
        if is_return_call {
            vm_try!(facade.enter_function_return_call(param_size, local_size, frame,));
        } else {
            vm_try!(facade.enter_function_call(param_size, local_size, frame, return_addr,));
        }

        VMResult::Success(code_ptr)
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
    let funcidx = decode_direct_call_funcidx(tail_code);
    let funcaddr = ctx.instance().funcs.as_slice()[funcidx as usize];
    let ptr = vm_try!(internal_op_call(
        direct_call_return_addr(tail_code),
        funcaddr,
        ctx,
        false,
    ));
    if ptr.is_null() {
        VMResult::Success(())
    } else {
        call_next(ptr, 0, ctx)
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
    let funcidx = decode_direct_call_funcidx(tail_code);
    let funcaddr = ctx.instance().funcs.as_slice()[funcidx as usize];
    let ptr = vm_try!(internal_op_call(
        direct_call_return_addr(tail_code),
        funcaddr,
        ctx,
        true,
    ));
    if ptr.is_null() {
        VMResult::Success(())
    } else {
        call_next(ptr, 0, ctx)
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
unsafe fn resolve_indirect_call_target(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<GcRef> {
    let mut facade = ExecuteContextFacade::new(ctx);
    let i = facade.pop_u32();
    let tableidx = decode_indirect_call_tableidx(tail_code);
    let table_addr = *vm_try!(VMResult::from_option(
        facade.instance().tables.as_slice().get(tableidx),
        || { VMResult::TableIndexOutOfRange }
    ));
    let func_addr = vm_try!(VMResult::from_option(
        facade.table_get_value(table_addr, i as usize),
        || { VMResult::TableIndexOutOfRange }
    ));
    trace!("internal_op_call_indirect: {tableidx} {table_addr:?} {func_addr}");
    if func_addr == TABLE_UNINITIALIZED {
        return VMResult::TableUninitialized;
    }
    let func_addr = GcRef(func_addr);
    let actual_ft = facade.function_type_by_addr(func_addr) as *const FuncType;
    let expected_typeidx = decode_indirect_call_expected_typeidx(tail_code);
    let expected_ft = facade
        .module_function_type(expected_typeidx)
        .expect("validated call_indirect type index must exist")
        as *const FuncType;
    trace!("{:?} {:?}", unsafe { &*actual_ft }, unsafe {
        &*expected_ft
    });
    if unsafe { &*actual_ft } != unsafe { &*expected_ft } {
        return VMResult::CallIndirectInvalidType;
    }
    VMResult::Success(func_addr)
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
    let func_addr = vm_try!(resolve_indirect_call_target(tail_code, ctx));
    let ptr = vm_try!(internal_op_call(
        indirect_call_return_addr(tail_code),
        func_addr,
        ctx,
        false,
    ));
    if ptr.is_null() {
        VMResult::Success(())
    } else {
        call_next(ptr, 0, ctx)
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
    let func_addr = vm_try!(resolve_indirect_call_target(tail_code, ctx));
    let ptr = vm_try!(internal_op_call(
        indirect_call_return_addr(tail_code),
        func_addr,
        ctx,
        true,
    ));
    if ptr.is_null() {
        VMResult::Success(())
    } else {
        call_next(ptr, 0, ctx)
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
    let mut facade = ExecuteContextFacade::new(ctx);
    let function_type = unsafe { &*current_function_type_ptr(&facade) };
    unsafe {
        invoke_host_function(
            &VM_END as *const Instr,
            &mut facade,
            &function_type.0,
            &function_type.1,
        )
    }
}
