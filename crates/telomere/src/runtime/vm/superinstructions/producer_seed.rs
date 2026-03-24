use super::memory::{
    local_imm_addr_mem_start_from_parts, local_mem_start_from_local, read_local_load4_kind,
    read_local_load8_kind,
};
use super::scalar::{i32_scalar_eval, i64_scalar_eval};
use super::*;

pub(super) enum ProducerSeedKind {
    Local = 0,
    LocalImmScalar = 1,
    LocalLocalScalar = 2,
    LocalAddrLoad = 3,
    LocalImmAddrLoad = 4,
    ConstAddrLoad = 5,
}

impl ProducerSeedKind {
    #[inline(always)]
    fn from_raw(raw: u32) -> Self {
        match raw {
            0 => Self::Local,
            1 => Self::LocalImmScalar,
            2 => Self::LocalLocalScalar,
            3 => Self::LocalAddrLoad,
            4 => Self::LocalImmAddrLoad,
            5 => Self::ConstAddrLoad,
            _ => unreachable!("invalid ProducerSeedKind: {raw}"),
        }
    }
}

#[inline(always)]
pub(super) unsafe fn producer_seed_u32(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<u32> {
    match ProducerSeedKind::from_raw((*tail_code).operand.u32) {
        ProducerSeedKind::Local => VMResult::Success(local_u32(
            ctx.stack,
            &ctx.local_reference(),
            (*tail_code.add(1)).operand.local_addr,
        )),
        ProducerSeedKind::LocalImmScalar => i32_scalar_eval(
            local_u32(
                ctx.stack,
                &ctx.local_reference(),
                (*tail_code.add(1)).operand.local_addr,
            ),
            (*tail_code.add(2)).operand.u32,
            I32ScalarKind::from_raw((*tail_code.add(3)).operand.u32),
        ),
        ProducerSeedKind::LocalLocalScalar => i32_scalar_eval(
            local_u32(
                ctx.stack,
                &ctx.local_reference(),
                (*tail_code.add(1)).operand.local_addr,
            ),
            local_u32(
                ctx.stack,
                &ctx.local_reference(),
                (*tail_code.add(2)).operand.local_addr,
            ),
            I32ScalarKind::from_raw((*tail_code.add(3)).operand.u32),
        ),
        ProducerSeedKind::LocalAddrLoad => {
            let start = vm_try!(local_mem_start_from_local(
                ctx,
                (*tail_code.add(1)).operand.local_addr,
                (*tail_code.add(2)).operand.memarg,
            ));
            read_local_load4_kind(
                ctx,
                start,
                Load4Kind::from_raw((*tail_code.add(3)).operand.u32),
            )
        }
        ProducerSeedKind::LocalImmAddrLoad => {
            let start = vm_try!(local_imm_addr_mem_start_from_parts(
                ctx,
                (*tail_code.add(1)).operand.local_addr,
                (*tail_code.add(2)).operand.i32 as u32,
                (*tail_code.add(3)).operand.memarg,
            ));
            read_local_load4_kind(
                ctx,
                start,
                Load4Kind::from_raw((*tail_code.add(4)).operand.u32),
            )
        }
        ProducerSeedKind::ConstAddrLoad => read_local_load4_kind(
            ctx,
            (*tail_code.add(1)).operand.u32 as usize,
            Load4Kind::from_raw((*tail_code.add(2)).operand.u32),
        ),
    }
}

#[inline(always)]
pub(super) unsafe fn producer_seed_u64(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<u64> {
    match ProducerSeedKind::from_raw((*tail_code).operand.u32) {
        ProducerSeedKind::Local => VMResult::Success(local_u64(
            ctx.stack,
            &ctx.local_reference(),
            (*tail_code.add(1)).operand.local_addr,
        )),
        ProducerSeedKind::LocalImmScalar => i64_scalar_eval(
            local_u64(
                ctx.stack,
                &ctx.local_reference(),
                (*tail_code.add(1)).operand.local_addr,
            ),
            (*tail_code.add(2)).operand.u64,
            I64ScalarKind::from_raw((*tail_code.add(3)).operand.u32),
        ),
        ProducerSeedKind::LocalLocalScalar => i64_scalar_eval(
            local_u64(
                ctx.stack,
                &ctx.local_reference(),
                (*tail_code.add(1)).operand.local_addr,
            ),
            local_u64(
                ctx.stack,
                &ctx.local_reference(),
                (*tail_code.add(2)).operand.local_addr,
            ),
            I64ScalarKind::from_raw((*tail_code.add(3)).operand.u32),
        ),
        ProducerSeedKind::LocalAddrLoad => {
            let start = vm_try!(local_mem_start_from_local(
                ctx,
                (*tail_code.add(1)).operand.local_addr,
                (*tail_code.add(2)).operand.memarg,
            ));
            read_local_load8_kind(
                ctx,
                start,
                Load8Kind::from_raw((*tail_code.add(3)).operand.u32),
            )
        }
        ProducerSeedKind::LocalImmAddrLoad => {
            let start = vm_try!(local_imm_addr_mem_start_from_parts(
                ctx,
                (*tail_code.add(1)).operand.local_addr,
                (*tail_code.add(2)).operand.i32 as u32,
                (*tail_code.add(3)).operand.memarg,
            ));
            read_local_load8_kind(
                ctx,
                start,
                Load8Kind::from_raw((*tail_code.add(4)).operand.u32),
            )
        }
        ProducerSeedKind::ConstAddrLoad => read_local_load8_kind(
            ctx,
            (*tail_code.add(1)).operand.u32 as usize,
            Load8Kind::from_raw((*tail_code.add(2)).operand.u32),
        ),
    }
}

pub unsafe fn op_i32_seed_imm_scalar_set4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let value = vm_try!(i32_scalar_eval(
        vm_try!(producer_seed_u32(tail_code, ctx)),
        (*tail_code.add(5)).operand.u32,
        I32ScalarKind::from_raw((*tail_code.add(7)).operand.u32),
    ));
    write_local_u32(
        ctx.stack,
        &ctx.local_reference(),
        (*tail_code.add(6)).operand.local_addr,
        value,
    );
    call_next(tail_code, 8, ctx)
}

pub unsafe fn op_i32_seed_imm_scalar_tee4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let value = vm_try!(i32_scalar_eval(
        vm_try!(producer_seed_u32(tail_code, ctx)),
        (*tail_code.add(5)).operand.u32,
        I32ScalarKind::from_raw((*tail_code.add(7)).operand.u32),
    ));
    write_local_u32(
        ctx.stack,
        &ctx.local_reference(),
        (*tail_code.add(6)).operand.local_addr,
        value,
    );
    vm_try!(ctx.stack.push_u32(value));
    call_next(tail_code, 8, ctx)
}

pub unsafe fn op_i64_seed_imm_scalar_set8(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let value = vm_try!(i64_scalar_eval(
        vm_try!(producer_seed_u64(tail_code, ctx)),
        (*tail_code.add(5)).operand.u64,
        I64ScalarKind::from_raw((*tail_code.add(7)).operand.u32),
    ));
    write_local_u64(
        ctx.stack,
        &ctx.local_reference(),
        (*tail_code.add(6)).operand.local_addr,
        value,
    );
    call_next(tail_code, 8, ctx)
}

pub unsafe fn op_i64_seed_imm_scalar_tee8(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let value = vm_try!(i64_scalar_eval(
        vm_try!(producer_seed_u64(tail_code, ctx)),
        (*tail_code.add(5)).operand.u64,
        I64ScalarKind::from_raw((*tail_code.add(7)).operand.u32),
    ));
    write_local_u64(
        ctx.stack,
        &ctx.local_reference(),
        (*tail_code.add(6)).operand.local_addr,
        value,
    );
    vm_try!(ctx.stack.push_u64(value));
    call_next(tail_code, 8, ctx)
}

pub unsafe fn op_i32_seed_tee_imm_scalar_set4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let seed = vm_try!(producer_seed_u32(tail_code, ctx));
    write_local_u32(
        ctx.stack,
        &ctx.local_reference(),
        (*tail_code.add(5)).operand.local_addr,
        seed,
    );
    let value = vm_try!(i32_scalar_eval(
        seed,
        (*tail_code.add(6)).operand.u32,
        I32ScalarKind::from_raw((*tail_code.add(8)).operand.u32),
    ));
    write_local_u32(
        ctx.stack,
        &ctx.local_reference(),
        (*tail_code.add(7)).operand.local_addr,
        value,
    );
    call_next(tail_code, 9, ctx)
}

pub unsafe fn op_i32_seed_tee_imm_scalar_tee4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let seed = vm_try!(producer_seed_u32(tail_code, ctx));
    write_local_u32(
        ctx.stack,
        &ctx.local_reference(),
        (*tail_code.add(5)).operand.local_addr,
        seed,
    );
    let value = vm_try!(i32_scalar_eval(
        seed,
        (*tail_code.add(6)).operand.u32,
        I32ScalarKind::from_raw((*tail_code.add(8)).operand.u32),
    ));
    write_local_u32(
        ctx.stack,
        &ctx.local_reference(),
        (*tail_code.add(7)).operand.local_addr,
        value,
    );
    vm_try!(ctx.stack.push_u32(value));
    call_next(tail_code, 9, ctx)
}

pub unsafe fn op_i64_seed_tee_imm_scalar_set8(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let seed = vm_try!(producer_seed_u64(tail_code, ctx));
    write_local_u64(
        ctx.stack,
        &ctx.local_reference(),
        (*tail_code.add(5)).operand.local_addr,
        seed,
    );
    let value = vm_try!(i64_scalar_eval(
        seed,
        (*tail_code.add(6)).operand.u64,
        I64ScalarKind::from_raw((*tail_code.add(8)).operand.u32),
    ));
    write_local_u64(
        ctx.stack,
        &ctx.local_reference(),
        (*tail_code.add(7)).operand.local_addr,
        value,
    );
    call_next(tail_code, 9, ctx)
}

pub unsafe fn op_i64_seed_tee_imm_scalar_tee8(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let seed = vm_try!(producer_seed_u64(tail_code, ctx));
    write_local_u64(
        ctx.stack,
        &ctx.local_reference(),
        (*tail_code.add(5)).operand.local_addr,
        seed,
    );
    let value = vm_try!(i64_scalar_eval(
        seed,
        (*tail_code.add(6)).operand.u64,
        I64ScalarKind::from_raw((*tail_code.add(8)).operand.u32),
    ));
    write_local_u64(
        ctx.stack,
        &ctx.local_reference(),
        (*tail_code.add(7)).operand.local_addr,
        value,
    );
    vm_try!(ctx.stack.push_u64(value));
    call_next(tail_code, 9, ctx)
}

pub unsafe fn op_i32_seed_tee_const_self_select4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let seed = vm_try!(producer_seed_u32(tail_code, ctx));
    write_local_u32(
        ctx.stack,
        &ctx.local_reference(),
        (*tail_code.add(5)).operand.local_addr,
        seed,
    );
    vm_try!(ctx.stack.push_u32(if seed == 0 {
        (*tail_code.add(6)).operand.u32
    } else {
        seed
    }));
    call_next(tail_code, 7, ctx)
}

pub unsafe fn op_i64_seed_tee_const_self_select8(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let seed = vm_try!(producer_seed_u64(tail_code, ctx));
    write_local_u64(
        ctx.stack,
        &ctx.local_reference(),
        (*tail_code.add(5)).operand.local_addr,
        seed,
    );
    vm_try!(ctx.stack.push_u64(if seed == 0 {
        (*tail_code.add(6)).operand.u64
    } else {
        seed
    }));
    call_next(tail_code, 7, ctx)
}
