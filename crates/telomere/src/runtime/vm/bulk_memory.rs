use super::*;

#[inline(never)]
fn mem_init_bytes(
    ctx: &mut ExecuteContext,
    idx: u32,
    src: u32,
    len: u32,
) -> VMResult<Option<Vec<u8>>> {
    let instance_id = ctx.instance_id();
    let copied = {
        let segments = ctx.store.lock_segments();
        let data = segments.data.get(&(instance_id, idx));
        if data.is_none() && len == 0 && src == 0 {
            None
        } else {
            let data = vm_try!(VMResult::from_option(data, || {
                VMResult::MemoryIndexOutOfRange
            }));
            let src_last = vm_try!(VMResult::from_option(src.checked_add(len), || {
                VMResult::MemoryIndexOutOfRange
            })) as usize;
            let data = vm_try!(VMResult::from_option(
                data.init.get(src as usize..src_last),
                || { VMResult::MemoryIndexOutOfRange }
            ));
            Some(data.to_vec())
        }
    };
    VMResult::Success(copied)
}

#[inline(never)]
fn mem_init_impl_local(
    ctx: &mut ExecuteContext,
    idx: u32,
    dst: u32,
    src: u32,
    len: u32,
) -> VMResult<()> {
    mem_init_impl_local_with_id(
        ctx,
        unsafe { ctx.default_local_memory_id_unchecked() },
        idx,
        dst,
        src,
        len,
    )
}

#[inline(never)]
fn mem_init_impl_local_with_id(
    ctx: &mut ExecuteContext,
    memory: crate::common::store::LocalMemoryId,
    idx: u32,
    dst: u32,
    src: u32,
    len: u32,
) -> VMResult<()> {
    let copied = vm_try!(mem_init_bytes(ctx, idx, src, len));
    ctx.gc
        .local_write_bytes(memory, dst as usize, copied.as_deref().unwrap_or(&[]))
}

#[inline(never)]
fn mem_init_impl_shared(
    ctx: &mut ExecuteContext,
    idx: u32,
    dst: u32,
    src: u32,
    len: u32,
) -> VMResult<()> {
    mem_init_impl_shared_with_id(
        ctx,
        unsafe { ctx.default_shared_memory_id_unchecked() },
        idx,
        dst,
        src,
        len,
    )
}

#[inline(never)]
fn mem_init_impl_shared_with_id(
    ctx: &mut ExecuteContext,
    memory: crate::common::store::SharedMemoryId,
    idx: u32,
    dst: u32,
    src: u32,
    len: u32,
) -> VMResult<()> {
    let copied = vm_try!(mem_init_bytes(ctx, idx, src, len));
    ctx.gc
        .shared_write_bytes(memory, dst as usize, copied.as_deref().unwrap_or(&[]))
}

#[inline(never)]
fn data_drop_impl(ctx: &mut ExecuteContext, instance_id: u32, idx: u32) {
    let _ = ctx.store.lock_segments().data.remove(&(instance_id, idx));
}

/// WebAssembly `memory.init`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[dst, src, len] -> []`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Implements the bulk-memory instruction on the default linear memory and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_mem_init(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let idx = (*tail_code).operand.u32;
    let len = ctx.stack.pop_u32();
    let src = ctx.stack.pop_u32();
    let dst = ctx.stack.pop_u32();
    vm_try!(mem_init_impl_local(ctx, idx, dst, src, len));
    call_next(tail_code, 1, ctx)
}

/// WebAssembly `data.drop`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[] -> []`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Implements the bulk-memory instruction on the default linear memory and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_data_drop(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    if ctx.effect.get_pending_count() != 0 {
        trace!("waiting effect: {:?}", ctx.cont);
        return VMResult::Success(());
    }
    let idx = (*tail_code).operand.u32;
    let instance_id = ctx.instance_id();
    data_drop_impl(ctx, instance_id, idx);
    call_next(tail_code, 1, ctx)
}

/// WebAssembly `memory.copy`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[dst, src, len] -> []`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Implements the bulk-memory instruction on the default linear memory and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_mem_copy(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    if ctx.effect.get_pending_count() != 0 {
        trace!("waiting effect: {:?}", ctx.cont);
        return VMResult::Success(());
    }
    let len = ctx.stack.pop_u32();
    let src = ctx.stack.pop_u32();
    let dst = ctx.stack.pop_u32();
    trace!("op_mem_copy src: {src},dst: {dst},len: {len}");
    vm_try!(ctx
        .gc
        .local_copy_memory(ctx.default_local_memory_id_unchecked(), dst, src, len,));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `memory.fill`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[dst, value, len] -> []`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Implements the bulk-memory instruction on the default linear memory and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_mem_fill(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let len = ctx.stack.pop_u32();
    let data = ctx.stack.pop_u32();
    let ptr = ctx.stack.pop_u32();
    vm_try!(ctx
        .gc
        .local_fill_memory(ctx.default_local_memory_id_unchecked(), ptr, len, data,));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `memory.init` on shared default memory.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[dst, src, len] -> []`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Uses the shared-memory specialized fast path selected by the parser and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose default memory is shared.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_mem_init_shared(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let idx = (*tail_code).operand.u32;
    let len = ctx.stack.pop_u32();
    let src = ctx.stack.pop_u32();
    let dst = ctx.stack.pop_u32();
    vm_try!(mem_init_impl_shared(ctx, idx, dst, src, len));
    call_next(tail_code, 1, ctx)
}

/// WebAssembly `memory.copy` on shared default memory.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[dst, src, len] -> []`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Uses the shared-memory specialized fast path selected by the parser and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose default memory is shared.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_mem_copy_shared(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    if ctx.effect.get_pending_count() != 0 {
        trace!("waiting effect: {:?}", ctx.cont);
        return VMResult::Success(());
    }
    let len = ctx.stack.pop_u32();
    let src = ctx.stack.pop_u32();
    let dst = ctx.stack.pop_u32();
    trace!("op_mem_copy_shared src: {src},dst: {dst},len: {len}");
    vm_try!(ctx
        .gc
        .shared_copy_memory(ctx.default_shared_memory_id_unchecked(), dst, src, len,));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `memory.fill` on shared default memory.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[dst, value, len] -> []`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Uses the shared-memory specialized fast path selected by the parser and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose default memory is shared.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_mem_fill_shared(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let len = ctx.stack.pop_u32();
    let data = ctx.stack.pop_u32();
    let ptr = ctx.stack.pop_u32();
    vm_try!(ctx
        .gc
        .shared_fill_memory(ctx.default_shared_memory_id_unchecked(), ptr, len, data,));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `memory.init` on indexed local memory.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/multi-memory/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/multi-memory/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/multi-memory/core/exec/instructions.html
///
/// Stack effect: `[dst, src, len] -> []`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Uses the typed indexed local-memory fast path and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose indexed memory operand is in-bounds and local.
pub unsafe fn op_mem_init_indexed_local(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let idx = (*tail_code).operand.u32;
    let memidx = (*tail_code.add(1)).operand.u32;
    let len = ctx.stack.pop_u32();
    let src = ctx.stack.pop_u32();
    let dst = ctx.stack.pop_u32();
    vm_try!(mem_init_impl_local_with_id(
        ctx,
        ctx.local_memory_id_at_unchecked(memidx),
        idx,
        dst,
        src,
        len,
    ));
    call_next(tail_code, 2, ctx)
}

/// WebAssembly `memory.init` on indexed shared memory.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/multi-memory/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/multi-memory/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/multi-memory/core/exec/instructions.html
///
/// Stack effect: `[dst, src, len] -> []`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Uses the typed indexed shared-memory fast path and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose indexed memory operand is in-bounds and shared.
pub unsafe fn op_mem_init_indexed_shared(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let idx = (*tail_code).operand.u32;
    let memidx = (*tail_code.add(1)).operand.u32;
    let len = ctx.stack.pop_u32();
    let src = ctx.stack.pop_u32();
    let dst = ctx.stack.pop_u32();
    vm_try!(mem_init_impl_shared_with_id(
        ctx,
        ctx.shared_memory_id_at_unchecked(memidx),
        idx,
        dst,
        src,
        len,
    ));
    call_next(tail_code, 2, ctx)
}

/// WebAssembly `memory.copy` from indexed local memory to indexed local memory.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/multi-memory/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/multi-memory/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/multi-memory/core/exec/instructions.html
///
/// Stack effect: `[dst, src, len] -> []`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Uses the typed indexed local-to-local fast path and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose indexed memory operands are in-bounds and local.
pub unsafe fn op_mem_copy_indexed_local_local(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    if ctx.effect.get_pending_count() != 0 {
        trace!("waiting effect: {:?}", ctx.cont);
        return VMResult::Success(());
    }
    let dst_memidx = (*tail_code).operand.u32;
    let src_memidx = (*tail_code.add(1)).operand.u32;
    let len = ctx.stack.pop_u32();
    let src = ctx.stack.pop_u32();
    let dst = ctx.stack.pop_u32();
    vm_try!(ctx.gc.copy_memory_local_to_local(
        ctx.local_memory_id_at_unchecked(dst_memidx),
        ctx.local_memory_id_at_unchecked(src_memidx),
        dst,
        src,
        len,
    ));
    call_next(tail_code, 2, ctx)
}

/// WebAssembly `memory.copy` from indexed shared memory to indexed local memory.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/multi-memory/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/multi-memory/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/multi-memory/core/exec/instructions.html
///
/// Stack effect: `[dst, src, len] -> []`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Uses the typed indexed shared-to-local path and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose destination memory is local and source memory is shared.
pub unsafe fn op_mem_copy_indexed_local_shared(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    if ctx.effect.get_pending_count() != 0 {
        trace!("waiting effect: {:?}", ctx.cont);
        return VMResult::Success(());
    }
    let dst_memidx = (*tail_code).operand.u32;
    let src_memidx = (*tail_code.add(1)).operand.u32;
    let len = ctx.stack.pop_u32();
    let src = ctx.stack.pop_u32();
    let dst = ctx.stack.pop_u32();
    vm_try!(ctx.gc.copy_memory_shared_to_local(
        ctx.local_memory_id_at_unchecked(dst_memidx),
        ctx.shared_memory_id_at_unchecked(src_memidx),
        dst,
        src,
        len,
    ));
    call_next(tail_code, 2, ctx)
}

/// WebAssembly `memory.copy` from indexed local memory to indexed shared memory.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/multi-memory/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/multi-memory/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/multi-memory/core/exec/instructions.html
///
/// Stack effect: `[dst, src, len] -> []`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Uses the typed indexed local-to-shared path and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose destination memory is shared and source memory is local.
pub unsafe fn op_mem_copy_indexed_shared_local(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    if ctx.effect.get_pending_count() != 0 {
        trace!("waiting effect: {:?}", ctx.cont);
        return VMResult::Success(());
    }
    let dst_memidx = (*tail_code).operand.u32;
    let src_memidx = (*tail_code.add(1)).operand.u32;
    let len = ctx.stack.pop_u32();
    let src = ctx.stack.pop_u32();
    let dst = ctx.stack.pop_u32();
    vm_try!(ctx.gc.copy_memory_local_to_shared(
        ctx.shared_memory_id_at_unchecked(dst_memidx),
        ctx.local_memory_id_at_unchecked(src_memidx),
        dst,
        src,
        len,
    ));
    call_next(tail_code, 2, ctx)
}

/// WebAssembly `memory.copy` from indexed shared memory to indexed shared memory.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/multi-memory/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/multi-memory/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/multi-memory/core/exec/instructions.html
///
/// Stack effect: `[dst, src, len] -> []`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Uses the typed indexed shared-to-shared path and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose indexed memory operands are in-bounds and shared.
pub unsafe fn op_mem_copy_indexed_shared_shared(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    if ctx.effect.get_pending_count() != 0 {
        trace!("waiting effect: {:?}", ctx.cont);
        return VMResult::Success(());
    }
    let dst_memidx = (*tail_code).operand.u32;
    let src_memidx = (*tail_code.add(1)).operand.u32;
    let len = ctx.stack.pop_u32();
    let src = ctx.stack.pop_u32();
    let dst = ctx.stack.pop_u32();
    vm_try!(ctx.gc.copy_memory_shared_to_shared(
        ctx.shared_memory_id_at_unchecked(dst_memidx),
        ctx.shared_memory_id_at_unchecked(src_memidx),
        dst,
        src,
        len,
    ));
    call_next(tail_code, 2, ctx)
}

/// WebAssembly `memory.fill` on indexed local memory.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/multi-memory/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/multi-memory/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/multi-memory/core/exec/instructions.html
///
/// Stack effect: `[dst, value, len] -> []`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Uses the typed indexed local-memory fast path and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose indexed memory operand is in-bounds and local.
pub unsafe fn op_mem_fill_indexed_local(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let memidx = (*tail_code).operand.u32;
    let len = ctx.stack.pop_u32();
    let data = ctx.stack.pop_u32();
    let ptr = ctx.stack.pop_u32();
    vm_try!(ctx
        .gc
        .local_fill_memory(ctx.local_memory_id_at_unchecked(memidx), ptr, len, data,));
    call_next(tail_code, 1, ctx)
}

/// WebAssembly `memory.fill` on indexed shared memory.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/multi-memory/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/multi-memory/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/multi-memory/core/exec/instructions.html
///
/// Stack effect: `[dst, value, len] -> []`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Uses the typed indexed shared-memory fast path and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose indexed memory operand is in-bounds and shared.
pub unsafe fn op_mem_fill_indexed_shared(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let memidx = (*tail_code).operand.u32;
    let len = ctx.stack.pop_u32();
    let data = ctx.stack.pop_u32();
    let ptr = ctx.stack.pop_u32();
    vm_try!(ctx
        .gc
        .shared_fill_memory(ctx.shared_memory_id_at_unchecked(memidx), ptr, len, data,));
    call_next(tail_code, 1, ctx)
}

pub(crate) use op_mem_copy as op_mem_copy_local;
pub(crate) use op_mem_fill as op_mem_fill_local;
pub(crate) use op_mem_init as op_mem_init_local;
