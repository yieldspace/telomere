use super::*;
use vstd::prelude::*;

verus! {

#[allow(dead_code)]
pub(crate) enum TableStepWitnessParts {
    Get {
        table_id: u32,
        index: nat,
        next_cont: nat,
    },
    Set {
        table_id: u32,
        index: nat,
        value: u32,
        next_cont: nat,
    },
    Size {
        table_id: u32,
        next_cont: nat,
    },
    Grow {
        table_id: u32,
        len: nat,
        value: u32,
        next_cont: nat,
    },
    Fill {
        table_id: u32,
        index: nat,
        len: nat,
        value: u32,
        next_cont: nat,
    },
    Copy {
        dst_table_id: u32,
        src_table_id: u32,
        dst: nat,
        src: nat,
        len: nat,
        next_cont: nat,
    },
    Init {
        table_id: u32,
        elem_segment_id: u32,
        dst: nat,
        src: nat,
        len: nat,
        next_cont: nat,
    },
    ElemDrop {
        elem_segment_id: u32,
        next_cont: nat,
    },
}

#[allow(dead_code)]
pub(crate) open spec fn table_get_witness_for_handler(
    table_id: u32,
    index: nat,
    next_cont: nat,
) -> TableStepWitnessParts {
    TableStepWitnessParts::Get {
        table_id,
        index,
        next_cont,
    }
}

#[allow(dead_code)]
pub(crate) open spec fn table_set_witness_for_handler(
    table_id: u32,
    index: nat,
    value: u32,
    next_cont: nat,
) -> TableStepWitnessParts {
    TableStepWitnessParts::Set {
        table_id,
        index,
        value,
        next_cont,
    }
}

#[allow(dead_code)]
pub(crate) open spec fn table_size_witness_for_handler(
    table_id: u32,
    next_cont: nat,
) -> TableStepWitnessParts {
    TableStepWitnessParts::Size {
        table_id,
        next_cont,
    }
}

#[allow(dead_code)]
pub(crate) open spec fn table_grow_witness_for_handler(
    table_id: u32,
    len: nat,
    value: u32,
    next_cont: nat,
) -> TableStepWitnessParts {
    TableStepWitnessParts::Grow {
        table_id,
        len,
        value,
        next_cont,
    }
}

#[allow(dead_code)]
pub(crate) open spec fn table_fill_witness_for_handler(
    table_id: u32,
    index: nat,
    len: nat,
    value: u32,
    next_cont: nat,
) -> TableStepWitnessParts {
    TableStepWitnessParts::Fill {
        table_id,
        index,
        len,
        value,
        next_cont,
    }
}

#[allow(dead_code)]
pub(crate) open spec fn table_copy_witness_for_handler(
    dst_table_id: u32,
    src_table_id: u32,
    dst: nat,
    src: nat,
    len: nat,
    next_cont: nat,
) -> TableStepWitnessParts {
    TableStepWitnessParts::Copy {
        dst_table_id,
        src_table_id,
        dst,
        src,
        len,
        next_cont,
    }
}

#[allow(dead_code)]
pub(crate) open spec fn table_init_witness_for_handler(
    table_id: u32,
    elem_segment_id: u32,
    dst: nat,
    src: nat,
    len: nat,
    next_cont: nat,
) -> TableStepWitnessParts {
    TableStepWitnessParts::Init {
        table_id,
        elem_segment_id,
        dst,
        src,
        len,
        next_cont,
    }
}

#[allow(dead_code)]
pub(crate) open spec fn table_elem_drop_witness_for_handler(
    elem_segment_id: u32,
    next_cont: nat,
) -> TableStepWitnessParts {
    TableStepWitnessParts::ElemDrop {
        elem_segment_id,
        next_cont,
    }
}

pub(crate) open spec fn table_step_from_witness_parts(
    witness: TableStepWitnessParts,
) -> crate::common::formal::TableStep {
    match witness {
        TableStepWitnessParts::Get {
            table_id,
            index,
            next_cont,
        } => crate::common::formal::TableStep::Get {
            table_id: table_id as nat,
            index,
            next_cont,
        },
        TableStepWitnessParts::Set {
            table_id,
            index,
            value,
            next_cont,
        } => crate::common::formal::TableStep::Set {
            table_id: table_id as nat,
            index,
            value,
            next_cont,
        },
        TableStepWitnessParts::Size {
            table_id,
            next_cont,
        } => crate::common::formal::TableStep::Size {
            table_id: table_id as nat,
            next_cont,
        },
        TableStepWitnessParts::Grow {
            table_id,
            len,
            value,
            next_cont,
        } => crate::common::formal::TableStep::Grow {
            table_id: table_id as nat,
            len,
            value,
            next_cont,
        },
        TableStepWitnessParts::Fill {
            table_id,
            index,
            len,
            value,
            next_cont,
        } => crate::common::formal::TableStep::Fill {
            table_id: table_id as nat,
            index,
            len,
            value,
            next_cont,
        },
        TableStepWitnessParts::Copy {
            dst_table_id,
            src_table_id,
            dst,
            src,
            len,
            next_cont,
        } => crate::common::formal::TableStep::Copy {
            dst_table_id: dst_table_id as nat,
            src_table_id: src_table_id as nat,
            dst,
            src,
            len,
            next_cont,
        },
        TableStepWitnessParts::Init {
            table_id,
            elem_segment_id,
            dst,
            src,
            len,
            next_cont,
        } => crate::common::formal::TableStep::Init {
            table_id: table_id as nat,
            elem_segment_id: elem_segment_id as nat,
            dst,
            src,
            len,
            next_cont,
        },
        TableStepWitnessParts::ElemDrop {
            elem_segment_id,
            next_cont,
        } => crate::common::formal::TableStep::ElemDrop {
            elem_segment_id: elem_segment_id as nat,
            next_cont,
        },
    }
}

pub open spec fn spec_table_get_result(
    table: crate::common::formal::TableView,
    idx: nat,
) -> Option<u32> {
    crate::common::formal::table_get_result(table, idx)
}

pub open spec fn spec_table_set_result(
    table: crate::common::formal::TableView,
    idx: nat,
    value: u32,
) -> Option<crate::common::formal::TableView> {
    crate::common::formal::table_set_result(table, idx, value)
}

pub open spec fn spec_table_size_result(table: crate::common::formal::TableView) -> nat {
    crate::common::formal::table_size_result(table)
}

pub open spec fn spec_table_grow_result(
    table: crate::common::formal::TableView,
    count: nat,
    value: u32,
) -> (crate::common::formal::TableView, int) {
    crate::common::formal::table_grow_result(table, count, value)
}

pub open spec fn table_continue_cont(step: crate::common::formal::TableStep) -> nat {
    match step {
        crate::common::formal::TableStep::Get { next_cont, .. } => next_cont,
        crate::common::formal::TableStep::Set { next_cont, .. } => next_cont,
        crate::common::formal::TableStep::Size { next_cont, .. } => next_cont,
        crate::common::formal::TableStep::Grow { next_cont, .. } => next_cont,
        crate::common::formal::TableStep::Fill { next_cont, .. } => next_cont,
        crate::common::formal::TableStep::Copy { next_cont, .. } => next_cont,
        crate::common::formal::TableStep::Init { next_cont, .. } => next_cont,
        crate::common::formal::TableStep::ElemDrop { next_cont, .. } => next_cont,
    }
}

pub(crate) open spec fn table_observation_refines_spec_step(
    before: crate::common::CoreStepStateProjectionParts,
    step: crate::common::formal::TableStep,
    after: crate::common::CoreStepStateProjectionParts,
    outcome: crate::common::formal::CoreOutcome,
) -> bool {
    crate::common::runtime_observation_refines_instr(
        before,
        crate::common::formal::CoreStepInstr::Table(step),
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
                == table_continue_cont(step)
        }
}

proof fn lemma_table_family_state_refines_spec_step(
    before: crate::common::formal::CoreStepState,
    step: crate::common::formal::TableStep,
)
    ensures
        crate::common::formal::spec_step(
            before,
            crate::common::formal::CoreStepInstr::Table(step),
        ) == crate::common::formal::spec_step_table(before, step),
        crate::common::formal::task_id_preserved(
            before,
            crate::common::formal::spec_step_table(before, step).0,
        ),
        crate::common::formal::current_default_memory_of(
            crate::common::formal::spec_step_table(before, step).0,
        ) == crate::common::formal::current_default_memory_of(before),
        crate::common::formal::caller_default_memory_of(
            crate::common::formal::spec_step_table(before, step).0,
        ) == crate::common::formal::caller_default_memory_of(before),
        if crate::common::formal::outcome_is_trap(
            crate::common::formal::spec_step_table(before, step).1,
        ) {
            crate::common::formal::spec_step_table(before, step).0.context.cont_addr
                == before.context.cont_addr
        } else {
            crate::common::formal::spec_step_table(before, step).0.context.cont_addr
                == table_continue_cont(step)
        },
{
}

pub(crate) proof fn lemma_table_family_refines_spec_step(
    before: crate::common::formal::CoreStepState,
    step: crate::common::formal::TableStep,
)
    ensures
        crate::common::formal::spec_step(
            before,
            crate::common::formal::CoreStepInstr::Table(step),
        ) == crate::common::formal::spec_step_table(before, step),
        crate::common::formal::task_id_preserved(
            before,
            crate::common::formal::spec_step_table(before, step).0,
        ),
        crate::common::formal::current_default_memory_of(
            crate::common::formal::spec_step_table(before, step).0,
        ) == crate::common::formal::current_default_memory_of(before),
        crate::common::formal::caller_default_memory_of(
            crate::common::formal::spec_step_table(before, step).0,
        ) == crate::common::formal::caller_default_memory_of(before),
        if crate::common::formal::outcome_is_trap(
            crate::common::formal::spec_step_table(before, step).1,
        ) {
            crate::common::formal::spec_step_table(before, step).0.context.cont_addr
                == before.context.cont_addr
        } else {
            crate::common::formal::spec_step_table(before, step).0.context.cont_addr
                == table_continue_cont(step)
        },
{
}

pub(crate) proof fn lemma_table_observation_refines_spec_step(
    before: crate::common::CoreStepStateProjectionParts,
    step: crate::common::formal::TableStep,
    after: crate::common::CoreStepStateProjectionParts,
    outcome: crate::common::formal::CoreOutcome,
)
    requires
        crate::common::runtime_observation_refines_instr(
            before,
            crate::common::formal::CoreStepInstr::Table(step),
            after,
            outcome,
        ),
    ensures
        table_observation_refines_spec_step(before, step, after, outcome),
{
    lemma_table_family_refines_spec_step(
        crate::common::core_step_state_from_projection_parts(before),
        step,
    );
}

pub(crate) open spec fn table_witness_observation_refines_spec_step(
    before: crate::common::CoreStepStateProjectionParts,
    witness: TableStepWitnessParts,
    after: crate::common::CoreStepStateProjectionParts,
    outcome: crate::common::formal::CoreOutcome,
) -> bool {
    table_observation_refines_spec_step(
        before,
        table_step_from_witness_parts(witness),
        after,
        outcome,
    )
}

pub(crate) proof fn lemma_table_witness_observation_refines_spec_step(
    before: crate::common::CoreStepStateProjectionParts,
    witness: TableStepWitnessParts,
    after: crate::common::CoreStepStateProjectionParts,
    outcome: crate::common::formal::CoreOutcome,
)
    requires
        crate::common::runtime_observation_refines_instr(
            before,
            crate::common::formal::CoreStepInstr::Table(table_step_from_witness_parts(witness)),
            after,
            outcome,
        ),
    ensures
        table_witness_observation_refines_spec_step(before, witness, after, outcome),
{
    lemma_table_observation_refines_spec_step(
        before,
        table_step_from_witness_parts(witness),
        after,
        outcome,
    );
}

pub(crate) proof fn lemma_table_handler_refines_spec_step(
    before: crate::common::CoreStepStateProjectionParts,
    witness: TableStepWitnessParts,
    after: crate::common::CoreStepStateProjectionParts,
    outcome: crate::common::formal::CoreOutcome,
)
    requires
        crate::common::runtime_observation_refines_instr(
            before,
            crate::common::formal::CoreStepInstr::Table(table_step_from_witness_parts(witness)),
            after,
            outcome,
        ),
    ensures
        table_witness_observation_refines_spec_step(before, witness, after, outcome),
{
    lemma_table_witness_observation_refines_spec_step(before, witness, after, outcome);
}

} // verus!

#[inline(always)]
/// Decode the single table index immediate.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for the current handler.
unsafe fn decode_table_index(tail_code: *const Instr) -> usize {
    (*tail_code).operand.u32 as usize
}

#[inline(always)]
/// Decode the `(dst_table, src_table)` immediates for `table.copy`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for the current handler.
unsafe fn decode_table_pair_indices(tail_code: *const Instr) -> (usize, usize) {
    (
        (*tail_code).operand.u32 as usize,
        (*tail_code.offset(1)).operand.u32 as usize,
    )
}

#[inline(always)]
/// Decode the `(elem_segment, dst_table)` immediates for `table.init`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for the current handler.
unsafe fn decode_table_init_ids(tail_code: *const Instr) -> (u32, usize) {
    (
        (*tail_code).operand.u32,
        (*tail_code.offset(1)).operand.u32 as usize,
    )
}

#[inline(always)]
/// Decode the element-segment index immediate for `elem.drop`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for the current handler.
unsafe fn decode_elem_segment_id(tail_code: *const Instr) -> u32 {
    (*tail_code).operand.u32
}

#[inline(always)]
unsafe fn table_addr(facade: &ExecuteContextFacade<'_, '_>, table_idx: usize) -> GcRef {
    facade.table_addr(table_idx)
}

#[inline(always)]
unsafe fn table_get_impl(
    facade: &mut ExecuteContextFacade<'_, '_>,
    table_idx: usize,
    i: u32,
) -> VMResult<u32> {
    let addr = table_addr(facade, table_idx);
    let value = vm_try!(VMResult::from_option(
        facade.table_get_value(addr, i as usize),
        || { VMResult::TableIndexOutOfRange }
    ));
    trace!("op_table_get: {table_idx} {addr:?} {i} {value}");
    VMResult::Success(value)
}

#[inline(always)]
unsafe fn table_set_impl(
    facade: &mut ExecuteContextFacade<'_, '_>,
    table_idx: usize,
    i: u32,
    value: u32,
) -> VMResult<()> {
    let addr = table_addr(facade, table_idx);
    trace!("op_table_set: {table_idx} {addr:?} {i} {value}");
    facade.table_set_value(addr, i as usize, value)
}

#[inline(never)]
unsafe fn table_copy_impl(
    facade: &mut ExecuteContextFacade<'_, '_>,
    dst_table_idx: usize,
    src_table_idx: usize,
    dst: usize,
    src: usize,
    len: usize,
) -> VMResult<()> {
    let src_table_addr = table_addr(facade, src_table_idx);
    let dst_table_addr = table_addr(facade, dst_table_idx);
    facade.table_copy(dst_table_addr, src_table_addr, dst, src, len)
}

#[inline(always)]
unsafe fn table_grow_impl(
    facade: &mut ExecuteContextFacade<'_, '_>,
    table_idx: usize,
    n: i32,
    val: u32,
) -> VMResult<u32> {
    let table_addr = table_addr(facade, table_idx);
    VMResult::Success(facade.table_grow(table_addr, n, val))
}

#[inline(always)]
unsafe fn table_size_impl(facade: &mut ExecuteContextFacade<'_, '_>, table_idx: usize) -> u32 {
    let table_addr = table_addr(facade, table_idx);
    let value = facade.table_len(table_addr) as u32;
    trace!("op_table_size: {table_idx} {table_addr:?} => {value}");
    value
}

#[inline(always)]
unsafe fn table_fill_impl(
    facade: &mut ExecuteContextFacade<'_, '_>,
    table_idx: usize,
    i: usize,
    val: u32,
    n: usize,
) -> VMResult<()> {
    let table_addr = table_addr(facade, table_idx);
    facade.table_fill(table_addr, i, n, val)
}

#[inline(always)]
unsafe fn pop_table_access_index(facade: &mut ExecuteContextFacade<'_, '_>) -> u32 {
    facade.pop_u32()
}

#[inline(always)]
unsafe fn pop_table_set_operands(facade: &mut ExecuteContextFacade<'_, '_>) -> (u32, u32) {
    let value = facade.pop_u32();
    let index = facade.pop_u32();
    (index, value)
}

#[inline(always)]
unsafe fn pop_table_range_operands(
    facade: &mut ExecuteContextFacade<'_, '_>,
) -> (usize, usize, usize) {
    let len = facade.pop_u32() as usize;
    let src = facade.pop_u32() as usize;
    let dst = facade.pop_u32() as usize;
    (dst, src, len)
}

#[inline(always)]
unsafe fn pop_table_fill_operands(
    facade: &mut ExecuteContextFacade<'_, '_>,
) -> (usize, u32, usize) {
    let len = facade.pop_u32() as usize;
    let value = facade.pop_u32();
    let index = facade.pop_u32() as usize;
    (index, value, len)
}

#[inline(always)]
unsafe fn pop_table_grow_operands(facade: &mut ExecuteContextFacade<'_, '_>) -> (u32, i32) {
    let count = facade.pop_i32();
    let value = facade.pop_u32();
    (value, count)
}

#[inline(always)]
unsafe fn push_table_result(
    tail_code: *const Instr,
    facade: &mut ExecuteContextFacade<'_, '_>,
    skip: isize,
    value: u32,
) -> VMResult<()> {
    vm_try!(facade.push_u32(value));
    facade_call_next(tail_code, skip, facade)
}

/// WebAssembly `table.get`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[i32] -> [ref]`.
/// Traps: traps on out-of-bounds table access or type mismatch.
/// Notes: Accesses the instance table storage and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_table_get(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let mut facade = ExecuteContextFacade::new(ctx);
    let idx = decode_table_index(tail_code);
    let i = pop_table_access_index(&mut facade);
    let val = vm_try!(table_get_impl(&mut facade, idx, i));
    push_table_result(tail_code, &mut facade, 1, val)
}

/// WebAssembly `table.set`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[i32, ref] -> []`.
/// Traps: traps on out-of-bounds table access or type mismatch.
/// Notes: Accesses the instance table storage and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_table_set(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let mut facade = ExecuteContextFacade::new(ctx);
    let idx = decode_table_index(tail_code);
    let (i, val) = pop_table_set_operands(&mut facade);
    vm_try!(table_set_impl(&mut facade, idx, i, val));
    facade_call_next(tail_code, 1, &mut facade)
}

#[inline(never)]
/// WebAssembly bulk-memory `table.init` helper.
///
/// Spec:
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: internal `table.init` operand handling.
/// Traps: traps on table bounds violations or invalid element segment access.
/// Notes: Resolves the destination table and source element segment before copying the validated payload.
///
/// # Safety
/// - `ctx` must reference a live execution context whose table and element metadata are still valid for the current frame.
/// - `src_elem_idx`, `dst_table_idx`, `dst_pos`, `src`, and `len` must have already passed the instruction-level validation performed by the caller.
/// - This helper must not keep borrows, locks, or guards alive across any follow-up tail-dispatch.
unsafe fn table_init_impl(
    facade: &mut ExecuteContextFacade<'_, '_>,
    src_elem_idx: u32,
    dst_table_idx: usize,
    dst_pos: usize,
    src: usize,
    len: usize,
) -> VMResult<()> {
    let dst_table_addr = facade.table_addr(dst_table_idx);
    let reftype = facade.table_reftype(dst_table_addr);
    let globals = facade.instance_globals_snapshot();
    let funcs = facade.instance_funcs_snapshot();
    let func_addrs = facade.instance_func_addrs_snapshot();
    let dst_table_len = facade.table_len(dst_table_addr);
    if dst_pos.checked_add(len).is_none() || dst_pos + len > dst_table_len {
        return VMResult::TableIndexOutOfRange;
    }
    let Some(elem_init) = facade.elem_init(src_elem_idx) else {
        return if len == 0 && src == 0 {
            VMResult::Success(())
        } else {
            VMResult::TableIndexOutOfRange
        };
    };
    match elem_init {
        ElemInit::FuncIdx(idxs) => {
            let slice = vm_try!(VMResult::from_option(idxs.get(src..(src + len)), || {
                VMResult::TableIndexOutOfRange
            }));
            let values = slice
                .iter()
                .map(|funcidx| func_addrs[*funcidx as usize])
                .collect::<Vec<_>>();
            vm_try!(facade.table_write_slice(dst_table_addr, dst_pos, &values));
        }
        ElemInit::ConstExpr(exprs) => {
            let slice = vm_try!(VMResult::from_option(exprs.get(src..(src + len)), || {
                VMResult::TableIndexOutOfRange
            }));
            let values = vm_try!(facade.eval_elem_init_exprs(
                slice,
                globals.as_slice(),
                funcs.as_slice(),
                reftype,
            ));
            vm_try!(facade.table_write_slice(dst_table_addr, dst_pos, &values));
        }
    }
    VMResult::Success(())
}

/// WebAssembly `table.init`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[dst, src, len] -> []`.
/// Traps: traps on out-of-bounds table access or type mismatch.
/// Notes: Accesses the instance table storage and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_table_init(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let mut facade = ExecuteContextFacade::new(ctx);
    let (dst_pos, src, len) = pop_table_range_operands(&mut facade);
    let (src_elem_idx, dst_table_idx) = decode_table_init_ids(tail_code);
    vm_try!(table_init_impl(
        &mut facade,
        src_elem_idx,
        dst_table_idx,
        dst_pos,
        src,
        len,
    ));
    facade_call_next(tail_code, 2, &mut facade)
}

#[inline(never)]
fn elem_drop_impl(facade: &ExecuteContextFacade<'_, '_>, elem_idx: u32) {
    facade.drop_elem_segment(elem_idx);
}

/// WebAssembly `elem.drop`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[] -> []`.
/// Traps: traps on out-of-bounds table access or type mismatch.
/// Notes: Accesses the instance table storage and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_elem_drop(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let mut facade = ExecuteContextFacade::new(ctx);
    let elem_idx = decode_elem_segment_id(tail_code);
    elem_drop_impl(&facade, elem_idx);
    facade_call_next(tail_code, 1, &mut facade)
}

/// WebAssembly `table.copy`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[dst, src, len] -> []`.
/// Traps: traps on out-of-bounds table access or type mismatch.
/// Notes: Accesses the instance table storage and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_table_copy(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let mut facade = ExecuteContextFacade::new(ctx);
    let (dst, src, len) = pop_table_range_operands(&mut facade);
    let (dst_table_idx, src_table_idx) = decode_table_pair_indices(tail_code);
    vm_try!(table_copy_impl(
        &mut facade,
        dst_table_idx,
        src_table_idx,
        dst,
        src,
        len
    ));
    facade_call_next(tail_code, 2, &mut facade)
}

/// WebAssembly `table.grow`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[ref, delta] -> [i32]`.
/// Traps: traps on out-of-bounds table access or type mismatch.
/// Notes: Accesses the instance table storage and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_table_grow(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let mut facade = ExecuteContextFacade::new(ctx);
    let table_idx = decode_table_index(tail_code);
    let (val, n) = pop_table_grow_operands(&mut facade);
    let result = vm_try!(table_grow_impl(&mut facade, table_idx, n, val));
    push_table_result(tail_code, &mut facade, 1, result)
}

/// WebAssembly `table.size`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[] -> [i32]`.
/// Traps: traps on out-of-bounds table access or type mismatch.
/// Notes: Accesses the instance table storage and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_table_size(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let mut facade = ExecuteContextFacade::new(ctx);
    let table_idx = decode_table_index(tail_code);
    let val = table_size_impl(&mut facade, table_idx);
    push_table_result(tail_code, &mut facade, 1, val)
}

/// WebAssembly `table.fill`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[dst, ref, len] -> []`.
/// Traps: traps on out-of-bounds table access or type mismatch.
/// Notes: Accesses the instance table storage and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_table_fill(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let mut facade = ExecuteContextFacade::new(ctx);
    let (i, val, n) = pop_table_fill_operands(&mut facade);
    let table_idx = decode_table_index(tail_code);
    vm_try!(table_fill_impl(&mut facade, table_idx, i, val, n));
    facade_call_next(tail_code, 1, &mut facade)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        common::{
            stack::CachedMemoryKind,
            store::{self, InstanceData},
            CallFrameCache, ExecuteContext, GcRef, Limits, LocalReference, Operand, RefType, Store,
            StoreInner, TableType,
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
            PendingOpEmitter::from_parts(29, pending_effects, pending_ops),
            std::ptr::null(),
            29,
        )
    }

    unsafe fn stop_op(_tail_code: *const Instr, _ctx: &mut ExecuteContext) -> VMResult<()> {
        VMResult::Success(())
    }

    #[test]
    fn table_observation_tracks_size_continue() {
        let store = Store::new();
        let mut gc = StoreInner::new();
        let mut pending_effects = 0;
        let mut pending_ops = VecDeque::new();
        let mut stack = Stack::new(32);

        let table = gc.new_table(TableType {
            reftype: RefType::FuncRef,
            limits: Limits {
                min: 2,
                max: Some(3),
            },
        });
        let instance = store::InstanceId::from_index(0);
        let _instance_addr = gc.new_instance(&InstanceData {
            instance_id: 1,
            module_addr: GcRef(0),
            globals: Vec::new(),
            funcs: Vec::new(),
            tables: vec![table],
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
        let result = unsafe { op_table_size(program.as_ptr(), &mut ctx) };
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
        assert_eq!(value, 2);
        assert_eq!(ctx.pending_len(), 0);
        assert_eq!(ctx.cont(), unsafe { program.as_ptr().add(1) });
    }
}
