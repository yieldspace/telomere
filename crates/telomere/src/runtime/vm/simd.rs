use crate::{
    common::stack::LaneType,
    runtime::vm::{
        compute_memory_offset, store_internal_local, store_internal_local_indexed,
        store_internal_shared, store_internal_shared_indexed, StoreBytes,
    },
};
use telomere_macros::define_simd_operation;
use wide::{f32x4, f64x2, i16x8, i32x4, i64x2, i8x16, u16x8, u32x4, u64x2, u8x16};

use crate::{
    common::{stack::StackOperation, ExecuteContext, Instr, Stack},
    runtime::vm::call_next,
    VMResult,
};

/// Telomere internal SIMD local-memory push helper.
///
/// Spec:
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: internal specialized SIMD memory helper.
/// Traps: propagates out-of-bounds memory traps from the local-memory access.
/// Notes: Uses the cached local default-memory id and never decodes a tagged `MemoryHandle`.
///
/// # Safety
/// - `ctx` must reference a live execution context whose active frame has local default memory.
/// - `start` must be the validated effective address for the current instruction.
/// - Callers must not retain borrows or guards across the tail-dispatch that follows this helper.
#[inline(always)]
unsafe fn push_memory_to_stack_local<const N: usize>(
    ctx: &mut ExecuteContext,
    start: usize,
) -> VMResult<()> {
    ctx.gc.local_push_memory_to_stack::<N>(
        ctx.default_local_memory_id_unchecked(),
        ctx.stack,
        start,
    )
}

/// Telomere internal SIMD shared-memory push helper.
///
/// Spec:
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: internal specialized SIMD memory helper.
/// Traps: propagates out-of-bounds memory traps from the shared-memory access.
/// Notes: Uses the cached shared default-memory id and never decodes a tagged `MemoryHandle`.
///
/// # Safety
/// - `ctx` must reference a live execution context whose active frame has shared default memory.
/// - `start` must be the validated effective address for the current instruction.
/// - Callers must not retain borrows or guards across the tail-dispatch that follows this helper.
#[inline(always)]
unsafe fn push_memory_to_stack_shared<const N: usize>(
    ctx: &mut ExecuteContext,
    start: usize,
) -> VMResult<()> {
    ctx.gc.shared_push_memory_to_stack::<N>(
        ctx.default_shared_memory_id_unchecked(),
        ctx.stack,
        start,
    )
}

#[inline(always)]
unsafe fn push_memory_to_stack_local_indexed<const N: usize>(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    start: usize,
) -> VMResult<()> {
    let memidx = (*tail_code.add(1)).operand.u32;
    ctx.gc.local_push_memory_to_stack::<N>(
        ctx.local_memory_id_at_unchecked(memidx),
        ctx.stack,
        start,
    )
}

#[inline(always)]
unsafe fn push_memory_to_stack_shared_indexed<const N: usize>(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    start: usize,
) -> VMResult<()> {
    let memidx = (*tail_code.add(1)).operand.u32;
    ctx.gc.shared_push_memory_to_stack::<N>(
        ctx.shared_memory_id_at_unchecked(memidx),
        ctx.stack,
        start,
    )
}

/// Telomere internal SIMD local-memory read helper.
///
/// Spec:
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: internal specialized SIMD memory helper.
/// Traps: traps on memory index overflow or out-of-bounds local-memory access.
/// Notes: Computes the effective address once and reads an unaligned byte array from local memory.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction stream for the current active frame.
/// - `ctx` must reference a live execution context whose active frame has local default memory.
/// - This helper must not retain borrows across the memory read.
#[inline(always)]
unsafe fn read_memory_bytes_local<const N: usize>(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<[u8; N]> {
    let memarg = (*tail_code).operand.memarg;
    let offset = ctx.stack.pop_u32();
    let start = vm_try!(compute_memory_offset(memarg, offset));
    ctx.gc
        .local_read_u8_array::<N>(ctx.default_local_memory_id_unchecked(), start)
}

/// Telomere internal SIMD shared-memory read helper.
///
/// Spec:
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: internal specialized SIMD memory helper.
/// Traps: traps on memory index overflow or out-of-bounds shared-memory access.
/// Notes: Computes the effective address once and reads an unaligned byte array from shared memory.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction stream for the current active frame.
/// - `ctx` must reference a live execution context whose active frame has shared default memory.
/// - This helper must not retain borrows across the memory read.
#[inline(always)]
unsafe fn read_memory_bytes_shared<const N: usize>(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<[u8; N]> {
    let memarg = (*tail_code).operand.memarg;
    let offset = ctx.stack.pop_u32();
    let start = vm_try!(compute_memory_offset(memarg, offset));
    ctx.gc
        .shared_read_u8_array::<N>(ctx.default_shared_memory_id_unchecked(), start)
}

#[inline(always)]
unsafe fn read_memory_bytes_local_indexed<const N: usize>(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<[u8; N]> {
    let memarg = (*tail_code).operand.memarg;
    let offset = ctx.stack.pop_u32();
    let start = vm_try!(compute_memory_offset(memarg, offset));
    let memidx = (*tail_code.add(1)).operand.u32;
    ctx.gc
        .local_read_u8_array::<N>(ctx.local_memory_id_at_unchecked(memidx), start)
}

#[inline(always)]
unsafe fn read_memory_bytes_shared_indexed<const N: usize>(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<[u8; N]> {
    let memarg = (*tail_code).operand.memarg;
    let offset = ctx.stack.pop_u32();
    let start = vm_try!(compute_memory_offset(memarg, offset));
    let memidx = (*tail_code.add(1)).operand.u32;
    ctx.gc
        .shared_read_u8_array::<N>(ctx.shared_memory_id_at_unchecked(memidx), start)
}

macro_rules! define_shared_simd_memory_handler {
    ($shared_name:ident, $mnemonic:literal, $impl:ident) => {
        #[doc = concat!("WebAssembly `", $mnemonic, "` on shared default memory.")]
        ///
        /// Spec:
        /// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
        /// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
        /// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
        ///
        /// Stack effect: specialized SIMD memory handler.
        /// Traps: traps on out-of-bounds memory access.
        /// Notes: Uses the shared-memory specialized fast path selected by the parser and tail-dispatches with `call_next`.
        ///
        /// # Safety
        /// - `tail_code` must point to the decoded instruction for this handler in the active function body.
        /// - `ctx` must reference a live execution context whose default memory is shared.
        /// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
        pub unsafe fn $shared_name(
            tail_code: *const Instr,
            ctx: &mut ExecuteContext,
        ) -> VMResult<()> {
            $impl::<true, false>(tail_code, ctx)
        }
    };
}

macro_rules! define_local_simd_memory_handler {
    ($name:ident, $mnemonic:literal, $impl:ident) => {
        #[doc = concat!("WebAssembly `", $mnemonic, "`.")]
        ///
        /// Spec:
        /// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
        /// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
        /// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
        ///
        /// Stack effect: specialized SIMD memory handler.
        /// Traps: traps on out-of-bounds memory access.
        /// Notes: Uses the local-memory specialized fast path selected by the parser and tail-dispatches with `call_next`.
        ///
        /// # Safety
        /// - `tail_code` must point to the decoded instruction for this handler in the active function body.
        /// - `ctx` must reference a live execution context whose default memory is local.
        /// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
        pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            $impl::<false, false>(tail_code, ctx)
        }
    };
}

macro_rules! define_indexed_shared_simd_memory_handler {
    ($shared_name:ident, $mnemonic:literal, $impl:ident) => {
        #[doc = concat!("WebAssembly `", $mnemonic, "` on shared indexed memory.")]
        ///
        /// Spec:
        /// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
        /// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
        /// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
        ///
        /// Stack effect: specialized SIMD memory handler.
        /// Traps: traps on out-of-bounds memory access.
        /// Notes: Uses the shared-memory specialized fast path selected by the parser for `memidx > 0`.
        ///
        /// # Safety
        /// - `tail_code` must point to the decoded instruction for this handler in the active function body.
        /// - `ctx` must reference a live execution context whose selected indexed memory is shared.
        /// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
        pub unsafe fn $shared_name(
            tail_code: *const Instr,
            ctx: &mut ExecuteContext,
        ) -> VMResult<()> {
            $impl::<true, true>(tail_code, ctx)
        }
    };
}

macro_rules! define_indexed_local_simd_memory_handler {
    ($name:ident, $mnemonic:literal, $impl:ident) => {
        #[doc = concat!("WebAssembly `", $mnemonic, "` on indexed local memory.")]
        ///
        /// Spec:
        /// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
        /// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
        /// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
        ///
        /// Stack effect: specialized SIMD memory handler.
        /// Traps: traps on out-of-bounds memory access.
        /// Notes: Uses the local-memory specialized fast path selected by the parser for `memidx > 0`.
        ///
        /// # Safety
        /// - `tail_code` must point to the decoded instruction for this handler in the active function body.
        /// - `ctx` must reference a live execution context whose selected indexed memory is local.
        /// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
        pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            $impl::<false, true>(tail_code, ctx)
        }
    };
}

/// Telomere internal SIMD handler implementation for `v128.load`.
///
/// Spec:
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: internal specialized SIMD handler.
/// Traps: traps on memory index overflow or out-of-bounds memory access.
/// Notes: `SHARED` selects the typed local/shared fast path chosen by the parser without decoding `MemoryHandle`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction stream for the current active frame.
/// - `ctx` must reference a live execution context whose active frame default memory matches `SHARED`.
/// - This helper must not keep borrows, locks, or guards alive across `call_next`.
#[inline(always)]
unsafe fn op_v128_load_impl<const SHARED: bool, const INDEXED: bool>(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let memarg = (*tail_code).operand.memarg;
    let offset = ctx.stack.pop_u32();
    let start = vm_try!(compute_memory_offset(memarg, offset));
    if SHARED && INDEXED {
        vm_try!(push_memory_to_stack_shared_indexed::<16>(
            tail_code, ctx, start
        ));
    } else if SHARED {
        vm_try!(push_memory_to_stack_shared::<16>(ctx, start));
    } else if INDEXED {
        vm_try!(push_memory_to_stack_local_indexed::<16>(
            tail_code, ctx, start
        ));
    } else {
        vm_try!(push_memory_to_stack_local::<16>(ctx, start));
    }
    call_next(tail_code, if INDEXED { 2 } else { 1 }, ctx)
}

define_local_simd_memory_handler!(op_v128_load, "v128.load", op_v128_load_impl);
define_shared_simd_memory_handler!(op_v128_load_shared, "v128.load", op_v128_load_impl);
define_indexed_local_simd_memory_handler!(
    op_v128_load_indexed_local,
    "v128.load",
    op_v128_load_impl
);
define_indexed_shared_simd_memory_handler!(
    op_v128_load_indexed_shared,
    "v128.load",
    op_v128_load_impl
);

#[inline(always)]
/// WebAssembly SIMD memory helper for lane-sized reads.
///
/// Spec:
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: internal SIMD memory helper.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Computes the effective address once and reads an unaligned byte array from the active memory.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction stream for the current active frame.
/// - `ctx` must reference a live execution context whose validated operand stack matches this SIMD memory operation.
/// - This helper must not retain borrows across the memory read or the follow-up stack push.
unsafe fn read_memory_bytes<const N: usize, const SHARED: bool, const INDEXED: bool>(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<[u8; N]> {
    if SHARED && INDEXED {
        read_memory_bytes_shared_indexed::<N>(tail_code, ctx)
    } else if SHARED {
        read_memory_bytes_shared::<N>(tail_code, ctx)
    } else if INDEXED {
        read_memory_bytes_local_indexed::<N>(tail_code, ctx)
    } else {
        read_memory_bytes_local::<N>(tail_code, ctx)
    }
}
/// WebAssembly `v128.load8x8_s`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[i32] -> [v128]`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Implements the validated SIMD semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
/// - `SHARED` must match the default memory kind selected for this handler.
unsafe fn v128_load8x8_s_impl<const SHARED: bool, const INDEXED: bool>(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let data = vm_try!(read_memory_bytes::<8, SHARED, INDEXED>(tail_code, ctx));
    let extended = [
        data[0] as i8 as i16,
        data[1] as i8 as i16,
        data[2] as i8 as i16,
        data[3] as i8 as i16,
        data[4] as i8 as i16,
        data[5] as i8 as i16,
        data[6] as i8 as i16,
        data[7] as i8 as i16,
    ];
    vm_try!(ctx.stack.push(i16x8::from(extended)));
    call_next(tail_code, if INDEXED { 2 } else { 1 }, ctx)
}
define_local_simd_memory_handler!(v128_load8x8_s, "v128.load8x8_s", v128_load8x8_s_impl);
define_shared_simd_memory_handler!(v128_load8x8_s_shared, "v128.load8x8_s", v128_load8x8_s_impl);
define_indexed_local_simd_memory_handler!(
    v128_load8x8_s_indexed_local,
    "v128.load8x8_s",
    v128_load8x8_s_impl
);
define_indexed_shared_simd_memory_handler!(
    v128_load8x8_s_indexed_shared,
    "v128.load8x8_s",
    v128_load8x8_s_impl
);
/// WebAssembly `v128.load8x8_u`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[i32] -> [v128]`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Implements the validated SIMD semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
/// - `SHARED` must match the default memory kind selected for this handler.
unsafe fn v128_load8x8_u_impl<const SHARED: bool, const INDEXED: bool>(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let data = vm_try!(read_memory_bytes::<8, SHARED, INDEXED>(tail_code, ctx));
    let extended = [
        data[0] as u16,
        data[1] as u16,
        data[2] as u16,
        data[3] as u16,
        data[4] as u16,
        data[5] as u16,
        data[6] as u16,
        data[7] as u16,
    ];
    vm_try!(ctx.stack.push(u16x8::from(extended)));
    call_next(tail_code, if INDEXED { 2 } else { 1 }, ctx)
}
define_local_simd_memory_handler!(v128_load8x8_u, "v128.load8x8_u", v128_load8x8_u_impl);
define_shared_simd_memory_handler!(v128_load8x8_u_shared, "v128.load8x8_u", v128_load8x8_u_impl);
define_indexed_local_simd_memory_handler!(
    v128_load8x8_u_indexed_local,
    "v128.load8x8_u",
    v128_load8x8_u_impl
);
define_indexed_shared_simd_memory_handler!(
    v128_load8x8_u_indexed_shared,
    "v128.load8x8_u",
    v128_load8x8_u_impl
);

/// WebAssembly `v128.load16x4_s`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[i32] -> [v128]`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Implements the validated SIMD semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
unsafe fn v128_load16x4_s_impl<const SHARED: bool, const INDEXED: bool>(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let data = vm_try!(read_memory_bytes::<8, SHARED, INDEXED>(tail_code, ctx));
    let i16s = [
        i16::from_le_bytes([data[0], data[1]]),
        i16::from_le_bytes([data[2], data[3]]),
        i16::from_le_bytes([data[4], data[5]]),
        i16::from_le_bytes([data[6], data[7]]),
    ];

    let extended = [
        i16s[0] as i32,
        i16s[1] as i32,
        i16s[2] as i32,
        i16s[3] as i32,
    ];
    vm_try!(ctx.stack.push(i32x4::from(extended)));
    call_next(tail_code, if INDEXED { 2 } else { 1 }, ctx)
}
define_local_simd_memory_handler!(v128_load16x4_s, "v128.load16x4_s", v128_load16x4_s_impl);
define_shared_simd_memory_handler!(
    v128_load16x4_s_shared,
    "v128.load16x4_s",
    v128_load16x4_s_impl
);
define_indexed_local_simd_memory_handler!(
    v128_load16x4_s_indexed_local,
    "v128.load16x4_s",
    v128_load16x4_s_impl
);
define_indexed_shared_simd_memory_handler!(
    v128_load16x4_s_indexed_shared,
    "v128.load16x4_s",
    v128_load16x4_s_impl
);
/// WebAssembly `v128.load16x4_u`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[i32] -> [v128]`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Implements the validated SIMD semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
unsafe fn v128_load16x4_u_impl<const SHARED: bool, const INDEXED: bool>(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let data = vm_try!(read_memory_bytes::<8, SHARED, INDEXED>(tail_code, ctx));
    let u16s = [
        u16::from_le_bytes([data[0], data[1]]),
        u16::from_le_bytes([data[2], data[3]]),
        u16::from_le_bytes([data[4], data[5]]),
        u16::from_le_bytes([data[6], data[7]]),
    ];

    let extended = [
        u16s[0] as u32,
        u16s[1] as u32,
        u16s[2] as u32,
        u16s[3] as u32,
    ];
    vm_try!(ctx.stack.push(u32x4::from(extended)));
    call_next(tail_code, if INDEXED { 2 } else { 1 }, ctx)
}
define_local_simd_memory_handler!(v128_load16x4_u, "v128.load16x4_u", v128_load16x4_u_impl);
define_shared_simd_memory_handler!(
    v128_load16x4_u_shared,
    "v128.load16x4_u",
    v128_load16x4_u_impl
);
define_indexed_local_simd_memory_handler!(
    v128_load16x4_u_indexed_local,
    "v128.load16x4_u",
    v128_load16x4_u_impl
);
define_indexed_shared_simd_memory_handler!(
    v128_load16x4_u_indexed_shared,
    "v128.load16x4_u",
    v128_load16x4_u_impl
);

/// WebAssembly `v128.store`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[i32, v128] -> []`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Implements the validated SIMD semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn v128_store(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    store_internal_local(tail_code, ctx, "v128_store", |ctx| {
        StoreBytes::Write16(ctx.stack.pop_u128().to_le_bytes())
    })
}

/// WebAssembly `v128.store` on indexed local memory.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/multi-memory/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/multi-memory/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[i32, v128] -> []`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Uses the indexed local-memory SIMD fast path selected by the parser for `memidx > 0`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose selected indexed memory is local.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn v128_store_indexed_local(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    store_internal_local_indexed(tail_code, ctx, "v128_store_indexed_local", |ctx| {
        StoreBytes::Write16(ctx.stack.pop_u128().to_le_bytes())
    })
}

define_shared_simd_memory_handler!(v128_store_shared, "v128.store", v128_store_shared_impl);

#[inline(always)]
/// Telomere internal SIMD handler implementation for `v128.store` on shared memory.
///
/// Spec:
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: internal specialized SIMD handler.
/// Traps: traps on memory index overflow or out-of-bounds shared-memory access.
/// Notes: Materializes the store payload first and then writes through the typed shared-memory fast path.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction stream for the current active frame.
/// - `ctx` must reference a live execution context whose active frame has shared default memory.
/// - This helper must not keep borrows, locks, or guards alive across `call_next`.
unsafe fn v128_store_shared_impl<const SHARED: bool, const INDEXED: bool>(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    debug_assert!(SHARED);
    debug_assert!(!INDEXED);
    store_internal_shared(tail_code, ctx, "v128_store_shared", |ctx| {
        StoreBytes::Write16(ctx.stack.pop_u128().to_le_bytes())
    })
}

/// WebAssembly `v128.store` on indexed shared memory.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/multi-memory/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/multi-memory/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/threads/core/exec/instructions.html
///
/// Stack effect: `[i32, v128] -> []`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Uses the indexed shared-memory SIMD fast path selected by the parser for `memidx > 0`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose selected indexed memory is shared.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn v128_store_indexed_shared(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    store_internal_shared_indexed(tail_code, ctx, "v128_store_indexed_shared", |ctx| {
        StoreBytes::Write16(ctx.stack.pop_u128().to_le_bytes())
    })
}

/// WebAssembly `v128.load32x2_s`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[i32] -> [v128]`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Implements the validated SIMD semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
unsafe fn v128_load32x2_s_impl<const SHARED: bool, const INDEXED: bool>(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let bytes = vm_try!(read_memory_bytes::<8, SHARED, INDEXED>(tail_code, ctx));
    let i32s = [
        i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
        i32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
    ];
    let extended = [i32s[0] as i64, i32s[1] as i64];
    vm_try!(ctx.stack.push(i64x2::from(extended)));
    call_next(tail_code, if INDEXED { 2 } else { 1 }, ctx)
}
define_local_simd_memory_handler!(v128_load32x2_s, "v128.load32x2_s", v128_load32x2_s_impl);
define_shared_simd_memory_handler!(
    v128_load32x2_s_shared,
    "v128.load32x2_s",
    v128_load32x2_s_impl
);
define_indexed_local_simd_memory_handler!(
    v128_load32x2_s_indexed_local,
    "v128.load32x2_s",
    v128_load32x2_s_impl
);
define_indexed_shared_simd_memory_handler!(
    v128_load32x2_s_indexed_shared,
    "v128.load32x2_s",
    v128_load32x2_s_impl
);
/// WebAssembly `v128.load32x2_u`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[i32] -> [v128]`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Implements the validated SIMD semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
unsafe fn v128_load32x2_u_impl<const SHARED: bool, const INDEXED: bool>(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let bytes = vm_try!(read_memory_bytes::<8, SHARED, INDEXED>(tail_code, ctx));
    let u32s = [
        u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
        u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
    ];
    let extended = [u32s[0] as u64, u32s[1] as u64];
    vm_try!(ctx.stack.push(u64x2::from(extended)));
    call_next(tail_code, if INDEXED { 2 } else { 1 }, ctx)
}
define_local_simd_memory_handler!(v128_load32x2_u, "v128.load32x2_u", v128_load32x2_u_impl);
define_shared_simd_memory_handler!(
    v128_load32x2_u_shared,
    "v128.load32x2_u",
    v128_load32x2_u_impl
);
define_indexed_local_simd_memory_handler!(
    v128_load32x2_u_indexed_local,
    "v128.load32x2_u",
    v128_load32x2_u_impl
);
define_indexed_shared_simd_memory_handler!(
    v128_load32x2_u_indexed_shared,
    "v128.load32x2_u",
    v128_load32x2_u_impl
);

/// WebAssembly `v128.load8_splat`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[i32] -> [v128]`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Implements the validated SIMD semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
unsafe fn v128_load8_splat_impl<const SHARED: bool, const INDEXED: bool>(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let bytes = vm_try!(read_memory_bytes::<1, SHARED, INDEXED>(tail_code, ctx));
    vm_try!(ctx.stack.push(i8x16::from(bytes[0] as i8)));
    call_next(tail_code, if INDEXED { 2 } else { 1 }, ctx)
}
define_local_simd_memory_handler!(v128_load8_splat, "v128.load8_splat", v128_load8_splat_impl);
define_shared_simd_memory_handler!(
    v128_load8_splat_shared,
    "v128.load8_splat",
    v128_load8_splat_impl
);
define_indexed_local_simd_memory_handler!(
    v128_load8_splat_indexed_local,
    "v128.load8_splat",
    v128_load8_splat_impl
);
define_indexed_shared_simd_memory_handler!(
    v128_load8_splat_indexed_shared,
    "v128.load8_splat",
    v128_load8_splat_impl
);

/// WebAssembly `v128.load16_splat`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[i32] -> [v128]`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Implements the validated SIMD semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
unsafe fn v128_load16_splat_impl<const SHARED: bool, const INDEXED: bool>(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let bytes = vm_try!(read_memory_bytes::<2, SHARED, INDEXED>(tail_code, ctx));
    vm_try!(ctx
        .stack
        .push(i16x8::from(i16::from_le_bytes([bytes[0], bytes[1]]))));
    call_next(tail_code, if INDEXED { 2 } else { 1 }, ctx)
}
define_local_simd_memory_handler!(
    v128_load16_splat,
    "v128.load16_splat",
    v128_load16_splat_impl
);
define_shared_simd_memory_handler!(
    v128_load16_splat_shared,
    "v128.load16_splat",
    v128_load16_splat_impl
);
define_indexed_local_simd_memory_handler!(
    v128_load16_splat_indexed_local,
    "v128.load16_splat",
    v128_load16_splat_impl
);
define_indexed_shared_simd_memory_handler!(
    v128_load16_splat_indexed_shared,
    "v128.load16_splat",
    v128_load16_splat_impl
);

/// WebAssembly `v128.load32_splat`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[i32] -> [v128]`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Implements the validated SIMD semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
unsafe fn v128_load32_splat_impl<const SHARED: bool, const INDEXED: bool>(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let bytes = vm_try!(read_memory_bytes::<4, SHARED, INDEXED>(tail_code, ctx));
    vm_try!(ctx.stack.push(i32x4::from(i32::from_le_bytes([
        bytes[0], bytes[1], bytes[2], bytes[3],
    ]))));
    call_next(tail_code, if INDEXED { 2 } else { 1 }, ctx)
}
define_local_simd_memory_handler!(
    v128_load32_splat,
    "v128.load32_splat",
    v128_load32_splat_impl
);
define_shared_simd_memory_handler!(
    v128_load32_splat_shared,
    "v128.load32_splat",
    v128_load32_splat_impl
);
define_indexed_local_simd_memory_handler!(
    v128_load32_splat_indexed_local,
    "v128.load32_splat",
    v128_load32_splat_impl
);
define_indexed_shared_simd_memory_handler!(
    v128_load32_splat_indexed_shared,
    "v128.load32_splat",
    v128_load32_splat_impl
);

/// WebAssembly `v128.load64_splat`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[i32] -> [v128]`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Implements the validated SIMD semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
unsafe fn v128_load64_splat_impl<const SHARED: bool, const INDEXED: bool>(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let bytes = vm_try!(read_memory_bytes::<8, SHARED, INDEXED>(tail_code, ctx));
    vm_try!(ctx.stack.push(i64x2::from(i64::from_le_bytes([
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
    ]))));
    call_next(tail_code, if INDEXED { 2 } else { 1 }, ctx)
}
define_local_simd_memory_handler!(
    v128_load64_splat,
    "v128.load64_splat",
    v128_load64_splat_impl
);
define_shared_simd_memory_handler!(
    v128_load64_splat_shared,
    "v128.load64_splat",
    v128_load64_splat_impl
);
define_indexed_local_simd_memory_handler!(
    v128_load64_splat_indexed_local,
    "v128.load64_splat",
    v128_load64_splat_impl
);
define_indexed_shared_simd_memory_handler!(
    v128_load64_splat_indexed_shared,
    "v128.load64_splat",
    v128_load64_splat_impl
);

/// WebAssembly `v128.const`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[v128] -> [v128]`.
/// Traps: none.
/// Notes: Implements the validated SIMD semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn v128_const(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let left_buf = &(*tail_code).operand.encoded;
    let right_buf = &(*tail_code.add(1)).operand.encoded;
    let mut buf = [0u8; 16];
    buf[0..8].copy_from_slice(left_buf);
    buf[8..16].copy_from_slice(right_buf);

    vm_try!(ctx.stack.push_u128(u128::from_le_bytes(buf)));
    call_next(tail_code, 2, ctx)
}

fn replace_lane_bytes<const N: usize>(bytes: &mut [u8; 16], lane: usize, value: [u8; N]) {
    let start = lane * N;
    bytes[start..start + N].copy_from_slice(&value);
}

/// WebAssembly SIMD lane-load helper.
///
/// Spec:
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: internal SIMD lane-load helper.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Reads a lane-aligned byte slice from memory and materializes it into the stack element type.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction stream for the current active frame.
/// - `ctx` must reference a live execution context whose validated stack and memory layout satisfy this instruction.
/// - This helper must not keep borrows, locks, or guards alive across the call into `call_next`.
unsafe fn load_lane_internal<const N: usize, const SHARED: bool, const INDEXED: bool>(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lane = (*tail_code.add(1)).operand.u32 as usize;
    let mut bytes = ctx.stack.pop_u128().to_le_bytes();
    let data = vm_try!(read_memory_bytes::<N, SHARED, INDEXED>(tail_code, ctx));
    replace_lane_bytes::<N>(&mut bytes, lane, data);
    vm_try!(ctx.stack.push_u128(u128::from_le_bytes(bytes)));
    call_next(tail_code, if INDEXED { 3 } else { 2 }, ctx)
}

/// WebAssembly SIMD lane-store helper.
///
/// Spec:
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: internal SIMD lane-store helper.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Materializes the lane payload first, then performs the memory write through the shared store helper.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction stream for the current active frame.
/// - `ctx` must reference a live execution context whose validated stack and memory layout satisfy this instruction.
/// - This helper must not keep borrows, locks, or guards alive across `call_next`.
unsafe fn store_lane_internal<const N: usize, const SHARED: bool, const INDEXED: bool>(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lane = (*tail_code.add(1)).operand.u32 as usize;
    let bytes = ctx.stack.pop_u128().to_le_bytes();
    let start = lane * N;
    let offset = ctx.stack.pop_u32();
    let mem_start = vm_try!(compute_memory_offset((*tail_code).operand.memarg, offset));
    if SHARED && INDEXED {
        let memidx = (*tail_code.add(2)).operand.u32;
        vm_try!(ctx.gc.shared_write_bytes(
            ctx.shared_memory_id_at_unchecked(memidx),
            mem_start,
            &bytes[start..start + N],
        ));
    } else if SHARED {
        vm_try!(ctx.gc.shared_write_bytes(
            ctx.default_shared_memory_id_unchecked(),
            mem_start,
            &bytes[start..start + N],
        ));
    } else if INDEXED {
        let memidx = (*tail_code.add(2)).operand.u32;
        vm_try!(ctx.gc.local_write_bytes(
            ctx.local_memory_id_at_unchecked(memidx),
            mem_start,
            &bytes[start..start + N],
        ));
    } else {
        vm_try!(ctx.gc.local_write_bytes(
            ctx.default_local_memory_id_unchecked(),
            mem_start,
            &bytes[start..start + N],
        ));
    }
    call_next(tail_code, if INDEXED { 3 } else { 2 }, ctx)
}

/// WebAssembly `i8x16.shuffle`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[v128, v128] -> [v128]`.
/// Traps: none.
/// Notes: Implements the validated SIMD semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn i8x16_shuffle(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let mut lanes = [0u8; 16];
    lanes[0..8].copy_from_slice(&(*tail_code).operand.encoded);
    lanes[8..16].copy_from_slice(&(*tail_code.add(1)).operand.encoded);
    let b = ctx.stack.pop_u128().to_le_bytes();
    let a = ctx.stack.pop_u128().to_le_bytes();
    let mut result = [0u8; 16];
    for (i, lane) in lanes.into_iter().enumerate() {
        let lane = lane as usize;
        result[i] = if lane < 16 { a[lane] } else { b[lane - 16] };
    }
    vm_try!(ctx.stack.push_u128(u128::from_le_bytes(result)));
    call_next(tail_code, 2, ctx)
}

/// WebAssembly `i8x16.splat`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[scalar] -> [v128]`.
/// Traps: none.
/// Notes: Implements the validated SIMD semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn i8x16_splat(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let value = ctx.stack.pop_i32() as i8;
    vm_try!(ctx.stack.push(i8x16::from(value)));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `i16x8.splat`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[scalar] -> [v128]`.
/// Traps: none.
/// Notes: Implements the validated SIMD semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn i16x8_splat(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let value = ctx.stack.pop_i32() as i16;
    vm_try!(ctx.stack.push(i16x8::from(value)));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `i32x4.splat`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[scalar] -> [v128]`.
/// Traps: none.
/// Notes: Implements the validated SIMD semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn i32x4_splat(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let value = ctx.stack.pop_i32();
    vm_try!(ctx.stack.push(i32x4::from(value)));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `i64x2.splat`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[scalar] -> [v128]`.
/// Traps: none.
/// Notes: Implements the validated SIMD semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn i64x2_splat(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let value = ctx.stack.pop_i64();
    vm_try!(ctx.stack.push(i64x2::from(value)));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `f32x4.splat`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[scalar] -> [v128]`.
/// Traps: none.
/// Notes: Implements the validated SIMD semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn f32x4_splat(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let value = ctx.stack.pop_f32();
    vm_try!(ctx.stack.push(f32x4::from(value)));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `f64x2.splat`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[scalar] -> [v128]`.
/// Traps: none.
/// Notes: Implements the validated SIMD semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn f64x2_splat(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let value = ctx.stack.pop_f64();
    vm_try!(ctx.stack.push(f64x2::from(value)));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `i8x16.extract_lane_s`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[v128] -> [scalar]`.
/// Traps: none.
/// Notes: Implements the validated SIMD semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i8x16_extract_lane_s(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lane = (*tail_code).operand.u32;
    let v = ctx.stack.pop_u128();
    let bytes = v.to_le_bytes();
    let value = bytes[lane as usize] as i8 as i32;
    vm_try!(ctx.stack.push_i32(value));
    call_next(tail_code, 1, ctx)
}

/// WebAssembly `i8x16.extract_lane_u`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[v128] -> [scalar]`.
/// Traps: none.
/// Notes: Implements the validated SIMD semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn i8x16_extract_lane_u(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lane = (*tail_code).operand.u32 as usize;
    let bytes = ctx.stack.pop_u128().to_le_bytes();
    vm_try!(ctx.stack.push_i32(bytes[lane] as i32));
    call_next(tail_code, 1, ctx)
}

/// WebAssembly `i8x16.replace_lane`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[v128, scalar] -> [v128]`.
/// Traps: none.
/// Notes: Implements the validated SIMD semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn i8x16_replace_lane(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lane = (*tail_code).operand.u32 as usize;
    let value = (ctx.stack.pop_i32() as i8).to_le_bytes();
    let mut bytes = ctx.stack.pop_u128().to_le_bytes();
    replace_lane_bytes(&mut bytes, lane, value);
    vm_try!(ctx.stack.push_u128(u128::from_le_bytes(bytes)));
    call_next(tail_code, 1, ctx)
}

/// WebAssembly `i16x8.extract_lane_s`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[v128] -> [scalar]`.
/// Traps: none.
/// Notes: Implements the validated SIMD semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn i16x8_extract_lane_s(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lane = (*tail_code).operand.u32 as usize * 2;
    let bytes = ctx.stack.pop_u128().to_le_bytes();
    vm_try!(ctx
        .stack
        .push_i32(i16::from_le_bytes([bytes[lane], bytes[lane + 1]]) as i32));
    call_next(tail_code, 1, ctx)
}

/// WebAssembly `i16x8.extract_lane_u`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[v128] -> [scalar]`.
/// Traps: none.
/// Notes: Implements the validated SIMD semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn i16x8_extract_lane_u(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lane = (*tail_code).operand.u32 as usize * 2;
    let bytes = ctx.stack.pop_u128().to_le_bytes();
    vm_try!(ctx
        .stack
        .push_i32(u16::from_le_bytes([bytes[lane], bytes[lane + 1]]) as i32));
    call_next(tail_code, 1, ctx)
}

/// WebAssembly `i16x8.replace_lane`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[v128, scalar] -> [v128]`.
/// Traps: none.
/// Notes: Implements the validated SIMD semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn i16x8_replace_lane(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lane = (*tail_code).operand.u32 as usize;
    let value = (ctx.stack.pop_i32() as i16).to_le_bytes();
    let mut bytes = ctx.stack.pop_u128().to_le_bytes();
    replace_lane_bytes(&mut bytes, lane, value);
    vm_try!(ctx.stack.push_u128(u128::from_le_bytes(bytes)));
    call_next(tail_code, 1, ctx)
}

/// WebAssembly `i32x4.extract_lane`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[v128] -> [scalar]`.
/// Traps: none.
/// Notes: Implements the validated SIMD semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn i32x4_extract_lane(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lane = (*tail_code).operand.u32 as usize * 4;
    let bytes = ctx.stack.pop_u128().to_le_bytes();
    vm_try!(ctx.stack.push_i32(i32::from_le_bytes([
        bytes[lane],
        bytes[lane + 1],
        bytes[lane + 2],
        bytes[lane + 3],
    ])));
    call_next(tail_code, 1, ctx)
}

/// WebAssembly `i32x4.replace_lane`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[v128, scalar] -> [v128]`.
/// Traps: none.
/// Notes: Implements the validated SIMD semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn i32x4_replace_lane(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lane = (*tail_code).operand.u32 as usize;
    let value = ctx.stack.pop_i32().to_le_bytes();
    let mut bytes = ctx.stack.pop_u128().to_le_bytes();
    replace_lane_bytes(&mut bytes, lane, value);
    vm_try!(ctx.stack.push_u128(u128::from_le_bytes(bytes)));
    call_next(tail_code, 1, ctx)
}

/// WebAssembly `i64x2.extract_lane`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[v128] -> [scalar]`.
/// Traps: none.
/// Notes: Implements the validated SIMD semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn i64x2_extract_lane(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lane = (*tail_code).operand.u32 as usize * 8;
    let bytes = ctx.stack.pop_u128().to_le_bytes();
    vm_try!(ctx.stack.push_i64(i64::from_le_bytes([
        bytes[lane],
        bytes[lane + 1],
        bytes[lane + 2],
        bytes[lane + 3],
        bytes[lane + 4],
        bytes[lane + 5],
        bytes[lane + 6],
        bytes[lane + 7],
    ])));
    call_next(tail_code, 1, ctx)
}

/// WebAssembly `i64x2.replace_lane`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[v128, scalar] -> [v128]`.
/// Traps: none.
/// Notes: Implements the validated SIMD semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn i64x2_replace_lane(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lane = (*tail_code).operand.u32 as usize;
    let value = ctx.stack.pop_i64().to_le_bytes();
    let mut bytes = ctx.stack.pop_u128().to_le_bytes();
    replace_lane_bytes(&mut bytes, lane, value);
    vm_try!(ctx.stack.push_u128(u128::from_le_bytes(bytes)));
    call_next(tail_code, 1, ctx)
}

/// WebAssembly `f32x4.extract_lane`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[v128] -> [scalar]`.
/// Traps: none.
/// Notes: Implements the validated SIMD semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn f32x4_extract_lane(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lane = (*tail_code).operand.u32 as usize * 4;
    let bytes = ctx.stack.pop_u128().to_le_bytes();
    vm_try!(ctx.stack.push_f32(f32::from_le_bytes([
        bytes[lane],
        bytes[lane + 1],
        bytes[lane + 2],
        bytes[lane + 3],
    ])));
    call_next(tail_code, 1, ctx)
}

/// WebAssembly `f32x4.replace_lane`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[v128, scalar] -> [v128]`.
/// Traps: none.
/// Notes: Implements the validated SIMD semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn f32x4_replace_lane(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lane = (*tail_code).operand.u32 as usize;
    let value = ctx.stack.pop_f32().to_le_bytes();
    let mut bytes = ctx.stack.pop_u128().to_le_bytes();
    replace_lane_bytes(&mut bytes, lane, value);
    vm_try!(ctx.stack.push_u128(u128::from_le_bytes(bytes)));
    call_next(tail_code, 1, ctx)
}

/// WebAssembly `f64x2.extract_lane`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[v128] -> [scalar]`.
/// Traps: none.
/// Notes: Implements the validated SIMD semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn f64x2_extract_lane(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lane = (*tail_code).operand.u32 as usize * 8;
    let bytes = ctx.stack.pop_u128().to_le_bytes();
    vm_try!(ctx.stack.push_f64(f64::from_le_bytes([
        bytes[lane],
        bytes[lane + 1],
        bytes[lane + 2],
        bytes[lane + 3],
        bytes[lane + 4],
        bytes[lane + 5],
        bytes[lane + 6],
        bytes[lane + 7],
    ])));
    call_next(tail_code, 1, ctx)
}

/// WebAssembly `f64x2.replace_lane`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[v128, scalar] -> [v128]`.
/// Traps: none.
/// Notes: Implements the validated SIMD semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn f64x2_replace_lane(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lane = (*tail_code).operand.u32 as usize;
    let value = ctx.stack.pop_f64().to_le_bytes();
    let mut bytes = ctx.stack.pop_u128().to_le_bytes();
    replace_lane_bytes(&mut bytes, lane, value);
    vm_try!(ctx.stack.push_u128(u128::from_le_bytes(bytes)));
    call_next(tail_code, 1, ctx)
}

define_local_simd_memory_handler!(v128_load8_lane, "v128.load8_lane", v128_load8_lane_impl);
define_shared_simd_memory_handler!(
    v128_load8_lane_shared,
    "v128.load8_lane",
    v128_load8_lane_impl
);

#[inline(always)]
/// Telomere internal SIMD handler implementation for `v128.load8_lane`.
///
/// Spec:
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: internal specialized SIMD handler.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Delegates to the typed lane-load fast path selected by `SHARED`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction stream for the current active frame.
/// - `ctx` must reference a live execution context whose active frame default memory matches `SHARED`.
/// - This helper must not keep borrows, locks, or guards alive across `call_next`.
unsafe fn v128_load8_lane_impl<const SHARED: bool, const INDEXED: bool>(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    load_lane_internal::<1, SHARED, INDEXED>(tail_code, ctx)
}
define_indexed_local_simd_memory_handler!(
    v128_load8_lane_indexed_local,
    "v128.load8_lane",
    v128_load8_lane_impl
);
define_indexed_shared_simd_memory_handler!(
    v128_load8_lane_indexed_shared,
    "v128.load8_lane",
    v128_load8_lane_impl
);

define_local_simd_memory_handler!(v128_load16_lane, "v128.load16_lane", v128_load16_lane_impl);
define_shared_simd_memory_handler!(
    v128_load16_lane_shared,
    "v128.load16_lane",
    v128_load16_lane_impl
);

#[inline(always)]
/// Telomere internal SIMD handler implementation for `v128.load16_lane`.
///
/// Spec:
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: internal specialized SIMD handler.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Delegates to the typed lane-load fast path selected by `SHARED`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction stream for the current active frame.
/// - `ctx` must reference a live execution context whose active frame default memory matches `SHARED`.
/// - This helper must not keep borrows, locks, or guards alive across `call_next`.
unsafe fn v128_load16_lane_impl<const SHARED: bool, const INDEXED: bool>(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    load_lane_internal::<2, SHARED, INDEXED>(tail_code, ctx)
}
define_indexed_local_simd_memory_handler!(
    v128_load16_lane_indexed_local,
    "v128.load16_lane",
    v128_load16_lane_impl
);
define_indexed_shared_simd_memory_handler!(
    v128_load16_lane_indexed_shared,
    "v128.load16_lane",
    v128_load16_lane_impl
);

define_local_simd_memory_handler!(v128_load32_lane, "v128.load32_lane", v128_load32_lane_impl);
define_shared_simd_memory_handler!(
    v128_load32_lane_shared,
    "v128.load32_lane",
    v128_load32_lane_impl
);

#[inline(always)]
/// Telomere internal SIMD handler implementation for `v128.load32_lane`.
///
/// Spec:
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: internal specialized SIMD handler.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Delegates to the typed lane-load fast path selected by `SHARED`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction stream for the current active frame.
/// - `ctx` must reference a live execution context whose active frame default memory matches `SHARED`.
/// - This helper must not keep borrows, locks, or guards alive across `call_next`.
unsafe fn v128_load32_lane_impl<const SHARED: bool, const INDEXED: bool>(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    load_lane_internal::<4, SHARED, INDEXED>(tail_code, ctx)
}
define_indexed_local_simd_memory_handler!(
    v128_load32_lane_indexed_local,
    "v128.load32_lane",
    v128_load32_lane_impl
);
define_indexed_shared_simd_memory_handler!(
    v128_load32_lane_indexed_shared,
    "v128.load32_lane",
    v128_load32_lane_impl
);

define_local_simd_memory_handler!(v128_load64_lane, "v128.load64_lane", v128_load64_lane_impl);
define_shared_simd_memory_handler!(
    v128_load64_lane_shared,
    "v128.load64_lane",
    v128_load64_lane_impl
);

#[inline(always)]
/// Telomere internal SIMD handler implementation for `v128.load64_lane`.
///
/// Spec:
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: internal specialized SIMD handler.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Delegates to the typed lane-load fast path selected by `SHARED`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction stream for the current active frame.
/// - `ctx` must reference a live execution context whose active frame default memory matches `SHARED`.
/// - This helper must not keep borrows, locks, or guards alive across `call_next`.
unsafe fn v128_load64_lane_impl<const SHARED: bool, const INDEXED: bool>(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    load_lane_internal::<8, SHARED, INDEXED>(tail_code, ctx)
}
define_indexed_local_simd_memory_handler!(
    v128_load64_lane_indexed_local,
    "v128.load64_lane",
    v128_load64_lane_impl
);
define_indexed_shared_simd_memory_handler!(
    v128_load64_lane_indexed_shared,
    "v128.load64_lane",
    v128_load64_lane_impl
);

define_local_simd_memory_handler!(v128_store8_lane, "v128.store8_lane", v128_store8_lane_impl);
define_shared_simd_memory_handler!(
    v128_store8_lane_shared,
    "v128.store8_lane",
    v128_store8_lane_impl
);

#[inline(always)]
/// Telomere internal SIMD handler implementation for `v128.store8_lane`.
///
/// Spec:
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: internal specialized SIMD handler.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Delegates to the typed lane-store fast path selected by `SHARED`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction stream for the current active frame.
/// - `ctx` must reference a live execution context whose active frame default memory matches `SHARED`.
/// - This helper must not keep borrows, locks, or guards alive across `call_next`.
unsafe fn v128_store8_lane_impl<const SHARED: bool, const INDEXED: bool>(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    store_lane_internal::<1, SHARED, INDEXED>(tail_code, ctx)
}
define_indexed_local_simd_memory_handler!(
    v128_store8_lane_indexed_local,
    "v128.store8_lane",
    v128_store8_lane_impl
);
define_indexed_shared_simd_memory_handler!(
    v128_store8_lane_indexed_shared,
    "v128.store8_lane",
    v128_store8_lane_impl
);

define_local_simd_memory_handler!(
    v128_store16_lane,
    "v128.store16_lane",
    v128_store16_lane_impl
);
define_shared_simd_memory_handler!(
    v128_store16_lane_shared,
    "v128.store16_lane",
    v128_store16_lane_impl
);

#[inline(always)]
/// Telomere internal SIMD handler implementation for `v128.store16_lane`.
///
/// Spec:
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: internal specialized SIMD handler.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Delegates to the typed lane-store fast path selected by `SHARED`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction stream for the current active frame.
/// - `ctx` must reference a live execution context whose active frame default memory matches `SHARED`.
/// - This helper must not keep borrows, locks, or guards alive across `call_next`.
unsafe fn v128_store16_lane_impl<const SHARED: bool, const INDEXED: bool>(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    store_lane_internal::<2, SHARED, INDEXED>(tail_code, ctx)
}
define_indexed_local_simd_memory_handler!(
    v128_store16_lane_indexed_local,
    "v128.store16_lane",
    v128_store16_lane_impl
);
define_indexed_shared_simd_memory_handler!(
    v128_store16_lane_indexed_shared,
    "v128.store16_lane",
    v128_store16_lane_impl
);

define_local_simd_memory_handler!(
    v128_store32_lane,
    "v128.store32_lane",
    v128_store32_lane_impl
);
define_shared_simd_memory_handler!(
    v128_store32_lane_shared,
    "v128.store32_lane",
    v128_store32_lane_impl
);

#[inline(always)]
/// Telomere internal SIMD handler implementation for `v128.store32_lane`.
///
/// Spec:
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: internal specialized SIMD handler.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Delegates to the typed lane-store fast path selected by `SHARED`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction stream for the current active frame.
/// - `ctx` must reference a live execution context whose active frame default memory matches `SHARED`.
/// - This helper must not keep borrows, locks, or guards alive across `call_next`.
unsafe fn v128_store32_lane_impl<const SHARED: bool, const INDEXED: bool>(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    store_lane_internal::<4, SHARED, INDEXED>(tail_code, ctx)
}
define_indexed_local_simd_memory_handler!(
    v128_store32_lane_indexed_local,
    "v128.store32_lane",
    v128_store32_lane_impl
);
define_indexed_shared_simd_memory_handler!(
    v128_store32_lane_indexed_shared,
    "v128.store32_lane",
    v128_store32_lane_impl
);

define_local_simd_memory_handler!(
    v128_store64_lane,
    "v128.store64_lane",
    v128_store64_lane_impl
);
define_shared_simd_memory_handler!(
    v128_store64_lane_shared,
    "v128.store64_lane",
    v128_store64_lane_impl
);

#[inline(always)]
/// Telomere internal SIMD handler implementation for `v128.store64_lane`.
///
/// Spec:
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: internal specialized SIMD handler.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Delegates to the typed lane-store fast path selected by `SHARED`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction stream for the current active frame.
/// - `ctx` must reference a live execution context whose active frame default memory matches `SHARED`.
/// - This helper must not keep borrows, locks, or guards alive across `call_next`.
unsafe fn v128_store64_lane_impl<const SHARED: bool, const INDEXED: bool>(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    store_lane_internal::<8, SHARED, INDEXED>(tail_code, ctx)
}
define_indexed_local_simd_memory_handler!(
    v128_store64_lane_indexed_local,
    "v128.store64_lane",
    v128_store64_lane_impl
);
define_indexed_shared_simd_memory_handler!(
    v128_store64_lane_indexed_shared,
    "v128.store64_lane",
    v128_store64_lane_impl
);

/// WebAssembly `v128.load32_zero`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[i32] -> [v128]`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Implements the validated SIMD semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
unsafe fn v128_load32_zero_impl<const SHARED: bool, const INDEXED: bool>(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let bytes = vm_try!(read_memory_bytes::<4, SHARED, INDEXED>(tail_code, ctx));
    let mut result = [0u8; 16];
    result[0..4].copy_from_slice(&bytes);
    vm_try!(ctx.stack.push_u128(u128::from_le_bytes(result)));
    call_next(tail_code, if INDEXED { 2 } else { 1 }, ctx)
}
define_local_simd_memory_handler!(v128_load32_zero, "v128.load32_zero", v128_load32_zero_impl);
define_shared_simd_memory_handler!(
    v128_load32_zero_shared,
    "v128.load32_zero",
    v128_load32_zero_impl
);
define_indexed_local_simd_memory_handler!(
    v128_load32_zero_indexed_local,
    "v128.load32_zero",
    v128_load32_zero_impl
);
define_indexed_shared_simd_memory_handler!(
    v128_load32_zero_indexed_shared,
    "v128.load32_zero",
    v128_load32_zero_impl
);

/// WebAssembly `v128.load64_zero`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[i32] -> [v128]`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Implements the validated SIMD semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
unsafe fn v128_load64_zero_impl<const SHARED: bool, const INDEXED: bool>(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let bytes = vm_try!(read_memory_bytes::<8, SHARED, INDEXED>(tail_code, ctx));
    let mut result = [0u8; 16];
    result[0..8].copy_from_slice(&bytes);
    vm_try!(ctx.stack.push_u128(u128::from_le_bytes(result)));
    call_next(tail_code, if INDEXED { 2 } else { 1 }, ctx)
}
define_local_simd_memory_handler!(v128_load64_zero, "v128.load64_zero", v128_load64_zero_impl);
define_shared_simd_memory_handler!(
    v128_load64_zero_shared,
    "v128.load64_zero",
    v128_load64_zero_impl
);
define_indexed_local_simd_memory_handler!(
    v128_load64_zero_indexed_local,
    "v128.load64_zero",
    v128_load64_zero_impl
);
define_indexed_shared_simd_memory_handler!(
    v128_load64_zero_indexed_shared,
    "v128.load64_zero",
    v128_load64_zero_impl
);

/// WebAssembly `v128.not`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[v128] -> [v128]`.
/// Traps: none.
/// Notes: Implements the validated SIMD semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn v128_not(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_u128();
    vm_try!(ctx.stack.push_u128(!b));
    call_next(tail_code, 0, ctx)
}
/// WebAssembly `v128.and`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[v128] -> [v128]`.
/// Traps: none.
/// Notes: Implements the validated SIMD semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn v128_and(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_u128();
    let a = ctx.stack.pop_u128();
    vm_try!(ctx.stack.push_u128(a & b));
    call_next(tail_code, 0, ctx)
}
/// WebAssembly `v128.andnot`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[v128] -> [v128]`.
/// Traps: none.
/// Notes: Implements the validated SIMD semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn v128_andnot(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_u128();
    let a = ctx.stack.pop_u128();
    vm_try!(ctx.stack.push_u128(a & !b));
    call_next(tail_code, 0, ctx)
}
/// WebAssembly `v128.or`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[v128] -> [v128]`.
/// Traps: none.
/// Notes: Implements the validated SIMD semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn v128_or(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_u128();
    let a = ctx.stack.pop_u128();
    vm_try!(ctx.stack.push_u128(a | b));
    call_next(tail_code, 0, ctx)
}
/// WebAssembly `v128.xor`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[v128] -> [v128]`.
/// Traps: none.
/// Notes: Implements the validated SIMD semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn v128_xor(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b = ctx.stack.pop_u128();
    let a = ctx.stack.pop_u128();
    vm_try!(ctx.stack.push_u128(a ^ b));
    call_next(tail_code, 0, ctx)
}
/// WebAssembly `v128.any_true`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[v128] -> [i32]`.
/// Traps: none.
/// Notes: Implements the validated SIMD semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn v128_any_true(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let v = ctx.stack.pop_u128();
    let result = if v == 0 { 0 } else { 1 };
    vm_try!(ctx.stack.push_i32(result));
    call_next(tail_code, 0, ctx)
}

macro_rules! all_true_instruction {
    ($name: ident,$target: ident) => {
        #[doc = concat!("WebAssembly SIMD handler `", stringify!($name), "`.")]
        #[doc = ""]
        #[doc = "Related spec:"]
        #[doc = "- WebAssembly Core Spec: https://webassembly.github.io/spec/core/index.html"]
        #[doc = ""]
        #[doc = "Stack effect: see the validated SIMD operand and result shape for this handler."]
        #[doc = "Traps: memory-using variants trap on out-of-bounds access; pure vector variants do not trap."]
        #[doc = "Notes: This handler preserves direct-threaded execution and tail-dispatches with `call_next`."]
        #[doc = ""]
        #[doc = "# Safety"]
        #[doc = "- `tail_code` must point to the decoded instruction for this handler in the active function body."]
        #[doc = "- `ctx` must reference a live execution context whose validated operand stack and locals satisfy this instruction."]
        #[doc = "- This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`."]
        pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            let v: $target = ctx.stack.pop();
            let mut all_true = 0x01;
            for v in v.to_array() {
                all_true &= (v != 0) as i32;
            }
            vm_try!(ctx.stack.push_i32(all_true));
            call_next(tail_code, 0, ctx)
        }
    };
}

all_true_instruction!(i8x16_all_true, i8x16);
all_true_instruction!(i16x8_all_true, i16x8);
all_true_instruction!(i32x4_all_true, i32x4);
all_true_instruction!(i64x2_all_true, i64x2);
macro_rules! bitmask_instruction {
    ($name: ident,$target: ident) => {
        #[doc = concat!("WebAssembly SIMD handler `", stringify!($name), "`.")]
        #[doc = ""]
        #[doc = "Related spec:"]
        #[doc = "- WebAssembly Core Spec: https://webassembly.github.io/spec/core/index.html"]
        #[doc = ""]
        #[doc = "Stack effect: see the validated SIMD operand and result shape for this handler."]
        #[doc = "Traps: memory-using variants trap on out-of-bounds access; pure vector variants do not trap."]
        #[doc = "Notes: This handler preserves direct-threaded execution and tail-dispatches with `call_next`."]
        #[doc = ""]
        #[doc = "# Safety"]
        #[doc = "- `tail_code` must point to the decoded instruction for this handler in the active function body."]
        #[doc = "- `ctx` must reference a live execution context whose validated operand stack and locals satisfy this instruction."]
        #[doc = "- This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`."]
        pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            let v: $target = ctx.stack.pop();
            let result = v.move_mask();
            vm_try!(ctx.stack.push_i32(result));
            call_next(tail_code, 0, ctx)
        }
    };
}
bitmask_instruction!(i8x16_bitmask, i8x16);
bitmask_instruction!(i16x8_bitmask, i16x8);
bitmask_instruction!(i32x4_bitmask, i32x4);
bitmask_instruction!(i64x2_bitmask, i64x2);

/// WebAssembly `v128.bitselect`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[v128, v128] -> [v128]`.
/// Traps: none.
/// Notes: Implements the validated SIMD semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_v128_bitselect(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let mask = ctx.stack.pop_u128();
    let b = ctx.stack.pop_u128();
    let a = ctx.stack.pop_u128();

    let result = a & mask | b & !mask;

    vm_try!(ctx.stack.push_u128(result));
    call_next(tail_code, 0, ctx)
}
macro_rules! shl_instruction {
    ($name: ident,$target: ident) => {
        #[doc = concat!("WebAssembly SIMD handler `", stringify!($name), "`.")]
        #[doc = ""]
        #[doc = "Related spec:"]
        #[doc = "- WebAssembly Core Spec: https://webassembly.github.io/spec/core/index.html"]
        #[doc = ""]
        #[doc = "Stack effect: see the validated SIMD operand and result shape for this handler."]
        #[doc = "Traps: memory-using variants trap on out-of-bounds access; pure vector variants do not trap."]
        #[doc = "Notes: This handler preserves direct-threaded execution and tail-dispatches with `call_next`."]
        #[doc = ""]
        #[doc = "# Safety"]
        #[doc = "- `tail_code` must point to the decoded instruction for this handler in the active function body."]
        #[doc = "- `ctx` must reference a live execution context whose validated operand stack and locals satisfy this instruction."]
        #[doc = "- This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`."]
        pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            let shift = ctx.stack.pop_u32();
            let v: $target = ctx.stack.pop();

            let shift = shift as u32;

            let mut v = v.to_array();
            for i in 0..v.len() {
                v[i] = v[i].wrapping_shl(shift);
            }

            vm_try!(ctx.stack.push($target::from(v)));
            call_next(tail_code, 0, ctx)
        }
    };
}

macro_rules! shr_instruction {
    ($name: ident,$target: ident) => {
        #[doc = concat!("WebAssembly SIMD handler `", stringify!($name), "`.")]
        #[doc = ""]
        #[doc = "Related spec:"]
        #[doc = "- WebAssembly Core Spec: https://webassembly.github.io/spec/core/index.html"]
        #[doc = ""]
        #[doc = "Stack effect: see the validated SIMD operand and result shape for this handler."]
        #[doc = "Traps: memory-using variants trap on out-of-bounds access; pure vector variants do not trap."]
        #[doc = "Notes: This handler preserves direct-threaded execution and tail-dispatches with `call_next`."]
        #[doc = ""]
        #[doc = "# Safety"]
        #[doc = "- `tail_code` must point to the decoded instruction for this handler in the active function body."]
        #[doc = "- `ctx` must reference a live execution context whose validated operand stack and locals satisfy this instruction."]
        #[doc = "- This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`."]
        pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            let shift = ctx.stack.pop_u32();
            let v: $target = ctx.stack.pop();

            let shift = shift as u32;

            let mut v = v.to_array();
            for i in 0..v.len() {
                v[i] = v[i].wrapping_shr(shift);
            }

            vm_try!(ctx.stack.push($target::from(v)));
            call_next(tail_code, 0, ctx)
        }
    };
}

shl_instruction!(i8x16_shl, i8x16);
shl_instruction!(i16x8_shl, i16x8);
shl_instruction!(i32x4_shl, i32x4);
shl_instruction!(i64x2_shl, i64x2);

shr_instruction!(i8x16_shr, i8x16);
shr_instruction!(i16x8_shr, i16x8);
shr_instruction!(i32x4_shr, i32x4);
shr_instruction!(i64x2_shr, i64x2);
shr_instruction!(u8x16_shr, u8x16);
shr_instruction!(u16x8_shr, u16x8);
shr_instruction!(u32x4_shr, u32x4);
shr_instruction!(u64x2_shr, u64x2);
/// WebAssembly `i32x4.trunc_sat_f32x4_s`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[v128] -> [v128]`.
/// Traps: none.
/// Notes: Implements the validated SIMD semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn i32x4_trunc_sat_f32x4_s(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    use crate::common::stack::StackOperation;
    let a: f32x4 = ctx.stack.pop();
    let result = a.trunc_int();
    vm_try!(ctx.stack.push(result));
    call_next(tail_code, 0, ctx)
}
/// WebAssembly `i32x4.trunc_sat_f32x4_u`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[v128] -> [v128]`.
/// Traps: none.
/// Notes: Implements the validated SIMD semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn i32x4_trunc_sat_f32x4_u(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    use crate::common::stack::StackOperation;
    let a: f32x4 = ctx.stack.pop();
    let a = a.to_array();
    vm_try!(ctx.stack.push(u32x4::from([
        a[0].trunc() as u32,
        a[1].trunc() as u32,
        a[2].trunc() as u32,
        a[3].trunc() as u32
    ])));
    call_next(tail_code, 0, ctx)
}
/// WebAssembly `i32x4.trunc_sat_f64x2_s`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[v128] -> [v128]`.
/// Traps: none.
/// Notes: Implements the validated SIMD semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn i32x4_trunc_sat_f64x2_s(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    use crate::common::stack::StackOperation;
    let a: f64x2 = ctx.stack.pop();
    let a = a.to_array();
    vm_try!(ctx.stack.push(i32x4::from([
        a[0].trunc() as i32,
        a[1].trunc() as i32,
        0,
        0
    ])));
    call_next(tail_code, 0, ctx)
}
/// WebAssembly `i32x4.trunc_sat_f64x2_u`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[v128] -> [v128]`.
/// Traps: none.
/// Notes: Implements the validated SIMD semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn i32x4_trunc_sat_f64x2_u(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    use crate::common::stack::StackOperation;
    let a: f64x2 = ctx.stack.pop();
    let a = a.to_array();
    vm_try!(ctx.stack.push(u32x4::from([
        a[0].trunc() as u32,
        a[1].trunc() as u32,
        0,
        0
    ])));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `f32x4.convert_i32x4_s`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[v128] -> [v128]`.
/// Traps: none.
/// Notes: Implements the validated SIMD semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn f32x4_convert_i32x4_s(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    use crate::common::stack::StackOperation;
    let a: i32x4 = ctx.stack.pop();
    let [a, b, c, d] = a.to_array();
    let result = f32x4::from([a as f32, b as f32, c as f32, d as f32]);
    vm_try!(ctx.stack.push(result));
    call_next(tail_code, 0, ctx)
}
/// WebAssembly `f32x4.convert_i32x4_u`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[v128] -> [v128]`.
/// Traps: none.
/// Notes: Implements the validated SIMD semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn f32x4_convert_i32x4_u(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    use crate::common::stack::StackOperation;
    let a: u32x4 = ctx.stack.pop();
    let [a, b, c, d] = a.to_array();
    let result = f32x4::from([a as f32, b as f32, c as f32, d as f32]);
    vm_try!(ctx.stack.push(result));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `f64x2.convert_low_i32x4_s`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[v128] -> [v128]`.
/// Traps: none.
/// Notes: Implements the validated SIMD semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn f64x2_convert_low_i32x4_s(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    use crate::common::stack::StackOperation;
    let v: i32x4 = ctx.stack.pop();
    let result = f64x2::from_i32x4_lower2(v);
    vm_try!(ctx.stack.push(result));
    call_next(tail_code, 0, ctx)
}
/// WebAssembly `f64x2.convert_low_i32x4_u`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[v128] -> [v128]`.
/// Traps: none.
/// Notes: Implements the validated SIMD semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn f64x2_convert_low_i32x4_u(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    use crate::common::stack::StackOperation;
    let a: u32x4 = ctx.stack.pop();
    let [a, b, _c, _d] = a.to_array();
    let result = f64x2::from([a as f64, b as f64]);
    vm_try!(ctx.stack.push(result));
    call_next(tail_code, 0, ctx)
}
macro_rules! narrow_instruction {
    ($name: ident,$from: ident,$to: ident) => {
        #[doc = concat!("WebAssembly SIMD handler `", stringify!($name), "`.")]
        #[doc = ""]
        #[doc = "Related spec:"]
        #[doc = "- WebAssembly Core Spec: https://webassembly.github.io/spec/core/index.html"]
        #[doc = ""]
        #[doc = "Stack effect: see the validated SIMD operand and result shape for this handler."]
        #[doc = "Traps: memory-using variants trap on out-of-bounds access; pure vector variants do not trap."]
        #[doc = "Notes: This handler preserves direct-threaded execution and tail-dispatches with `call_next`."]
        #[doc = ""]
        #[doc = "# Safety"]
        #[doc = "- `tail_code` must point to the decoded instruction for this handler in the active function body."]
        #[doc = "- `ctx` must reference a live execution context whose validated operand stack and locals satisfy this instruction."]
        #[doc = "- This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`."]
        pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            use crate::common::stack::StackOperation;

            let b: $from = ctx.stack.pop();
            let a: $from = ctx.stack.pop();
            let mut result: [<$to as LaneType>::BaseType; <$to as LaneType>::LANE_SIZE] =
                [0; <$to as LaneType>::LANE_SIZE];
            let a_arr = a.to_array();
            let b_arr = b.to_array();

            for i in 0..<$from as LaneType>::LANE_SIZE {
                result[i] = a_arr[i].clamp(
                    <$to as LaneType>::BaseType::MIN as <$from as LaneType>::BaseType,
                    <$to as LaneType>::BaseType::MAX as <$from as LaneType>::BaseType,
                ) as <$to as LaneType>::BaseType;
                result[i + <$from as LaneType>::LANE_SIZE] = b_arr[i].clamp(
                    <$to as LaneType>::BaseType::MIN as <$from as LaneType>::BaseType,
                    <$to as LaneType>::BaseType::MAX as <$from as LaneType>::BaseType,
                )
                    as <$to as LaneType>::BaseType;
            }

            vm_try!(ctx.stack.push($to::from(result)));
            call_next(tail_code, 0, ctx)
        }
    };
}

narrow_instruction!(i8x16_narrow_i16x8_s, i16x8, i8x16);
narrow_instruction!(i8x16_narrow_i16x8_u, i16x8, u8x16);
narrow_instruction!(i16x8_narrow_i32x4_s, i32x4, i16x8);
narrow_instruction!(i16x8_narrow_i32x4_u, i32x4, u16x8);

/// WebAssembly `f32x4.demote_f64x2_zero`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[v128] -> [v128]`.
/// Traps: none.
/// Notes: Implements the validated SIMD semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn f32x4_demote_f64x2_zero(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let v: f64x2 = ctx.stack.pop();
    let [a, b] = v.to_array();
    vm_try!(ctx
        .stack
        .push(f32x4::from([a as f32, b as f32, 0.0f32, 0.0f32])));
    call_next(tail_code, 0, ctx)
}
/// WebAssembly `f64x2.promote_low_f32x4`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[v128] -> [v128]`.
/// Traps: none.
/// Notes: Implements the validated SIMD semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn f64x2_promote_low_f32x4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let v: f32x4 = ctx.stack.pop();
    let [a, b, _c, _d] = v.to_array();
    vm_try!(ctx.stack.push(f64x2::from([a as f64, b as f64])));
    call_next(tail_code, 0, ctx)
}
macro_rules! extend_instruction {
    ($name: ident,$from: ident,$to: ident,$($index: expr),*)=> {
        #[doc = concat!("WebAssembly SIMD handler `", stringify!($name), "`.")]
        #[doc = ""]
        #[doc = "Related spec:"]
        #[doc = "- WebAssembly Core Spec: https://webassembly.github.io/spec/core/index.html"]
        #[doc = ""]
        #[doc = "Stack effect: see the validated SIMD operand and result shape for this handler."]
        #[doc = "Traps: memory-using variants trap on out-of-bounds access; pure vector variants do not trap."]
        #[doc = "Notes: This handler preserves direct-threaded execution and tail-dispatches with `call_next`."]
        #[doc = ""]
        #[doc = "# Safety"]
        #[doc = "- `tail_code` must point to the decoded instruction for this handler in the active function body."]
        #[doc = "- `ctx` must reference a live execution context whose validated operand stack and locals satisfy this instruction."]
        #[doc = "- This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`."]
        pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            let v: $from = ctx.stack.pop();
            let v = v.to_array();
            vm_try!(ctx.stack.push($to::from([$(v[$index] as <$to as LaneType>::BaseType),*])));

            call_next(tail_code, 0, ctx)
        }
    }
}

extend_instruction!(
    i16x8_extend_low_i8x16_s,
    i8x16,
    i16x8,
    0,
    1,
    2,
    3,
    4,
    5,
    6,
    7
);
extend_instruction!(
    i16x8_extend_high_i8x16_s,
    i8x16,
    i16x8,
    8,
    9,
    10,
    11,
    12,
    13,
    14,
    15
);
extend_instruction!(
    i16x8_extend_low_i8x16_u,
    u8x16,
    u16x8,
    0,
    1,
    2,
    3,
    4,
    5,
    6,
    7
);
extend_instruction!(
    i16x8_extend_high_i8x16_u,
    u8x16,
    u16x8,
    8,
    9,
    10,
    11,
    12,
    13,
    14,
    15
);

extend_instruction!(i32x4_extend_low_i16x8_s, i16x8, i32x4, 0, 1, 2, 3);
extend_instruction!(i32x4_extend_high_i16x8_s, i16x8, i32x4, 4, 5, 6, 7);
extend_instruction!(i32x4_extend_low_i16x8_u, u16x8, u32x4, 0, 1, 2, 3);
extend_instruction!(i32x4_extend_high_i16x8_u, u16x8, u32x4, 4, 5, 6, 7);
extend_instruction!(i64x2_extend_low_i32x4_s, i32x4, i64x2, 0, 1);
extend_instruction!(i64x2_extend_high_i32x4_s, i32x4, i64x2, 2, 3);
extend_instruction!(i64x2_extend_low_i32x4_u, u32x4, u64x2, 0, 1);
extend_instruction!(i64x2_extend_high_i32x4_u, u32x4, u64x2, 2, 3);

#[inline]
/// WebAssembly SIMD unary helper.
///
/// Spec:
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: internal SIMD unary helper.
/// Traps: traps only if the underlying stack push fails or the decoded instruction stream is invalid.
/// Notes: Applies a lane-wise unary operation and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction stream for the current active frame.
/// - `ctx` must reference a live execution context whose validated stack layout satisfies this instruction.
/// - This helper must not keep borrows or guards alive across the tail-dispatch it performs.
unsafe fn handle_unary_op<T>(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    op: impl FnOnce(T, T) -> T,
) -> VMResult<()>
where
    Stack: StackOperation<T>,
{
    use crate::common::stack::StackOperation;
    let v2: T = ctx.stack.pop();
    let v1: T = ctx.stack.pop();
    let result = op(v1, v2);
    vm_try!(ctx.stack.push(result));
    call_next(tail_code, 0, ctx)
}

#[inline]
/// WebAssembly SIMD binary helper.
///
/// Spec:
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: internal SIMD binary helper.
/// Traps: traps only if the underlying stack push fails or the decoded instruction stream is invalid.
/// Notes: Applies a lane-wise binary operation and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction stream for the current active frame.
/// - `ctx` must reference a live execution context whose validated stack layout satisfies this instruction.
/// - This helper must not keep borrows or guards alive across the tail-dispatch it performs.
unsafe fn handle_binary_op<T>(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    op: impl FnOnce(T) -> T,
) -> VMResult<()>
where
    Stack: StackOperation<T>,
{
    use crate::common::stack::StackOperation;
    let a: T = ctx.stack.pop();
    let result = op(a);
    vm_try!(ctx.stack.push(result));
    call_next(tail_code, 0, ctx)
}

macro_rules! define_unary_simd_operation {
    ($op: ident,[$($target: ident),*],$expr: expr) => {
        define_simd_operation!(handle_unary_op,$op,[$($target),*],$expr);
    };
}
macro_rules! define_binary_simd_operation {
    ($op: ident,[$($target: ident),*],$expr: expr) => {
        define_simd_operation!(handle_binary_op,$op,[$($target),*],$expr);
    };
}
define_unary_simd_operation!(avgr, [u8x16], |a, b| {
    let mut res = [0u8; 16];
    let a = a.to_array();
    let b = b.to_array();
    for i in 0..16 {
        res[i] = (a[i] as u16 + b[i] as u16).div_ceil(2) as u8;
    }
    res.into()
});

define_unary_simd_operation!(avgr, [u16x8], |a, b| {
    let mut res = [0u16; 8];
    let a = a.to_array();
    let b = b.to_array();
    for i in 0..8 {
        res[i] = (a[i] as u32 + b[i] as u32).div_ceil(2) as u16;
    }
    res.into()
});
define_binary_simd_operation!(popcnt, [u8x16], |a| {
    let mut res = [0u8; 16];
    let a = a.to_array();
    for i in 0..16 {
        res[i] = a[i].count_ones() as u8;
    }
    res.into()
});

define_unary_simd_operation!(add, [f32x4, f64x2, i8x16, i16x8, i32x4, i64x2], |a, b| a
    + b);
define_unary_simd_operation!(add_sat, [i8x16, u8x16, i16x8, u16x8], |a, b| a
    .saturating_add(b));
define_unary_simd_operation!(sub, [f32x4, f64x2, i8x16, i16x8, i32x4], |a, b| a - b);
define_unary_simd_operation!(sub_sat, [i8x16, u8x16, i16x8, u16x8], |a, b| a
    .saturating_sub(b));
define_unary_simd_operation!(mul, [f32x4, f64x2, i16x8, i32x4], |a, b| a * b);
define_unary_simd_operation!(div, [f32x4, f64x2], |a, b| a / b);
define_unary_simd_operation!(swizzle, [i8x16], |a, b| a.swizzle(b));
define_unary_simd_operation!(min, [i8x16, u8x16, i16x8, u16x8, i32x4, u32x4], |a, b| a
    .min(b));
define_unary_simd_operation!(min, [f32x4], |a, b| {
    let aa = a.to_array();
    let bb = b.to_array();
    let mut result = [0.0f32; 4];

    for i in 0..4 {
        let (x, y) = (aa[i], bb[i]);
        result[i] = if x.is_nan() || y.is_nan() {
            f32::NAN
        } else if x == y {
            if x == 0.0 && y == 0.0 {
                if x.to_bits() == 0x8000_0000 || y.to_bits() == 0x8000_0000 {
                    -0.0
                } else {
                    0.0
                }
            } else {
                x
            }
        } else {
            x.min(y)
        };
    }

    f32x4::from(result)
});
define_unary_simd_operation!(min, [f64x2], |a, b| {
    let aa = a.to_array();
    let bb = b.to_array();
    let mut result = [0.0f64; 2];

    for i in 0..2 {
        let (x, y) = (aa[i], bb[i]);
        result[i] = if x.is_nan() || y.is_nan() {
            f64::NAN
        } else if x == y {
            if x == 0.0 && y == 0.0 {
                if x.to_bits() == 0x8000_0000_0000_0000 || y.to_bits() == 0x8000_0000_0000_0000 {
                    -0.0
                } else {
                    0.0
                }
            } else {
                x
            }
        } else {
            x.min(y)
        };
    }

    f64x2::from(result)
});

define_unary_simd_operation!(max, [i8x16, u8x16, i16x8, u16x8, i32x4, u32x4], |a, b| a
    .max(b));
define_unary_simd_operation!(max, [f32x4], |a, b| {
    let aa = a.to_array();
    let bb = b.to_array();
    let mut result = [0.0f32; 4];

    for i in 0..4 {
        let (x, y) = (aa[i], bb[i]);
        result[i] = if x.is_nan() || y.is_nan() {
            f32::NAN
        } else if x == y {
            if x == 0.0 && y == 0.0 {
                if x.to_bits() == 0x0000_0000 || y.to_bits() == 0x0000_0000 {
                    0.0
                } else {
                    -0.0
                }
            } else {
                x
            }
        } else {
            x.max(y)
        };
    }

    f32x4::from(result)
});
define_unary_simd_operation!(max, [f64x2], |a, b| {
    let aa = a.to_array();
    let bb = b.to_array();
    let mut result = [0.0f64; 2];

    for i in 0..2 {
        let (x, y) = (aa[i], bb[i]);
        result[i] = if x.is_nan() || y.is_nan() {
            f64::NAN
        } else if x == y {
            if x == 0.0 && y == 0.0 {
                if x.to_bits() == 0x0000_0000_0000_0000 || y.to_bits() == 0x0000_0000_0000_0000 {
                    0.0
                } else {
                    -0.0
                }
            } else {
                x
            }
        } else {
            x.max(y)
        };
    }

    f64x2::from(result)
});
define_unary_simd_operation!(pmin, [f32x4], |a, b| {
    let a_arr = a.to_array();
    let b_arr = b.to_array();
    let mut result = [0.0f32; 4];

    for i in 0..4 {
        let (va, vb) = (a_arr[i], b_arr[i]);
        match (va.is_nan(), vb.is_nan()) {
            (true, _) => result[i] = va,
            (_, true) => result[i] = va,
            (false, false) => {
                if va == vb && va == 0.0 {
                    result[i] = va;
                } else {
                    result[i] = va.min(vb);
                }
            }
        }
    }

    f32x4::from(result)
});
define_unary_simd_operation!(pmax, [f32x4], |a, b| {
    let a_arr = a.to_array();
    let b_arr = b.to_array();
    let mut result = [0.0f32; 4];

    for i in 0..4 {
        let (va, vb) = (a_arr[i], b_arr[i]);
        match (va.is_nan(), vb.is_nan()) {
            (true, _) => result[i] = va,
            (_, true) => result[i] = va,
            (false, false) => {
                if va == vb && va == 0.0 {
                    result[i] = va;
                } else {
                    result[i] = va.max(vb);
                }
            }
        }
    }

    f32x4::from(result)
});

define_unary_simd_operation!(pmin, [f64x2], |a, b| {
    let a_arr = a.to_array();
    let b_arr = b.to_array();
    let mut result = [0.0f64; 2];

    for i in 0..2 {
        let (va, vb) = (a_arr[i], b_arr[i]);
        match (va.is_nan(), vb.is_nan()) {
            (true, _) => result[i] = va,
            (_, true) => result[i] = va,
            (false, false) => {
                if va == vb && va == 0.0 {
                    result[i] = va;
                } else {
                    result[i] = va.min(vb);
                }
            }
        }
    }

    f64x2::from(result)
});
define_unary_simd_operation!(pmax, [f64x2], |a, b| {
    let a_arr = a.to_array();
    let b_arr = b.to_array();
    let mut result = [0.0f64; 2];

    for i in 0..2 {
        let (va, vb) = (a_arr[i], b_arr[i]);
        match (va.is_nan(), vb.is_nan()) {
            (true, _) => result[i] = va,
            (_, true) => result[i] = va,
            (false, false) => {
                if va == vb && va == 0.0 {
                    result[i] = va;
                } else {
                    result[i] = va.max(vb);
                }
            }
        }
    }

    f64x2::from(result)
});

define_binary_simd_operation!(abs, [f64x2, f32x4, i32x4, i8x16, i16x8], |a| a.abs());
define_binary_simd_operation!(ceil, [f64x2, f32x4], |a| a.ceil());
define_binary_simd_operation!(floor, [f64x2, f32x4], |a| a.floor());
define_binary_simd_operation!(trunc, [f32x4], |a| {
    let arr = a.to_array();
    f32x4::from([
        arr[0].trunc(),
        arr[1].trunc(),
        arr[2].trunc(),
        arr[3].trunc(),
    ])
});
define_binary_simd_operation!(trunc, [f64x2], |a| {
    let arr = a.to_array();
    f64x2::from([arr[0].trunc(), arr[1].trunc()])
});
define_binary_simd_operation!(nearest, [f32x4], |a| {
    let arr = a.to_array();
    f32x4::from([
        arr[0].round_ties_even(),
        arr[1].round_ties_even(),
        arr[2].round_ties_even(),
        arr[3].round_ties_even(),
    ])
});
define_binary_simd_operation!(nearest, [f64x2], |a| {
    let arr = a.to_array();
    f64x2::from([arr[0].round_ties_even(), arr[1].round_ties_even()])
});

/// WebAssembly `i64x2.abs`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[v128] -> [v128]`.
/// Traps: none.
/// Notes: Implements the validated SIMD semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn i64x2_abs(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let v: i64x2 = ctx.stack.pop();
    let [a, b] = v.to_array();
    vm_try!(ctx
        .stack
        .push(i64x2::from([a.wrapping_abs(), b.wrapping_abs()])));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `i64x2.neg`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[v128, v128] -> [v128]`.
/// Traps: none.
/// Notes: Implements the validated SIMD semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn i64x2_neg(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let v: i64x2 = ctx.stack.pop();
    let [a, b] = v.to_array();
    vm_try!(ctx
        .stack
        .push(i64x2::from([a.wrapping_neg(), b.wrapping_neg()])));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `f32x4.neg`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[v128, v128] -> [v128]`.
/// Traps: none.
/// Notes: Implements the validated SIMD semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn f32x4_neg(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let v: f32x4 = ctx.stack.pop();
    let [a, b, c, d] = v.to_array();
    vm_try!(ctx.stack.push(f32x4::from([-a, -b, -c, -d])));
    call_next(tail_code, 0, ctx)
}
/// WebAssembly `f64x2.neg`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[v128, v128] -> [v128]`.
/// Traps: none.
/// Notes: Implements the validated SIMD semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn f64x2_neg(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let v: f64x2 = ctx.stack.pop();
    let [a, b] = v.to_array();
    vm_try!(ctx.stack.push(f64x2::from([-a, -b])));
    call_next(tail_code, 0, ctx)
}
/// WebAssembly `i8x16.neg`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[v128, v128] -> [v128]`.
/// Traps: none.
/// Notes: Implements the validated SIMD semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn i8x16_neg(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    use std::ops::BitXor;
    let a: i8x16 = ctx.stack.pop();

    vm_try!(ctx.stack.push(a.bitxor(-i8x16::ONE) + i8x16::ONE));
    call_next(tail_code, 0, ctx)
}
/// WebAssembly `i16x8.neg`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[v128, v128] -> [v128]`.
/// Traps: none.
/// Notes: Implements the validated SIMD semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn i16x8_neg(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    use std::ops::BitXor;
    let a: i16x8 = ctx.stack.pop();

    vm_try!(ctx.stack.push(a.bitxor(-i16x8::ONE) + i16x8::ONE));
    call_next(tail_code, 0, ctx)
}
/// WebAssembly `i32x4.neg`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[v128, v128] -> [v128]`.
/// Traps: none.
/// Notes: Implements the validated SIMD semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn i32x4_neg(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    use std::ops::BitXor;
    let a: i32x4 = ctx.stack.pop();

    vm_try!(ctx.stack.push(a.bitxor(-i32x4::ONE) + i32x4::ONE));
    call_next(tail_code, 0, ctx)
}
define_binary_simd_operation!(sqrt, [f64x2, f32x4], |a| a.sqrt());
use std::ops::Not;
use wide::CmpEq;
use wide::CmpGe;
use wide::CmpGt;
use wide::CmpLe;
use wide::CmpLt;
use wide::CmpNe;
macro_rules! define_simd_cmp_operation {
    ($op_name: ident,[$($ty: ident),*],$op: expr) => {
        $(define_simd_operation!(handle_unary_op, $op_name, [$ty], |a, b| {
            let mut res = [0; $ty::LANE_SIZE];
            let a: [<$ty as LaneType>::BaseType; $ty::LANE_SIZE] = a.to_array();
            let b: [<$ty as LaneType>::BaseType; $ty::LANE_SIZE] = b.to_array();

            for i in 0..$ty::LANE_SIZE {
                let a: <$ty as LaneType>::BaseType = a[i];
                let b: <$ty as LaneType>::BaseType = b[i];

                res[i] = if ($op)(a, b) {
                    !(0 as <$ty as LaneType>::BaseType)
                } else {
                    0 as <$ty as LaneType>::BaseType
                };
            }
            res.into()
        });)*
    };
}

define_unary_simd_operation!(eq, [f32x4, f64x2, i8x16, i16x8, i32x4], |a, b| a.cmp_eq(b));
define_unary_simd_operation!(ne, [f32x4, f64x2], |a, b| a.cmp_ne(b));
define_unary_simd_operation!(ne, [i8x16, i16x8, i32x4], |a, b| a.cmp_eq(b).not());
define_simd_cmp_operation!(eq, [i64x2], |a, b| a == b);
define_simd_cmp_operation!(ne, [i64x2], |a, b| a != b);
define_unary_simd_operation!(lt, [f32x4, f64x2], |a, b| a.cmp_lt(b));
define_simd_cmp_operation!(lt, [i8x16, u8x16, i16x8, u16x8, i32x4, u32x4], |a, b| a < b);
define_simd_cmp_operation!(lt, [i64x2], |a, b| a < b);

define_unary_simd_operation!(gt, [f32x4, f64x2], |a, b| a.cmp_gt(b));
define_simd_cmp_operation!(gt, [i8x16, u8x16, i16x8, u16x8, i32x4, u32x4], |a, b| a > b);
define_simd_cmp_operation!(gt, [i64x2], |a, b| a > b);
define_unary_simd_operation!(le, [f32x4, f64x2], |a, b| a.cmp_le(b));
define_simd_cmp_operation!(le, [i8x16, u8x16, i16x8, u16x8, i32x4, u32x4], |a, b| a
    <= b);
define_simd_cmp_operation!(le, [i64x2], |a, b| a <= b);
define_unary_simd_operation!(ge, [f32x4, f64x2], |a, b| a.cmp_ge(b));
define_simd_cmp_operation!(ge, [i8x16, u8x16, i16x8, u16x8, i32x4, u32x4], |a, b| a
    >= b);
define_simd_cmp_operation!(ge, [i64x2], |a, b| a >= b);
/// WebAssembly `i16x8.extadd_pairwise_i8x16`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[v128, v128] -> [v128]`.
/// Traps: none.
/// Notes: Implements the validated SIMD semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn i16x8_extadd_pairwise_i8x16(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let mut res = [0i16; 8];
    let a: i8x16 = ctx.stack.pop();
    let a = a.to_array();
    for i in 0..8 {
        res[i] = a[i * 2] as i16 + a[i * 2 + 1] as i16;
    }
    vm_try!(ctx.stack.push(i16x8::from(res)));
    call_next(tail_code, 0, ctx)
}
/// WebAssembly `u16x8.extadd_pairwise_i8x16`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[v128, v128] -> [v128]`.
/// Traps: none.
/// Notes: Implements the validated SIMD semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn u16x8_extadd_pairwise_i8x16(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let mut res = [0u16; 8];
    let a: u8x16 = ctx.stack.pop();
    let a = a.to_array();
    for i in 0..8 {
        res[i] = a[i * 2] as u16 + a[i * 2 + 1] as u16;
    }
    vm_try!(ctx.stack.push(u16x8::from(res)));
    call_next(tail_code, 0, ctx)
}
/// WebAssembly `i32x4.extadd_pairwise_i16x8`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[v128, v128] -> [v128]`.
/// Traps: none.
/// Notes: Implements the validated SIMD semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn i32x4_extadd_pairwise_i16x8(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let mut res = [0i32; 4];
    let a: i16x8 = ctx.stack.pop();
    let a = a.to_array();
    for i in 0..4 {
        res[i] = a[i * 2] as i32 + a[i * 2 + 1] as i32;
    }
    vm_try!(ctx.stack.push(i32x4::from(res)));
    call_next(tail_code, 0, ctx)
}
/// WebAssembly `u32x4.extadd_pairwise_i16x8`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[v128, v128] -> [v128]`.
/// Traps: none.
/// Notes: Implements the validated SIMD semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn u32x4_extadd_pairwise_i16x8(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let mut res = [0u32; 4];
    let a: u16x8 = ctx.stack.pop();
    let a = a.to_array();
    for i in 0..4 {
        res[i] = a[i * 2] as u32 + a[i * 2 + 1] as u32;
    }
    vm_try!(ctx.stack.push(u32x4::from(res)));
    call_next(tail_code, 0, ctx)
}

fn q15mulr_sat_s_lane(a: i16, b: i16) -> i16 {
    let rounded = (a as i32 * b as i32 + 0x4000) >> 15;
    rounded.clamp(i16::MIN as i32, i16::MAX as i32) as i16
}

define_unary_simd_operation!(q15mulr_sat_s, [i16x8], |a, b| {
    let a = a.to_array();
    let b = b.to_array();
    let mut result = [0i16; 8];
    for i in 0..8 {
        result[i] = q15mulr_sat_s_lane(a[i], b[i]);
    }
    i16x8::from(result)
});
fn extend_low_i8x16_to_i16x8(input: i8x16) -> i16x8 {
    let arr = input.to_array();
    let mut extended = [0i16; 8];
    for i in 0..8 {
        extended[i] = arr[i] as i16;
    }
    i16x8::from(extended)
}
fn extend_high_i8x16_to_i16x8(input: i8x16) -> i16x8 {
    let arr = input.to_array();
    let mut extended = [0i16; 8];
    for i in 0..8 {
        extended[i] = arr[i + 8] as i16;
    }
    i16x8::from(extended)
}
fn extend_low_i16x8_to_i32x4(input: i16x8) -> i32x4 {
    let arr = input.to_array();
    let mut extended = [0i32; 4];
    for i in 0..4 {
        extended[i] = arr[i] as i32;
    }
    i32x4::from(extended)
}
fn extend_high_i16x8_to_i32x4(input: i16x8) -> i32x4 {
    let arr = input.to_array();
    let mut extended = [0i32; 4];
    for i in 0..4 {
        extended[i] = arr[i + 4] as i32;
    }
    i32x4::from(extended)
}
fn extend_low_u16x8_to_u32x4(input: u16x8) -> u32x4 {
    let arr = input.to_array();
    let mut extended = [0u32; 4];
    for i in 0..4 {
        extended[i] = arr[i] as u32;
    }
    u32x4::from(extended)
}
fn extend_high_u16x8_to_u32x4(input: u16x8) -> u32x4 {
    let arr = input.to_array();
    let mut extended = [0u32; 4];
    for i in 0..4 {
        extended[i] = arr[i + 4] as u32;
    }
    u32x4::from(extended)
}

fn extend_low_i32x4_to_i64x2(input: i32x4) -> i64x2 {
    let arr = input.to_array();
    i64x2::from([arr[0] as i64, arr[1] as i64])
}

fn extend_high_i32x4_to_i64x2(input: i32x4) -> i64x2 {
    let arr = input.to_array();
    i64x2::from([arr[2] as i64, arr[3] as i64])
}

fn extend_low_u32x4_to_u64x2(input: u32x4) -> u64x2 {
    let arr = input.to_array();
    u64x2::from([arr[0] as u64, arr[1] as u64])
}

fn extend_high_u32x4_to_u64x2(input: u32x4) -> u64x2 {
    let arr = input.to_array();
    u64x2::from([arr[2] as u64, arr[3] as u64])
}

/// WebAssembly `i16x8.extmul_low`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[v128, v128] -> [v128]`.
/// Traps: none.
/// Notes: Implements the validated SIMD semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn i16x8_extmul_low(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let a: i8x16 = ctx.stack.pop();
    let b: i8x16 = ctx.stack.pop();

    let a = extend_low_i8x16_to_i16x8(a);
    let b = extend_low_i8x16_to_i16x8(b);
    vm_try!(ctx.stack.push(a * b));
    call_next(tail_code, 0, ctx)
}
/// WebAssembly `i16x8.extmul_high`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[v128, v128] -> [v128]`.
/// Traps: none.
/// Notes: Implements the validated SIMD semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn i16x8_extmul_high(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let a: i8x16 = ctx.stack.pop();
    let b: i8x16 = ctx.stack.pop();

    let a = extend_high_i8x16_to_i16x8(a);
    let b = extend_high_i8x16_to_i16x8(b);
    vm_try!(ctx.stack.push(a * b));
    call_next(tail_code, 0, ctx)
}
/// WebAssembly `u16x8.extmul_low`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[v128, v128] -> [v128]`.
/// Traps: none.
/// Notes: Implements the validated SIMD semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn u16x8_extmul_low(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let a: u8x16 = ctx.stack.pop();
    let b: u8x16 = ctx.stack.pop();

    let a = u16x8::from_u8x16_low(a);
    let b = u16x8::from_u8x16_low(b);
    vm_try!(ctx.stack.push(a * b));
    call_next(tail_code, 0, ctx)
}
/// WebAssembly `u16x8.extmul_high`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[v128, v128] -> [v128]`.
/// Traps: none.
/// Notes: Implements the validated SIMD semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn u16x8_extmul_high(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let a: u8x16 = ctx.stack.pop();
    let b: u8x16 = ctx.stack.pop();

    let a = u16x8::from_u8x16_high(a);
    let b = u16x8::from_u8x16_high(b);
    vm_try!(ctx.stack.push(a * b));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `i32x4.extmul_low`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[v128, v128] -> [v128]`.
/// Traps: none.
/// Notes: Implements the validated SIMD semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn i32x4_extmul_low(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let a: i16x8 = ctx.stack.pop();
    let b: i16x8 = ctx.stack.pop();

    let a = extend_low_i16x8_to_i32x4(a);
    let b = extend_low_i16x8_to_i32x4(b);
    vm_try!(ctx.stack.push(a * b));
    call_next(tail_code, 0, ctx)
}
/// WebAssembly `i32x4.extmul_high`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[v128, v128] -> [v128]`.
/// Traps: none.
/// Notes: Implements the validated SIMD semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn i32x4_extmul_high(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let a: i16x8 = ctx.stack.pop();
    let b: i16x8 = ctx.stack.pop();

    let a = extend_high_i16x8_to_i32x4(a);
    let b = extend_high_i16x8_to_i32x4(b);
    vm_try!(ctx.stack.push(a * b));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `i64x2.sub`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[v128, v128] -> [v128]`.
/// Traps: none.
/// Notes: Implements the validated SIMD semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn i64x2_sub(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b: i64x2 = ctx.stack.pop();
    let a: i64x2 = ctx.stack.pop();
    let [b0, b1] = b.to_array();
    let [a0, a1] = a.to_array();
    vm_try!(ctx
        .stack
        .push(i64x2::from([a0.wrapping_sub(b0), a1.wrapping_sub(b1),])));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `i64x2.mul`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[v128, v128] -> [v128]`.
/// Traps: none.
/// Notes: Implements the validated SIMD semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn i64x2_mul(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let b: i64x2 = ctx.stack.pop();
    let a: i64x2 = ctx.stack.pop();
    let [b0, b1] = b.to_array();
    let [a0, a1] = a.to_array();
    vm_try!(ctx
        .stack
        .push(i64x2::from([a0.wrapping_mul(b0), a1.wrapping_mul(b1),])));
    call_next(tail_code, 0, ctx)
}
/// WebAssembly `u32x4.extmul_low`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[v128, v128] -> [v128]`.
/// Traps: none.
/// Notes: Implements the validated SIMD semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn u32x4_extmul_low(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let a: u16x8 = ctx.stack.pop();
    let b: u16x8 = ctx.stack.pop();

    let a = extend_low_u16x8_to_u32x4(a);
    let b = extend_low_u16x8_to_u32x4(b);
    vm_try!(ctx.stack.push(a * b));
    call_next(tail_code, 0, ctx)
}
/// WebAssembly `u32x4.extmul_high`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[v128, v128] -> [v128]`.
/// Traps: none.
/// Notes: Implements the validated SIMD semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn u32x4_extmul_high(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let a: u16x8 = ctx.stack.pop();
    let b: u16x8 = ctx.stack.pop();

    let a = extend_high_u16x8_to_u32x4(a);
    let b = extend_high_u16x8_to_u32x4(b);
    vm_try!(ctx.stack.push(a * b));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `i64x2.extmul_low_i32x4_s`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[v128, v128] -> [v128]`.
/// Traps: none.
/// Notes: Implements the validated SIMD semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn i64x2_extmul_low_i32x4_s(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let a: i32x4 = ctx.stack.pop();
    let b: i32x4 = ctx.stack.pop();
    let [a0, a1] = extend_low_i32x4_to_i64x2(a).to_array();
    let [b0, b1] = extend_low_i32x4_to_i64x2(b).to_array();
    vm_try!(ctx
        .stack
        .push(i64x2::from([a0.wrapping_mul(b0), a1.wrapping_mul(b1),])));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `i64x2.extmul_high_i32x4_s`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[v128, v128] -> [v128]`.
/// Traps: none.
/// Notes: Implements the validated SIMD semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn i64x2_extmul_high_i32x4_s(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let a: i32x4 = ctx.stack.pop();
    let b: i32x4 = ctx.stack.pop();
    let [a0, a1] = extend_high_i32x4_to_i64x2(a).to_array();
    let [b0, b1] = extend_high_i32x4_to_i64x2(b).to_array();
    vm_try!(ctx
        .stack
        .push(i64x2::from([a0.wrapping_mul(b0), a1.wrapping_mul(b1),])));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `i64x2.extmul_low_i32x4_u`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[v128, v128] -> [v128]`.
/// Traps: none.
/// Notes: Implements the validated SIMD semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn i64x2_extmul_low_i32x4_u(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let a: u32x4 = ctx.stack.pop();
    let b: u32x4 = ctx.stack.pop();
    let [a0, a1] = extend_low_u32x4_to_u64x2(a).to_array();
    let [b0, b1] = extend_low_u32x4_to_u64x2(b).to_array();
    vm_try!(ctx
        .stack
        .push(u64x2::from([a0.wrapping_mul(b0), a1.wrapping_mul(b1),])));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `i64x2.extmul_high_i32x4_u`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[v128, v128] -> [v128]`.
/// Traps: none.
/// Notes: Implements the validated SIMD semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn i64x2_extmul_high_i32x4_u(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let a: u32x4 = ctx.stack.pop();
    let b: u32x4 = ctx.stack.pop();
    let [a0, a1] = extend_high_u32x4_to_u64x2(a).to_array();
    let [b0, b1] = extend_high_u32x4_to_u64x2(b).to_array();
    vm_try!(ctx
        .stack
        .push(u64x2::from([a0.wrapping_mul(b0), a1.wrapping_mul(b1),])));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `i32x4.dot_i16x8`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[v128, v128] -> [v128]`.
/// Traps: none.
/// Notes: Implements the validated SIMD semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn i32x4_dot_i16x8(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let a: i16x8 = ctx.stack.pop();
    let b: i16x8 = ctx.stack.pop();

    let a = a.to_array();
    let b = b.to_array();
    vm_try!(ctx.stack.push(i32x4::from([
        i32::wrapping_add(a[0] as i32 * b[0] as i32, a[1] as i32 * b[1] as i32),
        i32::wrapping_add(a[2] as i32 * b[2] as i32, a[3] as i32 * b[3] as i32),
        i32::wrapping_add(a[4] as i32 * b[4] as i32, a[5] as i32 * b[5] as i32),
        i32::wrapping_add(a[6] as i32 * b[6] as i32, a[7] as i32 * b[7] as i32)
    ])));
    call_next(tail_code, 0, ctx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn q15mulr_sat_s_saturates_min_times_min() {
        assert_eq!(q15mulr_sat_s_lane(i16::MIN, i16::MIN), i16::MAX);
    }
}
