use crate::common::{ExecuteContext, Instr, VMResult};

#[inline(always)]
pub(crate) unsafe fn call_code(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    ctx.cont = tail_code;
    ((*tail_code).op)(tail_code.add(1), ctx)
}
