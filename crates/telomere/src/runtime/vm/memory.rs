use super::*;

#[inline(always)]
fn profile_memory_family(_label: &'static str) {}

#[inline(always)]
fn truncate_u32_to_u8_bytes(value: u32) -> [u8; 1] {
    [(value & 0xff) as u8]
}

#[inline(always)]
fn truncate_u32_to_u16_bytes(value: u32) -> [u8; 2] {
    [(value & 0xff) as u8, ((value >> 8) & 0xff) as u8]
}

#[inline(always)]
fn truncate_u64_to_u8_bytes(value: u64) -> [u8; 1] {
    [(value & 0xff) as u8]
}

#[inline(always)]
fn truncate_u64_to_u16_bytes(value: u64) -> [u8; 2] {
    [(value & 0xff) as u8, ((value >> 8) & 0xff) as u8]
}

#[inline(always)]
fn truncate_u64_to_u32_bytes(value: u64) -> [u8; 4] {
    [
        (value & 0xff) as u8,
        ((value >> 8) & 0xff) as u8,
        ((value >> 16) & 0xff) as u8,
        ((value >> 24) & 0xff) as u8,
    ]
}

#[inline(always)]
/// WebAssembly linear-memory access helper.
///
/// Spec:
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: consumes the address operand and computes an effective memory offset.
/// Traps: traps on memory index overflow when computing the effective address.
/// Notes: Reads the memarg from the active instruction and reuses the validated operand stack layout.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction stream for the current active frame.
/// - `ctx` must reference a live execution context whose validated stack layout matches this memory instruction.
/// - This helper must not retain borrows across the call boundary into memory access helpers.
unsafe fn load_start(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<usize> {
    debug_assert!(ctx.snapshot().has_default_memory());
    let memarg = (*tail_code).operand.memarg;
    let offset = ctx.stack.pop_u32();
    trace!("memory access: {:?} {}", memarg, offset);
    compute_memory_offset(memarg, offset)
}

#[inline(always)]
unsafe fn default_local_push_to_stack<const N: usize>(
    ctx: &mut ExecuteContext,
    offset: usize,
) -> VMResult<()> {
    debug_assert!(!ctx.default_local_memory_ptr.is_null());
    unsafe { (&*ctx.default_local_memory_ptr).push_to_stack::<N>(ctx.stack, offset) }
}

#[inline(always)]
unsafe fn load_start_indexed(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<(usize, u32)> {
    let memarg = (*tail_code).operand.memarg;
    let memidx = (*tail_code.add(1)).operand.u32;
    let offset = ctx.stack.pop_u32();
    trace!(
        "indexed memory access: {:?} {} memidx={}",
        memarg,
        offset,
        memidx
    );
    let start = vm_try!(compute_memory_offset(memarg, offset));
    VMResult::Success((start, memidx))
}

#[inline(always)]
unsafe fn load_start_local_base(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<usize> {
    debug_assert!(ctx.snapshot().has_default_memory());
    let local_addr = (*tail_code).operand.local_addr as usize;
    let delta = (*tail_code.add(1)).operand.i32 as u32;
    let memarg = (*tail_code.add(2)).operand.memarg;
    let offset = ctx
        .stack
        .local_u32_from_base(ctx.local_base_ptr as *const u8, local_addr)
        .wrapping_add(delta);
    compute_memory_offset(memarg, offset)
}

#[inline(always)]
unsafe fn load_start_indexed_local_base(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<(usize, u32)> {
    let local_addr = (*tail_code).operand.local_addr as usize;
    let delta = (*tail_code.add(1)).operand.i32 as u32;
    let memarg = (*tail_code.add(2)).operand.memarg;
    let memidx = (*tail_code.add(3)).operand.u32;
    let offset = ctx
        .stack
        .local_u32_from_base(ctx.local_base_ptr as *const u8, local_addr)
        .wrapping_add(delta);
    let start = vm_try!(compute_memory_offset(memarg, offset));
    VMResult::Success((start, memidx))
}

#[inline(always)]
unsafe fn load_start_local_scaled_index(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<usize> {
    let base_addr = (*tail_code).operand.local_addr as usize;
    let index_addr = (*tail_code.add(1)).operand.local_addr as usize;
    let scale_log2 = (*tail_code.add(2)).operand.u32;
    let delta = (*tail_code.add(3)).operand.i32 as u32;
    let memarg = (*tail_code.add(4)).operand.memarg;
    debug_assert!(scale_log2 <= 3);
    let base = ctx
        .stack
        .local_u32_from_base(ctx.local_base_ptr as *const u8, base_addr);
    let index = ctx
        .stack
        .local_u32_from_base(ctx.local_base_ptr as *const u8, index_addr);
    let offset = base
        .wrapping_add(index.wrapping_shl(scale_log2))
        .wrapping_add(delta);
    compute_memory_offset(memarg, offset)
}

#[inline(always)]
unsafe fn load_start_indexed_local_scaled_index(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<(usize, u32)> {
    let base_addr = (*tail_code).operand.local_addr as usize;
    let index_addr = (*tail_code.add(1)).operand.local_addr as usize;
    let scale_log2 = (*tail_code.add(2)).operand.u32;
    let delta = (*tail_code.add(3)).operand.i32 as u32;
    let memarg = (*tail_code.add(4)).operand.memarg;
    let memidx = (*tail_code.add(5)).operand.u32;
    debug_assert!(scale_log2 <= 3);
    let base = ctx
        .stack
        .local_u32_from_base(ctx.local_base_ptr as *const u8, base_addr);
    let index = ctx
        .stack
        .local_u32_from_base(ctx.local_base_ptr as *const u8, index_addr);
    let offset = base
        .wrapping_add(index.wrapping_shl(scale_log2))
        .wrapping_add(delta);
    let start = vm_try!(compute_memory_offset(memarg, offset));
    VMResult::Success((start, memidx))
}

#[inline(always)]
unsafe fn load_start_shared_local_scaled_index(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<usize> {
    let base_addr = (*tail_code).operand.local_addr as usize;
    let index_addr = (*tail_code.add(1)).operand.local_addr as usize;
    let scale_log2 = (*tail_code.add(2)).operand.u32;
    let delta = (*tail_code.add(3)).operand.i32 as u32;
    let memarg = (*tail_code.add(4)).operand.memarg;
    debug_assert!(scale_log2 <= 3);
    let base = ctx
        .stack
        .local_u32_from_base(ctx.local_base_ptr as *const u8, base_addr);
    let index = ctx
        .stack
        .local_u32_from_base(ctx.local_base_ptr as *const u8, index_addr);
    let offset = base
        .wrapping_add(index.wrapping_shl(scale_log2))
        .wrapping_add(delta);
    compute_memory_offset(memarg, offset)
}

#[inline(always)]
unsafe fn load_start_indexed_shared_local_scaled_index(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<(usize, u32)> {
    let base_addr = (*tail_code).operand.local_addr as usize;
    let index_addr = (*tail_code.add(1)).operand.local_addr as usize;
    let scale_log2 = (*tail_code.add(2)).operand.u32;
    let delta = (*tail_code.add(3)).operand.i32 as u32;
    let memarg = (*tail_code.add(4)).operand.memarg;
    let memidx = (*tail_code.add(5)).operand.u32;
    debug_assert!(scale_log2 <= 3);
    let base = ctx
        .stack
        .local_u32_from_base(ctx.local_base_ptr as *const u8, base_addr);
    let index = ctx
        .stack
        .local_u32_from_base(ctx.local_base_ptr as *const u8, index_addr);
    let offset = base
        .wrapping_add(index.wrapping_shl(scale_log2))
        .wrapping_add(delta);
    let start = vm_try!(compute_memory_offset(memarg, offset));
    VMResult::Success((start, memidx))
}

#[inline(always)]
#[allow(dead_code)]
unsafe fn store_internal_local_base(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    _label: &'static str,
    make_operation: impl FnOnce(&mut ExecuteContext) -> StoreBytes,
) -> VMResult<()> {
    let local_addr = (*tail_code).operand.local_addr as usize;
    let delta = (*tail_code.add(1)).operand.i32 as u32;
    let memarg = (*tail_code.add(2)).operand.memarg;
    let operation = make_operation(ctx);
    let offset = ctx
        .stack
        .local_u32_from_base(ctx.local_base_ptr as *const u8, local_addr)
        .wrapping_add(delta);
    let bytes = operation.as_slice();
    let start = vm_try!(compute_memory_offset(memarg, offset));
    vm_try!(unsafe { ctx.default_local_memory_mut_unchecked() }.write_bytes(start, bytes));
    call_next(tail_code, 3, ctx)
}

#[inline(always)]
#[allow(dead_code)]
unsafe fn store_internal_indexed_local_base(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    _label: &'static str,
    make_operation: impl FnOnce(&mut ExecuteContext) -> StoreBytes,
) -> VMResult<()> {
    let local_addr = (*tail_code).operand.local_addr as usize;
    let delta = (*tail_code.add(1)).operand.i32 as u32;
    let memarg = (*tail_code.add(2)).operand.memarg;
    let memidx = (*tail_code.add(3)).operand.u32;
    let operation = make_operation(ctx);
    let offset = ctx
        .stack
        .local_u32_from_base(ctx.local_base_ptr as *const u8, local_addr)
        .wrapping_add(delta);
    let bytes = operation.as_slice();
    let start = vm_try!(compute_memory_offset(memarg, offset));
    vm_try!(ctx
        .gc
        .local_write_bytes(ctx.local_memory_id_at_unchecked(memidx), start, bytes));
    call_next(tail_code, 4, ctx)
}

#[inline(always)]
#[allow(dead_code)]
unsafe fn store_internal_shared_local_base(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    _label: &'static str,
    make_operation: impl FnOnce(&mut ExecuteContext) -> StoreBytes,
) -> VMResult<()> {
    let start = vm_try!(load_start_local_base(tail_code, ctx));
    let operation = make_operation(ctx);
    let bytes = operation.as_slice();
    vm_try!(ctx
        .gc
        .shared_write_bytes(ctx.default_shared_memory_id_unchecked(), start, bytes));
    call_next(tail_code, 3, ctx)
}

#[inline(always)]
#[allow(dead_code)]
unsafe fn store_internal_indexed_shared_local_base(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    _label: &'static str,
    make_operation: impl FnOnce(&mut ExecuteContext) -> StoreBytes,
) -> VMResult<()> {
    let (start, memidx) = vm_try!(load_start_indexed_local_base(tail_code, ctx));
    let operation = make_operation(ctx);
    let bytes = operation.as_slice();
    vm_try!(ctx
        .gc
        .shared_write_bytes(ctx.shared_memory_id_at_unchecked(memidx), start, bytes));
    call_next(tail_code, 4, ctx)
}

#[inline(always)]
#[allow(dead_code)]
unsafe fn store_internal_local_scaled_index(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    _label: &'static str,
    make_operation: impl FnOnce(&mut ExecuteContext) -> StoreBytes,
) -> VMResult<()> {
    let start = vm_try!(load_start_local_scaled_index(tail_code, ctx));
    let operation = make_operation(ctx);
    let bytes = operation.as_slice();
    vm_try!(unsafe { ctx.default_local_memory_mut_unchecked() }.write_bytes(start, bytes));
    call_next(tail_code, 5, ctx)
}

#[inline(always)]
#[allow(dead_code)]
unsafe fn store_internal_shared_local_scaled_index(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    _label: &'static str,
    make_operation: impl FnOnce(&mut ExecuteContext) -> StoreBytes,
) -> VMResult<()> {
    let start = vm_try!(load_start_shared_local_scaled_index(tail_code, ctx));
    let operation = make_operation(ctx);
    let bytes = operation.as_slice();
    vm_try!(ctx
        .gc
        .shared_write_bytes(ctx.default_shared_memory_id_unchecked(), start, bytes));
    call_next(tail_code, 5, ctx)
}

#[inline(always)]
#[allow(dead_code)]
unsafe fn store_internal_indexed_local_scaled_index(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    _label: &'static str,
    make_operation: impl FnOnce(&mut ExecuteContext) -> StoreBytes,
) -> VMResult<()> {
    let (start, memidx) = vm_try!(load_start_indexed_local_scaled_index(tail_code, ctx));
    let operation = make_operation(ctx);
    let bytes = operation.as_slice();
    vm_try!(ctx
        .gc
        .local_write_bytes(ctx.local_memory_id_at_unchecked(memidx), start, bytes));
    call_next(tail_code, 6, ctx)
}

#[inline(always)]
#[allow(dead_code)]
unsafe fn store_internal_indexed_shared_local_scaled_index(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    _label: &'static str,
    make_operation: impl FnOnce(&mut ExecuteContext) -> StoreBytes,
) -> VMResult<()> {
    let (start, memidx) = vm_try!(load_start_indexed_shared_local_scaled_index(tail_code, ctx));
    let operation = make_operation(ctx);
    let bytes = operation.as_slice();
    vm_try!(ctx
        .gc
        .shared_write_bytes(ctx.shared_memory_id_at_unchecked(memidx), start, bytes));
    call_next(tail_code, 6, ctx)
}

#[inline(always)]
unsafe fn load_start_shared_local_base(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<usize> {
    debug_assert!(ctx.snapshot().has_default_memory());
    let local_addr = (*tail_code).operand.local_addr as usize;
    let delta = (*tail_code.add(1)).operand.i32 as u32;
    let memarg = (*tail_code.add(2)).operand.memarg;
    let offset = ctx
        .stack
        .local_u32_from_base(ctx.local_base_ptr as *const u8, local_addr)
        .wrapping_add(delta);
    compute_memory_offset(memarg, offset)
}

#[inline(always)]
unsafe fn load_start_indexed_shared_local_base(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<(usize, u32)> {
    let local_addr = (*tail_code).operand.local_addr as usize;
    let delta = (*tail_code.add(1)).operand.i32 as u32;
    let memarg = (*tail_code.add(2)).operand.memarg;
    let memidx = (*tail_code.add(3)).operand.u32;
    let offset = ctx
        .stack
        .local_u32_from_base(ctx.local_base_ptr as *const u8, local_addr)
        .wrapping_add(delta);
    let start = vm_try!(compute_memory_offset(memarg, offset));
    VMResult::Success((start, memidx))
}

macro_rules! define_indexed_push_load {
    ($local:ident, $shared:ident, $mnemonic:literal, $bytes:expr) => {
        #[doc = concat!("WebAssembly `", $mnemonic, "` on indexed local memory.")]
        ///
        /// Spec:
        /// - Syntax: https://webassembly.github.io/multi-memory/core/syntax/instructions.html
        /// - Validation: https://webassembly.github.io/multi-memory/core/valid/instructions.html
        /// - Execution: https://webassembly.github.io/multi-memory/core/exec/instructions.html
        ///
        /// Stack effect: `[i32] -> [value]`.
        /// Traps: traps on out-of-bounds memory access.
        /// Notes: Uses the typed indexed local-memory fast path and tail-dispatches with `call_next`.
        ///
        /// # Safety
        /// - `tail_code` must point to the decoded instruction for this handler in the active function body.
        /// - `ctx` must reference a live execution context whose operand stack satisfies this instruction.
        /// - The memory index operand must be in-bounds and refer to a local memory.
        pub unsafe fn $local(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            let (start, memidx) = vm_try!(load_start_indexed(tail_code, ctx));
            vm_try!(ctx.gc.local_push_memory_to_stack::<$bytes>(
                ctx.local_memory_id_at_unchecked(memidx),
                ctx.stack,
                start,
            ));
            call_next(tail_code, 2, ctx)
        }

        #[doc = concat!("WebAssembly `", $mnemonic, "` on indexed shared memory.")]
        ///
        /// Spec:
        /// - Syntax: https://webassembly.github.io/multi-memory/core/syntax/instructions.html
        /// - Validation: https://webassembly.github.io/multi-memory/core/valid/instructions.html
        /// - Execution: https://webassembly.github.io/multi-memory/core/exec/instructions.html
        ///
        /// Stack effect: `[i32] -> [value]`.
        /// Traps: traps on out-of-bounds memory access.
        /// Notes: Uses the typed indexed shared-memory fast path and tail-dispatches with `call_next`.
        ///
        /// # Safety
        /// - `tail_code` must point to the decoded instruction for this handler in the active function body.
        /// - `ctx` must reference a live execution context whose operand stack satisfies this instruction.
        /// - The memory index operand must be in-bounds and refer to a shared memory.
        pub unsafe fn $shared(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            let (start, memidx) = vm_try!(load_start_indexed(tail_code, ctx));
            vm_try!(ctx.gc.shared_push_memory_to_stack::<$bytes>(
                ctx.shared_memory_id_at_unchecked(memidx),
                ctx.stack,
                start,
            ));
            call_next(tail_code, 2, ctx)
        }
    };
}

macro_rules! define_indexed_scalar_load {
    ($local:ident, $shared:ident, $mnemonic:literal, $local_reader:ident, $shared_reader:ident, $push:ident, $convert:path) => {
        #[doc = concat!("WebAssembly `", $mnemonic, "` on indexed local memory.")]
        ///
        /// Spec:
        /// - Syntax: https://webassembly.github.io/multi-memory/core/syntax/instructions.html
        /// - Validation: https://webassembly.github.io/multi-memory/core/valid/instructions.html
        /// - Execution: https://webassembly.github.io/multi-memory/core/exec/instructions.html
        ///
        /// Stack effect: `[i32] -> [value]`.
        /// Traps: traps on out-of-bounds memory access.
        /// Notes: Uses the typed indexed local-memory fast path and tail-dispatches with `call_next`.
        ///
        /// # Safety
        /// - `tail_code` must point to the decoded instruction for this handler in the active function body.
        /// - `ctx` must reference a live execution context whose operand stack satisfies this instruction.
        /// - The memory index operand must be in-bounds and refer to a local memory.
        pub unsafe fn $local(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            let (start, memidx) = vm_try!(load_start_indexed(tail_code, ctx));
            let value = vm_try!(ctx
                .gc
                .$local_reader(ctx.local_memory_id_at_unchecked(memidx), start));
            vm_try!(ctx.stack.$push($convert(value)));
            call_next(tail_code, 2, ctx)
        }

        #[doc = concat!("WebAssembly `", $mnemonic, "` on indexed shared memory.")]
        ///
        /// Spec:
        /// - Syntax: https://webassembly.github.io/multi-memory/core/syntax/instructions.html
        /// - Validation: https://webassembly.github.io/multi-memory/core/valid/instructions.html
        /// - Execution: https://webassembly.github.io/multi-memory/core/exec/instructions.html
        ///
        /// Stack effect: `[i32] -> [value]`.
        /// Traps: traps on out-of-bounds memory access.
        /// Notes: Uses the typed indexed shared-memory fast path and tail-dispatches with `call_next`.
        ///
        /// # Safety
        /// - `tail_code` must point to the decoded instruction for this handler in the active function body.
        /// - `ctx` must reference a live execution context whose operand stack satisfies this instruction.
        /// - The memory index operand must be in-bounds and refer to a shared memory.
        pub unsafe fn $shared(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            let (start, memidx) = vm_try!(load_start_indexed(tail_code, ctx));
            let value = vm_try!(ctx
                .gc
                .$shared_reader(ctx.shared_memory_id_at_unchecked(memidx), start));
            vm_try!(ctx.stack.$push($convert(value)));
            call_next(tail_code, 2, ctx)
        }
    };
}

macro_rules! define_indexed_store_alias {
    ($local:ident, $shared:ident, $mnemonic:literal, $make_operation:expr) => {
        #[doc = concat!("WebAssembly `", $mnemonic, "` on indexed local memory.")]
        ///
        /// Spec:
        /// - Syntax: https://webassembly.github.io/multi-memory/core/syntax/instructions.html
        /// - Validation: https://webassembly.github.io/multi-memory/core/valid/instructions.html
        /// - Execution: https://webassembly.github.io/multi-memory/core/exec/instructions.html
        ///
        /// Stack effect: `[i32, value] -> []`.
        /// Traps: traps on out-of-bounds memory access.
        /// Notes: Uses the typed indexed local-memory fast path and tail-dispatches with `call_next`.
        ///
        /// # Safety
        /// - `tail_code` must point to the decoded instruction for this handler in the active function body.
        /// - `ctx` must reference a live execution context whose operand stack satisfies this instruction.
        /// - The memory index operand must be in-bounds and refer to a local memory.
        pub unsafe fn $local(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            store_internal_local_indexed(tail_code, ctx, stringify!($local), $make_operation)
        }

        #[doc = concat!("WebAssembly `", $mnemonic, "` on indexed shared memory.")]
        ///
        /// Spec:
        /// - Syntax: https://webassembly.github.io/multi-memory/core/syntax/instructions.html
        /// - Validation: https://webassembly.github.io/multi-memory/core/valid/instructions.html
        /// - Execution: https://webassembly.github.io/multi-memory/core/exec/instructions.html
        ///
        /// Stack effect: `[i32, value] -> []`.
        /// Traps: traps on out-of-bounds memory access.
        /// Notes: Uses the typed indexed shared-memory fast path and tail-dispatches with `call_next`.
        ///
        /// # Safety
        /// - `tail_code` must point to the decoded instruction for this handler in the active function body.
        /// - `ctx` must reference a live execution context whose operand stack satisfies this instruction.
        /// - The memory index operand must be in-bounds and refer to a shared memory.
        pub unsafe fn $shared(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            store_internal_shared_indexed(tail_code, ctx, stringify!($shared), $make_operation)
        }
    };
}

macro_rules! define_local_base_push_load {
    ($name:ident, $mnemonic:literal, $bytes:expr) => {
        #[doc = concat!("WebAssembly `", $mnemonic, "` with local-base address on default local memory.")]
        pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            profile_memory_family(stringify!($name));
            let start = vm_try!(load_start_local_base(tail_code, ctx));
            vm_try!(default_local_push_to_stack::<$bytes>(ctx, start));
            call_next(tail_code, 3, ctx)
        }
    };
}

macro_rules! define_indexed_local_base_push_load {
    ($name:ident, $mnemonic:literal, $bytes:expr) => {
        #[doc = concat!("WebAssembly `", $mnemonic, "` with local-base address on indexed local memory.")]
        pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            profile_memory_family(stringify!($name));
            let (start, memidx) = vm_try!(load_start_indexed_local_base(tail_code, ctx));
            vm_try!(ctx.gc.local_push_memory_to_stack::<$bytes>(
                ctx.local_memory_id_at_unchecked(memidx),
                ctx.stack,
                start,
            ));
            call_next(tail_code, 4, ctx)
        }
    };
}

macro_rules! define_local_base_scalar_load {
    ($name:ident, $mnemonic:literal, $reader:ident, $push:ident, $convert:path) => {
        #[doc = concat!("WebAssembly `", $mnemonic, "` with local-base address on default local memory.")]
        pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            profile_memory_family(stringify!($name));
            let start = vm_try!(load_start_local_base(tail_code, ctx));
            let value = vm_try!(unsafe { ctx.default_local_memory_unchecked() }.$reader(start));
            vm_try!(ctx.stack.$push($convert(value)));
            call_next(tail_code, 3, ctx)
        }
    };
}

macro_rules! define_indexed_local_base_scalar_load {
    ($name:ident, $mnemonic:literal, $reader:ident, $push:ident, $convert:path) => {
        #[doc = concat!("WebAssembly `", $mnemonic, "` with local-base address on indexed local memory.")]
        pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            profile_memory_family(stringify!($name));
            let (start, memidx) = vm_try!(load_start_indexed_local_base(tail_code, ctx));
            let value = vm_try!(ctx.gc.$reader(ctx.local_memory_id_at_unchecked(memidx), start));
            vm_try!(ctx.stack.$push($convert(value)));
            call_next(tail_code, 4, ctx)
        }
    };
}

macro_rules! define_local_base_store_alias {
    ($name:ident, $make_operation:expr) => {
        #[allow(dead_code)]
        pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            store_internal_local_base(tail_code, ctx, stringify!($name), $make_operation)
        }
    };
}

macro_rules! define_indexed_local_base_store_alias {
    ($name:ident, $make_operation:expr) => {
        #[allow(dead_code)]
        pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            store_internal_indexed_local_base(tail_code, ctx, stringify!($name), $make_operation)
        }
    };
}

macro_rules! define_shared_local_base_push_load {
    ($name:ident, $mnemonic:literal, $bytes:expr) => {
        #[doc = concat!("WebAssembly `", $mnemonic, "` with local-base address on shared default memory.")]
        pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            profile_memory_family(stringify!($name));
            let start = vm_try!(load_start_shared_local_base(tail_code, ctx));
            vm_try!(ctx.gc.shared_push_memory_to_stack::<$bytes>(
                ctx.default_shared_memory_id_unchecked(),
                ctx.stack,
                start,
            ));
            call_next(tail_code, 3, ctx)
        }
    };
}

macro_rules! define_indexed_shared_local_base_push_load {
    ($name:ident, $mnemonic:literal, $bytes:expr) => {
        #[doc = concat!("WebAssembly `", $mnemonic, "` with local-base address on indexed shared memory.")]
        pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            profile_memory_family(stringify!($name));
            let (start, memidx) = vm_try!(load_start_indexed_shared_local_base(tail_code, ctx));
            vm_try!(ctx.gc.shared_push_memory_to_stack::<$bytes>(
                ctx.shared_memory_id_at_unchecked(memidx),
                ctx.stack,
                start,
            ));
            call_next(tail_code, 4, ctx)
        }
    };
}

macro_rules! define_shared_local_base_scalar_load {
    ($name:ident, $mnemonic:literal, $reader:ident, $push:ident, $convert:path) => {
        #[doc = concat!("WebAssembly `", $mnemonic, "` with local-base address on shared default memory.")]
        pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            profile_memory_family(stringify!($name));
            let start = vm_try!(load_start_shared_local_base(tail_code, ctx));
            let value = vm_try!(ctx
                .gc
                .$reader(ctx.default_shared_memory_id_unchecked(), start));
            vm_try!(ctx.stack.$push($convert(value)));
            call_next(tail_code, 3, ctx)
        }
    };
}

macro_rules! define_indexed_shared_local_base_scalar_load {
    ($name:ident, $mnemonic:literal, $reader:ident, $push:ident, $convert:path) => {
        #[doc = concat!("WebAssembly `", $mnemonic, "` with local-base address on indexed shared memory.")]
        pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            profile_memory_family(stringify!($name));
            let (start, memidx) = vm_try!(load_start_indexed_shared_local_base(tail_code, ctx));
            let value = vm_try!(ctx.gc.$reader(ctx.shared_memory_id_at_unchecked(memidx), start));
            vm_try!(ctx.stack.$push($convert(value)));
            call_next(tail_code, 4, ctx)
        }
    };
}

macro_rules! define_shared_local_base_store_alias {
    ($name:ident, $make_operation:expr) => {
        #[allow(dead_code)]
        pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            store_internal_shared_local_base(tail_code, ctx, stringify!($name), $make_operation)
        }
    };
}

macro_rules! define_indexed_shared_local_base_store_alias {
    ($name:ident, $make_operation:expr) => {
        #[allow(dead_code)]
        pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            store_internal_indexed_shared_local_base(
                tail_code,
                ctx,
                stringify!($name),
                $make_operation,
            )
        }
    };
}

macro_rules! define_local_scaled_index_push_load {
    ($name:ident, $mnemonic:literal, $bytes:expr) => {
        #[doc = concat!("WebAssembly `", $mnemonic, "` with scaled-index address on default local memory.")]
        pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            profile_memory_family(stringify!($name));
            let start = vm_try!(load_start_local_scaled_index(tail_code, ctx));
            vm_try!(default_local_push_to_stack::<$bytes>(ctx, start));
            call_next(tail_code, 5, ctx)
        }
    };
}

macro_rules! define_indexed_local_scaled_index_push_load {
    ($name:ident, $mnemonic:literal, $bytes:expr) => {
        #[doc = concat!("WebAssembly `", $mnemonic, "` with scaled-index address on indexed local memory.")]
        pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            profile_memory_family(stringify!($name));
            let (start, memidx) = vm_try!(load_start_indexed_local_scaled_index(tail_code, ctx));
            vm_try!(ctx.gc.local_push_memory_to_stack::<$bytes>(
                ctx.local_memory_id_at_unchecked(memidx),
                ctx.stack,
                start,
            ));
            call_next(tail_code, 6, ctx)
        }
    };
}

macro_rules! define_shared_local_scaled_index_push_load {
    ($name:ident, $mnemonic:literal, $bytes:expr) => {
        #[doc = concat!("WebAssembly `", $mnemonic, "` with scaled-index address on shared default memory.")]
        pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            profile_memory_family(stringify!($name));
            let start = vm_try!(load_start_shared_local_scaled_index(tail_code, ctx));
            vm_try!(ctx.gc.shared_push_memory_to_stack::<$bytes>(
                ctx.default_shared_memory_id_unchecked(),
                ctx.stack,
                start,
            ));
            call_next(tail_code, 5, ctx)
        }
    };
}

macro_rules! define_indexed_shared_local_scaled_index_push_load {
    ($name:ident, $mnemonic:literal, $bytes:expr) => {
        #[doc = concat!("WebAssembly `", $mnemonic, "` with scaled-index address on indexed shared memory.")]
        pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            profile_memory_family(stringify!($name));
            let (start, memidx) =
                vm_try!(load_start_indexed_shared_local_scaled_index(tail_code, ctx));
            vm_try!(ctx.gc.shared_push_memory_to_stack::<$bytes>(
                ctx.shared_memory_id_at_unchecked(memidx),
                ctx.stack,
                start,
            ));
            call_next(tail_code, 6, ctx)
        }
    };
}

macro_rules! define_local_scaled_index_scalar_load {
    ($name:ident, $mnemonic:literal, $reader:ident, $push:ident, $convert:path) => {
        #[doc = concat!("WebAssembly `", $mnemonic, "` with scaled-index address on default local memory.")]
        pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            profile_memory_family(stringify!($name));
            let start = vm_try!(load_start_local_scaled_index(tail_code, ctx));
            let value = vm_try!(unsafe { ctx.default_local_memory_unchecked() }.$reader(start));
            vm_try!(ctx.stack.$push($convert(value)));
            call_next(tail_code, 5, ctx)
        }
    };
}

macro_rules! define_indexed_local_scaled_index_scalar_load {
    ($name:ident, $mnemonic:literal, $reader:ident, $push:ident, $convert:path) => {
        #[doc = concat!("WebAssembly `", $mnemonic, "` with scaled-index address on indexed local memory.")]
        pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            profile_memory_family(stringify!($name));
            let (start, memidx) = vm_try!(load_start_indexed_local_scaled_index(tail_code, ctx));
            let value = vm_try!(ctx.gc.$reader(ctx.local_memory_id_at_unchecked(memidx), start));
            vm_try!(ctx.stack.$push($convert(value)));
            call_next(tail_code, 6, ctx)
        }
    };
}

macro_rules! define_shared_local_scaled_index_scalar_load {
    ($name:ident, $mnemonic:literal, $reader:ident, $push:ident, $convert:path) => {
        #[doc = concat!("WebAssembly `", $mnemonic, "` with scaled-index address on shared default memory.")]
        pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            profile_memory_family(stringify!($name));
            let start = vm_try!(load_start_shared_local_scaled_index(tail_code, ctx));
            let value = vm_try!(ctx
                .gc
                .$reader(ctx.default_shared_memory_id_unchecked(), start));
            vm_try!(ctx.stack.$push($convert(value)));
            call_next(tail_code, 5, ctx)
        }
    };
}

macro_rules! define_indexed_shared_local_scaled_index_scalar_load {
    ($name:ident, $mnemonic:literal, $reader:ident, $push:ident, $convert:path) => {
        #[doc = concat!("WebAssembly `", $mnemonic, "` with scaled-index address on indexed shared memory.")]
        pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            profile_memory_family(stringify!($name));
            let (start, memidx) =
                vm_try!(load_start_indexed_shared_local_scaled_index(tail_code, ctx));
            let value = vm_try!(ctx.gc.$reader(ctx.shared_memory_id_at_unchecked(memidx), start));
            vm_try!(ctx.stack.$push($convert(value)));
            call_next(tail_code, 6, ctx)
        }
    };
}

macro_rules! define_local_scaled_index_store_alias {
    ($name:ident, $make_operation:expr) => {
        #[allow(dead_code)]
        pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            store_internal_local_scaled_index(tail_code, ctx, stringify!($name), $make_operation)
        }
    };
}

macro_rules! define_indexed_local_scaled_index_store_alias {
    ($name:ident, $make_operation:expr) => {
        #[allow(dead_code)]
        pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            store_internal_indexed_local_scaled_index(
                tail_code,
                ctx,
                stringify!($name),
                $make_operation,
            )
        }
    };
}

macro_rules! define_shared_local_scaled_index_store_alias {
    ($name:ident, $make_operation:expr) => {
        #[allow(dead_code)]
        pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            store_internal_shared_local_scaled_index(
                tail_code,
                ctx,
                stringify!($name),
                $make_operation,
            )
        }
    };
}

macro_rules! define_indexed_shared_local_scaled_index_store_alias {
    ($name:ident, $make_operation:expr) => {
        #[allow(dead_code)]
        pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            store_internal_indexed_shared_local_scaled_index(
                tail_code,
                ctx,
                stringify!($name),
                $make_operation,
            )
        }
    };
}

/// WebAssembly `i32.load`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[i32] -> [value]`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Uses little-endian linear-memory access and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_load(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    profile_memory_family("op_i32_load");
    let start = vm_try!(load_start(tail_code, ctx));
    vm_try!(default_local_push_to_stack::<4>(ctx, start));
    call_next(tail_code, 1, ctx)
}

/// WebAssembly `i32.load` const-base fast path for default local memory.
///
/// Spec:
/// - Validation: equivalent to a validated `i32.const <addr>; i32.load` sequence.
/// - Execution: uses the folded `memarg.offset` as the effective address.
///
/// Stack effect: `[] -> [i32]`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: This handler exists only for load-time-specialized const-base scalar loads and keeps tail dispatch unchanged.
///
/// # Safety
/// - `tail_code` must point to a decoded instruction whose folded `memarg` came from a validated const-base `i32.load`.
/// - `ctx` must reference a live execution context with a valid default local memory.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_load_const_base(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    profile_memory_family("op_i32_load_const_base");
    let memarg = (*tail_code).operand.memarg;
    let start = vm_try!(compute_memory_offset(memarg, 0));
    let value = vm_try!(unsafe { ctx.default_local_memory_unchecked() }.read_u32_at(start));
    vm_try!(ctx.stack.push_u32_fast(value));
    call_next(tail_code, 1, ctx)
}

/// WebAssembly `i32.store` const-base fast path for default local memory with a local-backed value.
///
/// Spec:
/// - Validation: equivalent to a validated `i32.const <addr>; local.get; i32.store` sequence.
/// - Execution: uses the folded `memarg.offset` as the effective address and reads the value directly from the local slot.
///
/// Stack effect: `[] -> []`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: This handler removes both the address materialization and the store-value stack roundtrip.
///
/// # Safety
/// - `tail_code` must point to a decoded instruction whose operands came from a validated const-base `i32.store` pattern.
/// - `ctx` must reference a live execution context with a valid default local memory and local area.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_store_const_base_local4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    profile_memory_family("op_i32_store_const_base_local4");
    let memarg = (*tail_code).operand.memarg;
    let src = (*tail_code.add(1)).operand.local_addr as usize;
    let start = vm_try!(compute_memory_offset(memarg, 0));
    let value = ctx
        .stack
        .local_u32_from_base(ctx.local_base_ptr as *const u8, src);
    vm_try!(unsafe { ctx.default_local_memory_mut_unchecked() }
        .write_bytes(start, &value.to_le_bytes()));
    call_next(tail_code, 2, ctx)
}

/// WebAssembly `i32.load + local.get4 + i32.add + local.set4` fused const-base fast path.
///
/// Spec:
/// - Validation: equivalent to a validated `i32.const <addr>; i32.load; local.get; i32.add; local.set` sequence.
/// - Execution: loads from default local memory using the folded `memarg.offset`, adds a local-backed rhs, and writes the result directly to the destination local.
///
/// Stack effect: `[] -> []`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: This is a bounded cross-family fusion used only for the default local-memory scalar path.
///
/// # Safety
/// - `tail_code` must point to a decoded instruction whose operands came from a validated fused const-base pattern.
/// - `ctx` must reference a live execution context with a valid default local memory and local area.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_load_const_base_local_get4_i32_add_set4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    profile_memory_family("op_i32_load_const_base_local_get4_i32_add_set4");
    let memarg = (*tail_code).operand.memarg;
    let rhs = (*tail_code.add(1)).operand.local_addr as usize;
    let dst = (*tail_code.add(2)).operand.local_addr as usize;
    let start = vm_try!(compute_memory_offset(memarg, 0));
    let loaded = vm_try!(unsafe { ctx.default_local_memory_unchecked() }.read_u32_at(start));
    let rhs = ctx
        .stack
        .local_u32_from_base(ctx.local_base_ptr as *const u8, rhs);
    let result = loaded.wrapping_add(rhs);
    ctx.stack
        .local_set4_from_base_value(ctx.local_base_ptr, dst, result);
    call_next(tail_code, 3, ctx)
}

/// WebAssembly `i64.load`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[i32] -> [value]`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Uses little-endian linear-memory access and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_load(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    profile_memory_family("op_i64_load");
    let start = vm_try!(load_start(tail_code, ctx));
    vm_try!(default_local_push_to_stack::<8>(ctx, start));
    call_next(tail_code, 1, ctx)
}

/// WebAssembly `f32.load`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[i32] -> [value]`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Uses little-endian linear-memory access and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f32_load(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    profile_memory_family("op_f32_load");
    let start = vm_try!(load_start(tail_code, ctx));
    vm_try!(default_local_push_to_stack::<4>(ctx, start));
    call_next(tail_code, 1, ctx)
}

/// WebAssembly `f64.load`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[i32] -> [value]`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Uses little-endian linear-memory access and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f64_load(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    profile_memory_family("op_f64_load");
    let start = vm_try!(load_start(tail_code, ctx));
    vm_try!(default_local_push_to_stack::<8>(ctx, start));
    call_next(tail_code, 1, ctx)
}

/// WebAssembly `i32.load8_u`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[i32] -> [value]`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Uses little-endian linear-memory access and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_load8_u(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    profile_memory_family("op_i32_load8_u");
    let start = vm_try!(load_start(tail_code, ctx));
    let value = vm_try!(unsafe { ctx.default_local_memory_unchecked() }.read_u8_at(start));
    vm_try!(ctx.stack.push_u32_fast(u32::from(value)));
    call_next(tail_code, 1, ctx)
}

/// WebAssembly `i32.load8_s`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[i32] -> [value]`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Uses little-endian linear-memory access and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_load8_s(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    profile_memory_family("op_i32_load8_s");
    let start = vm_try!(load_start(tail_code, ctx));
    let value = vm_try!(unsafe { ctx.default_local_memory_unchecked() }.read_i8_at(start));
    vm_try!(ctx.stack.push_i32_fast(i32::from(value)));
    call_next(tail_code, 1, ctx)
}

/// WebAssembly `i32.load16_s`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[i32] -> [value]`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Uses little-endian linear-memory access and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_load16_s(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    profile_memory_family("op_i32_load16_s");
    let start = vm_try!(load_start(tail_code, ctx));
    let value = vm_try!(unsafe { ctx.default_local_memory_unchecked() }.read_i16_at(start));
    vm_try!(ctx.stack.push_i32_fast(i32::from(value)));
    call_next(tail_code, 1, ctx)
}

/// WebAssembly `i32.load16_u`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[i32] -> [value]`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Uses little-endian linear-memory access and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_load16_u(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    profile_memory_family("op_i32_load16_u");
    let start = vm_try!(load_start(tail_code, ctx));
    let value = vm_try!(unsafe { ctx.default_local_memory_unchecked() }.read_u16_at(start));
    vm_try!(ctx.stack.push_u32_fast(u32::from(value)));
    call_next(tail_code, 1, ctx)
}

/// WebAssembly `i64.load8_s`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[i32] -> [value]`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Uses little-endian linear-memory access and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_load8_s(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    profile_memory_family("op_i64_load8_s");
    let start = vm_try!(load_start(tail_code, ctx));
    let value = vm_try!(unsafe { ctx.default_local_memory_unchecked() }.read_i8_at(start));
    vm_try!(ctx.stack.push_i64(i64::from(value)));
    call_next(tail_code, 1, ctx)
}

/// WebAssembly `i64.load8_u`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[i32] -> [value]`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Uses little-endian linear-memory access and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_load8_u(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    profile_memory_family("op_i64_load8_u");
    let start = vm_try!(load_start(tail_code, ctx));
    let value = vm_try!(unsafe { ctx.default_local_memory_unchecked() }.read_u8_at(start));
    vm_try!(ctx.stack.push_u64(u64::from(value)));
    call_next(tail_code, 1, ctx)
}

/// WebAssembly `i64.load16_s`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[i32] -> [value]`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Uses little-endian linear-memory access and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_load16_s(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    profile_memory_family("op_i64_load16_s");
    let start = vm_try!(load_start(tail_code, ctx));
    let value = vm_try!(unsafe { ctx.default_local_memory_unchecked() }.read_i16_at(start));
    vm_try!(ctx.stack.push_i64(i64::from(value)));
    call_next(tail_code, 1, ctx)
}

/// WebAssembly `i64.load16_u`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[i32] -> [value]`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Uses little-endian linear-memory access and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_load16_u(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    profile_memory_family("op_i64_load16_u");
    let start = vm_try!(load_start(tail_code, ctx));
    let value = vm_try!(unsafe { ctx.default_local_memory_unchecked() }.read_u16_at(start));
    vm_try!(ctx.stack.push_u64(u64::from(value)));
    call_next(tail_code, 1, ctx)
}

/// WebAssembly `i64.load32_s`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[i32] -> [value]`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Uses little-endian linear-memory access and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_load32_s(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    profile_memory_family("op_i64_load32_s");
    let start = vm_try!(load_start(tail_code, ctx));
    let value = vm_try!(unsafe { ctx.default_local_memory_unchecked() }.read_i32_at(start));
    vm_try!(ctx.stack.push_i64(i64::from(value)));
    call_next(tail_code, 1, ctx)
}

/// WebAssembly `i64.load32_u`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[i32] -> [value]`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Uses little-endian linear-memory access and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_load32_u(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    profile_memory_family("op_i64_load32_u");
    let start = vm_try!(load_start(tail_code, ctx));
    let value = vm_try!(unsafe { ctx.default_local_memory_unchecked() }.read_u32_at(start));
    vm_try!(ctx.stack.push_u64(u64::from(value)));
    call_next(tail_code, 1, ctx)
}

/// WebAssembly `i32.store`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[i32, value] -> []`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Uses little-endian linear-memory access and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_store(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    store_internal_local(tail_code, ctx, "op_i32_store", |ctx| {
        StoreBytes::Write4(ctx.stack.pop_u8_array::<4>())
    })
}

/// WebAssembly `i64.store`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[i32, value] -> []`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Uses little-endian linear-memory access and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_store(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    store_internal_local(tail_code, ctx, "op_i64_store", |ctx| {
        StoreBytes::Write8(ctx.stack.pop_u8_array::<8>())
    })
}

/// WebAssembly `f32.store`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[i32, value] -> []`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Uses little-endian linear-memory access and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f32_store(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    store_internal_local(tail_code, ctx, "op_f32_store", |ctx| {
        StoreBytes::Write4(ctx.stack.pop_u8_array::<4>())
    })
}

/// WebAssembly `f64.store`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[i32, value] -> []`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Uses little-endian linear-memory access and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f64_store(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    store_internal_local(tail_code, ctx, "op_f64_store", |ctx| {
        StoreBytes::Write8(ctx.stack.pop_u8_array::<8>())
    })
}

/// WebAssembly `i32.store8`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[i32, value] -> []`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Uses little-endian linear-memory access and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_store8(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    store_internal_local(tail_code, ctx, "op_i32_store8", |ctx| {
        StoreBytes::Write1(truncate_u32_to_u8_bytes(ctx.stack.pop_u32()))
    })
}

/// WebAssembly `i32.store16`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[i32, value] -> []`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Uses little-endian linear-memory access and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_store16(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    store_internal_local(tail_code, ctx, "op_i32_store16", |ctx| {
        StoreBytes::Write2(truncate_u32_to_u16_bytes(ctx.stack.pop_u32()))
    })
}

/// WebAssembly `i64.store8`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[i32, value] -> []`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Uses little-endian linear-memory access and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_store8(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    store_internal_local(tail_code, ctx, "op_i64_store8", |ctx| {
        StoreBytes::Write1(truncate_u64_to_u8_bytes(ctx.stack.pop_u64()))
    })
}

/// WebAssembly `i64.store16`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[i32, value] -> []`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Uses little-endian linear-memory access and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_store16(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    store_internal_local(tail_code, ctx, "op_i64_store16", |ctx| {
        StoreBytes::Write2(truncate_u64_to_u16_bytes(ctx.stack.pop_u64()))
    })
}

/// WebAssembly `i64.store32`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[i32, value] -> []`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Uses little-endian linear-memory access and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_store32(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    store_internal_local(tail_code, ctx, "op_i64_store32", |ctx| {
        StoreBytes::Write4(truncate_u64_to_u32_bytes(ctx.stack.pop_u64()))
    })
}

/// WebAssembly `memory.size`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[] -> [i32]`.
/// Traps: traps when no default memory exists.
/// Notes: Uses little-endian linear-memory access and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_mem_size(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let page_size = unsafe { ctx.default_local_memory_unchecked() }.page_size();
    vm_try!(ctx.stack.push_u32(page_size));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `memory.grow`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[i32] -> [i32]`.
/// Traps: traps when no default memory exists; otherwise returns `-1` on growth failure.
/// Notes: Uses little-endian linear-memory access and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_mem_grow(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let page_size_delta = ctx.stack.pop_u32();
    let result = vm_try!(ctx
        .gc
        .local_grow_memory(ctx.default_local_memory_id_unchecked(), page_size_delta,));
    vm_try!(ctx.stack.push_i32(result));
    call_next(tail_code, 0, ctx)
}

macro_rules! define_shared_push_load {
    ($name:ident, $mnemonic:literal, $bytes:expr) => {
        #[doc = concat!("WebAssembly `", $mnemonic, "` on shared default memory.")]
        ///
        /// Spec:
        /// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
        /// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
        /// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
        ///
        /// Stack effect: `[i32] -> [value]`.
        /// Traps: traps on out-of-bounds memory access.
        /// Notes: Uses the shared-memory specialized fast path selected by the parser and tail-dispatches with `call_next`.
        ///
        /// # Safety
        /// - `tail_code` must point to the decoded instruction for this handler in the active function body.
        /// - `ctx` must reference a live execution context whose default memory is shared.
        /// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
        pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            profile_memory_family(stringify!($name));
            let start = vm_try!(load_start(tail_code, ctx));
            vm_try!(ctx.gc.shared_push_memory_to_stack::<$bytes>(
                ctx.default_shared_memory_id_unchecked(),
                ctx.stack,
                start,
            ));
            call_next(tail_code, 1, ctx)
        }
    };
}

macro_rules! define_shared_scalar_load {
    ($name:ident, $mnemonic:literal, $reader:ident, $push:ident, $convert:path) => {
        #[doc = concat!("WebAssembly `", $mnemonic, "` on shared default memory.")]
        ///
        /// Spec:
        /// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
        /// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
        /// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
        ///
        /// Stack effect: `[i32] -> [value]`.
        /// Traps: traps on out-of-bounds memory access.
        /// Notes: Uses the shared-memory specialized fast path selected by the parser and tail-dispatches with `call_next`.
        ///
        /// # Safety
        /// - `tail_code` must point to the decoded instruction for this handler in the active function body.
        /// - `ctx` must reference a live execution context whose default memory is shared.
        /// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
        pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            profile_memory_family(stringify!($name));
            let start = vm_try!(load_start(tail_code, ctx));
            let value = vm_try!(ctx
                .gc
                .$reader(ctx.default_shared_memory_id_unchecked(), start));
            vm_try!(ctx.stack.$push($convert(value)));
            call_next(tail_code, 1, ctx)
        }
    };
}

macro_rules! define_shared_store_alias {
    ($name:ident, $mnemonic:literal, $make_operation:expr) => {
        #[doc = concat!("WebAssembly `", $mnemonic, "` on shared default memory.")]
        ///
        /// Spec:
        /// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
        /// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
        /// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
        ///
        /// Stack effect: `[i32, value] -> []`.
        /// Traps: traps on out-of-bounds memory access.
        /// Notes: Uses the shared-memory specialized fast path selected by the parser and tail-dispatches with `call_next`.
        ///
        /// # Safety
        /// - `tail_code` must point to the decoded instruction for this handler in the active function body.
        /// - `ctx` must reference a live execution context whose default memory is shared.
        /// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
        pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            store_internal_shared(tail_code, ctx, stringify!($name), $make_operation)
        }
    };
}

define_shared_push_load!(op_i32_load_shared, "i32.load", 4);
define_shared_push_load!(op_i64_load_shared, "i64.load", 8);
define_shared_push_load!(op_f32_load_shared, "f32.load", 4);
define_shared_push_load!(op_f64_load_shared, "f64.load", 8);
define_shared_scalar_load!(
    op_i32_load8_u_shared,
    "i32.load8_u",
    shared_read_u8_at,
    push_u32_fast,
    u32::from
);
define_shared_scalar_load!(
    op_i32_load8_s_shared,
    "i32.load8_s",
    shared_read_i8_at,
    push_i32_fast,
    i32::from
);
define_shared_scalar_load!(
    op_i32_load16_s_shared,
    "i32.load16_s",
    shared_read_i16_at,
    push_i32_fast,
    i32::from
);
define_shared_scalar_load!(
    op_i32_load16_u_shared,
    "i32.load16_u",
    shared_read_u16_at,
    push_u32_fast,
    u32::from
);
define_shared_scalar_load!(
    op_i64_load8_s_shared,
    "i64.load8_s",
    shared_read_i8_at,
    push_i64_fast,
    i64::from
);
define_shared_scalar_load!(
    op_i64_load8_u_shared,
    "i64.load8_u",
    shared_read_u8_at,
    push_u64_fast,
    u64::from
);
define_shared_scalar_load!(
    op_i64_load16_s_shared,
    "i64.load16_s",
    shared_read_i16_at,
    push_i64_fast,
    i64::from
);
define_shared_scalar_load!(
    op_i64_load16_u_shared,
    "i64.load16_u",
    shared_read_u16_at,
    push_u64_fast,
    u64::from
);
define_shared_scalar_load!(
    op_i64_load32_s_shared,
    "i64.load32_s",
    shared_read_i32_at,
    push_i64_fast,
    i64::from
);
define_shared_scalar_load!(
    op_i64_load32_u_shared,
    "i64.load32_u",
    shared_read_u32_at,
    push_u64_fast,
    u64::from
);
define_shared_store_alias!(op_i32_store_shared, "i32.store", |ctx| {
    StoreBytes::Write4(ctx.stack.pop_u8_array::<4>())
});
define_shared_store_alias!(op_i64_store_shared, "i64.store", |ctx| {
    StoreBytes::Write8(ctx.stack.pop_u8_array::<8>())
});
define_shared_store_alias!(op_f32_store_shared, "f32.store", |ctx| {
    StoreBytes::Write4(ctx.stack.pop_u8_array::<4>())
});
define_shared_store_alias!(op_f64_store_shared, "f64.store", |ctx| {
    StoreBytes::Write8(ctx.stack.pop_u8_array::<8>())
});
define_shared_store_alias!(op_i32_store8_shared, "i32.store8", |ctx| {
    StoreBytes::Write1(truncate_u32_to_u8_bytes(ctx.stack.pop_u32()))
});
define_shared_store_alias!(op_i32_store16_shared, "i32.store16", |ctx| {
    StoreBytes::Write2(truncate_u32_to_u16_bytes(ctx.stack.pop_u32()))
});
define_shared_store_alias!(op_i64_store8_shared, "i64.store8", |ctx| {
    StoreBytes::Write1(truncate_u64_to_u8_bytes(ctx.stack.pop_u64()))
});
define_shared_store_alias!(op_i64_store16_shared, "i64.store16", |ctx| {
    StoreBytes::Write2(truncate_u64_to_u16_bytes(ctx.stack.pop_u64()))
});
define_shared_store_alias!(op_i64_store32_shared, "i64.store32", |ctx| {
    StoreBytes::Write4(truncate_u64_to_u32_bytes(ctx.stack.pop_u64()))
});

define_indexed_push_load!(
    op_i32_load_indexed_local,
    op_i32_load_indexed_shared,
    "i32.load",
    4
);
define_indexed_push_load!(
    op_i64_load_indexed_local,
    op_i64_load_indexed_shared,
    "i64.load",
    8
);
define_indexed_push_load!(
    op_f32_load_indexed_local,
    op_f32_load_indexed_shared,
    "f32.load",
    4
);
define_indexed_push_load!(
    op_f64_load_indexed_local,
    op_f64_load_indexed_shared,
    "f64.load",
    8
);
define_indexed_scalar_load!(
    op_i32_load8_u_indexed_local,
    op_i32_load8_u_indexed_shared,
    "i32.load8_u",
    local_read_u8_at,
    shared_read_u8_at,
    push_u32_fast,
    u32::from
);
define_indexed_scalar_load!(
    op_i32_load8_s_indexed_local,
    op_i32_load8_s_indexed_shared,
    "i32.load8_s",
    local_read_i8_at,
    shared_read_i8_at,
    push_i32_fast,
    i32::from
);
define_indexed_scalar_load!(
    op_i32_load16_s_indexed_local,
    op_i32_load16_s_indexed_shared,
    "i32.load16_s",
    local_read_i16_at,
    shared_read_i16_at,
    push_i32_fast,
    i32::from
);
define_indexed_scalar_load!(
    op_i32_load16_u_indexed_local,
    op_i32_load16_u_indexed_shared,
    "i32.load16_u",
    local_read_u16_at,
    shared_read_u16_at,
    push_u32_fast,
    u32::from
);
define_indexed_scalar_load!(
    op_i64_load8_s_indexed_local,
    op_i64_load8_s_indexed_shared,
    "i64.load8_s",
    local_read_i8_at,
    shared_read_i8_at,
    push_i64_fast,
    i64::from
);
define_indexed_scalar_load!(
    op_i64_load8_u_indexed_local,
    op_i64_load8_u_indexed_shared,
    "i64.load8_u",
    local_read_u8_at,
    shared_read_u8_at,
    push_u64_fast,
    u64::from
);
define_indexed_scalar_load!(
    op_i64_load16_s_indexed_local,
    op_i64_load16_s_indexed_shared,
    "i64.load16_s",
    local_read_i16_at,
    shared_read_i16_at,
    push_i64_fast,
    i64::from
);
define_indexed_scalar_load!(
    op_i64_load16_u_indexed_local,
    op_i64_load16_u_indexed_shared,
    "i64.load16_u",
    local_read_u16_at,
    shared_read_u16_at,
    push_u64_fast,
    u64::from
);
define_indexed_scalar_load!(
    op_i64_load32_s_indexed_local,
    op_i64_load32_s_indexed_shared,
    "i64.load32_s",
    local_read_i32_at,
    shared_read_i32_at,
    push_i64_fast,
    i64::from
);
define_indexed_scalar_load!(
    op_i64_load32_u_indexed_local,
    op_i64_load32_u_indexed_shared,
    "i64.load32_u",
    local_read_u32_at,
    shared_read_u32_at,
    push_u64_fast,
    u64::from
);
define_indexed_store_alias!(
    op_i32_store_indexed_local,
    op_i32_store_indexed_shared,
    "i32.store",
    |ctx| { StoreBytes::Write4(ctx.stack.pop_u8_array::<4>()) }
);
define_indexed_store_alias!(
    op_i64_store_indexed_local,
    op_i64_store_indexed_shared,
    "i64.store",
    |ctx| { StoreBytes::Write8(ctx.stack.pop_u8_array::<8>()) }
);
define_indexed_store_alias!(
    op_f32_store_indexed_local,
    op_f32_store_indexed_shared,
    "f32.store",
    |ctx| { StoreBytes::Write4(ctx.stack.pop_u8_array::<4>()) }
);
define_indexed_store_alias!(
    op_f64_store_indexed_local,
    op_f64_store_indexed_shared,
    "f64.store",
    |ctx| { StoreBytes::Write8(ctx.stack.pop_u8_array::<8>()) }
);
define_indexed_store_alias!(
    op_i32_store8_indexed_local,
    op_i32_store8_indexed_shared,
    "i32.store8",
    |ctx| { StoreBytes::Write1(truncate_u32_to_u8_bytes(ctx.stack.pop_u32())) }
);
define_indexed_store_alias!(
    op_i32_store16_indexed_local,
    op_i32_store16_indexed_shared,
    "i32.store16",
    |ctx| { StoreBytes::Write2(truncate_u32_to_u16_bytes(ctx.stack.pop_u32())) }
);
define_indexed_store_alias!(
    op_i64_store8_indexed_local,
    op_i64_store8_indexed_shared,
    "i64.store8",
    |ctx| { StoreBytes::Write1(truncate_u64_to_u8_bytes(ctx.stack.pop_u64())) }
);
define_indexed_store_alias!(
    op_i64_store16_indexed_local,
    op_i64_store16_indexed_shared,
    "i64.store16",
    |ctx| { StoreBytes::Write2(truncate_u64_to_u16_bytes(ctx.stack.pop_u64())) }
);
define_indexed_store_alias!(
    op_i64_store32_indexed_local,
    op_i64_store32_indexed_shared,
    "i64.store32",
    |ctx| { StoreBytes::Write4(truncate_u64_to_u32_bytes(ctx.stack.pop_u64())) }
);

define_local_base_push_load!(op_i32_load_local_base, "i32.load", 4);
define_local_base_push_load!(op_i64_load_local_base, "i64.load", 8);
define_local_base_push_load!(op_f32_load_local_base, "f32.load", 4);
define_local_base_push_load!(op_f64_load_local_base, "f64.load", 8);
define_local_base_scalar_load!(
    op_i32_load8_u_local_base,
    "i32.load8_u",
    read_u8_at,
    push_u32_fast,
    u32::from
);
define_local_base_scalar_load!(
    op_i32_load8_s_local_base,
    "i32.load8_s",
    read_i8_at,
    push_i32_fast,
    i32::from
);
define_local_base_scalar_load!(
    op_i32_load16_s_local_base,
    "i32.load16_s",
    read_i16_at,
    push_i32_fast,
    i32::from
);
define_local_base_scalar_load!(
    op_i32_load16_u_local_base,
    "i32.load16_u",
    read_u16_at,
    push_u32_fast,
    u32::from
);
define_local_base_scalar_load!(
    op_i64_load8_s_local_base,
    "i64.load8_s",
    read_i8_at,
    push_i64_fast,
    i64::from
);
define_local_base_scalar_load!(
    op_i64_load8_u_local_base,
    "i64.load8_u",
    read_u8_at,
    push_u64_fast,
    u64::from
);
define_local_base_scalar_load!(
    op_i64_load16_s_local_base,
    "i64.load16_s",
    read_i16_at,
    push_i64_fast,
    i64::from
);
define_local_base_scalar_load!(
    op_i64_load16_u_local_base,
    "i64.load16_u",
    read_u16_at,
    push_u64_fast,
    u64::from
);
define_local_base_scalar_load!(
    op_i64_load32_s_local_base,
    "i64.load32_s",
    read_i32_at,
    push_i64_fast,
    i64::from
);
define_local_base_scalar_load!(
    op_i64_load32_u_local_base,
    "i64.load32_u",
    read_u32_at,
    push_u64_fast,
    u64::from
);
define_local_base_store_alias!(op_i32_store_local_base, |ctx| {
    StoreBytes::Write4(ctx.stack.pop_u8_array::<4>())
});
define_local_base_store_alias!(op_i64_store_local_base, |ctx| {
    StoreBytes::Write8(ctx.stack.pop_u8_array::<8>())
});
define_local_base_store_alias!(op_f32_store_local_base, |ctx| {
    StoreBytes::Write4(ctx.stack.pop_u8_array::<4>())
});
define_local_base_store_alias!(op_f64_store_local_base, |ctx| {
    StoreBytes::Write8(ctx.stack.pop_u8_array::<8>())
});
define_local_base_store_alias!(op_i32_store8_local_base, |ctx| {
    StoreBytes::Write1(truncate_u32_to_u8_bytes(ctx.stack.pop_u32()))
});
define_local_base_store_alias!(op_i32_store16_local_base, |ctx| {
    StoreBytes::Write2(truncate_u32_to_u16_bytes(ctx.stack.pop_u32()))
});
define_local_base_store_alias!(op_i64_store8_local_base, |ctx| {
    StoreBytes::Write1(truncate_u64_to_u8_bytes(ctx.stack.pop_u64()))
});
define_local_base_store_alias!(op_i64_store16_local_base, |ctx| {
    StoreBytes::Write2(truncate_u64_to_u16_bytes(ctx.stack.pop_u64()))
});
define_local_base_store_alias!(op_i64_store32_local_base, |ctx| {
    StoreBytes::Write4(truncate_u64_to_u32_bytes(ctx.stack.pop_u64()))
});

define_indexed_local_base_push_load!(op_i32_load_indexed_local_base, "i32.load", 4);
define_indexed_local_base_push_load!(op_i64_load_indexed_local_base, "i64.load", 8);
define_indexed_local_base_push_load!(op_f32_load_indexed_local_base, "f32.load", 4);
define_indexed_local_base_push_load!(op_f64_load_indexed_local_base, "f64.load", 8);
define_indexed_local_base_scalar_load!(
    op_i32_load8_u_indexed_local_base,
    "i32.load8_u",
    local_read_u8_at,
    push_u32_fast,
    u32::from
);
define_indexed_local_base_scalar_load!(
    op_i32_load8_s_indexed_local_base,
    "i32.load8_s",
    local_read_i8_at,
    push_i32_fast,
    i32::from
);
define_indexed_local_base_scalar_load!(
    op_i32_load16_s_indexed_local_base,
    "i32.load16_s",
    local_read_i16_at,
    push_i32_fast,
    i32::from
);
define_indexed_local_base_scalar_load!(
    op_i32_load16_u_indexed_local_base,
    "i32.load16_u",
    local_read_u16_at,
    push_u32_fast,
    u32::from
);
define_indexed_local_base_scalar_load!(
    op_i64_load8_s_indexed_local_base,
    "i64.load8_s",
    local_read_i8_at,
    push_i64_fast,
    i64::from
);
define_indexed_local_base_scalar_load!(
    op_i64_load8_u_indexed_local_base,
    "i64.load8_u",
    local_read_u8_at,
    push_u64_fast,
    u64::from
);
define_indexed_local_base_scalar_load!(
    op_i64_load16_s_indexed_local_base,
    "i64.load16_s",
    local_read_i16_at,
    push_i64_fast,
    i64::from
);
define_indexed_local_base_scalar_load!(
    op_i64_load16_u_indexed_local_base,
    "i64.load16_u",
    local_read_u16_at,
    push_u64_fast,
    u64::from
);
define_indexed_local_base_scalar_load!(
    op_i64_load32_s_indexed_local_base,
    "i64.load32_s",
    local_read_i32_at,
    push_i64_fast,
    i64::from
);
define_indexed_local_base_scalar_load!(
    op_i64_load32_u_indexed_local_base,
    "i64.load32_u",
    local_read_u32_at,
    push_u64_fast,
    u64::from
);
define_indexed_local_base_store_alias!(op_i32_store_indexed_local_base, |ctx| {
    StoreBytes::Write4(ctx.stack.pop_u8_array::<4>())
});
define_indexed_local_base_store_alias!(op_i64_store_indexed_local_base, |ctx| {
    StoreBytes::Write8(ctx.stack.pop_u8_array::<8>())
});
define_indexed_local_base_store_alias!(op_f32_store_indexed_local_base, |ctx| {
    StoreBytes::Write4(ctx.stack.pop_u8_array::<4>())
});
define_indexed_local_base_store_alias!(op_f64_store_indexed_local_base, |ctx| {
    StoreBytes::Write8(ctx.stack.pop_u8_array::<8>())
});
define_indexed_local_base_store_alias!(op_i32_store8_indexed_local_base, |ctx| {
    StoreBytes::Write1(truncate_u32_to_u8_bytes(ctx.stack.pop_u32()))
});
define_indexed_local_base_store_alias!(op_i32_store16_indexed_local_base, |ctx| {
    StoreBytes::Write2(truncate_u32_to_u16_bytes(ctx.stack.pop_u32()))
});
define_indexed_local_base_store_alias!(op_i64_store8_indexed_local_base, |ctx| {
    StoreBytes::Write1(truncate_u64_to_u8_bytes(ctx.stack.pop_u64()))
});
define_indexed_local_base_store_alias!(op_i64_store16_indexed_local_base, |ctx| {
    StoreBytes::Write2(truncate_u64_to_u16_bytes(ctx.stack.pop_u64()))
});
define_indexed_local_base_store_alias!(op_i64_store32_indexed_local_base, |ctx| {
    StoreBytes::Write4(truncate_u64_to_u32_bytes(ctx.stack.pop_u64()))
});

define_shared_local_base_push_load!(op_i32_load_shared_local_base, "i32.load", 4);
define_shared_local_base_push_load!(op_i64_load_shared_local_base, "i64.load", 8);
define_shared_local_base_push_load!(op_f32_load_shared_local_base, "f32.load", 4);
define_shared_local_base_push_load!(op_f64_load_shared_local_base, "f64.load", 8);
define_shared_local_base_scalar_load!(
    op_i32_load8_u_shared_local_base,
    "i32.load8_u",
    shared_read_u8_at,
    push_u32_fast,
    u32::from
);
define_shared_local_base_scalar_load!(
    op_i32_load8_s_shared_local_base,
    "i32.load8_s",
    shared_read_i8_at,
    push_i32_fast,
    i32::from
);
define_shared_local_base_scalar_load!(
    op_i32_load16_s_shared_local_base,
    "i32.load16_s",
    shared_read_i16_at,
    push_i32_fast,
    i32::from
);
define_shared_local_base_scalar_load!(
    op_i32_load16_u_shared_local_base,
    "i32.load16_u",
    shared_read_u16_at,
    push_u32_fast,
    u32::from
);
define_shared_local_base_scalar_load!(
    op_i64_load8_s_shared_local_base,
    "i64.load8_s",
    shared_read_i8_at,
    push_i64_fast,
    i64::from
);
define_shared_local_base_scalar_load!(
    op_i64_load8_u_shared_local_base,
    "i64.load8_u",
    shared_read_u8_at,
    push_u64_fast,
    u64::from
);
define_shared_local_base_scalar_load!(
    op_i64_load16_s_shared_local_base,
    "i64.load16_s",
    shared_read_i16_at,
    push_i64_fast,
    i64::from
);
define_shared_local_base_scalar_load!(
    op_i64_load16_u_shared_local_base,
    "i64.load16_u",
    shared_read_u16_at,
    push_u64_fast,
    u64::from
);
define_shared_local_base_scalar_load!(
    op_i64_load32_s_shared_local_base,
    "i64.load32_s",
    shared_read_i32_at,
    push_i64_fast,
    i64::from
);
define_shared_local_base_scalar_load!(
    op_i64_load32_u_shared_local_base,
    "i64.load32_u",
    shared_read_u32_at,
    push_u64_fast,
    u64::from
);
define_shared_local_base_store_alias!(op_i32_store_shared_local_base, |ctx| {
    StoreBytes::Write4(ctx.stack.pop_u8_array::<4>())
});
define_shared_local_base_store_alias!(op_i64_store_shared_local_base, |ctx| {
    StoreBytes::Write8(ctx.stack.pop_u8_array::<8>())
});
define_shared_local_base_store_alias!(op_f32_store_shared_local_base, |ctx| {
    StoreBytes::Write4(ctx.stack.pop_u8_array::<4>())
});
define_shared_local_base_store_alias!(op_f64_store_shared_local_base, |ctx| {
    StoreBytes::Write8(ctx.stack.pop_u8_array::<8>())
});
define_shared_local_base_store_alias!(op_i32_store8_shared_local_base, |ctx| {
    StoreBytes::Write1(truncate_u32_to_u8_bytes(ctx.stack.pop_u32()))
});
define_shared_local_base_store_alias!(op_i32_store16_shared_local_base, |ctx| {
    StoreBytes::Write2(truncate_u32_to_u16_bytes(ctx.stack.pop_u32()))
});
define_shared_local_base_store_alias!(op_i64_store8_shared_local_base, |ctx| {
    StoreBytes::Write1(truncate_u64_to_u8_bytes(ctx.stack.pop_u64()))
});
define_shared_local_base_store_alias!(op_i64_store16_shared_local_base, |ctx| {
    StoreBytes::Write2(truncate_u64_to_u16_bytes(ctx.stack.pop_u64()))
});
define_shared_local_base_store_alias!(op_i64_store32_shared_local_base, |ctx| {
    StoreBytes::Write4(truncate_u64_to_u32_bytes(ctx.stack.pop_u64()))
});

define_indexed_shared_local_base_push_load!(op_i32_load_indexed_shared_local_base, "i32.load", 4);
define_indexed_shared_local_base_push_load!(op_i64_load_indexed_shared_local_base, "i64.load", 8);
define_indexed_shared_local_base_push_load!(op_f32_load_indexed_shared_local_base, "f32.load", 4);
define_indexed_shared_local_base_push_load!(op_f64_load_indexed_shared_local_base, "f64.load", 8);
define_indexed_shared_local_base_scalar_load!(
    op_i32_load8_u_indexed_shared_local_base,
    "i32.load8_u",
    shared_read_u8_at,
    push_u32_fast,
    u32::from
);
define_indexed_shared_local_base_scalar_load!(
    op_i32_load8_s_indexed_shared_local_base,
    "i32.load8_s",
    shared_read_i8_at,
    push_i32_fast,
    i32::from
);
define_indexed_shared_local_base_scalar_load!(
    op_i32_load16_s_indexed_shared_local_base,
    "i32.load16_s",
    shared_read_i16_at,
    push_i32_fast,
    i32::from
);
define_indexed_shared_local_base_scalar_load!(
    op_i32_load16_u_indexed_shared_local_base,
    "i32.load16_u",
    shared_read_u16_at,
    push_u32_fast,
    u32::from
);
define_indexed_shared_local_base_scalar_load!(
    op_i64_load8_s_indexed_shared_local_base,
    "i64.load8_s",
    shared_read_i8_at,
    push_i64_fast,
    i64::from
);
define_indexed_shared_local_base_scalar_load!(
    op_i64_load8_u_indexed_shared_local_base,
    "i64.load8_u",
    shared_read_u8_at,
    push_u64_fast,
    u64::from
);
define_indexed_shared_local_base_scalar_load!(
    op_i64_load16_s_indexed_shared_local_base,
    "i64.load16_s",
    shared_read_i16_at,
    push_i64_fast,
    i64::from
);
define_indexed_shared_local_base_scalar_load!(
    op_i64_load16_u_indexed_shared_local_base,
    "i64.load16_u",
    shared_read_u16_at,
    push_u64_fast,
    u64::from
);
define_indexed_shared_local_base_scalar_load!(
    op_i64_load32_s_indexed_shared_local_base,
    "i64.load32_s",
    shared_read_i32_at,
    push_i64_fast,
    i64::from
);
define_indexed_shared_local_base_scalar_load!(
    op_i64_load32_u_indexed_shared_local_base,
    "i64.load32_u",
    shared_read_u32_at,
    push_u64_fast,
    u64::from
);
define_indexed_shared_local_base_store_alias!(op_i32_store_indexed_shared_local_base, |ctx| {
    StoreBytes::Write4(ctx.stack.pop_u8_array::<4>())
});
define_indexed_shared_local_base_store_alias!(op_i64_store_indexed_shared_local_base, |ctx| {
    StoreBytes::Write8(ctx.stack.pop_u8_array::<8>())
});
define_indexed_shared_local_base_store_alias!(op_f32_store_indexed_shared_local_base, |ctx| {
    StoreBytes::Write4(ctx.stack.pop_u8_array::<4>())
});
define_indexed_shared_local_base_store_alias!(op_f64_store_indexed_shared_local_base, |ctx| {
    StoreBytes::Write8(ctx.stack.pop_u8_array::<8>())
});
define_indexed_shared_local_base_store_alias!(op_i32_store8_indexed_shared_local_base, |ctx| {
    StoreBytes::Write1(truncate_u32_to_u8_bytes(ctx.stack.pop_u32()))
});
define_indexed_shared_local_base_store_alias!(op_i32_store16_indexed_shared_local_base, |ctx| {
    StoreBytes::Write2(truncate_u32_to_u16_bytes(ctx.stack.pop_u32()))
});
define_indexed_shared_local_base_store_alias!(op_i64_store8_indexed_shared_local_base, |ctx| {
    StoreBytes::Write1(truncate_u64_to_u8_bytes(ctx.stack.pop_u64()))
});
define_indexed_shared_local_base_store_alias!(op_i64_store16_indexed_shared_local_base, |ctx| {
    StoreBytes::Write2(truncate_u64_to_u16_bytes(ctx.stack.pop_u64()))
});
define_indexed_shared_local_base_store_alias!(op_i64_store32_indexed_shared_local_base, |ctx| {
    StoreBytes::Write4(truncate_u64_to_u32_bytes(ctx.stack.pop_u64()))
});

define_local_scaled_index_push_load!(op_i32_load_local_scaled_index, "i32.load", 4);
define_local_scaled_index_push_load!(op_i64_load_local_scaled_index, "i64.load", 8);
define_local_scaled_index_push_load!(op_f32_load_local_scaled_index, "f32.load", 4);
define_local_scaled_index_push_load!(op_f64_load_local_scaled_index, "f64.load", 8);
define_local_scaled_index_scalar_load!(
    op_i32_load8_u_local_scaled_index,
    "i32.load8_u",
    read_u8_at,
    push_u32_fast,
    u32::from
);
define_local_scaled_index_scalar_load!(
    op_i32_load8_s_local_scaled_index,
    "i32.load8_s",
    read_i8_at,
    push_i32_fast,
    i32::from
);
define_local_scaled_index_scalar_load!(
    op_i32_load16_s_local_scaled_index,
    "i32.load16_s",
    read_i16_at,
    push_i32_fast,
    i32::from
);
define_local_scaled_index_scalar_load!(
    op_i32_load16_u_local_scaled_index,
    "i32.load16_u",
    read_u16_at,
    push_u32_fast,
    u32::from
);
define_local_scaled_index_scalar_load!(
    op_i64_load8_s_local_scaled_index,
    "i64.load8_s",
    read_i8_at,
    push_i64_fast,
    i64::from
);
define_local_scaled_index_scalar_load!(
    op_i64_load8_u_local_scaled_index,
    "i64.load8_u",
    read_u8_at,
    push_u64_fast,
    u64::from
);
define_local_scaled_index_scalar_load!(
    op_i64_load16_s_local_scaled_index,
    "i64.load16_s",
    read_i16_at,
    push_i64_fast,
    i64::from
);
define_local_scaled_index_scalar_load!(
    op_i64_load16_u_local_scaled_index,
    "i64.load16_u",
    read_u16_at,
    push_u64_fast,
    u64::from
);
define_local_scaled_index_scalar_load!(
    op_i64_load32_s_local_scaled_index,
    "i64.load32_s",
    read_i32_at,
    push_i64_fast,
    i64::from
);
define_local_scaled_index_scalar_load!(
    op_i64_load32_u_local_scaled_index,
    "i64.load32_u",
    read_u32_at,
    push_u64_fast,
    u64::from
);
define_local_scaled_index_store_alias!(op_i32_store_local_scaled_index, |ctx| {
    StoreBytes::Write4(ctx.stack.pop_u8_array::<4>())
});
define_local_scaled_index_store_alias!(op_i64_store_local_scaled_index, |ctx| {
    StoreBytes::Write8(ctx.stack.pop_u8_array::<8>())
});
define_local_scaled_index_store_alias!(op_f32_store_local_scaled_index, |ctx| {
    StoreBytes::Write4(ctx.stack.pop_u8_array::<4>())
});
define_local_scaled_index_store_alias!(op_f64_store_local_scaled_index, |ctx| {
    StoreBytes::Write8(ctx.stack.pop_u8_array::<8>())
});
define_local_scaled_index_store_alias!(op_i32_store8_local_scaled_index, |ctx| {
    StoreBytes::Write1(truncate_u32_to_u8_bytes(ctx.stack.pop_u32()))
});
define_local_scaled_index_store_alias!(op_i32_store16_local_scaled_index, |ctx| {
    StoreBytes::Write2(truncate_u32_to_u16_bytes(ctx.stack.pop_u32()))
});
define_local_scaled_index_store_alias!(op_i64_store8_local_scaled_index, |ctx| {
    StoreBytes::Write1(truncate_u64_to_u8_bytes(ctx.stack.pop_u64()))
});
define_local_scaled_index_store_alias!(op_i64_store16_local_scaled_index, |ctx| {
    StoreBytes::Write2(truncate_u64_to_u16_bytes(ctx.stack.pop_u64()))
});
define_local_scaled_index_store_alias!(op_i64_store32_local_scaled_index, |ctx| {
    StoreBytes::Write4(truncate_u64_to_u32_bytes(ctx.stack.pop_u64()))
});

define_indexed_local_scaled_index_push_load!(op_i32_load_indexed_local_scaled_index, "i32.load", 4);
define_indexed_local_scaled_index_push_load!(op_i64_load_indexed_local_scaled_index, "i64.load", 8);
define_indexed_local_scaled_index_push_load!(op_f32_load_indexed_local_scaled_index, "f32.load", 4);
define_indexed_local_scaled_index_push_load!(op_f64_load_indexed_local_scaled_index, "f64.load", 8);
define_indexed_local_scaled_index_scalar_load!(
    op_i32_load8_u_indexed_local_scaled_index,
    "i32.load8_u",
    local_read_u8_at,
    push_u32_fast,
    u32::from
);
define_indexed_local_scaled_index_scalar_load!(
    op_i32_load8_s_indexed_local_scaled_index,
    "i32.load8_s",
    local_read_i8_at,
    push_i32_fast,
    i32::from
);
define_indexed_local_scaled_index_scalar_load!(
    op_i32_load16_s_indexed_local_scaled_index,
    "i32.load16_s",
    local_read_i16_at,
    push_i32_fast,
    i32::from
);
define_indexed_local_scaled_index_scalar_load!(
    op_i32_load16_u_indexed_local_scaled_index,
    "i32.load16_u",
    local_read_u16_at,
    push_u32_fast,
    u32::from
);
define_indexed_local_scaled_index_scalar_load!(
    op_i64_load8_s_indexed_local_scaled_index,
    "i64.load8_s",
    local_read_i8_at,
    push_i64_fast,
    i64::from
);
define_indexed_local_scaled_index_scalar_load!(
    op_i64_load8_u_indexed_local_scaled_index,
    "i64.load8_u",
    local_read_u8_at,
    push_u64_fast,
    u64::from
);
define_indexed_local_scaled_index_scalar_load!(
    op_i64_load16_s_indexed_local_scaled_index,
    "i64.load16_s",
    local_read_i16_at,
    push_i64_fast,
    i64::from
);
define_indexed_local_scaled_index_scalar_load!(
    op_i64_load16_u_indexed_local_scaled_index,
    "i64.load16_u",
    local_read_u16_at,
    push_u64_fast,
    u64::from
);
define_indexed_local_scaled_index_scalar_load!(
    op_i64_load32_s_indexed_local_scaled_index,
    "i64.load32_s",
    local_read_i32_at,
    push_i64_fast,
    i64::from
);
define_indexed_local_scaled_index_scalar_load!(
    op_i64_load32_u_indexed_local_scaled_index,
    "i64.load32_u",
    local_read_u32_at,
    push_u64_fast,
    u64::from
);
define_indexed_local_scaled_index_store_alias!(op_i32_store_indexed_local_scaled_index, |ctx| {
    StoreBytes::Write4(ctx.stack.pop_u8_array::<4>())
});
define_indexed_local_scaled_index_store_alias!(op_i64_store_indexed_local_scaled_index, |ctx| {
    StoreBytes::Write8(ctx.stack.pop_u8_array::<8>())
});
define_indexed_local_scaled_index_store_alias!(op_f32_store_indexed_local_scaled_index, |ctx| {
    StoreBytes::Write4(ctx.stack.pop_u8_array::<4>())
});
define_indexed_local_scaled_index_store_alias!(op_f64_store_indexed_local_scaled_index, |ctx| {
    StoreBytes::Write8(ctx.stack.pop_u8_array::<8>())
});
define_indexed_local_scaled_index_store_alias!(op_i32_store8_indexed_local_scaled_index, |ctx| {
    StoreBytes::Write1(truncate_u32_to_u8_bytes(ctx.stack.pop_u32()))
});
define_indexed_local_scaled_index_store_alias!(op_i32_store16_indexed_local_scaled_index, |ctx| {
    StoreBytes::Write2(truncate_u32_to_u16_bytes(ctx.stack.pop_u32()))
});
define_indexed_local_scaled_index_store_alias!(op_i64_store8_indexed_local_scaled_index, |ctx| {
    StoreBytes::Write1(truncate_u64_to_u8_bytes(ctx.stack.pop_u64()))
});
define_indexed_local_scaled_index_store_alias!(op_i64_store16_indexed_local_scaled_index, |ctx| {
    StoreBytes::Write2(truncate_u64_to_u16_bytes(ctx.stack.pop_u64()))
});
define_indexed_local_scaled_index_store_alias!(op_i64_store32_indexed_local_scaled_index, |ctx| {
    StoreBytes::Write4(truncate_u64_to_u32_bytes(ctx.stack.pop_u64()))
});

define_shared_local_scaled_index_push_load!(op_i32_load_shared_local_scaled_index, "i32.load", 4);
define_shared_local_scaled_index_push_load!(op_i64_load_shared_local_scaled_index, "i64.load", 8);
define_shared_local_scaled_index_push_load!(op_f32_load_shared_local_scaled_index, "f32.load", 4);
define_shared_local_scaled_index_push_load!(op_f64_load_shared_local_scaled_index, "f64.load", 8);
define_shared_local_scaled_index_scalar_load!(
    op_i32_load8_u_shared_local_scaled_index,
    "i32.load8_u",
    shared_read_u8_at,
    push_u32_fast,
    u32::from
);
define_shared_local_scaled_index_scalar_load!(
    op_i32_load8_s_shared_local_scaled_index,
    "i32.load8_s",
    shared_read_i8_at,
    push_i32_fast,
    i32::from
);
define_shared_local_scaled_index_scalar_load!(
    op_i32_load16_s_shared_local_scaled_index,
    "i32.load16_s",
    shared_read_i16_at,
    push_i32_fast,
    i32::from
);
define_shared_local_scaled_index_scalar_load!(
    op_i32_load16_u_shared_local_scaled_index,
    "i32.load16_u",
    shared_read_u16_at,
    push_u32_fast,
    u32::from
);
define_shared_local_scaled_index_scalar_load!(
    op_i64_load8_s_shared_local_scaled_index,
    "i64.load8_s",
    shared_read_i8_at,
    push_i64_fast,
    i64::from
);
define_shared_local_scaled_index_scalar_load!(
    op_i64_load8_u_shared_local_scaled_index,
    "i64.load8_u",
    shared_read_u8_at,
    push_u64_fast,
    u64::from
);
define_shared_local_scaled_index_scalar_load!(
    op_i64_load16_s_shared_local_scaled_index,
    "i64.load16_s",
    shared_read_i16_at,
    push_i64_fast,
    i64::from
);
define_shared_local_scaled_index_scalar_load!(
    op_i64_load16_u_shared_local_scaled_index,
    "i64.load16_u",
    shared_read_u16_at,
    push_u64_fast,
    u64::from
);
define_shared_local_scaled_index_scalar_load!(
    op_i64_load32_s_shared_local_scaled_index,
    "i64.load32_s",
    shared_read_i32_at,
    push_i64_fast,
    i64::from
);
define_shared_local_scaled_index_scalar_load!(
    op_i64_load32_u_shared_local_scaled_index,
    "i64.load32_u",
    shared_read_u32_at,
    push_u64_fast,
    u64::from
);
define_shared_local_scaled_index_store_alias!(op_i32_store_shared_local_scaled_index, |ctx| {
    StoreBytes::Write4(ctx.stack.pop_u8_array::<4>())
});
define_shared_local_scaled_index_store_alias!(op_i64_store_shared_local_scaled_index, |ctx| {
    StoreBytes::Write8(ctx.stack.pop_u8_array::<8>())
});
define_shared_local_scaled_index_store_alias!(op_f32_store_shared_local_scaled_index, |ctx| {
    StoreBytes::Write4(ctx.stack.pop_u8_array::<4>())
});
define_shared_local_scaled_index_store_alias!(op_f64_store_shared_local_scaled_index, |ctx| {
    StoreBytes::Write8(ctx.stack.pop_u8_array::<8>())
});
define_shared_local_scaled_index_store_alias!(op_i32_store8_shared_local_scaled_index, |ctx| {
    StoreBytes::Write1(truncate_u32_to_u8_bytes(ctx.stack.pop_u32()))
});
define_shared_local_scaled_index_store_alias!(op_i32_store16_shared_local_scaled_index, |ctx| {
    StoreBytes::Write2(truncate_u32_to_u16_bytes(ctx.stack.pop_u32()))
});
define_shared_local_scaled_index_store_alias!(op_i64_store8_shared_local_scaled_index, |ctx| {
    StoreBytes::Write1(truncate_u64_to_u8_bytes(ctx.stack.pop_u64()))
});
define_shared_local_scaled_index_store_alias!(op_i64_store16_shared_local_scaled_index, |ctx| {
    StoreBytes::Write2(truncate_u64_to_u16_bytes(ctx.stack.pop_u64()))
});
define_shared_local_scaled_index_store_alias!(op_i64_store32_shared_local_scaled_index, |ctx| {
    StoreBytes::Write4(truncate_u64_to_u32_bytes(ctx.stack.pop_u64()))
});

define_indexed_shared_local_scaled_index_push_load!(
    op_i32_load_indexed_shared_local_scaled_index,
    "i32.load",
    4
);
define_indexed_shared_local_scaled_index_push_load!(
    op_i64_load_indexed_shared_local_scaled_index,
    "i64.load",
    8
);
define_indexed_shared_local_scaled_index_push_load!(
    op_f32_load_indexed_shared_local_scaled_index,
    "f32.load",
    4
);
define_indexed_shared_local_scaled_index_push_load!(
    op_f64_load_indexed_shared_local_scaled_index,
    "f64.load",
    8
);
define_indexed_shared_local_scaled_index_scalar_load!(
    op_i32_load8_u_indexed_shared_local_scaled_index,
    "i32.load8_u",
    shared_read_u8_at,
    push_u32_fast,
    u32::from
);
define_indexed_shared_local_scaled_index_scalar_load!(
    op_i32_load8_s_indexed_shared_local_scaled_index,
    "i32.load8_s",
    shared_read_i8_at,
    push_i32_fast,
    i32::from
);
define_indexed_shared_local_scaled_index_scalar_load!(
    op_i32_load16_s_indexed_shared_local_scaled_index,
    "i32.load16_s",
    shared_read_i16_at,
    push_i32_fast,
    i32::from
);
define_indexed_shared_local_scaled_index_scalar_load!(
    op_i32_load16_u_indexed_shared_local_scaled_index,
    "i32.load16_u",
    shared_read_u16_at,
    push_u32_fast,
    u32::from
);
define_indexed_shared_local_scaled_index_scalar_load!(
    op_i64_load8_s_indexed_shared_local_scaled_index,
    "i64.load8_s",
    shared_read_i8_at,
    push_i64_fast,
    i64::from
);
define_indexed_shared_local_scaled_index_scalar_load!(
    op_i64_load8_u_indexed_shared_local_scaled_index,
    "i64.load8_u",
    shared_read_u8_at,
    push_u64_fast,
    u64::from
);
define_indexed_shared_local_scaled_index_scalar_load!(
    op_i64_load16_s_indexed_shared_local_scaled_index,
    "i64.load16_s",
    shared_read_i16_at,
    push_i64_fast,
    i64::from
);
define_indexed_shared_local_scaled_index_scalar_load!(
    op_i64_load16_u_indexed_shared_local_scaled_index,
    "i64.load16_u",
    shared_read_u16_at,
    push_u64_fast,
    u64::from
);
define_indexed_shared_local_scaled_index_scalar_load!(
    op_i64_load32_s_indexed_shared_local_scaled_index,
    "i64.load32_s",
    shared_read_i32_at,
    push_i64_fast,
    i64::from
);
define_indexed_shared_local_scaled_index_scalar_load!(
    op_i64_load32_u_indexed_shared_local_scaled_index,
    "i64.load32_u",
    shared_read_u32_at,
    push_u64_fast,
    u64::from
);
define_indexed_shared_local_scaled_index_store_alias!(
    op_i32_store_indexed_shared_local_scaled_index,
    |ctx| { StoreBytes::Write4(ctx.stack.pop_u8_array::<4>()) }
);
define_indexed_shared_local_scaled_index_store_alias!(
    op_i64_store_indexed_shared_local_scaled_index,
    |ctx| { StoreBytes::Write8(ctx.stack.pop_u8_array::<8>()) }
);
define_indexed_shared_local_scaled_index_store_alias!(
    op_f32_store_indexed_shared_local_scaled_index,
    |ctx| { StoreBytes::Write4(ctx.stack.pop_u8_array::<4>()) }
);
define_indexed_shared_local_scaled_index_store_alias!(
    op_f64_store_indexed_shared_local_scaled_index,
    |ctx| { StoreBytes::Write8(ctx.stack.pop_u8_array::<8>()) }
);
define_indexed_shared_local_scaled_index_store_alias!(
    op_i32_store8_indexed_shared_local_scaled_index,
    |ctx| { StoreBytes::Write1(truncate_u32_to_u8_bytes(ctx.stack.pop_u32())) }
);
define_indexed_shared_local_scaled_index_store_alias!(
    op_i32_store16_indexed_shared_local_scaled_index,
    |ctx| { StoreBytes::Write2(truncate_u32_to_u16_bytes(ctx.stack.pop_u32())) }
);
define_indexed_shared_local_scaled_index_store_alias!(
    op_i64_store8_indexed_shared_local_scaled_index,
    |ctx| { StoreBytes::Write1(truncate_u64_to_u8_bytes(ctx.stack.pop_u64())) }
);
define_indexed_shared_local_scaled_index_store_alias!(
    op_i64_store16_indexed_shared_local_scaled_index,
    |ctx| { StoreBytes::Write2(truncate_u64_to_u16_bytes(ctx.stack.pop_u64())) }
);
define_indexed_shared_local_scaled_index_store_alias!(
    op_i64_store32_indexed_shared_local_scaled_index,
    |ctx| { StoreBytes::Write4(truncate_u64_to_u32_bytes(ctx.stack.pop_u64())) }
);

/// WebAssembly `memory.size` on shared default memory.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[] -> [i32]`.
/// Traps: traps when no default memory exists.
/// Notes: Uses the shared-memory specialized fast path selected by the parser and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose default memory is shared.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_mem_size_shared(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let page_size = ctx
        .gc
        .shared_memory(ctx.default_shared_memory_id_unchecked())
        .page_size();
    vm_try!(ctx.stack.push_u32(page_size));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `memory.grow` on shared default memory.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[i32] -> [i32]`.
/// Traps: traps when no default memory exists; otherwise returns `-1` on growth failure.
/// Notes: Uses the shared-memory specialized fast path selected by the parser and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose default memory is shared.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_mem_grow_shared(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let page_size_delta = ctx.stack.pop_u32();
    let result = vm_try!(ctx
        .gc
        .shared_grow_memory(ctx.default_shared_memory_id_unchecked(), page_size_delta,));
    vm_try!(ctx.stack.push_i32(result));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `memory.size` on indexed local memory.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/multi-memory/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/multi-memory/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/multi-memory/core/exec/instructions.html
///
/// Stack effect: `[] -> [i32]`.
/// Traps: traps when the indexed memory does not exist.
/// Notes: Uses the typed indexed local-memory fast path and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose indexed memory operand is in-bounds and local.
pub unsafe fn op_mem_size_indexed_local(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let memidx = (*tail_code).operand.u32;
    let page_size = ctx
        .gc
        .local_memory(ctx.local_memory_id_at_unchecked(memidx))
        .page_size();
    vm_try!(ctx.stack.push_u32(page_size));
    call_next(tail_code, 1, ctx)
}

/// WebAssembly `memory.size` on indexed shared memory.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/multi-memory/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/multi-memory/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/multi-memory/core/exec/instructions.html
///
/// Stack effect: `[] -> [i32]`.
/// Traps: traps when the indexed memory does not exist.
/// Notes: Uses the typed indexed shared-memory fast path and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose indexed memory operand is in-bounds and shared.
pub unsafe fn op_mem_size_indexed_shared(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let memidx = (*tail_code).operand.u32;
    let page_size = ctx
        .gc
        .shared_memory(ctx.shared_memory_id_at_unchecked(memidx))
        .page_size();
    vm_try!(ctx.stack.push_u32(page_size));
    call_next(tail_code, 1, ctx)
}

/// WebAssembly `memory.grow` on indexed local memory.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/multi-memory/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/multi-memory/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/multi-memory/core/exec/instructions.html
///
/// Stack effect: `[i32] -> [i32]`.
/// Traps: traps when the indexed memory does not exist; otherwise returns `-1` on growth failure.
/// Notes: Uses the typed indexed local-memory fast path and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose indexed memory operand is in-bounds and local.
pub unsafe fn op_mem_grow_indexed_local(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let memidx = (*tail_code).operand.u32;
    let page_size_delta = ctx.stack.pop_u32();
    let result = vm_try!(ctx
        .gc
        .local_grow_memory(ctx.local_memory_id_at_unchecked(memidx), page_size_delta));
    vm_try!(ctx.stack.push_i32(result));
    call_next(tail_code, 1, ctx)
}

/// WebAssembly `memory.grow` on indexed shared memory.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/multi-memory/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/multi-memory/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/multi-memory/core/exec/instructions.html
///
/// Stack effect: `[i32] -> [i32]`.
/// Traps: traps when the indexed memory does not exist; otherwise returns `-1` on growth failure.
/// Notes: Uses the typed indexed shared-memory fast path and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose indexed memory operand is in-bounds and shared.
pub unsafe fn op_mem_grow_indexed_shared(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let memidx = (*tail_code).operand.u32;
    let page_size_delta = ctx.stack.pop_u32();
    let result = vm_try!(ctx
        .gc
        .shared_grow_memory(ctx.shared_memory_id_at_unchecked(memidx), page_size_delta));
    vm_try!(ctx.stack.push_i32(result));
    call_next(tail_code, 1, ctx)
}

pub(crate) use op_f32_load as op_f32_load_local;
pub(crate) use op_f32_store as op_f32_store_local;
pub(crate) use op_f64_load as op_f64_load_local;
pub(crate) use op_f64_store as op_f64_store_local;
pub(crate) use op_i32_load as op_i32_load_local;
pub(crate) use op_i32_load16_s as op_i32_load16_s_local;
pub(crate) use op_i32_load16_u as op_i32_load16_u_local;
pub(crate) use op_i32_load8_s as op_i32_load8_s_local;
pub(crate) use op_i32_load8_u as op_i32_load8_u_local;
pub(crate) use op_i32_store as op_i32_store_local;
pub(crate) use op_i32_store16 as op_i32_store16_local;
pub(crate) use op_i32_store8 as op_i32_store8_local;
pub(crate) use op_i64_load as op_i64_load_local;
pub(crate) use op_i64_load16_s as op_i64_load16_s_local;
pub(crate) use op_i64_load16_u as op_i64_load16_u_local;
pub(crate) use op_i64_load32_s as op_i64_load32_s_local;
pub(crate) use op_i64_load32_u as op_i64_load32_u_local;
pub(crate) use op_i64_load8_s as op_i64_load8_s_local;
pub(crate) use op_i64_load8_u as op_i64_load8_u_local;
pub(crate) use op_i64_store as op_i64_store_local;
pub(crate) use op_i64_store16 as op_i64_store16_local;
pub(crate) use op_i64_store32 as op_i64_store32_local;
pub(crate) use op_i64_store8 as op_i64_store8_local;
pub(crate) use op_mem_grow as op_mem_grow_local;
pub(crate) use op_mem_size as op_mem_size_local;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        common::{
            stack::{CachedMemoryKind, CallFrameCache},
            store::{InstanceId, MemoryHandle},
            ExecuteContext, LocalMemoryObject, LocalReference, Memory, ObjectRef, Operand, Store,
            StoreInner,
        },
        runtime::{memory_effect::PendingOp, scheduler::EffectSupplier},
    };
    #[cfg(feature = "vm-profile")]
    use crate::{
        common::{InstanceHandle, Registry, ResultValue, WasmValue},
        IoReadBinaryReader, WasmParser,
    };
    use std::collections::VecDeque;

    fn frame(kind: CachedMemoryKind, raw: u32) -> CallFrameCache {
        CallFrameCache {
            code_addr: ObjectRef(0),
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
        queue: &'a mut VecDeque<PendingOp>,
    ) -> ExecuteContext<'a> {
        let MemoryHandle::Local(memory_id) =
            gc.alloc_local_memory(LocalMemoryObject::new(1, 1).expect("test local memory"))
        else {
            unreachable!("test local memory handle must be local");
        };
        let local_reference = LocalReference {
            local_top: 0,
            local_size: 0,
        };
        let local_base_ptr = unsafe { stack.local_area_mut_ptr(&local_reference) };
        ExecuteContext {
            stack,
            local_reference,
            local_base_ptr,
            default_local_memory_ptr: gc.local_memory_mut(memory_id).memory_mut() as *mut Memory,
            current_frame: frame(CachedMemoryKind::Local, memory_id.raw()),
            store,
            gc,
            effect: EffectSupplier::from_parts(1, pending_effects, queue),
            cont: std::ptr::null(),
            task_id: 1,
        }
    }

    #[cfg(feature = "vm-profile")]
    async fn instantiate_wat(wat: &str, store: &Store, registry: &Registry) -> InstanceHandle {
        let bytes = wat::parse_str(wat).expect("wat must parse");
        let mut reader = IoReadBinaryReader::from(bytes.as_slice());
        let mut parser = WasmParser::new(&mut reader);
        let module = parser.parse_module().expect("module must parse");
        match crate::instantiate(module, store, registry).await {
            VMResult::Success(instance) => instance,
            other => panic!("module must instantiate, got {other:?}"),
        }
    }

    #[test]
    fn load_start_helpers_match_offset_and_index_contracts() {
        let store = Store::new();
        let mut gc = StoreInner::new();
        let mut pending_effects = 0;
        let mut effects = VecDeque::new();
        let mut stack = Stack::new(32);
        stack.push_u32(5).unwrap();

        let program = [
            Instr {
                operand: Operand {
                    memarg: MemArg {
                        align: 2,
                        offset: 7,
                    },
                },
            },
            Instr {
                operand: Operand { u32: 3 },
            },
        ];
        let mut ctx = test_context(
            &mut stack,
            &store,
            &mut gc,
            &mut pending_effects,
            &mut effects,
        );

        let start = unsafe { load_start(program.as_ptr(), &mut ctx) }.unwrap();
        assert_eq!(start, 12);

        ctx.stack.push_u32(11).unwrap();
        let (indexed_start, memidx) =
            unsafe { load_start_indexed(program.as_ptr(), &mut ctx) }.unwrap();
        assert_eq!(indexed_start, 18);
        assert_eq!(memidx, 3);
    }

    #[test]
    fn load_start_fail_closes_memory_offset_overflow() {
        let store = Store::new();
        let mut gc = StoreInner::new();
        let mut pending_effects = 0;
        let mut effects = VecDeque::new();
        let mut stack = Stack::new(16);
        stack.push_u32(1).unwrap();

        let program = [Instr {
            operand: Operand {
                memarg: MemArg {
                    align: 0,
                    offset: u32::MAX,
                },
            },
        }];
        let mut ctx = test_context(
            &mut stack,
            &store,
            &mut gc,
            &mut pending_effects,
            &mut effects,
        );

        assert!(matches!(
            unsafe { load_start(program.as_ptr(), &mut ctx) },
            VMResult::MemoryIndexOutOfRange
        ));
    }

    #[cfg(feature = "vm-profile")]
    #[ignore = "requires profiled memory wrapper ops"]
    #[tokio::test]
    async fn profiler_prefers_local_base_memory_families_over_generic_path() {
        let store = Store::new();
        let registry = Registry::new();
        let instance = instantiate_wat(
            r#"
            (module
              (memory 1)
              (func (export "run") (param $base i32) (param $remaining i32) (result i32)
                local.get $base
                i32.const 0
                i32.store
                block $done
                  loop $loop
                    local.get $remaining
                    i32.eqz
                    br_if $done

                    local.get $base
                    local.get $base
                    i32.load
                    i32.const 1
                    i32.add
                    i32.store

                    local.get $remaining
                    i32.const 1
                    i32.sub
                    local.set $remaining
                    br $loop
                  end
                end
                local.get $base
                i32.load))
            "#,
            &store,
            &registry,
        )
        .await;

        let _profile = super::super::DispatchProfileTestOverride::enable();
        let result = crate::run_module_function(
            &instance,
            &store,
            "run",
            &ResultValue::new(vec![WasmValue::I32(8), WasmValue::I32(12)]),
        )
        .await;

        match result {
            VMResult::Success(values) => {
                assert_eq!(values, ResultValue::new(vec![WasmValue::I32(12)]));
            }
            other => panic!("profiled local-base memory loop must succeed, got {other:?}"),
        }

        let snapshot = super::super::take_last_dispatch_profile_snapshot_for_test()
            .expect("profile snapshot must be recorded");
        let count = |label: &'static str| {
            snapshot
                .stats
                .iter()
                .find_map(|(candidate, stat)| (*candidate == label).then_some(stat.count))
                .unwrap_or_default()
        };

        let specialized_load = count("op_i32_load_local_base");
        let specialized_store = count("op_i32_store_local_base");
        let generic_load = count("op_i32_load");
        let generic_store = count("op_i32_store");

        assert!(
            specialized_load > 0,
            "local-base load family must appear in dispatch profile: {:?}",
            snapshot.stats
        );
        assert!(
            specialized_store > 0,
            "local-base store family must appear in dispatch profile: {:?}",
            snapshot.stats
        );
        assert!(
            specialized_load > generic_load,
            "specialized load family must dominate generic load path: {:?}",
            snapshot.stats
        );
        assert!(
            specialized_store > generic_store,
            "specialized store family must dominate generic store path: {:?}",
            snapshot.stats
        );
    }
}
