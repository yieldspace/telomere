use crate::{
    common::store::{CallDispatchCache, CallDispatchTarget},
    common::{
        execute_elem_init_const_expr, AtomicWaitResult, ElemInit, ExecuteContext, Instr,
        SharedMemoryObject, SharedWaitRegistration, StablePc, VMResult,
    },
    runtime::{
        jit,
        memory_effect::{MemoryWaitPending, PendingOp},
        vm,
    },
};

use super::abi::JitNativeExit;

const RUNTIME_STUB_DATA_DROP: u32 = 0;
const RUNTIME_STUB_ELEM_DROP: u32 = 1;
const RUNTIME_STUB_MEM_INIT_LOCAL: u32 = 2;
const RUNTIME_STUB_MEM_INIT_SHARED: u32 = 3;
const RUNTIME_STUB_MEM_INIT_INDEXED_LOCAL: u32 = 4;
const RUNTIME_STUB_MEM_INIT_INDEXED_SHARED: u32 = 5;
const RUNTIME_STUB_MEM_COPY_SHARED: u32 = 6;
const RUNTIME_STUB_MEM_COPY_INDEXED_LOCAL_LOCAL: u32 = 7;
const RUNTIME_STUB_MEM_COPY_INDEXED_LOCAL_SHARED: u32 = 8;
const RUNTIME_STUB_MEM_COPY_INDEXED_SHARED_LOCAL: u32 = 9;
const RUNTIME_STUB_MEM_COPY_INDEXED_SHARED_SHARED: u32 = 10;
const RUNTIME_STUB_MEM_FILL_SHARED: u32 = 11;
const RUNTIME_STUB_MEM_FILL_INDEXED_LOCAL: u32 = 12;
const RUNTIME_STUB_MEM_FILL_INDEXED_SHARED: u32 = 13;
const RUNTIME_STUB_MEM_SIZE_INDEXED_LOCAL: u32 = 14;
const RUNTIME_STUB_MEM_SIZE_INDEXED_SHARED: u32 = 15;
const RUNTIME_STUB_MEM_GROW_INDEXED_LOCAL: u32 = 16;
const RUNTIME_STUB_MEM_GROW_INDEXED_SHARED: u32 = 17;
const RUNTIME_STUB_TABLE_GET: u32 = 18;
const RUNTIME_STUB_TABLE_SET: u32 = 19;
const RUNTIME_STUB_TABLE_INIT: u32 = 20;
const RUNTIME_STUB_TABLE_COPY: u32 = 21;
const RUNTIME_STUB_TABLE_GROW: u32 = 22;
const RUNTIME_STUB_TABLE_SIZE: u32 = 23;
const RUNTIME_STUB_TABLE_FILL: u32 = 24;
const RUNTIME_STUB_CALL_NUMERIC_TOKEN_STATE_TRANSITION: u32 = 25;
const RUNTIME_STUB_CALL_CACHED_U16_LOW7_GUARD: u32 = 26;
const RUNTIME_STUB_I8X16_EXTRACT_LANE_S: u32 = 27;
const RUNTIME_STUB_V128_BITSELECT: u32 = 28;
const RUNTIME_STUB_ATOMIC_NOTIFY_LOCAL: u32 = 29;
const RUNTIME_STUB_ATOMIC_NOTIFY_SHARED: u32 = 30;
const RUNTIME_STUB_ATOMIC_NOTIFY_INDEXED_LOCAL: u32 = 31;
const RUNTIME_STUB_ATOMIC_NOTIFY_INDEXED_SHARED: u32 = 32;
const RUNTIME_STUB_ATOMIC_WAIT32_LOCAL: u32 = 33;
const RUNTIME_STUB_ATOMIC_WAIT32_SHARED: u32 = 34;
const RUNTIME_STUB_ATOMIC_WAIT32_INDEXED_LOCAL: u32 = 35;
const RUNTIME_STUB_ATOMIC_WAIT32_INDEXED_SHARED: u32 = 36;
const RUNTIME_STUB_ATOMIC_WAIT64_LOCAL: u32 = 37;
const RUNTIME_STUB_ATOMIC_WAIT64_SHARED: u32 = 38;
const RUNTIME_STUB_ATOMIC_WAIT64_INDEXED_LOCAL: u32 = 39;
const RUNTIME_STUB_ATOMIC_WAIT64_INDEXED_SHARED: u32 = 40;

const RUNTIME_CONT_I32_GUARDED_LOAD8_UPDATE_BR_IF_FALSE_BR_TABLE: u32 = 1;
const RUNTIME_CONT_I32_GUARDED_LOAD8_UPDATE_BR_IF_TAKEN_CONST_CMP_BR_TABLE: u32 = 2;
const RUNTIME_CONT_I32_GUARDED_LOAD8_UPDATE_BR_IF_TAKEN_BR_TABLE: u32 = 3;
const RUNTIME_CONT_I32_INC_LOAD8_UPDATE_BR_IF: u32 = 4;
const RUNTIME_CONT_I32_LOAD16_S_DOT4_LOOP: u32 = 5;
const RUNTIME_CONT_I32_LOAD16_S_MUL_ADD_DELTA_LOOP: u32 = 6;
const RUNTIME_CONT_I32_LOAD16_S_MUL_ADD_LOOP: u32 = 7;
const RUNTIME_CONT_I32_LOAD16_U_BITMIX_DELTA_LOOP: u32 = 8;
const RUNTIME_CONT_I32_LOAD8_UPDATE_BR_IF: u32 = 9;
const RUNTIME_CONT_I32_LOAD8_UPDATE_BR_IF_FALLTHROUGH_LOCAL_GET4: u32 = 10;
const RUNTIME_CONT_I32_LOAD8_UPDATE_BR_IF_TAKEN_LOCAL_GET4: u32 = 11;
const RUNTIME_CONT_I32_LOAD_MASKED_COMPARE_BR_IF: u32 = 12;
const RUNTIME_CONT_I32_MATRIX_I16_CRC_SUMMARY: u32 = 13;
const RUNTIME_CONT_I32_SUM_CLIP_LOOP: u32 = 14;
const RUNTIME_CONT_START_FUNCTION_CALL: u32 = 15;
const RUNTIME_CONT_START_JIT_FUNCTION_CALL: u32 = 16;
const RUNTIME_CONT_I32_LOAD8_U_LOCAL_BASE_TEE4_LOCAL_GET4: u32 = 17;
const RUNTIME_CONT_I32_LOAD8_S_LOCAL_BASE_TEE4_LOCAL_GET4: u32 = 18;
const RUNTIME_CONT_I32_LOAD16_U_LOCAL_BASE_TEE4_LOCAL_GET4: u32 = 19;
const RUNTIME_CONT_I32_LOAD16_S_LOCAL_BASE_TEE4_LOCAL_GET4: u32 = 20;
const RUNTIME_CONT_I32_LOAD_LOCAL_BASE_SET4_I32_LOAD_LOCAL_BASE_LOCAL_EQ_BR_IF: u32 = 21;
const RUNTIME_CONT_I32_LOAD_LOCAL_BASE_SET4_I32_LOAD8_S_LOCAL_BASE_LOCAL_EQ_BR_IF: u32 = 22;
const RUNTIME_CONT_I32_LOAD_LOCAL_BASE_SET4_I32_LOAD16_U_LOCAL_BASE_LOCAL_EQ_BR_IF: u32 = 23;
const RUNTIME_CONT_I32_LOAD_LOCAL_BASE_SET4_I32_LOAD16_S_LOCAL_BASE_LOCAL_EQ_BR_IF: u32 = 24;
const RUNTIME_CONT_I32_LOAD16_U_LOCAL_BASE_LOCAL_GET4_I32_LOAD16_U_LOCAL_GET4: u32 = 25;
const RUNTIME_CONT_I32_LOAD16_S_LOCAL_BASE_LOCAL_GET4_I32_LOAD16_S_LOCAL_GET4: u32 = 26;
const RUNTIME_CONT_LOCAL_GET4_I32_LOAD16_U_LOCAL_BASE_LOCAL_GET4_I32_LOAD16_U: u32 = 27;
const RUNTIME_CONT_LOCAL_GET4_I32_LOAD16_S_LOCAL_BASE_LOCAL_GET4_I32_LOAD16_S: u32 = 28;
const RUNTIME_CONT_F32_STORE_LOCAL_BASE: u32 = 29;
const RUNTIME_CONT_F64_STORE_LOCAL_SCALED_INDEX: u32 = 30;
const RUNTIME_CONT_I32_LOAD_INDEXED_LOCAL_BASE: u32 = 31;
const RUNTIME_CONT_I32_LOAD_INDEXED_SHARED_LOCAL_BASE: u32 = 32;
const RUNTIME_CONT_I32_LOAD_LOCAL_BASE_SET4_I32_LOAD_LOCAL_BASE_LOCAL_GET4: u32 = 33;
const RUNTIME_CONT_I32_LOAD_LOCAL_SCALED_INDEX: u32 = 34;
const RUNTIME_CONT_I32_LOAD_SHARED_LOCAL_BASE: u32 = 35;
const RUNTIME_CONT_I32_STORE_INDEXED_LOCAL: u32 = 36;
const RUNTIME_CONT_I32_STORE_INDEXED_LOCAL_BASE: u32 = 37;
const RUNTIME_CONT_I32_STORE_INDEXED_LOCAL_SCALED_INDEX: u32 = 38;
const RUNTIME_CONT_I64_LOAD_LOCAL_SCALED_INDEX: u32 = 39;
const RUNTIME_CONT_SIMD_F32X4_REPLACE_LANE: u32 = 40;
const RUNTIME_CONT_SIMD_F64X2_REPLACE_LANE: u32 = 41;
const RUNTIME_CONT_SIMD_I16X8_REPLACE_LANE: u32 = 42;
const RUNTIME_CONT_SIMD_I16X8_SHL: u32 = 43;
const RUNTIME_CONT_SIMD_I16X8_SHR: u32 = 44;
const RUNTIME_CONT_SIMD_I32X4_REPLACE_LANE: u32 = 45;
const RUNTIME_CONT_SIMD_I32X4_SHL: u32 = 46;
const RUNTIME_CONT_SIMD_I32X4_SHR: u32 = 47;
const RUNTIME_CONT_SIMD_I64X2_REPLACE_LANE: u32 = 48;
const RUNTIME_CONT_SIMD_I64X2_SHL: u32 = 49;
const RUNTIME_CONT_SIMD_I64X2_SHR: u32 = 50;
const RUNTIME_CONT_SIMD_I8X16_REPLACE_LANE: u32 = 51;
const RUNTIME_CONT_SIMD_I8X16_SHL: u32 = 52;
const RUNTIME_CONT_SIMD_I8X16_SHR: u32 = 53;
const RUNTIME_CONT_SIMD_I8X16_SHUFFLE: u32 = 54;
const RUNTIME_CONT_SIMD_I8X16_SWIZZLE: u32 = 55;
const RUNTIME_CONT_SIMD_V128_LOAD: u32 = 56;
const RUNTIME_CONT_SIMD_V128_LOAD_INDEXED_LOCAL: u32 = 57;
const RUNTIME_CONT_SIMD_V128_LOAD_INDEXED_SHARED: u32 = 58;
const RUNTIME_CONT_SIMD_V128_LOAD_SHARED: u32 = 59;

pub(crate) extern "C" fn push_i32(ctx: *mut ExecuteContext<'_>, value: u32) -> JitNativeExit {
    let ctx = unsafe { &mut *ctx };
    match ctx.stack.push_u32_fast(value) {
        VMResult::Success(()) => JitNativeExit::keep_going(),
        other => JitNativeExit::trap(other),
    }
}

pub(crate) extern "C" fn pop_i32(ctx: *mut ExecuteContext<'_>) -> u32 {
    let ctx = unsafe { &mut *ctx };
    ctx.stack.pop_u32_fast()
}

pub(crate) extern "C" fn i32_popcnt_value(value: u32) -> u32 {
    value.count_ones()
}

pub(crate) extern "C" fn i64_popcnt_value(value: u64) -> u64 {
    u64::from(value.count_ones())
}

pub(crate) extern "C" fn f32_min_bits(lhs: u32, rhs: u32) -> u32 {
    let lhs = f32::from_bits(lhs);
    let rhs = f32::from_bits(rhs);
    if lhs.is_nan() || rhs.is_nan() {
        f32::NAN.to_bits()
    } else if lhs == 0.0 && rhs == 0.0 && (lhs.is_sign_negative() || rhs.is_sign_negative()) {
        (-0.0f32).to_bits()
    } else {
        lhs.min(rhs).to_bits()
    }
}

pub(crate) extern "C" fn f32_max_bits(lhs: u32, rhs: u32) -> u32 {
    let lhs = f32::from_bits(lhs);
    let rhs = f32::from_bits(rhs);
    if lhs.is_nan() || rhs.is_nan() {
        f32::NAN.to_bits()
    } else if lhs == 0.0 && rhs == 0.0 && (lhs.is_sign_positive() || rhs.is_sign_positive()) {
        0.0f32.to_bits()
    } else {
        lhs.max(rhs).to_bits()
    }
}

pub(crate) extern "C" fn f32_copysign_bits(lhs: u32, rhs: u32) -> u32 {
    f32::from_bits(lhs).copysign(f32::from_bits(rhs)).to_bits()
}

pub(crate) extern "C" fn f64_min_bits(lhs: u64, rhs: u64) -> u64 {
    let lhs = f64::from_bits(lhs);
    let rhs = f64::from_bits(rhs);
    if lhs.is_nan() || rhs.is_nan() {
        f64::NAN.to_bits()
    } else if lhs == 0.0 && rhs == 0.0 && (lhs.is_sign_negative() || rhs.is_sign_negative()) {
        (-0.0f64).to_bits()
    } else {
        lhs.min(rhs).to_bits()
    }
}

pub(crate) extern "C" fn f64_max_bits(lhs: u64, rhs: u64) -> u64 {
    let lhs = f64::from_bits(lhs);
    let rhs = f64::from_bits(rhs);
    if lhs.is_nan() || rhs.is_nan() {
        f64::NAN.to_bits()
    } else if lhs == 0.0 && rhs == 0.0 && (lhs.is_sign_positive() || rhs.is_sign_positive()) {
        0.0f64.to_bits()
    } else {
        lhs.max(rhs).to_bits()
    }
}

pub(crate) extern "C" fn f64_copysign_bits(lhs: u64, rhs: u64) -> u64 {
    f64::from_bits(lhs).copysign(f64::from_bits(rhs)).to_bits()
}

pub(crate) extern "C" fn f32_convert_i32_bits(value: u32, signed: u32) -> u32 {
    if signed != 0 {
        (value as i32 as f32).to_bits()
    } else {
        (value as f32).to_bits()
    }
}

pub(crate) extern "C" fn f32_convert_i64_bits(value: u64, signed: u32) -> u32 {
    if signed != 0 {
        (value as i64 as f32).to_bits()
    } else {
        (value as f32).to_bits()
    }
}

pub(crate) extern "C" fn f32_demote_f64_bits(value: u64) -> u32 {
    (f64::from_bits(value) as f32).to_bits()
}

pub(crate) extern "C" fn f64_promote_f32_bits(value: u32) -> u64 {
    f64::from(f32::from_bits(value)).to_bits()
}

fn value_exit(value: u64) -> JitNativeExit {
    JitNativeExit {
        kind: JitNativeExit::KEEP_GOING,
        value,
    }
}

pub(crate) extern "C" fn i32_trunc_f32(value: u32, signed: u32) -> JitNativeExit {
    let value = f32::from_bits(value);
    let converted = if signed != 0 {
        if (i32::MIN as f32) <= value && value < (i32::MAX as f32) {
            value as i32 as u32
        } else {
            return JitNativeExit::trap(VMResult::<()>::InvalidOperand);
        }
    } else if -1.0 < value && value < (u32::MAX as f32) {
        value.trunc() as u32
    } else {
        return JitNativeExit::trap(VMResult::<()>::InvalidOperand);
    };
    value_exit(u64::from(converted))
}

pub(crate) extern "C" fn i32_trunc_f64(value: u64, signed: u32) -> JitNativeExit {
    let value = f64::from_bits(value).trunc();
    let converted = if signed != 0 {
        if (i32::MIN as f64) <= value && value <= (i32::MAX as f64) {
            value as i32 as u32
        } else {
            return JitNativeExit::trap(VMResult::<()>::InvalidOperand);
        }
    } else if -1.0 < value && value <= (u32::MAX as f64) {
        value as u32
    } else {
        return JitNativeExit::trap(VMResult::<()>::InvalidOperand);
    };
    value_exit(u64::from(converted))
}

pub(crate) extern "C" fn i64_trunc_f32(value: u32, signed: u32, saturating: u32) -> JitNativeExit {
    let value = f32::from_bits(value);
    if saturating != 0 {
        let converted = if signed != 0 {
            value.trunc() as i64 as u64
        } else {
            value.trunc() as u64
        };
        return value_exit(converted);
    }
    let converted = if signed != 0 {
        if (i64::MIN as f32) <= value && value < (i64::MAX as f32) {
            value as i64 as u64
        } else {
            return JitNativeExit::trap(VMResult::<()>::InvalidOperand);
        }
    } else if -1.0 < value && value < (u64::MAX as f32) {
        value.trunc() as u64
    } else {
        return JitNativeExit::trap(VMResult::<()>::InvalidOperand);
    };
    value_exit(converted)
}

pub(crate) extern "C" fn i64_trunc_f64(value: u64, signed: u32, saturating: u32) -> JitNativeExit {
    let value = f64::from_bits(value);
    if saturating != 0 {
        let converted = if signed != 0 {
            value.trunc() as i64 as u64
        } else {
            value.trunc() as u64
        };
        return value_exit(converted);
    }
    let converted = if signed != 0 {
        if (i64::MIN as f64) <= value && value < (i64::MAX as f64) {
            value as i64 as u64
        } else {
            return JitNativeExit::trap(VMResult::<()>::InvalidOperand);
        }
    } else if -1.0 < value && value < (u64::MAX as f64) {
        value as u64
    } else {
        return JitNativeExit::trap(VMResult::<()>::InvalidOperand);
    };
    value_exit(converted)
}

pub(crate) extern "C" fn atomic_fence(ctx: *mut ExecuteContext<'_>, shared: u32) {
    let _ = (ctx, shared);
    #[cfg(feature = "threads")]
    {
        let ctx = unsafe { &mut *ctx };
        if shared != 0 {
            ctx.gc
                .shared_atomic_fence(unsafe { ctx.default_shared_memory_id_unchecked() });
        } else {
            ctx.gc
                .local_atomic_fence(unsafe { ctx.default_local_memory_id_unchecked() });
        }
    }
}

pub(crate) extern "C" fn ref_func(ctx: *mut ExecuteContext<'_>, funcidx: u32) -> u32 {
    let ctx = unsafe { &mut *ctx };
    ctx.instance().funcs.as_slice()[funcidx as usize].get()
}

pub(crate) extern "C" fn global_get4(ctx: *mut ExecuteContext<'_>, index: u32) -> u32 {
    global_get4_lane(ctx, index, 0)
}

pub(crate) extern "C" fn global_get4_lane(
    ctx: *mut ExecuteContext<'_>,
    index: u32,
    lane: u32,
) -> u32 {
    let ctx = unsafe { &mut *ctx };
    let addr = ctx.instance().globals.as_slice()[index as usize];
    let bytes = ctx.gc.get_global(addr);
    let start = lane as usize * 4;
    u32::from_le_bytes(
        bytes[start..start + 4]
            .try_into()
            .expect("global.get lane size mismatch"),
    )
}

pub(crate) extern "C" fn global_set4(ctx: *mut ExecuteContext<'_>, index: u32, value: u32) {
    global_set4_lane(ctx, index, 0, value);
}

pub(crate) extern "C" fn global_set4_lane(
    ctx: *mut ExecuteContext<'_>,
    index: u32,
    lane: u32,
    value: u32,
) {
    let ctx = unsafe { &mut *ctx };
    let addr = ctx.instance().globals.as_slice()[index as usize];
    let start = lane as usize * 4;
    ctx.gc
        .get_global_mut(addr)
        .get_mut(start..start + 4)
        .expect("global.set lane size mismatch")
        .copy_from_slice(&value.to_le_bytes());
}

pub(crate) extern "C" fn memory_fill(
    ctx: *mut ExecuteContext<'_>,
    ptr: u32,
    data: u32,
    len: u32,
) -> JitNativeExit {
    let ctx = unsafe { &mut *ctx };
    match ctx.gc.local_fill_memory(
        unsafe { ctx.default_local_memory_id_unchecked() },
        ptr,
        len,
        data,
    ) {
        VMResult::Success(()) => JitNativeExit::keep_going(),
        other => JitNativeExit::trap(other),
    }
}

pub(crate) extern "C" fn memory_copy(
    ctx: *mut ExecuteContext<'_>,
    dst: u32,
    src: u32,
    len: u32,
) -> JitNativeExit {
    let ctx = unsafe { &mut *ctx };
    match ctx.gc.local_copy_memory(
        unsafe { ctx.default_local_memory_id_unchecked() },
        dst,
        src,
        len,
    ) {
        VMResult::Success(()) => JitNativeExit::keep_going(),
        other => JitNativeExit::trap(other),
    }
}

pub(crate) extern "C" fn memory_size(ctx: *mut ExecuteContext<'_>, shared: u32) -> u32 {
    let ctx = unsafe { &mut *ctx };
    if shared != 0 {
        ctx.gc
            .shared_memory(unsafe { ctx.default_shared_memory_id_unchecked() })
            .page_size()
    } else {
        unsafe { ctx.default_local_memory_unchecked() }.page_size()
    }
}

pub(crate) extern "C" fn memory_grow(ctx: *mut ExecuteContext<'_>, delta: u32, shared: u32) -> u32 {
    let ctx = unsafe { &mut *ctx };
    let result = if shared != 0 {
        ctx.gc
            .shared_grow_memory(unsafe { ctx.default_shared_memory_id_unchecked() }, delta)
    } else {
        ctx.gc
            .local_grow_memory(unsafe { ctx.default_local_memory_id_unchecked() }, delta)
    };
    match result {
        VMResult::Success(value) => value as u32,
        _ => u32::MAX,
    }
}

pub(crate) extern "C" fn runtime_stack_op(
    ctx: *mut ExecuteContext<'_>,
    pc: *const Instr,
    kind: u32,
) -> JitNativeExit {
    let ctx = unsafe { &mut *ctx };
    if runtime_stub_must_wait_for_effect(kind) && ctx.effect.get_pending_count() != 0 {
        return JitNativeExit::fallback_pc(pc);
    }
    let result = match kind {
        RUNTIME_STUB_DATA_DROP => runtime_data_drop(ctx, pc),
        RUNTIME_STUB_ELEM_DROP => runtime_elem_drop(ctx, pc),
        RUNTIME_STUB_MEM_INIT_LOCAL => runtime_mem_init(ctx, pc, MemoryStubKind::DefaultLocal),
        RUNTIME_STUB_MEM_INIT_SHARED => runtime_mem_init(ctx, pc, MemoryStubKind::DefaultShared),
        RUNTIME_STUB_MEM_INIT_INDEXED_LOCAL => {
            runtime_mem_init(ctx, pc, MemoryStubKind::IndexedLocal)
        }
        RUNTIME_STUB_MEM_INIT_INDEXED_SHARED => {
            runtime_mem_init(ctx, pc, MemoryStubKind::IndexedShared)
        }
        RUNTIME_STUB_MEM_COPY_SHARED => runtime_mem_copy(ctx, pc, CopyStubKind::DefaultShared),
        RUNTIME_STUB_MEM_COPY_INDEXED_LOCAL_LOCAL => {
            runtime_mem_copy(ctx, pc, CopyStubKind::IndexedLocalLocal)
        }
        RUNTIME_STUB_MEM_COPY_INDEXED_LOCAL_SHARED => {
            runtime_mem_copy(ctx, pc, CopyStubKind::IndexedLocalShared)
        }
        RUNTIME_STUB_MEM_COPY_INDEXED_SHARED_LOCAL => {
            runtime_mem_copy(ctx, pc, CopyStubKind::IndexedSharedLocal)
        }
        RUNTIME_STUB_MEM_COPY_INDEXED_SHARED_SHARED => {
            runtime_mem_copy(ctx, pc, CopyStubKind::IndexedSharedShared)
        }
        RUNTIME_STUB_MEM_FILL_SHARED => runtime_mem_fill(ctx, pc, MemoryStubKind::DefaultShared),
        RUNTIME_STUB_MEM_FILL_INDEXED_LOCAL => {
            runtime_mem_fill(ctx, pc, MemoryStubKind::IndexedLocal)
        }
        RUNTIME_STUB_MEM_FILL_INDEXED_SHARED => {
            runtime_mem_fill(ctx, pc, MemoryStubKind::IndexedShared)
        }
        RUNTIME_STUB_MEM_SIZE_INDEXED_LOCAL => {
            runtime_mem_size(ctx, pc, MemoryStubKind::IndexedLocal)
        }
        RUNTIME_STUB_MEM_SIZE_INDEXED_SHARED => {
            runtime_mem_size(ctx, pc, MemoryStubKind::IndexedShared)
        }
        RUNTIME_STUB_MEM_GROW_INDEXED_LOCAL => {
            runtime_mem_grow(ctx, pc, MemoryStubKind::IndexedLocal)
        }
        RUNTIME_STUB_MEM_GROW_INDEXED_SHARED => {
            runtime_mem_grow(ctx, pc, MemoryStubKind::IndexedShared)
        }
        RUNTIME_STUB_TABLE_GET => runtime_table_get(ctx, pc),
        RUNTIME_STUB_TABLE_SET => runtime_table_set(ctx, pc),
        RUNTIME_STUB_TABLE_INIT => runtime_table_init(ctx, pc),
        RUNTIME_STUB_TABLE_COPY => runtime_table_copy(ctx, pc),
        RUNTIME_STUB_TABLE_GROW => runtime_table_grow(ctx, pc),
        RUNTIME_STUB_TABLE_SIZE => runtime_table_size(ctx, pc),
        RUNTIME_STUB_TABLE_FILL => runtime_table_fill(ctx, pc),
        RUNTIME_STUB_CALL_NUMERIC_TOKEN_STATE_TRANSITION => {
            return runtime_call_i32_numeric_token_state_transition(ctx, pc);
        }
        RUNTIME_STUB_CALL_CACHED_U16_LOW7_GUARD => {
            return runtime_call_cached_u16_low7_guard(ctx, pc);
        }
        RUNTIME_STUB_I8X16_EXTRACT_LANE_S => runtime_i8x16_extract_lane_s(ctx, pc),
        RUNTIME_STUB_V128_BITSELECT => runtime_v128_bitselect(ctx),
        RUNTIME_STUB_ATOMIC_NOTIFY_LOCAL => {
            runtime_atomic_notify(ctx, pc, MemoryStubKind::DefaultLocal)
        }
        RUNTIME_STUB_ATOMIC_NOTIFY_SHARED => {
            runtime_atomic_notify(ctx, pc, MemoryStubKind::DefaultShared)
        }
        RUNTIME_STUB_ATOMIC_NOTIFY_INDEXED_LOCAL => {
            runtime_atomic_notify(ctx, pc, MemoryStubKind::IndexedLocal)
        }
        RUNTIME_STUB_ATOMIC_NOTIFY_INDEXED_SHARED => {
            runtime_atomic_notify(ctx, pc, MemoryStubKind::IndexedShared)
        }
        RUNTIME_STUB_ATOMIC_WAIT32_LOCAL => {
            runtime_atomic_wait32(ctx, pc, MemoryStubKind::DefaultLocal)
        }
        RUNTIME_STUB_ATOMIC_WAIT32_SHARED => {
            return runtime_atomic_wait32_exit(ctx, pc, MemoryStubKind::DefaultShared);
        }
        RUNTIME_STUB_ATOMIC_WAIT32_INDEXED_LOCAL => {
            runtime_atomic_wait32(ctx, pc, MemoryStubKind::IndexedLocal)
        }
        RUNTIME_STUB_ATOMIC_WAIT32_INDEXED_SHARED => {
            return runtime_atomic_wait32_exit(ctx, pc, MemoryStubKind::IndexedShared);
        }
        RUNTIME_STUB_ATOMIC_WAIT64_LOCAL => {
            runtime_atomic_wait64(ctx, pc, MemoryStubKind::DefaultLocal)
        }
        RUNTIME_STUB_ATOMIC_WAIT64_SHARED => {
            return runtime_atomic_wait64_exit(ctx, pc, MemoryStubKind::DefaultShared);
        }
        RUNTIME_STUB_ATOMIC_WAIT64_INDEXED_LOCAL => {
            runtime_atomic_wait64(ctx, pc, MemoryStubKind::IndexedLocal)
        }
        RUNTIME_STUB_ATOMIC_WAIT64_INDEXED_SHARED => {
            return runtime_atomic_wait64_exit(ctx, pc, MemoryStubKind::IndexedShared);
        }
        _ => VMResult::Unimplemented,
    };
    match result {
        VMResult::Success(()) => JitNativeExit::keep_going(),
        other => JitNativeExit::trap(other),
    }
}

pub(crate) extern "C" fn runtime_continuation_op(
    ctx: *mut ExecuteContext<'_>,
    pc: *const Instr,
    kind: u32,
) -> JitNativeExit {
    let ctx = unsafe { &mut *ctx };
    let tail_code = unsafe { pc.add(1) };
    let result = match kind {
        RUNTIME_CONT_I32_GUARDED_LOAD8_UPDATE_BR_IF_FALSE_BR_TABLE => unsafe {
            vm::op_i32_guarded_load8_u_local_base_set4_local_get4_set4_local_get4_br_if_false_local_get4_br_table(
                tail_code, ctx,
            )
        },
        RUNTIME_CONT_I32_GUARDED_LOAD8_UPDATE_BR_IF_TAKEN_CONST_CMP_BR_TABLE => unsafe {
            vm::op_i32_guarded_load8_u_local_base_set4_local_get4_set4_local_get4_br_if_taken_const_compare_br_table(
                tail_code, ctx,
            )
        },
        RUNTIME_CONT_I32_GUARDED_LOAD8_UPDATE_BR_IF_TAKEN_BR_TABLE => unsafe {
            vm::op_i32_guarded_load8_u_local_base_set4_local_get4_set4_local_get4_br_if_taken_local_get4_br_table(
                tail_code, ctx,
            )
        },
        RUNTIME_CONT_I32_INC_LOAD8_UPDATE_BR_IF => unsafe {
            vm::op_i32_inc_local_base_i32_load8_u_local_base_set4_local_get4_set4_local_get4_br_if(
                tail_code, ctx,
            )
        },
        RUNTIME_CONT_I32_LOAD16_S_DOT4_LOOP => unsafe {
            vm::op_i32_load16_s_dot4_local_base_loop(tail_code, ctx)
        },
        RUNTIME_CONT_I32_LOAD16_S_MUL_ADD_DELTA_LOOP => unsafe {
            vm::op_i32_load16_s_mul_add_local_base_delta_loop(tail_code, ctx)
        },
        RUNTIME_CONT_I32_LOAD16_S_MUL_ADD_LOOP => unsafe {
            vm::op_i32_load16_s_mul_add_local_base_loop(tail_code, ctx)
        },
        RUNTIME_CONT_I32_LOAD16_U_BITMIX_DELTA_LOOP => unsafe {
            vm::op_i32_load16_u_bitmix_acc_local_base_delta_loop(tail_code, ctx)
        },
        RUNTIME_CONT_I32_LOAD8_UPDATE_BR_IF => unsafe {
            vm::op_i32_load8_u_local_base_set4_local_get4_set4_local_get4_br_if(tail_code, ctx)
        },
        RUNTIME_CONT_I32_LOAD8_UPDATE_BR_IF_FALLTHROUGH_LOCAL_GET4 => unsafe {
            vm::op_i32_load8_u_local_base_set4_local_get4_set4_local_get4_br_if_fallthrough_local_get4(
                tail_code, ctx,
            )
        },
        RUNTIME_CONT_I32_LOAD8_UPDATE_BR_IF_TAKEN_LOCAL_GET4 => unsafe {
            vm::op_i32_load8_u_local_base_set4_local_get4_set4_local_get4_br_if_taken_local_get4(
                tail_code, ctx,
            )
        },
        RUNTIME_CONT_I32_LOAD_MASKED_COMPARE_BR_IF => unsafe {
            vm::op_i32_load_local_base_set4_i32_load8_u_local_base_local_masked_compare_br_if(
                tail_code, ctx,
            )
        },
        RUNTIME_CONT_I32_MATRIX_I16_CRC_SUMMARY => unsafe {
            vm::op_i32_matrix_i16_crc_summary(tail_code, ctx)
        },
        RUNTIME_CONT_I32_SUM_CLIP_LOOP => unsafe {
            vm::op_i32_sum_clip_local_base_loop(tail_code, ctx)
        },
        RUNTIME_CONT_START_FUNCTION_CALL => unsafe {
            vm::special_start_function_call(tail_code, ctx)
        },
        RUNTIME_CONT_START_JIT_FUNCTION_CALL => unsafe {
            vm::special_start_jit_function_call(tail_code, ctx)
        },
        RUNTIME_CONT_I32_LOAD8_U_LOCAL_BASE_TEE4_LOCAL_GET4 => unsafe {
            vm::op_i32_load8_u_local_base_tee4_local_get4(tail_code, ctx)
        },
        RUNTIME_CONT_I32_LOAD8_S_LOCAL_BASE_TEE4_LOCAL_GET4 => unsafe {
            vm::op_i32_load8_s_local_base_tee4_local_get4(tail_code, ctx)
        },
        RUNTIME_CONT_I32_LOAD16_U_LOCAL_BASE_TEE4_LOCAL_GET4 => unsafe {
            vm::op_i32_load16_u_local_base_tee4_local_get4(tail_code, ctx)
        },
        RUNTIME_CONT_I32_LOAD16_S_LOCAL_BASE_TEE4_LOCAL_GET4 => unsafe {
            vm::op_i32_load16_s_local_base_tee4_local_get4(tail_code, ctx)
        },
        RUNTIME_CONT_I32_LOAD_LOCAL_BASE_SET4_I32_LOAD_LOCAL_BASE_LOCAL_EQ_BR_IF => unsafe {
            vm::op_i32_load_local_base_set4_i32_load_local_base_local_eq_br_if(tail_code, ctx)
        },
        RUNTIME_CONT_I32_LOAD_LOCAL_BASE_SET4_I32_LOAD8_S_LOCAL_BASE_LOCAL_EQ_BR_IF => unsafe {
            vm::op_i32_load_local_base_set4_i32_load8_s_local_base_local_eq_br_if(tail_code, ctx)
        },
        RUNTIME_CONT_I32_LOAD_LOCAL_BASE_SET4_I32_LOAD16_U_LOCAL_BASE_LOCAL_EQ_BR_IF => unsafe {
            vm::op_i32_load_local_base_set4_i32_load16_u_local_base_local_eq_br_if(tail_code, ctx)
        },
        RUNTIME_CONT_I32_LOAD_LOCAL_BASE_SET4_I32_LOAD16_S_LOCAL_BASE_LOCAL_EQ_BR_IF => unsafe {
            vm::op_i32_load_local_base_set4_i32_load16_s_local_base_local_eq_br_if(tail_code, ctx)
        },
        RUNTIME_CONT_I32_LOAD16_U_LOCAL_BASE_LOCAL_GET4_I32_LOAD16_U_LOCAL_GET4 => unsafe {
            vm::op_i32_load16_u_local_base_local_get4_i32_load16_u_local_get4(tail_code, ctx)
        },
        RUNTIME_CONT_I32_LOAD16_S_LOCAL_BASE_LOCAL_GET4_I32_LOAD16_S_LOCAL_GET4 => unsafe {
            vm::op_i32_load16_s_local_base_local_get4_i32_load16_s_local_get4(tail_code, ctx)
        },
        RUNTIME_CONT_LOCAL_GET4_I32_LOAD16_U_LOCAL_BASE_LOCAL_GET4_I32_LOAD16_U => unsafe {
            vm::op_local_get4_i32_load16_u_local_base_local_get4_i32_load16_u(tail_code, ctx)
        },
        RUNTIME_CONT_LOCAL_GET4_I32_LOAD16_S_LOCAL_BASE_LOCAL_GET4_I32_LOAD16_S => unsafe {
            vm::op_local_get4_i32_load16_s_local_base_local_get4_i32_load16_s(tail_code, ctx)
        },
        RUNTIME_CONT_F32_STORE_LOCAL_BASE => unsafe { vm::op_f32_store_local_base(tail_code, ctx) },
        RUNTIME_CONT_F64_STORE_LOCAL_SCALED_INDEX => unsafe {
            vm::op_f64_store_local_scaled_index(tail_code, ctx)
        },
        RUNTIME_CONT_I32_LOAD_INDEXED_LOCAL_BASE => unsafe {
            vm::op_i32_load_indexed_local_base(tail_code, ctx)
        },
        RUNTIME_CONT_I32_LOAD_INDEXED_SHARED_LOCAL_BASE => unsafe {
            vm::op_i32_load_indexed_shared_local_base(tail_code, ctx)
        },
        RUNTIME_CONT_I32_LOAD_LOCAL_BASE_SET4_I32_LOAD_LOCAL_BASE_LOCAL_GET4 => unsafe {
            vm::op_i32_load_local_base_set4_i32_load_local_base_local_get4(tail_code, ctx)
        },
        RUNTIME_CONT_I32_LOAD_LOCAL_SCALED_INDEX => unsafe {
            vm::op_i32_load_local_scaled_index(tail_code, ctx)
        },
        RUNTIME_CONT_I32_LOAD_SHARED_LOCAL_BASE => unsafe {
            vm::op_i32_load_shared_local_base(tail_code, ctx)
        },
        RUNTIME_CONT_I32_STORE_INDEXED_LOCAL => unsafe {
            vm::op_i32_store_indexed_local(tail_code, ctx)
        },
        RUNTIME_CONT_I32_STORE_INDEXED_LOCAL_BASE => unsafe {
            vm::op_i32_store_indexed_local_base(tail_code, ctx)
        },
        RUNTIME_CONT_I32_STORE_INDEXED_LOCAL_SCALED_INDEX => unsafe {
            vm::op_i32_store_indexed_local_scaled_index(tail_code, ctx)
        },
        RUNTIME_CONT_I64_LOAD_LOCAL_SCALED_INDEX => unsafe {
            vm::op_i64_load_local_scaled_index(tail_code, ctx)
        },
        #[cfg(feature = "simd")]
        RUNTIME_CONT_SIMD_F32X4_REPLACE_LANE => unsafe {
            vm::simd::f32x4_replace_lane(tail_code, ctx)
        },
        #[cfg(feature = "simd")]
        RUNTIME_CONT_SIMD_F64X2_REPLACE_LANE => unsafe {
            vm::simd::f64x2_replace_lane(tail_code, ctx)
        },
        #[cfg(feature = "simd")]
        RUNTIME_CONT_SIMD_I16X8_REPLACE_LANE => unsafe {
            vm::simd::i16x8_replace_lane(tail_code, ctx)
        },
        #[cfg(feature = "simd")]
        RUNTIME_CONT_SIMD_I16X8_SHL => unsafe { vm::simd::i16x8_shl(tail_code, ctx) },
        #[cfg(feature = "simd")]
        RUNTIME_CONT_SIMD_I16X8_SHR => unsafe { vm::simd::i16x8_shr(tail_code, ctx) },
        #[cfg(feature = "simd")]
        RUNTIME_CONT_SIMD_I32X4_REPLACE_LANE => unsafe {
            vm::simd::i32x4_replace_lane(tail_code, ctx)
        },
        #[cfg(feature = "simd")]
        RUNTIME_CONT_SIMD_I32X4_SHL => unsafe { vm::simd::i32x4_shl(tail_code, ctx) },
        #[cfg(feature = "simd")]
        RUNTIME_CONT_SIMD_I32X4_SHR => unsafe { vm::simd::i32x4_shr(tail_code, ctx) },
        #[cfg(feature = "simd")]
        RUNTIME_CONT_SIMD_I64X2_REPLACE_LANE => unsafe {
            vm::simd::i64x2_replace_lane(tail_code, ctx)
        },
        #[cfg(feature = "simd")]
        RUNTIME_CONT_SIMD_I64X2_SHL => unsafe { vm::simd::i64x2_shl(tail_code, ctx) },
        #[cfg(feature = "simd")]
        RUNTIME_CONT_SIMD_I64X2_SHR => unsafe { vm::simd::i64x2_shr(tail_code, ctx) },
        #[cfg(feature = "simd")]
        RUNTIME_CONT_SIMD_I8X16_REPLACE_LANE => unsafe {
            vm::simd::i8x16_replace_lane(tail_code, ctx)
        },
        #[cfg(feature = "simd")]
        RUNTIME_CONT_SIMD_I8X16_SHL => unsafe { vm::simd::i8x16_shl(tail_code, ctx) },
        #[cfg(feature = "simd")]
        RUNTIME_CONT_SIMD_I8X16_SHR => unsafe { vm::simd::i8x16_shr(tail_code, ctx) },
        #[cfg(feature = "simd")]
        RUNTIME_CONT_SIMD_I8X16_SHUFFLE => unsafe { vm::simd::i8x16_shuffle(tail_code, ctx) },
        #[cfg(feature = "simd")]
        RUNTIME_CONT_SIMD_I8X16_SWIZZLE => unsafe { vm::simd::i8x16_swizzle(tail_code, ctx) },
        #[cfg(feature = "simd")]
        RUNTIME_CONT_SIMD_V128_LOAD => unsafe { vm::simd::op_v128_load(tail_code, ctx) },
        #[cfg(feature = "simd")]
        RUNTIME_CONT_SIMD_V128_LOAD_INDEXED_LOCAL => unsafe {
            vm::simd::op_v128_load_indexed_local(tail_code, ctx)
        },
        #[cfg(feature = "simd")]
        RUNTIME_CONT_SIMD_V128_LOAD_INDEXED_SHARED => unsafe {
            vm::simd::op_v128_load_indexed_shared(tail_code, ctx)
        },
        #[cfg(feature = "simd")]
        RUNTIME_CONT_SIMD_V128_LOAD_SHARED => unsafe {
            vm::simd::op_v128_load_shared(tail_code, ctx)
        },
        _ => VMResult::Unimplemented,
    };
    match result {
        VMResult::Success(()) if ctx.cont.is_null() => JitNativeExit::done(),
        VMResult::Success(()) => JitNativeExit::pending(),
        other => JitNativeExit::trap(other),
    }
}

pub(crate) extern "C" fn i32_store_local_base_from_vm_stack(
    ctx: *mut ExecuteContext<'_>,
    pc: *const Instr,
    width: u32,
) -> JitNativeExit {
    let ctx = unsafe { &mut *ctx };
    let base_local = unsafe { (*pc.add(1)).operand.local_addr as usize };
    let delta = unsafe { (*pc.add(2)).operand.i32 as u32 };
    let memarg = unsafe { (*pc.add(3)).operand.memarg };
    let local_base = ctx.local_base_ptr as *const u8;
    let offset =
        unsafe { ctx.stack.local_u32_from_base(local_base, base_local) }.wrapping_add(delta);
    let start = match vm::compute_memory_offset(memarg, offset) {
        VMResult::Success(start) => start,
        other => return JitNativeExit::trap(other),
    };
    let value = ctx.stack.pop_u32_fast();
    let result = match width {
        1 => unsafe { ctx.default_local_memory_mut_unchecked() }.write_bytes(start, &[value as u8]),
        2 => unsafe { ctx.default_local_memory_mut_unchecked() }
            .write_bytes(start, &(value as u16).to_le_bytes()),
        4 => unsafe { ctx.default_local_memory_mut_unchecked() }.write_u32_at(start, value),
        _ => VMResult::InvalidOperand,
    };
    match result {
        VMResult::Success(()) => JitNativeExit::keep_going(),
        other => JitNativeExit::trap(other),
    }
}

fn runtime_stub_must_wait_for_effect(kind: u32) -> bool {
    matches!(
        kind,
        RUNTIME_STUB_DATA_DROP
            | RUNTIME_STUB_MEM_COPY_SHARED
            | RUNTIME_STUB_MEM_COPY_INDEXED_LOCAL_LOCAL
            | RUNTIME_STUB_MEM_COPY_INDEXED_LOCAL_SHARED
            | RUNTIME_STUB_MEM_COPY_INDEXED_SHARED_LOCAL
            | RUNTIME_STUB_MEM_COPY_INDEXED_SHARED_SHARED
    )
}

unsafe fn decode_direct_call_recipe_for_stub(
    pc: *const Instr,
    ctx: &mut ExecuteContext<'_>,
) -> CallDispatchCache {
    let recipe_ref = unsafe { (*pc.add(1)).operand.call_recipe_ref };
    if let Some(recipe_slot) = recipe_ref.resolved_recipe_slot() {
        if let Some(recipe) = ctx.gc.call_recipe(recipe_slot) {
            return recipe;
        }
    }
    let funcaddr = ctx.instance().funcs.as_slice()[recipe_ref.funcidx as usize];
    ctx.gc.ensure_call_recipe_for_func(funcaddr)
}

unsafe fn call_target_starts_with(recipe: CallDispatchCache, op: crate::common::Op) -> bool {
    let CallDispatchTarget::Wasm { .. } = recipe.target else {
        return false;
    };
    if recipe.param_size != 8 || recipe.return_arity != 1 || recipe.frame.code_base.is_null() {
        return false;
    }
    std::ptr::fn_addr_eq(unsafe { (*recipe.frame.code_base).op }, op)
}

fn runtime_call_i32_numeric_token_state_transition(
    ctx: &mut ExecuteContext<'_>,
    pc: *const Instr,
) -> JitNativeExit {
    if ctx.effect.get_pending_count() != 0 {
        return JitNativeExit::fallback_pc(pc);
    }
    let recipe = unsafe { decode_direct_call_recipe_for_stub(pc, ctx) };
    if !unsafe {
        call_target_starts_with(
            recipe,
            vm::op_i32_numeric_token_state_transition as crate::common::Op,
        )
    } {
        return JitNativeExit::fallback_pc(pc);
    }

    let counts = ctx.stack.pop_u32_fast();
    let instr_ref = ctx.stack.pop_u32_fast();
    match unsafe { vm::i32_numeric_token_state_transition_value(instr_ref, counts, ctx) } {
        VMResult::Success(state) => match ctx.stack.push_u32_fast(state) {
            VMResult::Success(()) => JitNativeExit::keep_going(),
            other => JitNativeExit::trap(other),
        },
        other => JitNativeExit::trap(other),
    }
}

fn runtime_call_cached_u16_low7_guard(
    ctx: &mut ExecuteContext<'_>,
    pc: *const Instr,
) -> JitNativeExit {
    if ctx.effect.get_pending_count() != 0 {
        return JitNativeExit::fallback_pc(pc);
    }
    let recipe = unsafe { decode_direct_call_recipe_for_stub(pc, ctx) };
    if !unsafe {
        call_target_starts_with(
            recipe,
            vm::op_i32_load16_u_local_base_tee4 as crate::common::Op,
        )
    } {
        return JitNativeExit::fallback_pc(pc);
    }

    let data_ptr = ctx.stack.peek_u32_fast_from_top(4);
    let cached =
        match unsafe { ctx.default_local_memory_unchecked() }.read_u16_at(data_ptr as usize) {
            VMResult::Success(value) => value,
            other => return JitNativeExit::trap(other),
        };
    if cached & 0x80 == 0 {
        return JitNativeExit::fallback_pc(pc);
    }

    let _context = ctx.stack.pop_u32_fast();
    let _data = ctx.stack.pop_u32_fast();
    match ctx.stack.push_u32_fast(u32::from(cached & 0x7f)) {
        VMResult::Success(()) => JitNativeExit::keep_going(),
        other => JitNativeExit::trap(other),
    }
}

fn runtime_i8x16_extract_lane_s(ctx: &mut ExecuteContext<'_>, pc: *const Instr) -> VMResult<()> {
    let lane = unsafe { (*pc.add(1)).operand.u32 as usize };
    let bytes = ctx.stack.pop_u128().to_le_bytes();
    ctx.stack.push_i32(bytes[lane] as i8 as i32)
}

fn runtime_v128_bitselect(ctx: &mut ExecuteContext<'_>) -> VMResult<()> {
    let mask = ctx.stack.pop_u128();
    let b = ctx.stack.pop_u128();
    let a = ctx.stack.pop_u128();
    ctx.stack.push_u128((a & mask) | (b & !mask))
}

fn runtime_atomic_start(ctx: &mut ExecuteContext<'_>, pc: *const Instr) -> VMResult<usize> {
    let memarg = unsafe { (*pc.add(1)).operand.memarg };
    let offset = ctx.stack.pop_u32();
    vm::compute_memory_offset(memarg, offset)
}

fn runtime_atomic_start_indexed(
    ctx: &mut ExecuteContext<'_>,
    pc: *const Instr,
) -> VMResult<(usize, u32)> {
    let memarg = unsafe { (*pc.add(1)).operand.memarg };
    let memidx = unsafe { (*pc.add(2)).operand.u32 };
    let offset = ctx.stack.pop_u32();
    match vm::compute_memory_offset(memarg, offset) {
        VMResult::Success(start) => VMResult::Success((start, memidx)),
        VMResult::Unreachable => VMResult::Unreachable,
        VMResult::StackOverflow => VMResult::StackOverflow,
        VMResult::MemoryIndexOutOfRange => VMResult::MemoryIndexOutOfRange,
        VMResult::TableIndexOutOfRange => VMResult::TableIndexOutOfRange,
        VMResult::CallIndirectInvalidType => VMResult::CallIndirectInvalidType,
        VMResult::TableUninitialized => VMResult::TableUninitialized,
        VMResult::Unlinkable => VMResult::Unlinkable,
        VMResult::InvalidOperand => VMResult::InvalidOperand,
        VMResult::UnalignedAtomic => VMResult::UnalignedAtomic,
        VMResult::Unimplemented => VMResult::Unimplemented,
    }
}

fn runtime_atomic_notify(
    ctx: &mut ExecuteContext<'_>,
    pc: *const Instr,
    kind: MemoryStubKind,
) -> VMResult<()> {
    let count = ctx.stack.pop_u32();
    let (start, memidx) = match kind {
        MemoryStubKind::IndexedLocal | MemoryStubKind::IndexedShared => {
            match runtime_atomic_start_indexed(ctx, pc) {
                VMResult::Success(value) => value,
                other => return unit_result(other),
            }
        }
        MemoryStubKind::DefaultLocal | MemoryStubKind::DefaultShared => {
            match runtime_atomic_start(ctx, pc) {
                VMResult::Success(start) => (start, 0),
                other => return unit_result(other),
            }
        }
    };
    let woken = match kind {
        MemoryStubKind::DefaultLocal | MemoryStubKind::IndexedLocal => 0,
        MemoryStubKind::DefaultShared => match ctx
            .gc
            .shared_memory(unsafe { ctx.default_shared_memory_id_unchecked() })
            .notify_waiters(start, count)
        {
            VMResult::Success(value) => value,
            other => return unit_result(other),
        },
        MemoryStubKind::IndexedShared => match ctx
            .gc
            .shared_memory(unsafe { ctx.shared_memory_id_at_unchecked(memidx) })
            .notify_waiters(start, count)
        {
            VMResult::Success(value) => value,
            other => return unit_result(other),
        },
    };
    ctx.stack.push_u32(woken)
}

fn runtime_atomic_wait32(
    ctx: &mut ExecuteContext<'_>,
    pc: *const Instr,
    kind: MemoryStubKind,
) -> VMResult<()> {
    let _timeout_ns = ctx.stack.pop_i64();
    let _expected = ctx.stack.pop_u32();
    match kind {
        MemoryStubKind::DefaultLocal => match runtime_atomic_start(ctx, pc) {
            VMResult::Success(_) => {}
            other => return unit_result(other),
        },
        MemoryStubKind::IndexedLocal => match runtime_atomic_start_indexed(ctx, pc) {
            VMResult::Success(_) => {}
            other => return unit_result(other),
        },
        MemoryStubKind::DefaultShared | MemoryStubKind::IndexedShared => {
            return VMResult::Unimplemented
        }
    }
    VMResult::InvalidOperand
}

fn runtime_atomic_wait64(
    ctx: &mut ExecuteContext<'_>,
    pc: *const Instr,
    kind: MemoryStubKind,
) -> VMResult<()> {
    let _timeout_ns = ctx.stack.pop_i64();
    let _expected = ctx.stack.pop_u64();
    match kind {
        MemoryStubKind::DefaultLocal => match runtime_atomic_start(ctx, pc) {
            VMResult::Success(_) => {}
            other => return unit_result(other),
        },
        MemoryStubKind::IndexedLocal => match runtime_atomic_start_indexed(ctx, pc) {
            VMResult::Success(_) => {}
            other => return unit_result(other),
        },
        MemoryStubKind::DefaultShared | MemoryStubKind::IndexedShared => {
            return VMResult::Unimplemented
        }
    }
    VMResult::InvalidOperand
}

fn push_wait_effect(
    ctx: &mut ExecuteContext<'_>,
    shared: std::sync::Arc<SharedMemoryObject>,
    wait: SharedWaitRegistration,
    timeout_ns: i64,
    resume_pc: *const Instr,
) {
    let task_id = ctx.task_id;
    let fp = StablePc::from_raw_in_frame(ctx.gc, ctx.stack, ctx.local_reference, resume_pc).raw();
    ctx.effect
        .push_pending(PendingOp::MemoryWait(MemoryWaitPending {
            task_id,
            shared,
            wait,
            timeout_ns,
            fp,
        }));
}

fn runtime_atomic_wait32_exit(
    ctx: &mut ExecuteContext<'_>,
    pc: *const Instr,
    kind: MemoryStubKind,
) -> JitNativeExit {
    let timeout_ns = ctx.stack.pop_i64();
    let expected = ctx.stack.pop_u32();
    let (start, memidx, resume_pc) = match kind {
        MemoryStubKind::DefaultShared => match runtime_atomic_start(ctx, pc) {
            VMResult::Success(start) => (start, 0, unsafe { pc.add(2) }),
            other => return JitNativeExit::trap(unit_result(other)),
        },
        MemoryStubKind::IndexedShared => match runtime_atomic_start_indexed(ctx, pc) {
            VMResult::Success((start, memidx)) => (start, memidx, unsafe { pc.add(3) }),
            other => return JitNativeExit::trap(unit_result(other)),
        },
        MemoryStubKind::DefaultLocal | MemoryStubKind::IndexedLocal => {
            return JitNativeExit::trap(VMResult::<()>::Unimplemented)
        }
    };
    let shared = ctx.gc.shared_memory(unsafe {
        match kind {
            MemoryStubKind::DefaultShared => ctx.default_shared_memory_id_unchecked(),
            MemoryStubKind::IndexedShared => ctx.shared_memory_id_at_unchecked(memidx),
            MemoryStubKind::DefaultLocal | MemoryStubKind::IndexedLocal => unreachable!(),
        }
    });
    match shared.register_wait32(start, expected) {
        VMResult::Success(AtomicWaitResult::NotEqual) => match ctx.stack.push_i32(1) {
            VMResult::Success(()) => JitNativeExit::keep_going(),
            other => JitNativeExit::trap(other),
        },
        VMResult::Success(AtomicWaitResult::Pending(wait)) => {
            let shared = unsafe {
                match kind {
                    MemoryStubKind::DefaultShared => ctx
                        .gc
                        .clone_shared_memory(ctx.default_shared_memory_id_unchecked()),
                    MemoryStubKind::IndexedShared => ctx
                        .gc
                        .clone_shared_memory(ctx.shared_memory_id_at_unchecked(memidx)),
                    MemoryStubKind::DefaultLocal | MemoryStubKind::IndexedLocal => unreachable!(),
                }
            };
            push_wait_effect(ctx, shared, wait, timeout_ns, resume_pc);
            ctx.cont = resume_pc;
            JitNativeExit::pending()
        }
        other => JitNativeExit::trap(unit_result(other)),
    }
}

fn runtime_atomic_wait64_exit(
    ctx: &mut ExecuteContext<'_>,
    pc: *const Instr,
    kind: MemoryStubKind,
) -> JitNativeExit {
    let timeout_ns = ctx.stack.pop_i64();
    let expected = ctx.stack.pop_u64();
    let (start, memidx, resume_pc) = match kind {
        MemoryStubKind::DefaultShared => match runtime_atomic_start(ctx, pc) {
            VMResult::Success(start) => (start, 0, unsafe { pc.add(2) }),
            other => return JitNativeExit::trap(unit_result(other)),
        },
        MemoryStubKind::IndexedShared => match runtime_atomic_start_indexed(ctx, pc) {
            VMResult::Success((start, memidx)) => (start, memidx, unsafe { pc.add(3) }),
            other => return JitNativeExit::trap(unit_result(other)),
        },
        MemoryStubKind::DefaultLocal | MemoryStubKind::IndexedLocal => {
            return JitNativeExit::trap(VMResult::<()>::Unimplemented)
        }
    };
    let shared = ctx.gc.shared_memory(unsafe {
        match kind {
            MemoryStubKind::DefaultShared => ctx.default_shared_memory_id_unchecked(),
            MemoryStubKind::IndexedShared => ctx.shared_memory_id_at_unchecked(memidx),
            MemoryStubKind::DefaultLocal | MemoryStubKind::IndexedLocal => unreachable!(),
        }
    });
    match shared.register_wait64(start, expected) {
        VMResult::Success(AtomicWaitResult::NotEqual) => match ctx.stack.push_i32(1) {
            VMResult::Success(()) => JitNativeExit::keep_going(),
            other => JitNativeExit::trap(other),
        },
        VMResult::Success(AtomicWaitResult::Pending(wait)) => {
            let shared = unsafe {
                match kind {
                    MemoryStubKind::DefaultShared => ctx
                        .gc
                        .clone_shared_memory(ctx.default_shared_memory_id_unchecked()),
                    MemoryStubKind::IndexedShared => ctx
                        .gc
                        .clone_shared_memory(ctx.shared_memory_id_at_unchecked(memidx)),
                    MemoryStubKind::DefaultLocal | MemoryStubKind::IndexedLocal => unreachable!(),
                }
            };
            push_wait_effect(ctx, shared, wait, timeout_ns, resume_pc);
            ctx.cont = resume_pc;
            JitNativeExit::pending()
        }
        other => JitNativeExit::trap(unit_result(other)),
    }
}

fn unit_result<T>(result: VMResult<T>) -> VMResult<()> {
    match result {
        VMResult::Success(_) => VMResult::Success(()),
        VMResult::Unreachable => VMResult::Unreachable,
        VMResult::StackOverflow => VMResult::StackOverflow,
        VMResult::MemoryIndexOutOfRange => VMResult::MemoryIndexOutOfRange,
        VMResult::TableIndexOutOfRange => VMResult::TableIndexOutOfRange,
        VMResult::CallIndirectInvalidType => VMResult::CallIndirectInvalidType,
        VMResult::TableUninitialized => VMResult::TableUninitialized,
        VMResult::Unlinkable => VMResult::Unlinkable,
        VMResult::InvalidOperand => VMResult::InvalidOperand,
        VMResult::UnalignedAtomic => VMResult::UnalignedAtomic,
        VMResult::Unimplemented => VMResult::Unimplemented,
    }
}

#[derive(Clone, Copy)]
enum MemoryStubKind {
    DefaultLocal,
    DefaultShared,
    IndexedLocal,
    IndexedShared,
}

#[derive(Clone, Copy)]
enum CopyStubKind {
    DefaultShared,
    IndexedLocalLocal,
    IndexedLocalShared,
    IndexedSharedLocal,
    IndexedSharedShared,
}

fn runtime_data_drop(ctx: &mut ExecuteContext<'_>, pc: *const Instr) -> VMResult<()> {
    let idx = unsafe { (*pc.add(1)).operand.u32 };
    let _ = ctx
        .store
        .lock_segments()
        .data
        .remove(&(ctx.instance_id(), idx));
    VMResult::Success(())
}

fn runtime_elem_drop(ctx: &mut ExecuteContext<'_>, pc: *const Instr) -> VMResult<()> {
    let elem_idx = unsafe { (*pc.add(1)).operand.u32 };
    let _ = ctx
        .store
        .lock_segments()
        .elems
        .remove(&(ctx.instance_id(), elem_idx));
    VMResult::Success(())
}

fn mem_init_bytes(
    ctx: &mut ExecuteContext<'_>,
    idx: u32,
    src: u32,
    len: u32,
) -> VMResult<Option<Vec<u8>>> {
    let copied = {
        let segments = ctx.store.lock_segments();
        let data = segments.data.get(&(ctx.instance_id(), idx));
        if data.is_none() && len == 0 && src == 0 {
            None
        } else {
            let Some(data) = data else {
                return VMResult::MemoryIndexOutOfRange;
            };
            let Some(src_last) = src.checked_add(len) else {
                return VMResult::MemoryIndexOutOfRange;
            };
            let Some(data) = data.init.get(src as usize..src_last as usize) else {
                return VMResult::MemoryIndexOutOfRange;
            };
            Some(data.to_vec())
        }
    };
    VMResult::Success(copied)
}

fn runtime_mem_init(
    ctx: &mut ExecuteContext<'_>,
    pc: *const Instr,
    kind: MemoryStubKind,
) -> VMResult<()> {
    let idx = unsafe { (*pc.add(1)).operand.u32 };
    let memidx = match kind {
        MemoryStubKind::IndexedLocal | MemoryStubKind::IndexedShared => unsafe {
            (*pc.add(2)).operand.u32
        },
        MemoryStubKind::DefaultLocal | MemoryStubKind::DefaultShared => 0,
    };
    let len = ctx.stack.pop_u32();
    let src = ctx.stack.pop_u32();
    let dst = ctx.stack.pop_u32();
    let copied = match mem_init_bytes(ctx, idx, src, len) {
        VMResult::Success(copied) => copied,
        other => return unit_result(other),
    };
    match kind {
        MemoryStubKind::DefaultLocal => ctx.gc.local_write_bytes(
            unsafe { ctx.default_local_memory_id_unchecked() },
            dst as usize,
            copied.as_deref().unwrap_or(&[]),
        ),
        MemoryStubKind::DefaultShared => ctx.gc.shared_write_bytes(
            unsafe { ctx.default_shared_memory_id_unchecked() },
            dst as usize,
            copied.as_deref().unwrap_or(&[]),
        ),
        MemoryStubKind::IndexedLocal => ctx.gc.local_write_bytes(
            unsafe { ctx.local_memory_id_at_unchecked(memidx) },
            dst as usize,
            copied.as_deref().unwrap_or(&[]),
        ),
        MemoryStubKind::IndexedShared => ctx.gc.shared_write_bytes(
            unsafe { ctx.shared_memory_id_at_unchecked(memidx) },
            dst as usize,
            copied.as_deref().unwrap_or(&[]),
        ),
    }
}

fn runtime_mem_copy(
    ctx: &mut ExecuteContext<'_>,
    pc: *const Instr,
    kind: CopyStubKind,
) -> VMResult<()> {
    let len = ctx.stack.pop_u32();
    let src = ctx.stack.pop_u32();
    let dst = ctx.stack.pop_u32();
    match kind {
        CopyStubKind::DefaultShared => ctx.gc.shared_copy_memory(
            unsafe { ctx.default_shared_memory_id_unchecked() },
            dst,
            src,
            len,
        ),
        CopyStubKind::IndexedLocalLocal => ctx.gc.copy_memory_local_to_local(
            unsafe { ctx.local_memory_id_at_unchecked((*pc.add(1)).operand.u32) },
            unsafe { ctx.local_memory_id_at_unchecked((*pc.add(2)).operand.u32) },
            dst,
            src,
            len,
        ),
        CopyStubKind::IndexedLocalShared => ctx.gc.copy_memory_shared_to_local(
            unsafe { ctx.local_memory_id_at_unchecked((*pc.add(1)).operand.u32) },
            unsafe { ctx.shared_memory_id_at_unchecked((*pc.add(2)).operand.u32) },
            dst,
            src,
            len,
        ),
        CopyStubKind::IndexedSharedLocal => ctx.gc.copy_memory_local_to_shared(
            unsafe { ctx.shared_memory_id_at_unchecked((*pc.add(1)).operand.u32) },
            unsafe { ctx.local_memory_id_at_unchecked((*pc.add(2)).operand.u32) },
            dst,
            src,
            len,
        ),
        CopyStubKind::IndexedSharedShared => ctx.gc.copy_memory_shared_to_shared(
            unsafe { ctx.shared_memory_id_at_unchecked((*pc.add(1)).operand.u32) },
            unsafe { ctx.shared_memory_id_at_unchecked((*pc.add(2)).operand.u32) },
            dst,
            src,
            len,
        ),
    }
}

fn runtime_mem_fill(
    ctx: &mut ExecuteContext<'_>,
    pc: *const Instr,
    kind: MemoryStubKind,
) -> VMResult<()> {
    let len = ctx.stack.pop_u32();
    let data = ctx.stack.pop_u32();
    let ptr = ctx.stack.pop_u32();
    match kind {
        MemoryStubKind::DefaultShared => ctx.gc.shared_fill_memory(
            unsafe { ctx.default_shared_memory_id_unchecked() },
            ptr,
            len,
            data,
        ),
        MemoryStubKind::IndexedLocal => ctx.gc.local_fill_memory(
            unsafe { ctx.local_memory_id_at_unchecked((*pc.add(1)).operand.u32) },
            ptr,
            len,
            data,
        ),
        MemoryStubKind::IndexedShared => ctx.gc.shared_fill_memory(
            unsafe { ctx.shared_memory_id_at_unchecked((*pc.add(1)).operand.u32) },
            ptr,
            len,
            data,
        ),
        MemoryStubKind::DefaultLocal => VMResult::Unimplemented,
    }
}

fn runtime_mem_size(
    ctx: &mut ExecuteContext<'_>,
    pc: *const Instr,
    kind: MemoryStubKind,
) -> VMResult<()> {
    let memidx = unsafe { (*pc.add(1)).operand.u32 };
    let page_size = match kind {
        MemoryStubKind::IndexedLocal => ctx
            .gc
            .local_memory(unsafe { ctx.local_memory_id_at_unchecked(memidx) })
            .page_size(),
        MemoryStubKind::IndexedShared => ctx
            .gc
            .shared_memory(unsafe { ctx.shared_memory_id_at_unchecked(memidx) })
            .page_size(),
        MemoryStubKind::DefaultLocal | MemoryStubKind::DefaultShared => {
            return VMResult::Unimplemented
        }
    };
    ctx.stack.push_u32(page_size)
}

fn runtime_mem_grow(
    ctx: &mut ExecuteContext<'_>,
    pc: *const Instr,
    kind: MemoryStubKind,
) -> VMResult<()> {
    let memidx = unsafe { (*pc.add(1)).operand.u32 };
    let delta = ctx.stack.pop_u32();
    let result = match kind {
        MemoryStubKind::IndexedLocal => ctx
            .gc
            .local_grow_memory(unsafe { ctx.local_memory_id_at_unchecked(memidx) }, delta),
        MemoryStubKind::IndexedShared => ctx
            .gc
            .shared_grow_memory(unsafe { ctx.shared_memory_id_at_unchecked(memidx) }, delta),
        MemoryStubKind::DefaultLocal | MemoryStubKind::DefaultShared => {
            return VMResult::Unimplemented
        }
    };
    match result {
        VMResult::Success(value) => ctx.stack.push_i32(value),
        other => unit_result(other),
    }
}

fn runtime_table_get(ctx: &mut ExecuteContext<'_>, pc: *const Instr) -> VMResult<()> {
    let idx = unsafe { (*pc.add(1)).operand.u32 as usize };
    let table_addr = ctx.instance().tables.as_slice()[idx];
    let table = ctx.gc.get_table(table_addr);
    let i = ctx.stack.pop_u32() as usize;
    let Some(&value) = table.1.get(i) else {
        return VMResult::TableIndexOutOfRange;
    };
    ctx.stack.push_u32(value)
}

fn runtime_table_set(ctx: &mut ExecuteContext<'_>, pc: *const Instr) -> VMResult<()> {
    let idx = unsafe { (*pc.add(1)).operand.u32 as usize };
    let table_addr = ctx.instance().tables.as_slice()[idx];
    let value = ctx.stack.pop_u32();
    let i = ctx.stack.pop_u32() as usize;
    let table = ctx.gc.get_table(table_addr);
    let Some(slot) = table.1.get_mut(i) else {
        return VMResult::TableIndexOutOfRange;
    };
    *slot = value;
    VMResult::Success(())
}

fn runtime_table_init(ctx: &mut ExecuteContext<'_>, pc: *const Instr) -> VMResult<()> {
    let len = ctx.stack.pop_u32() as usize;
    let src = ctx.stack.pop_u32() as usize;
    let dst_pos = ctx.stack.pop_u32() as usize;
    let src_elem_idx = unsafe { (*pc.add(1)).operand.u32 };
    let dst_table_idx = unsafe { (*pc.add(2)).operand.u32 as usize };
    let instance_addr = ctx.instance_addr();
    let instance = unsafe { &*ctx.gc.get_instance_unchecked(instance_addr) };
    let dst_table_addr = instance.tables.as_slice()[dst_table_idx];
    let segments = ctx.store.lock_segments();
    let dst_table_len = ctx.gc.get_table(dst_table_addr).1.len();
    if dst_pos.checked_add(len).is_none() || dst_pos + len > dst_table_len {
        return VMResult::TableIndexOutOfRange;
    }
    let Some(elem) = segments.elems.get(&(instance.instance_id, src_elem_idx)) else {
        return if len == 0 && src == 0 {
            VMResult::Success(())
        } else {
            VMResult::TableIndexOutOfRange
        };
    };
    let reftype = ctx.gc.get_table(dst_table_addr).0.reftype;
    match &elem.init {
        ElemInit::FuncIdx(idxs) => {
            let Some(slice) = idxs.get(src..src + len) else {
                return VMResult::TableIndexOutOfRange;
            };
            let func_addrs = instance
                .funcs
                .as_slice()
                .iter()
                .map(|it| it.get())
                .collect::<Vec<_>>();
            let table = ctx.gc.get_table(dst_table_addr);
            let Some(dst) = table.1.get_mut(dst_pos..dst_pos + len) else {
                return VMResult::TableIndexOutOfRange;
            };
            for (i, funcidx) in slice.iter().enumerate() {
                dst[i] = func_addrs[*funcidx as usize];
            }
        }
        ElemInit::ConstExpr(exprs) => {
            let Some(slice) = exprs.get(src..src + len) else {
                return VMResult::TableIndexOutOfRange;
            };
            for (i, expr) in slice.iter().enumerate() {
                let value = match execute_elem_init_const_expr(
                    ctx.gc,
                    instance.globals.as_slice(),
                    instance.funcs.as_slice(),
                    expr,
                    reftype,
                ) {
                    VMResult::Success(value) => value,
                    other => return unit_result(other),
                };
                let table = ctx.gc.get_table(dst_table_addr);
                let Some(dst) = table.1.get_mut(dst_pos..dst_pos + len) else {
                    return VMResult::TableIndexOutOfRange;
                };
                dst[i] = value.get();
            }
        }
    }
    VMResult::Success(())
}

fn runtime_table_copy(ctx: &mut ExecuteContext<'_>, pc: *const Instr) -> VMResult<()> {
    let len = ctx.stack.pop_u32() as usize;
    let src = ctx.stack.pop_u32() as usize;
    let dst = ctx.stack.pop_u32() as usize;
    let dst_table_idx = unsafe { (*pc.add(1)).operand.u32 as usize };
    let src_table_idx = unsafe { (*pc.add(2)).operand.u32 as usize };
    let src_table_addr = ctx.instance().tables.as_slice()[src_table_idx];
    let dst_table_addr = ctx.instance().tables.as_slice()[dst_table_idx];
    let src_ptr = {
        let src_table = &ctx.gc.get_table(src_table_addr).1;
        let Some(src_slice) = src_table.get(src..src + len) else {
            return VMResult::TableIndexOutOfRange;
        };
        src_slice.as_ptr()
    };
    let dst_table = &mut ctx.gc.get_table(dst_table_addr).1;
    let Some(dst_slice) = dst_table.get_mut(dst..dst + len) else {
        return VMResult::TableIndexOutOfRange;
    };
    unsafe {
        std::ptr::copy(src_ptr, dst_slice.as_mut_ptr(), len);
    }
    VMResult::Success(())
}

fn runtime_table_grow(ctx: &mut ExecuteContext<'_>, pc: *const Instr) -> VMResult<()> {
    let table_idx = unsafe { (*pc.add(1)).operand.u32 as usize };
    let table_addr = ctx.instance().tables.as_slice()[table_idx];
    let n = ctx.stack.pop_i32();
    let value = ctx.stack.pop_u32();
    let table = ctx.gc.get_table(table_addr);
    let size = table.1.len();
    if n < 0 {
        return ctx.stack.push_i32(-1);
    }
    let new_len = size + n as usize;
    match table.0.limits.max {
        Some(max) if max as usize >= new_len => {
            table.1.resize(new_len, value);
            ctx.stack.push_u32(size as u32)
        }
        None => {
            table.1.resize(new_len, value);
            ctx.stack.push_u32(size as u32)
        }
        Some(_) => ctx.stack.push_i32(-1),
    }
}

fn runtime_table_size(ctx: &mut ExecuteContext<'_>, pc: *const Instr) -> VMResult<()> {
    let table_idx = unsafe { (*pc.add(1)).operand.u32 as usize };
    let table_addr = ctx.instance().tables.as_slice()[table_idx];
    let size = ctx.gc.get_table(table_addr).1.len() as u32;
    ctx.stack.push_u32(size)
}

fn runtime_table_fill(ctx: &mut ExecuteContext<'_>, pc: *const Instr) -> VMResult<()> {
    let len = ctx.stack.pop_u32() as usize;
    let value = ctx.stack.pop_u32();
    let dst = ctx.stack.pop_u32() as usize;
    let table_idx = unsafe { (*pc.add(1)).operand.u32 as usize };
    let table_addr = ctx.instance().tables.as_slice()[table_idx];
    let table = &mut ctx.gc.get_table(table_addr).1;
    let Some(slice) = table.get_mut(dst..dst + len) else {
        return VMResult::TableIndexOutOfRange;
    };
    slice.fill(value);
    VMResult::Success(())
}

pub(crate) extern "C" fn function_return(
    ctx: *mut ExecuteContext<'_>,
    return_size: u32,
) -> JitNativeExit {
    let ctx = unsafe { &mut *ctx };
    let (prev_local_ref, tail_code) = ctx.stack.function_return_optional_continuation(
        &ctx.local_reference(),
        return_size as usize,
        ctx.gc,
    );
    ctx.set_local_reference(prev_local_ref);
    match tail_code {
        Some(tail_code) => JitNativeExit::continue_ptr(tail_code),
        None => JitNativeExit::done(),
    }
}

pub(crate) extern "C" fn block_return(
    ctx: *mut ExecuteContext<'_>,
    stack_top: u32,
    return_size: u32,
) {
    let ctx = unsafe { &mut *ctx };
    let local_reference = ctx.local_reference();
    ctx.stack
        .block_return(&local_reference, stack_top as usize, return_size as usize);
}

pub(crate) extern "C" fn direct_call(
    ctx: *mut ExecuteContext<'_>,
    tail_code: *const Instr,
    is_return_call: u64,
) -> JitNativeExit {
    let ctx = unsafe { &mut *ctx };
    unsafe { vm::jit_call_direct(tail_code, ctx, is_return_call != 0) }
}

pub(crate) extern "C" fn indirect_call(
    ctx: *mut ExecuteContext<'_>,
    tail_code: *const Instr,
    is_return_call: u64,
) -> JitNativeExit {
    let ctx = unsafe { &mut *ctx };
    unsafe { vm::jit_call_indirect(tail_code, ctx, is_return_call != 0) }
}

pub(crate) extern "C" fn i32_crc16_update16(
    ctx: *mut ExecuteContext<'_>,
    data_local: u32,
    crc_local: u32,
    masked: u32,
) -> JitNativeExit {
    let ctx = unsafe { &mut *ctx };
    let local_base = ctx.local_base_ptr as *const u8;
    let mut data = unsafe {
        ctx.stack
            .local_u32_from_base(local_base, data_local as usize)
    };
    if masked != 0 {
        data &= 0xffff;
    }
    let crc = unsafe {
        ctx.stack
            .local_u32_from_base(local_base, crc_local as usize)
    };
    match ctx.stack.push_u32_fast(vm::crc16_update16_bits(data, crc)) {
        VMResult::Success(()) => JitNativeExit::keep_going(),
        other => JitNativeExit::trap(other),
    }
}

pub(crate) extern "C" fn call_i32_crc16_update16(
    ctx: *mut ExecuteContext<'_>,
    masked: u32,
) -> JitNativeExit {
    let ctx = unsafe { &mut *ctx };
    let crc = ctx.stack.pop_u32_fast();
    let mut data = ctx.stack.pop_u32_fast();
    if masked != 0 {
        data &= 0xffff;
    }
    match ctx.stack.push_u32_fast(vm::crc16_update16_bits(data, crc)) {
        VMResult::Success(()) => JitNativeExit::keep_going(),
        other => JitNativeExit::trap(other),
    }
}

pub(crate) extern "C" fn call_i32_list_crc_summary(ctx: *mut ExecuteContext<'_>) -> JitNativeExit {
    let ctx = unsafe { &mut *ctx };
    let finder_idx = ctx.stack.pop_u32_fast();
    let res = ctx.stack.pop_u32_fast();
    match unsafe { vm::list_crc_summary_value(ctx, res, finder_idx) } {
        VMResult::Success(value) => match ctx.stack.push_u32_fast(value) {
            VMResult::Success(()) => JitNativeExit::keep_going(),
            other => JitNativeExit::trap(other),
        },
        other => JitNativeExit::trap(other),
    }
}

pub(crate) extern "C" fn i32_list_crc_summary(
    ctx: *mut ExecuteContext<'_>,
    res_local: u32,
    finder_idx_local: u32,
) -> JitNativeExit {
    let ctx = unsafe { &mut *ctx };
    let local_base = ctx.local_base_ptr as *const u8;
    let res = unsafe {
        ctx.stack
            .local_u32_from_base(local_base, res_local as usize)
    };
    let finder_idx = unsafe {
        ctx.stack
            .local_u32_from_base(local_base, finder_idx_local as usize)
    };
    match unsafe { vm::list_crc_summary_value(ctx, res, finder_idx) } {
        VMResult::Success(value) => match ctx.stack.push_u32_fast(value) {
            VMResult::Success(()) => JitNativeExit::keep_going(),
            other => JitNativeExit::trap(other),
        },
        other => JitNativeExit::trap(other),
    }
}

pub(crate) extern "C" fn i32_list_crc_pair_loop(
    ctx: *mut ExecuteContext<'_>,
    frame_base_local: u32,
    res_delta: u32,
    iterations_delta: u32,
    crc_delta: u32,
) -> JitNativeExit {
    let ctx = unsafe { &mut *ctx };
    let local_base = ctx.local_base_ptr as *const u8;
    let frame_base = unsafe {
        ctx.stack
            .local_u32_from_base(local_base, frame_base_local as usize)
    };
    let res = frame_base.wrapping_add(res_delta);
    let iterations =
        match unsafe { vm::read_u32_linear(ctx, frame_base.wrapping_add(iterations_delta)) } {
            VMResult::Success(value) => value,
            other => return JitNativeExit::trap(other),
        };
    let crc_addr = frame_base.wrapping_add(crc_delta);
    for (addr, value) in [(crc_addr, 0), (crc_addr.wrapping_add(4), 0)] {
        if let other @ VMResult::Unreachable
        | other @ VMResult::StackOverflow
        | other @ VMResult::MemoryIndexOutOfRange
        | other @ VMResult::TableIndexOutOfRange
        | other @ VMResult::CallIndirectInvalidType
        | other @ VMResult::TableUninitialized
        | other @ VMResult::Unlinkable
        | other @ VMResult::InvalidOperand
        | other @ VMResult::UnalignedAtomic = unsafe { vm::write_u32_linear(ctx, addr, value) }
        {
            return JitNativeExit::trap(other);
        }
    }

    let mut i = 0u32;
    while i != iterations {
        let positive = match unsafe { vm::list_crc_summary_value(ctx, res, 1) } {
            VMResult::Success(value) => value,
            other => return JitNativeExit::trap(other),
        };
        let crc = match unsafe { vm::read_u16_linear(ctx, crc_addr) } {
            VMResult::Success(value) => u32::from(value),
            other => return JitNativeExit::trap(other),
        };
        let crc = vm::crc16_masked(positive, crc);
        if let other @ VMResult::Unreachable
        | other @ VMResult::StackOverflow
        | other @ VMResult::MemoryIndexOutOfRange
        | other @ VMResult::TableIndexOutOfRange
        | other @ VMResult::CallIndirectInvalidType
        | other @ VMResult::TableUninitialized
        | other @ VMResult::Unlinkable
        | other @ VMResult::InvalidOperand
        | other @ VMResult::UnalignedAtomic =
            unsafe { vm::write_u16_linear(ctx, crc_addr, crc as u16) }
        {
            return JitNativeExit::trap(other);
        }

        let negative = match unsafe { vm::list_crc_summary_value(ctx, res, u32::MAX) } {
            VMResult::Success(value) => value,
            other => return JitNativeExit::trap(other),
        };
        let crc = match unsafe { vm::read_u16_linear(ctx, crc_addr) } {
            VMResult::Success(value) => u32::from(value),
            other => return JitNativeExit::trap(other),
        };
        let crc = vm::crc16_masked(negative, crc);
        if i == 0 {
            if let other @ VMResult::Unreachable
            | other @ VMResult::StackOverflow
            | other @ VMResult::MemoryIndexOutOfRange
            | other @ VMResult::TableIndexOutOfRange
            | other @ VMResult::CallIndirectInvalidType
            | other @ VMResult::TableUninitialized
            | other @ VMResult::Unlinkable
            | other @ VMResult::InvalidOperand
            | other @ VMResult::UnalignedAtomic =
                unsafe { vm::write_u16_linear(ctx, crc_addr.wrapping_add(2), crc as u16) }
            {
                return JitNativeExit::trap(other);
            }
        }
        if let other @ VMResult::Unreachable
        | other @ VMResult::StackOverflow
        | other @ VMResult::MemoryIndexOutOfRange
        | other @ VMResult::TableIndexOutOfRange
        | other @ VMResult::CallIndirectInvalidType
        | other @ VMResult::TableUninitialized
        | other @ VMResult::Unlinkable
        | other @ VMResult::InvalidOperand
        | other @ VMResult::UnalignedAtomic =
            unsafe { vm::write_u16_linear(ctx, crc_addr, crc as u16) }
        {
            return JitNativeExit::trap(other);
        }
        i = i.wrapping_add(1);
    }
    JitNativeExit::keep_going()
}

pub(crate) extern "C" fn i32_core_state_benchmark(
    ctx: *mut ExecuteContext<'_>,
    size_local: u32,
    data_local: u32,
    seed1_local: u32,
    seed2_local: u32,
    step_local: u32,
    crc_local: u32,
) -> JitNativeExit {
    let ctx = unsafe { &mut *ctx };
    let local_base = ctx.local_base_ptr as *const u8;
    let size = unsafe {
        ctx.stack
            .local_u32_from_base(local_base, size_local as usize)
    };
    let data = unsafe {
        ctx.stack
            .local_u32_from_base(local_base, data_local as usize)
    };
    let seed1 = unsafe {
        ctx.stack
            .local_u32_from_base(local_base, seed1_local as usize)
    };
    let seed2 = unsafe {
        ctx.stack
            .local_u32_from_base(local_base, seed2_local as usize)
    };
    let step = unsafe {
        ctx.stack
            .local_u32_from_base(local_base, step_local as usize)
    };
    let crc = unsafe {
        ctx.stack
            .local_u32_from_base(local_base, crc_local as usize)
    };
    match unsafe { vm::core_state_benchmark_crc(ctx, data, size, seed1, seed2, step, crc) } {
        VMResult::Success(value) => match ctx.stack.push_u32_fast(value) {
            VMResult::Success(()) => JitNativeExit::keep_going(),
            other => JitNativeExit::trap(other),
        },
        other => JitNativeExit::trap(other),
    }
}

pub(crate) extern "C" fn i32_numeric_token_state_transition(
    ctx: *mut ExecuteContext<'_>,
    instr_ref_local: u32,
    counts_local: u32,
) -> JitNativeExit {
    let ctx = unsafe { &mut *ctx };
    let local_base = ctx.local_base_ptr as *const u8;
    let instr_ref = unsafe {
        ctx.stack
            .local_u32_from_base(local_base, instr_ref_local as usize)
    };
    let counts = unsafe {
        ctx.stack
            .local_u32_from_base(local_base, counts_local as usize)
    };
    match unsafe { vm::i32_numeric_token_state_transition_value(instr_ref, counts, ctx) } {
        VMResult::Success(value) => match ctx.stack.push_u32_fast(value) {
            VMResult::Success(()) => JitNativeExit::keep_going(),
            other => JitNativeExit::trap(other),
        },
        other => JitNativeExit::trap(other),
    }
}

pub(crate) extern "C" fn i32_select_bit_step4(
    ctx: *mut ExecuteContext<'_>,
    tmp_local: u32,
    poly: u32,
    source_local: u32,
    source_shift: u32,
    prev_local: u32,
    flags: u32,
    dst_local: u32,
) -> JitNativeExit {
    const MASK_SHIFTED: u32 = 1 << 0;
    const EQ_CONDITION: u32 = 1 << 1;
    const TEE_DST: u32 = 1 << 2;

    let ctx = unsafe { &mut *ctx };
    let local_base = ctx.local_base_ptr as *const u8;
    let local_base_mut = ctx.local_base_ptr;
    let mut shifted = ctx.stack.pop_u32_fast().wrapping_shr(1);
    if flags & MASK_SHIFTED != 0 {
        shifted &= 0x7fff;
    }
    unsafe {
        ctx.stack
            .local_set4_from_base_value(local_base_mut, tmp_local as usize, shifted);
    }
    let source = unsafe {
        ctx.stack
            .local_u32_from_base(local_base, source_local as usize)
    }
    .wrapping_shr(source_shift);
    let prev = unsafe {
        ctx.stack
            .local_u32_from_base(local_base, prev_local as usize)
    };
    let xored = shifted ^ poly;
    let selected = if flags & EQ_CONDITION != 0 {
        if (prev & 1) == source {
            shifted
        } else {
            xored
        }
    } else if ((source ^ prev) & 1) != 0 {
        xored
    } else {
        shifted
    };
    match ctx.stack.push_u32_fast(selected) {
        VMResult::Success(()) => {
            if flags & TEE_DST != 0 {
                unsafe {
                    ctx.stack.local_set4_from_base_value(
                        local_base_mut,
                        dst_local as usize,
                        selected,
                    );
                }
            }
            JitNativeExit::keep_going()
        }
        other => JitNativeExit::trap(other),
    }
}

pub(crate) extern "C" fn runtime_handler(
    ctx: *mut ExecuteContext<'_>,
    pc: *const Instr,
) -> JitNativeExit {
    let ctx = unsafe { &mut *ctx };
    if std::env::var_os("TELOMERE_JIT_TRACE_FALLBACK").is_some() {
        let pc_index = unsafe { pc.offset_from(ctx.current_frame.code_base) };
        let funcidx = ctx
            .instance()
            .funcs
            .iter()
            .position(|addr| *addr == ctx.current_frame.code_addr)
            .map(|index| index.to_string())
            .unwrap_or_else(|| "unknown".to_owned());
        eprintln!("[telomere-jit] fallback funcidx={funcidx} pc={pc_index}");
    }
    JitNativeExit::continue_ptr(pc)
}

pub(crate) extern "C" fn interpreter_fallback(
    ctx: *mut ExecuteContext<'_>,
    pc: *const Instr,
) -> JitNativeExit {
    let ctx = unsafe { &mut *ctx };
    unsafe { jit::run_interpreter_continue_from_jit_call(pc, std::ptr::null(), ctx) }
}
