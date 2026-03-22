use super::*;
use crate::common::AtomicRmwOp;
use vstd::prelude::*;

verus! {

#[allow(dead_code)]
pub(crate) enum AtomicWaitKindWitness {
    I32(u32),
    I64(u64),
}

#[allow(dead_code)]
pub(crate) open spec fn atomic_wait32_witness_for_handler(expected: u32) -> AtomicWaitKindWitness {
    AtomicWaitKindWitness::I32(expected)
}

#[allow(dead_code)]
pub(crate) open spec fn atomic_wait64_witness_for_handler(expected: u64) -> AtomicWaitKindWitness {
    AtomicWaitKindWitness::I64(expected)
}

#[allow(dead_code)]
pub(crate) enum AtomicCmpxchgExpectedWitness {
    U8(u8),
    U16(u16),
    U32(u32),
    U64(u64),
}

#[allow(dead_code)]
pub(crate) open spec fn atomic_cmpxchg_u8_witness_for_handler(
    expected: u8,
) -> AtomicCmpxchgExpectedWitness {
    AtomicCmpxchgExpectedWitness::U8(expected)
}

#[allow(dead_code)]
pub(crate) open spec fn atomic_cmpxchg_u16_witness_for_handler(
    expected: u16,
) -> AtomicCmpxchgExpectedWitness {
    AtomicCmpxchgExpectedWitness::U16(expected)
}

#[allow(dead_code)]
pub(crate) open spec fn atomic_cmpxchg_u32_witness_for_handler(
    expected: u32,
) -> AtomicCmpxchgExpectedWitness {
    AtomicCmpxchgExpectedWitness::U32(expected)
}

#[allow(dead_code)]
pub(crate) open spec fn atomic_cmpxchg_u64_witness_for_handler(
    expected: u64,
) -> AtomicCmpxchgExpectedWitness {
    AtomicCmpxchgExpectedWitness::U64(expected)
}

#[allow(inconsistent_fields)]
#[allow(dead_code)]
pub(crate) enum AtomicStepWitnessParts {
    Notify {
        selector: MemorySelectorWitness,
        start: nat,
        count: u32,
        aligned: bool,
        next_cont: nat,
    },
    Wait {
        selector: MemorySelectorWitness,
        start: nat,
        expected: AtomicWaitKindWitness,
        timeout_immediate: bool,
        aligned: bool,
        next_cont: nat,
    },
    Store {
        selector: MemorySelectorWitness,
        start: nat,
        bytes: Seq<u8>,
        aligned: bool,
        next_cont: nat,
    },
    Rmw {
        selector: MemorySelectorWitness,
        start: nat,
        result_bytes: Seq<u8>,
        write_bytes: Seq<u8>,
        aligned: bool,
        next_cont: nat,
    },
    Cmpxchg {
        selector: MemorySelectorWitness,
        start: nat,
        expected: AtomicCmpxchgExpectedWitness,
        value_bytes: Seq<u8>,
        aligned: bool,
        next_cont: nat,
    },
}

#[allow(dead_code)]
pub(crate) open spec fn atomic_notify_witness_for_handler(
    selector: MemorySelectorWitness,
    start: nat,
    count: u32,
    aligned: bool,
    next_cont: nat,
) -> AtomicStepWitnessParts {
    AtomicStepWitnessParts::Notify {
        selector,
        start,
        count,
        aligned,
        next_cont,
    }
}

#[allow(dead_code)]
pub(crate) open spec fn atomic_wait_witness_for_handler(
    selector: MemorySelectorWitness,
    start: nat,
    expected: AtomicWaitKindWitness,
    timeout_immediate: bool,
    aligned: bool,
    next_cont: nat,
) -> AtomicStepWitnessParts {
    AtomicStepWitnessParts::Wait {
        selector,
        start,
        expected,
        timeout_immediate,
        aligned,
        next_cont,
    }
}

#[allow(dead_code)]
pub(crate) open spec fn atomic_store_witness_for_handler(
    selector: MemorySelectorWitness,
    start: nat,
    bytes: Seq<u8>,
    aligned: bool,
    next_cont: nat,
) -> AtomicStepWitnessParts {
    AtomicStepWitnessParts::Store {
        selector,
        start,
        bytes,
        aligned,
        next_cont,
    }
}

#[allow(dead_code)]
pub(crate) open spec fn atomic_rmw_witness_for_handler(
    selector: MemorySelectorWitness,
    start: nat,
    result_bytes: Seq<u8>,
    write_bytes: Seq<u8>,
    aligned: bool,
    next_cont: nat,
) -> AtomicStepWitnessParts {
    AtomicStepWitnessParts::Rmw {
        selector,
        start,
        result_bytes,
        write_bytes,
        aligned,
        next_cont,
    }
}

#[allow(dead_code)]
pub(crate) open spec fn atomic_cmpxchg_witness_for_handler(
    selector: MemorySelectorWitness,
    start: nat,
    expected: AtomicCmpxchgExpectedWitness,
    value_bytes: Seq<u8>,
    aligned: bool,
    next_cont: nat,
) -> AtomicStepWitnessParts {
    AtomicStepWitnessParts::Cmpxchg {
        selector,
        start,
        expected,
        value_bytes,
        aligned,
        next_cont,
    }
}

pub(crate) open spec fn atomic_wait_kind_from_witness(
    witness: AtomicWaitKindWitness,
) -> crate::common::formal::AtomicWaitKind {
    match witness {
        AtomicWaitKindWitness::I32(value) => crate::common::formal::AtomicWaitKind::I32(value),
        AtomicWaitKindWitness::I64(value) => crate::common::formal::AtomicWaitKind::I64(value),
    }
}

pub(crate) open spec fn atomic_cmpxchg_expected_from_witness(
    witness: AtomicCmpxchgExpectedWitness,
) -> crate::common::formal::AtomicCmpxchgExpected {
    match witness {
        AtomicCmpxchgExpectedWitness::U8(value) => {
            crate::common::formal::AtomicCmpxchgExpected::U8(value)
        }
        AtomicCmpxchgExpectedWitness::U16(value) => {
            crate::common::formal::AtomicCmpxchgExpected::U16(value)
        }
        AtomicCmpxchgExpectedWitness::U32(value) => {
            crate::common::formal::AtomicCmpxchgExpected::U32(value)
        }
        AtomicCmpxchgExpectedWitness::U64(value) => {
            crate::common::formal::AtomicCmpxchgExpected::U64(value)
        }
    }
}

pub(crate) open spec fn atomic_step_from_witness_parts(
    witness: AtomicStepWitnessParts,
) -> crate::common::formal::AtomicStep {
    match witness {
        AtomicStepWitnessParts::Notify {
            selector,
            start,
            count,
            aligned,
            next_cont,
        } => crate::common::formal::AtomicStep::Notify {
            selector: crate::runtime::vm::memory_selector_from_witness(selector),
            start,
            count,
            aligned,
            next_cont,
        },
        AtomicStepWitnessParts::Wait {
            selector,
            start,
            expected,
            timeout_immediate,
            aligned,
            next_cont,
        } => crate::common::formal::AtomicStep::Wait {
            selector: crate::runtime::vm::memory_selector_from_witness(selector),
            start,
            expected: atomic_wait_kind_from_witness(expected),
            timeout_immediate,
            aligned,
            next_cont,
        },
        AtomicStepWitnessParts::Store {
            selector,
            start,
            bytes,
            aligned,
            next_cont,
        } => crate::common::formal::AtomicStep::Store {
            selector: crate::runtime::vm::memory_selector_from_witness(selector),
            start,
            bytes,
            aligned,
            next_cont,
        },
        AtomicStepWitnessParts::Rmw {
            selector,
            start,
            result_bytes,
            write_bytes,
            aligned,
            next_cont,
        } => crate::common::formal::AtomicStep::Rmw {
            selector: crate::runtime::vm::memory_selector_from_witness(selector),
            start,
            result_bytes,
            write_bytes,
            aligned,
            next_cont,
        },
        AtomicStepWitnessParts::Cmpxchg {
            selector,
            start,
            expected,
            value_bytes,
            aligned,
            next_cont,
        } => crate::common::formal::AtomicStep::Cmpxchg {
            selector: crate::runtime::vm::memory_selector_from_witness(selector),
            start,
            expected: atomic_cmpxchg_expected_from_witness(expected),
            value_bytes,
            aligned,
            next_cont,
        },
    }
}

#[inline(always)]
fn wait_result_not_equal() -> (result: i32)
    ensures
        result == 1,
{
    1
}

#[inline(always)]
#[cfg(test)]
fn wait_result_ok() -> (result: i32)
    ensures
        result == 0,
{
    0
}

#[inline(always)]
#[cfg(test)]
fn wait_result_timed_out() -> (result: i32)
    ensures
        result == 2,
{
    2
}

pub open spec fn spec_atomic_start_indexed_result(
    default_memory_present: bool,
    memarg_offset: u32,
    offset: u32,
    memidx: u32,
) -> Option<(int, int)> {
    match crate::runtime::vm::spec_load_start_result(default_memory_present, memarg_offset, offset) {
        Some(start) => Some((start, memidx as int)),
        None => None,
    }
}

pub open spec fn atomic_continue_cont(step: crate::common::formal::AtomicStep) -> nat {
    match step {
        crate::common::formal::AtomicStep::Notify { next_cont, .. } => next_cont,
        crate::common::formal::AtomicStep::Wait { next_cont, .. } => next_cont,
        crate::common::formal::AtomicStep::Store { next_cont, .. } => next_cont,
        crate::common::formal::AtomicStep::Rmw { next_cont, .. } => next_cont,
        crate::common::formal::AtomicStep::Cmpxchg { next_cont, .. } => next_cont,
    }
}

pub(crate) open spec fn atomic_observation_refines_spec_step(
    before: crate::common::CoreStepStateProjectionParts,
    step: crate::common::formal::AtomicStep,
    after: crate::common::CoreStepStateProjectionParts,
    outcome: crate::common::formal::CoreOutcome,
) -> bool {
    crate::common::runtime_observation_refines_instr(
        before,
        crate::common::formal::CoreStepInstr::Atomic(step),
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
                == atomic_continue_cont(step)
        }
}

proof fn lemma_atomic_family_state_refines_spec_step(
    before: crate::common::formal::CoreStepState,
    step: crate::common::formal::AtomicStep,
)
    ensures
        crate::common::formal::spec_step(
            before,
            crate::common::formal::CoreStepInstr::Atomic(step),
        ) == crate::common::formal::spec_step_atomic(before, step),
        crate::common::formal::task_id_preserved(
            before,
            crate::common::formal::spec_step_atomic(before, step).0,
        ),
        if crate::common::formal::outcome_is_trap(
            crate::common::formal::spec_step_atomic(before, step).1,
        ) {
            crate::common::formal::spec_step_atomic(before, step).0.context.cont_addr
                == before.context.cont_addr
        } else {
            crate::common::formal::spec_step_atomic(before, step).0.context.cont_addr
                == atomic_continue_cont(step)
        },
{
}

pub(crate) proof fn lemma_atomic_family_refines_spec_step(
    before: crate::common::formal::CoreStepState,
    step: crate::common::formal::AtomicStep,
)
    ensures
        crate::common::formal::spec_step(
            before,
            crate::common::formal::CoreStepInstr::Atomic(step),
        ) == crate::common::formal::spec_step_atomic(before, step),
        crate::common::formal::task_id_preserved(
            before,
            crate::common::formal::spec_step_atomic(before, step).0,
        ),
        if crate::common::formal::outcome_is_trap(
            crate::common::formal::spec_step_atomic(before, step).1,
        ) {
            crate::common::formal::spec_step_atomic(before, step).0.context.cont_addr
                == before.context.cont_addr
        } else {
            crate::common::formal::spec_step_atomic(before, step).0.context.cont_addr
                == atomic_continue_cont(step)
        },
{
}

pub(crate) proof fn lemma_atomic_observation_refines_spec_step(
    before: crate::common::CoreStepStateProjectionParts,
    step: crate::common::formal::AtomicStep,
    after: crate::common::CoreStepStateProjectionParts,
    outcome: crate::common::formal::CoreOutcome,
)
    requires
        crate::common::runtime_observation_refines_instr(
            before,
            crate::common::formal::CoreStepInstr::Atomic(step),
            after,
            outcome,
        ),
    ensures
        atomic_observation_refines_spec_step(before, step, after, outcome),
{
    lemma_atomic_family_refines_spec_step(
        crate::common::core_step_state_from_projection_parts(before),
        step,
    );
}

pub(crate) open spec fn atomic_witness_observation_refines_spec_step(
    before: crate::common::CoreStepStateProjectionParts,
    witness: AtomicStepWitnessParts,
    after: crate::common::CoreStepStateProjectionParts,
    outcome: crate::common::formal::CoreOutcome,
) -> bool {
    atomic_observation_refines_spec_step(
        before,
        atomic_step_from_witness_parts(witness),
        after,
        outcome,
    )
}

pub(crate) proof fn lemma_atomic_witness_observation_refines_spec_step(
    before: crate::common::CoreStepStateProjectionParts,
    witness: AtomicStepWitnessParts,
    after: crate::common::CoreStepStateProjectionParts,
    outcome: crate::common::formal::CoreOutcome,
)
    requires
        crate::common::runtime_observation_refines_instr(
            before,
            crate::common::formal::CoreStepInstr::Atomic(atomic_step_from_witness_parts(witness)),
            after,
            outcome,
        ),
    ensures
        atomic_witness_observation_refines_spec_step(before, witness, after, outcome),
{
    lemma_atomic_observation_refines_spec_step(
        before,
        atomic_step_from_witness_parts(witness),
        after,
        outcome,
    );
}

pub(crate) proof fn lemma_atomic_handler_refines_spec_step(
    before: crate::common::CoreStepStateProjectionParts,
    witness: AtomicStepWitnessParts,
    after: crate::common::CoreStepStateProjectionParts,
    outcome: crate::common::formal::CoreOutcome,
)
    requires
        crate::common::runtime_observation_refines_instr(
            before,
            crate::common::formal::CoreStepInstr::Atomic(atomic_step_from_witness_parts(witness)),
            after,
            outcome,
        ),
    ensures
        atomic_witness_observation_refines_spec_step(before, witness, after, outcome),
{
    lemma_atomic_witness_observation_refines_spec_step(before, witness, after, outcome);
}

} // verus!

#[inline(always)]
fn compute_atomic_start(memarg: MemArg, offset: u32) -> VMResult<usize> {
    match checked_compute_memory_offset(memarg.offset, offset) {
        Some(start) => VMResult::Success(start),
        None => VMResult::MemoryIndexOutOfRange,
    }
}

#[inline(always)]
fn compute_atomic_start_indexed(
    memarg: MemArg,
    memidx: u32,
    offset: u32,
) -> VMResult<(usize, u32)> {
    match checked_compute_memory_offset(memarg.offset, offset) {
        Some(start) => VMResult::Success((start, memidx)),
        None => VMResult::MemoryIndexOutOfRange,
    }
}

#[inline(always)]
/// Decode the single `memarg` immediate for the active atomic instruction.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for the current handler.
unsafe fn decode_atomic_memarg(tail_code: *const Instr) -> MemArg {
    (*tail_code).operand.memarg
}

#[inline(always)]
/// Decode the `memarg + memidx` immediates for the active indexed atomic instruction.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for the current handler.
unsafe fn decode_indexed_atomic_memarg(tail_code: *const Instr) -> (MemArg, u32) {
    ((*tail_code).operand.memarg, (*tail_code.add(1)).operand.u32)
}

#[inline(always)]
/// WebAssembly threads atomic offset helper.
///
/// Spec:
/// - Threads: https://webassembly.github.io/threads/core/
///
/// Stack effect: consumes the memory offset operand and computes the effective address.
/// Traps: traps on memory index overflow when computing the effective address.
/// Notes: Reads the memarg from the active instruction and reuses the validated operand stack layout.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction stream for the current active frame.
/// - `ctx` must reference a live execution context whose validated stack layout matches this atomic memory instruction.
/// - This helper must not retain borrows across the call boundary into memory access helpers.
unsafe fn atomic_start(
    tail_code: *const Instr,
    facade: &mut ExecuteContextFacade<'_, '_>,
) -> VMResult<usize> {
    let memarg = decode_atomic_memarg(tail_code);
    let offset = facade.pop_u32();
    compute_atomic_start(memarg, offset)
}

#[inline(always)]
/// WebAssembly threads indexed atomic offset helper.
///
/// Spec:
/// - Threads: https://webassembly.github.io/threads/core/
///
/// Stack effect: consumes the memory offset operand and returns the effective address plus the validated indexed memory immediate.
/// Traps: traps on memory index overflow when computing the effective address.
/// Notes: Reads the memarg and indexed memory immediate from the active instruction stream and reuses the validated operand stack layout.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction stream for the current active frame.
/// - `ctx` must reference a live execution context whose validated stack layout matches this indexed atomic memory instruction.
/// - This helper must not retain borrows across the call boundary into memory access helpers.
unsafe fn atomic_start_indexed(
    tail_code: *const Instr,
    facade: &mut ExecuteContextFacade<'_, '_>,
) -> VMResult<(usize, u32)> {
    let (memarg, memidx) = decode_indexed_atomic_memarg(tail_code);
    let offset = facade.pop_u32();
    compute_atomic_start_indexed(memarg, memidx, offset)
}

#[inline(always)]
unsafe fn push_u32_and_continue(
    tail_code: *const Instr,
    facade: &mut ExecuteContextFacade<'_, '_>,
    skip: isize,
    value: u32,
) -> VMResult<()> {
    vm_try!(facade.push_u32(value));
    facade_call_next(tail_code, skip, facade)
}

#[inline(always)]
unsafe fn push_i32_and_continue(
    tail_code: *const Instr,
    facade: &mut ExecuteContextFacade<'_, '_>,
    skip: isize,
    value: i32,
) -> VMResult<()> {
    vm_try!(facade.push_i32(value));
    facade_call_next(tail_code, skip, facade)
}

#[inline(always)]
unsafe fn pop_notify_operands(
    tail_code: *const Instr,
    facade: &mut ExecuteContextFacade<'_, '_>,
) -> VMResult<(usize, u32)> {
    let count = facade.pop_u32();
    let start = vm_try!(atomic_start(tail_code, facade));
    VMResult::Success((start, count))
}

#[inline(always)]
unsafe fn pop_notify_operands_indexed(
    tail_code: *const Instr,
    facade: &mut ExecuteContextFacade<'_, '_>,
) -> VMResult<(usize, u32, u32)> {
    let count = facade.pop_u32();
    let (start, memidx) = vm_try!(atomic_start_indexed(tail_code, facade));
    VMResult::Success((start, memidx, count))
}

#[inline(always)]
unsafe fn pop_wait32_operands(
    tail_code: *const Instr,
    facade: &mut ExecuteContextFacade<'_, '_>,
) -> VMResult<(usize, u32, i64)> {
    let timeout_ns = facade.pop_i64();
    let expected = facade.pop_u32();
    let start = vm_try!(atomic_start(tail_code, facade));
    VMResult::Success((start, expected, timeout_ns))
}

#[inline(always)]
unsafe fn pop_wait32_operands_indexed(
    tail_code: *const Instr,
    facade: &mut ExecuteContextFacade<'_, '_>,
) -> VMResult<(usize, u32, u32, i64)> {
    let timeout_ns = facade.pop_i64();
    let expected = facade.pop_u32();
    let (start, memidx) = vm_try!(atomic_start_indexed(tail_code, facade));
    VMResult::Success((start, memidx, expected, timeout_ns))
}

#[inline(always)]
unsafe fn pop_wait64_operands(
    tail_code: *const Instr,
    facade: &mut ExecuteContextFacade<'_, '_>,
) -> VMResult<(usize, u64, i64)> {
    let timeout_ns = facade.pop_i64();
    let expected = facade.pop_u64();
    let start = vm_try!(atomic_start(tail_code, facade));
    VMResult::Success((start, expected, timeout_ns))
}

#[inline(always)]
unsafe fn pop_wait64_operands_indexed(
    tail_code: *const Instr,
    facade: &mut ExecuteContextFacade<'_, '_>,
) -> VMResult<(usize, u32, u64, i64)> {
    let timeout_ns = facade.pop_i64();
    let expected = facade.pop_u64();
    let (start, memidx) = vm_try!(atomic_start_indexed(tail_code, facade));
    VMResult::Success((start, memidx, expected, timeout_ns))
}

#[inline(always)]
unsafe fn finish_wait_not_equal(
    tail_code: *const Instr,
    facade: &mut ExecuteContextFacade<'_, '_>,
    skip: isize,
) -> VMResult<()> {
    push_i32_and_continue(tail_code, facade, skip, wait_result_not_equal())
}

macro_rules! atomic_load_op {
    ($name:ident, $reader:ident, $push:ident, $cast:ty) => {
        #[doc = concat!("WebAssembly threads atomic load `", stringify!($name), "`.")]
        #[doc = ""]
        #[doc = "Related spec:"]
        #[doc = "- Threads: https://webassembly.github.io/threads/core/"]
        #[doc = ""]
        #[doc = "Stack effect: `[i32] -> [value]`."]
        #[doc = "Traps: traps on out-of-bounds access or unaligned access."]
        #[doc = "Notes: Observes the threads atomic memory model and tail-dispatches with `call_next`."]
        #[doc = ""]
        #[doc = "# Safety"]
        #[doc = "- `tail_code` must point to the decoded instruction for this handler in the active function body."]
        #[doc = "- `ctx` must reference a live execution context whose validated operand stack and default memory satisfy this instruction."]
        #[doc = "- This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`."]
        pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            let mut facade = ExecuteContextFacade::new(ctx);
            let start = vm_try!(atomic_start(tail_code, &mut facade));
            let value = vm_try!(facade.$reader(start));
            vm_try!(facade.$push(value as $cast));
            facade_call_next(tail_code, 1, &mut facade)
        }
    };
}

macro_rules! atomic_load_op_shared {
    ($name:ident, $reader:ident, $push:ident, $cast:ty) => {
        #[doc = concat!("WebAssembly threads atomic load `", stringify!($name), "` on shared default memory.")]
        ///
        /// Related spec:
        /// - Threads: https://webassembly.github.io/threads/core/
        ///
        /// Stack effect: `[i32] -> [value]`.
        /// Traps: traps on out-of-bounds access or unaligned access.
        /// Notes: Observes the threads atomic memory model and tail-dispatches with `call_next`.
        ///
        /// # Safety
        /// - `tail_code` must point to the decoded instruction for this handler in the active function body.
        /// - `ctx` must reference a live execution context whose default memory is shared.
        /// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
        pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            let mut facade = ExecuteContextFacade::new(ctx);
            let start = vm_try!(atomic_start(tail_code, &mut facade));
            let value = vm_try!(facade.$reader(start));
            vm_try!(facade.$push(value as $cast));
            facade_call_next(tail_code, 1, &mut facade)
        }
    };
}

macro_rules! atomic_store_op {
    ($name:ident, $pop:ident, $writer:ident, $ty:ty) => {
        #[doc = concat!("WebAssembly threads atomic store `", stringify!($name), "`.")]
        #[doc = ""]
        #[doc = "Related spec:"]
        #[doc = "- Threads: https://webassembly.github.io/threads/core/"]
        #[doc = ""]
        #[doc = "Stack effect: `[i32, value] -> []`."]
        #[doc = "Traps: traps on out-of-bounds access or unaligned access."]
        #[doc = "Notes: Performs a sequentially consistent store in the runtime memory model and tail-dispatches with `call_next`."]
        #[doc = ""]
        #[doc = "# Safety"]
        #[doc = "- `tail_code` must point to the decoded instruction for this handler in the active function body."]
        #[doc = "- `ctx` must reference a live execution context whose validated operand stack and default memory satisfy this instruction."]
        #[doc = "- This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`."]
        pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            let mut facade = ExecuteContextFacade::new(ctx);
            let value = facade.$pop() as $ty;
            let start = vm_try!(atomic_start(tail_code, &mut facade));
            vm_try!(facade.$writer(start, value));
            facade_call_next(tail_code, 1, &mut facade)
        }
    };
}

macro_rules! atomic_store_op_shared {
    ($name:ident, $pop:ident, $writer:ident, $ty:ty) => {
        #[doc = concat!("WebAssembly threads atomic store `", stringify!($name), "` on shared default memory.")]
        ///
        /// Related spec:
        /// - Threads: https://webassembly.github.io/threads/core/
        ///
        /// Stack effect: `[i32, value] -> []`.
        /// Traps: traps on out-of-bounds access or unaligned access.
        /// Notes: Performs a sequentially consistent store in the runtime memory model and tail-dispatches with `call_next`.
        ///
        /// # Safety
        /// - `tail_code` must point to the decoded instruction for this handler in the active function body.
        /// - `ctx` must reference a live execution context whose default memory is shared.
        /// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
        pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            let mut facade = ExecuteContextFacade::new(ctx);
            let value = facade.$pop() as $ty;
            let start = vm_try!(atomic_start(tail_code, &mut facade));
            vm_try!(facade.$writer(start, value));
            facade_call_next(tail_code, 1, &mut facade)
        }
    };
}

macro_rules! atomic_rmw_op {
    ($name:ident, $pop:ident, $rmw:ident, $push:ident, $pop_ty:ty, $push_ty:ty, $op:expr) => {
        #[doc = concat!("WebAssembly threads atomic read-modify-write `", stringify!($name), "`.")]
        #[doc = ""]
        #[doc = "Related spec:"]
        #[doc = "- Threads: https://webassembly.github.io/threads/core/"]
        #[doc = ""]
        #[doc = "Stack effect: `[i32, value] -> [old]`."]
        #[doc = "Traps: traps on out-of-bounds access or unaligned access."]
        #[doc = "Notes: Applies the RMW operation under the runtime's shared-memory linearization point, returns the previous value, and tail-dispatches with `call_next`."]
        #[doc = ""]
        #[doc = "# Safety"]
        #[doc = "- `tail_code` must point to the decoded instruction for this handler in the active function body."]
        #[doc = "- `ctx` must reference a live execution context whose validated operand stack and default memory satisfy this instruction."]
        #[doc = "- This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`."]
        pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            let mut facade = ExecuteContextFacade::new(ctx);
            let value = facade.$pop() as $pop_ty;
            let start = vm_try!(atomic_start(tail_code, &mut facade));
            let old = vm_try!(facade.$rmw(start, $op, value));
            vm_try!(facade.$push(old as $push_ty));
            facade_call_next(tail_code, 1, &mut facade)
        }
    };
}

macro_rules! atomic_rmw_op_shared {
    ($name:ident, $pop:ident, $rmw:ident, $push:ident, $pop_ty:ty, $push_ty:ty, $op:expr) => {
        #[doc = concat!("WebAssembly threads atomic read-modify-write `", stringify!($name), "` on shared default memory.")]
        ///
        /// Related spec:
        /// - Threads: https://webassembly.github.io/threads/core/
        ///
        /// Stack effect: `[i32, value] -> [old]`.
        /// Traps: traps on out-of-bounds access or unaligned access.
        /// Notes: Applies the RMW operation under the runtime's shared-memory linearization point, returns the previous value, and tail-dispatches with `call_next`.
        ///
        /// # Safety
        /// - `tail_code` must point to the decoded instruction for this handler in the active function body.
        /// - `ctx` must reference a live execution context whose default memory is shared.
        /// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
        pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            let mut facade = ExecuteContextFacade::new(ctx);
            let value = facade.$pop() as $pop_ty;
            let start = vm_try!(atomic_start(tail_code, &mut facade));
            let old = vm_try!(facade.$rmw(start, $op, value));
            vm_try!(facade.$push(old as $push_ty));
            facade_call_next(tail_code, 1, &mut facade)
        }
    };
}

macro_rules! atomic_cmpxchg_op {
    ($name:ident, $pop:ident, $cmpxchg:ident, $push:ident, $ty:ty, $push_ty:ty) => {
        #[doc = concat!("WebAssembly threads atomic compare-exchange `", stringify!($name), "`.")]
        #[doc = ""]
        #[doc = "Related spec:"]
        #[doc = "- Threads: https://webassembly.github.io/threads/core/"]
        #[doc = ""]
        #[doc = "Stack effect: `[i32, expected, replacement] -> [old]`."]
        #[doc = "Traps: traps on out-of-bounds access or unaligned access."]
        #[doc = "Notes: Compares the current memory value with `expected`, conditionally stores `replacement`, returns the observed old value, and tail-dispatches with `call_next`."]
        #[doc = ""]
        #[doc = "# Safety"]
        #[doc = "- `tail_code` must point to the decoded instruction for this handler in the active function body."]
        #[doc = "- `ctx` must reference a live execution context whose validated operand stack and default memory satisfy this instruction."]
        #[doc = "- This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`."]
        pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            let mut facade = ExecuteContextFacade::new(ctx);
            let value = facade.$pop() as $ty;
            let expected = facade.$pop() as $ty;
            let start = vm_try!(atomic_start(tail_code, &mut facade));
            let old = vm_try!(facade.$cmpxchg(start, expected, value));
            vm_try!(facade.$push(old as $push_ty));
            facade_call_next(tail_code, 1, &mut facade)
        }
    };
}

macro_rules! atomic_cmpxchg_op_shared {
    ($name:ident, $pop:ident, $cmpxchg:ident, $push:ident, $ty:ty, $push_ty:ty) => {
        #[doc = concat!("WebAssembly threads atomic compare-exchange `", stringify!($name), "` on shared default memory.")]
        ///
        /// Related spec:
        /// - Threads: https://webassembly.github.io/threads/core/
        ///
        /// Stack effect: `[i32, expected, replacement] -> [old]`.
        /// Traps: traps on out-of-bounds access or unaligned access.
        /// Notes: Compares the current memory value with `expected`, conditionally stores `replacement`, returns the observed old value, and tail-dispatches with `call_next`.
        ///
        /// # Safety
        /// - `tail_code` must point to the decoded instruction for this handler in the active function body.
        /// - `ctx` must reference a live execution context whose default memory is shared.
        /// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
        pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            let mut facade = ExecuteContextFacade::new(ctx);
            let value = facade.$pop() as $ty;
            let expected = facade.$pop() as $ty;
            let start = vm_try!(atomic_start(tail_code, &mut facade));
            let old = vm_try!(facade.$cmpxchg(start, expected, value));
            vm_try!(facade.$push(old as $push_ty));
            facade_call_next(tail_code, 1, &mut facade)
        }
    };
}

macro_rules! atomic_load_op_indexed {
    ($local:ident, $shared:ident, $reader_local:ident, $reader_shared:ident, $push:ident, $cast:ty) => {
        #[doc = concat!("WebAssembly threads atomic load `", stringify!($local), "` on indexed local memory.")]
        ///
        /// Related spec:
        /// - Threads: https://webassembly.github.io/threads/core/
        ///
        /// Stack effect: `[i32] -> [value]`.
        /// Traps: traps on out-of-bounds access or unaligned access.
        /// Notes: Uses the typed indexed local-memory atomic fast path and tail-dispatches with `call_next`.
        ///
        /// # Safety
        /// - `tail_code` must point to the decoded instruction for this handler in the active function body.
        /// - `ctx` must reference a live execution context whose indexed memory operand is in-bounds and local.
        pub unsafe fn $local(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            let mut facade = ExecuteContextFacade::new(ctx);
            let (start, memidx) = vm_try!(atomic_start_indexed(tail_code, &mut facade));
            let value = vm_try!(unsafe { facade.$reader_local(memidx, start) });
            vm_try!(facade.$push(value as $cast));
            facade_call_next(tail_code, 2, &mut facade)
        }

        #[doc = concat!("WebAssembly threads atomic load `", stringify!($shared), "` on indexed shared memory.")]
        ///
        /// Related spec:
        /// - Threads: https://webassembly.github.io/threads/core/
        ///
        /// Stack effect: `[i32] -> [value]`.
        /// Traps: traps on out-of-bounds access or unaligned access.
        /// Notes: Uses the typed indexed shared-memory atomic fast path and tail-dispatches with `call_next`.
        ///
        /// # Safety
        /// - `tail_code` must point to the decoded instruction for this handler in the active function body.
        /// - `ctx` must reference a live execution context whose indexed memory operand is in-bounds and shared.
        pub unsafe fn $shared(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            let mut facade = ExecuteContextFacade::new(ctx);
            let (start, memidx) = vm_try!(atomic_start_indexed(tail_code, &mut facade));
            let value = vm_try!(unsafe { facade.$reader_shared(memidx, start) });
            vm_try!(facade.$push(value as $cast));
            facade_call_next(tail_code, 2, &mut facade)
        }
    };
}

macro_rules! atomic_store_op_indexed {
    ($local:ident, $shared:ident, $pop:ident, $writer_local:ident, $writer_shared:ident, $ty:ty) => {
        #[doc = concat!("WebAssembly threads atomic store `", stringify!($local), "` on indexed local memory.")]
        ///
        /// Related spec:
        /// - Threads: https://webassembly.github.io/threads/core/
        ///
        /// Stack effect: `[i32, value] -> []`.
        /// Traps: traps on out-of-bounds access or unaligned access.
        /// Notes: Uses the typed indexed local-memory atomic fast path and tail-dispatches with `call_next`.
        ///
        /// # Safety
        /// - `tail_code` must point to the decoded instruction for this handler in the active function body.
        /// - `ctx` must reference a live execution context whose indexed memory operand is in-bounds and local.
        pub unsafe fn $local(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            let mut facade = ExecuteContextFacade::new(ctx);
            let value = facade.$pop() as $ty;
            let (start, memidx) = vm_try!(atomic_start_indexed(tail_code, &mut facade));
            vm_try!(unsafe { facade.$writer_local(memidx, start, value) });
            facade_call_next(tail_code, 2, &mut facade)
        }

        #[doc = concat!("WebAssembly threads atomic store `", stringify!($shared), "` on indexed shared memory.")]
        ///
        /// Related spec:
        /// - Threads: https://webassembly.github.io/threads/core/
        ///
        /// Stack effect: `[i32, value] -> []`.
        /// Traps: traps on out-of-bounds access or unaligned access.
        /// Notes: Uses the typed indexed shared-memory atomic fast path and tail-dispatches with `call_next`.
        ///
        /// # Safety
        /// - `tail_code` must point to the decoded instruction for this handler in the active function body.
        /// - `ctx` must reference a live execution context whose indexed memory operand is in-bounds and shared.
        pub unsafe fn $shared(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            let mut facade = ExecuteContextFacade::new(ctx);
            let value = facade.$pop() as $ty;
            let (start, memidx) = vm_try!(atomic_start_indexed(tail_code, &mut facade));
            vm_try!(unsafe { facade.$writer_shared(memidx, start, value) });
            facade_call_next(tail_code, 2, &mut facade)
        }
    };
}

macro_rules! atomic_rmw_op_indexed {
    ($local:ident, $shared:ident, $pop:ident, $rmw_local:ident, $rmw_shared:ident, $push:ident, $pop_ty:ty, $push_ty:ty, $op:expr) => {
        #[doc = concat!("WebAssembly threads atomic read-modify-write `", stringify!($local), "` on indexed local memory.")]
        ///
        /// Related spec:
        /// - Threads: https://webassembly.github.io/threads/core/
        ///
        /// Stack effect: `[i32, value] -> [old]`.
        /// Traps: traps on out-of-bounds access or unaligned access.
        /// Notes: Uses the typed indexed local-memory atomic fast path and tail-dispatches with `call_next`.
        ///
        /// # Safety
        /// - `tail_code` must point to the decoded instruction for this handler in the active function body.
        /// - `ctx` must reference a live execution context whose indexed memory operand is in-bounds and local.
        pub unsafe fn $local(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            let mut facade = ExecuteContextFacade::new(ctx);
            let value = facade.$pop() as $pop_ty;
            let (start, memidx) = vm_try!(atomic_start_indexed(tail_code, &mut facade));
            let old = vm_try!(unsafe { facade.$rmw_local(memidx, start, $op, value) });
            vm_try!(facade.$push(old as $push_ty));
            facade_call_next(tail_code, 2, &mut facade)
        }

        #[doc = concat!("WebAssembly threads atomic read-modify-write `", stringify!($shared), "` on indexed shared memory.")]
        ///
        /// Related spec:
        /// - Threads: https://webassembly.github.io/threads/core/
        ///
        /// Stack effect: `[i32, value] -> [old]`.
        /// Traps: traps on out-of-bounds access or unaligned access.
        /// Notes: Uses the typed indexed shared-memory atomic fast path and tail-dispatches with `call_next`.
        ///
        /// # Safety
        /// - `tail_code` must point to the decoded instruction for this handler in the active function body.
        /// - `ctx` must reference a live execution context whose indexed memory operand is in-bounds and shared.
        pub unsafe fn $shared(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            let mut facade = ExecuteContextFacade::new(ctx);
            let value = facade.$pop() as $pop_ty;
            let (start, memidx) = vm_try!(atomic_start_indexed(tail_code, &mut facade));
            let old = vm_try!(unsafe { facade.$rmw_shared(memidx, start, $op, value) });
            vm_try!(facade.$push(old as $push_ty));
            facade_call_next(tail_code, 2, &mut facade)
        }
    };
}

macro_rules! atomic_cmpxchg_op_indexed {
    ($local:ident, $shared:ident, $pop:ident, $cmpxchg_local:ident, $cmpxchg_shared:ident, $push:ident, $ty:ty, $push_ty:ty) => {
        #[doc = concat!("WebAssembly threads atomic compare-exchange `", stringify!($local), "` on indexed local memory.")]
        ///
        /// Related spec:
        /// - Threads: https://webassembly.github.io/threads/core/
        ///
        /// Stack effect: `[i32, expected, replacement] -> [old]`.
        /// Traps: traps on out-of-bounds access or unaligned access.
        /// Notes: Uses the typed indexed local-memory atomic fast path and tail-dispatches with `call_next`.
        ///
        /// # Safety
        /// - `tail_code` must point to the decoded instruction for this handler in the active function body.
        /// - `ctx` must reference a live execution context whose indexed memory operand is in-bounds and local.
        pub unsafe fn $local(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            let mut facade = ExecuteContextFacade::new(ctx);
            let value = facade.$pop() as $ty;
            let expected = facade.$pop() as $ty;
            let (start, memidx) = vm_try!(atomic_start_indexed(tail_code, &mut facade));
            let old = vm_try!(unsafe { facade.$cmpxchg_local(memidx, start, expected, value) });
            vm_try!(facade.$push(old as $push_ty));
            facade_call_next(tail_code, 2, &mut facade)
        }

        #[doc = concat!("WebAssembly threads atomic compare-exchange `", stringify!($shared), "` on indexed shared memory.")]
        ///
        /// Related spec:
        /// - Threads: https://webassembly.github.io/threads/core/
        ///
        /// Stack effect: `[i32, expected, replacement] -> [old]`.
        /// Traps: traps on out-of-bounds access or unaligned access.
        /// Notes: Uses the typed indexed shared-memory atomic fast path and tail-dispatches with `call_next`.
        ///
        /// # Safety
        /// - `tail_code` must point to the decoded instruction for this handler in the active function body.
        /// - `ctx` must reference a live execution context whose indexed memory operand is in-bounds and shared.
        pub unsafe fn $shared(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            let mut facade = ExecuteContextFacade::new(ctx);
            let value = facade.$pop() as $ty;
            let expected = facade.$pop() as $ty;
            let (start, memidx) = vm_try!(atomic_start_indexed(tail_code, &mut facade));
            let old = vm_try!(unsafe { facade.$cmpxchg_shared(memidx, start, expected, value) });
            vm_try!(facade.$push(old as $push_ty));
            facade_call_next(tail_code, 2, &mut facade)
        }
    };
}

atomic_load_op!(op_i32_atomic_load, local_atomic_load_u32, push_u32, u32);
atomic_load_op!(op_i64_atomic_load, local_atomic_load_u64, push_u64, u64);
atomic_load_op!(op_i32_atomic_load8_u, local_atomic_load_u8, push_u32, u32);
atomic_load_op!(op_i32_atomic_load16_u, local_atomic_load_u16, push_u32, u32);
atomic_load_op!(op_i64_atomic_load8_u, local_atomic_load_u8, push_u64, u64);
atomic_load_op!(op_i64_atomic_load16_u, local_atomic_load_u16, push_u64, u64);
atomic_load_op!(op_i64_atomic_load32_u, local_atomic_load_u32, push_u64, u64);

atomic_store_op!(op_i32_atomic_store, pop_u32, local_atomic_store_u32, u32);
atomic_store_op!(op_i64_atomic_store, pop_u64, local_atomic_store_u64, u64);
atomic_store_op!(op_i32_atomic_store8, pop_u32, local_atomic_store_u8, u8);
atomic_store_op!(op_i32_atomic_store16, pop_u32, local_atomic_store_u16, u16);
atomic_store_op!(op_i64_atomic_store8, pop_u64, local_atomic_store_u8, u8);
atomic_store_op!(op_i64_atomic_store16, pop_u64, local_atomic_store_u16, u16);
atomic_store_op!(op_i64_atomic_store32, pop_u64, local_atomic_store_u32, u32);

atomic_rmw_op!(
    op_i32_atomic_rmw_add,
    pop_u32,
    local_atomic_rmw_u32,
    push_u32,
    u32,
    u32,
    AtomicRmwOp::Add
);
atomic_rmw_op!(
    op_i64_atomic_rmw_add,
    pop_u64,
    local_atomic_rmw_u64,
    push_u64,
    u64,
    u64,
    AtomicRmwOp::Add
);
atomic_rmw_op!(
    op_i32_atomic_rmw8_add_u,
    pop_u32,
    local_atomic_rmw_u8,
    push_u32,
    u8,
    u32,
    AtomicRmwOp::Add
);
atomic_rmw_op!(
    op_i32_atomic_rmw16_add_u,
    pop_u32,
    local_atomic_rmw_u16,
    push_u32,
    u16,
    u32,
    AtomicRmwOp::Add
);
atomic_rmw_op!(
    op_i64_atomic_rmw8_add_u,
    pop_u64,
    local_atomic_rmw_u8,
    push_u64,
    u8,
    u64,
    AtomicRmwOp::Add
);
atomic_rmw_op!(
    op_i64_atomic_rmw16_add_u,
    pop_u64,
    local_atomic_rmw_u16,
    push_u64,
    u16,
    u64,
    AtomicRmwOp::Add
);
atomic_rmw_op!(
    op_i64_atomic_rmw32_add_u,
    pop_u64,
    local_atomic_rmw_u32,
    push_u64,
    u32,
    u64,
    AtomicRmwOp::Add
);

atomic_rmw_op!(
    op_i32_atomic_rmw_sub,
    pop_u32,
    local_atomic_rmw_u32,
    push_u32,
    u32,
    u32,
    AtomicRmwOp::Sub
);
atomic_rmw_op!(
    op_i64_atomic_rmw_sub,
    pop_u64,
    local_atomic_rmw_u64,
    push_u64,
    u64,
    u64,
    AtomicRmwOp::Sub
);
atomic_rmw_op!(
    op_i32_atomic_rmw8_sub_u,
    pop_u32,
    local_atomic_rmw_u8,
    push_u32,
    u8,
    u32,
    AtomicRmwOp::Sub
);
atomic_rmw_op!(
    op_i32_atomic_rmw16_sub_u,
    pop_u32,
    local_atomic_rmw_u16,
    push_u32,
    u16,
    u32,
    AtomicRmwOp::Sub
);
atomic_rmw_op!(
    op_i64_atomic_rmw8_sub_u,
    pop_u64,
    local_atomic_rmw_u8,
    push_u64,
    u8,
    u64,
    AtomicRmwOp::Sub
);
atomic_rmw_op!(
    op_i64_atomic_rmw16_sub_u,
    pop_u64,
    local_atomic_rmw_u16,
    push_u64,
    u16,
    u64,
    AtomicRmwOp::Sub
);
atomic_rmw_op!(
    op_i64_atomic_rmw32_sub_u,
    pop_u64,
    local_atomic_rmw_u32,
    push_u64,
    u32,
    u64,
    AtomicRmwOp::Sub
);

atomic_rmw_op!(
    op_i32_atomic_rmw_and,
    pop_u32,
    local_atomic_rmw_u32,
    push_u32,
    u32,
    u32,
    AtomicRmwOp::And
);
atomic_rmw_op!(
    op_i64_atomic_rmw_and,
    pop_u64,
    local_atomic_rmw_u64,
    push_u64,
    u64,
    u64,
    AtomicRmwOp::And
);
atomic_rmw_op!(
    op_i32_atomic_rmw8_and_u,
    pop_u32,
    local_atomic_rmw_u8,
    push_u32,
    u8,
    u32,
    AtomicRmwOp::And
);
atomic_rmw_op!(
    op_i32_atomic_rmw16_and_u,
    pop_u32,
    local_atomic_rmw_u16,
    push_u32,
    u16,
    u32,
    AtomicRmwOp::And
);
atomic_rmw_op!(
    op_i64_atomic_rmw8_and_u,
    pop_u64,
    local_atomic_rmw_u8,
    push_u64,
    u8,
    u64,
    AtomicRmwOp::And
);
atomic_rmw_op!(
    op_i64_atomic_rmw16_and_u,
    pop_u64,
    local_atomic_rmw_u16,
    push_u64,
    u16,
    u64,
    AtomicRmwOp::And
);
atomic_rmw_op!(
    op_i64_atomic_rmw32_and_u,
    pop_u64,
    local_atomic_rmw_u32,
    push_u64,
    u32,
    u64,
    AtomicRmwOp::And
);

atomic_rmw_op!(
    op_i32_atomic_rmw_or,
    pop_u32,
    local_atomic_rmw_u32,
    push_u32,
    u32,
    u32,
    AtomicRmwOp::Or
);
atomic_rmw_op!(
    op_i64_atomic_rmw_or,
    pop_u64,
    local_atomic_rmw_u64,
    push_u64,
    u64,
    u64,
    AtomicRmwOp::Or
);
atomic_rmw_op!(
    op_i32_atomic_rmw8_or_u,
    pop_u32,
    local_atomic_rmw_u8,
    push_u32,
    u8,
    u32,
    AtomicRmwOp::Or
);
atomic_rmw_op!(
    op_i32_atomic_rmw16_or_u,
    pop_u32,
    local_atomic_rmw_u16,
    push_u32,
    u16,
    u32,
    AtomicRmwOp::Or
);
atomic_rmw_op!(
    op_i64_atomic_rmw8_or_u,
    pop_u64,
    local_atomic_rmw_u8,
    push_u64,
    u8,
    u64,
    AtomicRmwOp::Or
);
atomic_rmw_op!(
    op_i64_atomic_rmw16_or_u,
    pop_u64,
    local_atomic_rmw_u16,
    push_u64,
    u16,
    u64,
    AtomicRmwOp::Or
);
atomic_rmw_op!(
    op_i64_atomic_rmw32_or_u,
    pop_u64,
    local_atomic_rmw_u32,
    push_u64,
    u32,
    u64,
    AtomicRmwOp::Or
);

atomic_rmw_op!(
    op_i32_atomic_rmw_xor,
    pop_u32,
    local_atomic_rmw_u32,
    push_u32,
    u32,
    u32,
    AtomicRmwOp::Xor
);
atomic_rmw_op!(
    op_i64_atomic_rmw_xor,
    pop_u64,
    local_atomic_rmw_u64,
    push_u64,
    u64,
    u64,
    AtomicRmwOp::Xor
);
atomic_rmw_op!(
    op_i32_atomic_rmw8_xor_u,
    pop_u32,
    local_atomic_rmw_u8,
    push_u32,
    u8,
    u32,
    AtomicRmwOp::Xor
);
atomic_rmw_op!(
    op_i32_atomic_rmw16_xor_u,
    pop_u32,
    local_atomic_rmw_u16,
    push_u32,
    u16,
    u32,
    AtomicRmwOp::Xor
);
atomic_rmw_op!(
    op_i64_atomic_rmw8_xor_u,
    pop_u64,
    local_atomic_rmw_u8,
    push_u64,
    u8,
    u64,
    AtomicRmwOp::Xor
);
atomic_rmw_op!(
    op_i64_atomic_rmw16_xor_u,
    pop_u64,
    local_atomic_rmw_u16,
    push_u64,
    u16,
    u64,
    AtomicRmwOp::Xor
);
atomic_rmw_op!(
    op_i64_atomic_rmw32_xor_u,
    pop_u64,
    local_atomic_rmw_u32,
    push_u64,
    u32,
    u64,
    AtomicRmwOp::Xor
);

atomic_rmw_op!(
    op_i32_atomic_rmw_xchg,
    pop_u32,
    local_atomic_rmw_u32,
    push_u32,
    u32,
    u32,
    AtomicRmwOp::Xchg
);
atomic_rmw_op!(
    op_i64_atomic_rmw_xchg,
    pop_u64,
    local_atomic_rmw_u64,
    push_u64,
    u64,
    u64,
    AtomicRmwOp::Xchg
);
atomic_rmw_op!(
    op_i32_atomic_rmw8_xchg_u,
    pop_u32,
    local_atomic_rmw_u8,
    push_u32,
    u8,
    u32,
    AtomicRmwOp::Xchg
);
atomic_rmw_op!(
    op_i32_atomic_rmw16_xchg_u,
    pop_u32,
    local_atomic_rmw_u16,
    push_u32,
    u16,
    u32,
    AtomicRmwOp::Xchg
);
atomic_rmw_op!(
    op_i64_atomic_rmw8_xchg_u,
    pop_u64,
    local_atomic_rmw_u8,
    push_u64,
    u8,
    u64,
    AtomicRmwOp::Xchg
);
atomic_rmw_op!(
    op_i64_atomic_rmw16_xchg_u,
    pop_u64,
    local_atomic_rmw_u16,
    push_u64,
    u16,
    u64,
    AtomicRmwOp::Xchg
);
atomic_rmw_op!(
    op_i64_atomic_rmw32_xchg_u,
    pop_u64,
    local_atomic_rmw_u32,
    push_u64,
    u32,
    u64,
    AtomicRmwOp::Xchg
);

atomic_cmpxchg_op!(
    op_i32_atomic_rmw_cmpxchg,
    pop_u32,
    local_atomic_cmpxchg_u32,
    push_u32,
    u32,
    u32
);
atomic_cmpxchg_op!(
    op_i64_atomic_rmw_cmpxchg,
    pop_u64,
    local_atomic_cmpxchg_u64,
    push_u64,
    u64,
    u64
);
atomic_cmpxchg_op!(
    op_i32_atomic_rmw8_cmpxchg_u,
    pop_u32,
    local_atomic_cmpxchg_u8,
    push_u32,
    u8,
    u32
);
atomic_cmpxchg_op!(
    op_i32_atomic_rmw16_cmpxchg_u,
    pop_u32,
    local_atomic_cmpxchg_u16,
    push_u32,
    u16,
    u32
);
atomic_cmpxchg_op!(
    op_i64_atomic_rmw8_cmpxchg_u,
    pop_u64,
    local_atomic_cmpxchg_u8,
    push_u64,
    u8,
    u64
);
atomic_cmpxchg_op!(
    op_i64_atomic_rmw16_cmpxchg_u,
    pop_u64,
    local_atomic_cmpxchg_u16,
    push_u64,
    u16,
    u64
);
atomic_cmpxchg_op!(
    op_i64_atomic_rmw32_cmpxchg_u,
    pop_u64,
    local_atomic_cmpxchg_u32,
    push_u64,
    u32,
    u64
);

atomic_load_op_shared!(
    op_i32_atomic_load_shared,
    shared_atomic_load_u32,
    push_u32,
    u32
);
atomic_load_op_shared!(
    op_i64_atomic_load_shared,
    shared_atomic_load_u64,
    push_u64,
    u64
);
atomic_load_op_shared!(
    op_i32_atomic_load8_u_shared,
    shared_atomic_load_u8,
    push_u32,
    u32
);
atomic_load_op_shared!(
    op_i32_atomic_load16_u_shared,
    shared_atomic_load_u16,
    push_u32,
    u32
);
atomic_load_op_shared!(
    op_i64_atomic_load8_u_shared,
    shared_atomic_load_u8,
    push_u64,
    u64
);
atomic_load_op_shared!(
    op_i64_atomic_load16_u_shared,
    shared_atomic_load_u16,
    push_u64,
    u64
);
atomic_load_op_shared!(
    op_i64_atomic_load32_u_shared,
    shared_atomic_load_u32,
    push_u64,
    u64
);

atomic_store_op_shared!(
    op_i32_atomic_store_shared,
    pop_u32,
    shared_atomic_store_u32,
    u32
);
atomic_store_op_shared!(
    op_i64_atomic_store_shared,
    pop_u64,
    shared_atomic_store_u64,
    u64
);
atomic_store_op_shared!(
    op_i32_atomic_store8_shared,
    pop_u32,
    shared_atomic_store_u8,
    u8
);
atomic_store_op_shared!(
    op_i32_atomic_store16_shared,
    pop_u32,
    shared_atomic_store_u16,
    u16
);
atomic_store_op_shared!(
    op_i64_atomic_store8_shared,
    pop_u64,
    shared_atomic_store_u8,
    u8
);
atomic_store_op_shared!(
    op_i64_atomic_store16_shared,
    pop_u64,
    shared_atomic_store_u16,
    u16
);
atomic_store_op_shared!(
    op_i64_atomic_store32_shared,
    pop_u64,
    shared_atomic_store_u32,
    u32
);

atomic_rmw_op_shared!(
    op_i32_atomic_rmw_add_shared,
    pop_u32,
    shared_atomic_rmw_u32,
    push_u32,
    u32,
    u32,
    AtomicRmwOp::Add
);
atomic_rmw_op_shared!(
    op_i64_atomic_rmw_add_shared,
    pop_u64,
    shared_atomic_rmw_u64,
    push_u64,
    u64,
    u64,
    AtomicRmwOp::Add
);
atomic_rmw_op_shared!(
    op_i32_atomic_rmw8_add_u_shared,
    pop_u32,
    shared_atomic_rmw_u8,
    push_u32,
    u8,
    u32,
    AtomicRmwOp::Add
);
atomic_rmw_op_shared!(
    op_i32_atomic_rmw16_add_u_shared,
    pop_u32,
    shared_atomic_rmw_u16,
    push_u32,
    u16,
    u32,
    AtomicRmwOp::Add
);
atomic_rmw_op_shared!(
    op_i64_atomic_rmw8_add_u_shared,
    pop_u64,
    shared_atomic_rmw_u8,
    push_u64,
    u8,
    u64,
    AtomicRmwOp::Add
);
atomic_rmw_op_shared!(
    op_i64_atomic_rmw16_add_u_shared,
    pop_u64,
    shared_atomic_rmw_u16,
    push_u64,
    u16,
    u64,
    AtomicRmwOp::Add
);
atomic_rmw_op_shared!(
    op_i64_atomic_rmw32_add_u_shared,
    pop_u64,
    shared_atomic_rmw_u32,
    push_u64,
    u32,
    u64,
    AtomicRmwOp::Add
);
atomic_rmw_op_shared!(
    op_i32_atomic_rmw_sub_shared,
    pop_u32,
    shared_atomic_rmw_u32,
    push_u32,
    u32,
    u32,
    AtomicRmwOp::Sub
);
atomic_rmw_op_shared!(
    op_i64_atomic_rmw_sub_shared,
    pop_u64,
    shared_atomic_rmw_u64,
    push_u64,
    u64,
    u64,
    AtomicRmwOp::Sub
);
atomic_rmw_op_shared!(
    op_i32_atomic_rmw8_sub_u_shared,
    pop_u32,
    shared_atomic_rmw_u8,
    push_u32,
    u8,
    u32,
    AtomicRmwOp::Sub
);
atomic_rmw_op_shared!(
    op_i32_atomic_rmw16_sub_u_shared,
    pop_u32,
    shared_atomic_rmw_u16,
    push_u32,
    u16,
    u32,
    AtomicRmwOp::Sub
);
atomic_rmw_op_shared!(
    op_i64_atomic_rmw8_sub_u_shared,
    pop_u64,
    shared_atomic_rmw_u8,
    push_u64,
    u8,
    u64,
    AtomicRmwOp::Sub
);
atomic_rmw_op_shared!(
    op_i64_atomic_rmw16_sub_u_shared,
    pop_u64,
    shared_atomic_rmw_u16,
    push_u64,
    u16,
    u64,
    AtomicRmwOp::Sub
);
atomic_rmw_op_shared!(
    op_i64_atomic_rmw32_sub_u_shared,
    pop_u64,
    shared_atomic_rmw_u32,
    push_u64,
    u32,
    u64,
    AtomicRmwOp::Sub
);
atomic_rmw_op_shared!(
    op_i32_atomic_rmw_and_shared,
    pop_u32,
    shared_atomic_rmw_u32,
    push_u32,
    u32,
    u32,
    AtomicRmwOp::And
);
atomic_rmw_op_shared!(
    op_i64_atomic_rmw_and_shared,
    pop_u64,
    shared_atomic_rmw_u64,
    push_u64,
    u64,
    u64,
    AtomicRmwOp::And
);
atomic_rmw_op_shared!(
    op_i32_atomic_rmw8_and_u_shared,
    pop_u32,
    shared_atomic_rmw_u8,
    push_u32,
    u8,
    u32,
    AtomicRmwOp::And
);
atomic_rmw_op_shared!(
    op_i32_atomic_rmw16_and_u_shared,
    pop_u32,
    shared_atomic_rmw_u16,
    push_u32,
    u16,
    u32,
    AtomicRmwOp::And
);
atomic_rmw_op_shared!(
    op_i64_atomic_rmw8_and_u_shared,
    pop_u64,
    shared_atomic_rmw_u8,
    push_u64,
    u8,
    u64,
    AtomicRmwOp::And
);
atomic_rmw_op_shared!(
    op_i64_atomic_rmw16_and_u_shared,
    pop_u64,
    shared_atomic_rmw_u16,
    push_u64,
    u16,
    u64,
    AtomicRmwOp::And
);
atomic_rmw_op_shared!(
    op_i64_atomic_rmw32_and_u_shared,
    pop_u64,
    shared_atomic_rmw_u32,
    push_u64,
    u32,
    u64,
    AtomicRmwOp::And
);
atomic_rmw_op_shared!(
    op_i32_atomic_rmw_or_shared,
    pop_u32,
    shared_atomic_rmw_u32,
    push_u32,
    u32,
    u32,
    AtomicRmwOp::Or
);
atomic_rmw_op_shared!(
    op_i64_atomic_rmw_or_shared,
    pop_u64,
    shared_atomic_rmw_u64,
    push_u64,
    u64,
    u64,
    AtomicRmwOp::Or
);
atomic_rmw_op_shared!(
    op_i32_atomic_rmw8_or_u_shared,
    pop_u32,
    shared_atomic_rmw_u8,
    push_u32,
    u8,
    u32,
    AtomicRmwOp::Or
);
atomic_rmw_op_shared!(
    op_i32_atomic_rmw16_or_u_shared,
    pop_u32,
    shared_atomic_rmw_u16,
    push_u32,
    u16,
    u32,
    AtomicRmwOp::Or
);
atomic_rmw_op_shared!(
    op_i64_atomic_rmw8_or_u_shared,
    pop_u64,
    shared_atomic_rmw_u8,
    push_u64,
    u8,
    u64,
    AtomicRmwOp::Or
);
atomic_rmw_op_shared!(
    op_i64_atomic_rmw16_or_u_shared,
    pop_u64,
    shared_atomic_rmw_u16,
    push_u64,
    u16,
    u64,
    AtomicRmwOp::Or
);
atomic_rmw_op_shared!(
    op_i64_atomic_rmw32_or_u_shared,
    pop_u64,
    shared_atomic_rmw_u32,
    push_u64,
    u32,
    u64,
    AtomicRmwOp::Or
);
atomic_rmw_op_shared!(
    op_i32_atomic_rmw_xor_shared,
    pop_u32,
    shared_atomic_rmw_u32,
    push_u32,
    u32,
    u32,
    AtomicRmwOp::Xor
);
atomic_rmw_op_shared!(
    op_i64_atomic_rmw_xor_shared,
    pop_u64,
    shared_atomic_rmw_u64,
    push_u64,
    u64,
    u64,
    AtomicRmwOp::Xor
);
atomic_rmw_op_shared!(
    op_i32_atomic_rmw8_xor_u_shared,
    pop_u32,
    shared_atomic_rmw_u8,
    push_u32,
    u8,
    u32,
    AtomicRmwOp::Xor
);
atomic_rmw_op_shared!(
    op_i32_atomic_rmw16_xor_u_shared,
    pop_u32,
    shared_atomic_rmw_u16,
    push_u32,
    u16,
    u32,
    AtomicRmwOp::Xor
);
atomic_rmw_op_shared!(
    op_i64_atomic_rmw8_xor_u_shared,
    pop_u64,
    shared_atomic_rmw_u8,
    push_u64,
    u8,
    u64,
    AtomicRmwOp::Xor
);
atomic_rmw_op_shared!(
    op_i64_atomic_rmw16_xor_u_shared,
    pop_u64,
    shared_atomic_rmw_u16,
    push_u64,
    u16,
    u64,
    AtomicRmwOp::Xor
);
atomic_rmw_op_shared!(
    op_i64_atomic_rmw32_xor_u_shared,
    pop_u64,
    shared_atomic_rmw_u32,
    push_u64,
    u32,
    u64,
    AtomicRmwOp::Xor
);
atomic_rmw_op_shared!(
    op_i32_atomic_rmw_xchg_shared,
    pop_u32,
    shared_atomic_rmw_u32,
    push_u32,
    u32,
    u32,
    AtomicRmwOp::Xchg
);
atomic_rmw_op_shared!(
    op_i64_atomic_rmw_xchg_shared,
    pop_u64,
    shared_atomic_rmw_u64,
    push_u64,
    u64,
    u64,
    AtomicRmwOp::Xchg
);
atomic_rmw_op_shared!(
    op_i32_atomic_rmw8_xchg_u_shared,
    pop_u32,
    shared_atomic_rmw_u8,
    push_u32,
    u8,
    u32,
    AtomicRmwOp::Xchg
);
atomic_rmw_op_shared!(
    op_i32_atomic_rmw16_xchg_u_shared,
    pop_u32,
    shared_atomic_rmw_u16,
    push_u32,
    u16,
    u32,
    AtomicRmwOp::Xchg
);
atomic_rmw_op_shared!(
    op_i64_atomic_rmw8_xchg_u_shared,
    pop_u64,
    shared_atomic_rmw_u8,
    push_u64,
    u8,
    u64,
    AtomicRmwOp::Xchg
);
atomic_rmw_op_shared!(
    op_i64_atomic_rmw16_xchg_u_shared,
    pop_u64,
    shared_atomic_rmw_u16,
    push_u64,
    u16,
    u64,
    AtomicRmwOp::Xchg
);
atomic_rmw_op_shared!(
    op_i64_atomic_rmw32_xchg_u_shared,
    pop_u64,
    shared_atomic_rmw_u32,
    push_u64,
    u32,
    u64,
    AtomicRmwOp::Xchg
);
atomic_cmpxchg_op_shared!(
    op_i32_atomic_rmw_cmpxchg_shared,
    pop_u32,
    shared_atomic_cmpxchg_u32,
    push_u32,
    u32,
    u32
);
atomic_cmpxchg_op_shared!(
    op_i64_atomic_rmw_cmpxchg_shared,
    pop_u64,
    shared_atomic_cmpxchg_u64,
    push_u64,
    u64,
    u64
);
atomic_cmpxchg_op_shared!(
    op_i32_atomic_rmw8_cmpxchg_u_shared,
    pop_u32,
    shared_atomic_cmpxchg_u8,
    push_u32,
    u8,
    u32
);
atomic_cmpxchg_op_shared!(
    op_i32_atomic_rmw16_cmpxchg_u_shared,
    pop_u32,
    shared_atomic_cmpxchg_u16,
    push_u32,
    u16,
    u32
);
atomic_cmpxchg_op_shared!(
    op_i64_atomic_rmw8_cmpxchg_u_shared,
    pop_u64,
    shared_atomic_cmpxchg_u8,
    push_u64,
    u8,
    u64
);
atomic_cmpxchg_op_shared!(
    op_i64_atomic_rmw16_cmpxchg_u_shared,
    pop_u64,
    shared_atomic_cmpxchg_u16,
    push_u64,
    u16,
    u64
);
atomic_cmpxchg_op_shared!(
    op_i64_atomic_rmw32_cmpxchg_u_shared,
    pop_u64,
    shared_atomic_cmpxchg_u32,
    push_u64,
    u32,
    u64
);

atomic_load_op_indexed!(
    op_i32_atomic_load_indexed_local,
    op_i32_atomic_load_indexed_shared,
    indexed_local_atomic_load_u32,
    indexed_shared_atomic_load_u32,
    push_u32,
    u32
);
atomic_load_op_indexed!(
    op_i64_atomic_load_indexed_local,
    op_i64_atomic_load_indexed_shared,
    indexed_local_atomic_load_u64,
    indexed_shared_atomic_load_u64,
    push_u64,
    u64
);
atomic_load_op_indexed!(
    op_i32_atomic_load8_u_indexed_local,
    op_i32_atomic_load8_u_indexed_shared,
    indexed_local_atomic_load_u8,
    indexed_shared_atomic_load_u8,
    push_u32,
    u32
);
atomic_load_op_indexed!(
    op_i32_atomic_load16_u_indexed_local,
    op_i32_atomic_load16_u_indexed_shared,
    indexed_local_atomic_load_u16,
    indexed_shared_atomic_load_u16,
    push_u32,
    u32
);
atomic_load_op_indexed!(
    op_i64_atomic_load8_u_indexed_local,
    op_i64_atomic_load8_u_indexed_shared,
    indexed_local_atomic_load_u8,
    indexed_shared_atomic_load_u8,
    push_u64,
    u64
);
atomic_load_op_indexed!(
    op_i64_atomic_load16_u_indexed_local,
    op_i64_atomic_load16_u_indexed_shared,
    indexed_local_atomic_load_u16,
    indexed_shared_atomic_load_u16,
    push_u64,
    u64
);
atomic_load_op_indexed!(
    op_i64_atomic_load32_u_indexed_local,
    op_i64_atomic_load32_u_indexed_shared,
    indexed_local_atomic_load_u32,
    indexed_shared_atomic_load_u32,
    push_u64,
    u64
);

atomic_store_op_indexed!(
    op_i32_atomic_store_indexed_local,
    op_i32_atomic_store_indexed_shared,
    pop_u32,
    indexed_local_atomic_store_u32,
    indexed_shared_atomic_store_u32,
    u32
);
atomic_store_op_indexed!(
    op_i64_atomic_store_indexed_local,
    op_i64_atomic_store_indexed_shared,
    pop_u64,
    indexed_local_atomic_store_u64,
    indexed_shared_atomic_store_u64,
    u64
);
atomic_store_op_indexed!(
    op_i32_atomic_store8_indexed_local,
    op_i32_atomic_store8_indexed_shared,
    pop_u32,
    indexed_local_atomic_store_u8,
    indexed_shared_atomic_store_u8,
    u8
);
atomic_store_op_indexed!(
    op_i32_atomic_store16_indexed_local,
    op_i32_atomic_store16_indexed_shared,
    pop_u32,
    indexed_local_atomic_store_u16,
    indexed_shared_atomic_store_u16,
    u16
);
atomic_store_op_indexed!(
    op_i64_atomic_store8_indexed_local,
    op_i64_atomic_store8_indexed_shared,
    pop_u64,
    indexed_local_atomic_store_u8,
    indexed_shared_atomic_store_u8,
    u8
);
atomic_store_op_indexed!(
    op_i64_atomic_store16_indexed_local,
    op_i64_atomic_store16_indexed_shared,
    pop_u64,
    indexed_local_atomic_store_u16,
    indexed_shared_atomic_store_u16,
    u16
);
atomic_store_op_indexed!(
    op_i64_atomic_store32_indexed_local,
    op_i64_atomic_store32_indexed_shared,
    pop_u64,
    indexed_local_atomic_store_u32,
    indexed_shared_atomic_store_u32,
    u32
);

atomic_rmw_op_indexed!(
    op_i32_atomic_rmw_add_indexed_local,
    op_i32_atomic_rmw_add_indexed_shared,
    pop_u32,
    indexed_local_atomic_rmw_u32,
    indexed_shared_atomic_rmw_u32,
    push_u32,
    u32,
    u32,
    AtomicRmwOp::Add
);
atomic_rmw_op_indexed!(
    op_i64_atomic_rmw_add_indexed_local,
    op_i64_atomic_rmw_add_indexed_shared,
    pop_u64,
    indexed_local_atomic_rmw_u64,
    indexed_shared_atomic_rmw_u64,
    push_u64,
    u64,
    u64,
    AtomicRmwOp::Add
);
atomic_rmw_op_indexed!(
    op_i32_atomic_rmw8_add_u_indexed_local,
    op_i32_atomic_rmw8_add_u_indexed_shared,
    pop_u32,
    indexed_local_atomic_rmw_u8,
    indexed_shared_atomic_rmw_u8,
    push_u32,
    u8,
    u32,
    AtomicRmwOp::Add
);
atomic_rmw_op_indexed!(
    op_i32_atomic_rmw16_add_u_indexed_local,
    op_i32_atomic_rmw16_add_u_indexed_shared,
    pop_u32,
    indexed_local_atomic_rmw_u16,
    indexed_shared_atomic_rmw_u16,
    push_u32,
    u16,
    u32,
    AtomicRmwOp::Add
);
atomic_rmw_op_indexed!(
    op_i64_atomic_rmw8_add_u_indexed_local,
    op_i64_atomic_rmw8_add_u_indexed_shared,
    pop_u64,
    indexed_local_atomic_rmw_u8,
    indexed_shared_atomic_rmw_u8,
    push_u64,
    u8,
    u64,
    AtomicRmwOp::Add
);
atomic_rmw_op_indexed!(
    op_i64_atomic_rmw16_add_u_indexed_local,
    op_i64_atomic_rmw16_add_u_indexed_shared,
    pop_u64,
    indexed_local_atomic_rmw_u16,
    indexed_shared_atomic_rmw_u16,
    push_u64,
    u16,
    u64,
    AtomicRmwOp::Add
);
atomic_rmw_op_indexed!(
    op_i64_atomic_rmw32_add_u_indexed_local,
    op_i64_atomic_rmw32_add_u_indexed_shared,
    pop_u64,
    indexed_local_atomic_rmw_u32,
    indexed_shared_atomic_rmw_u32,
    push_u64,
    u32,
    u64,
    AtomicRmwOp::Add
);
atomic_rmw_op_indexed!(
    op_i32_atomic_rmw_sub_indexed_local,
    op_i32_atomic_rmw_sub_indexed_shared,
    pop_u32,
    indexed_local_atomic_rmw_u32,
    indexed_shared_atomic_rmw_u32,
    push_u32,
    u32,
    u32,
    AtomicRmwOp::Sub
);
atomic_rmw_op_indexed!(
    op_i64_atomic_rmw_sub_indexed_local,
    op_i64_atomic_rmw_sub_indexed_shared,
    pop_u64,
    indexed_local_atomic_rmw_u64,
    indexed_shared_atomic_rmw_u64,
    push_u64,
    u64,
    u64,
    AtomicRmwOp::Sub
);
atomic_rmw_op_indexed!(
    op_i32_atomic_rmw8_sub_u_indexed_local,
    op_i32_atomic_rmw8_sub_u_indexed_shared,
    pop_u32,
    indexed_local_atomic_rmw_u8,
    indexed_shared_atomic_rmw_u8,
    push_u32,
    u8,
    u32,
    AtomicRmwOp::Sub
);
atomic_rmw_op_indexed!(
    op_i32_atomic_rmw16_sub_u_indexed_local,
    op_i32_atomic_rmw16_sub_u_indexed_shared,
    pop_u32,
    indexed_local_atomic_rmw_u16,
    indexed_shared_atomic_rmw_u16,
    push_u32,
    u16,
    u32,
    AtomicRmwOp::Sub
);
atomic_rmw_op_indexed!(
    op_i64_atomic_rmw8_sub_u_indexed_local,
    op_i64_atomic_rmw8_sub_u_indexed_shared,
    pop_u64,
    indexed_local_atomic_rmw_u8,
    indexed_shared_atomic_rmw_u8,
    push_u64,
    u8,
    u64,
    AtomicRmwOp::Sub
);
atomic_rmw_op_indexed!(
    op_i64_atomic_rmw16_sub_u_indexed_local,
    op_i64_atomic_rmw16_sub_u_indexed_shared,
    pop_u64,
    indexed_local_atomic_rmw_u16,
    indexed_shared_atomic_rmw_u16,
    push_u64,
    u16,
    u64,
    AtomicRmwOp::Sub
);
atomic_rmw_op_indexed!(
    op_i64_atomic_rmw32_sub_u_indexed_local,
    op_i64_atomic_rmw32_sub_u_indexed_shared,
    pop_u64,
    indexed_local_atomic_rmw_u32,
    indexed_shared_atomic_rmw_u32,
    push_u64,
    u32,
    u64,
    AtomicRmwOp::Sub
);
atomic_rmw_op_indexed!(
    op_i32_atomic_rmw_and_indexed_local,
    op_i32_atomic_rmw_and_indexed_shared,
    pop_u32,
    indexed_local_atomic_rmw_u32,
    indexed_shared_atomic_rmw_u32,
    push_u32,
    u32,
    u32,
    AtomicRmwOp::And
);
atomic_rmw_op_indexed!(
    op_i64_atomic_rmw_and_indexed_local,
    op_i64_atomic_rmw_and_indexed_shared,
    pop_u64,
    indexed_local_atomic_rmw_u64,
    indexed_shared_atomic_rmw_u64,
    push_u64,
    u64,
    u64,
    AtomicRmwOp::And
);
atomic_rmw_op_indexed!(
    op_i32_atomic_rmw8_and_u_indexed_local,
    op_i32_atomic_rmw8_and_u_indexed_shared,
    pop_u32,
    indexed_local_atomic_rmw_u8,
    indexed_shared_atomic_rmw_u8,
    push_u32,
    u8,
    u32,
    AtomicRmwOp::And
);
atomic_rmw_op_indexed!(
    op_i32_atomic_rmw16_and_u_indexed_local,
    op_i32_atomic_rmw16_and_u_indexed_shared,
    pop_u32,
    indexed_local_atomic_rmw_u16,
    indexed_shared_atomic_rmw_u16,
    push_u32,
    u16,
    u32,
    AtomicRmwOp::And
);
atomic_rmw_op_indexed!(
    op_i64_atomic_rmw8_and_u_indexed_local,
    op_i64_atomic_rmw8_and_u_indexed_shared,
    pop_u64,
    indexed_local_atomic_rmw_u8,
    indexed_shared_atomic_rmw_u8,
    push_u64,
    u8,
    u64,
    AtomicRmwOp::And
);
atomic_rmw_op_indexed!(
    op_i64_atomic_rmw16_and_u_indexed_local,
    op_i64_atomic_rmw16_and_u_indexed_shared,
    pop_u64,
    indexed_local_atomic_rmw_u16,
    indexed_shared_atomic_rmw_u16,
    push_u64,
    u16,
    u64,
    AtomicRmwOp::And
);
atomic_rmw_op_indexed!(
    op_i64_atomic_rmw32_and_u_indexed_local,
    op_i64_atomic_rmw32_and_u_indexed_shared,
    pop_u64,
    indexed_local_atomic_rmw_u32,
    indexed_shared_atomic_rmw_u32,
    push_u64,
    u32,
    u64,
    AtomicRmwOp::And
);
atomic_rmw_op_indexed!(
    op_i32_atomic_rmw_or_indexed_local,
    op_i32_atomic_rmw_or_indexed_shared,
    pop_u32,
    indexed_local_atomic_rmw_u32,
    indexed_shared_atomic_rmw_u32,
    push_u32,
    u32,
    u32,
    AtomicRmwOp::Or
);
atomic_rmw_op_indexed!(
    op_i64_atomic_rmw_or_indexed_local,
    op_i64_atomic_rmw_or_indexed_shared,
    pop_u64,
    indexed_local_atomic_rmw_u64,
    indexed_shared_atomic_rmw_u64,
    push_u64,
    u64,
    u64,
    AtomicRmwOp::Or
);
atomic_rmw_op_indexed!(
    op_i32_atomic_rmw8_or_u_indexed_local,
    op_i32_atomic_rmw8_or_u_indexed_shared,
    pop_u32,
    indexed_local_atomic_rmw_u8,
    indexed_shared_atomic_rmw_u8,
    push_u32,
    u8,
    u32,
    AtomicRmwOp::Or
);
atomic_rmw_op_indexed!(
    op_i32_atomic_rmw16_or_u_indexed_local,
    op_i32_atomic_rmw16_or_u_indexed_shared,
    pop_u32,
    indexed_local_atomic_rmw_u16,
    indexed_shared_atomic_rmw_u16,
    push_u32,
    u16,
    u32,
    AtomicRmwOp::Or
);
atomic_rmw_op_indexed!(
    op_i64_atomic_rmw8_or_u_indexed_local,
    op_i64_atomic_rmw8_or_u_indexed_shared,
    pop_u64,
    indexed_local_atomic_rmw_u8,
    indexed_shared_atomic_rmw_u8,
    push_u64,
    u8,
    u64,
    AtomicRmwOp::Or
);
atomic_rmw_op_indexed!(
    op_i64_atomic_rmw16_or_u_indexed_local,
    op_i64_atomic_rmw16_or_u_indexed_shared,
    pop_u64,
    indexed_local_atomic_rmw_u16,
    indexed_shared_atomic_rmw_u16,
    push_u64,
    u16,
    u64,
    AtomicRmwOp::Or
);
atomic_rmw_op_indexed!(
    op_i64_atomic_rmw32_or_u_indexed_local,
    op_i64_atomic_rmw32_or_u_indexed_shared,
    pop_u64,
    indexed_local_atomic_rmw_u32,
    indexed_shared_atomic_rmw_u32,
    push_u64,
    u32,
    u64,
    AtomicRmwOp::Or
);
atomic_rmw_op_indexed!(
    op_i32_atomic_rmw_xor_indexed_local,
    op_i32_atomic_rmw_xor_indexed_shared,
    pop_u32,
    indexed_local_atomic_rmw_u32,
    indexed_shared_atomic_rmw_u32,
    push_u32,
    u32,
    u32,
    AtomicRmwOp::Xor
);
atomic_rmw_op_indexed!(
    op_i64_atomic_rmw_xor_indexed_local,
    op_i64_atomic_rmw_xor_indexed_shared,
    pop_u64,
    indexed_local_atomic_rmw_u64,
    indexed_shared_atomic_rmw_u64,
    push_u64,
    u64,
    u64,
    AtomicRmwOp::Xor
);
atomic_rmw_op_indexed!(
    op_i32_atomic_rmw8_xor_u_indexed_local,
    op_i32_atomic_rmw8_xor_u_indexed_shared,
    pop_u32,
    indexed_local_atomic_rmw_u8,
    indexed_shared_atomic_rmw_u8,
    push_u32,
    u8,
    u32,
    AtomicRmwOp::Xor
);
atomic_rmw_op_indexed!(
    op_i32_atomic_rmw16_xor_u_indexed_local,
    op_i32_atomic_rmw16_xor_u_indexed_shared,
    pop_u32,
    indexed_local_atomic_rmw_u16,
    indexed_shared_atomic_rmw_u16,
    push_u32,
    u16,
    u32,
    AtomicRmwOp::Xor
);
atomic_rmw_op_indexed!(
    op_i64_atomic_rmw8_xor_u_indexed_local,
    op_i64_atomic_rmw8_xor_u_indexed_shared,
    pop_u64,
    indexed_local_atomic_rmw_u8,
    indexed_shared_atomic_rmw_u8,
    push_u64,
    u8,
    u64,
    AtomicRmwOp::Xor
);
atomic_rmw_op_indexed!(
    op_i64_atomic_rmw16_xor_u_indexed_local,
    op_i64_atomic_rmw16_xor_u_indexed_shared,
    pop_u64,
    indexed_local_atomic_rmw_u16,
    indexed_shared_atomic_rmw_u16,
    push_u64,
    u16,
    u64,
    AtomicRmwOp::Xor
);
atomic_rmw_op_indexed!(
    op_i64_atomic_rmw32_xor_u_indexed_local,
    op_i64_atomic_rmw32_xor_u_indexed_shared,
    pop_u64,
    indexed_local_atomic_rmw_u32,
    indexed_shared_atomic_rmw_u32,
    push_u64,
    u32,
    u64,
    AtomicRmwOp::Xor
);
atomic_rmw_op_indexed!(
    op_i32_atomic_rmw_xchg_indexed_local,
    op_i32_atomic_rmw_xchg_indexed_shared,
    pop_u32,
    indexed_local_atomic_rmw_u32,
    indexed_shared_atomic_rmw_u32,
    push_u32,
    u32,
    u32,
    AtomicRmwOp::Xchg
);
atomic_rmw_op_indexed!(
    op_i64_atomic_rmw_xchg_indexed_local,
    op_i64_atomic_rmw_xchg_indexed_shared,
    pop_u64,
    indexed_local_atomic_rmw_u64,
    indexed_shared_atomic_rmw_u64,
    push_u64,
    u64,
    u64,
    AtomicRmwOp::Xchg
);
atomic_rmw_op_indexed!(
    op_i32_atomic_rmw8_xchg_u_indexed_local,
    op_i32_atomic_rmw8_xchg_u_indexed_shared,
    pop_u32,
    indexed_local_atomic_rmw_u8,
    indexed_shared_atomic_rmw_u8,
    push_u32,
    u8,
    u32,
    AtomicRmwOp::Xchg
);
atomic_rmw_op_indexed!(
    op_i32_atomic_rmw16_xchg_u_indexed_local,
    op_i32_atomic_rmw16_xchg_u_indexed_shared,
    pop_u32,
    indexed_local_atomic_rmw_u16,
    indexed_shared_atomic_rmw_u16,
    push_u32,
    u16,
    u32,
    AtomicRmwOp::Xchg
);
atomic_rmw_op_indexed!(
    op_i64_atomic_rmw8_xchg_u_indexed_local,
    op_i64_atomic_rmw8_xchg_u_indexed_shared,
    pop_u64,
    indexed_local_atomic_rmw_u8,
    indexed_shared_atomic_rmw_u8,
    push_u64,
    u8,
    u64,
    AtomicRmwOp::Xchg
);
atomic_rmw_op_indexed!(
    op_i64_atomic_rmw16_xchg_u_indexed_local,
    op_i64_atomic_rmw16_xchg_u_indexed_shared,
    pop_u64,
    indexed_local_atomic_rmw_u16,
    indexed_shared_atomic_rmw_u16,
    push_u64,
    u16,
    u64,
    AtomicRmwOp::Xchg
);
atomic_rmw_op_indexed!(
    op_i64_atomic_rmw32_xchg_u_indexed_local,
    op_i64_atomic_rmw32_xchg_u_indexed_shared,
    pop_u64,
    indexed_local_atomic_rmw_u32,
    indexed_shared_atomic_rmw_u32,
    push_u64,
    u32,
    u64,
    AtomicRmwOp::Xchg
);

atomic_cmpxchg_op_indexed!(
    op_i32_atomic_rmw_cmpxchg_indexed_local,
    op_i32_atomic_rmw_cmpxchg_indexed_shared,
    pop_u32,
    indexed_local_atomic_cmpxchg_u32,
    indexed_shared_atomic_cmpxchg_u32,
    push_u32,
    u32,
    u32
);
atomic_cmpxchg_op_indexed!(
    op_i64_atomic_rmw_cmpxchg_indexed_local,
    op_i64_atomic_rmw_cmpxchg_indexed_shared,
    pop_u64,
    indexed_local_atomic_cmpxchg_u64,
    indexed_shared_atomic_cmpxchg_u64,
    push_u64,
    u64,
    u64
);
atomic_cmpxchg_op_indexed!(
    op_i32_atomic_rmw8_cmpxchg_u_indexed_local,
    op_i32_atomic_rmw8_cmpxchg_u_indexed_shared,
    pop_u32,
    indexed_local_atomic_cmpxchg_u8,
    indexed_shared_atomic_cmpxchg_u8,
    push_u32,
    u8,
    u32
);
atomic_cmpxchg_op_indexed!(
    op_i32_atomic_rmw16_cmpxchg_u_indexed_local,
    op_i32_atomic_rmw16_cmpxchg_u_indexed_shared,
    pop_u32,
    indexed_local_atomic_cmpxchg_u16,
    indexed_shared_atomic_cmpxchg_u16,
    push_u32,
    u16,
    u32
);
atomic_cmpxchg_op_indexed!(
    op_i64_atomic_rmw8_cmpxchg_u_indexed_local,
    op_i64_atomic_rmw8_cmpxchg_u_indexed_shared,
    pop_u64,
    indexed_local_atomic_cmpxchg_u8,
    indexed_shared_atomic_cmpxchg_u8,
    push_u64,
    u8,
    u64
);
atomic_cmpxchg_op_indexed!(
    op_i64_atomic_rmw16_cmpxchg_u_indexed_local,
    op_i64_atomic_rmw16_cmpxchg_u_indexed_shared,
    pop_u64,
    indexed_local_atomic_cmpxchg_u16,
    indexed_shared_atomic_cmpxchg_u16,
    push_u64,
    u16,
    u64
);
atomic_cmpxchg_op_indexed!(
    op_i64_atomic_rmw32_cmpxchg_u_indexed_local,
    op_i64_atomic_rmw32_cmpxchg_u_indexed_shared,
    pop_u64,
    indexed_local_atomic_cmpxchg_u32,
    indexed_shared_atomic_cmpxchg_u32,
    push_u64,
    u32,
    u64
);

/// WebAssembly `memory.atomic.notify`.
///
/// Related spec:
/// - Threads: https://webassembly.github.io/threads/core/
///
/// Stack effect: `[i32, i32] -> [i32]`.
/// Traps: traps on out-of-bounds access or unaligned access.
/// Notes: Uses the threads memory model and preserves the runtime wait/notify contract before tail-dispatching.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
/// - Callers must preserve the shared-memory linearization contract by dropping temporary guards before the tail-dispatch completes.
pub unsafe fn op_memory_atomic_notify(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let mut facade = ExecuteContextFacade::new(ctx);
    let (_start, _count) = vm_try!(pop_notify_operands(tail_code, &mut facade));
    push_u32_and_continue(tail_code, &mut facade, 1, 0)
}

/// WebAssembly `memory.atomic.notify` on shared default memory.
///
/// Related spec:
/// - Threads: https://webassembly.github.io/threads/core/
///
/// Stack effect: `[i32, i32] -> [i32]`.
/// Traps: traps on out-of-bounds access or unaligned access.
/// Notes: Uses the threads memory model and preserves the runtime wait/notify contract before tail-dispatching.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose default memory is shared.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_memory_atomic_notify_shared(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let mut facade = ExecuteContextFacade::new(ctx);
    let (start, count) = vm_try!(pop_notify_operands(tail_code, &mut facade));
    let shared_id = unsafe { facade.default_shared_memory_id_unchecked() };
    let notify = vm_try!(facade.with_shared_memory_ref(shared_id, |shared| {
        shared.notify_waiters_protocol(start, count)
    }));
    push_u32_and_continue(tail_code, &mut facade, 1, notify.woken)
}

/// WebAssembly `memory.atomic.notify` on unshared indexed memory.
///
/// Related spec:
/// - Threads: https://webassembly.github.io/threads/core/
///
/// Stack effect: `[i32, i32] -> [i32]`.
/// Traps: traps on out-of-bounds access or unaligned access.
/// Notes: Validates the indexed memory access and returns `0` for unshared memory.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose indexed memory slot is local.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_memory_atomic_notify_indexed_unshared(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let mut facade = ExecuteContextFacade::new(ctx);
    let (_start, _memidx, _count) = vm_try!(pop_notify_operands_indexed(tail_code, &mut facade));
    push_u32_and_continue(tail_code, &mut facade, 2, 0)
}

/// WebAssembly `memory.atomic.notify` on shared indexed memory.
///
/// Related spec:
/// - Threads: https://webassembly.github.io/threads/core/
///
/// Stack effect: `[i32, i32] -> [i32]`.
/// Traps: traps on out-of-bounds access or unaligned access.
/// Notes: Uses the indexed shared-memory specialized fast path and returns the number of waiters woken on the selected memory.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose indexed memory slot is shared.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_memory_atomic_notify_indexed_shared(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let mut facade = ExecuteContextFacade::new(ctx);
    let (start, memidx, count) = vm_try!(pop_notify_operands_indexed(tail_code, &mut facade));
    let shared_id = unsafe { facade.shared_memory_id_at_unchecked(memidx) };
    let notify = vm_try!(facade.with_shared_memory_ref(shared_id, |shared| {
        shared.notify_waiters_protocol(start, count)
    }));
    push_u32_and_continue(tail_code, &mut facade, 2, notify.woken)
}

/// WebAssembly threads `memory.atomic.wait` completion helper.
///
/// Spec:
/// - Threads: https://webassembly.github.io/threads/core/
///
/// Stack effect: internal suspend point for wait operations.
/// Traps: propagates the trap behavior of the underlying wait operation.
/// Notes: Registers a `MemoryWait` pending op so the execution driver can resume the task later.
///
/// # Safety
/// - `ctx` must reference a live execution context whose pending-op emitter is available.
/// - `shared` and `wait` must refer to a wait registration belonging to the active store and memory instance.
/// - This helper must not keep locks or borrows alive while constructing the async completion.
unsafe fn enqueue_wait_pending(
    facade: &mut ExecuteContextFacade<'_, '_>,
    shared: std::sync::Arc<crate::common::SharedMemoryObject>,
    wait: crate::common::SharedWaitRegistration,
    timeout_ns: i64,
    resume_pc: *const Instr,
) {
    let task_id = facade.task_id();
    let fp = facade.stable_pc_from_raw_in_frame(resume_pc);
    facade
        .pending_mut()
        .push_pending(crate::runtime::memory_effect::PendingOp::MemoryWait(
            crate::runtime::memory_effect::MemoryWaitPending {
                task_id,
                shared,
                wait,
                timeout_ns,
                fp,
            },
        ));
}

/// WebAssembly `memory.atomic.wait32`.
///
/// Related spec:
/// - Threads: https://webassembly.github.io/threads/core/
///
/// Stack effect: `[i32, i32, i64] -> [i32]`.
/// Traps: traps on out-of-bounds access, unaligned access, or when used with unshared memory.
/// Notes: Uses the threads memory model and preserves the runtime wait/notify contract before tail-dispatching.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
/// - Callers must preserve the shared-memory linearization contract by dropping temporary guards before the tail-dispatch completes.
pub unsafe fn op_memory_atomic_wait32(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let mut facade = ExecuteContextFacade::new(ctx);
    let (_start, _expected, _timeout_ns) = vm_try!(pop_wait32_operands(tail_code, &mut facade));
    VMResult::InvalidOperand
}

/// WebAssembly `memory.atomic.wait32` on shared default memory.
///
/// Related spec:
/// - Threads: https://webassembly.github.io/threads/core/
///
/// Stack effect: `[i32, i32, i64] -> [i32]`.
/// Traps: traps on out-of-bounds access or unaligned access.
/// Notes: Uses the threads memory model and preserves the runtime wait/notify contract before tail-dispatching.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose default memory is shared.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_memory_atomic_wait32_shared(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let mut facade = ExecuteContextFacade::new(ctx);
    let (start, expected, timeout_ns) = vm_try!(pop_wait32_operands(tail_code, &mut facade));
    let shared_id = unsafe { facade.default_shared_memory_id_unchecked() };
    let registration = vm_try!(facade.with_shared_memory_ref(shared_id, |shared| {
        shared.register_wait32_protocol(start, expected)
    }));
    match registration {
        None => finish_wait_not_equal(tail_code, &mut facade, 1),
        Some(protocol) => {
            let resume_pc = tail_code.offset(1);
            let shared = facade.clone_shared_memory(shared_id);
            enqueue_wait_pending(&mut facade, shared, protocol.wait, timeout_ns, resume_pc);
            facade.set_cont(resume_pc);
            VMResult::Success(())
        }
    }
}

/// WebAssembly `memory.atomic.wait32` on unshared indexed memory.
///
/// Related spec:
/// - Threads: https://webassembly.github.io/threads/core/
///
/// Stack effect: `[i32, i32, i64] -> [i32]`.
/// Traps: traps on out-of-bounds access, unaligned access, or when used with unshared memory.
/// Notes: Validates the indexed memory access and fail-closes for unshared memory.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose indexed memory slot is local.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_memory_atomic_wait32_indexed_unshared(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let mut facade = ExecuteContextFacade::new(ctx);
    let (_start, _memidx, _expected, _timeout_ns) =
        vm_try!(pop_wait32_operands_indexed(tail_code, &mut facade));
    VMResult::InvalidOperand
}

/// WebAssembly `memory.atomic.wait32` on shared indexed memory.
///
/// Related spec:
/// - Threads: https://webassembly.github.io/threads/core/
///
/// Stack effect: `[i32, i32, i64] -> [i32]`.
/// Traps: traps on out-of-bounds access or unaligned access.
/// Notes: Uses the indexed shared-memory specialized fast path and preserves the runtime wait/notify contract before tail-dispatching.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose indexed memory slot is shared.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_memory_atomic_wait32_indexed_shared(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let mut facade = ExecuteContextFacade::new(ctx);
    let (start, memidx, expected, timeout_ns) =
        vm_try!(pop_wait32_operands_indexed(tail_code, &mut facade));
    let shared_id = unsafe { facade.shared_memory_id_at_unchecked(memidx) };
    let registration = vm_try!(facade.with_shared_memory_ref(shared_id, |shared| {
        shared.register_wait32_protocol(start, expected)
    }));
    match registration {
        None => finish_wait_not_equal(tail_code, &mut facade, 2),
        Some(protocol) => {
            let resume_pc = tail_code.offset(2);
            let shared = facade.clone_shared_memory(shared_id);
            enqueue_wait_pending(&mut facade, shared, protocol.wait, timeout_ns, resume_pc);
            facade.set_cont(resume_pc);
            VMResult::Success(())
        }
    }
}

/// WebAssembly `memory.atomic.wait64`.
///
/// Related spec:
/// - Threads: https://webassembly.github.io/threads/core/
///
/// Stack effect: `[i32, i64, i64] -> [i32]`.
/// Traps: traps on out-of-bounds access, unaligned access, or when used with unshared memory.
/// Notes: Uses the threads memory model and preserves the runtime wait/notify contract before tail-dispatching.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
/// - Callers must preserve the shared-memory linearization contract by dropping temporary guards before the tail-dispatch completes.
pub unsafe fn op_memory_atomic_wait64(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let mut facade = ExecuteContextFacade::new(ctx);
    let (_start, _expected, _timeout_ns) = vm_try!(pop_wait64_operands(tail_code, &mut facade));
    VMResult::InvalidOperand
}

/// WebAssembly `memory.atomic.wait64` on shared default memory.
///
/// Related spec:
/// - Threads: https://webassembly.github.io/threads/core/
///
/// Stack effect: `[i32, i64, i64] -> [i32]`.
/// Traps: traps on out-of-bounds access or unaligned access.
/// Notes: Uses the threads memory model and preserves the runtime wait/notify contract before tail-dispatching.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose default memory is shared.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_memory_atomic_wait64_shared(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let mut facade = ExecuteContextFacade::new(ctx);
    let (start, expected, timeout_ns) = vm_try!(pop_wait64_operands(tail_code, &mut facade));
    let shared_id = unsafe { facade.default_shared_memory_id_unchecked() };
    let registration = vm_try!(facade.with_shared_memory_ref(shared_id, |shared| {
        shared.register_wait64_protocol(start, expected)
    }));
    match registration {
        None => finish_wait_not_equal(tail_code, &mut facade, 1),
        Some(protocol) => {
            let resume_pc = tail_code.offset(1);
            let shared = facade.clone_shared_memory(shared_id);
            enqueue_wait_pending(&mut facade, shared, protocol.wait, timeout_ns, resume_pc);
            facade.set_cont(resume_pc);
            VMResult::Success(())
        }
    }
}

/// WebAssembly `memory.atomic.wait64` on unshared indexed memory.
///
/// Related spec:
/// - Threads: https://webassembly.github.io/threads/core/
///
/// Stack effect: `[i32, i64, i64] -> [i32]`.
/// Traps: traps on out-of-bounds access, unaligned access, or when used with unshared memory.
/// Notes: Validates the indexed memory access and fail-closes for unshared memory.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose indexed memory slot is local.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_memory_atomic_wait64_indexed_unshared(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let mut facade = ExecuteContextFacade::new(ctx);
    let (_start, _memidx, _expected, _timeout_ns) =
        vm_try!(pop_wait64_operands_indexed(tail_code, &mut facade));
    VMResult::InvalidOperand
}

/// WebAssembly `memory.atomic.wait64` on shared indexed memory.
///
/// Related spec:
/// - Threads: https://webassembly.github.io/threads/core/
///
/// Stack effect: `[i32, i64, i64] -> [i32]`.
/// Traps: traps on out-of-bounds access or unaligned access.
/// Notes: Uses the indexed shared-memory specialized fast path and preserves the runtime wait/notify contract before tail-dispatching.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose indexed memory slot is shared.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_memory_atomic_wait64_indexed_shared(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let mut facade = ExecuteContextFacade::new(ctx);
    let (start, memidx, expected, timeout_ns) =
        vm_try!(pop_wait64_operands_indexed(tail_code, &mut facade));
    let shared_id = unsafe { facade.shared_memory_id_at_unchecked(memidx) };
    let registration = vm_try!(facade.with_shared_memory_ref(shared_id, |shared| {
        shared.register_wait64_protocol(start, expected)
    }));
    match registration {
        None => finish_wait_not_equal(tail_code, &mut facade, 2),
        Some(protocol) => {
            let resume_pc = tail_code.offset(2);
            let shared = facade.clone_shared_memory(shared_id);
            enqueue_wait_pending(&mut facade, shared, protocol.wait, timeout_ns, resume_pc);
            facade.set_cont(resume_pc);
            VMResult::Success(())
        }
    }
}

/// WebAssembly `atomic.fence`.
///
/// Related spec:
/// - Threads: https://webassembly.github.io/threads/core/
///
/// Stack effect: `[] -> []`.
/// Traps: none.
/// Notes: Uses the threads memory model and preserves the runtime wait/notify contract before tail-dispatching.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
/// - Callers must preserve the shared-memory linearization contract by dropping temporary guards before the tail-dispatch completes.
pub unsafe fn op_atomic_fence(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let mut facade = ExecuteContextFacade::new(ctx);
    facade.local_atomic_fence();
    facade_call_next(tail_code, 1, &mut facade)
}

/// WebAssembly `atomic.fence` on shared default memory.
///
/// Related spec:
/// - Threads: https://webassembly.github.io/threads/core/
///
/// Stack effect: `[] -> []`.
/// Traps: none.
/// Notes: Uses the threads memory model and preserves the runtime wait/notify contract before tail-dispatching.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose default memory is shared.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_atomic_fence_shared(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let mut facade = ExecuteContextFacade::new(ctx);
    facade.shared_atomic_fence();
    facade_call_next(tail_code, 1, &mut facade)
}

pub(crate) use op_atomic_fence as op_atomic_fence_local;
pub(crate) use op_i32_atomic_load as op_i32_atomic_load_local;
pub(crate) use op_i32_atomic_load16_u as op_i32_atomic_load16_u_local;
pub(crate) use op_i32_atomic_load8_u as op_i32_atomic_load8_u_local;
pub(crate) use op_i32_atomic_store as op_i32_atomic_store_local;
pub(crate) use op_i32_atomic_store16 as op_i32_atomic_store16_local;
pub(crate) use op_i32_atomic_store8 as op_i32_atomic_store8_local;
pub(crate) use op_i64_atomic_load as op_i64_atomic_load_local;
pub(crate) use op_i64_atomic_load16_u as op_i64_atomic_load16_u_local;
pub(crate) use op_i64_atomic_load32_u as op_i64_atomic_load32_u_local;
pub(crate) use op_i64_atomic_load8_u as op_i64_atomic_load8_u_local;
pub(crate) use op_i64_atomic_store as op_i64_atomic_store_local;
pub(crate) use op_i64_atomic_store16 as op_i64_atomic_store16_local;
pub(crate) use op_i64_atomic_store32 as op_i64_atomic_store32_local;
pub(crate) use op_i64_atomic_store8 as op_i64_atomic_store8_local;
pub(crate) use op_memory_atomic_notify as op_memory_atomic_notify_unshared;
pub(crate) use op_memory_atomic_wait32 as op_memory_atomic_wait32_unshared;
pub(crate) use op_memory_atomic_wait64 as op_memory_atomic_wait64_unshared;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        common::{
            stack::{CachedMemoryKind, CallFrameCache},
            store::InstanceId,
            AtomicWaitResult, ExecuteContext, GcRef, LocalReference, MemoryHandle, Operand,
            SharedMemoryObject, Store, StoreInner,
        },
        runtime::{memory_effect::PendingOp, scheduler::PendingOpEmitter},
    };
    use futures::future::poll_fn;
    use std::collections::VecDeque;

    fn frame(kind: CachedMemoryKind, raw: u32) -> CallFrameCache {
        CallFrameCache {
            code_addr: GcRef(0),
            code_base: std::ptr::null(),
            instance: InstanceId::from_index(0),
            memory0_kind: kind,
            memory0_raw: raw,
        }
    }

    fn test_context<'a>(
        stack: &'a mut Stack,
        store: &'a Store,
        gc: &'a mut StoreInner,
        pending_effects: &'a mut u32,
        pending_ops: &'a mut VecDeque<PendingOp>,
    ) -> ExecuteContext<'a> {
        test_context_with_frame(
            stack,
            store,
            gc,
            pending_effects,
            pending_ops,
            frame(CachedMemoryKind::Local, 1),
        )
    }

    fn test_context_with_frame<'a>(
        stack: &'a mut Stack,
        store: &'a Store,
        gc: &'a mut StoreInner,
        pending_effects: &'a mut u32,
        pending_ops: &'a mut VecDeque<PendingOp>,
        frame: CallFrameCache,
    ) -> ExecuteContext<'a> {
        ExecuteContext::new(
            stack,
            LocalReference {
                local_top: 0,
                local_size: 0,
            },
            frame,
            store,
            gc,
            PendingOpEmitter::from_parts(9, pending_effects, pending_ops),
            std::ptr::null(),
            9,
        )
    }

    unsafe fn stop_op(_tail_code: *const Instr, _ctx: &mut ExecuteContext) -> VMResult<()> {
        VMResult::Success(())
    }

    #[test]
    fn atomic_start_helpers_match_offset_index_and_wait_codes() {
        let store = Store::new();
        let mut gc = StoreInner::new();
        let mut pending_effects = 0;
        let mut pending_ops = VecDeque::new();
        let mut stack = Stack::new(32);
        stack.push_u32(4).unwrap();

        let program = [
            Instr {
                operand: Operand {
                    memarg: MemArg {
                        align: 2,
                        offset: 6,
                    },
                },
            },
            Instr {
                operand: Operand { u32: 8 },
            },
        ];
        let mut ctx = test_context(
            &mut stack,
            &store,
            &mut gc,
            &mut pending_effects,
            &mut pending_ops,
        );

        let mut facade = ExecuteContextFacade::new(&mut ctx);
        let start = unsafe { atomic_start(program.as_ptr(), &mut facade) }.unwrap();
        assert_eq!(start, 10);

        facade.push_u32(1).unwrap();
        let (indexed_start, memidx) =
            unsafe { atomic_start_indexed(program.as_ptr(), &mut facade) }.unwrap();
        assert_eq!(indexed_start, 7);
        assert_eq!(memidx, 8);
        assert_eq!(wait_result_not_equal(), 1);
        assert_eq!(wait_result_ok(), 0);
        assert_eq!(wait_result_timed_out(), 2);
    }

    #[tokio::test]
    async fn enqueue_wait_pending_preserves_resume_pc_and_task_id() {
        let store = Store::new();
        let mut gc = StoreInner::new();
        let mut pending_effects = 0;
        let mut pending_ops = VecDeque::new();
        let mut stack = Stack::new(32);
        let shared = SharedMemoryObject::new(1, 1);
        shared.atomic_store_u32(0, 7).unwrap();
        let wait = match shared.register_wait32(0, 7).unwrap() {
            AtomicWaitResult::Pending(wait) => wait,
            AtomicWaitResult::NotEqual => panic!("expected waiting registration"),
        };
        let program = [
            Instr {
                operand: Operand { u32: 0 },
            },
            Instr { op: stop_op },
        ];
        let resume_pc = unsafe { program.as_ptr().add(1) };
        let local_reference = {
            let mut ctx = test_context(
                &mut stack,
                &store,
                &mut gc,
                &mut pending_effects,
                &mut pending_ops,
            );
            let mut facade = ExecuteContextFacade::new(&mut ctx);
            let local_reference = facade.local_reference();
            unsafe {
                enqueue_wait_pending(&mut facade, shared.clone(), wait, -1, resume_pc);
            }
            local_reference
        };

        assert_eq!(pending_effects, 1);
        assert_eq!(pending_ops.len(), 1);
        assert_eq!(shared.notify_waiters(0, 1).unwrap(), 1);

        let pending = pending_ops.pop_front().expect("memory wait must be queued");
        let PendingOp::MemoryWait(pending) = pending else {
            panic!("unexpected pending op");
        };
        assert_eq!(pending.task_id, 9);
        poll_fn(|cx| pending.wait.poll_wait(cx)).await;
        let value = pending.wait.finish_notified(&shared);
        assert_eq!(value, wait_result_ok());
        assert_eq!(pending.fp.resolve(&gc, &stack, local_reference), resume_pc);
    }

    #[test]
    fn unshared_wait_and_notify_helpers_fail_close() {
        let store = Store::new();
        let mut gc = StoreInner::new();
        let mut pending_effects = 0;
        let mut pending_ops = VecDeque::new();
        let mut stack = Stack::new(64);
        let mut ctx = test_context(
            &mut stack,
            &store,
            &mut gc,
            &mut pending_effects,
            &mut pending_ops,
        );

        {
            let mut facade = ExecuteContextFacade::new(&mut ctx);
            facade.push_u32(5).unwrap();
            facade.push_u32(2).unwrap();
        }
        let notify_program = [
            Instr {
                operand: Operand {
                    memarg: MemArg {
                        align: 2,
                        offset: 3,
                    },
                },
            },
            Instr { op: stop_op },
        ];
        unsafe {
            op_memory_atomic_notify(notify_program.as_ptr(), &mut ctx).unwrap();
        }
        {
            let mut facade = ExecuteContextFacade::new(&mut ctx);
            assert_eq!(facade.pop_u32(), 0);
        }

        {
            let mut facade = ExecuteContextFacade::new(&mut ctx);
            facade.push_u32(4).unwrap();
            facade.push_u32(7).unwrap();
            facade.push_i64(-1).unwrap();
        }
        let wait_program = [Instr {
            operand: Operand {
                memarg: MemArg {
                    align: 2,
                    offset: 1,
                },
            },
        }];
        let result = unsafe { op_memory_atomic_wait32(wait_program.as_ptr(), &mut ctx) };
        assert!(matches!(result, VMResult::InvalidOperand));
    }

    #[test]
    fn shared_wait_and_notify_helpers_route_through_protocol_wrappers() {
        let store = Store::new();
        let mut gc = StoreInner::new();
        let mut pending_effects = 0;
        let mut pending_ops = VecDeque::new();
        let mut stack = Stack::new(64);
        let shared_id = match gc.alloc_shared_memory(SharedMemoryObject::new(1, 1)) {
            MemoryHandle::Shared(id) => id,
            MemoryHandle::Local(_) => panic!("expected shared memory handle"),
        };
        let shared = gc.clone_shared_memory(shared_id);
        shared.atomic_store_u32(0, 7).unwrap();

        {
            let mut ctx = test_context_with_frame(
                &mut stack,
                &store,
                &mut gc,
                &mut pending_effects,
                &mut pending_ops,
                frame(CachedMemoryKind::Shared, shared_id.raw()),
            );
            let mut facade = ExecuteContextFacade::new(&mut ctx);
            facade.push_u32(0).unwrap();
            facade.push_u32(7).unwrap();
            facade.push_i64(-1).unwrap();
            let wait_program = [
                Instr {
                    operand: Operand {
                        memarg: MemArg {
                            align: 2,
                            offset: 0,
                        },
                    },
                },
                Instr { op: stop_op },
            ];
            let pending_before = ctx.pending_len();
            let result = unsafe { op_memory_atomic_wait32_shared(wait_program.as_ptr(), &mut ctx) };
            let outcome = crate::common::formal::core_outcome_from_vm_result_with_pending(
                &result,
                pending_before,
                ctx.pending_len(),
                ctx.pending_code_delta(pending_before).unwrap_or(None),
            )
            .unwrap();
            assert_eq!(
                outcome,
                crate::common::formal::CoreOutcome::Pending(
                    crate::common::formal::PendingCode::Wait,
                )
            );
            result.unwrap();
        }

        assert_eq!(pending_effects, 1);
        assert_eq!(pending_ops.len(), 1);
        let after_wait = shared.projection();
        assert_eq!(after_wait.wait_queues.len(), 1);
        assert_eq!(after_wait.wait_queues[0].waiter_ids, vec![1]);

        {
            let mut ctx = test_context_with_frame(
                &mut stack,
                &store,
                &mut gc,
                &mut pending_effects,
                &mut pending_ops,
                frame(CachedMemoryKind::Shared, shared_id.raw()),
            );
            let mut facade = ExecuteContextFacade::new(&mut ctx);
            facade.push_u32(0).unwrap();
            facade.push_u32(1).unwrap();
            let notify_program = [
                Instr {
                    operand: Operand {
                        memarg: MemArg {
                            align: 2,
                            offset: 0,
                        },
                    },
                },
                Instr { op: stop_op },
            ];
            let pending_before = ctx.pending_len();
            let result =
                unsafe { op_memory_atomic_notify_shared(notify_program.as_ptr(), &mut ctx) };
            let outcome = crate::common::formal::core_outcome_from_vm_result_with_pending(
                &result,
                pending_before,
                ctx.pending_len(),
                ctx.pending_code_delta(pending_before).unwrap_or(None),
            )
            .unwrap();
            assert_eq!(outcome, crate::common::formal::CoreOutcome::Continue);
            result.unwrap();
            let mut facade = ExecuteContextFacade::new(&mut ctx);
            assert_eq!(facade.pop_u32(), 1);
        }

        let after_notify = shared.projection();
        assert!(after_notify.wait_queues.is_empty());
        assert_ne!(after_notify.waiters[0].state, after_wait.waiters[0].state);
    }

    #[test]
    fn shared_wait64_not_equal_returns_immediate_code() {
        let store = Store::new();
        let mut gc = StoreInner::new();
        let mut pending_effects = 0;
        let mut pending_ops = VecDeque::new();
        let mut stack = Stack::new(64);
        let shared_id = match gc.alloc_shared_memory(SharedMemoryObject::new(1, 1)) {
            MemoryHandle::Shared(id) => id,
            MemoryHandle::Local(_) => panic!("expected shared memory handle"),
        };
        let shared = gc.clone_shared_memory(shared_id);
        shared.atomic_store_u64(8, 0x0102_0304_0506_0708).unwrap();

        let mut ctx = test_context_with_frame(
            &mut stack,
            &store,
            &mut gc,
            &mut pending_effects,
            &mut pending_ops,
            frame(CachedMemoryKind::Shared, shared_id.raw()),
        );
        {
            let mut facade = ExecuteContextFacade::new(&mut ctx);
            facade.push_u32(8).unwrap();
            facade.push_u64(0xffff_ffff_ffff_ffff).unwrap();
            facade.push_i64(0).unwrap();
        }
        let wait_program = [
            Instr {
                operand: Operand {
                    memarg: MemArg {
                        align: 3,
                        offset: 0,
                    },
                },
            },
            Instr { op: stop_op },
        ];
        unsafe {
            op_memory_atomic_wait64_shared(wait_program.as_ptr(), &mut ctx).unwrap();
        }
        let mut facade = ExecuteContextFacade::new(&mut ctx);
        assert_eq!(facade.pop_i32(), wait_result_not_equal());
        assert_eq!(pending_effects, 0);
        assert!(pending_ops.is_empty());
    }
}
