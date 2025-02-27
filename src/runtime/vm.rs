use crate::{
    common::{ExecuteContext, Instr, Operand, Stack},
    parser::{
        core::{ExportDesc, ValType},
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
pub enum WasmValue {
    I32(i32),
    I64(i64),
    //F32(f32),
    //F64(f64),
    //V128,
    //FuncRef,
    //ExternRef,
}
pub fn op_i32_const(operand: Operand, tail_code: &[Instr], ctx: &mut ExecuteContext) {
    ctx.stack.push_i32(unsafe { operand.i32 });
    (tail_code[0].op)(tail_code[0].operand, &tail_code[1..], ctx)
}
pub fn op_i32_add(_operand: Operand, tail_code: &[Instr], ctx: &mut ExecuteContext) {
    let a = ctx.stack.pop_i32();
    let b = ctx.stack.pop_i32();
    ctx.stack.push_i32(a + b);
    (tail_code[0].op)(tail_code[0].operand, &tail_code[1..], ctx)
}
pub fn op_i64_const(operand: Operand, tail_code: &[Instr], ctx: &mut ExecuteContext) {
    ctx.stack.push_i64(unsafe { operand.i64 });
    (tail_code[0].op)(tail_code[0].operand, &tail_code[1..], ctx)
}
pub fn op_i64_add(_operand: Operand, tail_code: &[Instr], ctx: &mut ExecuteContext) {
    let a = ctx.stack.pop_i64();
    let b = ctx.stack.pop_i64();
    ctx.stack.push_i64(a + b);
    (tail_code[0].op)(tail_code[0].operand, &tail_code[1..], ctx)
}
pub fn op_return(operand: Operand, tail_code: &[Instr], ctx: &mut ExecuteContext) {
    // TODO:
}

pub fn op_end(operand: Operand, tail_code: &[Instr], ctx: &mut ExecuteContext) {
    // TODO:
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
            }
        }
        let mut ctx = ExecuteContext {
            stack: &mut stack,
            code: &code.expr,
        };
        (code.expr[0].op)(code.expr[0].operand, &code.expr[1..], &mut ctx);

        return ResultValue(
            ft.1.iter()
                .map(|t| match t {
                    ValType::I32 => WasmValue::I32(stack.pop_i32()),
                    ValType::I64 => WasmValue::I64(stack.pop_i64()),
                    _ => unimplemented!(),
                })
                .collect(),
        );
    }
    unimplemented!()
}
