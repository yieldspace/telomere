use super::*;
use vstd::prelude::*;

verus! {

#[allow(dead_code)]
pub(crate) enum LocalStepWitnessParts {
    Drop { size: nat, next_cont: nat },
    Select { size: nat, cond: u32, next_cont: nat },
    Get { local_addr: nat, size: nat, next_cont: nat },
    Set { local_addr: nat, size: nat, next_cont: nat },
    Tee { local_addr: nat, size: nat, next_cont: nat },
}

#[allow(dead_code)]
pub(crate) open spec fn local_drop_witness_for_handler(
    size: nat,
    next_cont: nat,
) -> LocalStepWitnessParts {
    LocalStepWitnessParts::Drop { size, next_cont }
}

#[allow(dead_code)]
pub(crate) open spec fn local_select_witness_for_handler(
    size: nat,
    cond: u32,
    next_cont: nat,
) -> LocalStepWitnessParts {
    LocalStepWitnessParts::Select {
        size,
        cond,
        next_cont,
    }
}

#[allow(dead_code)]
pub(crate) open spec fn local_get_witness_for_handler(
    local_addr: nat,
    size: nat,
    next_cont: nat,
) -> LocalStepWitnessParts {
    LocalStepWitnessParts::Get {
        local_addr,
        size,
        next_cont,
    }
}

#[allow(dead_code)]
pub(crate) open spec fn local_set_witness_for_handler(
    local_addr: nat,
    size: nat,
    next_cont: nat,
) -> LocalStepWitnessParts {
    LocalStepWitnessParts::Set {
        local_addr,
        size,
        next_cont,
    }
}

#[allow(dead_code)]
pub(crate) open spec fn local_tee_witness_for_handler(
    local_addr: nat,
    size: nat,
    next_cont: nat,
) -> LocalStepWitnessParts {
    LocalStepWitnessParts::Tee {
        local_addr,
        size,
        next_cont,
    }
}

pub(crate) open spec fn local_step_from_witness_parts(
    witness: LocalStepWitnessParts,
) -> crate::common::formal::LocalStep {
    match witness {
        LocalStepWitnessParts::Drop { size, next_cont } => {
            crate::common::formal::LocalStep::Drop { size, next_cont }
        }
        LocalStepWitnessParts::Select {
            size,
            cond,
            next_cont,
        } => crate::common::formal::LocalStep::Select {
            size,
            cond,
            next_cont,
        },
        LocalStepWitnessParts::Get {
            local_addr,
            size,
            next_cont,
        } => crate::common::formal::LocalStep::Get {
            local_addr,
            size,
            next_cont,
        },
        LocalStepWitnessParts::Set {
            local_addr,
            size,
            next_cont,
        } => crate::common::formal::LocalStep::Set {
            local_addr,
            size,
            next_cont,
        },
        LocalStepWitnessParts::Tee {
            local_addr,
            size,
            next_cont,
        } => crate::common::formal::LocalStep::Tee {
            local_addr,
            size,
            next_cont,
        },
    }
}

pub open spec fn spec_drop_result(
    view: crate::common::formal::StackView,
    size: nat,
) -> crate::common::formal::StackView {
    crate::common::formal::stack_drop_values(view, size)
}

pub open spec fn spec_select_result(
    view: crate::common::formal::StackView,
    size: nat,
    cond: u32,
) -> crate::common::formal::StackView {
    crate::common::formal::stack_select_bytes(view, size, cond)
}

pub(crate) open spec fn local_observation_refines_spec_step(
    before: crate::common::CoreStepStateProjectionParts,
    step: crate::common::formal::LocalStep,
    after: crate::common::CoreStepStateProjectionParts,
    outcome: crate::common::formal::CoreOutcome,
) -> bool {
    crate::common::runtime_observation_refines_instr(
        before,
        crate::common::formal::CoreStepInstr::Local(step),
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
                == local_continue_cont(step)
        }
}

proof fn lemma_local_family_state_refines_spec_step(
    before: crate::common::formal::CoreStepState,
    step: crate::common::formal::LocalStep,
)
    ensures
        crate::common::formal::spec_step(
            before,
            crate::common::formal::CoreStepInstr::Local(step),
        ) == crate::common::formal::spec_step_local(before, step),
        crate::common::formal::task_id_preserved(
            before,
            crate::common::formal::spec_step_local(before, step).0,
        ),
        crate::common::formal::current_default_memory_of(
            crate::common::formal::spec_step_local(before, step).0,
        ) == crate::common::formal::current_default_memory_of(before),
        crate::common::formal::caller_default_memory_of(
            crate::common::formal::spec_step_local(before, step).0,
        ) == crate::common::formal::caller_default_memory_of(before),
        if crate::common::formal::outcome_is_trap(
            crate::common::formal::spec_step_local(before, step).1,
        ) {
            crate::common::formal::spec_step_local(before, step).0.context.cont_addr
                == before.context.cont_addr
        } else {
            crate::common::formal::spec_step_local(before, step).0.context.cont_addr
                == local_continue_cont(step)
        },
{
}

pub open spec fn local_continue_cont(step: crate::common::formal::LocalStep) -> nat {
    match step {
        crate::common::formal::LocalStep::Drop { next_cont, .. } => next_cont,
        crate::common::formal::LocalStep::Select { next_cont, .. } => next_cont,
        crate::common::formal::LocalStep::Get { next_cont, .. } => next_cont,
        crate::common::formal::LocalStep::Set { next_cont, .. } => next_cont,
        crate::common::formal::LocalStep::Tee { next_cont, .. } => next_cont,
    }
}

pub(crate) proof fn lemma_local_family_refines_spec_step(
    before: crate::common::formal::CoreStepState,
    step: crate::common::formal::LocalStep,
)
    ensures
        crate::common::formal::spec_step(
            before,
            crate::common::formal::CoreStepInstr::Local(step),
        ) == crate::common::formal::spec_step_local(before, step),
        crate::common::formal::task_id_preserved(
            before,
            crate::common::formal::spec_step_local(before, step).0,
        ),
        crate::common::formal::current_default_memory_of(
            crate::common::formal::spec_step_local(before, step).0,
        ) == crate::common::formal::current_default_memory_of(before),
        crate::common::formal::caller_default_memory_of(
            crate::common::formal::spec_step_local(before, step).0,
        ) == crate::common::formal::caller_default_memory_of(before),
        if crate::common::formal::outcome_is_trap(
            crate::common::formal::spec_step_local(before, step).1,
        ) {
            crate::common::formal::spec_step_local(before, step).0.context.cont_addr
                == before.context.cont_addr
        } else {
            crate::common::formal::spec_step_local(before, step).0.context.cont_addr
                == local_continue_cont(step)
        },
{
}

pub(crate) proof fn lemma_local_observation_refines_spec_step(
    before: crate::common::CoreStepStateProjectionParts,
    step: crate::common::formal::LocalStep,
    after: crate::common::CoreStepStateProjectionParts,
    outcome: crate::common::formal::CoreOutcome,
)
    requires
        crate::common::runtime_observation_refines_instr(
            before,
            crate::common::formal::CoreStepInstr::Local(step),
            after,
            outcome,
        ),
    ensures
        local_observation_refines_spec_step(before, step, after, outcome),
{
    lemma_local_family_refines_spec_step(
        crate::common::core_step_state_from_projection_parts(before),
        step,
    );
}

pub(crate) open spec fn local_witness_observation_refines_spec_step(
    before: crate::common::CoreStepStateProjectionParts,
    witness: LocalStepWitnessParts,
    after: crate::common::CoreStepStateProjectionParts,
    outcome: crate::common::formal::CoreOutcome,
) -> bool {
    local_observation_refines_spec_step(
        before,
        local_step_from_witness_parts(witness),
        after,
        outcome,
    )
}

pub(crate) proof fn lemma_local_witness_observation_refines_spec_step(
    before: crate::common::CoreStepStateProjectionParts,
    witness: LocalStepWitnessParts,
    after: crate::common::CoreStepStateProjectionParts,
    outcome: crate::common::formal::CoreOutcome,
)
    requires
        crate::common::runtime_observation_refines_instr(
            before,
            crate::common::formal::CoreStepInstr::Local(local_step_from_witness_parts(witness)),
            after,
            outcome,
        ),
    ensures
        local_witness_observation_refines_spec_step(before, witness, after, outcome),
{
    lemma_local_observation_refines_spec_step(
        before,
        local_step_from_witness_parts(witness),
        after,
        outcome,
    );
}

pub(crate) proof fn lemma_local_handler_refines_spec_step(
    before: crate::common::CoreStepStateProjectionParts,
    witness: LocalStepWitnessParts,
    after: crate::common::CoreStepStateProjectionParts,
    outcome: crate::common::formal::CoreOutcome,
)
    requires
        crate::common::runtime_observation_refines_instr(
            before,
            crate::common::formal::CoreStepInstr::Local(local_step_from_witness_parts(witness)),
            after,
            outcome,
        ),
    ensures
        local_witness_observation_refines_spec_step(before, witness, after, outcome),
{
    lemma_local_witness_observation_refines_spec_step(before, witness, after, outcome);
}

} // verus!

#[inline(always)]
/// Decode the local-slot address immediate for the active local instruction.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for the current handler.
unsafe fn decode_local_addr(tail_code: *const Instr) -> usize {
    (*tail_code).operand.local_addr as usize
}

#[inline(always)]
/// Decode the byte width immediate for `drop`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for the current handler.
unsafe fn decode_drop_size(tail_code: *const Instr) -> usize {
    (*tail_code).operand.drop_size as usize
}

#[inline(always)]
/// Decode the byte width immediate for `select`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for the current handler.
unsafe fn decode_select_size(tail_code: *const Instr) -> usize {
    (*tail_code).operand.select as usize
}

#[inline(always)]
unsafe fn local_get<const SIZE: usize>(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let addr = decode_local_addr(tail_code);
    let mut facade = ExecuteContextFacade::new(ctx);
    vm_try!(facade.local_get(addr, SIZE));
    trace!("op_local_get{SIZE}: {addr}");
    facade_call_next(tail_code, 1, &mut facade)
}

#[inline(always)]
unsafe fn local_set<const SIZE: usize>(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let addr = decode_local_addr(tail_code);
    let mut facade = ExecuteContextFacade::new(ctx);
    facade.local_set(addr, SIZE);
    facade_call_next(tail_code, 1, &mut facade)
}

#[inline(always)]
unsafe fn local_tee<const SIZE: usize>(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let addr = decode_local_addr(tail_code);
    let mut facade = ExecuteContextFacade::new(ctx);
    facade.local_tee(addr, SIZE);
    facade_call_next(tail_code, 1, &mut facade)
}

/// WebAssembly `drop`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> []`.
/// Traps: none.
/// Notes: Reads or writes validated locals/stack slots and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_drop(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let size = decode_drop_size(tail_code);
    trace!("op_drop: {size}");
    let mut facade = ExecuteContextFacade::new(ctx);
    facade.drop_values(size);
    facade_call_next(tail_code, 1, &mut facade)
}

/// WebAssembly `select`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs, i32] -> [value]`.
/// Traps: none.
/// Notes: Reads or writes validated locals/stack slots and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_select(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let mut facade = ExecuteContextFacade::new(ctx);
    let x = decode_select_size(tail_code);
    let cond = facade.pop::<u32>();
    trace!("op_select: {x} {cond}");
    vm_try!(facade.select(x, cond));
    facade_call_next(tail_code, 1, &mut facade)
}

/// WebAssembly `local.get`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[] -> [local]`.
/// Traps: none.
/// Notes: Reads or writes validated locals/stack slots and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_local_get4(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    local_get::<4>(tail_code, ctx)
}

/// WebAssembly `local.get`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[] -> [local]`.
/// Traps: none.
/// Notes: Reads or writes validated locals/stack slots and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_local_get8(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    local_get::<8>(tail_code, ctx)
}

/// WebAssembly `local.get`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[] -> [local]`.
/// Traps: none.
/// Notes: Reads or writes validated locals/stack slots and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_local_get16(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    local_get::<16>(tail_code, ctx)
}

/// WebAssembly `local.set`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[local] -> []`.
/// Traps: none.
/// Notes: Reads or writes validated locals/stack slots and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_local_set4(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    local_set::<4>(tail_code, ctx)
}

/// WebAssembly `local.set`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[local] -> []`.
/// Traps: none.
/// Notes: Reads or writes validated locals/stack slots and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_local_set8(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    local_set::<8>(tail_code, ctx)
}

/// WebAssembly `local.set`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[local] -> []`.
/// Traps: none.
/// Notes: Reads or writes validated locals/stack slots and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_local_set16(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    local_set::<16>(tail_code, ctx)
}

/// WebAssembly `local.tee`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[local] -> [local]`.
/// Traps: none.
/// Notes: Reads or writes validated locals/stack slots and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_local_tee4(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    local_tee::<4>(tail_code, ctx)
}

/// WebAssembly `local.tee`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[local] -> [local]`.
/// Traps: none.
/// Notes: Reads or writes validated locals/stack slots and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_local_tee8(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    local_tee::<8>(tail_code, ctx)
}

/// WebAssembly `local.tee`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[local] -> [local]`.
/// Traps: none.
/// Notes: Reads or writes validated locals/stack slots and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_local_tee16(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    local_tee::<16>(tail_code, ctx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        common::{
            stack::{CachedMemoryKind, CallFrameCache},
            store::InstanceId,
            ExecuteContext, GcRef, LocalReference, Operand, Store, StoreInner,
        },
        runtime::{memory_effect::PendingOp, scheduler::PendingOpEmitter},
    };
    use std::collections::VecDeque;

    fn frame() -> CallFrameCache {
        CallFrameCache {
            code_addr: GcRef(0),
            code_base: std::ptr::null(),
            instance: InstanceId::from_index(0),
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
    ) -> ExecuteContext<'a> {
        ExecuteContext::new(
            stack,
            LocalReference {
                local_top: 0,
                local_size: 0,
            },
            frame(),
            store,
            gc,
            PendingOpEmitter::from_parts(17, pending_effects, pending_ops),
            std::ptr::null(),
            17,
        )
    }

    unsafe fn stop_op(_tail_code: *const Instr, _ctx: &mut ExecuteContext) -> VMResult<()> {
        VMResult::Success(())
    }

    #[test]
    fn local_observation_tracks_drop_continue() {
        let store = Store::new();
        let mut gc = StoreInner::new();
        let mut pending_effects = 0;
        let mut pending_ops = VecDeque::new();
        let mut stack = Stack::new(32);
        stack.push_u32(10).unwrap();
        stack.push_u32(20).unwrap();

        let program = [
            Instr {
                operand: Operand { drop_size: 4 },
            },
            Instr { op: stop_op },
        ];
        let mut ctx = test_context(
            &mut stack,
            &store,
            &mut gc,
            &mut pending_effects,
            &mut pending_ops,
        );

        let pending_before = ctx.pending_len();
        let result = unsafe { op_drop(program.as_ptr(), &mut ctx) };
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
        assert_eq!(value, 10);
        assert_eq!(ctx.pending_len(), 0);
        assert_eq!(ctx.cont(), unsafe { program.as_ptr().add(1) });
    }
}
