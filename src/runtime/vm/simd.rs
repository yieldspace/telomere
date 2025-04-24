use crate::{
    common::{ExecuteContext, Instr},
    runtime::vm::call_next,
    VMResult,
};

pub unsafe fn op_v128_load(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let memarg = (*tail_code).operand.memarg;
    let offset = ctx.stack.pop_u32();

    let memory = vm_try!(VMResult::from_option(ctx.memory(), || {
        VMResult::MemoryIndexOutOfRange
    }));
    let v = vm_try!(memory.read_u128(memarg, offset));
    vm_try!(ctx.stack.push_u128(v));
    call_next(tail_code, 1, ctx)
}
