use super::*;
use crate::common::{BlockReturn, LoopParam};
use vstd::prelude::*;

verus! {

#[allow(dead_code)]
pub(crate) enum ControlStepWitnessParts {
    SetCont { cont_addr: nat },
    ConditionalCont { taken: bool, true_addr: nat, false_addr: nat },
    BlockReturn { block_stack_top: nat, return_size: nat, cont_addr: nat },
    FunctionReturn { return_size: nat },
    Trap { code: crate::common::formal::TrapCode },
}

#[allow(dead_code)]
pub(crate) open spec fn control_set_cont_witness_for_handler(
    cont_addr: nat,
) -> ControlStepWitnessParts {
    ControlStepWitnessParts::SetCont { cont_addr }
}

#[allow(dead_code)]
pub(crate) open spec fn control_conditional_cont_witness_for_handler(
    taken: bool,
    true_addr: nat,
    false_addr: nat,
) -> ControlStepWitnessParts {
    ControlStepWitnessParts::ConditionalCont {
        taken,
        true_addr,
        false_addr,
    }
}

#[allow(dead_code)]
pub(crate) open spec fn control_block_return_witness_for_handler(
    block_stack_top: nat,
    return_size: nat,
    cont_addr: nat,
) -> ControlStepWitnessParts {
    ControlStepWitnessParts::BlockReturn {
        block_stack_top,
        return_size,
        cont_addr,
    }
}

#[allow(dead_code)]
pub(crate) open spec fn control_function_return_witness_for_handler(
    return_size: nat,
) -> ControlStepWitnessParts {
    ControlStepWitnessParts::FunctionReturn { return_size }
}

#[allow(dead_code)]
pub(crate) open spec fn control_trap_witness_for_handler(
    code: crate::common::formal::TrapCode,
) -> ControlStepWitnessParts {
    ControlStepWitnessParts::Trap { code }
}

pub(crate) open spec fn control_step_from_witness_parts(
    witness: ControlStepWitnessParts,
) -> crate::common::formal::ControlStep {
    match witness {
        ControlStepWitnessParts::SetCont { cont_addr } => {
            crate::common::formal::ControlStep::SetCont { cont_addr }
        }
        ControlStepWitnessParts::ConditionalCont {
            taken,
            true_addr,
            false_addr,
        } => crate::common::formal::ControlStep::ConditionalCont {
            taken,
            true_addr,
            false_addr,
        },
        ControlStepWitnessParts::BlockReturn {
            block_stack_top,
            return_size,
            cont_addr,
        } => crate::common::formal::ControlStep::BlockReturn {
            block_stack_top,
            return_size,
            cont_addr,
        },
        ControlStepWitnessParts::FunctionReturn { return_size } => {
            crate::common::formal::ControlStep::FunctionReturn { return_size }
        }
        ControlStepWitnessParts::Trap { code } => crate::common::formal::ControlStep::Trap { code },
    }
}

pub open spec fn spec_branch_target(taken: bool, branch_addr: nat, fallthrough_addr: nat) -> nat {
    if taken { branch_addr } else { fallthrough_addr }
}

pub open spec fn spec_branch_outcome() -> crate::common::formal::CoreOutcome {
    crate::common::formal::outcome_continue()
}

pub(crate) open spec fn control_observation_refines_spec_step(
    before: crate::common::CoreStepStateProjectionParts,
    step: crate::common::formal::ControlStep,
    after: crate::common::CoreStepStateProjectionParts,
    outcome: crate::common::formal::CoreOutcome,
) -> bool {
    crate::common::runtime_observation_refines_instr(
        before,
        crate::common::formal::CoreStepInstr::Control(step),
        after,
        outcome,
    ) && crate::common::observation_task_id_preserved(before, after)
        && if crate::common::formal::outcome_is_trap(outcome) {
            crate::common::core_step_state_from_projection_parts(after).context.cont_addr
                == crate::common::core_step_state_from_projection_parts(before)
                    .context
                    .cont_addr
        } else {
            crate::common::core_step_state_from_projection_parts(after).context.cont_addr
                == control_continue_cont(
                    crate::common::core_step_state_from_projection_parts(before),
                    step,
                )
        }
}

proof fn lemma_control_family_state_refines_spec_step(
    before: crate::common::formal::CoreStepState,
    step: crate::common::formal::ControlStep,
)
    ensures
        crate::common::formal::spec_step(
            before,
            crate::common::formal::CoreStepInstr::Control(step),
        ) == crate::common::formal::spec_step_control(before, step),
        crate::common::formal::task_id_preserved(
            before,
            crate::common::formal::spec_step_control(before, step).0,
        ),
        if crate::common::formal::outcome_is_trap(
            crate::common::formal::spec_step_control(before, step).1,
        ) {
            crate::common::formal::spec_step_control(before, step).0.context.cont_addr
                == before.context.cont_addr
        } else {
            crate::common::formal::spec_step_control(before, step).0.context.cont_addr
                == control_continue_cont(before, step)
        },
{
}

pub open spec fn control_continue_cont(
    before: crate::common::formal::CoreStepState,
    step: crate::common::formal::ControlStep,
) -> nat {
    match step {
        crate::common::formal::ControlStep::SetCont { cont_addr } => cont_addr,
        crate::common::formal::ControlStep::ConditionalCont {
            taken,
            true_addr,
            false_addr,
        } => {
            if taken { true_addr } else { false_addr }
        }
        crate::common::formal::ControlStep::BlockReturn { cont_addr, .. } => cont_addr,
        crate::common::formal::ControlStep::FunctionReturn { .. } => {
            before.context.current_frame.return_pc as nat
        }
        crate::common::formal::ControlStep::Trap { .. } => before.context.cont_addr,
    }
}

pub(crate) proof fn lemma_control_family_refines_spec_step(
    before: crate::common::formal::CoreStepState,
    step: crate::common::formal::ControlStep,
)
    ensures
        crate::common::formal::spec_step(
            before,
            crate::common::formal::CoreStepInstr::Control(step),
        ) == crate::common::formal::spec_step_control(before, step),
        crate::common::formal::task_id_preserved(
            before,
            crate::common::formal::spec_step_control(before, step).0,
        ),
        if crate::common::formal::outcome_is_trap(
            crate::common::formal::spec_step_control(before, step).1,
        ) {
            crate::common::formal::spec_step_control(before, step).0.context.cont_addr
                == before.context.cont_addr
        } else {
            crate::common::formal::spec_step_control(before, step).0.context.cont_addr
                == control_continue_cont(before, step)
        },
{
}

pub(crate) proof fn lemma_control_observation_refines_spec_step(
    before: crate::common::CoreStepStateProjectionParts,
    step: crate::common::formal::ControlStep,
    after: crate::common::CoreStepStateProjectionParts,
    outcome: crate::common::formal::CoreOutcome,
)
    requires
        crate::common::runtime_observation_refines_instr(
            before,
            crate::common::formal::CoreStepInstr::Control(step),
            after,
            outcome,
        ),
    ensures
        control_observation_refines_spec_step(before, step, after, outcome),
{
    lemma_control_family_refines_spec_step(
        crate::common::core_step_state_from_projection_parts(before),
        step,
    );
}

pub(crate) open spec fn control_witness_observation_refines_spec_step(
    before: crate::common::CoreStepStateProjectionParts,
    witness: ControlStepWitnessParts,
    after: crate::common::CoreStepStateProjectionParts,
    outcome: crate::common::formal::CoreOutcome,
) -> bool {
    control_observation_refines_spec_step(
        before,
        control_step_from_witness_parts(witness),
        after,
        outcome,
    )
}

pub(crate) proof fn lemma_control_witness_observation_refines_spec_step(
    before: crate::common::CoreStepStateProjectionParts,
    witness: ControlStepWitnessParts,
    after: crate::common::CoreStepStateProjectionParts,
    outcome: crate::common::formal::CoreOutcome,
)
    requires
        crate::common::runtime_observation_refines_instr(
            before,
            crate::common::formal::CoreStepInstr::Control(control_step_from_witness_parts(witness)),
            after,
            outcome,
        ),
    ensures
        control_witness_observation_refines_spec_step(before, witness, after, outcome),
{
    lemma_control_observation_refines_spec_step(
        before,
        control_step_from_witness_parts(witness),
        after,
        outcome,
    );
}

pub(crate) proof fn lemma_control_handler_refines_spec_step(
    before: crate::common::CoreStepStateProjectionParts,
    witness: ControlStepWitnessParts,
    after: crate::common::CoreStepStateProjectionParts,
    outcome: crate::common::formal::CoreOutcome,
)
    requires
        crate::common::runtime_observation_refines_instr(
            before,
            crate::common::formal::CoreStepInstr::Control(control_step_from_witness_parts(witness)),
            after,
            outcome,
        ),
    ensures
        control_witness_observation_refines_spec_step(before, witness, after, outcome),
{
    lemma_control_witness_observation_refines_spec_step(before, witness, after, outcome);
}

} // verus!

#[inline(always)]
unsafe fn jump_target(facade: &ExecuteContextFacade<'_, '_>, addr: u32) -> *const Instr {
    facade.resolve_branch_target(addr)
}

#[inline(always)]
unsafe fn tail_jump(
    ptr: *const Instr,
    skip: isize,
    facade: &mut ExecuteContextFacade<'_, '_>,
) -> VMResult<()> {
    facade_call_next(ptr, skip, facade)
}

#[inline(always)]
unsafe fn conditional_jump_target(
    tail_code: *const Instr,
    facade: &ExecuteContextFacade<'_, '_>,
    taken: bool,
    addr: u32,
) -> *const Instr {
    if taken {
        jump_target(facade, addr)
    } else {
        tail_code.offset(1)
    }
}

#[inline(always)]
/// Decode the branch target immediate for the active control instruction.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for the current handler.
unsafe fn decode_jump_addr(tail_code: *const Instr) -> u32 {
    (*tail_code).operand.jump_addr
}

#[inline(always)]
/// Decode the `br_table` entry count immediate.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for the current handler.
unsafe fn decode_br_table_size(tail_code: *const Instr) -> u32 {
    (*tail_code).operand.u32
}

#[inline(always)]
/// Decode the loop frame metadata payload.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for the current handler.
unsafe fn decode_loop_param(tail_code: *const Instr) -> LoopParam {
    (*tail_code).operand.loop_param
}

#[inline(always)]
/// Decode the `drop` byte width used by control fast paths.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for the current handler.
unsafe fn decode_drop_size(tail_code: *const Instr) -> usize {
    (*tail_code).operand.drop_size as usize
}

#[inline(always)]
/// Decode the packed block-return trailer payload.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for the current handler.
unsafe fn decode_block_return(tail_code: *const Instr) -> BlockReturn {
    (*tail_code).operand.block_return
}

#[inline(always)]
/// Decode a `br_table` target at the given slot.
///
/// # Safety
/// - `tail_code` must point to the decoded `br_table` instruction for the current handler.
unsafe fn decode_br_table_target(tail_code: *const Instr, slot: u32) -> u32 {
    (*tail_code.offset((slot + 1) as isize)).operand.jump_addr
}

#[inline(always)]
unsafe fn block_return_continue(
    tail_code: *const Instr,
    facade: &mut ExecuteContextFacade<'_, '_>,
    stack_top: usize,
    return_size: usize,
) -> VMResult<()> {
    facade.block_return(stack_top, return_size);
    tail_jump(tail_code, 1, facade)
}

/// WebAssembly `return`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[results] -> []`.
/// Traps: unreachable always traps; other handlers follow validated control-flow invariants.
/// Notes: Computes the next continuation in the direct-threaded interpreter and tail-dispatches without keeping temporary guards alive.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_return(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let ptr = {
        let facade = ExecuteContextFacade::new(ctx);
        let addr = decode_jump_addr(tail_code);
        trace!("op_return: {addr}");
        jump_target(&facade, addr)
    };
    call_code(ptr, ctx)
}

/// WebAssembly `end`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[] -> []`.
/// Traps: unreachable always traps; other handlers follow validated control-flow invariants.
/// Notes: Computes the next continuation in the direct-threaded interpreter and tail-dispatches without keeping temporary guards alive.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_end(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    trace!("op_end");
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `br`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[] -> []`.
/// Traps: unreachable always traps; other handlers follow validated control-flow invariants.
/// Notes: Computes the next continuation in the direct-threaded interpreter and tail-dispatches without keeping temporary guards alive.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_br(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let mut facade = ExecuteContextFacade::new(ctx);
    let addr = decode_jump_addr(tail_code);
    trace!("op_br: {addr}");
    tail_jump(jump_target(&facade, addr), 0, &mut facade)
}

/// WebAssembly `else`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[] -> []`.
/// Traps: unreachable always traps; other handlers follow validated control-flow invariants.
/// Notes: Computes the next continuation in the direct-threaded interpreter and tail-dispatches without keeping temporary guards alive.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_else(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let mut facade = ExecuteContextFacade::new(ctx);
    trace!("op_else");

    let addr = decode_jump_addr(tail_code);
    tail_jump(jump_target(&facade, addr), 1, &mut facade)
}

/// WebAssembly `br_if`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[..., i32] -> [...]`.
/// Traps: unreachable always traps; other handlers follow validated control-flow invariants.
/// Notes: Computes the next continuation in the direct-threaded interpreter and tail-dispatches without keeping temporary guards alive.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_br_if(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let mut facade = ExecuteContextFacade::new(ctx);
    let taken = facade.pop_branch_cond();
    trace!("op_br_if: {}", taken as u32);
    let ptr = conditional_jump_target(tail_code, &facade, taken, decode_jump_addr(tail_code));
    tail_jump(ptr, 0, &mut facade)
}

/// WebAssembly `br_table`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[..., i32] -> [...]`.
/// Traps: unreachable always traps; other handlers follow validated control-flow invariants.
/// Notes: Computes the next continuation in the direct-threaded interpreter and tail-dispatches without keeping temporary guards alive.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_br_table(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let mut facade = ExecuteContextFacade::new(ctx);
    let index = facade.pop::<u32>();
    let table_size = decode_br_table_size(tail_code);

    let addr = if index < table_size {
        decode_br_table_target(tail_code, index)
    } else {
        decode_br_table_target(tail_code, table_size)
    };
    trace!(
        "op_br_table: index={} table_size={} => addr={}",
        index,
        table_size,
        addr
    );
    tail_jump(jump_target(&facade, addr), 0, &mut facade)
}

/// WebAssembly `loop`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[params] -> [params]`.
/// Traps: unreachable always traps; other handlers follow validated control-flow invariants.
/// Notes: Computes the next continuation in the direct-threaded interpreter and tail-dispatches without keeping temporary guards alive.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_loop(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let mut facade = ExecuteContextFacade::new(ctx);
    trace!("op_loop: {}", decode_jump_addr(tail_code));

    let loop_param = decode_loop_param(tail_code);
    block_return_continue(
        tail_code,
        &mut facade,
        loop_param.stack_top as usize,
        loop_param.param_size as usize,
    )
}

/// WebAssembly `if`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[i32] -> []`.
/// Traps: unreachable always traps; other handlers follow validated control-flow invariants.
/// Notes: Computes the next continuation in the direct-threaded interpreter and tail-dispatches without keeping temporary guards alive.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_if(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let mut facade = ExecuteContextFacade::new(ctx);
    let else_addr = decode_jump_addr(tail_code);
    let taken = facade.pop_branch_cond();
    trace!("op_if: {else_addr} {}", taken as u32);

    let ptr = conditional_jump_target(tail_code, &facade, !taken, else_addr);
    tail_jump(ptr, 0, &mut facade)
}

/// Telomere internal `special_function_return` trampoline.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `internal runtime continuation`.
/// Traps: unreachable always traps; other handlers follow validated control-flow invariants.
/// Notes: Computes the next continuation in the direct-threaded interpreter and tail-dispatches without keeping temporary guards alive.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn special_function_return(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    trace!("function return");
    let tail_code = ctx.function_return(decode_drop_size(tail_code));
    call_next(tail_code, 0, ctx)
}

/// Telomere internal `special_block_return` trampoline.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `internal runtime continuation`.
/// Traps: unreachable always traps; other handlers follow validated control-flow invariants.
/// Notes: Computes the next continuation in the direct-threaded interpreter and tail-dispatches without keeping temporary guards alive.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn special_block_return(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let mut facade = ExecuteContextFacade::new(ctx);
    let block_return = decode_block_return(tail_code);
    trace!(
        "block return: {:?} {:?} {:?}",
        facade.local_reference(),
        &block_return,
        facade.stack_ref()
    );
    facade.apply_block_return(
        block_return.stack_top as usize,
        block_return.return_size as usize,
    );
    trace!("stack: {:?}", facade.stack_ref());
    tail_jump(tail_code, 1, &mut facade)
}

/// Telomere internal `special_function_vm_end` trampoline.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `internal runtime continuation`.
/// Traps: unreachable always traps; other handlers follow validated control-flow invariants.
/// Notes: Computes the next continuation in the direct-threaded interpreter and tail-dispatches without keeping temporary guards alive.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn special_function_vm_end(
    _tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    ExecuteContextFacade::new(ctx).clear_continuation();
    VMResult::Success(())
}

/// WebAssembly `unreachable`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[] -> trap`.
/// Traps: unreachable always traps; other handlers follow validated control-flow invariants.
/// Notes: Computes the next continuation in the direct-threaded interpreter and tail-dispatches without keeping temporary guards alive.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_unreachable(_tail_code: *const Instr, _ctx: &mut ExecuteContext) -> VMResult<()> {
    VMResult::Unreachable
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        common::{
            stack::CachedMemoryKind, store, BlockReturn, CallFrameCache, GcRef, Operand, StoreInner,
        },
        runtime::{memory_effect::PendingOp, scheduler::PendingOpEmitter},
    };
    use std::collections::VecDeque;

    fn frame(code_base: *const Instr) -> CallFrameCache {
        CallFrameCache {
            code_addr: GcRef(0),
            code_base,
            instance: store::InstanceId::from_index(0),
            memory0_kind: CachedMemoryKind::None,
            memory0_raw: 0,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn test_context<'a>(
        stack: &'a mut Stack,
        store: &'a Store,
        gc: &'a mut StoreInner,
        pending_effects: &'a mut u32,
        pending_ops: &'a mut VecDeque<PendingOp>,
        current_frame: CallFrameCache,
        local_reference: LocalReference,
        cont: *const Instr,
        task_id: u32,
    ) -> ExecuteContext<'a> {
        ExecuteContext::new(
            stack,
            local_reference,
            current_frame,
            store,
            gc,
            PendingOpEmitter::from_parts(task_id, pending_effects, pending_ops),
            cont,
            task_id,
        )
    }

    unsafe fn push_then_marker(_tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
        ExecuteContextFacade::new(ctx).push_u32(11)
    }

    unsafe fn push_else_marker(_tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
        ExecuteContextFacade::new(ctx).push_u32(22)
    }

    unsafe fn push_default_marker(
        _tail_code: *const Instr,
        ctx: &mut ExecuteContext,
    ) -> VMResult<()> {
        ExecuteContextFacade::new(ctx).push_u32(33)
    }

    unsafe fn push_resume_marker(
        _tail_code: *const Instr,
        ctx: &mut ExecuteContext,
    ) -> VMResult<()> {
        ExecuteContextFacade::new(ctx).push_u32(55)
    }

    #[test]
    fn op_br_if_routes_taken_and_fallthrough_via_facade_targets() {
        let store = Store::new();
        let mut gc = StoreInner::new();
        let mut pending_effects = 0;
        let mut pending_ops = VecDeque::new();

        let code = [
            Instr { op: op_unreachable },
            Instr {
                operand: Operand { jump_addr: 3 },
            },
            Instr {
                op: push_else_marker,
            },
            Instr {
                op: push_then_marker,
            },
        ];

        let mut stack = Stack::new(32);
        stack.push_u32(1).unwrap();
        let mut ctx = test_context(
            &mut stack,
            &store,
            &mut gc,
            &mut pending_effects,
            &mut pending_ops,
            frame(code.as_ptr()),
            LocalReference {
                local_top: 0,
                local_size: 0,
            },
            std::ptr::null(),
            1,
        );

        unsafe {
            op_br_if(code.as_ptr().add(1), &mut ctx).unwrap();
        }
        assert_eq!(ctx.cont(), unsafe { code.as_ptr().add(3) });
        assert_eq!(ctx.stack_mut().pop_u32(), 11);

        ctx.stack_mut().push_u32(0).unwrap();
        unsafe {
            op_br_if(code.as_ptr().add(1), &mut ctx).unwrap();
        }
        assert_eq!(ctx.cont(), unsafe { code.as_ptr().add(2) });
        assert_eq!(ctx.stack_mut().pop_u32(), 22);
    }

    #[test]
    fn control_observation_tracks_branch_continue() {
        let store = Store::new();
        let mut gc = StoreInner::new();
        let mut pending_effects = 0;
        let mut pending_ops = VecDeque::new();
        let if_code = [
            Instr { op: op_unreachable },
            Instr {
                operand: Operand { jump_addr: 3 },
            },
            Instr {
                op: push_then_marker,
            },
            Instr {
                op: push_else_marker,
            },
        ];
        let mut stack = Stack::new(32);
        stack.push_u32(1).unwrap();
        let mut ctx = test_context(
            &mut stack,
            &store,
            &mut gc,
            &mut pending_effects,
            &mut pending_ops,
            frame(if_code.as_ptr()),
            LocalReference {
                local_top: 0,
                local_size: 0,
            },
            if_code.as_ptr(),
            13,
        );

        let pending_before = ctx.pending_len();
        let result = unsafe { op_if(if_code.as_ptr().add(1), &mut ctx) };
        let outcome = crate::common::formal::core_outcome_from_vm_result_with_pending(
            &result,
            pending_before,
            ctx.pending_len(),
            ctx.pending_code_delta(pending_before).unwrap_or(None),
        )
        .unwrap();

        assert_eq!(outcome, crate::common::formal::CoreOutcome::Continue);
        assert_eq!(ctx.cont(), unsafe { if_code.as_ptr().add(2) });
        assert_eq!(ctx.stack_mut().pop_u32(), 11);
        assert_eq!(ctx.pending_len(), 0);
    }

    #[test]
    fn op_br_table_and_if_preserve_branch_dispatch_contract() {
        let store = Store::new();
        let mut gc = StoreInner::new();
        let mut pending_effects = 0;
        let mut pending_ops = VecDeque::new();
        let mut br_table_stack = Stack::new(32);

        let br_table_code = [
            Instr { op: op_unreachable },
            Instr {
                operand: Operand { u32: 2 },
            },
            Instr {
                operand: Operand { jump_addr: 5 },
            },
            Instr {
                operand: Operand { jump_addr: 6 },
            },
            Instr {
                operand: Operand { jump_addr: 7 },
            },
            Instr {
                op: push_then_marker,
            },
            Instr {
                op: push_else_marker,
            },
            Instr {
                op: push_default_marker,
            },
        ];

        br_table_stack.push_u32(3).unwrap();
        let mut br_table_ctx = test_context(
            &mut br_table_stack,
            &store,
            &mut gc,
            &mut pending_effects,
            &mut pending_ops,
            frame(br_table_code.as_ptr()),
            LocalReference {
                local_top: 0,
                local_size: 0,
            },
            std::ptr::null(),
            2,
        );
        unsafe {
            op_br_table(br_table_code.as_ptr().add(1), &mut br_table_ctx).unwrap();
        }
        assert_eq!(br_table_ctx.cont(), unsafe {
            br_table_code.as_ptr().add(7)
        });
        assert_eq!(br_table_ctx.stack_mut().pop_u32(), 33);

        let if_code = [
            Instr { op: op_unreachable },
            Instr {
                operand: Operand { jump_addr: 3 },
            },
            Instr {
                op: push_then_marker,
            },
            Instr {
                op: push_else_marker,
            },
        ];
        let mut if_stack = Stack::new(32);
        if_stack.push_u32(1).unwrap();
        let mut if_ctx = test_context(
            &mut if_stack,
            &store,
            &mut gc,
            &mut pending_effects,
            &mut pending_ops,
            frame(if_code.as_ptr()),
            LocalReference {
                local_top: 0,
                local_size: 0,
            },
            std::ptr::null(),
            12,
        );
        unsafe {
            op_if(if_code.as_ptr().add(1), &mut if_ctx).unwrap();
        }
        assert_eq!(if_ctx.cont(), unsafe { if_code.as_ptr().add(2) });
        assert_eq!(if_ctx.stack_mut().pop_u32(), 11);

        if_ctx.stack_mut().push_u32(0).unwrap();
        if_ctx.set_cont(std::ptr::null());
        unsafe {
            op_if(if_code.as_ptr().add(1), &mut if_ctx).unwrap();
        }
        assert_eq!(if_ctx.cont(), unsafe { if_code.as_ptr().add(3) });
        assert_eq!(if_ctx.stack_mut().pop_u32(), 22);
    }

    #[test]
    fn special_function_return_restores_previous_frame_and_resume_pc() {
        let store = Store::new();
        let mut gc = StoreInner::new();
        let mut pending_effects = 0;
        let mut pending_ops = VecDeque::new();
        let mut stack = Stack::new(64);
        let empty = LocalReference {
            local_top: 0,
            local_size: 0,
        };
        let resume_code = [Instr {
            op: push_resume_marker,
        }];
        let callee = stack
            .function_call(
                0,
                0,
                frame(std::ptr::null()),
                empty,
                resume_code.as_ptr(),
                &gc,
            )
            .unwrap();
        stack.push_u32(0xfeed_beef).unwrap();

        let mut ctx = test_context(
            &mut stack,
            &store,
            &mut gc,
            &mut pending_effects,
            &mut pending_ops,
            frame(std::ptr::null()),
            callee,
            std::ptr::null(),
            3,
        );
        let return_tail = [Instr {
            operand: Operand { drop_size: 4 },
        }];

        unsafe {
            special_function_return(return_tail.as_ptr(), &mut ctx).unwrap();
        }

        assert_eq!(ctx.local_reference(), empty);
        assert_eq!(ctx.cont(), resume_code.as_ptr());
        assert_eq!(ctx.stack_mut().pop_u32(), 55);
        assert_eq!(ctx.stack_mut().pop_u32(), 0xfeed_beef);
    }

    #[test]
    fn special_block_return_and_vm_end_use_facade_control_helpers() {
        let store = Store::new();
        let mut gc = StoreInner::new();
        let mut pending_effects = 0;
        let mut pending_ops = VecDeque::new();
        let mut stack = Stack::new(64);
        let empty = LocalReference {
            local_top: 0,
            local_size: 0,
        };
        let locals = stack
            .function_call(0, 0, frame(std::ptr::null()), empty, std::ptr::null(), &gc)
            .unwrap();
        stack.push_u32(99).unwrap();

        let mut ctx = test_context(
            &mut stack,
            &store,
            &mut gc,
            &mut pending_effects,
            &mut pending_ops,
            frame(std::ptr::null()),
            locals,
            std::ptr::dangling(),
            4,
        );
        let block_tail = [
            Instr {
                operand: Operand {
                    block_return: BlockReturn {
                        stack_top: 0,
                        return_size: 4,
                    },
                },
            },
            Instr {
                op: push_then_marker,
            },
        ];

        unsafe {
            special_block_return(block_tail.as_ptr(), &mut ctx).unwrap();
        }

        assert_eq!(ctx.cont(), unsafe { block_tail.as_ptr().add(1) });
        assert_eq!(ctx.stack_mut().pop_u32(), 11);
        assert_eq!(ctx.stack_mut().pop_u32(), 99);

        unsafe {
            special_function_vm_end(std::ptr::null(), &mut ctx).unwrap();
        }
        assert!(ctx.cont().is_null());
    }
}
