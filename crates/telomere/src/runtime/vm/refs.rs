use super::*;
use vstd::prelude::*;

verus! {

#[allow(dead_code)]
pub(crate) enum RefStepWitnessParts {
    Null { next_cont: nat },
    IsNull { next_cont: nat },
    Func { function_id: u32, next_cont: nat },
}

#[allow(dead_code)]
pub(crate) open spec fn ref_null_witness_for_handler(next_cont: nat) -> RefStepWitnessParts {
    RefStepWitnessParts::Null { next_cont }
}

#[allow(dead_code)]
pub(crate) open spec fn ref_is_null_witness_for_handler(next_cont: nat) -> RefStepWitnessParts {
    RefStepWitnessParts::IsNull { next_cont }
}

#[allow(dead_code)]
pub(crate) open spec fn ref_func_witness_for_handler(
    function_id: u32,
    next_cont: nat,
) -> RefStepWitnessParts {
    RefStepWitnessParts::Func {
        function_id,
        next_cont,
    }
}

pub(crate) open spec fn ref_step_from_witness_parts(
    witness: RefStepWitnessParts,
) -> crate::common::formal::RefStep {
    match witness {
        RefStepWitnessParts::Null { next_cont } => crate::common::formal::RefStep::Null {
            next_cont,
        },
        RefStepWitnessParts::IsNull { next_cont } => {
            crate::common::formal::RefStep::IsNull { next_cont }
        }
        RefStepWitnessParts::Func {
            function_id,
            next_cont,
        } => crate::common::formal::RefStep::Func {
            function_id: function_id as nat,
            next_cont,
        },
    }
}

pub open spec fn spec_ref_null_result() -> crate::common::formal::RefView {
    crate::common::formal::ref_null()
}

pub open spec fn spec_ref_is_null_result(value: u32) -> u32 {
    crate::common::formal::ref_is_null_result(value)
}

pub(crate) open spec fn ref_observation_refines_spec_step(
    before: crate::common::CoreStepStateProjectionParts,
    step: crate::common::formal::RefStep,
    after: crate::common::CoreStepStateProjectionParts,
    outcome: crate::common::formal::CoreOutcome,
) -> bool {
    crate::common::runtime_observation_refines_instr(
        before,
        crate::common::formal::CoreStepInstr::Ref(step),
        after,
        outcome,
    ) && crate::common::observation_task_id_preserved(before, after)
        && crate::common::observation_current_default_memory_preserved(before, after)
        && crate::common::observation_caller_default_memory_preserved(before, after)
        && if crate::common::formal::outcome_is_trap(outcome) {
            crate::common::core_step_state_from_projection_parts(after).context.cont_addr
                == crate::common::core_step_state_from_projection_parts(before)
                    .context
                    .cont_addr
        } else {
            crate::common::core_step_state_from_projection_parts(after).context.cont_addr
                == ref_continue_cont(step)
        }
}

proof fn lemma_ref_family_state_refines_spec_step(
    before: crate::common::formal::CoreStepState,
    step: crate::common::formal::RefStep,
)
    ensures
        crate::common::formal::spec_step(
            before,
            crate::common::formal::CoreStepInstr::Ref(step),
        ) == crate::common::formal::spec_step_ref(before, step),
        crate::common::formal::task_id_preserved(
            before,
            crate::common::formal::spec_step_ref(before, step).0,
        ),
        crate::common::formal::current_default_memory_of(
            crate::common::formal::spec_step_ref(before, step).0,
        ) == crate::common::formal::current_default_memory_of(before),
        crate::common::formal::caller_default_memory_of(
            crate::common::formal::spec_step_ref(before, step).0,
        ) == crate::common::formal::caller_default_memory_of(before),
        if crate::common::formal::outcome_is_trap(
            crate::common::formal::spec_step_ref(before, step).1,
        ) {
            crate::common::formal::spec_step_ref(before, step).0.context.cont_addr
                == before.context.cont_addr
        } else {
            crate::common::formal::spec_step_ref(before, step).0.context.cont_addr
                == ref_continue_cont(step)
        },
{
}

pub open spec fn ref_continue_cont(step: crate::common::formal::RefStep) -> nat {
    match step {
        crate::common::formal::RefStep::Null { next_cont } => next_cont,
        crate::common::formal::RefStep::IsNull { next_cont } => next_cont,
        crate::common::formal::RefStep::Func { next_cont, .. } => next_cont,
    }
}

pub(crate) proof fn lemma_ref_family_refines_spec_step(
    before: crate::common::formal::CoreStepState,
    step: crate::common::formal::RefStep,
)
    ensures
        crate::common::formal::spec_step(
            before,
            crate::common::formal::CoreStepInstr::Ref(step),
        ) == crate::common::formal::spec_step_ref(before, step),
        crate::common::formal::task_id_preserved(
            before,
            crate::common::formal::spec_step_ref(before, step).0,
        ),
        crate::common::formal::current_default_memory_of(
            crate::common::formal::spec_step_ref(before, step).0,
        ) == crate::common::formal::current_default_memory_of(before),
        crate::common::formal::caller_default_memory_of(
            crate::common::formal::spec_step_ref(before, step).0,
        ) == crate::common::formal::caller_default_memory_of(before),
        if crate::common::formal::outcome_is_trap(
            crate::common::formal::spec_step_ref(before, step).1,
        ) {
            crate::common::formal::spec_step_ref(before, step).0.context.cont_addr
                == before.context.cont_addr
        } else {
            crate::common::formal::spec_step_ref(before, step).0.context.cont_addr
                == ref_continue_cont(step)
        },
{
}

pub(crate) proof fn lemma_ref_observation_refines_spec_step(
    before: crate::common::CoreStepStateProjectionParts,
    step: crate::common::formal::RefStep,
    after: crate::common::CoreStepStateProjectionParts,
    outcome: crate::common::formal::CoreOutcome,
)
    requires
        crate::common::runtime_observation_refines_instr(
            before,
            crate::common::formal::CoreStepInstr::Ref(step),
            after,
            outcome,
        ),
    ensures
        ref_observation_refines_spec_step(before, step, after, outcome),
{
    lemma_ref_family_refines_spec_step(
        crate::common::core_step_state_from_projection_parts(before),
        step,
    );
}

pub(crate) open spec fn ref_witness_observation_refines_spec_step(
    before: crate::common::CoreStepStateProjectionParts,
    witness: RefStepWitnessParts,
    after: crate::common::CoreStepStateProjectionParts,
    outcome: crate::common::formal::CoreOutcome,
) -> bool {
    ref_observation_refines_spec_step(before, ref_step_from_witness_parts(witness), after, outcome)
}

pub(crate) proof fn lemma_ref_witness_observation_refines_spec_step(
    before: crate::common::CoreStepStateProjectionParts,
    witness: RefStepWitnessParts,
    after: crate::common::CoreStepStateProjectionParts,
    outcome: crate::common::formal::CoreOutcome,
)
    requires
        crate::common::runtime_observation_refines_instr(
            before,
            crate::common::formal::CoreStepInstr::Ref(ref_step_from_witness_parts(witness)),
            after,
            outcome,
        ),
    ensures
        ref_witness_observation_refines_spec_step(before, witness, after, outcome),
{
    lemma_ref_observation_refines_spec_step(
        before,
        ref_step_from_witness_parts(witness),
        after,
        outcome,
    );
}

pub(crate) proof fn lemma_ref_handler_refines_spec_step(
    before: crate::common::CoreStepStateProjectionParts,
    witness: RefStepWitnessParts,
    after: crate::common::CoreStepStateProjectionParts,
    outcome: crate::common::formal::CoreOutcome,
)
    requires
        crate::common::runtime_observation_refines_instr(
            before,
            crate::common::formal::CoreStepInstr::Ref(ref_step_from_witness_parts(witness)),
            after,
            outcome,
        ),
    ensures
        ref_witness_observation_refines_spec_step(before, witness, after, outcome),
{
    lemma_ref_witness_observation_refines_spec_step(before, witness, after, outcome);
}

#[inline(always)]
fn null_ref_value() -> (result: u32)
    ensures
        result == 0u32,
{
    0u32
}

#[inline(always)]
fn null_result(value: u32) -> (result: u32)
    ensures
        result == if value == 0u32 { 1u32 } else { 0u32 },
{
    if value == 0u32 { 1u32 } else { 0u32 }
}

} // verus!

#[inline(always)]
unsafe fn push_ref_value(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    skip: isize,
    value: u32,
) -> VMResult<()> {
    let mut facade = ExecuteContextFacade::new(ctx);
    vm_try!(facade.push_ref(value));
    facade_call_next(tail_code, skip, &mut facade)
}

#[inline(always)]
unsafe fn ref_func_value(facade: &ExecuteContextFacade<'_, '_>, funcidx: u32) -> u32 {
    facade.ref_func_value(funcidx)
}

#[inline(always)]
/// Decode the `ref.func` target index immediate.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for the current handler.
unsafe fn decode_ref_func_index(tail_code: *const Instr) -> u32 {
    (*tail_code).operand.u32
}

#[inline(always)]
unsafe fn pop_ref_value(ctx: &mut ExecuteContext) -> u32 {
    ExecuteContextFacade::new(ctx).pop_ref()
}

/// WebAssembly `ref.null`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[] -> [ref]`.
/// Traps: none.
/// Notes: Implements the validated reference semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_ref_null(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    push_ref_value(tail_code, ctx, 0, null_ref_value())
}

/// WebAssembly `ref.is.null`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[ref] -> [i32]`.
/// Traps: none.
/// Notes: Implements the validated reference semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_ref_is_null(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let value = pop_ref_value(ctx);
    push_ref_value(tail_code, ctx, 0, null_result(value))
}

/// WebAssembly `ref.func`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[] -> [funcref]`.
/// Traps: none.
/// Notes: Implements the validated reference semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_ref_func(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let funcidx = decode_ref_func_index(tail_code);
    let facade = ExecuteContextFacade::new(ctx);
    let value = ref_func_value(&facade, funcidx);
    push_ref_value(tail_code, ctx, 1, value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        common::{
            stack::CachedMemoryKind,
            store::{self, FunctionBody, FunctionInstanceData, InstanceData},
            CallFrameCache, ExecuteContext, GcRef, LocalReference, LocalsData, Operand, Store,
            StoreInner, TypeIdx,
        },
        runtime::{memory_effect::PendingOp, scheduler::PendingOpEmitter},
    };
    use std::{collections::VecDeque, sync::Arc};

    fn frame(instance: store::InstanceId) -> CallFrameCache {
        CallFrameCache {
            code_addr: GcRef(0),
            code_base: std::ptr::null(),
            instance,
            memory0_kind: CachedMemoryKind::None,
            memory0_raw: 0,
        }
    }

    fn test_context<'a>(
        stack: &'a mut Stack,
        store: &'a Store,
        gc: &'a mut StoreInner,
        pending_effects: &'a mut u32,
        pending_ops: &'a mut VecDeque<PendingOp>,
        instance: store::InstanceId,
    ) -> ExecuteContext<'a> {
        ExecuteContext::new(
            stack,
            LocalReference {
                local_top: 0,
                local_size: 0,
            },
            frame(instance),
            store,
            gc,
            PendingOpEmitter::from_parts(23, pending_effects, pending_ops),
            std::ptr::null(),
            23,
        )
    }

    unsafe fn stop_op(_tail_code: *const Instr, _ctx: &mut ExecuteContext) -> VMResult<()> {
        VMResult::Success(())
    }

    #[test]
    fn ref_observation_tracks_func_continue() {
        let store = Store::new();
        let mut gc = StoreInner::new();
        let mut pending_effects = 0;
        let mut pending_ops = VecDeque::new();
        let mut stack = Stack::new(32);

        let instance = store::InstanceId::from_index(0);
        let func = gc.new_func(&FunctionInstanceData {
            instance,
            funcidx: 0,
            typeidx: TypeIdx(0),
            param_size: 0,
            local_size: 0,
            body: FunctionBody::Wasm {
                locals: LocalsData::default(),
                code: Arc::<[Instr]>::from(vec![crate::runtime::vm::VM_END]),
            },
        });
        let _instance_addr = gc.new_instance(&InstanceData {
            instance_id: 1,
            module_addr: GcRef(0),
            globals: Vec::new(),
            funcs: vec![func],
            tables: Vec::new(),
            mems: Vec::new(),
            memory_slots: Vec::new(),
        });

        let program = [
            Instr {
                operand: Operand { u32: 0 },
            },
            Instr { op: stop_op },
        ];
        let mut ctx = test_context(
            &mut stack,
            &store,
            &mut gc,
            &mut pending_effects,
            &mut pending_ops,
            instance,
        );

        let pending_before = ctx.pending_len();
        let result = unsafe { op_ref_func(program.as_ptr(), &mut ctx) };
        let outcome = crate::common::formal::core_outcome_from_vm_result_with_pending(
            &result,
            pending_before,
            ctx.pending_len(),
            ctx.pending_code_delta(pending_before).unwrap_or(None),
        )
        .unwrap();

        assert_eq!(outcome, crate::common::formal::CoreOutcome::Continue);
        let value = {
            let mut facade = ExecuteContextFacade::new(&mut ctx);
            facade.pop_ref()
        };
        assert_eq!(value, func.get());
        assert_eq!(ctx.pending_len(), 0);
        assert_eq!(ctx.cont(), unsafe { program.as_ptr().add(1) });
    }
}
