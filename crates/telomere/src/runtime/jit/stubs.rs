use crate::{
    common::{ExecuteContext, Instr, VMResult},
    runtime::vm,
};

use super::abi::JitNativeExit;

pub(crate) extern "C" fn push_i32(ctx: *mut ExecuteContext<'_>, value: u32) -> JitNativeExit {
    let ctx = unsafe { &mut *ctx };
    match ctx.stack.push_u32_fast(value) {
        VMResult::Success(()) => JitNativeExit::keep_going(),
        other => JitNativeExit::trap(other),
    }
}

pub(crate) extern "C" fn function_return(
    ctx: *mut ExecuteContext<'_>,
    return_size: u32,
) -> JitNativeExit {
    let ctx = unsafe { &mut *ctx };
    let (prev_local_ref, tail_code) =
        ctx.stack
            .function_return(&ctx.local_reference(), return_size as usize, ctx.gc);
    ctx.set_local_reference(prev_local_ref);
    JitNativeExit::continue_ptr(tail_code)
}

pub(crate) extern "C" fn direct_call(
    ctx: *mut ExecuteContext<'_>,
    tail_code: *const Instr,
    is_return_call: u64,
) -> JitNativeExit {
    let ctx = unsafe { &mut *ctx };
    unsafe { vm::jit_call_direct(tail_code, ctx, is_return_call != 0) }
}

pub(crate) extern "C" fn i32_load(
    ctx: *mut ExecuteContext<'_>,
    addr: u32,
    offset: u32,
    width: u32,
    signed: u32,
) -> JitNativeExit {
    let ctx = unsafe { &mut *ctx };
    let sum = u64::from(addr) + u64::from(offset);
    if sum > u64::from(u32::MAX) {
        return JitNativeExit::trap(VMResult::<()>::MemoryIndexOutOfRange);
    }
    let start = sum as usize;
    let memory = unsafe { ctx.default_local_memory_unchecked() };
    let result = match (width, signed != 0) {
        (1, false) => match memory.read_u8_at(start) {
            VMResult::Success(value) => VMResult::Success(u32::from(value)),
            other => return JitNativeExit::trap(other),
        },
        (1, true) => match memory.read_i8_at(start) {
            VMResult::Success(value) => VMResult::Success(value as i32 as u32),
            other => return JitNativeExit::trap(other),
        },
        (2, false) => match memory.read_u16_at(start) {
            VMResult::Success(value) => VMResult::Success(u32::from(value)),
            other => return JitNativeExit::trap(other),
        },
        (2, true) => match memory.read_i16_at(start) {
            VMResult::Success(value) => VMResult::Success(value as i32 as u32),
            other => return JitNativeExit::trap(other),
        },
        (4, false) => memory.read_u32_at(start),
        _ => VMResult::InvalidOperand,
    };
    match result {
        VMResult::Success(value) => JitNativeExit {
            kind: JitNativeExit::KEEP_GOING,
            value: u64::from(value),
        },
        other => JitNativeExit::trap(other),
    }
}

pub(crate) extern "C" fn i32_store(
    ctx: *mut ExecuteContext<'_>,
    addr: u32,
    offset: u32,
    width: u32,
    value: u32,
) -> JitNativeExit {
    let ctx = unsafe { &mut *ctx };
    let sum = u64::from(addr) + u64::from(offset);
    if sum > u64::from(u32::MAX) {
        return JitNativeExit::trap(VMResult::<()>::MemoryIndexOutOfRange);
    }
    let start = sum as usize;
    let memory = unsafe { ctx.default_local_memory_mut_unchecked() };
    let result = match width {
        1 => memory.write_bytes(start, &[value as u8]),
        2 => memory.write_bytes(start, &(value as u16).to_le_bytes()),
        4 => memory.write_u32_at(start, value),
        _ => VMResult::InvalidOperand,
    };
    match result {
        VMResult::Success(()) => JitNativeExit::keep_going(),
        other => JitNativeExit::trap(other),
    }
}
