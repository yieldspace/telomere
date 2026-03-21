use super::*;
use vstd::prelude::*;

verus! {

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

pub open spec fn local_continue_cont(step: crate::common::formal::LocalStep) -> nat {
    match step {
        crate::common::formal::LocalStep::Drop { next_cont, .. } => next_cont,
        crate::common::formal::LocalStep::Select { next_cont, .. } => next_cont,
        crate::common::formal::LocalStep::Get { next_cont, .. } => next_cont,
        crate::common::formal::LocalStep::Set { next_cont, .. } => next_cont,
        crate::common::formal::LocalStep::Tee { next_cont, .. } => next_cont,
    }
}

pub proof fn lemma_local_family_refines_spec_step(
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

} // verus!

#[inline(always)]
unsafe fn local_get<const SIZE: usize>(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let addr = (*tail_code).operand.local_addr as usize;
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
    let addr = (*tail_code).operand.local_addr as usize;
    let mut facade = ExecuteContextFacade::new(ctx);
    facade.local_set(addr, SIZE);
    facade_call_next(tail_code, 1, &mut facade)
}

#[inline(always)]
unsafe fn local_tee<const SIZE: usize>(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let addr = (*tail_code).operand.local_addr as usize;
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
    let size = (*tail_code).operand.drop_size as usize;
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
    let x = (*tail_code).operand.select as usize;
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
