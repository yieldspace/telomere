use super::*;
use vstd::prelude::*;

verus! {

pub open spec fn spec_table_get_result(
    table: crate::common::formal::TableView,
    idx: nat,
) -> Option<u32> {
    crate::common::formal::table_get_result(table, idx)
}

pub open spec fn spec_table_set_result(
    table: crate::common::formal::TableView,
    idx: nat,
    value: u32,
) -> Option<crate::common::formal::TableView> {
    crate::common::formal::table_set_result(table, idx, value)
}

pub open spec fn spec_table_size_result(table: crate::common::formal::TableView) -> nat {
    crate::common::formal::table_size_result(table)
}

pub open spec fn spec_table_grow_result(
    table: crate::common::formal::TableView,
    count: nat,
    value: u32,
) -> (crate::common::formal::TableView, int) {
    crate::common::formal::table_grow_result(table, count, value)
}

#[inline(always)]
fn table_grow_failure() -> (result: i32)
    ensures
        result == -1,
{
    -1
}

} // verus!

#[inline(always)]
unsafe fn table_addr(facade: &ExecuteContextFacade<'_, '_>, table_idx: usize) -> GcRef {
    facade.instance().tables.as_slice()[table_idx]
}

#[inline(always)]
unsafe fn table_get_impl(mut facade: &mut ExecuteContextFacade<'_, '_>, table_idx: usize, i: u32) -> VMResult<u32> {
    let addr = table_addr(&mut facade, table_idx);
    let inst = facade.gc_mut().get_table(addr);
    if i as usize >= inst.1.len() {
        return VMResult::TableIndexOutOfRange;
    }
    let value = inst.1[i as usize];
    trace!("op_table_get: {table_idx} {addr:?} {i} {value}");
    VMResult::Success(value)
}

#[inline(always)]
unsafe fn table_set_impl(
    mut facade: &mut ExecuteContextFacade<'_, '_>,
    table_idx: usize,
    i: u32,
    value: u32,
) -> VMResult<()> {
    let addr = table_addr(&mut facade, table_idx);
    let inst = &mut facade.gc_mut().get_table(addr);
    trace!("op_table_set: {table_idx} {addr:?} {i} {value}");
    if i as usize >= inst.1.len() {
        return VMResult::TableIndexOutOfRange;
    }
    inst.1[i as usize] = value;
    VMResult::Success(())
}

#[inline(never)]
unsafe fn table_copy_impl(
    mut facade: &mut ExecuteContextFacade<'_, '_>,
    dst_table_idx: usize,
    src_table_idx: usize,
    dst: usize,
    src: usize,
    len: usize,
) -> VMResult<()> {
    let src_table_addr = table_addr(&mut facade, src_table_idx);
    let dst_table_addr = table_addr(&mut facade, dst_table_idx);
    let src_values = {
        let src_table = &facade.gc_mut().get_table(src_table_addr).1;
        vm_try!(VMResult::from_option(src_table.get(src..src + len), || {
            VMResult::TableIndexOutOfRange
        }))
        .to_vec()
    };
    let dst_table = &mut facade.gc_mut().get_table(dst_table_addr).1;
    let dst_slice = vm_try!(VMResult::from_option(
        dst_table.get_mut(dst..dst + len),
        || { VMResult::TableIndexOutOfRange }
    ));
    dst_slice.copy_from_slice(&src_values);
    VMResult::Success(())
}

#[inline(always)]
unsafe fn table_grow_impl(
    mut facade: &mut ExecuteContextFacade<'_, '_>,
    table_idx: usize,
    n: i32,
    val: u32,
) -> VMResult<u32> {
    let table_addr = table_addr(&mut facade, table_idx);
    let table_inst = &mut facade.gc_mut().get_table(table_addr);
    let sz = table_inst.1.len();
    if n < 0 {
        return VMResult::Success(table_grow_failure() as u32);
    }
    let new_len = sz + n as usize;
    match table_inst.0.limits.max {
        Some(max) if max as usize >= new_len => {
            table_inst.1.resize(new_len, val);
            VMResult::Success(sz as u32)
        }
        None => {
            table_inst.1.resize(new_len, val);
            VMResult::Success(sz as u32)
        }
        Some(_) => VMResult::Success(table_grow_failure() as u32),
    }
}

#[inline(always)]
unsafe fn table_size_impl(mut facade: &mut ExecuteContextFacade<'_, '_>, table_idx: usize) -> u32 {
    let table_addr = table_addr(&mut facade, table_idx);
    let table_inst = facade.gc_mut().get_table(table_addr);
    let value = table_inst.1.len() as u32;
    trace!("op_table_size: {table_idx} {table_addr:?} {table_inst:?} => {value}");
    value
}

#[inline(always)]
unsafe fn table_fill_impl(
    mut facade: &mut ExecuteContextFacade<'_, '_>,
    table_idx: usize,
    i: usize,
    val: u32,
    n: usize,
) -> VMResult<()> {
    let table_addr = table_addr(&mut facade, table_idx);
    let table = &mut facade.gc_mut().get_table(table_addr).1;
    let slice = vm_try!(VMResult::from_option(table.get_mut(i..i + n), || {
        VMResult::TableIndexOutOfRange
    }));
    slice.fill(val);
    VMResult::Success(())
}

#[inline(always)]
unsafe fn pop_table_access_index(mut facade: &mut ExecuteContextFacade<'_, '_>) -> u32 {
    facade.stack_mut().pop_u32()
}

#[inline(always)]
unsafe fn pop_table_set_operands(mut facade: &mut ExecuteContextFacade<'_, '_>) -> (u32, u32) {
    let value = facade.stack_mut().pop_u32();
    let index = facade.stack_mut().pop_u32();
    (index, value)
}

#[inline(always)]
unsafe fn pop_table_range_operands(mut facade: &mut ExecuteContextFacade<'_, '_>) -> (usize, usize, usize) {
    let len = facade.stack_mut().pop_u32() as usize;
    let src = facade.stack_mut().pop_u32() as usize;
    let dst = facade.stack_mut().pop_u32() as usize;
    (dst, src, len)
}

#[inline(always)]
unsafe fn pop_table_fill_operands(mut facade: &mut ExecuteContextFacade<'_, '_>) -> (usize, u32, usize) {
    let len = facade.stack_mut().pop_u32() as usize;
    let value = facade.stack_mut().pop_u32();
    let index = facade.stack_mut().pop_u32() as usize;
    (index, value, len)
}

#[inline(always)]
unsafe fn pop_table_grow_operands(mut facade: &mut ExecuteContextFacade<'_, '_>) -> (u32, i32) {
    let count = facade.stack_mut().pop_i32();
    let value = facade.stack_mut().pop_u32();
    (value, count)
}

#[inline(always)]
unsafe fn push_table_result(
    tail_code: *const Instr,
    mut facade: &mut ExecuteContextFacade<'_, '_>,
    skip: isize,
    value: u32,
) -> VMResult<()> {
    vm_try!(facade.stack_mut().push_u32(value));
    call_next(tail_code, skip, facade.as_ctx_mut())
}

/// WebAssembly `table.get`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[i32] -> [ref]`.
/// Traps: traps on out-of-bounds table access or type mismatch.
/// Notes: Accesses the instance table storage and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_table_get(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let mut facade = ExecuteContextFacade::new(ctx);
    let idx = (*tail_code).operand.u32 as usize;
    let i = pop_table_access_index(&mut facade);
    let val = vm_try!(table_get_impl(&mut facade, idx, i));
    push_table_result(tail_code, &mut facade, 1, val)
}

/// WebAssembly `table.set`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[i32, ref] -> []`.
/// Traps: traps on out-of-bounds table access or type mismatch.
/// Notes: Accesses the instance table storage and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_table_set(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let mut facade = ExecuteContextFacade::new(ctx);
    let idx = (*tail_code).operand.u32 as usize;
    let (i, val) = pop_table_set_operands(&mut facade);
    vm_try!(table_set_impl(&mut facade, idx, i, val));
    call_next(tail_code, 1, facade.as_ctx_mut())
}

#[inline(never)]
/// WebAssembly bulk-memory `table.init` helper.
///
/// Spec:
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: internal `table.init` operand handling.
/// Traps: traps on table bounds violations or invalid element segment access.
/// Notes: Resolves the destination table and source element segment before copying the validated payload.
///
/// # Safety
/// - `ctx` must reference a live execution context whose table and element metadata are still valid for the current frame.
/// - `src_elem_idx`, `dst_table_idx`, `dst_pos`, `src`, and `len` must have already passed the instruction-level validation performed by the caller.
/// - This helper must not keep borrows, locks, or guards alive across any follow-up tail-dispatch.
unsafe fn table_init_impl(
    mut facade: &mut ExecuteContextFacade<'_, '_>,
    src_elem_idx: u32,
    dst_table_idx: usize,
    dst_pos: usize,
    src: usize,
    len: usize,
) -> VMResult<()> {
    let instance_addr = facade.instance_addr();
    let (instance_id, dst_table_addr, reftype, globals, funcs, func_addrs) = {
        let gc = facade.gc_mut();
        let instance = unsafe { &*gc.get_instance_unchecked(instance_addr) };
        let dst_table_addr = instance.tables.as_slice()[dst_table_idx];
        let reftype = gc.get_table(dst_table_addr).0.reftype;
        let globals = instance.globals.as_slice().to_vec();
        let funcs = instance.funcs.as_slice().to_vec();
        let func_addrs = funcs.iter().map(|it| it.get()).collect::<Vec<_>>();
        (
            instance.instance_id,
            dst_table_addr,
            reftype,
            globals,
            funcs,
            func_addrs,
        )
    };
    let dst_table_len = {
        let dst_table = facade.gc_mut().get_table(dst_table_addr);
        dst_table.1.len()
    };
    if dst_pos.checked_add(len).is_none() || dst_pos + len > dst_table_len {
        return VMResult::TableIndexOutOfRange;
    }
    let elem_init = {
        let segments = facade.store_ref().lock_segments();
        let Some(elem) = segments.elems.get(&(instance_id, src_elem_idx)) else {
            return if len == 0 && src == 0 {
                VMResult::Success(())
            } else {
                VMResult::TableIndexOutOfRange
            };
        };
        elem.init.clone()
    };
    match elem_init {
        ElemInit::FuncIdx(idxs) => {
            let slice = vm_try!(VMResult::from_option(idxs.get(src..(src + len)), || {
                VMResult::TableIndexOutOfRange
            }));
            let values = slice
                .iter()
                .map(|funcidx| func_addrs[*funcidx as usize])
                .collect::<Vec<_>>();
            let dst_table = facade.gc_mut().get_table(dst_table_addr);
            let dst = vm_try!(VMResult::from_option(
                dst_table.1.get_mut(dst_pos..dst_pos + len),
                || { VMResult::TableIndexOutOfRange }
            ));
            dst.copy_from_slice(&values);
        }
        ElemInit::ConstExpr(exprs) => {
            let slice = vm_try!(VMResult::from_option(exprs.get(src..(src + len)), || {
                VMResult::TableIndexOutOfRange
            }));
            let values = {
                let gc = facade.gc_mut();
                let mut values = Vec::with_capacity(slice.len());
                for expr in slice.iter() {
                    let res = vm_try!(execute_elem_init_const_expr(
                        gc,
                        globals.as_slice(),
                        funcs.as_slice(),
                        expr,
                        reftype,
                    ));
                    values.push(res.get());
                }
                values
            };
            let dst_table = facade.gc_mut().get_table(dst_table_addr);
            let dst = vm_try!(VMResult::from_option(
                dst_table.1.get_mut(dst_pos..dst_pos + len),
                || { VMResult::TableIndexOutOfRange }
            ));
            dst.copy_from_slice(&values);
        }
    }
    VMResult::Success(())
}

/// WebAssembly `table.init`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[dst, src, len] -> []`.
/// Traps: traps on out-of-bounds table access or type mismatch.
/// Notes: Accesses the instance table storage and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_table_init(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let mut facade = ExecuteContextFacade::new(ctx);
    let (dst_pos, src, len) = pop_table_range_operands(&mut facade);
    let src_elem_idx = (*tail_code).operand.u32;
    let dst_table_idx = (*tail_code.offset(1)).operand.u32 as usize;
    vm_try!(table_init_impl(
        &mut facade,
        src_elem_idx,
        dst_table_idx,
        dst_pos,
        src,
        len,
    ));
    call_next(tail_code, 2, facade.as_ctx_mut())
}

#[inline(never)]
fn elem_drop_impl(mut facade: &mut ExecuteContextFacade<'_, '_>, instance_id: u32, elem_idx: u32) {
    let _ = facade
        .store_ref()
        .lock_segments()
        .elems
        .remove(&(instance_id, elem_idx));
}

/// WebAssembly `elem.drop`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[] -> []`.
/// Traps: traps on out-of-bounds table access or type mismatch.
/// Notes: Accesses the instance table storage and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_elem_drop(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let mut facade = ExecuteContextFacade::new(ctx);
    let elem_idx = (*tail_code).operand.u32;
    let instance_id = facade.instance_id();
    elem_drop_impl(&mut facade, instance_id, elem_idx);
    call_next(tail_code, 1, facade.as_ctx_mut())
}

/// WebAssembly `table.copy`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[dst, src, len] -> []`.
/// Traps: traps on out-of-bounds table access or type mismatch.
/// Notes: Accesses the instance table storage and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_table_copy(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let mut facade = ExecuteContextFacade::new(ctx);
    let (dst, src, len) = pop_table_range_operands(&mut facade);
    let dst_table_idx = (*tail_code).operand.u32 as usize;
    let src_table_idx = (*tail_code.offset(1)).operand.u32 as usize;
    vm_try!(table_copy_impl(
        &mut facade,
        dst_table_idx,
        src_table_idx,
        dst,
        src,
        len
    ));
    call_next(tail_code, 2, facade.as_ctx_mut())
}

/// WebAssembly `table.grow`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[ref, delta] -> [i32]`.
/// Traps: traps on out-of-bounds table access or type mismatch.
/// Notes: Accesses the instance table storage and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_table_grow(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let mut facade = ExecuteContextFacade::new(ctx);
    let table_idx = (*tail_code).operand.u32 as usize;
    let (val, n) = pop_table_grow_operands(&mut facade);
    let result = vm_try!(table_grow_impl(&mut facade, table_idx, n, val));
    push_table_result(tail_code, &mut facade, 1, result)
}

/// WebAssembly `table.size`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[] -> [i32]`.
/// Traps: traps on out-of-bounds table access or type mismatch.
/// Notes: Accesses the instance table storage and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_table_size(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let mut facade = ExecuteContextFacade::new(ctx);
    let table_idx = (*tail_code).operand.u32 as usize;
    let val = table_size_impl(&mut facade, table_idx);
    push_table_result(tail_code, &mut facade, 1, val)
}

/// WebAssembly `table.fill`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[dst, ref, len] -> []`.
/// Traps: traps on out-of-bounds table access or type mismatch.
/// Notes: Accesses the instance table storage and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_table_fill(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let mut facade = ExecuteContextFacade::new(ctx);
    let (i, val, n) = pop_table_fill_operands(&mut facade);
    let table_idx = (*tail_code).operand.u32 as usize;
    vm_try!(table_fill_impl(&mut facade, table_idx, i, val, n));
    call_next(tail_code, 1, facade.as_ctx_mut())
}
