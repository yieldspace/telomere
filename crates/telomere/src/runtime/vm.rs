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
mod superinstructions;
mod tables;

use crate::{
    common::{
        execute_elem_init_const_expr, CallFrameCache, ElemInit, ExecuteContext, ExportDesc,
        InstanceHandle, Instr, LocalReference, MemArg, ObjectRef, Op, ResultType, ResultValue,
        StablePc, Stack, VMResult, ValType, WasmValue, TABLE_UNINITIALIZED,
    },
    runtime::{
        memory_effect::{HostCallPending, PendingOp},
        scheduler::{ExecutionDriver, ReadyFlag, Scheduler, SyncRunError, Task, TokioDriver},
    },
    Store,
};

pub(crate) use crate::common::{
    FloatCompareKind, FloatScalarKind, I32ScalarKind, I64ScalarKind, IntCompareKind, Load4Kind,
    Load8Kind, Store4Kind, Store8Kind,
};

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

#[inline(always)]
fn stable_dispatch_hash(function_index: u32, instruction_ordinal: u32) -> usize {
    let mut x = ((function_index as u64) << 32) | u64::from(instruction_ordinal);
    x ^= x >> 33;
    x = x.wrapping_mul(0xff51_afd7_ed55_8ccd);
    x ^= x >> 33;
    x = x.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    x ^= x >> 33;
    x as usize
}

pub(crate) fn select_replicated_op(base: Op, function_index: u32, instruction_ordinal: u32) -> Op {
    let hash = stable_dispatch_hash(function_index, instruction_ordinal);
    if std::ptr::fn_addr_eq(base, locals::op_local_get4 as Op) {
        return [
            locals::op_local_get4_r0,
            locals::op_local_get4_r1,
            locals::op_local_get4_r2,
            locals::op_local_get4_r3,
        ][hash & 3];
    }
    if std::ptr::fn_addr_eq(base, locals::op_local_set4 as Op) {
        return [
            locals::op_local_set4_r0,
            locals::op_local_set4_r1,
            locals::op_local_set4_r2,
            locals::op_local_set4_r3,
        ][hash & 3];
    }
    if std::ptr::fn_addr_eq(base, locals::op_local_tee4 as Op) {
        return [
            locals::op_local_tee4_r0,
            locals::op_local_tee4_r1,
            locals::op_local_tee4_r2,
            locals::op_local_tee4_r3,
        ][hash & 3];
    }
    if std::ptr::fn_addr_eq(base, memory::op_i32_load8_u_local as Op) {
        return [
            memory::op_i32_load8_u_local_r0,
            memory::op_i32_load8_u_local_r1,
            memory::op_i32_load8_u_local_r2,
            memory::op_i32_load8_u_local_r3,
        ][hash & 3];
    }
    if std::ptr::fn_addr_eq(base, memory::op_f32_load_local as Op) {
        return [
            memory::op_f32_load_local_r0,
            memory::op_f32_load_local_r1,
            memory::op_f32_load_local_r2,
            memory::op_f32_load_local_r3,
        ][hash & 3];
    }
    if std::ptr::fn_addr_eq(base, control::op_br_if as Op) {
        return [
            control::op_br_if_r0,
            control::op_br_if_r1,
            control::op_br_if_r2,
            control::op_br_if_r3,
        ][hash & 3];
    }
    base
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
    let sum = memarg.offset as u64 + offset as u64;
    if sum <= u32::MAX as u64 {
        VMResult::Success(sum as usize)
    } else {
        VMResult::MemoryIndexOutOfRange
    }
}

enum CallOutcome {
    Immediate(*const Instr),
    Pending,
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
    ctx.cont = tail_code;
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

#[inline(always)]
fn touch_current_ref_ranges(_ctx: &ExecuteContext) {
    #[cfg(debug_assertions)]
    _ctx.stack
        .visit_local_and_operand_ref_ranges(&_ctx.local_reference, _ctx.safepoint, |_| {});
}

fn start_async_host_call(
    _return_pc: crate::common::StablePc,
    return_addr: *const Instr,
    safepoint: crate::common::SafepointMetadataCache,
    ctx: &mut ExecuteContext,
) -> VMResult<CallOutcome> {
    let async_host = ctx.func().async_host_code_pointer();
    let task_id = ctx.task_id;
    touch_current_ref_ranges(ctx);
    let future = async_host(ctx);
    ctx.effect
        .push_pending(PendingOp::HostCall(HostCallPending {
            task_id,
            future,
            safepoint,
        }));
    ctx.cont = return_addr;
    VMResult::Success(CallOutcome::Pending)
}

fn invoke_host_function(
    return_pc: crate::common::StablePc,
    return_addr: *const Instr,
    safepoint: crate::common::SafepointMetadataCache,
    ctx: &mut ExecuteContext,
) -> VMResult<CallOutcome> {
    if ctx.func().is_async_host_func() {
        start_async_host_call(return_pc, return_addr, safepoint, ctx)
    } else {
        let fp = ctx.func().host_code_pointer();
        let return_addr = vm_try!(fp(ctx));
        VMResult::Success(CallOutcome::Immediate(return_addr))
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
    op_call, op_call_import, op_call_import_precomputed, op_call_indirect,
    op_call_indirect_precomputed, op_call_precomputed, op_return_call, op_return_call_import,
    op_return_call_import_precomputed, op_return_call_indirect,
    op_return_call_indirect_precomputed, op_return_call_precomputed, special_start_function_call,
};
pub use control::special_function_return;
pub(crate) use control::*;
pub(crate) use globals::*;
pub(crate) use locals::*;
pub(crate) use memory::*;
pub(crate) use numeric::*;
pub(crate) use refs::*;
pub(crate) use superinstructions::*;
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
    let offset = ctx.stack.pop_u32();
    trace!("op_store: {:?} {}", memarg, offset);
    let bytes = operation.as_slice();
    let start = vm_try!(compute_memory_offset(memarg, offset));
    vm_try!(ctx
        .gc
        .local_write_bytes(ctx.default_local_memory_id_unchecked(), start, bytes,));
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
    let offset = ctx.stack.pop_u32();
    trace!("op_store_shared: {:?} {}", memarg, offset);
    let bytes = operation.as_slice();
    let start = vm_try!(compute_memory_offset(memarg, offset));
    vm_try!(ctx
        .gc
        .shared_write_bytes(ctx.default_shared_memory_id_unchecked(), start, bytes,));
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
    let offset = ctx.stack.pop_u32();
    let bytes = operation.as_slice();
    let start = vm_try!(compute_memory_offset(memarg, offset));
    vm_try!(ctx
        .gc
        .local_write_bytes(ctx.local_memory_id_at_unchecked(memidx), start, bytes));
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
    let offset = ctx.stack.pop_u32();
    let bytes = operation.as_slice();
    let start = vm_try!(compute_memory_offset(memarg, offset));
    vm_try!(ctx
        .gc
        .shared_write_bytes(ctx.shared_memory_id_at_unchecked(memidx), start, bytes));
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
    let mut scheduler: Scheduler<'_> = Scheduler::new(store);

    let ft = {
        let gc = store.lock_gc();
        let instance = gc.get_instance(vm_try!(VMResult::from_option(
            instance.object_ref_for_store(store),
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

            vm_try!(push_result_values(&mut stack, &ft.0, args));

            tracing::trace!("run_module_function: {name} {param_size}");
            let local_reference = if funcinst.is_host_func() {
                vm_try!(stack.function_call_raw(
                    param_size,
                    0,
                    frame,
                    LocalReference::empty(),
                    &VM_END as *const Instr,
                    &gc,
                ))
            } else {
                let wasm_metadata = funcinst
                    .wasm_metadata()
                    .expect("wasm function must expose execution metadata");
                vm_try!(stack.function_call_layout(
                    wasm_metadata.frame_layout.as_ref(),
                    frame,
                    LocalReference::empty(),
                    &VM_END as *const Instr,
                    &gc,
                ))
            };

            scheduler.push(Task {
                fp: StablePc::from_relative_index(0),
                task_id: 0,
                stack,
                local_reference,
                current_frame: frame,
                safepoint: crate::common::SafepointMetadataCache::EMPTY,
                ready_flag: ReadyFlag::Ready,
                pending_effects: 0,
                terminal_result: None,
            });
            ft
        } else {
            return VMResult::Unlinkable;
        };
        ft
    };
    scheduler.run_with_driver(driver).await;
    let ct = scheduler.completed_tasks.pop().unwrap();
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
    let mut scheduler: Scheduler<'_> = Scheduler::new(store);

    let ft = {
        let instance = gc.get_instance(match instance.object_ref_for_store(store) {
            Some(object_ref) => object_ref,
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

            let push_result = push_result_values(&mut stack, &ft.0, args);
            if !matches!(push_result, VMResult::Success(())) {
                return Ok(vm_result_err_into_result_value(push_result));
            }

            tracing::trace!("run_module_function: {name} {param_size}");
            let local_reference = match if funcinst.is_host_func() {
                stack.function_call_raw(
                    param_size,
                    0,
                    frame,
                    LocalReference::empty(),
                    &VM_END as *const Instr,
                    gc,
                )
            } else {
                let wasm_metadata = funcinst
                    .wasm_metadata()
                    .expect("wasm function must expose execution metadata");
                stack.function_call_layout(
                    wasm_metadata.frame_layout.as_ref(),
                    frame,
                    LocalReference::empty(),
                    &VM_END as *const Instr,
                    gc,
                )
            } {
                VMResult::Success(local_reference) => local_reference,
                other => return Ok(vm_result_err_into_result_value(other)),
            };

            scheduler.push(Task {
                fp: StablePc::from_relative_index(0),
                task_id: 0,
                stack,
                local_reference,
                current_frame: frame,
                safepoint: crate::common::SafepointMetadataCache::EMPTY,
                ready_flag: ReadyFlag::Ready,
                pending_effects: 0,
                terminal_result: None,
            });
            ft
        } else {
            return Ok(VMResult::Unlinkable);
        };
        ft
    };

    scheduler.run_sync_with_gc(gc)?;
    let ct = scheduler.completed_tasks.pop().unwrap();
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

fn read_global_value(bytes: &[u8], ty: ValType) -> Option<WasmValue> {
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

pub fn get_global(instance: &InstanceHandle, store: &Store, name: &str) -> VMResult<WasmValue> {
    if store.has_active_gc_on_current_thread() {
        tracing::error!("get_global is unsupported while the same store GC is already active");
        return VMResult::Unlinkable;
    }
    let gc = store.lock_gc();

    let instance = unsafe {
        &*gc.get_instance_unchecked(vm_try!(VMResult::from_option(
            instance.object_ref_for_store(store),
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
    let Some(value) = read_global_value(gc.get_global(addr), gt.0) else {
        return VMResult::Unlinkable;
    };
    VMResult::Success(value)
}
