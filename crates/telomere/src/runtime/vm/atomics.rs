use super::*;
#[cfg(feature = "async-runtime")]
use crate::common::AtomicWaitResult;
use crate::common::{AtomicRmwOp, MemoryHandle};
use vstd::prelude::*;

verus! {

#[inline(always)]
fn wait_result_not_equal() -> (result: i32)
    ensures
        result == 1,
{
    1
}

#[inline(always)]
fn wait_result_ok() -> (result: i32)
    ensures
        result == 0,
{
    0
}

#[inline(always)]
fn wait_result_timed_out() -> (result: i32)
    ensures
        result == 2,
{
    2
}

} // verus!

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
unsafe fn atomic_start(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<usize> {
    let memarg = (*tail_code).operand.memarg;
    let offset = ctx.stack.pop_u32();
    compute_memory_offset(memarg, offset)
}

#[inline(always)]
/// WebAssembly threads shared-memory handle helper.
///
/// Spec:
/// - Threads: https://webassembly.github.io/threads/core/
///
/// Stack effect: internal runtime memory dispatch.
/// Traps: propagates the trap behavior of the underlying memory lookup.
/// Notes: Resolves the active memory handle for atomic operations.
///
/// # Safety
/// - `ctx` must reference a live execution context whose active memory slot is valid for the current frame.
/// - This helper must not hold a borrow across any follow-up atomic memory access.
unsafe fn atomic_handle(ctx: &ExecuteContext) -> VMResult<MemoryHandle> {
    debug_assert!(ctx.snapshot().has_default_memory());
    ctx.memory_handle_result()
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
            let start = vm_try!(atomic_start(tail_code, ctx));
            let handle = vm_try!(atomic_handle(ctx));
            let value = vm_try!(ctx.gc.$reader(handle, start));
            vm_try!(ctx.stack.$push(value as $cast));
            call_next(tail_code, 1, ctx)
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
            let value = ctx.stack.$pop() as $ty;
            let start = vm_try!(atomic_start(tail_code, ctx));
            let handle = vm_try!(atomic_handle(ctx));
            vm_try!(ctx.gc.$writer(handle, start, value));
            call_next(tail_code, 1, ctx)
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
            let value = ctx.stack.$pop() as $pop_ty;
            let start = vm_try!(atomic_start(tail_code, ctx));
            let handle = vm_try!(atomic_handle(ctx));
            let old = vm_try!(ctx.gc.$rmw(handle, start, $op, value));
            vm_try!(ctx.stack.$push(old as $push_ty));
            call_next(tail_code, 1, ctx)
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
            let value = ctx.stack.$pop() as $ty;
            let expected = ctx.stack.$pop() as $ty;
            let start = vm_try!(atomic_start(tail_code, ctx));
            let handle = vm_try!(atomic_handle(ctx));
            let old = vm_try!(ctx.gc.$cmpxchg(handle, start, expected, value));
            vm_try!(ctx.stack.$push(old as $push_ty));
            call_next(tail_code, 1, ctx)
        }
    };
}

atomic_load_op!(op_i32_atomic_load, atomic_load_u32, push_u32, u32);
atomic_load_op!(op_i64_atomic_load, atomic_load_u64, push_u64, u64);
atomic_load_op!(op_i32_atomic_load8_u, atomic_load_u8, push_u32, u32);
atomic_load_op!(op_i32_atomic_load16_u, atomic_load_u16, push_u32, u32);
atomic_load_op!(op_i64_atomic_load8_u, atomic_load_u8, push_u64, u64);
atomic_load_op!(op_i64_atomic_load16_u, atomic_load_u16, push_u64, u64);
atomic_load_op!(op_i64_atomic_load32_u, atomic_load_u32, push_u64, u64);

atomic_store_op!(op_i32_atomic_store, pop_u32, atomic_store_u32, u32);
atomic_store_op!(op_i64_atomic_store, pop_u64, atomic_store_u64, u64);
atomic_store_op!(op_i32_atomic_store8, pop_u32, atomic_store_u8, u8);
atomic_store_op!(op_i32_atomic_store16, pop_u32, atomic_store_u16, u16);
atomic_store_op!(op_i64_atomic_store8, pop_u64, atomic_store_u8, u8);
atomic_store_op!(op_i64_atomic_store16, pop_u64, atomic_store_u16, u16);
atomic_store_op!(op_i64_atomic_store32, pop_u64, atomic_store_u32, u32);

atomic_rmw_op!(
    op_i32_atomic_rmw_add,
    pop_u32,
    atomic_rmw_u32,
    push_u32,
    u32,
    u32,
    AtomicRmwOp::Add
);
atomic_rmw_op!(
    op_i64_atomic_rmw_add,
    pop_u64,
    atomic_rmw_u64,
    push_u64,
    u64,
    u64,
    AtomicRmwOp::Add
);
atomic_rmw_op!(
    op_i32_atomic_rmw8_add_u,
    pop_u32,
    atomic_rmw_u8,
    push_u32,
    u8,
    u32,
    AtomicRmwOp::Add
);
atomic_rmw_op!(
    op_i32_atomic_rmw16_add_u,
    pop_u32,
    atomic_rmw_u16,
    push_u32,
    u16,
    u32,
    AtomicRmwOp::Add
);
atomic_rmw_op!(
    op_i64_atomic_rmw8_add_u,
    pop_u64,
    atomic_rmw_u8,
    push_u64,
    u8,
    u64,
    AtomicRmwOp::Add
);
atomic_rmw_op!(
    op_i64_atomic_rmw16_add_u,
    pop_u64,
    atomic_rmw_u16,
    push_u64,
    u16,
    u64,
    AtomicRmwOp::Add
);
atomic_rmw_op!(
    op_i64_atomic_rmw32_add_u,
    pop_u64,
    atomic_rmw_u32,
    push_u64,
    u32,
    u64,
    AtomicRmwOp::Add
);

atomic_rmw_op!(
    op_i32_atomic_rmw_sub,
    pop_u32,
    atomic_rmw_u32,
    push_u32,
    u32,
    u32,
    AtomicRmwOp::Sub
);
atomic_rmw_op!(
    op_i64_atomic_rmw_sub,
    pop_u64,
    atomic_rmw_u64,
    push_u64,
    u64,
    u64,
    AtomicRmwOp::Sub
);
atomic_rmw_op!(
    op_i32_atomic_rmw8_sub_u,
    pop_u32,
    atomic_rmw_u8,
    push_u32,
    u8,
    u32,
    AtomicRmwOp::Sub
);
atomic_rmw_op!(
    op_i32_atomic_rmw16_sub_u,
    pop_u32,
    atomic_rmw_u16,
    push_u32,
    u16,
    u32,
    AtomicRmwOp::Sub
);
atomic_rmw_op!(
    op_i64_atomic_rmw8_sub_u,
    pop_u64,
    atomic_rmw_u8,
    push_u64,
    u8,
    u64,
    AtomicRmwOp::Sub
);
atomic_rmw_op!(
    op_i64_atomic_rmw16_sub_u,
    pop_u64,
    atomic_rmw_u16,
    push_u64,
    u16,
    u64,
    AtomicRmwOp::Sub
);
atomic_rmw_op!(
    op_i64_atomic_rmw32_sub_u,
    pop_u64,
    atomic_rmw_u32,
    push_u64,
    u32,
    u64,
    AtomicRmwOp::Sub
);

atomic_rmw_op!(
    op_i32_atomic_rmw_and,
    pop_u32,
    atomic_rmw_u32,
    push_u32,
    u32,
    u32,
    AtomicRmwOp::And
);
atomic_rmw_op!(
    op_i64_atomic_rmw_and,
    pop_u64,
    atomic_rmw_u64,
    push_u64,
    u64,
    u64,
    AtomicRmwOp::And
);
atomic_rmw_op!(
    op_i32_atomic_rmw8_and_u,
    pop_u32,
    atomic_rmw_u8,
    push_u32,
    u8,
    u32,
    AtomicRmwOp::And
);
atomic_rmw_op!(
    op_i32_atomic_rmw16_and_u,
    pop_u32,
    atomic_rmw_u16,
    push_u32,
    u16,
    u32,
    AtomicRmwOp::And
);
atomic_rmw_op!(
    op_i64_atomic_rmw8_and_u,
    pop_u64,
    atomic_rmw_u8,
    push_u64,
    u8,
    u64,
    AtomicRmwOp::And
);
atomic_rmw_op!(
    op_i64_atomic_rmw16_and_u,
    pop_u64,
    atomic_rmw_u16,
    push_u64,
    u16,
    u64,
    AtomicRmwOp::And
);
atomic_rmw_op!(
    op_i64_atomic_rmw32_and_u,
    pop_u64,
    atomic_rmw_u32,
    push_u64,
    u32,
    u64,
    AtomicRmwOp::And
);

atomic_rmw_op!(
    op_i32_atomic_rmw_or,
    pop_u32,
    atomic_rmw_u32,
    push_u32,
    u32,
    u32,
    AtomicRmwOp::Or
);
atomic_rmw_op!(
    op_i64_atomic_rmw_or,
    pop_u64,
    atomic_rmw_u64,
    push_u64,
    u64,
    u64,
    AtomicRmwOp::Or
);
atomic_rmw_op!(
    op_i32_atomic_rmw8_or_u,
    pop_u32,
    atomic_rmw_u8,
    push_u32,
    u8,
    u32,
    AtomicRmwOp::Or
);
atomic_rmw_op!(
    op_i32_atomic_rmw16_or_u,
    pop_u32,
    atomic_rmw_u16,
    push_u32,
    u16,
    u32,
    AtomicRmwOp::Or
);
atomic_rmw_op!(
    op_i64_atomic_rmw8_or_u,
    pop_u64,
    atomic_rmw_u8,
    push_u64,
    u8,
    u64,
    AtomicRmwOp::Or
);
atomic_rmw_op!(
    op_i64_atomic_rmw16_or_u,
    pop_u64,
    atomic_rmw_u16,
    push_u64,
    u16,
    u64,
    AtomicRmwOp::Or
);
atomic_rmw_op!(
    op_i64_atomic_rmw32_or_u,
    pop_u64,
    atomic_rmw_u32,
    push_u64,
    u32,
    u64,
    AtomicRmwOp::Or
);

atomic_rmw_op!(
    op_i32_atomic_rmw_xor,
    pop_u32,
    atomic_rmw_u32,
    push_u32,
    u32,
    u32,
    AtomicRmwOp::Xor
);
atomic_rmw_op!(
    op_i64_atomic_rmw_xor,
    pop_u64,
    atomic_rmw_u64,
    push_u64,
    u64,
    u64,
    AtomicRmwOp::Xor
);
atomic_rmw_op!(
    op_i32_atomic_rmw8_xor_u,
    pop_u32,
    atomic_rmw_u8,
    push_u32,
    u8,
    u32,
    AtomicRmwOp::Xor
);
atomic_rmw_op!(
    op_i32_atomic_rmw16_xor_u,
    pop_u32,
    atomic_rmw_u16,
    push_u32,
    u16,
    u32,
    AtomicRmwOp::Xor
);
atomic_rmw_op!(
    op_i64_atomic_rmw8_xor_u,
    pop_u64,
    atomic_rmw_u8,
    push_u64,
    u8,
    u64,
    AtomicRmwOp::Xor
);
atomic_rmw_op!(
    op_i64_atomic_rmw16_xor_u,
    pop_u64,
    atomic_rmw_u16,
    push_u64,
    u16,
    u64,
    AtomicRmwOp::Xor
);
atomic_rmw_op!(
    op_i64_atomic_rmw32_xor_u,
    pop_u64,
    atomic_rmw_u32,
    push_u64,
    u32,
    u64,
    AtomicRmwOp::Xor
);

atomic_rmw_op!(
    op_i32_atomic_rmw_xchg,
    pop_u32,
    atomic_rmw_u32,
    push_u32,
    u32,
    u32,
    AtomicRmwOp::Xchg
);
atomic_rmw_op!(
    op_i64_atomic_rmw_xchg,
    pop_u64,
    atomic_rmw_u64,
    push_u64,
    u64,
    u64,
    AtomicRmwOp::Xchg
);
atomic_rmw_op!(
    op_i32_atomic_rmw8_xchg_u,
    pop_u32,
    atomic_rmw_u8,
    push_u32,
    u8,
    u32,
    AtomicRmwOp::Xchg
);
atomic_rmw_op!(
    op_i32_atomic_rmw16_xchg_u,
    pop_u32,
    atomic_rmw_u16,
    push_u32,
    u16,
    u32,
    AtomicRmwOp::Xchg
);
atomic_rmw_op!(
    op_i64_atomic_rmw8_xchg_u,
    pop_u64,
    atomic_rmw_u8,
    push_u64,
    u8,
    u64,
    AtomicRmwOp::Xchg
);
atomic_rmw_op!(
    op_i64_atomic_rmw16_xchg_u,
    pop_u64,
    atomic_rmw_u16,
    push_u64,
    u16,
    u64,
    AtomicRmwOp::Xchg
);
atomic_rmw_op!(
    op_i64_atomic_rmw32_xchg_u,
    pop_u64,
    atomic_rmw_u32,
    push_u64,
    u32,
    u64,
    AtomicRmwOp::Xchg
);

atomic_cmpxchg_op!(
    op_i32_atomic_rmw_cmpxchg,
    pop_u32,
    atomic_cmpxchg_u32,
    push_u32,
    u32,
    u32
);
atomic_cmpxchg_op!(
    op_i64_atomic_rmw_cmpxchg,
    pop_u64,
    atomic_cmpxchg_u64,
    push_u64,
    u64,
    u64
);
atomic_cmpxchg_op!(
    op_i32_atomic_rmw8_cmpxchg_u,
    pop_u32,
    atomic_cmpxchg_u8,
    push_u32,
    u8,
    u32
);
atomic_cmpxchg_op!(
    op_i32_atomic_rmw16_cmpxchg_u,
    pop_u32,
    atomic_cmpxchg_u16,
    push_u32,
    u16,
    u32
);
atomic_cmpxchg_op!(
    op_i64_atomic_rmw8_cmpxchg_u,
    pop_u64,
    atomic_cmpxchg_u8,
    push_u64,
    u8,
    u64
);
atomic_cmpxchg_op!(
    op_i64_atomic_rmw16_cmpxchg_u,
    pop_u64,
    atomic_cmpxchg_u16,
    push_u64,
    u16,
    u64
);
atomic_cmpxchg_op!(
    op_i64_atomic_rmw32_cmpxchg_u,
    pop_u64,
    atomic_cmpxchg_u32,
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
    let count = ctx.stack.pop_u32();
    let start = vm_try!(atomic_start(tail_code, ctx));
    let handle = vm_try!(atomic_handle(ctx));
    let woken = match handle {
        MemoryHandle::Local(_) => 0,
        MemoryHandle::Shared(id) => {
            vm_try!(ctx.gc.clone_shared_memory(id).notify_waiters(start, count))
        }
    };
    vm_try!(ctx.stack.push_u32(woken));
    call_next(tail_code, 1, ctx)
}

#[cfg(feature = "async-runtime")]
/// WebAssembly threads `memory.atomic.wait` completion helper.
///
/// Spec:
/// - Threads: https://webassembly.github.io/threads/core/
///
/// Stack effect: internal async completion for wait operations.
/// Traps: propagates the trap behavior of the underlying wait operation.
/// Notes: Packages the wake result into the async runtime effect queue.
///
/// # Safety
/// - `ctx` must reference a live execution context whose wait effect queue is available.
/// - `shared` and `wait` must refer to a wait registration belonging to the active store and memory instance.
/// - This helper must not keep locks or borrows alive while constructing the async completion.
unsafe fn push_wait_effect(
    ctx: &mut ExecuteContext,
    shared: std::sync::Arc<crate::common::SharedMemoryObject>,
    wait: crate::common::SharedWaitRegistration,
    timeout_ns: i64,
    resume_pc: *const Instr,
) {
    let task_id = ctx.task_id;
    let fp = StablePc::from_raw_in_frame(ctx.gc, ctx.stack, ctx.local_reference, resume_pc);
    ctx.effect.push_async_effect(Box::pin(async move {
        let value = wait.wait_result(shared, timeout_ns).await;
        let value = match value {
            0 => wait_result_ok(),
            2 => wait_result_timed_out(),
            other => other,
        };
        AsyncResult {
            task_id,
            completion: AsyncCompletion::ContinueWithI32 { fp, value },
        }
    }));
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
    let timeout_ns = ctx.stack.pop_i64();
    let expected = ctx.stack.pop_u32();
    let start = vm_try!(atomic_start(tail_code, ctx));
    let handle = vm_try!(atomic_handle(ctx));
    match handle {
        MemoryHandle::Local(_) => VMResult::InvalidOperand,
        MemoryHandle::Shared(id) => {
            #[cfg(feature = "async-runtime")]
            {
                let shared = ctx.gc.clone_shared_memory(id);
                match vm_try!(shared.register_wait32(start, expected)) {
                    AtomicWaitResult::NotEqual => {
                        vm_try!(ctx.stack.push_i32(wait_result_not_equal()));
                        call_next(tail_code, 1, ctx)
                    }
                    AtomicWaitResult::Pending(wait) => {
                        let resume_pc = tail_code.offset(1);
                        push_wait_effect(ctx, shared, wait, timeout_ns, resume_pc);
                        let _ = wait_effect(ctx, resume_pc);
                        VMResult::Success(())
                    }
                }
            }
            #[cfg(not(feature = "async-runtime"))]
            {
                let _ = id;
                VMResult::InvalidOperand
            }
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
    let timeout_ns = ctx.stack.pop_i64();
    let expected = ctx.stack.pop_u64();
    let start = vm_try!(atomic_start(tail_code, ctx));
    let handle = vm_try!(atomic_handle(ctx));
    match handle {
        MemoryHandle::Local(_) => VMResult::InvalidOperand,
        MemoryHandle::Shared(id) => {
            #[cfg(feature = "async-runtime")]
            {
                let shared = ctx.gc.clone_shared_memory(id);
                match vm_try!(shared.register_wait64(start, expected)) {
                    AtomicWaitResult::NotEqual => {
                        vm_try!(ctx.stack.push_i32(wait_result_not_equal()));
                        call_next(tail_code, 1, ctx)
                    }
                    AtomicWaitResult::Pending(wait) => {
                        let resume_pc = tail_code.offset(1);
                        push_wait_effect(ctx, shared, wait, timeout_ns, resume_pc);
                        let _ = wait_effect(ctx, resume_pc);
                        VMResult::Success(())
                    }
                }
            }
            #[cfg(not(feature = "async-runtime"))]
            {
                let _ = id;
                VMResult::InvalidOperand
            }
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
    if let Some(handle) = ctx.memory_addr() {
        ctx.gc.atomic_fence(handle);
    }
    call_next(tail_code, 1, ctx)
}
