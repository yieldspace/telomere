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
        execute_elem_init_const_expr, AsyncHostCallContext, CallFrameCache, ElemInit,
        ExecuteContext, ExportDesc, FuncType, GcRef, HostCallContext, HostCallControl,
        HostTailCallTarget, InstanceHandle, Instr, LocalReference, MemArg, ResultType, ResultValue,
        StablePc, Stack, VMResult, ValType, WasmValue, TABLE_UNINITIALIZED,
    },
    runtime::{
        memory_effect::{HostCallPending, PendingOp},
        scheduler::{ExecutionDriver, ExecutionKernel, ReadyFlag, SyncRunError, Task, TokioDriver},
    },
    Store,
};
use vstd::prelude::*;

verus! {

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

fn write_marshaled_results(
    stack: &mut Stack,
    slot_offset: usize,
    types: &ResultType,
    values: &ResultValue,
) -> VMResult<()> {
    let slot = LocalReference {
        local_top: slot_offset,
        local_size: result_type_size(types) as u32,
    };
    if types.0.len() != values.len() {
        return VMResult::InvalidOperand;
    }
    let mut offset = 0usize;
    let dst = unsafe { stack.local_area_mut_ptr(&slot) };
    for (ty, value) in types.iter().zip(values.iter()) {
        let size = ty.stack_size().usize();
        unsafe {
            match (ty, value) {
                (ValType::I32, WasmValue::I32(value)) => {
                    std::ptr::copy_nonoverlapping(
                        value.to_le_bytes().as_ptr(),
                        dst.add(offset),
                        size,
                    );
                }
                (ValType::I64, WasmValue::I64(value)) => {
                    std::ptr::copy_nonoverlapping(
                        value.to_le_bytes().as_ptr(),
                        dst.add(offset),
                        size,
                    );
                }
                (ValType::F32, WasmValue::F32(value)) => {
                    std::ptr::copy_nonoverlapping(
                        value.to_bits().to_le_bytes().as_ptr(),
                        dst.add(offset),
                        size,
                    );
                }
                (ValType::F64, WasmValue::F64(value)) => {
                    std::ptr::copy_nonoverlapping(
                        value.to_bits().to_le_bytes().as_ptr(),
                        dst.add(offset),
                        size,
                    );
                }
                (ValType::V128, WasmValue::V128(value)) => {
                    std::ptr::copy_nonoverlapping(
                        value.to_le_bytes().as_ptr(),
                        dst.add(offset),
                        size,
                    );
                }
                (ValType::FuncRef, WasmValue::FuncRef(value)) => {
                    std::ptr::copy_nonoverlapping(
                        value.to_le_bytes().as_ptr(),
                        dst.add(offset),
                        size,
                    );
                }
                (ValType::ExternRef, WasmValue::ExternRef(value)) => {
                    std::ptr::copy_nonoverlapping(
                        value.to_le_bytes().as_ptr(),
                        dst.add(offset),
                        size,
                    );
                }
                _ => return VMResult::InvalidOperand,
            }
        }
        offset += size;
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

unsafe fn function_type_ptr_by_func(
    ctx: &ExecuteContext,
    func: &crate::common::FunctionInstanceData,
) -> *const FuncType {
    let gc = ctx.gc_ref();
    let instance = gc.instance(func.instance);
    let module = gc.get_module(instance.module_addr);
    module.function_types.get_unchecked(func.typeidx.0 as usize) as *const FuncType
}

unsafe fn current_function_type_ptr(ctx: &ExecuteContext) -> *const FuncType {
    unsafe { function_type_ptr_by_func(ctx, ctx.func()) }
}

unsafe fn function_type_ptr_by_addr(ctx: &ExecuteContext, funcaddr: GcRef) -> *const FuncType {
    unsafe { function_type_ptr_by_func(ctx, ctx.func_by_addr(funcaddr)) }
}

fn start_async_host_call(
    ctx: &mut ExecuteContext,
    param_types: &ResultType,
    result_types: &ResultType,
) -> VMResult<()> {
    let async_host = ctx.func().async_host_code_pointer();
    let params = vm_try!(marshal_local_values(
        ctx.stack_ref(),
        &ctx.local_reference(),
        param_types,
    ));
    let result_types = result_types.clone();
    let result_slot = ctx.local_reference().local_top;
    let return_size = result_type_size(&result_types);
    let (resume_pc, _slot) = ctx.function_return_in_place(return_size);
    let fp = StablePc::from_raw_in_frame(
        ctx.gc_ref(),
        ctx.stack_ref(),
        ctx.local_reference(),
        resume_pc,
    );
    let future = async_host(AsyncHostCallContext {
        params,
        result_types: result_types.clone(),
        store_state: ctx.store_ref().state,
    });
    let task_id = ctx.task_id();
    ctx.pending_mut()
        .push_pending(PendingOp::HostCall(HostCallPending {
            task_id,
            future,
            fp,
            result_types,
            result_slot,
        }));
    ctx.set_cont(resume_pc);
    VMResult::Success(())
}

fn resolve_host_tail_call_target(
    ctx: &ExecuteContext,
    target: HostTailCallTarget,
) -> VMResult<GcRef> {
    match target {
        HostTailCallTarget::FuncIdx(funcidx) => VMResult::from_option(
            ctx.instance()
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
    ctx: &mut ExecuteContext,
    result_types: &ResultType,
    values: ResultValue,
) -> VMResult<()> {
    let return_size = result_type_size(result_types);
    let (return_addr, result_slot) = ctx.function_return_in_place(return_size);
    vm_try!(write_marshaled_results(
        ctx.stack_mut(),
        result_slot,
        result_types,
        &values,
    ));
    unsafe { call_code(return_addr, ctx) }
}

fn complete_sync_host_tail_call(
    ctx: &mut ExecuteContext,
    target: HostTailCallTarget,
    params: ResultValue,
) -> VMResult<()> {
    let funcaddr = vm_try!(resolve_host_tail_call_target(ctx, target));
    let function_type = unsafe { &*function_type_ptr_by_addr(ctx, funcaddr) };
    vm_try!(push_result_values(
        ctx.stack_mut(),
        &function_type.0,
        &params,
    ));
    let ptr = vm_try!(unsafe { call::internal_op_call(std::ptr::null(), funcaddr, ctx, true) });
    if ptr.is_null() {
        VMResult::Success(())
    } else {
        unsafe { call_next(ptr, 0, ctx) }
    }
}

unsafe fn invoke_host_function(
    return_addr: *const Instr,
    ctx: &mut ExecuteContext,
    param_types: *const ResultType,
    result_types: *const ResultType,
) -> VMResult<()> {
    let param_types = unsafe { &*param_types };
    let result_types = unsafe { &*result_types };
    if ctx.func().is_async_host_func() {
        let _ = return_addr;
        vm_try!(start_async_host_call(ctx, param_types, result_types));
        VMResult::Success(())
    } else {
        let fp = ctx.func().host_code_pointer();
        let control = vm_try!(fp(HostCallContext::new(ctx, param_types, result_types)));
        match control {
            HostCallControl::Return(values) => complete_sync_host_return(ctx, result_types, values),
            HostCallControl::TailCall { target, params } => {
                complete_sync_host_tail_call(ctx, target, params)
            }
            HostCallControl::EndProgram => {
                ctx.end_program();
                VMResult::Success(())
            }
        }
    }
}

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
    make_operation: impl FnOnce(&mut ExecuteContext) -> StoreBytes,
) -> VMResult<()> {
    let memarg = (*tail_code).operand.memarg;
    let operation = make_operation(ctx);
    let offset = ctx.stack_mut().pop_u32();
    trace!("op_store: {:?} {}", memarg, offset);
    let bytes = operation.as_slice();
    let start = vm_try!(compute_memory_offset(memarg, offset));
    vm_try!(ctx.write_memory_bytes(start, bytes));
    call_next(tail_code, 1, ctx)
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
    make_operation: impl FnOnce(&mut ExecuteContext) -> StoreBytes,
) -> VMResult<()> {
    let memarg = (*tail_code).operand.memarg;
    let operation = make_operation(ctx);
    let offset = ctx.stack_mut().pop_u32();
    trace!("op_store_shared: {:?} {}", memarg, offset);
    let bytes = operation.as_slice();
    let start = vm_try!(compute_memory_offset(memarg, offset));
    vm_try!(ctx.write_memory_bytes(start, bytes));
    call_next(tail_code, 1, ctx)
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
    make_operation: impl FnOnce(&mut ExecuteContext) -> StoreBytes,
) -> VMResult<()> {
    let memarg = (*tail_code).operand.memarg;
    let memidx = (*tail_code.add(1)).operand.u32;
    let operation = make_operation(ctx);
    let offset = ctx.stack_mut().pop_u32();
    let bytes = operation.as_slice();
    let start = vm_try!(compute_memory_offset(memarg, offset));
    vm_try!(ctx.write_memory_bytes_local_indexed(memidx, start, bytes));
    call_next(tail_code, 2, ctx)
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
    make_operation: impl FnOnce(&mut ExecuteContext) -> StoreBytes,
) -> VMResult<()> {
    let memarg = (*tail_code).operand.memarg;
    let memidx = (*tail_code.add(1)).operand.u32;
    let operation = make_operation(ctx);
    let offset = ctx.stack_mut().pop_u32();
    let bytes = operation.as_slice();
    let start = vm_try!(compute_memory_offset(memarg, offset));
    vm_try!(ctx.write_memory_bytes_shared_indexed(memidx, start, bytes));
    call_next(tail_code, 2, ctx)
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
