use super::*;
use vstd::prelude::*;

verus! {

#[allow(dead_code)]
pub(crate) enum BulkMemoryStepWitnessParts {
    Init {
        selector: MemorySelectorWitness,
        data_segment_id: u32,
        dst: nat,
        src: nat,
        len: nat,
        next_cont: nat,
    },
    Copy {
        dst_selector: MemorySelectorWitness,
        src_selector: MemorySelectorWitness,
        dst: nat,
        src: nat,
        len: nat,
        next_cont: nat,
    },
    Fill {
        selector: MemorySelectorWitness,
        start: nat,
        len: nat,
        value: u8,
        next_cont: nat,
    },
    DataDrop {
        data_segment_id: u32,
        next_cont: nat,
    },
}

#[allow(dead_code)]
pub(crate) open spec fn bulk_memory_init_witness_for_handler(
    selector: MemorySelectorWitness,
    data_segment_id: u32,
    dst: nat,
    src: nat,
    len: nat,
    next_cont: nat,
) -> BulkMemoryStepWitnessParts {
    BulkMemoryStepWitnessParts::Init {
        selector,
        data_segment_id,
        dst,
        src,
        len,
        next_cont,
    }
}

#[allow(dead_code)]
pub(crate) open spec fn bulk_memory_copy_witness_for_handler(
    dst_selector: MemorySelectorWitness,
    src_selector: MemorySelectorWitness,
    dst: nat,
    src: nat,
    len: nat,
    next_cont: nat,
) -> BulkMemoryStepWitnessParts {
    BulkMemoryStepWitnessParts::Copy {
        dst_selector,
        src_selector,
        dst,
        src,
        len,
        next_cont,
    }
}

#[allow(dead_code)]
pub(crate) open spec fn bulk_memory_fill_witness_for_handler(
    selector: MemorySelectorWitness,
    start: nat,
    len: nat,
    value: u8,
    next_cont: nat,
) -> BulkMemoryStepWitnessParts {
    BulkMemoryStepWitnessParts::Fill {
        selector,
        start,
        len,
        value,
        next_cont,
    }
}

#[allow(dead_code)]
pub(crate) open spec fn bulk_memory_data_drop_witness_for_handler(
    data_segment_id: u32,
    next_cont: nat,
) -> BulkMemoryStepWitnessParts {
    BulkMemoryStepWitnessParts::DataDrop {
        data_segment_id,
        next_cont,
    }
}

pub(crate) open spec fn bulk_memory_step_from_witness_parts(
    witness: BulkMemoryStepWitnessParts,
) -> crate::common::formal::BulkMemoryStep {
    match witness {
        BulkMemoryStepWitnessParts::Init {
            selector,
            data_segment_id,
            dst,
            src,
            len,
            next_cont,
        } => crate::common::formal::BulkMemoryStep::Init {
            selector: crate::runtime::vm::memory_selector_from_witness(selector),
            data_segment_id: data_segment_id as nat,
            dst,
            src,
            len,
            next_cont,
        },
        BulkMemoryStepWitnessParts::Copy {
            dst_selector,
            src_selector,
            dst,
            src,
            len,
            next_cont,
        } => crate::common::formal::BulkMemoryStep::Copy {
            dst_selector: crate::runtime::vm::memory_selector_from_witness(dst_selector),
            src_selector: crate::runtime::vm::memory_selector_from_witness(src_selector),
            dst,
            src,
            len,
            next_cont,
        },
        BulkMemoryStepWitnessParts::Fill {
            selector,
            start,
            len,
            value,
            next_cont,
        } => crate::common::formal::BulkMemoryStep::Fill {
            selector: crate::runtime::vm::memory_selector_from_witness(selector),
            start,
            len,
            value,
            next_cont,
        },
        BulkMemoryStepWitnessParts::DataDrop {
            data_segment_id,
            next_cont,
        } => crate::common::formal::BulkMemoryStep::DataDrop {
            data_segment_id: data_segment_id as nat,
            next_cont,
        },
    }
}

pub open spec fn bulk_memory_continue_cont(step: crate::common::formal::BulkMemoryStep) -> nat {
    match step {
        crate::common::formal::BulkMemoryStep::Init { next_cont, .. } => next_cont,
        crate::common::formal::BulkMemoryStep::Copy { next_cont, .. } => next_cont,
        crate::common::formal::BulkMemoryStep::Fill { next_cont, .. } => next_cont,
        crate::common::formal::BulkMemoryStep::DataDrop { next_cont, .. } => next_cont,
    }
}

pub(crate) open spec fn bulk_memory_observation_refines_spec_step(
    before: crate::common::CoreStepStateProjectionParts,
    step: crate::common::formal::BulkMemoryStep,
    after: crate::common::CoreStepStateProjectionParts,
    outcome: crate::common::formal::CoreOutcome,
) -> bool {
    crate::common::runtime_observation_refines_instr(
        before,
        crate::common::formal::CoreStepInstr::BulkMemory(step),
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
                == bulk_memory_continue_cont(step)
        }
}

proof fn lemma_bulk_memory_family_state_refines_spec_step(
    before: crate::common::formal::CoreStepState,
    step: crate::common::formal::BulkMemoryStep,
)
    ensures
        crate::common::formal::spec_step(
            before,
            crate::common::formal::CoreStepInstr::BulkMemory(step),
        ) == crate::common::formal::spec_step_bulk_memory(before, step),
        crate::common::formal::task_id_preserved(
            before,
            crate::common::formal::spec_step_bulk_memory(before, step).0,
        ),
        if crate::common::formal::outcome_is_trap(
            crate::common::formal::spec_step_bulk_memory(before, step).1,
        ) {
            crate::common::formal::spec_step_bulk_memory(before, step).0.context.cont_addr
                == before.context.cont_addr
        } else {
            crate::common::formal::spec_step_bulk_memory(before, step).0.context.cont_addr
                == bulk_memory_continue_cont(step)
        },
{
}

pub(crate) proof fn lemma_bulk_memory_family_refines_spec_step(
    before: crate::common::formal::CoreStepState,
    step: crate::common::formal::BulkMemoryStep,
)
    ensures
        crate::common::formal::spec_step(
            before,
            crate::common::formal::CoreStepInstr::BulkMemory(step),
        ) == crate::common::formal::spec_step_bulk_memory(before, step),
        crate::common::formal::task_id_preserved(
            before,
            crate::common::formal::spec_step_bulk_memory(before, step).0,
        ),
        if crate::common::formal::outcome_is_trap(
            crate::common::formal::spec_step_bulk_memory(before, step).1,
        ) {
            crate::common::formal::spec_step_bulk_memory(before, step).0.context.cont_addr
                == before.context.cont_addr
        } else {
            crate::common::formal::spec_step_bulk_memory(before, step).0.context.cont_addr
                == bulk_memory_continue_cont(step)
        },
{
}

pub(crate) proof fn lemma_bulk_memory_observation_refines_spec_step(
    before: crate::common::CoreStepStateProjectionParts,
    step: crate::common::formal::BulkMemoryStep,
    after: crate::common::CoreStepStateProjectionParts,
    outcome: crate::common::formal::CoreOutcome,
)
    requires
        crate::common::runtime_observation_refines_instr(
            before,
            crate::common::formal::CoreStepInstr::BulkMemory(step),
            after,
            outcome,
        ),
    ensures
        bulk_memory_observation_refines_spec_step(before, step, after, outcome),
{
    lemma_bulk_memory_family_refines_spec_step(
        crate::common::core_step_state_from_projection_parts(before),
        step,
    );
}

pub(crate) open spec fn bulk_memory_witness_observation_refines_spec_step(
    before: crate::common::CoreStepStateProjectionParts,
    witness: BulkMemoryStepWitnessParts,
    after: crate::common::CoreStepStateProjectionParts,
    outcome: crate::common::formal::CoreOutcome,
) -> bool {
    bulk_memory_observation_refines_spec_step(
        before,
        bulk_memory_step_from_witness_parts(witness),
        after,
        outcome,
    )
}

pub(crate) proof fn lemma_bulk_memory_witness_observation_refines_spec_step(
    before: crate::common::CoreStepStateProjectionParts,
    witness: BulkMemoryStepWitnessParts,
    after: crate::common::CoreStepStateProjectionParts,
    outcome: crate::common::formal::CoreOutcome,
)
    requires
        crate::common::runtime_observation_refines_instr(
            before,
            crate::common::formal::CoreStepInstr::BulkMemory(
                bulk_memory_step_from_witness_parts(witness),
            ),
            after,
            outcome,
        ),
    ensures
        bulk_memory_witness_observation_refines_spec_step(before, witness, after, outcome),
{
    lemma_bulk_memory_observation_refines_spec_step(
        before,
        bulk_memory_step_from_witness_parts(witness),
        after,
        outcome,
    );
}

pub(crate) proof fn lemma_bulk_memory_handler_refines_spec_step(
    before: crate::common::CoreStepStateProjectionParts,
    witness: BulkMemoryStepWitnessParts,
    after: crate::common::CoreStepStateProjectionParts,
    outcome: crate::common::formal::CoreOutcome,
)
    requires
        crate::common::runtime_observation_refines_instr(
            before,
            crate::common::formal::CoreStepInstr::BulkMemory(
                bulk_memory_step_from_witness_parts(witness),
            ),
            after,
            outcome,
        ),
    ensures
        bulk_memory_witness_observation_refines_spec_step(before, witness, after, outcome),
{
    lemma_bulk_memory_witness_observation_refines_spec_step(before, witness, after, outcome);
}

} // verus!

#[inline(always)]
/// Decode the data-segment index immediate for bulk-memory handlers.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for the current handler.
unsafe fn decode_data_index(tail_code: *const Instr) -> u32 {
    (*tail_code).operand.u32
}

#[inline(always)]
/// Decode the single memory-index immediate for indexed bulk-memory handlers.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for the current handler.
unsafe fn decode_memidx_operand(tail_code: *const Instr) -> u32 {
    (*tail_code).operand.u32
}

#[inline(always)]
/// Decode the `(data_segment, memidx)` immediates for indexed `memory.init`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for the current handler.
unsafe fn decode_init_index_and_memidx(tail_code: *const Instr) -> (u32, u32) {
    ((*tail_code).operand.u32, (*tail_code.add(1)).operand.u32)
}

#[inline(always)]
/// Decode the `(dst_memidx, src_memidx)` immediates for indexed `memory.copy`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for the current handler.
unsafe fn decode_copy_memidx_pair(tail_code: *const Instr) -> (u32, u32) {
    ((*tail_code).operand.u32, (*tail_code.add(1)).operand.u32)
}

#[inline(always)]
unsafe fn pop_copy_operands(facade: &mut ExecuteContextFacade<'_, '_>) -> (u32, u32, u32) {
    let len = facade.pop_u32();
    let src = facade.pop_u32();
    let dst = facade.pop_u32();
    (dst, src, len)
}

#[inline(always)]
unsafe fn pop_fill_operands(facade: &mut ExecuteContextFacade<'_, '_>) -> (u32, u32, u32) {
    let len = facade.pop_u32();
    let data = facade.pop_u32();
    let ptr = facade.pop_u32();
    (ptr, data, len)
}

#[inline(always)]
unsafe fn pop_init_operands(facade: &mut ExecuteContextFacade<'_, '_>) -> (u32, u32, u32) {
    let len = facade.pop_u32();
    let src = facade.pop_u32();
    let dst = facade.pop_u32();
    (dst, src, len)
}

#[inline(never)]
fn mem_init_bytes(
    facade: &mut ExecuteContextFacade<'_, '_>,
    idx: u32,
    src: u32,
    len: u32,
) -> VMResult<Option<Vec<u8>>> {
    facade.data_segment_bytes(idx, src, len)
}

#[inline(never)]
fn mem_init_impl_local(
    facade: &mut ExecuteContextFacade<'_, '_>,
    idx: u32,
    dst: u32,
    src: u32,
    len: u32,
) -> VMResult<()> {
    let memory = unsafe { facade.default_local_memory_id_unchecked() };
    mem_init_impl_local_with_id(facade, memory, idx, dst, src, len)
}

#[inline(never)]
fn mem_init_impl_local_with_id(
    facade: &mut ExecuteContextFacade<'_, '_>,
    memory: crate::common::store::LocalMemoryId,
    idx: u32,
    dst: u32,
    src: u32,
    len: u32,
) -> VMResult<()> {
    let copied = vm_try!(mem_init_bytes(facade, idx, src, len));
    facade.write_local_memory_bytes_by_id(memory, dst, copied.as_deref().unwrap_or(&[]))
}

#[inline(never)]
fn mem_init_impl_shared(
    facade: &mut ExecuteContextFacade<'_, '_>,
    idx: u32,
    dst: u32,
    src: u32,
    len: u32,
) -> VMResult<()> {
    let memory = unsafe { facade.default_shared_memory_id_unchecked() };
    mem_init_impl_shared_with_id(facade, memory, idx, dst, src, len)
}

#[inline(never)]
fn mem_init_impl_shared_with_id(
    facade: &mut ExecuteContextFacade<'_, '_>,
    memory: crate::common::store::SharedMemoryId,
    idx: u32,
    dst: u32,
    src: u32,
    len: u32,
) -> VMResult<()> {
    let copied = vm_try!(mem_init_bytes(facade, idx, src, len));
    facade.write_shared_memory_bytes_by_id(memory, dst, copied.as_deref().unwrap_or(&[]))
}

#[inline(never)]
fn data_drop_impl(facade: &ExecuteContextFacade<'_, '_>, idx: u32) {
    facade.drop_data_segment(idx);
}

#[inline(never)]
fn mem_copy_impl_local_with_id(
    facade: &mut ExecuteContextFacade<'_, '_>,
    memory: crate::common::store::LocalMemoryId,
    dst: u32,
    src: u32,
    len: u32,
) -> VMResult<()> {
    facade.copy_local_memory_by_id(memory, dst, src, len)
}

#[inline(never)]
fn mem_copy_impl_shared_with_id(
    facade: &mut ExecuteContextFacade<'_, '_>,
    memory: crate::common::store::SharedMemoryId,
    dst: u32,
    src: u32,
    len: u32,
) -> VMResult<()> {
    facade.copy_shared_memory_by_id(memory, dst, src, len)
}

#[inline(never)]
fn mem_fill_impl_local_with_id(
    facade: &mut ExecuteContextFacade<'_, '_>,
    memory: crate::common::store::LocalMemoryId,
    ptr: u32,
    len: u32,
    data: u32,
) -> VMResult<()> {
    facade.fill_local_memory_by_id(memory, ptr, len, data)
}

#[inline(never)]
fn mem_fill_impl_shared_with_id(
    facade: &mut ExecuteContextFacade<'_, '_>,
    memory: crate::common::store::SharedMemoryId,
    ptr: u32,
    len: u32,
    data: u32,
) -> VMResult<()> {
    facade.fill_shared_memory_by_id(memory, ptr, len, data)
}

#[inline(never)]
fn mem_copy_impl_local_to_local(
    facade: &mut ExecuteContextFacade<'_, '_>,
    dst: crate::common::store::LocalMemoryId,
    src: crate::common::store::LocalMemoryId,
    dst_offset: u32,
    src_offset: u32,
    len: u32,
) -> VMResult<()> {
    facade.copy_memory_local_to_local_by_id(dst, src, dst_offset, src_offset, len)
}

#[inline(never)]
fn mem_copy_impl_shared_to_local(
    facade: &mut ExecuteContextFacade<'_, '_>,
    dst: crate::common::store::LocalMemoryId,
    src: crate::common::store::SharedMemoryId,
    dst_offset: u32,
    src_offset: u32,
    len: u32,
) -> VMResult<()> {
    facade.copy_memory_shared_to_local_by_id(dst, src, dst_offset, src_offset, len)
}

#[inline(never)]
fn mem_copy_impl_local_to_shared(
    facade: &mut ExecuteContextFacade<'_, '_>,
    dst: crate::common::store::SharedMemoryId,
    src: crate::common::store::LocalMemoryId,
    dst_offset: u32,
    src_offset: u32,
    len: u32,
) -> VMResult<()> {
    facade.copy_memory_local_to_shared_by_id(dst, src, dst_offset, src_offset, len)
}

#[inline(never)]
fn mem_copy_impl_shared_to_shared(
    facade: &mut ExecuteContextFacade<'_, '_>,
    dst: crate::common::store::SharedMemoryId,
    src: crate::common::store::SharedMemoryId,
    dst_offset: u32,
    src_offset: u32,
    len: u32,
) -> VMResult<()> {
    facade.copy_memory_shared_to_shared_by_id(dst, src, dst_offset, src_offset, len)
}

/// WebAssembly `memory.init`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[dst, src, len] -> []`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Implements the bulk-memory instruction on the default linear memory and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_mem_init(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let mut facade = ExecuteContextFacade::new(ctx);
    let idx = decode_data_index(tail_code);
    let (dst, src, len) = pop_init_operands(&mut facade);
    vm_try!(mem_init_impl_local(&mut facade, idx, dst, src, len));
    facade_call_next(tail_code, 1, &mut facade)
}

/// WebAssembly `data.drop`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[] -> []`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Implements the bulk-memory instruction on the default linear memory and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_data_drop(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let mut facade = ExecuteContextFacade::new(ctx);
    let idx = decode_data_index(tail_code);
    data_drop_impl(&facade, idx);
    facade_call_next(tail_code, 1, &mut facade)
}

/// WebAssembly `memory.copy`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[dst, src, len] -> []`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Implements the bulk-memory instruction on the default linear memory and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_mem_copy(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let mut facade = ExecuteContextFacade::new(ctx);
    let (dst, src, len) = pop_copy_operands(&mut facade);
    let memory = facade.default_local_memory_id_unchecked();
    trace!("op_mem_copy src: {src},dst: {dst},len: {len}");
    vm_try!(mem_copy_impl_local_with_id(
        &mut facade,
        memory,
        dst,
        src,
        len,
    ));
    facade_call_next(tail_code, 0, &mut facade)
}

/// WebAssembly `memory.fill`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[dst, value, len] -> []`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Implements the bulk-memory instruction on the default linear memory and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_mem_fill(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let mut facade = ExecuteContextFacade::new(ctx);
    let (ptr, data, len) = pop_fill_operands(&mut facade);
    let memory = facade.default_local_memory_id_unchecked();
    vm_try!(mem_fill_impl_local_with_id(
        &mut facade,
        memory,
        ptr,
        len,
        data,
    ));
    facade_call_next(tail_code, 0, &mut facade)
}

/// WebAssembly `memory.init` on shared default memory.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[dst, src, len] -> []`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Uses the shared-memory specialized fast path selected by the parser and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose default memory is shared.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_mem_init_shared(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let mut facade = ExecuteContextFacade::new(ctx);
    let idx = decode_data_index(tail_code);
    let (dst, src, len) = pop_init_operands(&mut facade);
    vm_try!(mem_init_impl_shared(&mut facade, idx, dst, src, len));
    facade_call_next(tail_code, 1, &mut facade)
}

/// WebAssembly `memory.copy` on shared default memory.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[dst, src, len] -> []`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Uses the shared-memory specialized fast path selected by the parser and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose default memory is shared.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_mem_copy_shared(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let mut facade = ExecuteContextFacade::new(ctx);
    let (dst, src, len) = pop_copy_operands(&mut facade);
    let memory = facade.default_shared_memory_id_unchecked();
    trace!("op_mem_copy_shared src: {src},dst: {dst},len: {len}");
    vm_try!(mem_copy_impl_shared_with_id(
        &mut facade,
        memory,
        dst,
        src,
        len,
    ));
    facade_call_next(tail_code, 0, &mut facade)
}

/// WebAssembly `memory.fill` on shared default memory.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[dst, value, len] -> []`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Uses the shared-memory specialized fast path selected by the parser and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose default memory is shared.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_mem_fill_shared(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let mut facade = ExecuteContextFacade::new(ctx);
    let (ptr, data, len) = pop_fill_operands(&mut facade);
    let memory = facade.default_shared_memory_id_unchecked();
    vm_try!(mem_fill_impl_shared_with_id(
        &mut facade,
        memory,
        ptr,
        len,
        data,
    ));
    facade_call_next(tail_code, 0, &mut facade)
}

/// WebAssembly `memory.init` on indexed local memory.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/multi-memory/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/multi-memory/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/multi-memory/core/exec/instructions.html
///
/// Stack effect: `[dst, src, len] -> []`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Uses the typed indexed local-memory fast path and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose indexed memory operand is in-bounds and local.
pub unsafe fn op_mem_init_indexed_local(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let mut facade = ExecuteContextFacade::new(ctx);
    let (idx, memidx) = decode_init_index_and_memidx(tail_code);
    let (dst, src, len) = pop_init_operands(&mut facade);
    let memory = facade.local_memory_id_at_unchecked(memidx);
    vm_try!(mem_init_impl_local_with_id(
        &mut facade,
        memory,
        idx,
        dst,
        src,
        len,
    ));
    facade_call_next(tail_code, 2, &mut facade)
}

/// WebAssembly `memory.init` on indexed shared memory.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/multi-memory/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/multi-memory/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/multi-memory/core/exec/instructions.html
///
/// Stack effect: `[dst, src, len] -> []`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Uses the typed indexed shared-memory fast path and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose indexed memory operand is in-bounds and shared.
pub unsafe fn op_mem_init_indexed_shared(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let mut facade = ExecuteContextFacade::new(ctx);
    let (idx, memidx) = decode_init_index_and_memidx(tail_code);
    let (dst, src, len) = pop_init_operands(&mut facade);
    let memory = facade.shared_memory_id_at_unchecked(memidx);
    vm_try!(mem_init_impl_shared_with_id(
        &mut facade,
        memory,
        idx,
        dst,
        src,
        len,
    ));
    facade_call_next(tail_code, 2, &mut facade)
}

/// WebAssembly `memory.copy` from indexed local memory to indexed local memory.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/multi-memory/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/multi-memory/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/multi-memory/core/exec/instructions.html
///
/// Stack effect: `[dst, src, len] -> []`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Uses the typed indexed local-to-local fast path and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose indexed memory operands are in-bounds and local.
pub unsafe fn op_mem_copy_indexed_local_local(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let mut facade = ExecuteContextFacade::new(ctx);
    let (dst_memidx, src_memidx) = decode_copy_memidx_pair(tail_code);
    let (dst, src, len) = pop_copy_operands(&mut facade);
    let dst_memory = facade.local_memory_id_at_unchecked(dst_memidx);
    let src_memory = facade.local_memory_id_at_unchecked(src_memidx);
    vm_try!(mem_copy_impl_local_to_local(
        &mut facade,
        dst_memory,
        src_memory,
        dst,
        src,
        len,
    ));
    facade_call_next(tail_code, 2, &mut facade)
}

/// WebAssembly `memory.copy` from indexed shared memory to indexed local memory.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/multi-memory/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/multi-memory/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/multi-memory/core/exec/instructions.html
///
/// Stack effect: `[dst, src, len] -> []`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Uses the typed indexed shared-to-local path and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose destination memory is local and source memory is shared.
pub unsafe fn op_mem_copy_indexed_local_shared(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let mut facade = ExecuteContextFacade::new(ctx);
    let (dst_memidx, src_memidx) = decode_copy_memidx_pair(tail_code);
    let (dst, src, len) = pop_copy_operands(&mut facade);
    let dst_memory = facade.local_memory_id_at_unchecked(dst_memidx);
    let src_memory = facade.shared_memory_id_at_unchecked(src_memidx);
    vm_try!(mem_copy_impl_shared_to_local(
        &mut facade,
        dst_memory,
        src_memory,
        dst,
        src,
        len,
    ));
    facade_call_next(tail_code, 2, &mut facade)
}

/// WebAssembly `memory.copy` from indexed local memory to indexed shared memory.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/multi-memory/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/multi-memory/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/multi-memory/core/exec/instructions.html
///
/// Stack effect: `[dst, src, len] -> []`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Uses the typed indexed local-to-shared path and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose destination memory is shared and source memory is local.
pub unsafe fn op_mem_copy_indexed_shared_local(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let mut facade = ExecuteContextFacade::new(ctx);
    let (dst_memidx, src_memidx) = decode_copy_memidx_pair(tail_code);
    let (dst, src, len) = pop_copy_operands(&mut facade);
    let dst_memory = facade.shared_memory_id_at_unchecked(dst_memidx);
    let src_memory = facade.local_memory_id_at_unchecked(src_memidx);
    vm_try!(mem_copy_impl_local_to_shared(
        &mut facade,
        dst_memory,
        src_memory,
        dst,
        src,
        len,
    ));
    facade_call_next(tail_code, 2, &mut facade)
}

/// WebAssembly `memory.copy` from indexed shared memory to indexed shared memory.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/multi-memory/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/multi-memory/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/multi-memory/core/exec/instructions.html
///
/// Stack effect: `[dst, src, len] -> []`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Uses the typed indexed shared-to-shared path and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose indexed memory operands are in-bounds and shared.
pub unsafe fn op_mem_copy_indexed_shared_shared(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let mut facade = ExecuteContextFacade::new(ctx);
    let (dst_memidx, src_memidx) = decode_copy_memidx_pair(tail_code);
    let (dst, src, len) = pop_copy_operands(&mut facade);
    let dst_memory = facade.shared_memory_id_at_unchecked(dst_memidx);
    let src_memory = facade.shared_memory_id_at_unchecked(src_memidx);
    vm_try!(mem_copy_impl_shared_to_shared(
        &mut facade,
        dst_memory,
        src_memory,
        dst,
        src,
        len,
    ));
    facade_call_next(tail_code, 2, &mut facade)
}

/// WebAssembly `memory.fill` on indexed local memory.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/multi-memory/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/multi-memory/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/multi-memory/core/exec/instructions.html
///
/// Stack effect: `[dst, value, len] -> []`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Uses the typed indexed local-memory fast path and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose indexed memory operand is in-bounds and local.
pub unsafe fn op_mem_fill_indexed_local(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let mut facade = ExecuteContextFacade::new(ctx);
    let memidx = decode_memidx_operand(tail_code);
    let (ptr, data, len) = pop_fill_operands(&mut facade);
    let memory = facade.local_memory_id_at_unchecked(memidx);
    vm_try!(mem_fill_impl_local_with_id(
        &mut facade,
        memory,
        ptr,
        len,
        data,
    ));
    facade_call_next(tail_code, 1, &mut facade)
}

/// WebAssembly `memory.fill` on indexed shared memory.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/multi-memory/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/multi-memory/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/multi-memory/core/exec/instructions.html
///
/// Stack effect: `[dst, value, len] -> []`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Uses the typed indexed shared-memory fast path and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose indexed memory operand is in-bounds and shared.
pub unsafe fn op_mem_fill_indexed_shared(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let mut facade = ExecuteContextFacade::new(ctx);
    let memidx = decode_memidx_operand(tail_code);
    let (ptr, data, len) = pop_fill_operands(&mut facade);
    let memory = facade.shared_memory_id_at_unchecked(memidx);
    vm_try!(mem_fill_impl_shared_with_id(
        &mut facade,
        memory,
        ptr,
        len,
        data,
    ));
    facade_call_next(tail_code, 1, &mut facade)
}

pub(crate) use op_mem_copy as op_mem_copy_local;
pub(crate) use op_mem_fill as op_mem_fill_local;
pub(crate) use op_mem_init as op_mem_init_local;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        common::{
            stack::{CachedMemoryKind, CallFrameCache},
            store::{self, InstanceData, InstanceMemorySlot},
            ExecuteContext, GcRef, LocalMemoryObject, LocalReference, MemoryHandle, Store,
            StoreInner,
        },
        runtime::{memory_effect::PendingOp, scheduler::PendingOpEmitter},
    };
    use std::collections::VecDeque;

    fn frame(instance: store::InstanceId, raw: u32) -> CallFrameCache {
        CallFrameCache {
            code_addr: GcRef(0),
            code_base: std::ptr::null(),
            instance,
            memory0_kind: CachedMemoryKind::Local,
            memory0_raw: raw,
        }
    }

    fn test_context<'a>(
        stack: &'a mut Stack,
        store: &'a Store,
        gc: &'a mut StoreInner,
        pending_effects: &'a mut u32,
        pending_ops: &'a mut VecDeque<PendingOp>,
        instance: store::InstanceId,
        memory_raw: u32,
    ) -> ExecuteContext<'a> {
        ExecuteContext::new(
            stack,
            LocalReference {
                local_top: 0,
                local_size: 0,
            },
            frame(instance, memory_raw),
            store,
            gc,
            PendingOpEmitter::from_parts(31, pending_effects, pending_ops),
            std::ptr::null(),
            31,
        )
    }

    unsafe fn stop_op(_tail_code: *const Instr, _ctx: &mut ExecuteContext) -> VMResult<()> {
        VMResult::Success(())
    }

    #[test]
    fn bulk_memory_observation_tracks_fill_continue() {
        let store = Store::new();
        let mut gc = StoreInner::new();
        let mut pending_effects = 0;
        let mut pending_ops = VecDeque::new();
        let mut stack = Stack::new(32);

        let local = match gc.alloc_local_memory(LocalMemoryObject::new(1, 1)) {
            MemoryHandle::Local(id) => id,
            MemoryHandle::Shared(_) => panic!("expected local memory"),
        };
        let instance = store::InstanceId::from_index(0);
        let _instance_addr = gc.new_instance(&InstanceData {
            instance_id: 1,
            module_addr: GcRef(0),
            globals: Vec::new(),
            funcs: Vec::new(),
            tables: Vec::new(),
            mems: Vec::new(),
            memory_slots: vec![InstanceMemorySlot::Local(local)],
        });

        stack.push_u32(1).unwrap();
        stack.push_u32(0xaa).unwrap();
        stack.push_u32(3).unwrap();

        let program = [Instr { op: stop_op }];
        let (outcome, pending_len, cont) = {
            let mut ctx = test_context(
                &mut stack,
                &store,
                &mut gc,
                &mut pending_effects,
                &mut pending_ops,
                instance,
                local.raw(),
            );

            let pending_before = ctx.pending_len();
            let result = unsafe { op_mem_fill(program.as_ptr(), &mut ctx) };
            let outcome = crate::common::formal::core_outcome_from_vm_result_with_pending(
                &result,
                pending_before,
                ctx.pending_len(),
                ctx.pending_code_delta(pending_before).unwrap_or(None),
            )
            .unwrap();
            (outcome, ctx.pending_len(), ctx.cont())
        };
        assert_eq!(outcome, crate::common::formal::CoreOutcome::Continue);
        let bytes = gc.memory_projection(MemoryHandle::Local(local)).bytes;
        assert_eq!(&bytes[0..4], &[0, 0xaa, 0xaa, 0xaa]);
        assert_eq!(pending_len, 0);
        assert_eq!(cont, program.as_ptr());
    }
}
