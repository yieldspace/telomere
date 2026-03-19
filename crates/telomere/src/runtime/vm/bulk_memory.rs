#![allow(clippy::missing_safety_doc)]

use super::*;

#[inline(never)]
fn mem_init_impl(ctx: &mut ExecuteContext, idx: u32, dst: u32, src: u32, len: u32) -> VMResult<()> {
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
    ctx.write_memory_bytes(dst as usize, copied.as_deref().unwrap_or(&[]))
}

#[inline(never)]
fn data_drop_impl(ctx: &mut ExecuteContext, instance_id: u32, idx: u32) {
    let _ = ctx.store.lock_segments().data.remove(&(instance_id, idx));
}

pub unsafe fn op_mem_init(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let idx = (*tail_code).operand.u32;
    let len = ctx.stack.pop_u32();
    let src = ctx.stack.pop_u32();
    let dst = ctx.stack.pop_u32();
    vm_try!(mem_init_impl(ctx, idx, dst, src, len));
    call_next(tail_code, 1, ctx)
}

pub unsafe fn op_data_drop(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    if wait_effect(ctx, ctx.cont) {
        return VMResult::Success(());
    }
    let idx = (*tail_code).operand.u32;
    let instance_id = ctx.instance_id();
    data_drop_impl(ctx, instance_id, idx);
    call_next(tail_code, 1, ctx)
}

pub unsafe fn op_mem_copy(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    if wait_effect(ctx, ctx.cont) {
        return VMResult::Success(());
    }
    let len = ctx.stack.pop_u32();
    let src = ctx.stack.pop_u32();
    let dst = ctx.stack.pop_u32();
    trace!("op_mem_copy src: {src},dst: {dst},len: {len}");
    vm_try!(ctx.copy_memory(dst, src, len));
    call_next(tail_code, 0, ctx)
}

pub unsafe fn op_mem_fill(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let len = ctx.stack.pop_u32();
    let data = ctx.stack.pop_u32();
    let ptr = ctx.stack.pop_u32();
    vm_try!(ctx.fill_memory(ptr, len, data));
    call_next(tail_code, 0, ctx)
}
