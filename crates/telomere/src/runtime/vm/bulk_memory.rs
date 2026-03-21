use super::*;

#[inline(always)]
unsafe fn pop_copy_operands(mut facade: &mut ExecuteContextFacade<'_, '_>) -> (u32, u32, u32) {
    let len = facade.stack_mut().pop_u32();
    let src = facade.stack_mut().pop_u32();
    let dst = facade.stack_mut().pop_u32();
    (dst, src, len)
}

#[inline(always)]
unsafe fn pop_fill_operands(mut facade: &mut ExecuteContextFacade<'_, '_>) -> (u32, u32, u32) {
    let len = facade.stack_mut().pop_u32();
    let data = facade.stack_mut().pop_u32();
    let ptr = facade.stack_mut().pop_u32();
    (ptr, data, len)
}

#[inline(always)]
unsafe fn pop_init_operands(mut facade: &mut ExecuteContextFacade<'_, '_>) -> (u32, u32, u32) {
    let len = facade.stack_mut().pop_u32();
    let src = facade.stack_mut().pop_u32();
    let dst = facade.stack_mut().pop_u32();
    (dst, src, len)
}

#[inline(never)]
fn mem_init_bytes(
    mut facade: &mut ExecuteContextFacade<'_, '_>,
    idx: u32,
    src: u32,
    len: u32,
) -> VMResult<Option<Vec<u8>>> {
    let instance_id = facade.instance_id();
    let copied = {
        let segments = facade.store_ref().lock_segments();
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
    mut facade: &mut ExecuteContextFacade<'_, '_>,
    idx: u32,
    dst: u32,
    src: u32,
    len: u32,
) -> VMResult<()> {
    let memory = unsafe { facade.default_local_memory_id_unchecked() };
    mem_init_impl_local_with_id(
        facade,
        memory,
        idx,
        dst,
        src,
        len,
    )
}

#[inline(never)]
fn mem_init_impl_local_with_id(
    mut facade: &mut ExecuteContextFacade<'_, '_>,
    memory: crate::common::store::LocalMemoryId,
    idx: u32,
    dst: u32,
    src: u32,
    len: u32,
) -> VMResult<()> {
    let copied = vm_try!(mem_init_bytes(facade, idx, src, len));
    facade.gc_mut()
        .local_write_bytes(memory, dst as usize, copied.as_deref().unwrap_or(&[]))
}

#[inline(never)]
fn mem_init_impl_shared(
    mut facade: &mut ExecuteContextFacade<'_, '_>,
    idx: u32,
    dst: u32,
    src: u32,
    len: u32,
) -> VMResult<()> {
    let memory = unsafe { facade.default_shared_memory_id_unchecked() };
    mem_init_impl_shared_with_id(
        facade,
        memory,
        idx,
        dst,
        src,
        len,
    )
}

#[inline(never)]
fn mem_init_impl_shared_with_id(
    mut facade: &mut ExecuteContextFacade<'_, '_>,
    memory: crate::common::store::SharedMemoryId,
    idx: u32,
    dst: u32,
    src: u32,
    len: u32,
) -> VMResult<()> {
    let copied = vm_try!(mem_init_bytes(facade, idx, src, len));
    facade.gc_mut()
        .shared_write_bytes(memory, dst as usize, copied.as_deref().unwrap_or(&[]))
}

#[inline(never)]
 fn data_drop_impl(mut facade: &mut ExecuteContextFacade<'_, '_>, instance_id: u32, idx: u32) {
    let _ = facade
        .store_ref()
        .lock_segments()
        .data
        .remove(&(instance_id, idx));
}

#[inline(never)]
fn mem_copy_impl_local_with_id(
    mut facade: &mut ExecuteContextFacade<'_, '_>,
    memory: crate::common::store::LocalMemoryId,
    dst: u32,
    src: u32,
    len: u32,
) -> VMResult<()> {
    facade.gc_mut().local_copy_memory(memory, dst, src, len)
}

#[inline(never)]
fn mem_copy_impl_shared_with_id(
    mut facade: &mut ExecuteContextFacade<'_, '_>,
    memory: crate::common::store::SharedMemoryId,
    dst: u32,
    src: u32,
    len: u32,
) -> VMResult<()> {
    facade.gc_mut().shared_copy_memory(memory, dst, src, len)
}

#[inline(never)]
fn mem_fill_impl_local_with_id(
    mut facade: &mut ExecuteContextFacade<'_, '_>,
    memory: crate::common::store::LocalMemoryId,
    ptr: u32,
    len: u32,
    data: u32,
) -> VMResult<()> {
    facade.gc_mut().local_fill_memory(memory, ptr, len, data)
}

#[inline(never)]
fn mem_fill_impl_shared_with_id(
    mut facade: &mut ExecuteContextFacade<'_, '_>,
    memory: crate::common::store::SharedMemoryId,
    ptr: u32,
    len: u32,
    data: u32,
) -> VMResult<()> {
    facade.gc_mut().shared_fill_memory(memory, ptr, len, data)
}

#[inline(never)]
fn mem_copy_impl_local_to_local(
    mut facade: &mut ExecuteContextFacade<'_, '_>,
    dst: crate::common::store::LocalMemoryId,
    src: crate::common::store::LocalMemoryId,
    dst_offset: u32,
    src_offset: u32,
    len: u32,
) -> VMResult<()> {
    facade.gc_mut()
        .copy_memory_local_to_local(dst, src, dst_offset, src_offset, len)
}

#[inline(never)]
fn mem_copy_impl_shared_to_local(
    mut facade: &mut ExecuteContextFacade<'_, '_>,
    dst: crate::common::store::LocalMemoryId,
    src: crate::common::store::SharedMemoryId,
    dst_offset: u32,
    src_offset: u32,
    len: u32,
) -> VMResult<()> {
    facade.gc_mut()
        .copy_memory_shared_to_local(dst, src, dst_offset, src_offset, len)
}

#[inline(never)]
fn mem_copy_impl_local_to_shared(
    mut facade: &mut ExecuteContextFacade<'_, '_>,
    dst: crate::common::store::SharedMemoryId,
    src: crate::common::store::LocalMemoryId,
    dst_offset: u32,
    src_offset: u32,
    len: u32,
) -> VMResult<()> {
    facade.gc_mut()
        .copy_memory_local_to_shared(dst, src, dst_offset, src_offset, len)
}

#[inline(never)]
fn mem_copy_impl_shared_to_shared(
    mut facade: &mut ExecuteContextFacade<'_, '_>,
    dst: crate::common::store::SharedMemoryId,
    src: crate::common::store::SharedMemoryId,
    dst_offset: u32,
    src_offset: u32,
    len: u32,
) -> VMResult<()> {
    facade.gc_mut()
        .copy_memory_shared_to_shared(dst, src, dst_offset, src_offset, len)
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
    let mut facade = ExecuteContextFacade::new(ctx);
    let idx = (*tail_code).operand.u32;
    let (dst, src, len) = pop_init_operands(&mut facade);
    vm_try!(mem_init_impl_local(&mut facade, idx, dst, src, len));
    call_next(tail_code, 1, facade.as_ctx_mut())
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
    let mut facade = ExecuteContextFacade::new(ctx);
    let idx = (*tail_code).operand.u32;
    let instance_id = facade.instance_id();
    data_drop_impl(&mut facade, instance_id, idx);
    call_next(tail_code, 1, facade.as_ctx_mut())
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
    let mut facade = ExecuteContextFacade::new(ctx);
    let (dst, src, len) = pop_copy_operands(&mut facade);
    let memory = facade.default_local_memory_id_unchecked();
    trace!("op_mem_copy src: {src},dst: {dst},len: {len}");
    vm_try!(mem_copy_impl_local_with_id(
        &mut facade,
        memory,
        dst,
        src,
        len,
    ));
    call_next(tail_code, 0, facade.as_ctx_mut())
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
    let mut facade = ExecuteContextFacade::new(ctx);
    let (ptr, data, len) = pop_fill_operands(&mut facade);
    let memory = facade.default_local_memory_id_unchecked();
    vm_try!(mem_fill_impl_local_with_id(
        &mut facade,
        memory,
        ptr,
        len,
        data,
    ));
    call_next(tail_code, 0, facade.as_ctx_mut())
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
    let mut facade = ExecuteContextFacade::new(ctx);
    let idx = (*tail_code).operand.u32;
    let (dst, src, len) = pop_init_operands(&mut facade);
    vm_try!(mem_init_impl_shared(&mut facade, idx, dst, src, len));
    call_next(tail_code, 1, facade.as_ctx_mut())
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
    let mut facade = ExecuteContextFacade::new(ctx);
    let (dst, src, len) = pop_copy_operands(&mut facade);
    let memory = facade.default_shared_memory_id_unchecked();
    trace!("op_mem_copy_shared src: {src},dst: {dst},len: {len}");
    vm_try!(mem_copy_impl_shared_with_id(
        &mut facade,
        memory,
        dst,
        src,
        len,
    ));
    call_next(tail_code, 0, facade.as_ctx_mut())
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
    let mut facade = ExecuteContextFacade::new(ctx);
    let (ptr, data, len) = pop_fill_operands(&mut facade);
    let memory = facade.default_shared_memory_id_unchecked();
    vm_try!(mem_fill_impl_shared_with_id(
        &mut facade,
        memory,
        ptr,
        len,
        data,
    ));
    call_next(tail_code, 0, facade.as_ctx_mut())
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
    let mut facade = ExecuteContextFacade::new(ctx);
    let idx = (*tail_code).operand.u32;
    let memidx = (*tail_code.add(1)).operand.u32;
    let (dst, src, len) = pop_init_operands(&mut facade);
    let memory = facade.local_memory_id_at_unchecked(memidx);
    vm_try!(mem_init_impl_local_with_id(
        &mut facade,
        memory,
        idx,
        dst,
        src,
        len,
    ));
    call_next(tail_code, 2, facade.as_ctx_mut())
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
    let mut facade = ExecuteContextFacade::new(ctx);
    let idx = (*tail_code).operand.u32;
    let memidx = (*tail_code.add(1)).operand.u32;
    let (dst, src, len) = pop_init_operands(&mut facade);
    let memory = facade.shared_memory_id_at_unchecked(memidx);
    vm_try!(mem_init_impl_shared_with_id(
        &mut facade,
        memory,
        idx,
        dst,
        src,
        len,
    ));
    call_next(tail_code, 2, facade.as_ctx_mut())
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
    let mut facade = ExecuteContextFacade::new(ctx);
    let dst_memidx = (*tail_code).operand.u32;
    let src_memidx = (*tail_code.add(1)).operand.u32;
    let (dst, src, len) = pop_copy_operands(&mut facade);
    let dst_memory = facade.local_memory_id_at_unchecked(dst_memidx);
    let src_memory = facade.local_memory_id_at_unchecked(src_memidx);
    vm_try!(mem_copy_impl_local_to_local(
        &mut facade,
        dst_memory,
        src_memory,
        dst,
        src,
        len,
    ));
    call_next(tail_code, 2, facade.as_ctx_mut())
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
    let mut facade = ExecuteContextFacade::new(ctx);
    let dst_memidx = (*tail_code).operand.u32;
    let src_memidx = (*tail_code.add(1)).operand.u32;
    let (dst, src, len) = pop_copy_operands(&mut facade);
    let dst_memory = facade.local_memory_id_at_unchecked(dst_memidx);
    let src_memory = facade.shared_memory_id_at_unchecked(src_memidx);
    vm_try!(mem_copy_impl_shared_to_local(
        &mut facade,
        dst_memory,
        src_memory,
        dst,
        src,
        len,
    ));
    call_next(tail_code, 2, facade.as_ctx_mut())
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
    let mut facade = ExecuteContextFacade::new(ctx);
    let dst_memidx = (*tail_code).operand.u32;
    let src_memidx = (*tail_code.add(1)).operand.u32;
    let (dst, src, len) = pop_copy_operands(&mut facade);
    let dst_memory = facade.shared_memory_id_at_unchecked(dst_memidx);
    let src_memory = facade.local_memory_id_at_unchecked(src_memidx);
    vm_try!(mem_copy_impl_local_to_shared(
        &mut facade,
        dst_memory,
        src_memory,
        dst,
        src,
        len,
    ));
    call_next(tail_code, 2, facade.as_ctx_mut())
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
    let mut facade = ExecuteContextFacade::new(ctx);
    let dst_memidx = (*tail_code).operand.u32;
    let src_memidx = (*tail_code.add(1)).operand.u32;
    let (dst, src, len) = pop_copy_operands(&mut facade);
    let dst_memory = facade.shared_memory_id_at_unchecked(dst_memidx);
    let src_memory = facade.shared_memory_id_at_unchecked(src_memidx);
    vm_try!(mem_copy_impl_shared_to_shared(
        &mut facade,
        dst_memory,
        src_memory,
        dst,
        src,
        len,
    ));
    call_next(tail_code, 2, facade.as_ctx_mut())
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
    let mut facade = ExecuteContextFacade::new(ctx);
    let memidx = (*tail_code).operand.u32;
    let (ptr, data, len) = pop_fill_operands(&mut facade);
    let memory = facade.local_memory_id_at_unchecked(memidx);
    vm_try!(mem_fill_impl_local_with_id(
        &mut facade,
        memory,
        ptr,
        len,
        data,
    ));
    call_next(tail_code, 1, facade.as_ctx_mut())
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
    let mut facade = ExecuteContextFacade::new(ctx);
    let memidx = (*tail_code).operand.u32;
    let (ptr, data, len) = pop_fill_operands(&mut facade);
    let memory = facade.shared_memory_id_at_unchecked(memidx);
    vm_try!(mem_fill_impl_shared_with_id(
        &mut facade,
        memory,
        ptr,
        len,
        data,
    ));
    call_next(tail_code, 1, facade.as_ctx_mut())
}

pub(crate) use op_mem_copy as op_mem_copy_local;
pub(crate) use op_mem_fill as op_mem_fill_local;
pub(crate) use op_mem_init as op_mem_init_local;
