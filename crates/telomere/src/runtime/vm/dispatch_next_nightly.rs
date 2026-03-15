use crate::common::{ExecuteContext, Instr, VMResult};

#[inline(always)]
pub(super) unsafe fn call_next_0(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    become super::call_code(tail_code, ctx)
}

#[inline(always)]
pub(super) unsafe fn call_next_1(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    become super::call_code(tail_code.add(1), ctx)
}

#[inline(always)]
pub(super) unsafe fn call_next_2(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    become super::call_code(tail_code.add(2), ctx)
}

// Keep this list in sync with the literal `consumed` values used in `vm.rs`.
macro_rules! dispatch_next {
    ($tail_code:expr, 0, $ctx:expr) => {{
        become $crate::runtime::vm::dispatch_next_impl::call_next_0($tail_code, $ctx)
    }};
    ($tail_code:expr, 1, $ctx:expr) => {{
        become $crate::runtime::vm::dispatch_next_impl::call_next_1($tail_code, $ctx)
    }};
    ($tail_code:expr, 2, $ctx:expr) => {{
        become $crate::runtime::vm::dispatch_next_impl::call_next_2($tail_code, $ctx)
    }};
}
