use std::ops::Rem;

use crate::{
    common::{ExecuteContext, Instr, JumpTable, Memory, Operand, Stack},
    parser::{
        core::{ExportDesc, MemArg, ValType},
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
pub fn op_i32_const(tail_code: &[Instr], ctx: &mut ExecuteContext) {
    let v = unsafe { tail_code[0].operand.i32 };
    println!("op_i32_const: {v}");
    ctx.stack.push_i32(v);
    (unsafe { tail_code[1].op })(&tail_code[2..], ctx)
}
pub fn op_i32_add(tail_code: &[Instr], ctx: &mut ExecuteContext) {
    println!("op_i32_add");
    let a = ctx.stack.pop_i32();
    let b = ctx.stack.pop_i32();
    ctx.stack.push_i32(a + b);
    (unsafe { tail_code[0].op })(&tail_code[1..], ctx)
}
pub fn op_i32_sub(tail_code: &[Instr], ctx: &mut ExecuteContext) {
    let a = ctx.stack.pop_i32();
    let b = ctx.stack.pop_i32();
    let r = b - a;
    ctx.stack.push_i32(r);

    println!("op_i32_sub: {a} {b} {r}");

    (unsafe { tail_code[0].op })(&tail_code[1..], ctx)
}
pub fn op_i64_const(tail_code: &[Instr], ctx: &mut ExecuteContext) {
    println!("op_i64_const");
    ctx.stack.push_i64(unsafe { tail_code[0].operand.i64 });
    (unsafe { tail_code[1].op })(&tail_code[2..], ctx)
}
pub fn op_f32_const(tail_code: &[Instr], ctx: &mut ExecuteContext) {
    println!("op_f32_const");
    ctx.stack.push_f32(unsafe { tail_code[0].operand.f32 });
    (unsafe { tail_code[1].op })(&tail_code[2..], ctx)
}
pub fn op_f64_const(tail_code: &[Instr], ctx: &mut ExecuteContext) {
    println!("op_f64_const");
    ctx.stack.push_f64(unsafe { tail_code[0].operand.f64 });
    (unsafe { tail_code[1].op })(&tail_code[2..], ctx)
}
pub fn op_f32_gt(tail_code: &[Instr], ctx: &mut ExecuteContext) {
    println!("op_f32_gt");
    let a = ctx.stack.pop_f32();
    let b = ctx.stack.pop_f32();
    ctx.stack.push_u32(if a < b { 1 } else { 0 });

    (unsafe { tail_code[0].op })(&tail_code[1..], ctx)
}
pub fn op_i64_add(tail_code: &[Instr], ctx: &mut ExecuteContext) {
    let a = ctx.stack.pop_i64();
    let b = ctx.stack.pop_i64();
    ctx.stack.push_i64(a + b);
    (unsafe { tail_code[0].op })(&tail_code[1..], ctx)
}
pub fn op_return(tail_code: &[Instr], ctx: &mut ExecuteContext) {
    // TODO:
}

pub fn op_end(tail_code: &[Instr], ctx: &mut ExecuteContext) {
    ctx.jump_table.end();
    (unsafe { tail_code[0].op })(&tail_code[1..], ctx)
}
pub fn op_br(tail_code: &[Instr], ctx: &mut ExecuteContext) {
    let addr = ctx
        .jump_table
        .br(unsafe { tail_code[0].operand.u32 } as usize)
        .unwrap();
    let tail_code = &ctx.code[addr..];
    (unsafe { tail_code[0].op })(&tail_code[1..], ctx)
}
pub fn op_else(_tail_code: &[Instr], ctx: &mut ExecuteContext) {
    println!("op_else");

    let addr = ctx.jump_table.br(0).unwrap();
    let tail_code = &ctx.code[addr..];
    (unsafe { tail_code[0].op })(&tail_code[1..], ctx)
}
pub fn op_br_if(tail_code: &[Instr], ctx: &mut ExecuteContext) {
    println!("op_br_if");
    let cond = ctx.stack.pop_u32();
    if cond != 0 {
        let addr = ctx
            .jump_table
            .br(unsafe { tail_code[0].operand.u32 } as usize)
            .unwrap();
        let tail_code = &ctx.code[addr..];
        (unsafe { tail_code[0].op })(&tail_code[1..], ctx)
    } else {
        (unsafe { tail_code[1].op })(&tail_code[2..], ctx)
    }
}
pub fn op_br_table(tail_code: &[Instr], ctx: &mut ExecuteContext) {
    let index = ctx.stack.pop_u32();
    let table_size = unsafe { tail_code[0].operand.u32 };
    let idx = if index < table_size {
        unsafe { tail_code[(index + 1) as usize].operand.u32 }
    } else {
        unsafe { tail_code[(table_size + 1) as usize].operand.u32 }
    };
    let addr = ctx.jump_table.br(idx as usize).unwrap();
    let tail_code = &ctx.code[addr..];
    (unsafe { tail_code[0].op })(&tail_code[1..], ctx)
}
pub fn op_block(tail_code: &[Instr], ctx: &mut ExecuteContext) {
    println!("op_block: {}", unsafe { tail_code[0].operand.jump_addr });
    ctx.jump_table
        .push(unsafe { tail_code[0].operand.jump_addr });
    (unsafe { tail_code[1].op })(&tail_code[2..], ctx)
}

pub fn op_loop(tail_code: &[Instr], ctx: &mut ExecuteContext) {
    println!("op_loop: {}", unsafe { tail_code[0].operand.jump_addr });
    ctx.jump_table
        .push(unsafe { tail_code[0].operand.jump_addr });
    (unsafe { tail_code[1].op })(&tail_code[2..], ctx)
}
pub fn op_if(tail_code: &[Instr], ctx: &mut ExecuteContext) {
    println!("op_if: {}", unsafe { tail_code[0].operand.jump_addr });
    ctx.jump_table
        .push(unsafe { tail_code[0].operand.jump_addr });
    (unsafe { tail_code[1].op })(&tail_code[2..], ctx)
}

pub fn op_call(tail_code: &[Instr], ctx: &mut ExecuteContext) {
    //TODO:
    (unsafe { tail_code[1].op })(&tail_code[2..], ctx)
}
pub fn op_call_indirect(tail_code: &[Instr], ctx: &mut ExecuteContext) {
    //TODO:
    (unsafe { tail_code[1].op })(&tail_code[2..], ctx)
}
pub fn op_drop(tail_code: &[Instr], ctx: &mut ExecuteContext) {
    ctx.stack.drop(unsafe { tail_code[0].operand.drop_size });
    (unsafe { tail_code[1].op })(&tail_code[2..], ctx)
}
pub fn op_select(tail_code: &[Instr], ctx: &mut ExecuteContext) {
    let x = unsafe { tail_code[0].operand.select };
    let cond = ctx.stack.pop_u32();

    let a = ctx.stack.pop_u8_array_generic::<8>(x.into());
    let b = ctx.stack.pop_u8_array_generic::<8>(x.into());
    let v = if cond == 0 { a } else { b };
    println!("op_select: {x} {cond} {a:?} {b:?} => {v:?}");
    ctx.stack.push_slice(&v[0..x]);
    (unsafe { tail_code[1].op })(&tail_code[2..], ctx)
}
pub fn op_local_get4(tail_code: &[Instr], ctx: &mut ExecuteContext) {
    let addr = unsafe { tail_code[0].operand.local_addr } as usize;
    ctx.stack.push_slice(&ctx.locals[addr..addr + 4]);
    (unsafe { tail_code[1].op })(&tail_code[2..], ctx)
}
pub fn op_local_get8(tail_code: &[Instr], ctx: &mut ExecuteContext) {
    let addr = unsafe { tail_code[0].operand.local_addr } as usize;
    ctx.stack.push_slice(&ctx.locals[addr..addr + 8]);
    (unsafe { tail_code[1].op })(&tail_code[2..], ctx)
}

pub fn op_local_set4(tail_code: &[Instr], ctx: &mut ExecuteContext) {
    let addr = unsafe { tail_code[0].operand.local_addr } as usize;
    ctx.locals[addr..addr + 4].copy_from_slice(&ctx.stack.pop_u8_array::<4>());
    (unsafe { tail_code[1].op })(&tail_code[2..], ctx)
}
pub fn op_local_set8(tail_code: &[Instr], ctx: &mut ExecuteContext) {
    let addr = unsafe { tail_code[0].operand.local_addr } as usize;
    ctx.locals[addr..addr + 8].copy_from_slice(&ctx.stack.pop_u8_array::<8>());
    (unsafe { tail_code[1].op })(&tail_code[2..], ctx)
}
pub fn op_local_tee4(tail_code: &[Instr], ctx: &mut ExecuteContext) {
    let addr = unsafe { tail_code[0].operand.local_addr } as usize;
    let v = ctx.stack.pop_u8_array::<4>();
    ctx.locals[addr..addr + 4].copy_from_slice(&v);
    ctx.stack.push_u8_array(v);
    (unsafe { tail_code[1].op })(&tail_code[2..], ctx)
}
pub fn op_local_tee8(tail_code: &[Instr], ctx: &mut ExecuteContext) {
    let addr = unsafe { tail_code[0].operand.local_addr } as usize;
    let v = ctx.stack.pop_u8_array::<8>();
    ctx.locals[addr..addr + 8].copy_from_slice(&v);
    ctx.stack.push_u8_array(v);
    (unsafe { tail_code[1].op })(&tail_code[2..], ctx)
}
pub fn op_global_get4(tail_code: &[Instr], ctx: &mut ExecuteContext) {
    let addr = unsafe { tail_code[0].operand.local_addr } as usize;
    ctx.stack.push_slice(&ctx.globals[addr..addr + 4]);
    (unsafe { tail_code[1].op })(&tail_code[2..], ctx)
}
pub fn op_global_get8(tail_code: &[Instr], ctx: &mut ExecuteContext) {
    let addr = unsafe { tail_code[0].operand.local_addr } as usize;
    ctx.stack.push_slice(&ctx.globals[addr..addr + 8]);
    (unsafe { tail_code[1].op })(&tail_code[2..], ctx)
}
pub fn op_global_set4(tail_code: &[Instr], ctx: &mut ExecuteContext) {
    let addr = unsafe { tail_code[0].operand.local_addr } as usize;
    ctx.globals[addr..addr + 4].copy_from_slice(&ctx.stack.pop_u8_array::<4>());
    (unsafe { tail_code[1].op })(&tail_code[2..], ctx)
}
pub fn op_global_set8(tail_code: &[Instr], ctx: &mut ExecuteContext) {
    let addr = unsafe { tail_code[0].operand.local_addr } as usize;
    ctx.globals[addr..addr + 8].copy_from_slice(&ctx.stack.pop_u8_array::<8>());
    (unsafe { tail_code[1].op })(&tail_code[2..], ctx)
}
pub fn op_i32_load(tail_code: &[Instr], ctx: &mut ExecuteContext) {
    let memarg = unsafe { tail_code[0].operand.memarg };
    ctx.stack.push_u32(ctx.memory.read_u32(memarg));
    (unsafe { tail_code[1].op })(&tail_code[2..], ctx)
}
pub fn op_i32_store(tail_code: &[Instr], ctx: &mut ExecuteContext) {
    let memarg = unsafe { tail_code[0].operand.memarg };
    let v = ctx.stack.pop_u32();
    ctx.memory.write_u32(memarg, v);
    (unsafe { tail_code[1].op })(&tail_code[2..], ctx)
}
pub fn op_i32_ctz(tail_code: &[Instr], ctx: &mut ExecuteContext) {
    let v = ctx.stack.pop_u32().trailing_zeros();
    ctx.stack.push_u32(v);
    (unsafe { tail_code[0].op })(&tail_code[1..], ctx)
}
pub fn op_i32_popcnt(tail_code: &[Instr], ctx: &mut ExecuteContext) {
    let v = ctx.stack.pop_u32().count_ones();
    ctx.stack.push_u32(v);
    (unsafe { tail_code[0].op })(&tail_code[1..], ctx)
}
pub fn op_i32_mul(tail_code: &[Instr], ctx: &mut ExecuteContext) {
    let a = ctx.stack.pop_i32();
    let b = ctx.stack.pop_i32();
    let r = a * b;
    ctx.stack.push_i32(r);
    println!("op_i32_mul: {a} {b} => {r}");
    (unsafe { tail_code[0].op })(&tail_code[1..], ctx)
}
pub fn op_i32_rem_u(tail_code: &[Instr], ctx: &mut ExecuteContext) {
    let a = ctx.stack.pop_u32();
    let b = ctx.stack.pop_u32();
    ctx.stack.push_u32(a.rem(b));
    (unsafe { tail_code[0].op })(&tail_code[1..], ctx)
}
pub fn op_i32_eqz(tail_code: &[Instr], ctx: &mut ExecuteContext) {
    let a = ctx.stack.pop_u32();
    ctx.stack.push_u32(if a == 0 { 1 } else { 0 });

    (unsafe { tail_code[0].op })(&tail_code[1..], ctx)
}
pub fn op_i32_eq(tail_code: &[Instr], ctx: &mut ExecuteContext) {
    let a = ctx.stack.pop_u32();
    let b = ctx.stack.pop_u32();
    let r = if a == b { 1 } else { 0 };
    println!("op_i32_eq: {a} {b} => {r}");

    ctx.stack.push_u32(r);

    (unsafe { tail_code[0].op })(&tail_code[1..], ctx)
}
pub fn op_mem_glow(tail_code: &[Instr], ctx: &mut ExecuteContext) {
    let _page_size = ctx.stack.pop_i32();
    // FIXME: glow memory
    ctx.stack.push_i32(-1);
    (unsafe { tail_code[0].op })(&tail_code[1..], ctx)
}
pub fn op_unreachable(_tail_code: &[Instr], _ctx: &mut ExecuteContext) {
    unreachable!()
}
pub fn special_function_return(_tail_code: &[Instr], _ctx: &mut ExecuteContext) {
    println!("function return")
}
pub fn run_module_function(m: &Module, name: &str, args: &ResultValue) -> ResultValue {
    if let Some(ExportDesc::Func(idx)) = m.exs.find(name) {
        let code = m.codes.get(idx).unwrap();
        let mut stack = Stack::new(1000);
        let tidx = m.xs.get(idx).unwrap();
        let ft = m.fts.get(tidx).unwrap();

        // TODO: we must validate input argument type
        for arg in args.iter() {
            match arg {
                WasmValue::I32(i32) => stack.push_i32(*i32),
                WasmValue::I64(i64) => stack.push_i64(*i64),
                WasmValue::F32(v) => stack.push_f32(*v),
                WasmValue::F64(v) => stack.push_f64(*v),
            }
        }
        let mut global_size = 0usize;
        for global in m.gs.iter() {
            global_size += global.0 .0.stack_size().usize();
        }
        let mut local_size = 0usize;
        for local in &code.locals {
            local_size += local.n as usize * local.t.stack_size().usize();
        }
        println!(
            "run_module_function: {name} {local_size} {:?} {global_size} {:?}",
            code.locals, m.gs
        );

        let mut locals = Vec::new();
        locals.resize(local_size, 0);
        let mut globals = Vec::new();
        globals.resize(global_size, 0);
        let mut memory = Vec::new();
        memory.resize(65535, 0);
        let mut jump_table = JumpTable::new();
        jump_table.push(code.expr.len() - 1);
        let mut ctx = ExecuteContext {
            stack: &mut stack,
            code: &code.expr,
            jump_table,
            locals: &mut locals[..],
            globals: &mut globals[..],
            memory: Memory::new(&mut memory[..]),
        };
        ctx.jump_table.push(code.expr.len() - 1);
        (unsafe { code.expr[0].op })(&code.expr[1..], &mut ctx);

        return ResultValue(
            ft.1.iter()
                .map(|t| match t {
                    ValType::I32 => WasmValue::I32(stack.pop_i32()),
                    ValType::I64 => WasmValue::I64(stack.pop_i64()),
                    ValType::F32 => WasmValue::F32(stack.pop_f32()),
                    ValType::F64 => WasmValue::F64(stack.pop_f64()),
                    _ => unimplemented!(),
                })
                .collect(),
        );
    }
    unimplemented!()
}
