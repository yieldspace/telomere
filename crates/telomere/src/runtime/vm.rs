#[macro_use]
pub(crate) mod traps;
#[cfg(feature = "threads")]
mod atomics;
mod bulk_memory;
mod call;
mod control;
mod globals;
mod locals;
mod memory;
mod numeric;
mod refs;
#[cfg(feature = "simd")]
pub(crate) mod simd;
mod tables;

use crate::{
    common::{
        AsyncHostCallContext, CallFrameCache, ElemInit, ExecuteContext, ExportDesc, FuncType,
        GcRef, HostCallControl, HostTailCallTarget, InstanceHandle, Instr, LocalReference, MemArg,
        ResultType, ResultValue, StablePc, Stack, VMResult, ValType, WasmValue,
        TABLE_UNINITIALIZED,
    },
    runtime::{
        memory_effect::{HostCallPending, PendingOp},
        scheduler::{ExecutionDriver, ExecutionKernel, ReadyFlag, SyncRunError, Task, TokioDriver},
    },
    Store,
};
use vstd::prelude::*;

verus! {

#[allow(dead_code)]
pub(crate) enum MemorySelectorWitness {
    CurrentDefault,
    Explicit { shared: bool, raw: u32 },
}

#[allow(dead_code)]
pub(crate) open spec fn current_default_memory_selector_witness() -> MemorySelectorWitness {
    MemorySelectorWitness::CurrentDefault
}

#[allow(dead_code)]
pub(crate) open spec fn explicit_local_memory_selector_witness(raw: u32) -> MemorySelectorWitness {
    MemorySelectorWitness::Explicit { shared: false, raw }
}

#[allow(dead_code)]
pub(crate) open spec fn explicit_shared_memory_selector_witness(
    raw: u32,
) -> MemorySelectorWitness {
    MemorySelectorWitness::Explicit { shared: true, raw }
}

pub(crate) open spec fn memory_selector_from_witness(
    witness: MemorySelectorWitness,
) -> crate::common::formal::MemorySelector {
    match witness {
        MemorySelectorWitness::CurrentDefault => {
            crate::common::formal::MemorySelector::CurrentDefault
        }
        MemorySelectorWitness::Explicit { shared, raw } => {
            crate::common::formal::MemorySelector::Explicit(if shared {
                crate::common::formal::MemoryHandleView::Shared(raw as nat)
            } else {
                crate::common::formal::MemoryHandleView::Local(raw as nat)
            })
        }
    }
}

pub open spec fn spec_compute_memory_offset_result(memarg_offset: u32, offset: u32) -> Option<int> {
    if memarg_offset as int + offset as int <= u32::MAX as int {
        Some(memarg_offset as int + offset as int)
    } else {
        None
    }
}

#[inline(always)]
fn checked_compute_memory_offset(memarg_offset: u32, offset: u32) -> (result: Option<usize>)
    ensures
        match spec_compute_memory_offset_result(memarg_offset, offset) {
            Some(value) => result == Some(value as usize),
            None => result == Option::<usize>::None,
        },
{
    let sum = memarg_offset as u64 + offset as u64;
    if sum <= u32::MAX as u64 {
        Some(sum as usize)
    } else {
        None
    }
}

pub open spec fn spec_load_start_result(
    default_memory_present: bool,
    memarg_offset: u32,
    offset: u32,
) -> Option<int> {
    if default_memory_present {
        spec_compute_memory_offset_result(memarg_offset, offset)
    } else {
        None
    }
}

pub open spec fn spec_store_result(
    view: crate::common::formal::LinearMemoryView,
    start: int,
    payload: Seq<u8>,
) -> crate::common::formal::LinearMemoryView {
    crate::common::formal::linear_write_bytes(view, start, payload)
}

pub proof fn lemma_store_result_preserves_page_metadata(
    view: crate::common::formal::LinearMemoryView,
    start: int,
    payload: Seq<u8>,
)
    ensures
        spec_store_result(view, start, payload).current_pages == view.current_pages,
        spec_store_result(view, start, payload).max_pages == view.max_pages,
        spec_store_result(view, start, payload).shared == view.shared,
{
    crate::common::formal::lemma_linear_write_preserves_page_metadata(view, start, payload);
}

} // verus!

#[inline(always)]
fn wasm_shift_mask32(rhs: u32) -> u32 {
    rhs & 31
}

#[inline(always)]
fn wasm_shift_mask64(rhs: u32) -> u32 {
    rhs & 63
}

#[inline(always)]
fn wasm_i32_shl(lhs: i32, rhs: i32) -> i32 {
    lhs.wrapping_shl(wasm_shift_mask32(rhs as u32))
}

#[inline(always)]
fn wasm_i32_shr_s(lhs: i32, rhs: i32) -> i32 {
    lhs >> wasm_shift_mask32(rhs as u32)
}

#[inline(always)]
fn wasm_i32_shr_u(lhs: u32, rhs: u32) -> u32 {
    lhs.wrapping_shr(wasm_shift_mask32(rhs))
}

#[inline(always)]
fn wasm_i64_shl(lhs: i64, rhs: i64) -> i64 {
    lhs.wrapping_shl(wasm_shift_mask64(rhs as u32))
}

#[inline(always)]
fn wasm_i64_shr_s(lhs: i64, rhs: i64) -> i64 {
    lhs >> wasm_shift_mask64(rhs as u32)
}

#[inline(always)]
fn wasm_i64_shr_u(lhs: u64, rhs: u64) -> u64 {
    lhs.wrapping_shr(wasm_shift_mask64(rhs as u32))
}

pub(crate) enum StoreBytes {
    Write1([u8; 1]),
    Write2([u8; 2]),
    Write4([u8; 4]),
    Write8([u8; 8]),
    Write16([u8; 16]),
}

impl StoreBytes {
    #[inline(always)]
    fn as_slice(&self) -> &[u8] {
        match self {
            Self::Write1(bytes) => bytes,
            Self::Write2(bytes) => bytes,
            Self::Write4(bytes) => bytes,
            Self::Write8(bytes) => bytes,
            Self::Write16(bytes) => bytes,
        }
    }
}

#[inline(always)]
pub(crate) fn compute_memory_offset(memarg: MemArg, offset: u32) -> VMResult<usize> {
    VMResult::from_option(checked_compute_memory_offset(memarg.offset, offset), || {
        VMResult::MemoryIndexOutOfRange
    })
}

#[inline(always)]
/// Decode the single `memarg` immediate for the active instruction.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for the current handler.
unsafe fn decode_memarg_operand(tail_code: *const Instr) -> MemArg {
    (*tail_code).operand.memarg
}

#[inline(always)]
/// Decode the `memarg + memidx` immediates for the active indexed memory instruction.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for the current handler.
unsafe fn decode_indexed_memarg_operand(tail_code: *const Instr) -> (MemArg, u32) {
    ((*tail_code).operand.memarg, (*tail_code.add(1)).operand.u32)
}

#[inline(always)]
/// Telomere runtime helper `call_code`.
///
/// Related spec:
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: internal runtime continuation dispatch.
/// Traps: propagates the trap behavior of the target instruction.
/// Notes: Updates `ctx.cont` and performs the direct-threaded tail jump into the next instruction.
///
/// # Safety
/// - `tail_code` must reference the active decoded instruction stream for the current frame.
/// - `ctx` must reference a live execution context for the same store, frame, and validated locals/stack layout.
/// - Callers must not preserve borrows, locks, or guards across any tail-dispatch that this helper performs.
pub(crate) unsafe fn call_code(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    ctx.set_cont(tail_code);
    ((*tail_code).op)(tail_code.offset(1), ctx)
}

#[inline(always)]
/// Telomere runtime helper `call_next`.
///
/// Related spec:
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: internal runtime continuation dispatch.
/// Traps: propagates the trap behavior of the target instruction.
/// Notes: Advances from the current instruction by `consumed` operands and delegates to `call_code` without introducing non-tail cleanup.
///
/// # Safety
/// - `tail_code` must reference the active decoded instruction stream for the current frame.
/// - `ctx` must reference a live execution context for the same store, frame, and validated locals/stack layout.
/// - Callers must not preserve borrows, locks, or guards across any tail-dispatch that this helper performs.
pub(crate) unsafe fn call_next(
    tail_code: *const Instr,
    consumed: isize,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    call_code(tail_code.offset(consumed), ctx)
}

#[inline(always)]
/// Telomere runtime helper `facade_call_code`.
///
/// Related spec:
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: internal runtime continuation dispatch.
/// Traps: propagates the trap behavior of the target instruction.
/// Notes: Adapts `ExecuteContextFacade` callers to `call_code` without changing the continuation contract.
///
/// # Safety
/// - `tail_code` must reference the active decoded instruction stream for the current frame.
/// - `facade` must wrap a live execution context for the same store, frame, and validated locals/stack layout.
/// - Callers must not preserve borrows, locks, or guards across any tail-dispatch that this helper performs.
pub(crate) unsafe fn facade_call_code(
    tail_code: *const Instr,
    facade: &mut ExecuteContextFacade<'_, '_>,
) -> VMResult<()> {
    call_code(tail_code, facade.as_ctx_mut())
}

#[inline(always)]
/// Telomere runtime helper `facade_call_next`.
///
/// Related spec:
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: internal runtime continuation dispatch.
/// Traps: propagates the trap behavior of the target instruction.
/// Notes: Adapts `ExecuteContextFacade` callers to `call_next` while preserving the current operand decode offset.
///
/// # Safety
/// - `tail_code` must reference the active decoded instruction stream for the current frame.
/// - `facade` must wrap a live execution context for the same store, frame, and validated locals/stack layout.
/// - Callers must not preserve borrows, locks, or guards across any tail-dispatch that this helper performs.
pub(crate) unsafe fn facade_call_next(
    tail_code: *const Instr,
    consumed: isize,
    facade: &mut ExecuteContextFacade<'_, '_>,
) -> VMResult<()> {
    call_next(tail_code, consumed, facade.as_ctx_mut())
}

fn result_type_size(ty: &ResultType) -> usize {
    ty.iter().map(|value| value.stack_size().usize()).sum()
}

fn push_typed_value(stack: &mut Stack, ty: ValType, value: &WasmValue) -> VMResult<()> {
    match (ty, value) {
        (ValType::I32, WasmValue::I32(value)) => stack.push_i32(*value),
        (ValType::I64, WasmValue::I64(value)) => stack.push_i64(*value),
        (ValType::F32, WasmValue::F32(value)) => stack.push_f32(*value),
        (ValType::F64, WasmValue::F64(value)) => stack.push_f64(*value),
        (ValType::V128, WasmValue::V128(value)) => stack.push_u128(*value),
        (ValType::FuncRef, WasmValue::FuncRef(value)) => stack.push_u32(*value),
        (ValType::ExternRef, WasmValue::ExternRef(value)) => stack.push_u32(*value),
        _ => VMResult::InvalidOperand,
    }
}

fn push_result_values(stack: &mut Stack, types: &ResultType, values: &ResultValue) -> VMResult<()> {
    if types.0.len() != values.len() {
        return VMResult::InvalidOperand;
    }
    for (ty, value) in types.iter().zip(values.iter()) {
        vm_try!(push_typed_value(stack, *ty, value));
    }
    VMResult::Success(())
}

fn pop_result_values(stack: &mut Stack, ty: &ResultType) -> ResultValue {
    let mut result = ty
        .stack_pop_iter()
        .map(|t| match t {
            ValType::I32 => WasmValue::I32(stack.pop_i32()),
            ValType::I64 => WasmValue::I64(stack.pop_i64()),
            ValType::F32 => WasmValue::F32(stack.pop_f32()),
            ValType::F64 => WasmValue::F64(stack.pop_f64()),
            ValType::FuncRef => WasmValue::FuncRef(stack.pop_u32()),
            ValType::ExternRef => WasmValue::ExternRef(stack.pop_u32()),
            ValType::V128 => WasmValue::V128(stack.pop_u128()),
        })
        .collect::<Vec<_>>();
    result.reverse();
    ResultValue::new(result)
}

fn read_value(bytes: &[u8], ty: ValType) -> Option<WasmValue> {
    match ty {
        ValType::I32 => Some(WasmValue::I32(i32::from_le_bytes(bytes.try_into().ok()?))),
        ValType::I64 => Some(WasmValue::I64(i64::from_le_bytes(bytes.try_into().ok()?))),
        ValType::F32 => Some(WasmValue::F32(f32::from_bits(u32::from_le_bytes(
            bytes.try_into().ok()?,
        )))),
        ValType::F64 => Some(WasmValue::F64(f64::from_bits(u64::from_le_bytes(
            bytes.try_into().ok()?,
        )))),
        ValType::V128 => Some(WasmValue::V128(u128::from_le_bytes(bytes.try_into().ok()?))),
        ValType::FuncRef => Some(WasmValue::FuncRef(u32::from_le_bytes(
            bytes.try_into().ok()?,
        ))),
        ValType::ExternRef => Some(WasmValue::ExternRef(u32::from_le_bytes(
            bytes.try_into().ok()?,
        ))),
    }
}

fn marshal_local_values(
    stack: &Stack,
    local_reference: &LocalReference,
    types: &ResultType,
) -> VMResult<ResultValue> {
    let mut local_addr = 0usize;
    let mut values = Vec::with_capacity(types.0.len());
    for ty in types.iter() {
        let size = ty.stack_size().usize();
        let Some(value) = read_value(stack.local_bytes(local_reference, local_addr, size), *ty)
        else {
            return VMResult::InvalidOperand;
        };
        values.push(value);
        local_addr += size;
    }
    VMResult::Success(ResultValue::new(values))
}

unsafe fn current_function_type_ptr(facade: &ExecuteContextFacade<'_, '_>) -> *const FuncType {
    facade.current_function_type() as *const FuncType
}

unsafe fn function_type_ptr_by_addr(
    facade: &ExecuteContextFacade<'_, '_>,
    funcaddr: GcRef,
) -> *const FuncType {
    facade.function_type_by_addr(funcaddr) as *const FuncType
}

fn start_async_host_call(
    facade: &mut ExecuteContextFacade<'_, '_>,
    param_types: &ResultType,
    result_types: &ResultType,
) -> VMResult<()> {
    let async_host = facade.func().async_host_code_pointer();
    let params = vm_try!(marshal_local_values(
        facade.stack_ref(),
        &facade.local_reference(),
        param_types,
    ));
    let result_types = result_types.clone();
    let result_slot = facade.local_reference().local_top;
    let return_size = result_type_size(&result_types);
    let (resume_pc, _slot) = facade.function_return_in_place(return_size);
    let fp = facade.stable_pc_from_raw_in_frame(resume_pc);
    let future = async_host(AsyncHostCallContext {
        params,
        result_types: result_types.clone(),
        store_state: facade.store_ref().state,
    });
    let task_id = facade.task_id();
    facade
        .pending_mut()
        .push_pending(PendingOp::HostCall(HostCallPending {
            task_id,
            future,
            fp,
            result_types,
            result_slot,
        }));
    facade.set_cont(resume_pc);
    VMResult::Success(())
}

fn resolve_host_tail_call_target(
    facade: &ExecuteContextFacade<'_, '_>,
    target: HostTailCallTarget,
) -> VMResult<GcRef> {
    match target {
        HostTailCallTarget::FuncIdx(funcidx) => VMResult::from_option(
            facade
                .instance()
                .funcs
                .as_slice()
                .get(funcidx.0 as usize)
                .copied(),
            || VMResult::InvalidOperand,
        ),
        HostTailCallTarget::FuncRef(funcaddr) => {
            if funcaddr == GcRef(0) {
                VMResult::InvalidOperand
            } else {
                VMResult::Success(funcaddr)
            }
        }
    }
}

fn complete_sync_host_return(
    facade: &mut ExecuteContextFacade<'_, '_>,
    result_types: &ResultType,
    values: ResultValue,
) -> VMResult<()> {
    let return_size = result_type_size(result_types);
    let (return_addr, result_slot) = facade.function_return_in_place(return_size);
    vm_try!(facade.write_marshaled_results(result_slot, result_types, &values));
    unsafe { facade_call_code(return_addr, facade) }
}

fn complete_sync_host_tail_call(
    facade: &mut ExecuteContextFacade<'_, '_>,
    target: HostTailCallTarget,
    params: ResultValue,
) -> VMResult<()> {
    let funcaddr = vm_try!(resolve_host_tail_call_target(facade, target));
    let function_type = unsafe { &*function_type_ptr_by_addr(facade, funcaddr) };
    vm_try!(facade.push_result_values(&function_type.0, &params));
    let ptr = vm_try!(unsafe {
        call::internal_op_call(std::ptr::null(), funcaddr, facade.as_ctx_mut(), true)
    });
    if ptr.is_null() {
        VMResult::Success(())
    } else {
        unsafe { facade_call_next(ptr, 0, facade) }
    }
}

unsafe fn invoke_host_function(
    return_addr: *const Instr,
    facade: &mut ExecuteContextFacade<'_, '_>,
    param_types: *const ResultType,
    result_types: *const ResultType,
) -> VMResult<()> {
    let param_types = unsafe { &*param_types };
    let result_types = unsafe { &*result_types };
    if facade.func().is_async_host_func() {
        let _ = return_addr;
        vm_try!(start_async_host_call(facade, param_types, result_types));
        VMResult::Success(())
    } else {
        let fp = facade.func().host_code_pointer();
        let control = vm_try!(facade.run_sync_host_function(fp, param_types, result_types));
        match control {
            HostCallControl::Return(values) => {
                complete_sync_host_return(facade, result_types, values)
            }
            HostCallControl::TailCall { target, params } => {
                complete_sync_host_tail_call(facade, target, params)
            }
            HostCallControl::EndProgram => {
                facade.end_program();
                VMResult::Success(())
            }
        }
    }
}

pub(crate) use crate::common::ExecuteContextFacade;
#[cfg(feature = "threads")]
pub(crate) use atomics::*;
pub(crate) use bulk_memory::{
    op_data_drop, op_mem_copy_indexed_local_local, op_mem_copy_indexed_local_shared,
    op_mem_copy_indexed_shared_local, op_mem_copy_indexed_shared_shared, op_mem_copy_local,
    op_mem_copy_shared, op_mem_fill_indexed_local, op_mem_fill_indexed_shared, op_mem_fill_local,
    op_mem_fill_shared, op_mem_init_indexed_local, op_mem_init_indexed_shared, op_mem_init_local,
    op_mem_init_shared,
};
pub(crate) use call::{
    op_call, op_call_import, op_call_indirect, op_return_call, op_return_call_import,
    op_return_call_indirect, special_start_function_call,
};
pub use control::special_function_return;
pub(crate) use control::*;
pub(crate) use globals::*;
pub(crate) use locals::*;
pub(crate) use memory::*;
pub(crate) use numeric::*;
pub(crate) use refs::*;
pub(crate) use tables::*;

/// Telomere runtime helper `store_internal_local`.
///
/// Related spec:
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[i32, value] -> []`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Materializes the store payload before consuming the address so the write can tail-dispatch through `call_next`.
///
/// # Safety
/// - `tail_code` must reference the active decoded instruction stream for the current frame.
/// - `ctx` must reference a live execution context for the same store, frame, and validated locals/stack layout.
/// - Callers must not preserve borrows, locks, or guards across any tail-dispatch that this helper performs.
/// - `make_operation` must not retain references into `ctx` after it returns because the helper will continue by tail-dispatching immediately after the write.
pub(crate) unsafe fn store_internal_local(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    make_operation: impl FnOnce(&mut ExecuteContextFacade<'_, '_>) -> StoreBytes,
) -> VMResult<()> {
    let mut facade = ExecuteContextFacade::new(ctx);
    let memarg = decode_memarg_operand(tail_code);
    let operation = make_operation(&mut facade);
    let offset = facade.pop::<u32>();
    trace!("op_store: {:?} {}", memarg, offset);
    let bytes = operation.as_slice();
    let start = vm_try!(compute_memory_offset(memarg, offset));
    vm_try!(facade.write_memory_bytes(start, bytes));
    facade_call_next(tail_code, 1, &mut facade)
}

/// Telomere runtime helper `store_internal_shared`.
///
/// Related spec:
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[i32, value] -> []`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Materializes the store payload before consuming the address so the write can tail-dispatch through `call_next`.
///
/// # Safety
/// - `tail_code` must reference the active decoded instruction stream for the current frame.
/// - `ctx` must reference a live execution context for the same store, frame, and validated locals/stack layout.
/// - The active frame must have shared default memory.
/// - Callers must not preserve borrows, locks, or guards across any tail-dispatch that this helper performs.
/// - `make_operation` must not retain references into `ctx` after it returns because the helper will continue by tail-dispatching immediately after the write.
pub(crate) unsafe fn store_internal_shared(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    make_operation: impl FnOnce(&mut ExecuteContextFacade<'_, '_>) -> StoreBytes,
) -> VMResult<()> {
    let mut facade = ExecuteContextFacade::new(ctx);
    let memarg = decode_memarg_operand(tail_code);
    let operation = make_operation(&mut facade);
    let offset = facade.pop::<u32>();
    trace!("op_store_shared: {:?} {}", memarg, offset);
    let bytes = operation.as_slice();
    let start = vm_try!(compute_memory_offset(memarg, offset));
    vm_try!(facade.write_memory_bytes(start, bytes));
    facade_call_next(tail_code, 1, &mut facade)
}

/// Telomere runtime helper `store_internal_local_indexed`.
///
/// Related spec:
/// - Execution: https://webassembly.github.io/multi-memory/core/exec/instructions.html
///
/// Stack effect: `[i32, value] -> []`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Uses the pre-decoded indexed local-memory fast path and tail-dispatches after consuming `memarg + memidx`.
///
/// # Safety
/// - `tail_code` must reference the active decoded instruction stream for the current frame.
/// - `ctx` must reference a live execution context for the same store, frame, and validated locals/stack layout.
/// - The memory index operand at `tail_code.add(1)` must be in-bounds and refer to a local memory.
/// - Callers must not preserve borrows, locks, or guards across any tail-dispatch that this helper performs.
pub(crate) unsafe fn store_internal_local_indexed(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    make_operation: impl FnOnce(&mut ExecuteContextFacade<'_, '_>) -> StoreBytes,
) -> VMResult<()> {
    let mut facade = ExecuteContextFacade::new(ctx);
    let (memarg, memidx) = decode_indexed_memarg_operand(tail_code);
    let operation = make_operation(&mut facade);
    let offset = facade.pop::<u32>();
    let bytes = operation.as_slice();
    let start = vm_try!(compute_memory_offset(memarg, offset));
    vm_try!(facade.write_memory_bytes_local_indexed(memidx, start, bytes));
    facade_call_next(tail_code, 2, &mut facade)
}

/// Telomere runtime helper `store_internal_shared_indexed`.
///
/// Related spec:
/// - Execution: https://webassembly.github.io/multi-memory/core/exec/instructions.html
///
/// Stack effect: `[i32, value] -> []`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Uses the pre-decoded indexed shared-memory fast path and tail-dispatches after consuming `memarg + memidx`.
///
/// # Safety
/// - `tail_code` must reference the active decoded instruction stream for the current frame.
/// - `ctx` must reference a live execution context for the same store, frame, and validated locals/stack layout.
/// - The memory index operand at `tail_code.add(1)` must be in-bounds and refer to a shared memory.
/// - Callers must not preserve borrows, locks, or guards across any tail-dispatch that this helper performs.
pub(crate) unsafe fn store_internal_shared_indexed(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    make_operation: impl FnOnce(&mut ExecuteContextFacade<'_, '_>) -> StoreBytes,
) -> VMResult<()> {
    let mut facade = ExecuteContextFacade::new(ctx);
    let (memarg, memidx) = decode_indexed_memarg_operand(tail_code);
    let operation = make_operation(&mut facade);
    let offset = facade.pop::<u32>();
    let bytes = operation.as_slice();
    let start = vm_try!(compute_memory_offset(memarg, offset));
    vm_try!(facade.write_memory_bytes_shared_indexed(memidx, start, bytes));
    facade_call_next(tail_code, 2, &mut facade)
}
pub(crate) const VM_END: Instr = Instr {
    op: special_function_vm_end,
};

pub(crate) const START_HOST_FUNCTION_PROGRAM: [Instr; 1] = [Instr {
    op: special_start_function_call,
}];

pub async fn run_module_function(
    instance: &InstanceHandle,
    store: &Store,
    name: &str,
    args: &ResultValue,
) -> VMResult<ResultValue> {
    let mut driver = TokioDriver::new();
    run_module_function_with_driver(instance, store, name, args, &mut driver).await
}

pub async fn run_module_function_with_driver<D: ExecutionDriver>(
    instance: &InstanceHandle,
    store: &Store,
    name: &str,
    args: &ResultValue,
    driver: &mut D,
) -> VMResult<ResultValue> {
    if store.has_active_gc_on_current_thread() {
        tracing::error!(
            "run_module_function is unsupported while the same store GC is already active"
        );
        return VMResult::Unlinkable;
    }
    let mut kernel = ExecutionKernel::new(store);

    let ft = {
        let gc = store.lock_gc();
        let instance = gc.get_instance(vm_try!(VMResult::from_option(
            instance.get_gc_ref_with_pool(store, &gc),
            || { VMResult::Unlinkable }
        )));
        let module_inst = gc.get_module(instance.module_addr);
        trace!("{:?}", module_inst.exports);
        let ft = if let Some(ExportDesc::Func(idx)) = module_inst.exports.find(name) {
            let code_addr = *vm_try!(VMResult::from_option(
                instance.funcs.as_slice().get(idx.0 as usize),
                || { VMResult::Unlinkable }
            ));
            let funcinst = gc.get_func(code_addr);
            let func_instance = gc.instance(funcinst.instance);
            let frame = CallFrameCache::from_parts(
                code_addr,
                funcinst,
                func_instance
                    .memory_slots
                    .first()
                    .copied()
                    .and_then(|slot| slot.handle()),
            );
            let mut stack = Stack::new(128 * 1024);
            let tidx = *vm_try!(VMResult::from_option(
                module_inst.functions.get(idx.0 as usize),
                || { VMResult::Unlinkable }
            ));
            let ft = vm_try!(VMResult::from_option(
                module_inst.function_types.get(tidx.0 as usize),
                || { VMResult::Unlinkable }
            ))
            .clone();
            let param_size = result_type_size(&ft.0);

            let locals_data = funcinst.locals();
            let local_size = locals_data.byte_size();
            vm_try!(push_result_values(&mut stack, &ft.0, args));

            tracing::trace!("run_module_function: {name} {local_size}");
            let local_reference = vm_try!(stack.function_call(
                param_size,
                local_size,
                frame,
                LocalReference {
                    local_size: 0,
                    local_top: 0,
                },
                &VM_END as *const Instr,
                &gc,
            ));

            kernel.push(Task {
                fp: StablePc::from_relative_index(0),
                task_id: 0,
                stack,
                local_reference,
                ready_flag: ReadyFlag::Ready,
                pending_ops: 0,
                terminal_result: None,
            });
            ft
        } else {
            return VMResult::Unlinkable;
        };
        ft
    };
    kernel.run(driver).await;
    let ct = kernel.completed_tasks.pop().unwrap();
    vm_try!(ct.result);
    let mut stack = ct.stack;
    VMResult::Success(pop_result_values(&mut stack, &ft.1))
}

pub(crate) fn run_module_function_sync_with_gc(
    instance: &InstanceHandle,
    store: &Store,
    gc: &mut crate::common::StoreInner,
    name: &str,
    args: &ResultValue,
) -> Result<VMResult<ResultValue>, SyncRunError> {
    let mut kernel = ExecutionKernel::new(store);

    let ft = {
        let instance = gc.get_instance(match instance.get_gc_ref_with_pool(store, gc) {
            Some(gc_ref) => gc_ref,
            None => return Ok(VMResult::Unlinkable),
        });
        let module_inst = gc.get_module(instance.module_addr);
        trace!("{:?}", module_inst.exports);
        let ft = if let Some(ExportDesc::Func(idx)) = module_inst.exports.find(name) {
            let code_addr = match instance.funcs.as_slice().get(idx.0 as usize) {
                Some(code_addr) => *code_addr,
                None => return Ok(VMResult::Unlinkable),
            };
            let funcinst = gc.get_func(code_addr);
            let func_instance = gc.instance(funcinst.instance);
            let frame = CallFrameCache::from_parts(
                code_addr,
                funcinst,
                func_instance
                    .memory_slots
                    .first()
                    .copied()
                    .and_then(|slot| slot.handle()),
            );
            let mut stack = Stack::new(128 * 1024);
            let tidx = match module_inst.functions.get(idx.0 as usize) {
                Some(tidx) => *tidx,
                None => return Ok(VMResult::Unlinkable),
            };
            let ft = match module_inst.function_types.get(tidx.0 as usize) {
                Some(ft) => ft.clone(),
                None => return Ok(VMResult::Unlinkable),
            };
            let param_size = result_type_size(&ft.0);

            let locals_data = funcinst.locals();
            let local_size = locals_data.byte_size();
            let push_result = push_result_values(&mut stack, &ft.0, args);
            if !matches!(push_result, VMResult::Success(())) {
                return Ok(vm_result_err_into_result_value(push_result));
            }

            tracing::trace!("run_module_function: {name} {local_size}");
            let local_reference = match stack.function_call(
                param_size,
                local_size,
                frame,
                LocalReference {
                    local_size: 0,
                    local_top: 0,
                },
                &VM_END as *const Instr,
                gc,
            ) {
                VMResult::Success(local_reference) => local_reference,
                other => return Ok(vm_result_err_into_result_value(other)),
            };

            kernel.push(Task {
                fp: StablePc::from_relative_index(0),
                task_id: 0,
                stack,
                local_reference,
                ready_flag: ReadyFlag::Ready,
                pending_ops: 0,
                terminal_result: None,
            });
            ft
        } else {
            return Ok(VMResult::Unlinkable);
        };
        ft
    };

    kernel.run_sync_with_gc(gc)?;
    let ct = kernel.completed_tasks.pop().unwrap();
    match ct.result {
        VMResult::Success(()) => {
            let mut stack = ct.stack;
            Ok(VMResult::Success(pop_result_values(&mut stack, &ft.1)))
        }
        VMResult::Unreachable => Ok(VMResult::Unreachable),
        VMResult::StackOverflow => Ok(VMResult::StackOverflow),
        VMResult::MemoryIndexOutOfRange => Ok(VMResult::MemoryIndexOutOfRange),
        VMResult::TableIndexOutOfRange => Ok(VMResult::TableIndexOutOfRange),
        VMResult::CallIndirectInvalidType => Ok(VMResult::CallIndirectInvalidType),
        VMResult::TableUninitialized => Ok(VMResult::TableUninitialized),
        VMResult::Unlinkable => Ok(VMResult::Unlinkable),
        VMResult::InvalidOperand => Ok(VMResult::InvalidOperand),
        VMResult::UnalignedAtomic => Ok(VMResult::UnalignedAtomic),
    }
}

fn vm_result_err_into_result_value<T>(result: VMResult<T>) -> VMResult<ResultValue> {
    match result {
        VMResult::Success(_) => unreachable!(),
        VMResult::Unreachable => VMResult::Unreachable,
        VMResult::StackOverflow => VMResult::StackOverflow,
        VMResult::MemoryIndexOutOfRange => VMResult::MemoryIndexOutOfRange,
        VMResult::TableIndexOutOfRange => VMResult::TableIndexOutOfRange,
        VMResult::CallIndirectInvalidType => VMResult::CallIndirectInvalidType,
        VMResult::TableUninitialized => VMResult::TableUninitialized,
        VMResult::Unlinkable => VMResult::Unlinkable,
        VMResult::InvalidOperand => VMResult::InvalidOperand,
        VMResult::UnalignedAtomic => VMResult::UnalignedAtomic,
    }
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::*;
    use crate::common::{
        stack::CachedMemoryKind,
        store::{self, FunctionBody},
        AsyncHostFuture, ExportSection, FuncType, InstanceData, LocalsData, ModuleInstance,
        StoreInner, TypeIdx,
    };
    use crate::runtime::{memory_effect::PendingOp, scheduler::PendingOpEmitter};
    use std::{collections::VecDeque, future::ready, sync::Arc};

    fn async_host_noop(_ctx: AsyncHostCallContext) -> AsyncHostFuture {
        Box::pin(ready(VMResult::Success(ResultValue::new(Vec::new()))))
    }

    fn frame_for(funcaddr: GcRef, instance: store::InstanceId) -> CallFrameCache {
        CallFrameCache {
            code_addr: funcaddr,
            code_base: std::ptr::null(),
            instance,
            memory0_kind: CachedMemoryKind::None,
            memory0_raw: 0,
        }
    }

    #[test]
    fn start_async_host_call_observes_pending_host_call() {
        let store = Store::new();
        let mut gc = StoreInner::new();
        let module = gc.new_module(ModuleInstance {
            exports: ExportSection(Vec::new()),
            tables: Vec::new(),
            globals: Vec::new(),
            functions: vec![TypeIdx(0), TypeIdx(1)],
            function_types: vec![
                FuncType(ResultType(Vec::new()), ResultType(Vec::new())),
                FuncType(ResultType(Vec::new()), ResultType(Vec::new())),
            ],
            mems: Vec::new(),
        });
        let instance = store::InstanceId::from_index(0);
        let root_func = gc.new_func(&store::FunctionInstanceData {
            instance,
            funcidx: 0,
            typeidx: TypeIdx(0),
            param_size: 0,
            local_size: 0,
            body: FunctionBody::Wasm {
                locals: LocalsData::default(),
                code: Arc::<[Instr]>::from(vec![VM_END]),
            },
        });
        let async_func = gc.new_func(&store::FunctionInstanceData {
            instance,
            funcidx: 1,
            typeidx: TypeIdx(1),
            param_size: 0,
            local_size: 0,
            body: FunctionBody::AsyncHost(async_host_noop),
        });
        let _instance_addr = gc.new_instance(&InstanceData {
            instance_id: 1,
            module_addr: module,
            globals: Vec::new(),
            funcs: vec![root_func, async_func],
            tables: Vec::new(),
            mems: Vec::new(),
            memory_slots: Vec::new(),
        });

        let mut stack = Stack::new(128);
        let empty = LocalReference {
            local_top: 0,
            local_size: 0,
        };
        let root = stack
            .function_call(
                0,
                0,
                frame_for(root_func, instance),
                empty,
                std::ptr::null(),
                &gc,
            )
            .unwrap();
        let program = [VM_END, VM_END];
        let callee = stack
            .function_call(
                0,
                0,
                frame_for(async_func, instance),
                root,
                unsafe { program.as_ptr().add(1) },
                &gc,
            )
            .unwrap();

        let mut pending_effects = 0;
        let mut pending_ops = VecDeque::new();
        let cont = {
            let mut ctx = ExecuteContext::new(
                &mut stack,
                callee,
                frame_for(async_func, instance),
                &store,
                &mut gc,
                PendingOpEmitter::from_parts(41, &mut pending_effects, &mut pending_ops),
                program.as_ptr(),
                41,
            );

            let start = {
                let mut facade = ExecuteContextFacade::new(&mut ctx);
                facade.start_core_step_observation().unwrap()
            };
            let params = ResultType(Vec::new());
            let results = ResultType(Vec::new());
            let result = {
                let mut facade = ExecuteContextFacade::new(&mut ctx);
                start_async_host_call(&mut facade, &params, &results)
            };
            let observation = {
                let mut facade = ExecuteContextFacade::new(&mut ctx);
                facade.finish_core_step_observation(start, &result).unwrap()
            };

            result.unwrap();
            assert_eq!(
                observation.outcome,
                crate::common::formal::CoreOutcome::Pending(
                    crate::common::formal::PendingCode::HostCall,
                )
            );
            assert_eq!(
                observation.pending_code_delta,
                Some(crate::common::formal::PendingCode::HostCall)
            );
            ctx.cont()
        };
        assert_eq!(pending_effects, 1);
        assert_eq!(pending_ops.len(), 1);
        assert_eq!(cont, unsafe { program.as_ptr().add(1) });
        let PendingOp::HostCall(pending) = pending_ops.pop_front().unwrap() else {
            panic!("expected host-call pending op");
        };
        assert_eq!(pending.task_id, 41);
    }
}

pub fn get_global(instance: &InstanceHandle, store: &Store, name: &str) -> VMResult<WasmValue> {
    if store.has_active_gc_on_current_thread() {
        tracing::error!("get_global is unsupported while the same store GC is already active");
        return VMResult::Unlinkable;
    }
    let gc = store.lock_gc();

    let instance = unsafe {
        &*gc.get_instance_unchecked(vm_try!(VMResult::from_option(
            instance.get_gc_ref_with_pool(store, &gc),
            || { VMResult::Unlinkable }
        )))
    };
    let module_inst = gc.get_module(instance.module_addr);
    let Some(ExportDesc::Global(idx)) = module_inst.exports.find(name) else {
        return VMResult::Unlinkable;
    };
    let addr = *vm_try!(VMResult::from_option(
        instance.globals.as_slice().get(idx.0 as usize),
        || { VMResult::Unlinkable }
    ));
    let gt = *vm_try!(VMResult::from_option(
        module_inst.globals.get(idx.0 as usize),
        || { VMResult::Unlinkable }
    ));
    let Some(value) = read_value(gc.get_global(addr), gt.0) else {
        return VMResult::Unlinkable;
    };
    VMResult::Success(value)
}
