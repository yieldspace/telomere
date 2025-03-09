use std::ops::Rem;

use crate::{
    common::{ExecuteContext, Instr, JumpTable, LocalState, Memory, Stack, VMError},
    parser::{
        core::{ExportDesc, FuncIdx, ValType},
        Module,
    },
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
#[derive(Debug, Clone)]
pub enum WasmValue {
    I32(i32),
    I64(i64),
    F32(f32),
    F64(f64),
    //V128,
    //FuncRef,
    //ExternRef,
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
// Required for direct function call threading.
// If unset, LLVM will not replace the end of op_call with a jump.
#[inline(never)]
unsafe fn internal_op_call(tail_code: *const Instr, ctx: &mut ExecuteContext) -> *const Instr {
    let ptr = {
        let funcidx = (*tail_code).operand.u32;
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
        let local_reference = ctx
            .stack
            .function_call(param_size, local_size, tail_code.offset(1));
        /*trace!(
            "op_call: {funcidx} {local_size} {:?} {:?} {:?}",
            ft,
            code.locals,
            local_reference
        );*/
        ctx.local_state.push(LocalState {
            local_reference,
            jump_table,
            code: &code.expr,
        });
        code.expr.as_ptr()
    };
    ptr
}
pub unsafe fn op_call(tail_code: *const Instr, ctx: &mut ExecuteContext) -> Result<u32, VMError> {
    let ptr = internal_op_call(tail_code, ctx);
    call_next(ptr, 0, ctx)
}
pub unsafe fn op_call_indirect(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> Result<u32, VMError> {
    //TODO:
    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_drop(tail_code: *const Instr, ctx: &mut ExecuteContext) -> Result<u32, VMError> {
    ctx.stack.drop((*tail_code).operand.drop_size);
    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_select(tail_code: *const Instr, ctx: &mut ExecuteContext) -> Result<u32, VMError> {
    let x = (*tail_code).operand.select;
    let cond = ctx.stack.pop_u32();

    let a = ctx.stack.pop_u8_array_generic::<8>(x.into());
    let b = ctx.stack.pop_u8_array_generic::<8>(x.into());
    let v = if cond == 0 { a } else { b };
    trace!("op_select: {x} {cond} {a:?} {b:?} => {v:?}");
    ctx.stack.push_slice(&v[0..x]);
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
    ctx.stack.push_slice(&ctx.globals[addr..addr + 4]);
    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_global_get8(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> Result<u32, VMError> {
    let addr = (*tail_code).operand.local_addr as usize;
    ctx.stack.push_slice(&ctx.globals[addr..addr + 8]);
    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_global_set4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> Result<u32, VMError> {
    let addr = (*tail_code).operand.local_addr as usize;
    ctx.globals[addr..addr + 4].copy_from_slice(&ctx.stack.pop_u8_array::<4>());
    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_global_set8(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> Result<u32, VMError> {
    let addr = (*tail_code).operand.local_addr as usize;
    ctx.globals[addr..addr + 8].copy_from_slice(&ctx.stack.pop_u8_array::<8>());
    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_i32_load(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> Result<u32, VMError> {
    let memarg = (*tail_code).operand.memarg;
    ctx.stack.push_u32(ctx.memory.read_u32(memarg));
    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_i32_store(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> Result<u32, VMError> {
    let memarg = (*tail_code).operand.memarg;
    let v = ctx.stack.pop_u32();
    ctx.memory.write_u32(memarg, v);
    call_next(tail_code, 1, ctx)
}
pub unsafe fn op_i32_ctz(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> Result<u32, VMError> {
    let v = ctx.stack.pop_u32().trailing_zeros();
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
    let a = ctx.stack.pop_u32();
    let b = ctx.stack.pop_u32();
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
pub unsafe fn op_mem_glow(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> Result<u32, VMError> {
    let _page_size = ctx.stack.pop_i32();
    // FIXME: glow memory
    ctx.stack.push_i32(-1);
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
pub fn run_module_function(m: &Module, name: &str, args: &ResultValue) -> ResultValue {
    if let Some(ExportDesc::Func(idx)) = m.exs.find(name) {
        let code = m.codes.get(idx).unwrap();
        let mut stack = Stack::new(16 * 1024);
        let tidx = m.xs.get(idx).unwrap();
        let ft = m.fts.get(tidx).unwrap();

        let mut global_size = 0usize;
        for global in m.gs.iter() {
            global_size += global.0 .0.stack_size().usize();
        }
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
            }
        }
        let local_reference = stack.function_call(param_size, local_size, &VM_END as *const Instr);

        tracing::trace!(
            "run_module_function: {name} {local_size} {:?} {global_size} {:?}",
            code.locals,
            m.gs
        );

        let mut globals = Vec::new();
        globals.resize(global_size, 0);
        let mut memory = Vec::new();
        memory.resize(65535, 0);
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
            globals: &mut globals[..],
            memory: Memory::new(&mut memory[..]),
        };
        ctx.jump_table().push(code.expr.len() as u32 - 1);
        let _ = unsafe { call_next(code.expr.as_ptr(), 0, &mut ctx) };
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
        return ResultValue(result);
    }
    unimplemented!()
}
