use super::*;
use crate::common::store::PrecomputedWaitSite;
use crate::common::AtomicRmwOp;
use crate::common::AtomicWaitResult;
use crate::common::SafepointMetadataCache;
use crate::runtime::memory_effect::{MemoryWaitPending, PendingOp};

const WAIT_RESULT_NOT_EQUAL: i32 = 1;

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
    ctx: &mut ExecuteContext,
) -> VMResult<(usize, u32)> {
    let memarg = (*tail_code).operand.memarg;
    let memidx = (*tail_code.add(1)).operand.u32;
    let offset = ctx.stack.pop_u32();
    let start = vm_try!(compute_memory_offset(memarg, offset));
    VMResult::Success((start, memidx))
}

#[inline(always)]
unsafe fn precomputed_wait_site_unchecked(tail_code: *const Instr) -> &'static PrecomputedWaitSite {
    &*((*tail_code).operand.code_ptr as *const PrecomputedWaitSite)
}

#[inline(always)]
unsafe fn precomputed_wait_start(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<usize> {
    let site = precomputed_wait_site_unchecked(tail_code);
    let offset = ctx.stack.pop_u32();
    compute_memory_offset(site.memarg, offset)
}

#[inline(always)]
unsafe fn precomputed_wait_start_indexed(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<(usize, u32)> {
    let site = precomputed_wait_site_unchecked(tail_code);
    let offset = ctx.stack.pop_u32();
    let start = vm_try!(compute_memory_offset(site.memarg, offset));
    VMResult::Success((start, site.memidx))
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
            let value = vm_try!(ctx.gc.$reader(ctx.default_local_memory_id_unchecked(), start));
            vm_try!(ctx.stack.$push(value as $cast));
            call_next(tail_code, 1, ctx)
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
            let start = vm_try!(atomic_start(tail_code, ctx));
            let value = vm_try!(ctx.gc.$reader(ctx.default_shared_memory_id_unchecked(), start));
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
            vm_try!(ctx.gc.$writer(
                ctx.default_local_memory_id_unchecked(),
                start,
                value,
            ));
            call_next(tail_code, 1, ctx)
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
            let value = ctx.stack.$pop() as $ty;
            let start = vm_try!(atomic_start(tail_code, ctx));
            vm_try!(ctx.gc.$writer(
                ctx.default_shared_memory_id_unchecked(),
                start,
                value,
            ));
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
            let old = vm_try!(ctx.gc.$rmw(
                ctx.default_local_memory_id_unchecked(),
                start,
                $op,
                value,
            ));
            vm_try!(ctx.stack.$push(old as $push_ty));
            call_next(tail_code, 1, ctx)
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
            let value = ctx.stack.$pop() as $pop_ty;
            let start = vm_try!(atomic_start(tail_code, ctx));
            let old = vm_try!(ctx.gc.$rmw(
                ctx.default_shared_memory_id_unchecked(),
                start,
                $op,
                value,
            ));
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
            let old = vm_try!(ctx.gc.$cmpxchg(
                ctx.default_local_memory_id_unchecked(),
                start,
                expected,
                value,
            ));
            vm_try!(ctx.stack.$push(old as $push_ty));
            call_next(tail_code, 1, ctx)
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
            let value = ctx.stack.$pop() as $ty;
            let expected = ctx.stack.$pop() as $ty;
            let start = vm_try!(atomic_start(tail_code, ctx));
            let old = vm_try!(ctx.gc.$cmpxchg(
                ctx.default_shared_memory_id_unchecked(),
                start,
                expected,
                value,
            ));
            vm_try!(ctx.stack.$push(old as $push_ty));
            call_next(tail_code, 1, ctx)
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
            let (start, memidx) = vm_try!(atomic_start_indexed(tail_code, ctx));
            let value = vm_try!(ctx.gc.$reader_local(ctx.local_memory_id_at_unchecked(memidx), start));
            vm_try!(ctx.stack.$push(value as $cast));
            call_next(tail_code, 2, ctx)
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
            let (start, memidx) = vm_try!(atomic_start_indexed(tail_code, ctx));
            let value = vm_try!(ctx.gc.$reader_shared(ctx.shared_memory_id_at_unchecked(memidx), start));
            vm_try!(ctx.stack.$push(value as $cast));
            call_next(tail_code, 2, ctx)
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
            let value = ctx.stack.$pop() as $ty;
            let (start, memidx) = vm_try!(atomic_start_indexed(tail_code, ctx));
            vm_try!(ctx.gc.$writer_local(ctx.local_memory_id_at_unchecked(memidx), start, value));
            call_next(tail_code, 2, ctx)
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
            let value = ctx.stack.$pop() as $ty;
            let (start, memidx) = vm_try!(atomic_start_indexed(tail_code, ctx));
            vm_try!(ctx.gc.$writer_shared(ctx.shared_memory_id_at_unchecked(memidx), start, value));
            call_next(tail_code, 2, ctx)
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
            let value = ctx.stack.$pop() as $pop_ty;
            let (start, memidx) = vm_try!(atomic_start_indexed(tail_code, ctx));
            let old = vm_try!(ctx.gc.$rmw_local(
                ctx.local_memory_id_at_unchecked(memidx),
                start,
                $op,
                value,
            ));
            vm_try!(ctx.stack.$push(old as $push_ty));
            call_next(tail_code, 2, ctx)
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
            let value = ctx.stack.$pop() as $pop_ty;
            let (start, memidx) = vm_try!(atomic_start_indexed(tail_code, ctx));
            let old = vm_try!(ctx.gc.$rmw_shared(
                ctx.shared_memory_id_at_unchecked(memidx),
                start,
                $op,
                value,
            ));
            vm_try!(ctx.stack.$push(old as $push_ty));
            call_next(tail_code, 2, ctx)
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
            let value = ctx.stack.$pop() as $ty;
            let expected = ctx.stack.$pop() as $ty;
            let (start, memidx) = vm_try!(atomic_start_indexed(tail_code, ctx));
            let old = vm_try!(ctx.gc.$cmpxchg_local(
                ctx.local_memory_id_at_unchecked(memidx),
                start,
                expected,
                value,
            ));
            vm_try!(ctx.stack.$push(old as $push_ty));
            call_next(tail_code, 2, ctx)
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
            let value = ctx.stack.$pop() as $ty;
            let expected = ctx.stack.$pop() as $ty;
            let (start, memidx) = vm_try!(atomic_start_indexed(tail_code, ctx));
            let old = vm_try!(ctx.gc.$cmpxchg_shared(
                ctx.shared_memory_id_at_unchecked(memidx),
                start,
                expected,
                value,
            ));
            vm_try!(ctx.stack.$push(old as $push_ty));
            call_next(tail_code, 2, ctx)
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
    local_atomic_load_u32,
    shared_atomic_load_u32,
    push_u32,
    u32
);
atomic_load_op_indexed!(
    op_i64_atomic_load_indexed_local,
    op_i64_atomic_load_indexed_shared,
    local_atomic_load_u64,
    shared_atomic_load_u64,
    push_u64,
    u64
);
atomic_load_op_indexed!(
    op_i32_atomic_load8_u_indexed_local,
    op_i32_atomic_load8_u_indexed_shared,
    local_atomic_load_u8,
    shared_atomic_load_u8,
    push_u32,
    u32
);
atomic_load_op_indexed!(
    op_i32_atomic_load16_u_indexed_local,
    op_i32_atomic_load16_u_indexed_shared,
    local_atomic_load_u16,
    shared_atomic_load_u16,
    push_u32,
    u32
);
atomic_load_op_indexed!(
    op_i64_atomic_load8_u_indexed_local,
    op_i64_atomic_load8_u_indexed_shared,
    local_atomic_load_u8,
    shared_atomic_load_u8,
    push_u64,
    u64
);
atomic_load_op_indexed!(
    op_i64_atomic_load16_u_indexed_local,
    op_i64_atomic_load16_u_indexed_shared,
    local_atomic_load_u16,
    shared_atomic_load_u16,
    push_u64,
    u64
);
atomic_load_op_indexed!(
    op_i64_atomic_load32_u_indexed_local,
    op_i64_atomic_load32_u_indexed_shared,
    local_atomic_load_u32,
    shared_atomic_load_u32,
    push_u64,
    u64
);

atomic_store_op_indexed!(
    op_i32_atomic_store_indexed_local,
    op_i32_atomic_store_indexed_shared,
    pop_u32,
    local_atomic_store_u32,
    shared_atomic_store_u32,
    u32
);
atomic_store_op_indexed!(
    op_i64_atomic_store_indexed_local,
    op_i64_atomic_store_indexed_shared,
    pop_u64,
    local_atomic_store_u64,
    shared_atomic_store_u64,
    u64
);
atomic_store_op_indexed!(
    op_i32_atomic_store8_indexed_local,
    op_i32_atomic_store8_indexed_shared,
    pop_u32,
    local_atomic_store_u8,
    shared_atomic_store_u8,
    u8
);
atomic_store_op_indexed!(
    op_i32_atomic_store16_indexed_local,
    op_i32_atomic_store16_indexed_shared,
    pop_u32,
    local_atomic_store_u16,
    shared_atomic_store_u16,
    u16
);
atomic_store_op_indexed!(
    op_i64_atomic_store8_indexed_local,
    op_i64_atomic_store8_indexed_shared,
    pop_u64,
    local_atomic_store_u8,
    shared_atomic_store_u8,
    u8
);
atomic_store_op_indexed!(
    op_i64_atomic_store16_indexed_local,
    op_i64_atomic_store16_indexed_shared,
    pop_u64,
    local_atomic_store_u16,
    shared_atomic_store_u16,
    u16
);
atomic_store_op_indexed!(
    op_i64_atomic_store32_indexed_local,
    op_i64_atomic_store32_indexed_shared,
    pop_u64,
    local_atomic_store_u32,
    shared_atomic_store_u32,
    u32
);

atomic_rmw_op_indexed!(
    op_i32_atomic_rmw_add_indexed_local,
    op_i32_atomic_rmw_add_indexed_shared,
    pop_u32,
    local_atomic_rmw_u32,
    shared_atomic_rmw_u32,
    push_u32,
    u32,
    u32,
    AtomicRmwOp::Add
);
atomic_rmw_op_indexed!(
    op_i64_atomic_rmw_add_indexed_local,
    op_i64_atomic_rmw_add_indexed_shared,
    pop_u64,
    local_atomic_rmw_u64,
    shared_atomic_rmw_u64,
    push_u64,
    u64,
    u64,
    AtomicRmwOp::Add
);
atomic_rmw_op_indexed!(
    op_i32_atomic_rmw8_add_u_indexed_local,
    op_i32_atomic_rmw8_add_u_indexed_shared,
    pop_u32,
    local_atomic_rmw_u8,
    shared_atomic_rmw_u8,
    push_u32,
    u8,
    u32,
    AtomicRmwOp::Add
);
atomic_rmw_op_indexed!(
    op_i32_atomic_rmw16_add_u_indexed_local,
    op_i32_atomic_rmw16_add_u_indexed_shared,
    pop_u32,
    local_atomic_rmw_u16,
    shared_atomic_rmw_u16,
    push_u32,
    u16,
    u32,
    AtomicRmwOp::Add
);
atomic_rmw_op_indexed!(
    op_i64_atomic_rmw8_add_u_indexed_local,
    op_i64_atomic_rmw8_add_u_indexed_shared,
    pop_u64,
    local_atomic_rmw_u8,
    shared_atomic_rmw_u8,
    push_u64,
    u8,
    u64,
    AtomicRmwOp::Add
);
atomic_rmw_op_indexed!(
    op_i64_atomic_rmw16_add_u_indexed_local,
    op_i64_atomic_rmw16_add_u_indexed_shared,
    pop_u64,
    local_atomic_rmw_u16,
    shared_atomic_rmw_u16,
    push_u64,
    u16,
    u64,
    AtomicRmwOp::Add
);
atomic_rmw_op_indexed!(
    op_i64_atomic_rmw32_add_u_indexed_local,
    op_i64_atomic_rmw32_add_u_indexed_shared,
    pop_u64,
    local_atomic_rmw_u32,
    shared_atomic_rmw_u32,
    push_u64,
    u32,
    u64,
    AtomicRmwOp::Add
);
atomic_rmw_op_indexed!(
    op_i32_atomic_rmw_sub_indexed_local,
    op_i32_atomic_rmw_sub_indexed_shared,
    pop_u32,
    local_atomic_rmw_u32,
    shared_atomic_rmw_u32,
    push_u32,
    u32,
    u32,
    AtomicRmwOp::Sub
);
atomic_rmw_op_indexed!(
    op_i64_atomic_rmw_sub_indexed_local,
    op_i64_atomic_rmw_sub_indexed_shared,
    pop_u64,
    local_atomic_rmw_u64,
    shared_atomic_rmw_u64,
    push_u64,
    u64,
    u64,
    AtomicRmwOp::Sub
);
atomic_rmw_op_indexed!(
    op_i32_atomic_rmw8_sub_u_indexed_local,
    op_i32_atomic_rmw8_sub_u_indexed_shared,
    pop_u32,
    local_atomic_rmw_u8,
    shared_atomic_rmw_u8,
    push_u32,
    u8,
    u32,
    AtomicRmwOp::Sub
);
atomic_rmw_op_indexed!(
    op_i32_atomic_rmw16_sub_u_indexed_local,
    op_i32_atomic_rmw16_sub_u_indexed_shared,
    pop_u32,
    local_atomic_rmw_u16,
    shared_atomic_rmw_u16,
    push_u32,
    u16,
    u32,
    AtomicRmwOp::Sub
);
atomic_rmw_op_indexed!(
    op_i64_atomic_rmw8_sub_u_indexed_local,
    op_i64_atomic_rmw8_sub_u_indexed_shared,
    pop_u64,
    local_atomic_rmw_u8,
    shared_atomic_rmw_u8,
    push_u64,
    u8,
    u64,
    AtomicRmwOp::Sub
);
atomic_rmw_op_indexed!(
    op_i64_atomic_rmw16_sub_u_indexed_local,
    op_i64_atomic_rmw16_sub_u_indexed_shared,
    pop_u64,
    local_atomic_rmw_u16,
    shared_atomic_rmw_u16,
    push_u64,
    u16,
    u64,
    AtomicRmwOp::Sub
);
atomic_rmw_op_indexed!(
    op_i64_atomic_rmw32_sub_u_indexed_local,
    op_i64_atomic_rmw32_sub_u_indexed_shared,
    pop_u64,
    local_atomic_rmw_u32,
    shared_atomic_rmw_u32,
    push_u64,
    u32,
    u64,
    AtomicRmwOp::Sub
);
atomic_rmw_op_indexed!(
    op_i32_atomic_rmw_and_indexed_local,
    op_i32_atomic_rmw_and_indexed_shared,
    pop_u32,
    local_atomic_rmw_u32,
    shared_atomic_rmw_u32,
    push_u32,
    u32,
    u32,
    AtomicRmwOp::And
);
atomic_rmw_op_indexed!(
    op_i64_atomic_rmw_and_indexed_local,
    op_i64_atomic_rmw_and_indexed_shared,
    pop_u64,
    local_atomic_rmw_u64,
    shared_atomic_rmw_u64,
    push_u64,
    u64,
    u64,
    AtomicRmwOp::And
);
atomic_rmw_op_indexed!(
    op_i32_atomic_rmw8_and_u_indexed_local,
    op_i32_atomic_rmw8_and_u_indexed_shared,
    pop_u32,
    local_atomic_rmw_u8,
    shared_atomic_rmw_u8,
    push_u32,
    u8,
    u32,
    AtomicRmwOp::And
);
atomic_rmw_op_indexed!(
    op_i32_atomic_rmw16_and_u_indexed_local,
    op_i32_atomic_rmw16_and_u_indexed_shared,
    pop_u32,
    local_atomic_rmw_u16,
    shared_atomic_rmw_u16,
    push_u32,
    u16,
    u32,
    AtomicRmwOp::And
);
atomic_rmw_op_indexed!(
    op_i64_atomic_rmw8_and_u_indexed_local,
    op_i64_atomic_rmw8_and_u_indexed_shared,
    pop_u64,
    local_atomic_rmw_u8,
    shared_atomic_rmw_u8,
    push_u64,
    u8,
    u64,
    AtomicRmwOp::And
);
atomic_rmw_op_indexed!(
    op_i64_atomic_rmw16_and_u_indexed_local,
    op_i64_atomic_rmw16_and_u_indexed_shared,
    pop_u64,
    local_atomic_rmw_u16,
    shared_atomic_rmw_u16,
    push_u64,
    u16,
    u64,
    AtomicRmwOp::And
);
atomic_rmw_op_indexed!(
    op_i64_atomic_rmw32_and_u_indexed_local,
    op_i64_atomic_rmw32_and_u_indexed_shared,
    pop_u64,
    local_atomic_rmw_u32,
    shared_atomic_rmw_u32,
    push_u64,
    u32,
    u64,
    AtomicRmwOp::And
);
atomic_rmw_op_indexed!(
    op_i32_atomic_rmw_or_indexed_local,
    op_i32_atomic_rmw_or_indexed_shared,
    pop_u32,
    local_atomic_rmw_u32,
    shared_atomic_rmw_u32,
    push_u32,
    u32,
    u32,
    AtomicRmwOp::Or
);
atomic_rmw_op_indexed!(
    op_i64_atomic_rmw_or_indexed_local,
    op_i64_atomic_rmw_or_indexed_shared,
    pop_u64,
    local_atomic_rmw_u64,
    shared_atomic_rmw_u64,
    push_u64,
    u64,
    u64,
    AtomicRmwOp::Or
);
atomic_rmw_op_indexed!(
    op_i32_atomic_rmw8_or_u_indexed_local,
    op_i32_atomic_rmw8_or_u_indexed_shared,
    pop_u32,
    local_atomic_rmw_u8,
    shared_atomic_rmw_u8,
    push_u32,
    u8,
    u32,
    AtomicRmwOp::Or
);
atomic_rmw_op_indexed!(
    op_i32_atomic_rmw16_or_u_indexed_local,
    op_i32_atomic_rmw16_or_u_indexed_shared,
    pop_u32,
    local_atomic_rmw_u16,
    shared_atomic_rmw_u16,
    push_u32,
    u16,
    u32,
    AtomicRmwOp::Or
);
atomic_rmw_op_indexed!(
    op_i64_atomic_rmw8_or_u_indexed_local,
    op_i64_atomic_rmw8_or_u_indexed_shared,
    pop_u64,
    local_atomic_rmw_u8,
    shared_atomic_rmw_u8,
    push_u64,
    u8,
    u64,
    AtomicRmwOp::Or
);
atomic_rmw_op_indexed!(
    op_i64_atomic_rmw16_or_u_indexed_local,
    op_i64_atomic_rmw16_or_u_indexed_shared,
    pop_u64,
    local_atomic_rmw_u16,
    shared_atomic_rmw_u16,
    push_u64,
    u16,
    u64,
    AtomicRmwOp::Or
);
atomic_rmw_op_indexed!(
    op_i64_atomic_rmw32_or_u_indexed_local,
    op_i64_atomic_rmw32_or_u_indexed_shared,
    pop_u64,
    local_atomic_rmw_u32,
    shared_atomic_rmw_u32,
    push_u64,
    u32,
    u64,
    AtomicRmwOp::Or
);
atomic_rmw_op_indexed!(
    op_i32_atomic_rmw_xor_indexed_local,
    op_i32_atomic_rmw_xor_indexed_shared,
    pop_u32,
    local_atomic_rmw_u32,
    shared_atomic_rmw_u32,
    push_u32,
    u32,
    u32,
    AtomicRmwOp::Xor
);
atomic_rmw_op_indexed!(
    op_i64_atomic_rmw_xor_indexed_local,
    op_i64_atomic_rmw_xor_indexed_shared,
    pop_u64,
    local_atomic_rmw_u64,
    shared_atomic_rmw_u64,
    push_u64,
    u64,
    u64,
    AtomicRmwOp::Xor
);
atomic_rmw_op_indexed!(
    op_i32_atomic_rmw8_xor_u_indexed_local,
    op_i32_atomic_rmw8_xor_u_indexed_shared,
    pop_u32,
    local_atomic_rmw_u8,
    shared_atomic_rmw_u8,
    push_u32,
    u8,
    u32,
    AtomicRmwOp::Xor
);
atomic_rmw_op_indexed!(
    op_i32_atomic_rmw16_xor_u_indexed_local,
    op_i32_atomic_rmw16_xor_u_indexed_shared,
    pop_u32,
    local_atomic_rmw_u16,
    shared_atomic_rmw_u16,
    push_u32,
    u16,
    u32,
    AtomicRmwOp::Xor
);
atomic_rmw_op_indexed!(
    op_i64_atomic_rmw8_xor_u_indexed_local,
    op_i64_atomic_rmw8_xor_u_indexed_shared,
    pop_u64,
    local_atomic_rmw_u8,
    shared_atomic_rmw_u8,
    push_u64,
    u8,
    u64,
    AtomicRmwOp::Xor
);
atomic_rmw_op_indexed!(
    op_i64_atomic_rmw16_xor_u_indexed_local,
    op_i64_atomic_rmw16_xor_u_indexed_shared,
    pop_u64,
    local_atomic_rmw_u16,
    shared_atomic_rmw_u16,
    push_u64,
    u16,
    u64,
    AtomicRmwOp::Xor
);
atomic_rmw_op_indexed!(
    op_i64_atomic_rmw32_xor_u_indexed_local,
    op_i64_atomic_rmw32_xor_u_indexed_shared,
    pop_u64,
    local_atomic_rmw_u32,
    shared_atomic_rmw_u32,
    push_u64,
    u32,
    u64,
    AtomicRmwOp::Xor
);
atomic_rmw_op_indexed!(
    op_i32_atomic_rmw_xchg_indexed_local,
    op_i32_atomic_rmw_xchg_indexed_shared,
    pop_u32,
    local_atomic_rmw_u32,
    shared_atomic_rmw_u32,
    push_u32,
    u32,
    u32,
    AtomicRmwOp::Xchg
);
atomic_rmw_op_indexed!(
    op_i64_atomic_rmw_xchg_indexed_local,
    op_i64_atomic_rmw_xchg_indexed_shared,
    pop_u64,
    local_atomic_rmw_u64,
    shared_atomic_rmw_u64,
    push_u64,
    u64,
    u64,
    AtomicRmwOp::Xchg
);
atomic_rmw_op_indexed!(
    op_i32_atomic_rmw8_xchg_u_indexed_local,
    op_i32_atomic_rmw8_xchg_u_indexed_shared,
    pop_u32,
    local_atomic_rmw_u8,
    shared_atomic_rmw_u8,
    push_u32,
    u8,
    u32,
    AtomicRmwOp::Xchg
);
atomic_rmw_op_indexed!(
    op_i32_atomic_rmw16_xchg_u_indexed_local,
    op_i32_atomic_rmw16_xchg_u_indexed_shared,
    pop_u32,
    local_atomic_rmw_u16,
    shared_atomic_rmw_u16,
    push_u32,
    u16,
    u32,
    AtomicRmwOp::Xchg
);
atomic_rmw_op_indexed!(
    op_i64_atomic_rmw8_xchg_u_indexed_local,
    op_i64_atomic_rmw8_xchg_u_indexed_shared,
    pop_u64,
    local_atomic_rmw_u8,
    shared_atomic_rmw_u8,
    push_u64,
    u8,
    u64,
    AtomicRmwOp::Xchg
);
atomic_rmw_op_indexed!(
    op_i64_atomic_rmw16_xchg_u_indexed_local,
    op_i64_atomic_rmw16_xchg_u_indexed_shared,
    pop_u64,
    local_atomic_rmw_u16,
    shared_atomic_rmw_u16,
    push_u64,
    u16,
    u64,
    AtomicRmwOp::Xchg
);
atomic_rmw_op_indexed!(
    op_i64_atomic_rmw32_xchg_u_indexed_local,
    op_i64_atomic_rmw32_xchg_u_indexed_shared,
    pop_u64,
    local_atomic_rmw_u32,
    shared_atomic_rmw_u32,
    push_u64,
    u32,
    u64,
    AtomicRmwOp::Xchg
);

atomic_cmpxchg_op_indexed!(
    op_i32_atomic_rmw_cmpxchg_indexed_local,
    op_i32_atomic_rmw_cmpxchg_indexed_shared,
    pop_u32,
    local_atomic_cmpxchg_u32,
    shared_atomic_cmpxchg_u32,
    push_u32,
    u32,
    u32
);
atomic_cmpxchg_op_indexed!(
    op_i64_atomic_rmw_cmpxchg_indexed_local,
    op_i64_atomic_rmw_cmpxchg_indexed_shared,
    pop_u64,
    local_atomic_cmpxchg_u64,
    shared_atomic_cmpxchg_u64,
    push_u64,
    u64,
    u64
);
atomic_cmpxchg_op_indexed!(
    op_i32_atomic_rmw8_cmpxchg_u_indexed_local,
    op_i32_atomic_rmw8_cmpxchg_u_indexed_shared,
    pop_u32,
    local_atomic_cmpxchg_u8,
    shared_atomic_cmpxchg_u8,
    push_u32,
    u8,
    u32
);
atomic_cmpxchg_op_indexed!(
    op_i32_atomic_rmw16_cmpxchg_u_indexed_local,
    op_i32_atomic_rmw16_cmpxchg_u_indexed_shared,
    pop_u32,
    local_atomic_cmpxchg_u16,
    shared_atomic_cmpxchg_u16,
    push_u32,
    u16,
    u32
);
atomic_cmpxchg_op_indexed!(
    op_i64_atomic_rmw8_cmpxchg_u_indexed_local,
    op_i64_atomic_rmw8_cmpxchg_u_indexed_shared,
    pop_u64,
    local_atomic_cmpxchg_u8,
    shared_atomic_cmpxchg_u8,
    push_u64,
    u8,
    u64
);
atomic_cmpxchg_op_indexed!(
    op_i64_atomic_rmw16_cmpxchg_u_indexed_local,
    op_i64_atomic_rmw16_cmpxchg_u_indexed_shared,
    pop_u64,
    local_atomic_cmpxchg_u16,
    shared_atomic_cmpxchg_u16,
    push_u64,
    u16,
    u64
);
atomic_cmpxchg_op_indexed!(
    op_i64_atomic_rmw32_cmpxchg_u_indexed_local,
    op_i64_atomic_rmw32_cmpxchg_u_indexed_shared,
    pop_u64,
    local_atomic_cmpxchg_u32,
    shared_atomic_cmpxchg_u32,
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
    let _count = ctx.stack.pop_u32();
    let _start = vm_try!(atomic_start(tail_code, ctx));
    let woken = 0;
    vm_try!(ctx.stack.push_u32(woken));
    call_next(tail_code, 1, ctx)
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
    let count = ctx.stack.pop_u32();
    let start = vm_try!(atomic_start(tail_code, ctx));
    let woken = vm_try!(ctx
        .gc
        .shared_memory(ctx.default_shared_memory_id_unchecked())
        .notify_waiters(start, count));
    vm_try!(ctx.stack.push_u32(woken));
    call_next(tail_code, 1, ctx)
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
    let count = ctx.stack.pop_u32();
    let (start, _memidx) = vm_try!(atomic_start_indexed(tail_code, ctx));
    let _ = (count, start);
    vm_try!(ctx.stack.push_u32(0));
    call_next(tail_code, 2, ctx)
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
    let count = ctx.stack.pop_u32();
    let (start, memidx) = vm_try!(atomic_start_indexed(tail_code, ctx));
    let woken = vm_try!(ctx
        .gc
        .shared_memory(ctx.shared_memory_id_at_unchecked(memidx))
        .notify_waiters(start, count));
    vm_try!(ctx.stack.push_u32(woken));
    call_next(tail_code, 2, ctx)
}

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
    #[cfg(debug_assertions)]
    ctx.visit_current_ref_ranges(|_| {});
    let fp = StablePc::from_raw_in_call_frame(ctx.current_frame, resume_pc).raw();
    ctx.effect
        .push_pending(PendingOp::MemoryWait(MemoryWaitPending {
            task_id,
            shared,
            wait,
            timeout_ns,
            fp,
        }));
}

#[inline(always)]
unsafe fn cold_lookup_wait_safepoint(
    ctx: &ExecuteContext,
    tail_code: *const Instr,
) -> SafepointMetadataCache {
    let Some(layout) = ctx.func().frame_layout_header() else {
        return SafepointMetadataCache::EMPTY;
    };
    let raw_start = unsafe { tail_code.offset_from(ctx.code()) };
    let Some(raw_start) = usize::try_from(raw_start)
        .ok()
        .and_then(|value| value.checked_sub(1))
    else {
        return SafepointMetadataCache::EMPTY;
    };
    let Some(instruction_ordinal) = layout.instruction_ordinal_for_raw_start(raw_start) else {
        return SafepointMetadataCache::EMPTY;
    };
    SafepointMetadataCache::new(
        layout
            .stack_map_site(instruction_ordinal)
            .map_or(0, |site| site as *const _ as usize),
        layout
            .unwind_site(instruction_ordinal)
            .map_or(0, |site| site as *const _ as usize),
    )
}

#[inline(always)]
unsafe fn push_wait_effect_precomputed(
    ctx: &mut ExecuteContext,
    shared: std::sync::Arc<crate::common::SharedMemoryObject>,
    wait: crate::common::SharedWaitRegistration,
    timeout_ns: i64,
    resume_pc: StablePc,
) {
    let task_id = ctx.task_id;
    #[cfg(debug_assertions)]
    ctx.visit_current_ref_ranges(|_| {});
    ctx.effect
        .push_pending(PendingOp::MemoryWait(MemoryWaitPending {
            task_id,
            shared,
            wait,
            timeout_ns,
            fp: resume_pc.raw(),
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
    let _timeout_ns = ctx.stack.pop_i64();
    let _expected = ctx.stack.pop_u32();
    let _start = vm_try!(atomic_start(tail_code, ctx));
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
    let timeout_ns = ctx.stack.pop_i64();
    let expected = ctx.stack.pop_u32();
    let start = vm_try!(atomic_start(tail_code, ctx));
    let shared = ctx
        .gc
        .shared_memory(ctx.default_shared_memory_id_unchecked());
    match vm_try!(shared.register_wait32(start, expected)) {
        AtomicWaitResult::NotEqual => {
            vm_try!(ctx.stack.push_i32(WAIT_RESULT_NOT_EQUAL));
            call_next(tail_code, 1, ctx)
        }
        AtomicWaitResult::Pending(wait) => {
            let resume_pc = tail_code.offset(1);
            let safepoint = cold_lookup_wait_safepoint(ctx, tail_code);
            let shared = ctx
                .gc
                .clone_shared_memory(ctx.default_shared_memory_id_unchecked());
            ctx.set_safepoint(safepoint);
            push_wait_effect(ctx, shared, wait, timeout_ns, resume_pc);
            trace!("waiting effect: {:?}", resume_pc);
            ctx.cont = resume_pc;
            VMResult::Success(())
        }
    }
}

/// WebAssembly `memory.atomic.wait32` on shared default memory with precomputed continuation.
///
/// Related spec:
/// - Threads: https://webassembly.github.io/threads/core/
///
/// Stack effect: `[i32, i32, i64] -> [i32]`.
/// Traps: traps on out-of-bounds access or unaligned access.
/// Notes: Uses instantiate-time precomputed resume metadata to avoid rebuilding the continuation on the pending path.
///
/// # Safety
/// - `tail_code` must point to the metadata-bearing operand for this handler in the active instruction stream.
/// - `ctx` must reference a live execution context whose default memory is shared.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_memory_atomic_wait32_shared_precomputed(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let timeout_ns = ctx.stack.pop_i64();
    let expected = ctx.stack.pop_u32();
    let start = vm_try!(precomputed_wait_start(tail_code, ctx));
    let shared = ctx
        .gc
        .shared_memory(ctx.default_shared_memory_id_unchecked());
    match vm_try!(shared.register_wait32(start, expected)) {
        AtomicWaitResult::NotEqual => {
            vm_try!(ctx.stack.push_i32(WAIT_RESULT_NOT_EQUAL));
            call_next(tail_code, 1, ctx)
        }
        AtomicWaitResult::Pending(wait) => {
            let site = precomputed_wait_site_unchecked(tail_code);
            let safepoint = site.safepoint_cache();
            let shared = ctx
                .gc
                .clone_shared_memory(ctx.default_shared_memory_id_unchecked());
            ctx.set_safepoint(safepoint);
            push_wait_effect_precomputed(ctx, shared, wait, timeout_ns, site.resume_pc);
            ctx.cont = site.resume_pc.resolve_in_call_frame(ctx.current_frame);
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
    let _timeout_ns = ctx.stack.pop_i64();
    let _expected = ctx.stack.pop_u32();
    let _start = vm_try!(atomic_start_indexed(tail_code, ctx));
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
    let timeout_ns = ctx.stack.pop_i64();
    let expected = ctx.stack.pop_u32();
    let (start, memidx) = vm_try!(atomic_start_indexed(tail_code, ctx));
    let shared = ctx
        .gc
        .shared_memory(ctx.shared_memory_id_at_unchecked(memidx));
    match vm_try!(shared.register_wait32(start, expected)) {
        AtomicWaitResult::NotEqual => {
            vm_try!(ctx.stack.push_i32(WAIT_RESULT_NOT_EQUAL));
            call_next(tail_code, 2, ctx)
        }
        AtomicWaitResult::Pending(wait) => {
            let resume_pc = tail_code.offset(2);
            let safepoint = cold_lookup_wait_safepoint(ctx, tail_code);
            let shared = ctx
                .gc
                .clone_shared_memory(ctx.shared_memory_id_at_unchecked(memidx));
            ctx.set_safepoint(safepoint);
            push_wait_effect(ctx, shared, wait, timeout_ns, resume_pc);
            trace!("waiting effect: {:?}", resume_pc);
            ctx.cont = resume_pc;
            VMResult::Success(())
        }
    }
}

/// WebAssembly `memory.atomic.wait32` on shared indexed memory with precomputed continuation.
///
/// Related spec:
/// - Threads: https://webassembly.github.io/threads/core/
///
/// Stack effect: `[i32, i32, i64] -> [i32]`.
/// Traps: traps on out-of-bounds access or unaligned access.
/// Notes: Uses instantiate-time precomputed indexed-memory metadata and resume continuation on the pending path.
///
/// # Safety
/// - `tail_code` must point to the metadata-bearing operand for this handler in the active instruction stream.
/// - `ctx` must reference a live execution context whose indexed memory slot is shared.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_memory_atomic_wait32_indexed_shared_precomputed(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let timeout_ns = ctx.stack.pop_i64();
    let expected = ctx.stack.pop_u32();
    let (start, memidx) = vm_try!(precomputed_wait_start_indexed(tail_code, ctx));
    let shared = ctx
        .gc
        .shared_memory(ctx.shared_memory_id_at_unchecked(memidx));
    match vm_try!(shared.register_wait32(start, expected)) {
        AtomicWaitResult::NotEqual => {
            vm_try!(ctx.stack.push_i32(WAIT_RESULT_NOT_EQUAL));
            call_next(tail_code, 2, ctx)
        }
        AtomicWaitResult::Pending(wait) => {
            let site = precomputed_wait_site_unchecked(tail_code);
            let safepoint = site.safepoint_cache();
            let shared = ctx
                .gc
                .clone_shared_memory(ctx.shared_memory_id_at_unchecked(memidx));
            ctx.set_safepoint(safepoint);
            push_wait_effect_precomputed(ctx, shared, wait, timeout_ns, site.resume_pc);
            ctx.cont = site.resume_pc.resolve_in_call_frame(ctx.current_frame);
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
    let _timeout_ns = ctx.stack.pop_i64();
    let _expected = ctx.stack.pop_u64();
    let _start = vm_try!(atomic_start(tail_code, ctx));
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
    let timeout_ns = ctx.stack.pop_i64();
    let expected = ctx.stack.pop_u64();
    let start = vm_try!(atomic_start(tail_code, ctx));
    let shared = ctx
        .gc
        .shared_memory(ctx.default_shared_memory_id_unchecked());
    match vm_try!(shared.register_wait64(start, expected)) {
        AtomicWaitResult::NotEqual => {
            vm_try!(ctx.stack.push_i32(WAIT_RESULT_NOT_EQUAL));
            call_next(tail_code, 1, ctx)
        }
        AtomicWaitResult::Pending(wait) => {
            let resume_pc = tail_code.offset(1);
            let safepoint = cold_lookup_wait_safepoint(ctx, tail_code);
            let shared = ctx
                .gc
                .clone_shared_memory(ctx.default_shared_memory_id_unchecked());
            ctx.set_safepoint(safepoint);
            push_wait_effect(ctx, shared, wait, timeout_ns, resume_pc);
            trace!("waiting effect: {:?}", resume_pc);
            ctx.cont = resume_pc;
            VMResult::Success(())
        }
    }
}

/// WebAssembly `memory.atomic.wait64` on shared default memory with precomputed continuation.
///
/// Related spec:
/// - Threads: https://webassembly.github.io/threads/core/
///
/// Stack effect: `[i32, i64, i64] -> [i32]`.
/// Traps: traps on out-of-bounds access or unaligned access.
/// Notes: Uses instantiate-time precomputed resume metadata to avoid rebuilding the continuation on the pending path.
///
/// # Safety
/// - `tail_code` must point to the metadata-bearing operand for this handler in the active instruction stream.
/// - `ctx` must reference a live execution context whose default memory is shared.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_memory_atomic_wait64_shared_precomputed(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let timeout_ns = ctx.stack.pop_i64();
    let expected = ctx.stack.pop_u64();
    let start = vm_try!(precomputed_wait_start(tail_code, ctx));
    let shared = ctx
        .gc
        .shared_memory(ctx.default_shared_memory_id_unchecked());
    match vm_try!(shared.register_wait64(start, expected)) {
        AtomicWaitResult::NotEqual => {
            vm_try!(ctx.stack.push_i32(WAIT_RESULT_NOT_EQUAL));
            call_next(tail_code, 1, ctx)
        }
        AtomicWaitResult::Pending(wait) => {
            let site = precomputed_wait_site_unchecked(tail_code);
            let safepoint = site.safepoint_cache();
            let shared = ctx
                .gc
                .clone_shared_memory(ctx.default_shared_memory_id_unchecked());
            ctx.set_safepoint(safepoint);
            push_wait_effect_precomputed(ctx, shared, wait, timeout_ns, site.resume_pc);
            ctx.cont = site.resume_pc.resolve_in_call_frame(ctx.current_frame);
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
    let _timeout_ns = ctx.stack.pop_i64();
    let _expected = ctx.stack.pop_u64();
    let _start = vm_try!(atomic_start_indexed(tail_code, ctx));
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
    let timeout_ns = ctx.stack.pop_i64();
    let expected = ctx.stack.pop_u64();
    let (start, memidx) = vm_try!(atomic_start_indexed(tail_code, ctx));
    let shared = ctx
        .gc
        .shared_memory(ctx.shared_memory_id_at_unchecked(memidx));
    match vm_try!(shared.register_wait64(start, expected)) {
        AtomicWaitResult::NotEqual => {
            vm_try!(ctx.stack.push_i32(WAIT_RESULT_NOT_EQUAL));
            call_next(tail_code, 2, ctx)
        }
        AtomicWaitResult::Pending(wait) => {
            let resume_pc = tail_code.offset(2);
            let safepoint = cold_lookup_wait_safepoint(ctx, tail_code);
            let shared = ctx
                .gc
                .clone_shared_memory(ctx.shared_memory_id_at_unchecked(memidx));
            ctx.set_safepoint(safepoint);
            push_wait_effect(ctx, shared, wait, timeout_ns, resume_pc);
            trace!("waiting effect: {:?}", resume_pc);
            ctx.cont = resume_pc;
            VMResult::Success(())
        }
    }
}

/// WebAssembly `memory.atomic.wait64` on shared indexed memory with precomputed continuation.
///
/// Related spec:
/// - Threads: https://webassembly.github.io/threads/core/
///
/// Stack effect: `[i32, i64, i64] -> [i32]`.
/// Traps: traps on out-of-bounds access or unaligned access.
/// Notes: Uses instantiate-time precomputed indexed-memory metadata and resume continuation on the pending path.
///
/// # Safety
/// - `tail_code` must point to the metadata-bearing operand for this handler in the active instruction stream.
/// - `ctx` must reference a live execution context whose indexed memory slot is shared.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_memory_atomic_wait64_indexed_shared_precomputed(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let timeout_ns = ctx.stack.pop_i64();
    let expected = ctx.stack.pop_u64();
    let (start, memidx) = vm_try!(precomputed_wait_start_indexed(tail_code, ctx));
    let shared = ctx
        .gc
        .shared_memory(ctx.shared_memory_id_at_unchecked(memidx));
    match vm_try!(shared.register_wait64(start, expected)) {
        AtomicWaitResult::NotEqual => {
            vm_try!(ctx.stack.push_i32(WAIT_RESULT_NOT_EQUAL));
            call_next(tail_code, 2, ctx)
        }
        AtomicWaitResult::Pending(wait) => {
            let site = precomputed_wait_site_unchecked(tail_code);
            let safepoint = site.safepoint_cache();
            let shared = ctx
                .gc
                .clone_shared_memory(ctx.shared_memory_id_at_unchecked(memidx));
            ctx.set_safepoint(safepoint);
            push_wait_effect_precomputed(ctx, shared, wait, timeout_ns, site.resume_pc);
            ctx.cont = site.resume_pc.resolve_in_call_frame(ctx.current_frame);
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
    ctx.gc
        .local_atomic_fence(ctx.default_local_memory_id_unchecked());
    call_next(tail_code, 1, ctx)
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
    ctx.gc
        .shared_atomic_fence(ctx.default_shared_memory_id_unchecked());
    call_next(tail_code, 1, ctx)
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
