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

} // verus!

#[inline(always)]
unsafe fn table_addr(facade: &ExecuteContextFacade<'_, '_>, table_idx: usize) -> GcRef {
    facade.table_addr(table_idx)
}

#[inline(always)]
unsafe fn table_get_impl(
    facade: &mut ExecuteContextFacade<'_, '_>,
    table_idx: usize,
    i: u32,
) -> VMResult<u32> {
    let addr = table_addr(facade, table_idx);
    let value = vm_try!(VMResult::from_option(
        facade.table_get_value(addr, i as usize),
        || { VMResult::TableIndexOutOfRange }
    ));
    trace!("op_table_get: {table_idx} {addr:?} {i} {value}");
    VMResult::Success(value)
}

#[inline(always)]
unsafe fn table_set_impl(
    facade: &mut ExecuteContextFacade<'_, '_>,
    table_idx: usize,
    i: u32,
    value: u32,
) -> VMResult<()> {
    let addr = table_addr(facade, table_idx);
    trace!("op_table_set: {table_idx} {addr:?} {i} {value}");
    facade.table_set_value(addr, i as usize, value)
}

#[inline(never)]
unsafe fn table_copy_impl(
    facade: &mut ExecuteContextFacade<'_, '_>,
    dst_table_idx: usize,
    src_table_idx: usize,
    dst: usize,
    src: usize,
    len: usize,
) -> VMResult<()> {
    let src_table_addr = table_addr(facade, src_table_idx);
    let dst_table_addr = table_addr(facade, dst_table_idx);
    facade.table_copy(dst_table_addr, src_table_addr, dst, src, len)
}

#[inline(always)]
unsafe fn table_grow_impl(
    facade: &mut ExecuteContextFacade<'_, '_>,
    table_idx: usize,
    n: i32,
    val: u32,
) -> VMResult<u32> {
    let table_addr = table_addr(facade, table_idx);
    VMResult::Success(facade.table_grow(table_addr, n, val))
}

#[inline(always)]
unsafe fn table_size_impl(facade: &mut ExecuteContextFacade<'_, '_>, table_idx: usize) -> u32 {
    let table_addr = table_addr(facade, table_idx);
    let value = facade.table_len(table_addr) as u32;
    trace!("op_table_size: {table_idx} {table_addr:?} => {value}");
    value
}

#[inline(always)]
unsafe fn table_fill_impl(
    facade: &mut ExecuteContextFacade<'_, '_>,
    table_idx: usize,
    i: usize,
    val: u32,
    n: usize,
) -> VMResult<()> {
    let table_addr = table_addr(facade, table_idx);
    facade.table_fill(table_addr, i, n, val)
}

#[inline(always)]
unsafe fn pop_table_access_index(facade: &mut ExecuteContextFacade<'_, '_>) -> u32 {
    facade.pop_u32()
}

#[inline(always)]
unsafe fn pop_table_set_operands(facade: &mut ExecuteContextFacade<'_, '_>) -> (u32, u32) {
    let value = facade.pop_u32();
    let index = facade.pop_u32();
    (index, value)
}

#[inline(always)]
unsafe fn pop_table_range_operands(
    facade: &mut ExecuteContextFacade<'_, '_>,
) -> (usize, usize, usize) {
    let len = facade.pop_u32() as usize;
    let src = facade.pop_u32() as usize;
    let dst = facade.pop_u32() as usize;
    (dst, src, len)
}

#[inline(always)]
unsafe fn pop_table_fill_operands(
    facade: &mut ExecuteContextFacade<'_, '_>,
) -> (usize, u32, usize) {
    let len = facade.pop_u32() as usize;
    let value = facade.pop_u32();
    let index = facade.pop_u32() as usize;
    (index, value, len)
}

#[inline(always)]
unsafe fn pop_table_grow_operands(facade: &mut ExecuteContextFacade<'_, '_>) -> (u32, i32) {
    let count = facade.pop_i32();
    let value = facade.pop_u32();
    (value, count)
}

#[inline(always)]
unsafe fn push_table_result(
    tail_code: *const Instr,
    facade: &mut ExecuteContextFacade<'_, '_>,
    skip: isize,
    value: u32,
) -> VMResult<()> {
    vm_try!(facade.push_u32(value));
    facade_call_next(tail_code, skip, facade)
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
    facade_call_next(tail_code, 1, &mut facade)
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
    facade: &mut ExecuteContextFacade<'_, '_>,
    src_elem_idx: u32,
    dst_table_idx: usize,
    dst_pos: usize,
    src: usize,
    len: usize,
) -> VMResult<()> {
    let dst_table_addr = facade.table_addr(dst_table_idx);
    let reftype = facade.table_reftype(dst_table_addr);
    let globals = facade.instance_globals_snapshot();
    let funcs = facade.instance_funcs_snapshot();
    let func_addrs = facade.instance_func_addrs_snapshot();
    let dst_table_len = facade.table_len(dst_table_addr);
    if dst_pos.checked_add(len).is_none() || dst_pos + len > dst_table_len {
        return VMResult::TableIndexOutOfRange;
    }
    let Some(elem_init) = facade.elem_init(src_elem_idx) else {
        return if len == 0 && src == 0 {
            VMResult::Success(())
        } else {
            VMResult::TableIndexOutOfRange
        };
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
            vm_try!(facade.table_write_slice(dst_table_addr, dst_pos, &values));
        }
        ElemInit::ConstExpr(exprs) => {
            let slice = vm_try!(VMResult::from_option(exprs.get(src..(src + len)), || {
                VMResult::TableIndexOutOfRange
            }));
            let values = vm_try!(facade.eval_elem_init_exprs(
                slice,
                globals.as_slice(),
                funcs.as_slice(),
                reftype,
            ));
            vm_try!(facade.table_write_slice(dst_table_addr, dst_pos, &values));
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
    facade_call_next(tail_code, 2, &mut facade)
}

#[inline(never)]
fn elem_drop_impl(facade: &ExecuteContextFacade<'_, '_>, elem_idx: u32) {
    facade.drop_elem_segment(elem_idx);
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
    elem_drop_impl(&facade, elem_idx);
    facade_call_next(tail_code, 1, &mut facade)
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
    facade_call_next(tail_code, 2, &mut facade)
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
    facade_call_next(tail_code, 1, &mut facade)
}
