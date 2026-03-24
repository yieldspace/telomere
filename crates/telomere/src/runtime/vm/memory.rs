use super::*;

macro_rules! replicated_local_push_load4 {
    ($name:ident) => {
        #[inline(never)]
        pub(crate) unsafe fn $name(
            tail_code: *const Instr,
            ctx: &mut ExecuteContext,
        ) -> VMResult<()> {
            let start = vm_try!(load_start(tail_code, ctx));
            vm_try!(ctx.gc.local_push_memory_to_stack::<4>(
                ctx.default_local_memory_id_unchecked(),
                ctx.stack,
                start,
            ));
            call_next(tail_code, 1, ctx)
        }
    };
}

macro_rules! replicated_local_i32_load8_u {
    ($name:ident) => {
        #[inline(never)]
        pub(crate) unsafe fn $name(
            tail_code: *const Instr,
            ctx: &mut ExecuteContext,
        ) -> VMResult<()> {
            let start = vm_try!(load_start(tail_code, ctx));
            let value = vm_try!(ctx
                .gc
                .local_read_u8_at(ctx.default_local_memory_id_unchecked(), start,));
            vm_try!(ctx.stack.push_u32(u32::from(value)));
            call_next(tail_code, 1, ctx)
        }
    };
}

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
fn pop_store_bytes4(ctx: &mut ExecuteContext) -> StoreBytes {
    StoreBytes::Write4(ctx.stack.pop_u32_bytes())
}

#[inline(always)]
fn pop_store_bytes8(ctx: &mut ExecuteContext) -> StoreBytes {
    StoreBytes::Write8(ctx.stack.pop_u64_bytes())
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
            store_internal_local_indexed(tail_code, ctx, $make_operation)
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
            store_internal_shared_indexed(tail_code, ctx, $make_operation)
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
    let start = vm_try!(load_start(tail_code, ctx));
    vm_try!(ctx.gc.local_push_memory_to_stack::<4>(
        ctx.default_local_memory_id_unchecked(),
        ctx.stack,
        start,
    ));
    call_next(tail_code, 1, ctx)
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
    let start = vm_try!(load_start(tail_code, ctx));
    vm_try!(ctx.gc.local_push_memory_to_stack::<8>(
        ctx.default_local_memory_id_unchecked(),
        ctx.stack,
        start,
    ));
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
    let start = vm_try!(load_start(tail_code, ctx));
    vm_try!(ctx.gc.local_push_memory_to_stack::<4>(
        ctx.default_local_memory_id_unchecked(),
        ctx.stack,
        start,
    ));
    call_next(tail_code, 1, ctx)
}

replicated_local_push_load4!(op_f32_load_local_r0);
replicated_local_push_load4!(op_f32_load_local_r1);
replicated_local_push_load4!(op_f32_load_local_r2);
replicated_local_push_load4!(op_f32_load_local_r3);

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
    let start = vm_try!(load_start(tail_code, ctx));
    vm_try!(ctx.gc.local_push_memory_to_stack::<8>(
        ctx.default_local_memory_id_unchecked(),
        ctx.stack,
        start,
    ));
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
    let start = vm_try!(load_start(tail_code, ctx));
    let value = vm_try!(ctx
        .gc
        .local_read_u8_at(ctx.default_local_memory_id_unchecked(), start,));
    vm_try!(ctx.stack.push_u32(u32::from(value)));
    call_next(tail_code, 1, ctx)
}

replicated_local_i32_load8_u!(op_i32_load8_u_local_r0);
replicated_local_i32_load8_u!(op_i32_load8_u_local_r1);
replicated_local_i32_load8_u!(op_i32_load8_u_local_r2);
replicated_local_i32_load8_u!(op_i32_load8_u_local_r3);

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
    let start = vm_try!(load_start(tail_code, ctx));
    let value = vm_try!(ctx
        .gc
        .local_read_i8_at(ctx.default_local_memory_id_unchecked(), start,));
    vm_try!(ctx.stack.push_i32(i32::from(value)));
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
    let start = vm_try!(load_start(tail_code, ctx));
    let value = vm_try!(ctx
        .gc
        .local_read_i16_at(ctx.default_local_memory_id_unchecked(), start,));
    vm_try!(ctx.stack.push_i32(i32::from(value)));
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
    let start = vm_try!(load_start(tail_code, ctx));
    let value = vm_try!(ctx
        .gc
        .local_read_u16_at(ctx.default_local_memory_id_unchecked(), start,));
    vm_try!(ctx.stack.push_u32(u32::from(value)));
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
    let start = vm_try!(load_start(tail_code, ctx));
    let value = vm_try!(ctx
        .gc
        .local_read_i8_at(ctx.default_local_memory_id_unchecked(), start,));
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
    let start = vm_try!(load_start(tail_code, ctx));
    let value = vm_try!(ctx
        .gc
        .local_read_u8_at(ctx.default_local_memory_id_unchecked(), start,));
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
    let start = vm_try!(load_start(tail_code, ctx));
    let value = vm_try!(ctx
        .gc
        .local_read_i16_at(ctx.default_local_memory_id_unchecked(), start,));
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
    let start = vm_try!(load_start(tail_code, ctx));
    let value = vm_try!(ctx
        .gc
        .local_read_u16_at(ctx.default_local_memory_id_unchecked(), start,));
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
    let start = vm_try!(load_start(tail_code, ctx));
    let value = vm_try!(ctx
        .gc
        .local_read_i32_at(ctx.default_local_memory_id_unchecked(), start,));
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
    let start = vm_try!(load_start(tail_code, ctx));
    let value = vm_try!(ctx
        .gc
        .local_read_u32_at(ctx.default_local_memory_id_unchecked(), start,));
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
    store_internal_local(tail_code, ctx, pop_store_bytes4)
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
    store_internal_local(tail_code, ctx, pop_store_bytes8)
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
    store_internal_local(tail_code, ctx, pop_store_bytes4)
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
    store_internal_local(tail_code, ctx, pop_store_bytes8)
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
    store_internal_local(tail_code, ctx, |ctx| {
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
    store_internal_local(tail_code, ctx, |ctx| {
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
    store_internal_local(tail_code, ctx, |ctx| {
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
    store_internal_local(tail_code, ctx, |ctx| {
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
    store_internal_local(tail_code, ctx, |ctx| {
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
    let page_size = ctx
        .gc
        .local_memory(ctx.default_local_memory_id_unchecked())
        .page_size();
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
            store_internal_shared(tail_code, ctx, $make_operation)
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
    push_u32,
    u32::from
);
define_shared_scalar_load!(
    op_i32_load8_s_shared,
    "i32.load8_s",
    shared_read_i8_at,
    push_i32,
    i32::from
);
define_shared_scalar_load!(
    op_i32_load16_s_shared,
    "i32.load16_s",
    shared_read_i16_at,
    push_i32,
    i32::from
);
define_shared_scalar_load!(
    op_i32_load16_u_shared,
    "i32.load16_u",
    shared_read_u16_at,
    push_u32,
    u32::from
);
define_shared_scalar_load!(
    op_i64_load8_s_shared,
    "i64.load8_s",
    shared_read_i8_at,
    push_i64,
    i64::from
);
define_shared_scalar_load!(
    op_i64_load8_u_shared,
    "i64.load8_u",
    shared_read_u8_at,
    push_u64,
    u64::from
);
define_shared_scalar_load!(
    op_i64_load16_s_shared,
    "i64.load16_s",
    shared_read_i16_at,
    push_i64,
    i64::from
);
define_shared_scalar_load!(
    op_i64_load16_u_shared,
    "i64.load16_u",
    shared_read_u16_at,
    push_u64,
    u64::from
);
define_shared_scalar_load!(
    op_i64_load32_s_shared,
    "i64.load32_s",
    shared_read_i32_at,
    push_i64,
    i64::from
);
define_shared_scalar_load!(
    op_i64_load32_u_shared,
    "i64.load32_u",
    shared_read_u32_at,
    push_u64,
    u64::from
);
define_shared_store_alias!(op_i32_store_shared, "i32.store", |ctx| {
    pop_store_bytes4(ctx)
});
define_shared_store_alias!(op_i64_store_shared, "i64.store", |ctx| {
    pop_store_bytes8(ctx)
});
define_shared_store_alias!(op_f32_store_shared, "f32.store", |ctx| {
    pop_store_bytes4(ctx)
});
define_shared_store_alias!(op_f64_store_shared, "f64.store", |ctx| {
    pop_store_bytes8(ctx)
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
    push_u32,
    u32::from
);
define_indexed_scalar_load!(
    op_i32_load8_s_indexed_local,
    op_i32_load8_s_indexed_shared,
    "i32.load8_s",
    local_read_i8_at,
    shared_read_i8_at,
    push_i32,
    i32::from
);
define_indexed_scalar_load!(
    op_i32_load16_s_indexed_local,
    op_i32_load16_s_indexed_shared,
    "i32.load16_s",
    local_read_i16_at,
    shared_read_i16_at,
    push_i32,
    i32::from
);
define_indexed_scalar_load!(
    op_i32_load16_u_indexed_local,
    op_i32_load16_u_indexed_shared,
    "i32.load16_u",
    local_read_u16_at,
    shared_read_u16_at,
    push_u32,
    u32::from
);
define_indexed_scalar_load!(
    op_i64_load8_s_indexed_local,
    op_i64_load8_s_indexed_shared,
    "i64.load8_s",
    local_read_i8_at,
    shared_read_i8_at,
    push_i64,
    i64::from
);
define_indexed_scalar_load!(
    op_i64_load8_u_indexed_local,
    op_i64_load8_u_indexed_shared,
    "i64.load8_u",
    local_read_u8_at,
    shared_read_u8_at,
    push_u64,
    u64::from
);
define_indexed_scalar_load!(
    op_i64_load16_s_indexed_local,
    op_i64_load16_s_indexed_shared,
    "i64.load16_s",
    local_read_i16_at,
    shared_read_i16_at,
    push_i64,
    i64::from
);
define_indexed_scalar_load!(
    op_i64_load16_u_indexed_local,
    op_i64_load16_u_indexed_shared,
    "i64.load16_u",
    local_read_u16_at,
    shared_read_u16_at,
    push_u64,
    u64::from
);
define_indexed_scalar_load!(
    op_i64_load32_s_indexed_local,
    op_i64_load32_s_indexed_shared,
    "i64.load32_s",
    local_read_i32_at,
    shared_read_i32_at,
    push_i64,
    i64::from
);
define_indexed_scalar_load!(
    op_i64_load32_u_indexed_local,
    op_i64_load32_u_indexed_shared,
    "i64.load32_u",
    local_read_u32_at,
    shared_read_u32_at,
    push_u64,
    u64::from
);
define_indexed_store_alias!(
    op_i32_store_indexed_local,
    op_i32_store_indexed_shared,
    "i32.store",
    |ctx| { pop_store_bytes4(ctx) }
);
define_indexed_store_alias!(
    op_i64_store_indexed_local,
    op_i64_store_indexed_shared,
    "i64.store",
    |ctx| { pop_store_bytes8(ctx) }
);
define_indexed_store_alias!(
    op_f32_store_indexed_local,
    op_f32_store_indexed_shared,
    "f32.store",
    |ctx| { pop_store_bytes4(ctx) }
);
define_indexed_store_alias!(
    op_f64_store_indexed_local,
    op_f64_store_indexed_shared,
    "f64.store",
    |ctx| { pop_store_bytes8(ctx) }
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
            store::InstanceId,
            ExecuteContext, LocalReference, ObjectRef, Operand, SafepointMetadataCache, Store,
            StoreInner,
        },
        runtime::{memory_effect::PendingOp, scheduler::EffectSupplier},
    };
    use std::collections::VecDeque;

    fn frame(kind: CachedMemoryKind, raw: u32) -> CallFrameCache {
        CallFrameCache {
            code_addr: ObjectRef(0),
            code_base: std::ptr::null(),
            code_len: 0,
            function_return_site_addr: 0,
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
        ExecuteContext {
            stack,
            local_reference: LocalReference::empty(),
            current_frame: frame(CachedMemoryKind::Local, 1),
            safepoint: SafepointMetadataCache::EMPTY,
            store,
            gc,
            effect: EffectSupplier::from_parts(1, pending_effects, queue),
            cont: std::ptr::null(),
            task_id: 1,
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
}
