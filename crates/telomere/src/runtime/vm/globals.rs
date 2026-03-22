use super::*;
use vstd::prelude::*;

verus! {

#[allow(dead_code)]
pub(crate) enum GlobalStepWitnessParts {
    Get { global_id: u32, next_cont: nat },
    Set { global_id: u32, next_cont: nat },
}

#[allow(dead_code)]
pub(crate) open spec fn global_get_witness_for_handler(
    global_id: u32,
    next_cont: nat,
) -> GlobalStepWitnessParts {
    GlobalStepWitnessParts::Get {
        global_id,
        next_cont,
    }
}

#[allow(dead_code)]
pub(crate) open spec fn global_set_witness_for_handler(
    global_id: u32,
    next_cont: nat,
) -> GlobalStepWitnessParts {
    GlobalStepWitnessParts::Set {
        global_id,
        next_cont,
    }
}

pub(crate) open spec fn global_step_from_witness_parts(
    witness: GlobalStepWitnessParts,
) -> crate::common::formal::GlobalStep {
    match witness {
        GlobalStepWitnessParts::Get {
            global_id,
            next_cont,
        } => crate::common::formal::GlobalStep::Get {
            global_id: global_id as nat,
            next_cont,
        },
        GlobalStepWitnessParts::Set {
            global_id,
            next_cont,
        } => crate::common::formal::GlobalStep::Set {
            global_id: global_id as nat,
            next_cont,
        },
    }
}

pub open spec fn spec_global_get_result(global: crate::common::formal::GlobalView) -> Seq<u8> {
    crate::common::formal::global_get_bytes(global)
}

pub open spec fn spec_global_set_result(
    global: crate::common::formal::GlobalView,
    bytes: Seq<u8>,
) -> crate::common::formal::GlobalView {
    crate::common::formal::global_set_bytes(global, bytes)
}

pub(crate) open spec fn global_observation_refines_spec_step(
    before: crate::common::CoreStepStateProjectionParts,
    step: crate::common::formal::GlobalStep,
    after: crate::common::CoreStepStateProjectionParts,
    outcome: crate::common::formal::CoreOutcome,
) -> bool {
    crate::common::runtime_observation_refines_instr(
        before,
        crate::common::formal::CoreStepInstr::Global(step),
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
                == global_continue_cont(step)
        }
}

proof fn lemma_global_family_state_refines_spec_step(
    before: crate::common::formal::CoreStepState,
    step: crate::common::formal::GlobalStep,
)
    ensures
        crate::common::formal::spec_step(
            before,
            crate::common::formal::CoreStepInstr::Global(step),
        ) == crate::common::formal::spec_step_global(before, step),
        crate::common::formal::task_id_preserved(
            before,
            crate::common::formal::spec_step_global(before, step).0,
        ),
        crate::common::formal::current_default_memory_of(
            crate::common::formal::spec_step_global(before, step).0,
        ) == crate::common::formal::current_default_memory_of(before),
        crate::common::formal::caller_default_memory_of(
            crate::common::formal::spec_step_global(before, step).0,
        ) == crate::common::formal::caller_default_memory_of(before),
        if crate::common::formal::outcome_is_trap(
            crate::common::formal::spec_step_global(before, step).1,
        ) {
            crate::common::formal::spec_step_global(before, step).0.context.cont_addr
                == before.context.cont_addr
        } else {
            crate::common::formal::spec_step_global(before, step).0.context.cont_addr
                == global_continue_cont(step)
        },
{
}

pub open spec fn global_continue_cont(step: crate::common::formal::GlobalStep) -> nat {
    match step {
        crate::common::formal::GlobalStep::Get { next_cont, .. } => next_cont,
        crate::common::formal::GlobalStep::Set { next_cont, .. } => next_cont,
    }
}

pub(crate) proof fn lemma_global_family_refines_spec_step(
    before: crate::common::formal::CoreStepState,
    step: crate::common::formal::GlobalStep,
)
    ensures
        crate::common::formal::spec_step(
            before,
            crate::common::formal::CoreStepInstr::Global(step),
        ) == crate::common::formal::spec_step_global(before, step),
        crate::common::formal::task_id_preserved(
            before,
            crate::common::formal::spec_step_global(before, step).0,
        ),
        crate::common::formal::current_default_memory_of(
            crate::common::formal::spec_step_global(before, step).0,
        ) == crate::common::formal::current_default_memory_of(before),
        crate::common::formal::caller_default_memory_of(
            crate::common::formal::spec_step_global(before, step).0,
        ) == crate::common::formal::caller_default_memory_of(before),
        if crate::common::formal::outcome_is_trap(
            crate::common::formal::spec_step_global(before, step).1,
        ) {
            crate::common::formal::spec_step_global(before, step).0.context.cont_addr
                == before.context.cont_addr
        } else {
            crate::common::formal::spec_step_global(before, step).0.context.cont_addr
                == global_continue_cont(step)
        },
{
}

pub(crate) proof fn lemma_global_observation_refines_spec_step(
    before: crate::common::CoreStepStateProjectionParts,
    step: crate::common::formal::GlobalStep,
    after: crate::common::CoreStepStateProjectionParts,
    outcome: crate::common::formal::CoreOutcome,
)
    requires
        crate::common::runtime_observation_refines_instr(
            before,
            crate::common::formal::CoreStepInstr::Global(step),
            after,
            outcome,
        ),
    ensures
        global_observation_refines_spec_step(before, step, after, outcome),
{
    lemma_global_family_refines_spec_step(
        crate::common::core_step_state_from_projection_parts(before),
        step,
    );
}

pub(crate) open spec fn global_witness_observation_refines_spec_step(
    before: crate::common::CoreStepStateProjectionParts,
    witness: GlobalStepWitnessParts,
    after: crate::common::CoreStepStateProjectionParts,
    outcome: crate::common::formal::CoreOutcome,
) -> bool {
    global_observation_refines_spec_step(
        before,
        global_step_from_witness_parts(witness),
        after,
        outcome,
    )
}

pub(crate) proof fn lemma_global_witness_observation_refines_spec_step(
    before: crate::common::CoreStepStateProjectionParts,
    witness: GlobalStepWitnessParts,
    after: crate::common::CoreStepStateProjectionParts,
    outcome: crate::common::formal::CoreOutcome,
)
    requires
        crate::common::runtime_observation_refines_instr(
            before,
            crate::common::formal::CoreStepInstr::Global(global_step_from_witness_parts(witness)),
            after,
            outcome,
        ),
    ensures
        global_witness_observation_refines_spec_step(before, witness, after, outcome),
{
    lemma_global_observation_refines_spec_step(
        before,
        global_step_from_witness_parts(witness),
        after,
        outcome,
    );
}

pub(crate) proof fn lemma_global_handler_refines_spec_step(
    before: crate::common::CoreStepStateProjectionParts,
    witness: GlobalStepWitnessParts,
    after: crate::common::CoreStepStateProjectionParts,
    outcome: crate::common::formal::CoreOutcome,
)
    requires
        crate::common::runtime_observation_refines_instr(
            before,
            crate::common::formal::CoreStepInstr::Global(global_step_from_witness_parts(witness)),
            after,
            outcome,
        ),
    ensures
        global_witness_observation_refines_spec_step(before, witness, after, outcome),
{
    lemma_global_witness_observation_refines_spec_step(before, witness, after, outcome);
}

#[allow(dead_code)]
#[inline(always)]
fn global_index(idx: usize) -> (result: usize)
    ensures
        result == idx,
{
    idx
}

} // verus!

#[inline(always)]
/// Decode the global index immediate for the active global instruction.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for the current handler.
unsafe fn decode_global_index(tail_code: *const Instr) -> usize {
    (*tail_code).operand.u32 as usize
}

#[allow(dead_code)]
#[inline(always)]
unsafe fn global_addr(tail_code: *const Instr, facade: &ExecuteContextFacade<'_, '_>) -> GcRef {
    let idx = global_index(decode_global_index(tail_code));
    facade.global_addr(idx)
}

#[inline(always)]
unsafe fn global_get<const SIZE: usize>(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let mut facade = ExecuteContextFacade::new(ctx);
    trace!("op_global_get{SIZE}: {:?}", global_addr(tail_code, &facade));
    vm_try!(facade.push_global_bytes::<SIZE>(decode_global_index(tail_code)));
    facade_call_next(tail_code, 1, &mut facade)
}

#[inline(always)]
unsafe fn global_set<const SIZE: usize>(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let mut facade = ExecuteContextFacade::new(ctx);
    let value = facade.pop_u8_array::<SIZE>();
    trace!("op_global_set{SIZE}: {:?}", global_addr(tail_code, &facade));
    facade.write_global_bytes(decode_global_index(tail_code), value);
    facade_call_next(tail_code, 1, &mut facade)
}

/// WebAssembly `global.get`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[] -> [global]`.
/// Traps: none.
/// Notes: Accesses the instance global storage and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_global_get4(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    global_get::<4>(tail_code, ctx)
}

/// WebAssembly `global.get`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[] -> [global]`.
/// Traps: none.
/// Notes: Accesses the instance global storage and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_global_get8(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    global_get::<8>(tail_code, ctx)
}

/// WebAssembly `global.get`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[] -> [global]`.
/// Traps: none.
/// Notes: Accesses the instance global storage and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_global_get16(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    global_get::<16>(tail_code, ctx)
}

/// WebAssembly `global.set`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[global] -> []`.
/// Traps: none.
/// Notes: Accesses the instance global storage and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_global_set4(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    global_set::<4>(tail_code, ctx)
}

/// WebAssembly `global.set`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[global] -> []`.
/// Traps: none.
/// Notes: Accesses the instance global storage and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_global_set8(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    global_set::<8>(tail_code, ctx)
}

/// WebAssembly `global.set`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[global] -> []`.
/// Traps: none.
/// Notes: Accesses the instance global storage and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_global_set16(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    global_set::<16>(tail_code, ctx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        common::{
            stack::CachedMemoryKind,
            store::{self, InstanceData},
            CallFrameCache, ExecuteContext, GcRef, LocalReference, Operand, Store, StoreInner,
        },
        runtime::{memory_effect::PendingOp, scheduler::PendingOpEmitter},
    };
    use std::collections::VecDeque;

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
            PendingOpEmitter::from_parts(19, pending_effects, pending_ops),
            std::ptr::null(),
            19,
        )
    }

    unsafe fn stop_op(_tail_code: *const Instr, _ctx: &mut ExecuteContext) -> VMResult<()> {
        VMResult::Success(())
    }

    #[test]
    fn global_observation_tracks_get_continue() {
        let store = Store::new();
        let mut gc = StoreInner::new();
        let mut pending_effects = 0;
        let mut pending_ops = VecDeque::new();
        let mut stack = Stack::new(32);

        let global = gc.new_global_data4(0x1122_3344);
        let instance = store::InstanceId::from_index(0);
        let _instance_addr = gc.new_instance(&InstanceData {
            instance_id: 1,
            module_addr: GcRef(0),
            globals: vec![global],
            funcs: Vec::new(),
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
        let result = unsafe { op_global_get4(program.as_ptr(), &mut ctx) };
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
            facade.pop_u32()
        };
        assert_eq!(value, 0x1122_3344);
        assert_eq!(ctx.pending_len(), 0);
        assert_eq!(ctx.cont(), unsafe { program.as_ptr().add(1) });
    }
}
