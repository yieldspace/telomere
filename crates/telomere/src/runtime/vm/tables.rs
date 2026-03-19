#![allow(clippy::missing_safety_doc)]

use super::*;
use vstd::prelude::*;

verus! {

#[inline(always)]
fn table_grow_failure() -> (result: i32)
    ensures
        result == -1,
{
    -1
}

} // verus!

pub unsafe fn op_table_get(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let idx = (*tail_code).operand.u32 as usize;
    let addr = ctx.instance().tables.as_slice()[idx];
    let inst = ctx.gc.get_table(addr);
    let i = ctx.stack.pop_u32();
    if i as usize >= inst.1.len() {
        return VMResult::TableIndexOutOfRange;
    }
    let val = inst.1[i as usize];
    trace!("op_table_get: {idx} {addr:?} {i} {val}");

    vm_try!(ctx.stack.push_u32(val));
    call_next(tail_code, 1, ctx)
}

pub unsafe fn op_table_set(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let idx = (*tail_code).operand.u32 as usize;
    let addr = ctx.instance().tables.as_slice()[idx];
    let inst = &mut ctx.gc.get_table(addr);
    let val = ctx.stack.pop_u32();
    let i = ctx.stack.pop_u32();
    trace!("op_table_set: {idx} {addr:?} {i} {val}");

    if i as usize >= inst.1.len() {
        return VMResult::TableIndexOutOfRange;
    }
    inst.1[i as usize] = val;
    call_next(tail_code, 1, ctx)
}

#[inline(never)]
unsafe fn table_init_impl(
    ctx: &mut ExecuteContext,
    src_elem_idx: u32,
    dst_table_idx: usize,
    dst_pos: usize,
    src: usize,
    len: usize,
) -> VMResult<()> {
    let instance_addr = ctx.instance_addr();
    let ExecuteContext { store, gc, .. } = ctx;
    let instance = unsafe { &*gc.get_instance_unchecked(instance_addr) };
    let dst_table_addr = instance.tables.as_slice()[dst_table_idx];
    let segments = store.lock_segments();
    let dst_table_len = {
        let dst_table = gc.get_table(dst_table_addr);
        dst_table.1.len()
    };
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
    let reftype = {
        let dst_table = gc.get_table(dst_table_addr);
        dst_table.0.reftype
    };
    match &elem.init {
        ElemInit::FuncIdx(idxs) => {
            let slice = vm_try!(VMResult::from_option(idxs.get(src..(src + len)), || {
                VMResult::TableIndexOutOfRange
            }));
            let func_addrs = instance
                .funcs
                .as_slice()
                .iter()
                .map(|it| it.get())
                .collect::<Vec<_>>();
            let dst_table = gc.get_table(dst_table_addr);
            let dst = vm_try!(VMResult::from_option(
                dst_table.1.get_mut(dst_pos..dst_pos + len),
                || { VMResult::TableIndexOutOfRange }
            ));
            for (i, funcidx) in slice.iter().enumerate() {
                dst[i] = func_addrs[*funcidx as usize];
            }
        }
        ElemInit::ConstExpr(exprs) => {
            let slice = vm_try!(VMResult::from_option(exprs.get(src..(src + len)), || {
                VMResult::TableIndexOutOfRange
            }));
            for (i, expr) in slice.iter().enumerate() {
                let res = vm_try!(execute_elem_init_const_expr(
                    gc,
                    instance.globals.as_slice(),
                    instance.funcs.as_slice(),
                    expr,
                    reftype,
                ));
                let dst_table = gc.get_table(dst_table_addr);
                let dst = vm_try!(VMResult::from_option(
                    dst_table.1.get_mut(dst_pos..dst_pos + len),
                    || { VMResult::TableIndexOutOfRange }
                ));
                dst[i] = res.get();
            }
        }
    }
    VMResult::Success(())
}

pub unsafe fn op_table_init(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let len = ctx.stack.pop_u32() as usize;
    let src = ctx.stack.pop_u32() as usize;
    let dst_pos = ctx.stack.pop_u32() as usize;
    let src_elem_idx = (*tail_code).operand.u32;
    let dst_table_idx = (*tail_code.offset(1)).operand.u32 as usize;
    vm_try!(table_init_impl(
        ctx,
        src_elem_idx,
        dst_table_idx,
        dst_pos,
        src,
        len,
    ));
    call_next(tail_code, 2, ctx)
}

#[inline(never)]
fn elem_drop_impl(ctx: &mut ExecuteContext, instance_id: u32, elem_idx: u32) {
    let _ = ctx
        .store
        .lock_segments()
        .elems
        .remove(&(instance_id, elem_idx));
}

pub unsafe fn op_elem_drop(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let elem_idx = (*tail_code).operand.u32;
    let instance_id = ctx.instance_id();
    elem_drop_impl(ctx, instance_id, elem_idx);
    call_next(tail_code, 1, ctx)
}

pub unsafe fn op_table_copy(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let len = ctx.stack.pop_u32() as usize;
    let src = ctx.stack.pop_u32() as usize;
    let dst = ctx.stack.pop_u32() as usize;
    let dst_table_idx = (*tail_code).operand.u32 as usize;
    let src_table_idx = (*tail_code.offset(1)).operand.u32 as usize;

    let src_table_addr = ctx.instance().tables.as_slice()[src_table_idx];
    let dst_table_addr = ctx.instance().tables.as_slice()[dst_table_idx];
    let src_table = &ctx.gc.get_table(src_table_addr).1;
    let src_ptr = vm_try!(VMResult::from_option(src_table.get(src..src + len), || {
        VMResult::TableIndexOutOfRange
    }))
    .as_ptr();
    let dst_table = &mut ctx.gc.get_table(dst_table_addr).1;
    let dst_ptr = vm_try!(VMResult::from_option(
        dst_table.get_mut(dst..dst + len),
        || { VMResult::TableIndexOutOfRange }
    ))
    .as_mut_ptr();
    std::ptr::copy(src_ptr, dst_ptr, len);
    call_next(tail_code, 2, ctx)
}

pub unsafe fn op_table_grow(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let table_idx = (*tail_code).operand.u32 as usize;
    let table_addr = ctx.instance().tables.as_slice()[table_idx];
    let table_inst = &mut ctx.gc.get_table(table_addr);
    let n = ctx.stack.pop_i32();
    let val = ctx.stack.pop_u32();
    let sz = table_inst.1.len();
    if n < 0 {
        vm_try!(ctx.stack.push_i32(table_grow_failure()));
    } else {
        let new_len = sz + n as usize;
        match table_inst.0.limits.max {
            Some(max) if max as usize >= new_len => {
                table_inst.1.resize(new_len, val);
                vm_try!(ctx.stack.push_u32(sz as u32));
            }
            None => {
                table_inst.1.resize(new_len, val);
                vm_try!(ctx.stack.push_u32(sz as u32));
            }
            Some(_) => {
                vm_try!(ctx.stack.push_i32(table_grow_failure()));
            }
        }
    }
    call_next(tail_code, 1, ctx)
}

pub unsafe fn op_table_size(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let table_idx = (*tail_code).operand.u32 as usize;
    let table_addr = ctx.instance().tables.as_slice()[table_idx];
    let table_inst = &mut ctx.gc.get_table(table_addr);
    let val = table_inst.1.len() as u32;
    trace!("op_table_size: {table_idx} {table_addr:?} {table_inst:?} => {val}");
    vm_try!(ctx.stack.push_u32(val));
    call_next(tail_code, 1, ctx)
}

pub unsafe fn op_table_fill(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let n = ctx.stack.pop_u32() as usize;
    let val = ctx.stack.pop_u32();
    let i = ctx.stack.pop_u32() as usize;
    let table_idx = (*tail_code).operand.u32 as usize;

    let table_addr = ctx.instance().tables.as_slice()[table_idx];
    let table = &mut ctx.gc.get_table(table_addr).1;
    let slice = vm_try!(VMResult::from_option(table.get_mut(i..i + n), || {
        VMResult::TableIndexOutOfRange
    }));
    slice.fill(val);
    call_next(tail_code, 1, ctx)
}
