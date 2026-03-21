use super::*;
use vstd::prelude::*;

verus! {

pub open spec fn spec_global_get_result(global: crate::common::formal::GlobalView) -> Seq<u8> {
    crate::common::formal::global_get_bytes(global)
}

pub open spec fn spec_global_set_result(
    global: crate::common::formal::GlobalView,
    bytes: Seq<u8>,
) -> crate::common::formal::GlobalView {
    crate::common::formal::global_set_bytes(global, bytes)
}

pub open spec fn global_continue_cont(step: crate::common::formal::GlobalStep) -> nat {
    match step {
        crate::common::formal::GlobalStep::Get { next_cont, .. } => next_cont,
        crate::common::formal::GlobalStep::Set { next_cont, .. } => next_cont,
    }
}

pub proof fn lemma_global_family_refines_spec_step(
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

#[allow(dead_code)]
#[inline(always)]
fn global_index(idx: usize) -> (result: usize)
    ensures
        result == idx,
{
    idx
}

} // verus!

#[allow(dead_code)]
#[inline(always)]
unsafe fn global_addr(tail_code: *const Instr, facade: &ExecuteContextFacade<'_, '_>) -> GcRef {
    let idx = global_index((*tail_code).operand.u32 as usize);
    facade.global_addr(idx)
}

#[inline(always)]
unsafe fn global_get<const SIZE: usize>(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let mut facade = ExecuteContextFacade::new(ctx);
    trace!("op_global_get{SIZE}: {:?}", global_addr(tail_code, &facade));
    vm_try!(facade.push_global_bytes::<SIZE>((*tail_code).operand.u32 as usize));
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
    facade.write_global_bytes((*tail_code).operand.u32 as usize, value);
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
