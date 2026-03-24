use super::*;

enum NarrowCopyKind {
    Load8Store8,
    Load16Store16,
}

#[inline(always)]
fn store4_bytes(value: u32, kind: Store4Kind) -> StoreBytes {
    match kind {
        Store4Kind::I32 | Store4Kind::F32 => StoreBytes::Write4(value.to_le_bytes()),
        Store4Kind::I32Store8 => StoreBytes::Write1([(value & 0xff) as u8]),
        Store4Kind::I32Store16 => {
            StoreBytes::Write2([(value & 0xff) as u8, ((value >> 8) & 0xff) as u8])
        }
    }
}

#[inline(always)]
fn store8_bytes(value: u64, kind: Store8Kind) -> StoreBytes {
    match kind {
        Store8Kind::I64 | Store8Kind::F64 => StoreBytes::Write8(value.to_le_bytes()),
        Store8Kind::I64Store8 => StoreBytes::Write1([(value & 0xff) as u8]),
        Store8Kind::I64Store16 => {
            StoreBytes::Write2([(value & 0xff) as u8, ((value >> 8) & 0xff) as u8])
        }
        Store8Kind::I64Store32 => StoreBytes::Write4([
            (value & 0xff) as u8,
            ((value >> 8) & 0xff) as u8,
            ((value >> 16) & 0xff) as u8,
            ((value >> 24) & 0xff) as u8,
        ]),
    }
}

#[inline(always)]
pub(super) unsafe fn read_local_load4_kind(
    ctx: &mut ExecuteContext,
    start: usize,
    kind: Load4Kind,
) -> VMResult<u32> {
    match kind {
        Load4Kind::I32 => ctx
            .gc
            .local_read_u32_at(ctx.default_local_memory_id_unchecked(), start),
        Load4Kind::I32Load8S => VMResult::Success(i32::from(vm_try!(ctx
            .gc
            .local_read_i8_at(ctx.default_local_memory_id_unchecked(), start,)))
            as u32),
        Load4Kind::I32Load8U => VMResult::Success(u32::from(vm_try!(ctx
            .gc
            .local_read_u8_at(ctx.default_local_memory_id_unchecked(), start)))),
        Load4Kind::I32Load16S => VMResult::Success(i32::from(vm_try!(ctx
            .gc
            .local_read_i16_at(ctx.default_local_memory_id_unchecked(), start,)))
            as u32),
        Load4Kind::I32Load16U => VMResult::Success(u32::from(vm_try!(ctx
            .gc
            .local_read_u16_at(ctx.default_local_memory_id_unchecked(), start)))),
        Load4Kind::F32 => ctx
            .gc
            .local_read_u32_at(ctx.default_local_memory_id_unchecked(), start),
    }
}

#[inline(always)]
pub(super) unsafe fn read_local_load8_kind(
    ctx: &mut ExecuteContext,
    start: usize,
    kind: Load8Kind,
) -> VMResult<u64> {
    match kind {
        Load8Kind::I64 => ctx
            .gc
            .local_read_u64_at(ctx.default_local_memory_id_unchecked(), start),
        Load8Kind::I64Load8S => VMResult::Success(i64::from(vm_try!(ctx
            .gc
            .local_read_i8_at(ctx.default_local_memory_id_unchecked(), start,)))
            as u64),
        Load8Kind::I64Load8U => VMResult::Success(u64::from(vm_try!(ctx
            .gc
            .local_read_u8_at(ctx.default_local_memory_id_unchecked(), start)))),
        Load8Kind::I64Load16S => VMResult::Success(i64::from(vm_try!(ctx
            .gc
            .local_read_i16_at(ctx.default_local_memory_id_unchecked(), start,)))
            as u64),
        Load8Kind::I64Load16U => VMResult::Success(u64::from(vm_try!(ctx
            .gc
            .local_read_u16_at(ctx.default_local_memory_id_unchecked(), start)))),
        Load8Kind::I64Load32S => VMResult::Success(i64::from(vm_try!(ctx
            .gc
            .local_read_i32_at(ctx.default_local_memory_id_unchecked(), start,)))
            as u64),
        Load8Kind::I64Load32U => VMResult::Success(u64::from(vm_try!(ctx
            .gc
            .local_read_u32_at(ctx.default_local_memory_id_unchecked(), start)))),
        Load8Kind::F64 => ctx
            .gc
            .local_read_u64_at(ctx.default_local_memory_id_unchecked(), start),
    }
}

pub unsafe fn op_i32_load_const_local(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let start = (*tail_code).operand.u32 as usize;
    vm_try!(ctx.gc.local_push_memory_to_stack::<4>(
        ctx.default_local_memory_id_unchecked(),
        ctx.stack,
        start,
    ));
    call_next(tail_code, 1, ctx)
}

pub unsafe fn op_i32_local_get4_store_const_local(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let start = (*tail_code).operand.u32 as usize;
    let local_addr = (*tail_code.add(1)).operand.local_addr;
    let bytes = local_u32(ctx.stack, &ctx.local_reference(), local_addr).to_le_bytes();
    vm_try!(ctx
        .gc
        .local_write_bytes(ctx.default_local_memory_id_unchecked(), start, &bytes));
    call_next(tail_code, 2, ctx)
}

#[inline(always)]
pub(super) unsafe fn local_addr_mem_start(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<usize> {
    let local_addr = (*tail_code).operand.local_addr;
    let memarg = (*tail_code.add(1)).operand.memarg;
    local_mem_start_from_local(ctx, local_addr, memarg)
}

#[inline(always)]
pub(super) unsafe fn local_imm_addr_mem_start(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<usize> {
    let local_addr = (*tail_code).operand.local_addr;
    let imm = (*tail_code.add(1)).operand.i32 as u32;
    let memarg = (*tail_code.add(2)).operand.memarg;
    local_imm_addr_mem_start_from_parts(ctx, local_addr, imm, memarg)
}

#[inline(always)]
pub(super) unsafe fn local_imm_addr_mem_start_from_parts(
    ctx: &mut ExecuteContext,
    local_addr: u32,
    imm: u32,
    memarg: MemArg,
) -> VMResult<usize> {
    compute_memory_offset(
        memarg,
        local_u32(ctx.stack, &ctx.local_reference(), local_addr).wrapping_add(imm),
    )
}

#[inline(always)]
pub(super) unsafe fn local_mem_start_from_local(
    ctx: &mut ExecuteContext,
    local_addr: u32,
    memarg: MemArg,
) -> VMResult<usize> {
    compute_memory_offset(
        memarg,
        local_u32(ctx.stack, &ctx.local_reference(), local_addr),
    )
}

#[inline(always)]
pub unsafe fn op_i32_local_addr_load(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let start = vm_try!(local_addr_mem_start(tail_code, ctx));
    let value = vm_try!(ctx
        .gc
        .local_read_u32_at(ctx.default_local_memory_id_unchecked(), start));
    vm_try!(ctx.stack.push_u32(value));
    call_next(tail_code, 2, ctx)
}

pub unsafe fn op_i32_local_imm_addr_load(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let start = vm_try!(local_imm_addr_mem_start(tail_code, ctx));
    let value = vm_try!(ctx
        .gc
        .local_read_u32_at(ctx.default_local_memory_id_unchecked(), start));
    vm_try!(ctx.stack.push_u32(value));
    call_next(tail_code, 3, ctx)
}

pub unsafe fn op_i32_local_addr_load8_u(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let start = vm_try!(local_addr_mem_start(tail_code, ctx));
    let value = vm_try!(ctx
        .gc
        .local_read_u8_at(ctx.default_local_memory_id_unchecked(), start));
    vm_try!(ctx.stack.push_u32(u32::from(value)));
    call_next(tail_code, 2, ctx)
}

pub unsafe fn op_i32_local_imm_addr_load8_u(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let start = vm_try!(local_imm_addr_mem_start(tail_code, ctx));
    let value = vm_try!(ctx
        .gc
        .local_read_u8_at(ctx.default_local_memory_id_unchecked(), start));
    vm_try!(ctx.stack.push_u32(u32::from(value)));
    call_next(tail_code, 3, ctx)
}

pub unsafe fn op_i32_local_addr_load16_s(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let start = vm_try!(local_addr_mem_start(tail_code, ctx));
    let value = vm_try!(ctx
        .gc
        .local_read_i16_at(ctx.default_local_memory_id_unchecked(), start));
    vm_try!(ctx.stack.push_i32(i32::from(value)));
    call_next(tail_code, 2, ctx)
}

pub unsafe fn op_i32_local_imm_addr_load16_s(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let start = vm_try!(local_imm_addr_mem_start(tail_code, ctx));
    let value = vm_try!(ctx
        .gc
        .local_read_i16_at(ctx.default_local_memory_id_unchecked(), start));
    vm_try!(ctx.stack.push_i32(i32::from(value)));
    call_next(tail_code, 3, ctx)
}

pub unsafe fn op_i32_local_addr_load16_u(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let start = vm_try!(local_addr_mem_start(tail_code, ctx));
    let value = vm_try!(ctx
        .gc
        .local_read_u16_at(ctx.default_local_memory_id_unchecked(), start));
    vm_try!(ctx.stack.push_u32(u32::from(value)));
    call_next(tail_code, 2, ctx)
}

pub unsafe fn op_i32_local_imm_addr_load16_u(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let start = vm_try!(local_imm_addr_mem_start(tail_code, ctx));
    let value = vm_try!(ctx
        .gc
        .local_read_u16_at(ctx.default_local_memory_id_unchecked(), start));
    vm_try!(ctx.stack.push_u32(u32::from(value)));
    call_next(tail_code, 3, ctx)
}

pub unsafe fn op_f32_local_addr_load(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let start = vm_try!(local_addr_mem_start(tail_code, ctx));
    let value = vm_try!(ctx
        .gc
        .local_read_u32_at(ctx.default_local_memory_id_unchecked(), start));
    vm_try!(ctx.stack.push_u32(value));
    call_next(tail_code, 2, ctx)
}

pub unsafe fn op_f32_local_imm_addr_load(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let start = vm_try!(local_imm_addr_mem_start(tail_code, ctx));
    let value = vm_try!(ctx
        .gc
        .local_read_u32_at(ctx.default_local_memory_id_unchecked(), start));
    vm_try!(ctx.stack.push_u32(value));
    call_next(tail_code, 3, ctx)
}

#[inline(always)]
unsafe fn local_local_store_start(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<usize> {
    let addr_local = (*tail_code).operand.local_addr;
    let memarg = (*tail_code.add(2)).operand.memarg;
    compute_memory_offset(
        memarg,
        local_u32(ctx.stack, &ctx.local_reference(), addr_local),
    )
}

#[inline(always)]
unsafe fn local_imm_local_store_start(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<usize> {
    let addr_local = (*tail_code).operand.local_addr;
    let imm = (*tail_code.add(1)).operand.i32 as u32;
    let memarg = (*tail_code.add(3)).operand.memarg;
    compute_memory_offset(
        memarg,
        local_u32(ctx.stack, &ctx.local_reference(), addr_local).wrapping_add(imm),
    )
}

pub unsafe fn op_i32_local_local_store(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let start = vm_try!(local_local_store_start(tail_code, ctx));
    let value_local = (*tail_code.add(1)).operand.local_addr;
    let bytes = local_u32(ctx.stack, &ctx.local_reference(), value_local).to_le_bytes();
    vm_try!(ctx
        .gc
        .local_write_bytes(ctx.default_local_memory_id_unchecked(), start, &bytes));
    call_next(tail_code, 3, ctx)
}

pub unsafe fn op_i32_local_imm_local_store(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let start = vm_try!(local_imm_local_store_start(tail_code, ctx));
    let value_local = (*tail_code.add(2)).operand.local_addr;
    let bytes = local_u32(ctx.stack, &ctx.local_reference(), value_local).to_le_bytes();
    vm_try!(ctx
        .gc
        .local_write_bytes(ctx.default_local_memory_id_unchecked(), start, &bytes));
    call_next(tail_code, 4, ctx)
}

pub unsafe fn op_i32_local_local_store8(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let start = vm_try!(local_local_store_start(tail_code, ctx));
    let value_local = (*tail_code.add(1)).operand.local_addr;
    let bytes = [(local_u32(ctx.stack, &ctx.local_reference(), value_local) & 0xff) as u8];
    vm_try!(ctx
        .gc
        .local_write_bytes(ctx.default_local_memory_id_unchecked(), start, &bytes));
    call_next(tail_code, 3, ctx)
}

pub unsafe fn op_i32_local_imm_local_store8(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let start = vm_try!(local_imm_local_store_start(tail_code, ctx));
    let value_local = (*tail_code.add(2)).operand.local_addr;
    let bytes = [(local_u32(ctx.stack, &ctx.local_reference(), value_local) & 0xff) as u8];
    vm_try!(ctx
        .gc
        .local_write_bytes(ctx.default_local_memory_id_unchecked(), start, &bytes));
    call_next(tail_code, 4, ctx)
}

pub unsafe fn op_i32_local_local_store16(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let start = vm_try!(local_local_store_start(tail_code, ctx));
    let value_local = (*tail_code.add(1)).operand.local_addr;
    let value = local_u32(ctx.stack, &ctx.local_reference(), value_local);
    let bytes = [(value & 0xff) as u8, ((value >> 8) & 0xff) as u8];
    vm_try!(ctx
        .gc
        .local_write_bytes(ctx.default_local_memory_id_unchecked(), start, &bytes));
    call_next(tail_code, 3, ctx)
}

pub unsafe fn op_i32_local_imm_local_store16(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let start = vm_try!(local_imm_local_store_start(tail_code, ctx));
    let value_local = (*tail_code.add(2)).operand.local_addr;
    let value = local_u32(ctx.stack, &ctx.local_reference(), value_local);
    let bytes = [(value & 0xff) as u8, ((value >> 8) & 0xff) as u8];
    vm_try!(ctx
        .gc
        .local_write_bytes(ctx.default_local_memory_id_unchecked(), start, &bytes));
    call_next(tail_code, 4, ctx)
}

#[cold]
#[inline(never)]
pub unsafe fn op_i32_local_local_load_tee_add_imm_store(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let store_addr_local = (*tail_code).operand.local_addr;
    let load_addr_local = (*tail_code.add(1)).operand.local_addr;
    let tee_local = (*tail_code.add(2)).operand.local_addr;
    let imm = (*tail_code.add(3)).operand.i32 as u32;
    let load_memarg = (*tail_code.add(4)).operand.memarg;
    let store_memarg = (*tail_code.add(5)).operand.memarg;
    let load_start = vm_try!(local_mem_start_from_local(
        ctx,
        load_addr_local,
        load_memarg
    ));
    let loaded = vm_try!(ctx
        .gc
        .local_read_u32_at(ctx.default_local_memory_id_unchecked(), load_start));
    write_local_u32(ctx.stack, &ctx.local_reference(), tee_local, loaded);
    let value = loaded.wrapping_add(imm);
    let store_start = vm_try!(local_mem_start_from_local(
        ctx,
        store_addr_local,
        store_memarg
    ));
    let bytes = value.to_le_bytes();
    vm_try!(ctx
        .gc
        .local_write_bytes(ctx.default_local_memory_id_unchecked(), store_start, &bytes));
    call_next(tail_code, 6, ctx)
}

#[cold]
#[inline(never)]
unsafe fn op_i32_local_local_narrow_copy(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    kind: NarrowCopyKind,
) -> VMResult<()> {
    let dst_local = (*tail_code).operand.local_addr;
    let src_local = (*tail_code.add(1)).operand.local_addr;
    let load_memarg = (*tail_code.add(2)).operand.memarg;
    let store_memarg = (*tail_code.add(3)).operand.memarg;
    let load_start = vm_try!(local_mem_start_from_local(ctx, src_local, load_memarg));
    let store_start = vm_try!(local_mem_start_from_local(ctx, dst_local, store_memarg));
    let bytes = match kind {
        NarrowCopyKind::Load8Store8 => StoreBytes::Write1([vm_try!(ctx
            .gc
            .local_read_u8_at(ctx.default_local_memory_id_unchecked(), load_start))]),
        NarrowCopyKind::Load16Store16 => StoreBytes::Write2(
            vm_try!(ctx
                .gc
                .local_read_u16_at(ctx.default_local_memory_id_unchecked(), load_start))
            .to_le_bytes(),
        ),
    };
    vm_try!(ctx.gc.local_write_bytes(
        ctx.default_local_memory_id_unchecked(),
        store_start,
        bytes.as_slice(),
    ));
    call_next(tail_code, 4, ctx)
}

#[cold]
#[inline(never)]
pub unsafe fn op_i32_local_local_load8_u_store8(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_i32_local_local_narrow_copy(tail_code, ctx, NarrowCopyKind::Load8Store8)
}

#[cold]
#[inline(never)]
pub unsafe fn op_i32_local_local_load16_u_store16(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_i32_local_local_narrow_copy(tail_code, ctx, NarrowCopyKind::Load16Store16)
}

pub unsafe fn op_load_const_local4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let start = (*tail_code).operand.u32 as usize;
    let kind = Load4Kind::from_raw((*tail_code.add(1)).operand.u32);
    match kind {
        Load4Kind::I32 => vm_try!(ctx.stack.push_u32(vm_try!(ctx
            .gc
            .local_read_u32_at(ctx.default_local_memory_id_unchecked(), start)))),
        Load4Kind::I32Load8S => vm_try!(ctx.stack.push_i32(i32::from(vm_try!(ctx
            .gc
            .local_read_i8_at(ctx.default_local_memory_id_unchecked(), start))))),
        Load4Kind::I32Load8U => vm_try!(ctx.stack.push_u32(u32::from(vm_try!(ctx
            .gc
            .local_read_u8_at(ctx.default_local_memory_id_unchecked(), start))))),
        Load4Kind::I32Load16S => vm_try!(ctx.stack.push_i32(i32::from(vm_try!(ctx
            .gc
            .local_read_i16_at(ctx.default_local_memory_id_unchecked(), start))))),
        Load4Kind::I32Load16U => vm_try!(ctx.stack.push_u32(u32::from(vm_try!(ctx
            .gc
            .local_read_u16_at(ctx.default_local_memory_id_unchecked(), start))))),
        Load4Kind::F32 => vm_try!(ctx.stack.push_u32(vm_try!(ctx
            .gc
            .local_read_u32_at(ctx.default_local_memory_id_unchecked(), start)))),
    }
    call_next(tail_code, 2, ctx)
}

pub unsafe fn op_load_const_local8(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let start = (*tail_code).operand.u32 as usize;
    let kind = Load8Kind::from_raw((*tail_code.add(1)).operand.u32);
    match kind {
        Load8Kind::I64 => vm_try!(ctx.stack.push_u64(vm_try!(ctx
            .gc
            .local_read_u64_at(ctx.default_local_memory_id_unchecked(), start)))),
        Load8Kind::I64Load8S => vm_try!(ctx.stack.push_i64(i64::from(vm_try!(ctx
            .gc
            .local_read_i8_at(ctx.default_local_memory_id_unchecked(), start))))),
        Load8Kind::I64Load8U => vm_try!(ctx.stack.push_u64(u64::from(vm_try!(ctx
            .gc
            .local_read_u8_at(ctx.default_local_memory_id_unchecked(), start))))),
        Load8Kind::I64Load16S => vm_try!(ctx.stack.push_i64(i64::from(vm_try!(ctx
            .gc
            .local_read_i16_at(ctx.default_local_memory_id_unchecked(), start))))),
        Load8Kind::I64Load16U => vm_try!(ctx.stack.push_u64(u64::from(vm_try!(ctx
            .gc
            .local_read_u16_at(ctx.default_local_memory_id_unchecked(), start))))),
        Load8Kind::I64Load32S => vm_try!(ctx.stack.push_i64(i64::from(vm_try!(ctx
            .gc
            .local_read_i32_at(ctx.default_local_memory_id_unchecked(), start))))),
        Load8Kind::I64Load32U => vm_try!(ctx.stack.push_u64(u64::from(vm_try!(ctx
            .gc
            .local_read_u32_at(ctx.default_local_memory_id_unchecked(), start))))),
        Load8Kind::F64 => vm_try!(ctx.stack.push_u64(vm_try!(ctx
            .gc
            .local_read_u64_at(ctx.default_local_memory_id_unchecked(), start)))),
    }
    call_next(tail_code, 2, ctx)
}

pub unsafe fn op_local_addr_load4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let start = vm_try!(local_addr_mem_start(tail_code, ctx));
    let kind = Load4Kind::from_raw((*tail_code.add(2)).operand.u32);
    match kind {
        Load4Kind::I32 => vm_try!(ctx.stack.push_u32(vm_try!(ctx
            .gc
            .local_read_u32_at(ctx.default_local_memory_id_unchecked(), start)))),
        Load4Kind::I32Load8S => vm_try!(ctx.stack.push_i32(i32::from(vm_try!(ctx
            .gc
            .local_read_i8_at(ctx.default_local_memory_id_unchecked(), start))))),
        Load4Kind::I32Load8U => vm_try!(ctx.stack.push_u32(u32::from(vm_try!(ctx
            .gc
            .local_read_u8_at(ctx.default_local_memory_id_unchecked(), start))))),
        Load4Kind::I32Load16S => vm_try!(ctx.stack.push_i32(i32::from(vm_try!(ctx
            .gc
            .local_read_i16_at(ctx.default_local_memory_id_unchecked(), start))))),
        Load4Kind::I32Load16U => vm_try!(ctx.stack.push_u32(u32::from(vm_try!(ctx
            .gc
            .local_read_u16_at(ctx.default_local_memory_id_unchecked(), start))))),
        Load4Kind::F32 => vm_try!(ctx.stack.push_u32(vm_try!(ctx
            .gc
            .local_read_u32_at(ctx.default_local_memory_id_unchecked(), start)))),
    }
    call_next(tail_code, 3, ctx)
}

pub unsafe fn op_local_addr_load8(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let start = vm_try!(local_addr_mem_start(tail_code, ctx));
    let kind = Load8Kind::from_raw((*tail_code.add(2)).operand.u32);
    match kind {
        Load8Kind::I64 => vm_try!(ctx.stack.push_u64(vm_try!(ctx
            .gc
            .local_read_u64_at(ctx.default_local_memory_id_unchecked(), start)))),
        Load8Kind::I64Load8S => vm_try!(ctx.stack.push_i64(i64::from(vm_try!(ctx
            .gc
            .local_read_i8_at(ctx.default_local_memory_id_unchecked(), start))))),
        Load8Kind::I64Load8U => vm_try!(ctx.stack.push_u64(u64::from(vm_try!(ctx
            .gc
            .local_read_u8_at(ctx.default_local_memory_id_unchecked(), start))))),
        Load8Kind::I64Load16S => vm_try!(ctx.stack.push_i64(i64::from(vm_try!(ctx
            .gc
            .local_read_i16_at(ctx.default_local_memory_id_unchecked(), start))))),
        Load8Kind::I64Load16U => vm_try!(ctx.stack.push_u64(u64::from(vm_try!(ctx
            .gc
            .local_read_u16_at(ctx.default_local_memory_id_unchecked(), start))))),
        Load8Kind::I64Load32S => vm_try!(ctx.stack.push_i64(i64::from(vm_try!(ctx
            .gc
            .local_read_i32_at(ctx.default_local_memory_id_unchecked(), start))))),
        Load8Kind::I64Load32U => vm_try!(ctx.stack.push_u64(u64::from(vm_try!(ctx
            .gc
            .local_read_u32_at(ctx.default_local_memory_id_unchecked(), start))))),
        Load8Kind::F64 => vm_try!(ctx.stack.push_u64(vm_try!(ctx
            .gc
            .local_read_u64_at(ctx.default_local_memory_id_unchecked(), start)))),
    }
    call_next(tail_code, 3, ctx)
}

pub unsafe fn op_local_store_const_local4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let start = (*tail_code).operand.u32 as usize;
    let value_local = (*tail_code.add(1)).operand.local_addr;
    let kind = Store4Kind::from_raw((*tail_code.add(2)).operand.u32);
    let bytes = store4_bytes(
        local_u32(ctx.stack, &ctx.local_reference(), value_local),
        kind,
    );
    vm_try!(ctx.gc.local_write_bytes(
        ctx.default_local_memory_id_unchecked(),
        start,
        bytes.as_slice()
    ));
    call_next(tail_code, 3, ctx)
}

pub unsafe fn op_local_store_const_local8(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let start = (*tail_code).operand.u32 as usize;
    let value_local = (*tail_code.add(1)).operand.local_addr;
    let kind = Store8Kind::from_raw((*tail_code.add(2)).operand.u32);
    let bytes = store8_bytes(
        local_u64(ctx.stack, &ctx.local_reference(), value_local),
        kind,
    );
    vm_try!(ctx.gc.local_write_bytes(
        ctx.default_local_memory_id_unchecked(),
        start,
        bytes.as_slice()
    ));
    call_next(tail_code, 3, ctx)
}

pub unsafe fn op_local_local_store4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let start = vm_try!(local_local_store_start(tail_code, ctx));
    let value_local = (*tail_code.add(1)).operand.local_addr;
    let kind = Store4Kind::from_raw((*tail_code.add(3)).operand.u32);
    let bytes = store4_bytes(
        local_u32(ctx.stack, &ctx.local_reference(), value_local),
        kind,
    );
    vm_try!(ctx.gc.local_write_bytes(
        ctx.default_local_memory_id_unchecked(),
        start,
        bytes.as_slice()
    ));
    call_next(tail_code, 4, ctx)
}

pub unsafe fn op_local_local_store8(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let start = vm_try!(local_local_store_start(tail_code, ctx));
    let value_local = (*tail_code.add(1)).operand.local_addr;
    let kind = Store8Kind::from_raw((*tail_code.add(3)).operand.u32);
    let bytes = store8_bytes(
        local_u64(ctx.stack, &ctx.local_reference(), value_local),
        kind,
    );
    vm_try!(ctx.gc.local_write_bytes(
        ctx.default_local_memory_id_unchecked(),
        start,
        bytes.as_slice()
    ));
    call_next(tail_code, 4, ctx)
}
