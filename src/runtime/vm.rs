use std::ops::Rem;

use crate::{
    common::{
        ElemMode, ExecuteContext, ExportDesc, FuncIdx, Instance, Instr, JumpTable, LocalState,
        Memory, Stack, TableInstance, TypeIdx, VMError, ValType, WasmValue, PAGE_SIZE,
        PAGE_SIZE_MAX,
    },
    parser::Module,
};
pub struct ResultValue(Vec<WasmValue>);
impl ResultValue {
    pub fn new(args: Vec<WasmValue>) -> Self {
        Self(args)
    }
    pub fn iter(&self) -> impl Iterator<Item = &WasmValue> + use<'_> {
        self.0.iter()
    }
}

#[inline(always)]
unsafe fn call_next(
    tail_code: *const Instr,
    consumed: isize,
    ctx: &mut ExecuteContext,
) -> Result<u32, VMError> {
    ((*tail_code.offset(consumed)).op)(tail_code.offset(consumed + 1), ctx)
}
pub unsafe fn op_i32_const(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> Result<u32, VMError> {
    let v = (*tail_code).operand.i32;
    trace!("op_i32_const: {v}");
    ctx.stack.push_i32(v);
    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_i32_add(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> Result<u32, VMError> {
    trace!("op_i32_add");
    let a = ctx.stack.pop_i32();
    let b = ctx.stack.pop_i32();
    ctx.stack.push_i32(a + b);
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i32_sub(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> Result<u32, VMError> {
    let a = ctx.stack.pop_i32();
    let b = ctx.stack.pop_i32();
    let r = b - a;
    ctx.stack.push_i32(r);

    trace!("op_i32_sub: {a} {b} {r}");

    call_next(tail_code, 0, ctx)
}

pub unsafe fn op_i64_sub(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> Result<u32, VMError> {
    let a = ctx.stack.pop_i64();
    let b = ctx.stack.pop_i64();
    let r = b - a;
    ctx.stack.push_i64(r);

    trace!("op_i64_sub: {a} {b} {r}");

    call_next(tail_code, 0, ctx)
}

pub unsafe fn op_i64_const(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> Result<u32, VMError> {
    trace!("op_i64_const");
    ctx.stack.push_i64((*tail_code).operand.i64);
    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_f32_const(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> Result<u32, VMError> {
    trace!("op_f32_const");
    ctx.stack.push_f32((*tail_code).operand.f32);
    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_f64_const(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> Result<u32, VMError> {
    trace!("op_f64_const");
    ctx.stack.push_f64((*tail_code).operand.f64);
    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_f32_gt(tail_code: *const Instr, ctx: &mut ExecuteContext) -> Result<u32, VMError> {
    trace!("op_f32_gt");
    let a = ctx.stack.pop_f32();
    let b = ctx.stack.pop_f32();
    ctx.stack.push_u32(if a < b { 1 } else { 0 });

    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_f32_sqrt(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> Result<u32, VMError> {
    trace!("op_f32_sqrt");
    let a = ctx.stack.pop_f32();
    ctx.stack.push_f32(a.sqrt());

    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_f32_add(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> Result<u32, VMError> {
    trace!("op_f32_add");
    let b = ctx.stack.pop_f32();
    let a = ctx.stack.pop_f32();

    ctx.stack.push_f32(a + b);

    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_f32_sub(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> Result<u32, VMError> {
    trace!("op_f32_sub");
    let a = ctx.stack.pop_f32();

    let b = ctx.stack.pop_f32();

    ctx.stack.push_f32(b - a);

    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_f32_mul(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> Result<u32, VMError> {
    trace!("op_f32_mul");
    let b = ctx.stack.pop_f32();
    let a = ctx.stack.pop_f32();

    ctx.stack.push_f32(a * b);

    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_f64_add(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> Result<u32, VMError> {
    trace!("op_f64_add");
    let b = ctx.stack.pop_f64();
    let a = ctx.stack.pop_f64();

    ctx.stack.push_f64(a + b);

    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_f64_sub(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> Result<u32, VMError> {
    trace!("op_f64_sub");
    let a = ctx.stack.pop_f64();

    let b = ctx.stack.pop_f64();

    ctx.stack.push_f64(b - a);

    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_f64_mul(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> Result<u32, VMError> {
    trace!("op_f64_mul");
    let b = ctx.stack.pop_f64();
    let a = ctx.stack.pop_f64();

    ctx.stack.push_f64(a * b);

    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i64_add(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> Result<u32, VMError> {
    let a = ctx.stack.pop_i64();
    let b = ctx.stack.pop_i64();
    ctx.stack.push_i64(a + b);
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i64_extend_i32_s(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> Result<u32, VMError> {
    let a = ctx.stack.pop_i32();
    ctx.stack.push_i64(a.into());
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_return(
    _tail_code: *const Instr,
    _ctx: &mut ExecuteContext,
) -> Result<u32, VMError> {
    Ok(0)
}

pub unsafe fn op_end(tail_code: *const Instr, ctx: &mut ExecuteContext) -> Result<u32, VMError> {
    ctx.jump_table().end();
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_br(tail_code: *const Instr, ctx: &mut ExecuteContext) -> Result<u32, VMError> {
    let addr = ctx
        .jump_table()
        .br((*tail_code).operand.u32 as usize)
        .unwrap_unchecked();
    let tail_code = ctx.code().offset(addr as isize);
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_else(_tail_code: *const Instr, ctx: &mut ExecuteContext) -> Result<u32, VMError> {
    trace!("op_else");

    let addr = ctx.jump_table().br(0).unwrap_unchecked();
    let tail_code = ctx.code().offset(addr as isize);
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_br_if(tail_code: *const Instr, ctx: &mut ExecuteContext) -> Result<u32, VMError> {
    trace!("op_br_if");
    let cond = ctx.stack.pop_u32();
    let ptr = if cond != 0 {
        let addr = ctx
            .jump_table()
            .br((*tail_code).operand.u32 as usize)
            .unwrap_unchecked();
        let tail_code = ctx.code().offset(addr as isize);
        tail_code
    } else {
        tail_code.offset(1)
    };
    call_next(ptr, 0, ctx)
}
pub unsafe fn op_br_table(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> Result<u32, VMError> {
    let index = ctx.stack.pop_u32();
    let table_size = (*tail_code).operand.u32;
    let idx = if index < table_size {
        (*tail_code.offset((index + 1) as isize)).operand.u32
    } else {
        (*tail_code.offset((table_size + 1) as isize)).operand.u32
    };
    let addr = ctx.jump_table().br(idx as usize).unwrap_unchecked();
    let tail_code = ctx.code().offset(addr as isize);
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_block(tail_code: *const Instr, ctx: &mut ExecuteContext) -> Result<u32, VMError> {
    trace!("op_block: {}", (*tail_code).operand.jump_addr);
    ctx.jump_table().push((*tail_code).operand.jump_addr);
    call_next(tail_code, 1, ctx)
}

pub unsafe fn op_loop(tail_code: *const Instr, ctx: &mut ExecuteContext) -> Result<u32, VMError> {
    trace!("op_loop: {}", (*tail_code).operand.jump_addr);
    ctx.jump_table().push((*tail_code).operand.jump_addr);
    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_if(tail_code: *const Instr, ctx: &mut ExecuteContext) -> Result<u32, VMError> {
    let (end_addr, else_addr) = (*tail_code).operand.jump_addr2;
    ctx.jump_table().push(end_addr);
    let v = ctx.stack.pop_u32();
    trace!("op_if: {end_addr} {else_addr} {v}");

    let ptr = if v == 0 {
        ctx.code().offset(else_addr as isize)
    } else {
        tail_code.offset(1)
    };
    call_next(ptr, 0, ctx)
}
const MAX_CALL_STACK: usize = 10000;
// Required for direct function call threading.
// If unset, LLVM will not replace the end of op_call with a jump.
#[inline(never)]
unsafe fn internal_op_call(
    return_addr: *const Instr,
    funcidx: u32,
    ctx: &mut ExecuteContext,
) -> Result<*const Instr, VMError> {
    if ctx.local_state.len() > MAX_CALL_STACK {
        Err(VMError::StackOverflow)?
    }
    //FIXME: unwrap
    let code = ctx.module.codes.get(FuncIdx(funcidx)).unwrap_unchecked();
    let typeidx = ctx.module.xs.get(FuncIdx(funcidx)).unwrap_unchecked();
    let ft = ctx.module.fts.get(typeidx).unwrap_unchecked();

    let mut jump_table = JumpTable::new();
    jump_table.push(code.expr.len() as u32 - 1);

    let mut param_size = 0usize;
    for t in ft.0.iter() {
        param_size += t.stack_size().usize();
    }
    let mut local_size = 0usize;
    for local in &code.locals {
        local_size += local.n as usize * local.t.stack_size().usize();
    }
    let local_reference = ctx.stack.function_call(param_size, local_size, return_addr);
    trace!(
        "op_call: {funcidx} {local_size} {:?} {:?} {:?}",
        ft,
        code.locals,
        local_reference
    );
    ctx.local_state.push(LocalState {
        local_reference,
        jump_table,
        code: &code.expr,
    });
    Ok(code.expr.as_ptr())
}

pub unsafe fn op_call(tail_code: *const Instr, ctx: &mut ExecuteContext) -> Result<u32, VMError> {
    let funcidx = (*tail_code).operand.u32;

    let ptr = internal_op_call(tail_code.offset(1), funcidx, ctx)?;
    call_next(ptr, 0, ctx)
}

#[inline(never)]
unsafe fn internal_op_call_indirect(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> Result<*const Instr, VMError> {
    let i = ctx.stack.pop_u32();
    let tableidx = (*tail_code).operand.u32 as usize;
    let table = ctx
        .instance
        .table
        .get(tableidx)
        .ok_or(VMError::TableIndexOutOfRange)?;
    let funcidx = *table
        .1
        .get(i as usize)
        .ok_or(VMError::TableIndexOutOfRange)?;
    if funcidx == TABLE_UNINITIALIZED {
        Err(VMError::TableUninitialized)?;
    }
    let actual_typeidx = ctx.module.xs.get(FuncIdx(funcidx)).unwrap(); //FIXME:
    let actual_ft = ctx.module.fts.get(actual_typeidx).unwrap(); //FIXME:
    let expected_typeidx = (*tail_code.offset(1)).operand.u32;
    let expected_ft = ctx
        .module
        .fts
        .get(TypeIdx(expected_typeidx))
        .unwrap_unchecked();
    if actual_ft != expected_ft {
        Err(VMError::CallIndirectInvalidType)?
    }
    let ptr = internal_op_call(tail_code.offset(2), funcidx, ctx)?;
    Ok(ptr)
}
pub unsafe fn op_call_indirect(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> Result<u32, VMError> {
    let ptr = internal_op_call_indirect(tail_code, ctx)?;
    call_next(ptr, 0, ctx)
}
pub unsafe fn op_drop(tail_code: *const Instr, ctx: &mut ExecuteContext) -> Result<u32, VMError> {
    ctx.stack.drop((*tail_code).operand.drop_size);
    call_next(tail_code, 1, ctx)
}
#[inline(never)]
unsafe fn internal_op_select(tail_code: *const Instr, ctx: &mut ExecuteContext) {
    let x = (*tail_code).operand.select;
    let cond = ctx.stack.pop_u32();

    let a = ctx.stack.pop_u8_array_generic::<8>(x.into());
    let b = ctx.stack.pop_u8_array_generic::<8>(x.into());
    let v = if cond == 0 { a } else { b };
    trace!("op_select: {x} {cond} {a:?} {b:?} => {v:?}");
    ctx.stack.push_slice(&v[0..x]);
}
pub unsafe fn op_select(tail_code: *const Instr, ctx: &mut ExecuteContext) -> Result<u32, VMError> {
    internal_op_select(tail_code, ctx);
    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_local_get4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> Result<u32, VMError> {
    let addr = (*tail_code).operand.local_addr as usize;
    ctx.stack.local_get(&ctx.local_reference(), addr, 4);
    trace!("op_local_get4: {addr}");

    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_local_get8(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> Result<u32, VMError> {
    let addr = (*tail_code).operand.local_addr as usize;
    ctx.stack.local_get(&ctx.local_reference(), addr, 8);
    trace!("op_local_get8: {addr}");

    call_next(tail_code, 1, ctx)
}

pub unsafe fn op_local_set4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> Result<u32, VMError> {
    let addr = (*tail_code).operand.local_addr as usize;
    ctx.stack.local_set(&ctx.local_reference(), addr, 4);
    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_local_set8(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> Result<u32, VMError> {
    let addr = (*tail_code).operand.local_addr as usize;
    ctx.stack.local_set(&ctx.local_reference(), addr, 8);

    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_local_tee4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> Result<u32, VMError> {
    let addr = (*tail_code).operand.local_addr as usize;
    ctx.stack.local_tee(&ctx.local_reference(), addr, 4);
    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_local_tee8(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> Result<u32, VMError> {
    let addr = (*tail_code).operand.local_addr as usize;
    ctx.stack.local_tee(&ctx.local_reference(), addr, 8);

    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_global_get4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> Result<u32, VMError> {
    let addr = (*tail_code).operand.local_addr as usize;
    ctx.stack.push_slice(&ctx.instance.globals[addr..addr + 4]);
    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_global_get8(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> Result<u32, VMError> {
    let addr = (*tail_code).operand.local_addr as usize;
    ctx.stack.push_slice(&ctx.instance.globals[addr..addr + 8]);
    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_global_set4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> Result<u32, VMError> {
    let addr = (*tail_code).operand.local_addr as usize;
    ctx.instance.globals[addr..addr + 4].copy_from_slice(&ctx.stack.pop_u8_array::<4>());
    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_global_set8(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> Result<u32, VMError> {
    let addr = (*tail_code).operand.local_addr as usize;
    ctx.instance.globals[addr..addr + 8].copy_from_slice(&ctx.stack.pop_u8_array::<8>());
    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_i32_load(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> Result<u32, VMError> {
    let memarg = (*tail_code).operand.memarg;
    let offset = ctx.stack.pop_u32();
    let v = ctx.instance.memory.read_u32(memarg, offset)?;
    ctx.stack.push_u32(v);
    trace!("op_i32_load: {:?} {} => {v}", memarg, offset);
    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_i32_load8_u(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> Result<u32, VMError> {
    let memarg = (*tail_code).operand.memarg;
    let offset = ctx.stack.pop_u32();
    let v = ctx.instance.memory.read_u8(memarg, offset)? as u32;
    ctx.stack.push_u32(v);
    trace!("op_i32_load8_u: {:?} {} => {v}", memarg, offset);
    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_i32_load8_s(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> Result<u32, VMError> {
    let memarg = (*tail_code).operand.memarg;
    let offset = ctx.stack.pop_u32();
    let v = ctx.instance.memory.read_i8(memarg, offset)? as i32;
    ctx.stack.push_i32(v);
    trace!("op_i32_load8_u: {:?} {} => {v}", memarg, offset);
    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_i32_store(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> Result<u32, VMError> {
    let memarg = (*tail_code).operand.memarg;
    let v = ctx.stack.pop_u32();
    let offset = ctx.stack.pop_u32();
    trace!("op_i32_store: {:?} offset={} value={v}", memarg, offset);
    ctx.instance.memory.write_u32(memarg, offset, v)?;
    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_f64_store(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> Result<u32, VMError> {
    let memarg = (*tail_code).operand.memarg;
    let v = ctx.stack.pop_f64();
    let offset = ctx.stack.pop_u32();
    trace!("op_i32_store: {:?} offset={} value={v}", memarg, offset);
    ctx.instance.memory.write_f64(memarg, offset, v)?;
    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_i32_store8(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> Result<u32, VMError> {
    let memarg = (*tail_code).operand.memarg;
    let v = ctx.stack.pop_u32();
    let offset = ctx.stack.pop_u32();
    trace!("op_i32_store: {:?} offset={} value={v}", memarg, offset);
    ctx.instance.memory.write_u8(memarg, offset, v as u8)?;
    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_i32_store16(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> Result<u32, VMError> {
    let memarg = (*tail_code).operand.memarg;
    let v = ctx.stack.pop_u32();
    let offset = ctx.stack.pop_u32();
    trace!("op_i32_store: {:?} offset={} value={v}", memarg, offset);
    ctx.instance.memory.write_u16(memarg, offset, v as u16)?;
    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_f32_eq(tail_code: *const Instr, ctx: &mut ExecuteContext) -> Result<u32, VMError> {
    let a = ctx.stack.pop_f32();
    let b = ctx.stack.pop_f32();
    ctx.stack.push_u32(if a == b { 1 } else { 0 });
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_f32_le(tail_code: *const Instr, ctx: &mut ExecuteContext) -> Result<u32, VMError> {
    let b = ctx.stack.pop_f32();
    let a = ctx.stack.pop_f32();
    ctx.stack.push_u32(if a <= b { 1 } else { 0 });
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_f64_eq(tail_code: *const Instr, ctx: &mut ExecuteContext) -> Result<u32, VMError> {
    let a = ctx.stack.pop_f64();
    let b = ctx.stack.pop_f64();
    ctx.stack.push_u32(if a == b { 1 } else { 0 });
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_f64_le(tail_code: *const Instr, ctx: &mut ExecuteContext) -> Result<u32, VMError> {
    let b = ctx.stack.pop_f64();
    let a = ctx.stack.pop_f64();
    ctx.stack.push_u32(if a <= b { 1 } else { 0 });
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i32_ctz(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> Result<u32, VMError> {
    let v = ctx.stack.pop_u32().trailing_zeros();
    ctx.stack.push_u32(v);
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i32_clz(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> Result<u32, VMError> {
    let v = ctx.stack.pop_u32().leading_zeros();
    ctx.stack.push_u32(v);
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i32_popcnt(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> Result<u32, VMError> {
    let v = ctx.stack.pop_u32().count_ones();
    ctx.stack.push_u32(v);
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i32_mul(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> Result<u32, VMError> {
    let a = ctx.stack.pop_i32();
    let b = ctx.stack.pop_i32();
    let r = a.wrapping_mul(b);
    ctx.stack.push_i32(r);
    trace!("op_i32_mul: {a} {b} => {r}");
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i32_div_u(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> Result<u32, VMError> {
    let a = ctx.stack.pop_u32();
    let b = ctx.stack.pop_u32();
    let r = b / a;
    ctx.stack.push_u32(r);
    trace!("op_i32_div_u: {a} {b} => {r}");
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i64_mul(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> Result<u32, VMError> {
    let a = ctx.stack.pop_i64();
    let b = ctx.stack.pop_i64();
    let r = a.wrapping_mul(b);
    ctx.stack.push_i64(r);
    trace!("op_i64_mul: {a} {b} => {r}");
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i32_rem_u(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> Result<u32, VMError> {
    let b = ctx.stack.pop_u32();
    let a = ctx.stack.pop_u32();
    ctx.stack.push_u32(a.rem(b));
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i32_eqz(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> Result<u32, VMError> {
    let a = ctx.stack.pop_u32();
    ctx.stack.push_u32(if a == 0 { 1 } else { 0 });

    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i64_eqz(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> Result<u32, VMError> {
    let a = ctx.stack.pop_u64();
    let r = if a == 0 { 1 } else { 0 };
    trace!("op_i64_eqz: {a} => {r}");
    ctx.stack.push_u32(r);

    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i64_le_u(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> Result<u32, VMError> {
    let b = ctx.stack.pop_u64();
    let a = ctx.stack.pop_u64();

    ctx.stack.push_u32(if a <= b { 1 } else { 0 });

    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i32_eq(tail_code: *const Instr, ctx: &mut ExecuteContext) -> Result<u32, VMError> {
    let a = ctx.stack.pop_u32();
    let b = ctx.stack.pop_u32();
    let r = if a == b { 1 } else { 0 };
    trace!("op_i32_eq: {a} {b} => {r}");

    ctx.stack.push_u32(r);

    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i32_ne(tail_code: *const Instr, ctx: &mut ExecuteContext) -> Result<u32, VMError> {
    let a = ctx.stack.pop_u32();
    let b = ctx.stack.pop_u32();
    let r = if a != b { 1 } else { 0 };
    trace!("op_i32_ne: {a} {b} => {r}");

    ctx.stack.push_u32(r);

    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i32_le_s(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> Result<u32, VMError> {
    let b = ctx.stack.pop_i32();
    let a = ctx.stack.pop_i32();
    let r = if a <= b { 1 } else { 0 };
    trace!("op_i32_eq: {a} {b} => {r}");

    ctx.stack.push_u32(r);

    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i32_le_u(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> Result<u32, VMError> {
    let b = ctx.stack.pop_u32();
    let a = ctx.stack.pop_u32();
    let r = if a <= b { 1 } else { 0 };
    trace!("op_i32_eq: {a} {b} => {r}");

    ctx.stack.push_u32(r);

    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_i32_ge_u(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> Result<u32, VMError> {
    let b = ctx.stack.pop_u32();
    let a = ctx.stack.pop_u32();
    let r = if a >= b { 1 } else { 0 };
    trace!("op_i32_eq: {a} {b} => {r}");

    ctx.stack.push_u32(r);

    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_mem_size(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> Result<u32, VMError> {
    ctx.stack.push_u32(ctx.instance.memory.page_size());
    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_mem_grow(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> Result<u32, VMError> {
    let page_size_delta = ctx.stack.pop_u32();
    let current_page_size = ctx.instance.memory.page_size();
    let new_page_size = current_page_size + page_size_delta;

    if ctx.module.mems.0[0].0.max.unwrap_or(PAGE_SIZE_MAX as u32) >= new_page_size {
        ctx.stack.push_u32(current_page_size);
        ctx.instance.memory.grow(page_size_delta);
    } else {
        ctx.stack.push_i32(-1);
    }

    call_next(tail_code, 0, ctx)
}
pub unsafe fn op_unreachable(
    _tail_code: *const Instr,
    _ctx: &mut ExecuteContext,
) -> Result<u32, VMError> {
    Err(VMError::Unreachable)
}
pub unsafe fn special_function_return(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> Result<u32, VMError> {
    trace!("function return");
    let tail_code = ctx
        .stack
        .function_return(&ctx.local_reference(), (*tail_code).operand.drop_size);

    ctx.local_state.pop();
    call_next(tail_code, 0, ctx)
}
pub unsafe fn special_function_vm_end(
    _tail_code: *const Instr,
    _ctx: &mut ExecuteContext,
) -> Result<u32, VMError> {
    Ok(0)
}
const VM_END: Instr = Instr {
    op: special_function_vm_end,
};
const TABLE_UNINITIALIZED: u32 = 0xFFFFFFFF;
pub fn instantiate(m: &Module) -> Result<Instance, ()> {
    let mut global_size = 0usize;
    for global in m.gs.iter() {
        global_size += global.0 .0.stack_size().usize();
    }
    let mut instance = Instance {
        memory: Memory(vec![
            0;
            m.mems.0.first().map(|v| v.0.min).unwrap_or_else(|| 0)
                as usize
                * PAGE_SIZE
        ]),
        table: m
            .tables
            .0
            .iter()
            .map(|v| TableInstance(v.0, vec![TABLE_UNINITIALIZED; v.0.limits.min as usize]))
            .collect(),
        globals: vec![0; global_size],
    };
    for elem in &m.elems.0 {
        match &elem.mode {
            ElemMode::Active(idx, offset) => {
                let offset = match offset {
                    WasmValue::I32(v) => *v as usize,
                    WasmValue::I64(v) => *v as usize,
                    _ => panic!(),
                };
                let instance = instance.table.get_mut(idx.0 as usize).unwrap();
                if instance.0.reftype != elem.kind {
                    panic!("reftype mismatch")
                }
                let expected_len = offset + elem.init.len();
                if instance.1.len() < expected_len {
                    instance.1.resize(expected_len, TABLE_UNINITIALIZED);
                }

                for (idx, e) in elem.init.iter().enumerate() {
                    instance.1[offset + idx] = *e;
                }
            }
            _ => {
                // do nothing
            }
        }
    }
    Ok(instance)
}
pub fn run_module_function(
    m: &Module,
    instance: &mut Instance,
    name: &str,
    args: &ResultValue,
) -> Result<ResultValue, VMError> {
    if let Some(ExportDesc::Func(idx)) = m.exs.find(name) {
        let code = m.codes.get(idx).unwrap();
        let mut stack = Stack::new(128 * 1024);
        let tidx = m.xs.get(idx).unwrap();
        let ft = m.fts.get(tidx).unwrap();

        let mut param_size = 0usize;
        for t in ft.0.iter() {
            param_size += t.stack_size().usize();
        }
        let mut local_size = 0usize;
        for local in &code.locals {
            local_size += local.n as usize * local.t.stack_size().usize();
        }
        for arg in args.iter() {
            match arg {
                WasmValue::I32(i32) => stack.push_i32(*i32),
                WasmValue::I64(i64) => stack.push_i64(*i64),
                WasmValue::F32(v) => stack.push_f32(*v),
                WasmValue::F64(v) => stack.push_f64(*v),
                _ => unimplemented!(),
            }
        }
        let local_reference = stack.function_call(param_size, local_size, &VM_END as *const Instr);

        tracing::trace!(
            "run_module_function: {name} {local_size} {:?} {:?}",
            code.locals,
            m.gs
        );

        let mut jump_table = JumpTable::new();
        jump_table.push(code.expr.len() as u32 - 1);
        let mut ctx = ExecuteContext {
            module: m,
            stack: &mut stack,
            local_state: vec![LocalState {
                code: &code.expr,
                jump_table,
                local_reference,
            }],
            instance,
        };
        ctx.jump_table().push(code.expr.len() as u32 - 1);
        let res = unsafe { call_next(code.expr.as_ptr(), 0, &mut ctx) };
        match res {
            Ok(_) => {
                let mut result =
                    ft.1.stack_pop_iter()
                        .map(|t| match t {
                            ValType::I32 => WasmValue::I32(stack.pop_i32()),
                            ValType::I64 => WasmValue::I64(stack.pop_i64()),
                            ValType::F32 => WasmValue::F32(stack.pop_f32()),
                            ValType::F64 => WasmValue::F64(stack.pop_f64()),
                            _ => unimplemented!(),
                        })
                        .collect::<Vec<_>>();
                result.reverse();
                Ok(ResultValue(result))
            }
            Err(err) => Err(err),
        }
    } else {
        unimplemented!()
    }
}
