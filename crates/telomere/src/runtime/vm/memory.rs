use super::*;
use crate::common::{decode_local_cmp32_kind, LocalCmp32Op, LocalFastRhsShape, Op};

#[cfg(feature = "vm-profile")]
#[cold]
#[inline(never)]
fn profile_memory_family_enabled(label: &'static str) {
    dispatch_profile_count(label);
}

#[inline(always)]
fn profile_memory_family(_label: &'static str) {
    #[cfg(feature = "vm-profile")]
    if dispatch_profile_enabled() {
        profile_memory_family_enabled(_label);
    }
}

#[inline(always)]
unsafe fn memory_br_if_ptr(
    tail_code: *const Instr,
    target_offset: usize,
    taken_advance: usize,
    cond: u32,
    ctx: &mut ExecuteContext,
) -> *const Instr {
    let ptr = if cond != 0 {
        let jump_addr = (*tail_code.add(target_offset)).operand.jump_addr;
        ctx.code().offset(jump_addr as isize)
    } else {
        tail_code.add(taken_advance)
    };
    skip_end_ops(ptr)
}

#[inline(always)]
unsafe fn skip_end_ops(mut ptr: *const Instr) -> *const Instr {
    while std::ptr::fn_addr_eq((*ptr).op, op_end as Op) {
        ptr = ptr.add(1);
    }
    ptr
}

#[inline(always)]
unsafe fn br_table_target_ptr_at(
    tail_code: *const Instr,
    table_size_offset: isize,
    first_target_offset: isize,
    index: u32,
    ctx: &mut ExecuteContext,
) -> *const Instr {
    let table_size = (*tail_code.offset(table_size_offset)).operand.u32;
    let target_offset = if index < table_size {
        first_target_offset + index as isize
    } else {
        first_target_offset + table_size as isize
    };
    let addr = (*tail_code.offset(target_offset)).operand.jump_addr;
    skip_end_ops(ctx.code().offset(addr as isize))
}

#[inline(always)]
fn i32_const_compare(kind: u32, lhs: i32, rhs: i32) -> bool {
    match kind {
        0 => lhs == rhs,
        1 => lhs != rhs,
        2 => lhs < rhs,
        3 => (lhs as u32) < (rhs as u32),
        4 => lhs > rhs,
        5 => (lhs as u32) > (rhs as u32),
        6 => lhs <= rhs,
        7 => (lhs as u32) <= (rhs as u32),
        8 => lhs >= rhs,
        9 => (lhs as u32) >= (rhs as u32),
        _ => false,
    }
}

#[inline(always)]
unsafe fn read_u8_linear(ctx: &mut ExecuteContext, addr: u32) -> VMResult<u8> {
    let start = addr as usize;
    let memory = unsafe { ctx.default_local_memory_unchecked() };
    if start >= memory.data_size() {
        return VMResult::MemoryIndexOutOfRange;
    }
    VMResult::Success(unsafe { *memory.data_ptr().add(start) })
}

#[inline(always)]
unsafe fn read_u32_linear(ctx: &mut ExecuteContext, addr: u32) -> VMResult<u32> {
    let start = addr as usize;
    let Some(end) = start.checked_add(4) else {
        return VMResult::MemoryIndexOutOfRange;
    };
    let memory = unsafe { ctx.default_local_memory_unchecked() };
    if end > memory.data_size() {
        return VMResult::MemoryIndexOutOfRange;
    }
    VMResult::Success(u32::from_le(unsafe {
        memory.data_ptr().add(start).cast::<u32>().read_unaligned()
    }))
}

#[inline(always)]
unsafe fn read_u16_linear(ctx: &mut ExecuteContext, addr: u32) -> VMResult<u16> {
    let start = addr as usize;
    let Some(end) = start.checked_add(2) else {
        return VMResult::MemoryIndexOutOfRange;
    };
    let memory = unsafe { ctx.default_local_memory_unchecked() };
    if end > memory.data_size() {
        return VMResult::MemoryIndexOutOfRange;
    }
    VMResult::Success(u16::from_le(unsafe {
        memory.data_ptr().add(start).cast::<u16>().read_unaligned()
    }))
}

#[inline(always)]
unsafe fn read_i16_linear(ctx: &mut ExecuteContext, addr: u32) -> VMResult<i32> {
    VMResult::Success(i32::from(
        vm_try!(unsafe { read_u16_linear(ctx, addr) }) as i16
    ))
}

#[inline(always)]
unsafe fn write_u32_linear(ctx: &mut ExecuteContext, addr: u32, value: u32) -> VMResult<()> {
    let start = addr as usize;
    let Some(end) = start.checked_add(4) else {
        return VMResult::MemoryIndexOutOfRange;
    };
    let memory = unsafe { ctx.default_local_memory_mut_unchecked() };
    if end > memory.data_size() {
        return VMResult::MemoryIndexOutOfRange;
    }
    unsafe {
        memory
            .data_mut_ptr()
            .add(start)
            .cast::<u32>()
            .write_unaligned(value.to_le());
    }
    VMResult::Success(())
}

#[inline(always)]
unsafe fn write_u16_linear(ctx: &mut ExecuteContext, addr: u32, value: u16) -> VMResult<()> {
    let start = addr as usize;
    let Some(end) = start.checked_add(2) else {
        return VMResult::MemoryIndexOutOfRange;
    };
    let memory = unsafe { ctx.default_local_memory_mut_unchecked() };
    if end > memory.data_size() {
        return VMResult::MemoryIndexOutOfRange;
    }
    unsafe {
        memory
            .data_mut_ptr()
            .add(start)
            .cast::<u16>()
            .write_unaligned(value.to_le());
    }
    VMResult::Success(())
}

#[inline(always)]
unsafe fn write_u8_linear(ctx: &mut ExecuteContext, addr: u32, value: u8) -> VMResult<()> {
    let start = addr as usize;
    let memory = unsafe { ctx.default_local_memory_mut_unchecked() };
    if start >= memory.data_size() {
        return VMResult::MemoryIndexOutOfRange;
    }
    unsafe {
        *memory.data_mut_ptr().add(start) = value;
    }
    VMResult::Success(())
}

#[inline(always)]
unsafe fn inc_u32_counter(ctx: &mut ExecuteContext, base: u32, state: u32) -> VMResult<()> {
    let addr = base.wrapping_add(state.wrapping_mul(4));
    let value = vm_try!(unsafe { read_u32_linear(ctx, addr) }).wrapping_add(1);
    unsafe { write_u32_linear(ctx, addr, value) }
}

#[inline(always)]
fn inc_counter_array(counts: &mut [u32; 8], state: u32) {
    counts[state as usize] = counts[state as usize].wrapping_add(1);
}

#[inline(always)]
fn is_coremark_digit(ch: u8) -> bool {
    ch.wrapping_sub(b'0') <= 9
}

#[inline(always)]
fn is_coremark_sign(ch: u8) -> bool {
    ch == b'+' || ch == b'-'
}

#[inline(always)]
fn local_u64_bits(ctx: &mut ExecuteContext, addr: usize) -> u64 {
    unsafe {
        u64::from_le(
            (ctx.local_base_ptr as *const u8)
                .add(addr)
                .cast::<u64>()
                .read_unaligned(),
        )
    }
}

#[inline(always)]
fn truncate_u32_to_u8_bytes(value: u32) -> [u8; 1] {
    [(value & 0xff) as u8]
}

#[inline(always)]
fn truncate_u32_to_u16_bytes(value: u32) -> [u8; 2] {
    [(value & 0xff) as u8, ((value >> 8) & 0xff) as u8]
}

#[inline(always)]
fn truncate_u64_to_u8_bytes(value: u64) -> [u8; 1] {
    [(value & 0xff) as u8]
}

#[inline(always)]
fn truncate_u64_to_u16_bytes(value: u64) -> [u8; 2] {
    [(value & 0xff) as u8, ((value >> 8) & 0xff) as u8]
}

#[inline(always)]
fn truncate_u64_to_u32_bytes(value: u64) -> [u8; 4] {
    [
        (value & 0xff) as u8,
        ((value >> 8) & 0xff) as u8,
        ((value >> 16) & 0xff) as u8,
        ((value >> 24) & 0xff) as u8,
    ]
}

#[inline(always)]
/// WebAssembly linear-memory access helper.
///
/// Spec:
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: consumes the address operand and computes an effective memory offset.
/// Traps: traps on memory index overflow when computing the effective address.
/// Notes: Reads the memarg from the active instruction and reuses the validated operand stack layout.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction stream for the current active frame.
/// - `ctx` must reference a live execution context whose validated stack layout matches this memory instruction.
/// - This helper must not retain borrows across the call boundary into memory access helpers.
unsafe fn load_start(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<usize> {
    debug_assert!(ctx.snapshot().has_default_memory());
    let memarg = (*tail_code).operand.memarg;
    let offset = ctx.stack.pop_u32_fast();
    trace!("memory access: {:?} {}", memarg, offset);
    compute_memory_offset(memarg, offset)
}

#[inline(always)]
unsafe fn default_local_push_to_stack<const N: usize>(
    ctx: &mut ExecuteContext,
    offset: usize,
) -> VMResult<()> {
    debug_assert!(!ctx.default_local_memory_ptr.is_null());
    unsafe { (&*ctx.default_local_memory_ptr).push_to_stack::<N>(ctx.stack, offset) }
}

#[inline(always)]
unsafe fn load_start_indexed(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<(usize, u32)> {
    let memarg = (*tail_code).operand.memarg;
    let memidx = (*tail_code.add(1)).operand.u32;
    let offset = ctx.stack.pop_u32_fast();
    trace!(
        "indexed memory access: {:?} {} memidx={}",
        memarg,
        offset,
        memidx
    );
    let start = vm_try!(compute_memory_offset(memarg, offset));
    VMResult::Success((start, memidx))
}

#[inline(always)]
unsafe fn load_start_local_base(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<usize> {
    debug_assert!(ctx.snapshot().has_default_memory());
    let local_addr = (*tail_code).operand.local_addr as usize;
    let delta = (*tail_code.add(1)).operand.i32 as u32;
    let memarg = (*tail_code.add(2)).operand.memarg;
    let offset = ctx
        .stack
        .local_u32_from_base(ctx.local_base_ptr as *const u8, local_addr)
        .wrapping_add(delta);
    compute_memory_offset(memarg, offset)
}

#[inline(always)]
unsafe fn load_start_indexed_local_base(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<(usize, u32)> {
    let local_addr = (*tail_code).operand.local_addr as usize;
    let delta = (*tail_code.add(1)).operand.i32 as u32;
    let memarg = (*tail_code.add(2)).operand.memarg;
    let memidx = (*tail_code.add(3)).operand.u32;
    let offset = ctx
        .stack
        .local_u32_from_base(ctx.local_base_ptr as *const u8, local_addr)
        .wrapping_add(delta);
    let start = vm_try!(compute_memory_offset(memarg, offset));
    VMResult::Success((start, memidx))
}

#[inline(always)]
unsafe fn load_start_local_scaled_index(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<usize> {
    let base_addr = (*tail_code).operand.local_addr as usize;
    let index_addr = (*tail_code.add(1)).operand.local_addr as usize;
    let scale_log2 = (*tail_code.add(2)).operand.u32;
    let delta = (*tail_code.add(3)).operand.i32 as u32;
    let memarg = (*tail_code.add(4)).operand.memarg;
    debug_assert!(scale_log2 <= 3);
    let base = ctx
        .stack
        .local_u32_from_base(ctx.local_base_ptr as *const u8, base_addr);
    let index = ctx
        .stack
        .local_u32_from_base(ctx.local_base_ptr as *const u8, index_addr);
    let offset = base
        .wrapping_add(index.wrapping_shl(scale_log2))
        .wrapping_add(delta);
    compute_memory_offset(memarg, offset)
}

#[inline(always)]
unsafe fn load_start_indexed_local_scaled_index(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<(usize, u32)> {
    let base_addr = (*tail_code).operand.local_addr as usize;
    let index_addr = (*tail_code.add(1)).operand.local_addr as usize;
    let scale_log2 = (*tail_code.add(2)).operand.u32;
    let delta = (*tail_code.add(3)).operand.i32 as u32;
    let memarg = (*tail_code.add(4)).operand.memarg;
    let memidx = (*tail_code.add(5)).operand.u32;
    debug_assert!(scale_log2 <= 3);
    let base = ctx
        .stack
        .local_u32_from_base(ctx.local_base_ptr as *const u8, base_addr);
    let index = ctx
        .stack
        .local_u32_from_base(ctx.local_base_ptr as *const u8, index_addr);
    let offset = base
        .wrapping_add(index.wrapping_shl(scale_log2))
        .wrapping_add(delta);
    let start = vm_try!(compute_memory_offset(memarg, offset));
    VMResult::Success((start, memidx))
}

#[inline(always)]
unsafe fn load_start_shared_local_scaled_index(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<usize> {
    let base_addr = (*tail_code).operand.local_addr as usize;
    let index_addr = (*tail_code.add(1)).operand.local_addr as usize;
    let scale_log2 = (*tail_code.add(2)).operand.u32;
    let delta = (*tail_code.add(3)).operand.i32 as u32;
    let memarg = (*tail_code.add(4)).operand.memarg;
    debug_assert!(scale_log2 <= 3);
    let base = ctx
        .stack
        .local_u32_from_base(ctx.local_base_ptr as *const u8, base_addr);
    let index = ctx
        .stack
        .local_u32_from_base(ctx.local_base_ptr as *const u8, index_addr);
    let offset = base
        .wrapping_add(index.wrapping_shl(scale_log2))
        .wrapping_add(delta);
    compute_memory_offset(memarg, offset)
}

#[inline(always)]
unsafe fn load_start_indexed_shared_local_scaled_index(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<(usize, u32)> {
    let base_addr = (*tail_code).operand.local_addr as usize;
    let index_addr = (*tail_code.add(1)).operand.local_addr as usize;
    let scale_log2 = (*tail_code.add(2)).operand.u32;
    let delta = (*tail_code.add(3)).operand.i32 as u32;
    let memarg = (*tail_code.add(4)).operand.memarg;
    let memidx = (*tail_code.add(5)).operand.u32;
    debug_assert!(scale_log2 <= 3);
    let base = ctx
        .stack
        .local_u32_from_base(ctx.local_base_ptr as *const u8, base_addr);
    let index = ctx
        .stack
        .local_u32_from_base(ctx.local_base_ptr as *const u8, index_addr);
    let offset = base
        .wrapping_add(index.wrapping_shl(scale_log2))
        .wrapping_add(delta);
    let start = vm_try!(compute_memory_offset(memarg, offset));
    VMResult::Success((start, memidx))
}

#[inline(always)]
unsafe fn read_i32_load16_s_default(
    ctx: &mut ExecuteContext,
    memarg: MemArg,
    offset: u32,
) -> VMResult<i32> {
    let start = vm_try!(compute_memory_offset(memarg, offset));
    VMResult::Success(i32::from(vm_try!(unsafe {
        ctx.default_local_memory_unchecked()
    }
    .read_i16_at(start))))
}

#[inline(always)]
unsafe fn read_i32_scalar_default_at(
    ctx: &mut ExecuteContext,
    load_kind: u32,
    start: usize,
) -> VMResult<u32> {
    let memory = unsafe { ctx.default_local_memory_unchecked() };
    match load_kind {
        0 => VMResult::Success(vm_try!(memory.read_u32_at(start))),
        1 => VMResult::Success(i32::from(vm_try!(memory.read_i8_at(start))) as u32),
        2 => VMResult::Success(u32::from(vm_try!(memory.read_u8_at(start)))),
        3 => VMResult::Success(i32::from(vm_try!(memory.read_i16_at(start))) as u32),
        4 => VMResult::Success(u32::from(vm_try!(memory.read_u16_at(start)))),
        _ => VMResult::InvalidOperand,
    }
}

#[inline(always)]
unsafe fn write_i32_scalar_default_at(
    ctx: &mut ExecuteContext,
    store_kind: u32,
    start: usize,
    value: u32,
) -> VMResult<()> {
    let memory = unsafe { ctx.default_local_memory_mut_unchecked() };
    match store_kind {
        0 => memory.write_u32_at(start, value),
        1 => memory.write_bytes(start, &truncate_u32_to_u8_bytes(value)),
        2 => memory.write_bytes(start, &truncate_u32_to_u16_bytes(value)),
        _ => VMResult::InvalidOperand,
    }
}

#[inline(always)]
unsafe fn copy_scalar_default_at(
    ctx: &mut ExecuteContext,
    width: u32,
    src_start: usize,
    dst_start: usize,
) -> VMResult<()> {
    match width {
        1 => {
            let value =
                vm_try!(unsafe { ctx.default_local_memory_unchecked() }.read_u8_at(src_start));
            unsafe { ctx.default_local_memory_mut_unchecked() }.write_bytes(dst_start, &[value])
        }
        2 => {
            let value =
                vm_try!(unsafe { ctx.default_local_memory_unchecked() }.read_u16_at(src_start));
            unsafe { ctx.default_local_memory_mut_unchecked() }
                .write_bytes(dst_start, &value.to_le_bytes())
        }
        4 => {
            let value =
                vm_try!(unsafe { ctx.default_local_memory_unchecked() }.read_u32_at(src_start));
            unsafe { ctx.default_local_memory_mut_unchecked() }.write_u32_at(dst_start, value)
        }
        8 => {
            let value =
                vm_try!(unsafe { ctx.default_local_memory_unchecked() }.read_u64_at(src_start));
            unsafe { ctx.default_local_memory_mut_unchecked() }
                .write_bytes(dst_start, &value.to_le_bytes())
        }
        _ => VMResult::InvalidOperand,
    }
}

#[inline(always)]
fn compare_i32_scalar(kind: u32, lhs: u32, rhs: u32) -> u32 {
    let matched = match kind {
        0 => lhs == rhs,
        1 => lhs != rhs,
        2 => (lhs as i32) < (rhs as i32),
        3 => lhs < rhs,
        4 => (lhs as i32) > (rhs as i32),
        5 => lhs > rhs,
        6 => (lhs as i32) <= (rhs as i32),
        7 => lhs <= rhs,
        8 => (lhs as i32) >= (rhs as i32),
        9 => lhs >= rhs,
        _ => false,
    };
    matched as u32
}

#[inline(always)]
#[allow(dead_code)]
unsafe fn store_internal_local_base(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    _label: &'static str,
    make_operation: impl FnOnce(&mut ExecuteContext) -> StoreBytes,
) -> VMResult<()> {
    let local_addr = (*tail_code).operand.local_addr as usize;
    let delta = (*tail_code.add(1)).operand.i32 as u32;
    let memarg = (*tail_code.add(2)).operand.memarg;
    let operation = make_operation(ctx);
    let offset = ctx
        .stack
        .local_u32_from_base(ctx.local_base_ptr as *const u8, local_addr)
        .wrapping_add(delta);
    let bytes = operation.as_slice();
    let start = vm_try!(compute_memory_offset(memarg, offset));
    vm_try!(unsafe { ctx.default_local_memory_mut_unchecked() }.write_bytes(start, bytes));
    call_next(tail_code, 3, ctx)
}

#[inline(always)]
#[allow(dead_code)]
unsafe fn store_internal_indexed_local_base(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    _label: &'static str,
    make_operation: impl FnOnce(&mut ExecuteContext) -> StoreBytes,
) -> VMResult<()> {
    let local_addr = (*tail_code).operand.local_addr as usize;
    let delta = (*tail_code.add(1)).operand.i32 as u32;
    let memarg = (*tail_code.add(2)).operand.memarg;
    let memidx = (*tail_code.add(3)).operand.u32;
    let operation = make_operation(ctx);
    let offset = ctx
        .stack
        .local_u32_from_base(ctx.local_base_ptr as *const u8, local_addr)
        .wrapping_add(delta);
    let bytes = operation.as_slice();
    let start = vm_try!(compute_memory_offset(memarg, offset));
    vm_try!(ctx
        .gc
        .local_write_bytes(ctx.local_memory_id_at_unchecked(memidx), start, bytes));
    call_next(tail_code, 4, ctx)
}

#[allow(dead_code)]
/// WebAssembly `i32.store` with a local-base address and local.get value on default local memory.
///
/// # Safety
/// - `tail_code` must point to operands decoded for this fused memory handler.
/// - `ctx` must hold a valid frame, local base, and default memory for the active module.
pub unsafe fn op_i32_store_local_base_local_get4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    profile_memory_family("op_i32_store_local_base_local_get4");
    let addr_local = (*tail_code).operand.local_addr as usize;
    let delta = (*tail_code.add(1)).operand.i32 as u32;
    let value_local = (*tail_code.add(2)).operand.local_addr as usize;
    let memarg = (*tail_code.add(3)).operand.memarg;
    let local_base = ctx.local_base_ptr as *const u8;
    let offset = ctx
        .stack
        .local_u32_from_base(local_base, addr_local)
        .wrapping_add(delta);
    let value = ctx.stack.local_u32_from_base(local_base, value_local);
    let start = vm_try!(compute_memory_offset(memarg, offset));
    vm_try!(unsafe { ctx.default_local_memory_mut_unchecked() }.write_u32_at(start, value));
    call_next(tail_code, 4, ctx)
}

#[allow(dead_code)]
/// WebAssembly `i32.load*` followed by an independent local-base `i32.store*`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[addr] -> [loaded_i32]`.
/// Traps: preserves the load before store trap order and uses validated memory operands.
/// Notes: Generic fusion for `i32.load*; local.get addr; local.get value; i32.store*` when the store value is a local.
///
/// # Safety
/// - `tail_code` must point to operands decoded for this fused memory handler.
/// - `ctx` must hold a valid frame, local base, and default memory for the active module.
/// - This handler must not keep borrows, locks, or guards alive across `call_next`.
pub unsafe fn op_i32_load_store_local_base_local_get4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    profile_memory_family("op_i32_load_store_local_base_local_get4");
    let kind = (*tail_code).operand.u32;
    let load_kind = kind & 0xff;
    let store_kind = (kind >> 8) & 0xff;
    let load_memarg = (*tail_code.add(1)).operand.memarg;
    let store_addr_local = (*tail_code.add(2)).operand.local_addr as usize;
    let store_delta = (*tail_code.add(3)).operand.i32 as u32;
    let value_local = (*tail_code.add(4)).operand.local_addr as usize;
    let store_memarg = (*tail_code.add(5)).operand.memarg;
    let skip_slots = (*tail_code.add(6)).operand.u32 as isize;

    let load_offset = ctx.stack.pop_u32_fast();
    let load_start = vm_try!(compute_memory_offset(load_memarg, load_offset));
    let loaded = vm_try!(read_i32_scalar_default_at(ctx, load_kind, load_start));
    vm_try!(ctx.stack.push_u32_fast(loaded));

    let local_base = ctx.local_base_ptr as *const u8;
    let store_offset = ctx
        .stack
        .local_u32_from_base(local_base, store_addr_local)
        .wrapping_add(store_delta);
    let value = ctx.stack.local_u32_from_base(local_base, value_local);
    let store_start = vm_try!(compute_memory_offset(store_memarg, store_offset));
    vm_try!(write_i32_scalar_default_at(
        ctx,
        store_kind,
        store_start,
        value
    ));
    call_next(tail_code, skip_slots, ctx)
}

#[allow(dead_code)]
/// WebAssembly repeated scalar copy from one local-base address stream to another.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Notes: Generic fusion for repeated `local.get dst; local.get src; load; store` scalar copy lanes.
///
/// # Safety
/// - `tail_code` must point to operands decoded for this copy-run fusion.
/// - `ctx` must hold a valid frame, local base, and default memory for the active module.
/// - The handler preserves per-lane load-before-store order and must not keep memory guards across `call_next`.
pub unsafe fn op_scalar_copy_local_base_run(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    profile_memory_family("op_scalar_copy_local_base_run");
    let kind = (*tail_code).operand.u32;
    let width = kind & 0xff;
    let count = (kind >> 8) & 0xff;
    let dst_base_local = (*tail_code.add(1)).operand.local_addr as usize;
    let src_base_local = (*tail_code.add(2)).operand.local_addr as usize;
    let local_base = ctx.local_base_ptr as *const u8;
    let dst_base = ctx.stack.local_u32_from_base(local_base, dst_base_local);
    let src_base = ctx.stack.local_u32_from_base(local_base, src_base_local);

    let mut operand_offset = 3usize;
    for _ in 0..count {
        let dst_delta = (*tail_code.add(operand_offset)).operand.i32 as u32;
        let src_delta = (*tail_code.add(operand_offset + 1)).operand.i32 as u32;
        let load_memarg = (*tail_code.add(operand_offset + 2)).operand.memarg;
        let store_memarg = (*tail_code.add(operand_offset + 3)).operand.memarg;
        let src_offset = src_base.wrapping_add(src_delta);
        let src_start = vm_try!(compute_memory_offset(load_memarg, src_offset));
        let dst_offset = dst_base.wrapping_add(dst_delta);
        let dst_start = vm_try!(compute_memory_offset(store_memarg, dst_offset));
        vm_try!(copy_scalar_default_at(ctx, width, src_start, dst_start));
        operand_offset += 4;
    }

    call_next(tail_code, operand_offset as isize, ctx)
}

#[allow(dead_code)]
/// WebAssembly local-base `i32.load*`, local-address `i32.load*`, `i32` compare, and `br_if`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[] -> []`.
/// Traps: preserves first-load before second-load trap order and branches only to validated continuations.
/// Notes: Generic fusion for two scalar i32 memory loads with optional local.tee side effects and an immediate branch.
///
/// # Safety
/// - `tail_code` must point to operands decoded for this fused memory/compare/branch handler.
/// - `ctx` must hold a valid frame, local base, default memory, and branch target for the active module.
/// - This handler must not keep borrows, locks, or guards alive across `call_next`.
pub unsafe fn op_i32_load_local_base_local_get4_i32_load_tee4_cmp_br_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    profile_memory_family("op_i32_load_local_base_local_get4_i32_load_tee4_cmp_br_if");
    let kind = (*tail_code).operand.u32;
    let first_load_kind = kind & 0xff;
    let second_load_kind = (kind >> 8) & 0xff;
    let compare_kind = (kind >> 16) & 0xff;
    let first_base_local = (*tail_code.add(1)).operand.local_addr as usize;
    let first_delta = (*tail_code.add(2)).operand.i32 as u32;
    let first_memarg = (*tail_code.add(3)).operand.memarg;
    let first_dst = (*tail_code.add(4)).operand.local_addr as usize;
    let second_addr_local = (*tail_code.add(5)).operand.local_addr as usize;
    let second_memarg = (*tail_code.add(6)).operand.memarg;
    let second_dst = (*tail_code.add(7)).operand.local_addr as usize;
    let local_base = ctx.local_base_ptr as *const u8;

    let first_offset = ctx
        .stack
        .local_u32_from_base(local_base, first_base_local)
        .wrapping_add(first_delta);
    let first_start = vm_try!(compute_memory_offset(first_memarg, first_offset));
    let first = vm_try!(read_i32_scalar_default_at(
        ctx,
        first_load_kind,
        first_start
    ));
    ctx.stack
        .local_set4_from_base_value(ctx.local_base_ptr, first_dst, first);

    let second_offset = ctx.stack.local_u32_from_base(local_base, second_addr_local);
    let second_start = vm_try!(compute_memory_offset(second_memarg, second_offset));
    let second = vm_try!(read_i32_scalar_default_at(
        ctx,
        second_load_kind,
        second_start
    ));
    ctx.stack
        .local_set4_from_base_value(ctx.local_base_ptr, second_dst, second);

    let ptr = memory_br_if_ptr(
        tail_code,
        8,
        9,
        compare_i32_scalar(compare_kind, first, second),
        ctx,
    );
    call_next(ptr, 0, ctx)
}

#[allow(dead_code)]
/// WebAssembly `local.tee; i32.load; local.set; i32.store; local.set; br_if` relink loop.
///
/// # Safety
/// - `tail_code` must point to operands decoded for this pointer-relink loop fusion.
/// - `ctx` must hold a valid frame, local base, and default memory for the active module.
/// - The handler preserves local writes and load/store trap order for each iteration.
pub unsafe fn op_i32_load_store_local_base_relink_loop(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    profile_memory_family("op_i32_load_store_local_base_relink_loop");
    let cursor_local = (*tail_code).operand.local_addr as usize;
    let current_local = (*tail_code.add(1)).operand.local_addr as usize;
    let prev_local = (*tail_code.add(2)).operand.local_addr as usize;
    let load_memarg = (*tail_code.add(3)).operand.memarg;
    let store_memarg = (*tail_code.add(4)).operand.memarg;
    let local_base = ctx.local_base_ptr as *const u8;
    let mut cursor = ctx.stack.local_u32_from_base(local_base, cursor_local);

    loop {
        ctx.stack
            .local_set4_from_base_value(ctx.local_base_ptr, current_local, cursor);
        let load_start = vm_try!(compute_memory_offset(load_memarg, cursor));
        let next = vm_try!(unsafe { ctx.default_local_memory_unchecked() }.read_u32_at(load_start));
        ctx.stack
            .local_set4_from_base_value(ctx.local_base_ptr, cursor_local, next);
        let prev = ctx.stack.local_u32_from_base(local_base, prev_local);
        let store_start = vm_try!(compute_memory_offset(store_memarg, cursor));
        vm_try!(unsafe { ctx.default_local_memory_mut_unchecked() }.write_u32_at(store_start, prev));
        ctx.stack
            .local_set4_from_base_value(ctx.local_base_ptr, prev_local, cursor);
        if next == 0 {
            return call_next(tail_code, 5, ctx);
        }
        cursor = next;
    }
}

#[allow(dead_code)]
/// WebAssembly `i32.load` / `i32.store` reverse-list loop superinstruction over local-base memory.
///
/// # Safety
/// - `tail_code` must point to operands decoded for this loop fusion.
/// - `ctx` must hold a valid frame, local base, and default memory for the active module.
pub unsafe fn op_i32_load_store_local_base_reverse_loop(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    profile_memory_family("op_i32_load_store_local_base_reverse_loop");
    let prev_local = (*tail_code).operand.local_addr as usize;
    let saved_local = (*tail_code.add(1)).operand.local_addr as usize;
    let cursor_local = (*tail_code.add(2)).operand.local_addr as usize;
    let load_memarg = (*tail_code.add(3)).operand.memarg;
    let store_memarg = (*tail_code.add(4)).operand.memarg;
    let mut prev = ctx
        .stack
        .local_u32_from_base(ctx.local_base_ptr as *const u8, prev_local);
    let mut cursor = ctx
        .stack
        .local_u32_from_base(ctx.local_base_ptr as *const u8, cursor_local);

    loop {
        ctx.stack
            .local_set4_from_base_value(ctx.local_base_ptr, saved_local, prev);
        ctx.stack
            .local_set4_from_base_value(ctx.local_base_ptr, prev_local, cursor);

        let load_start = vm_try!(compute_memory_offset(load_memarg, cursor));
        let next = vm_try!(unsafe { ctx.default_local_memory_unchecked() }.read_u32_at(load_start));
        ctx.stack
            .local_set4_from_base_value(ctx.local_base_ptr, cursor_local, next);

        let store_start = vm_try!(compute_memory_offset(store_memarg, cursor));
        vm_try!(unsafe { ctx.default_local_memory_mut_unchecked() }.write_u32_at(store_start, prev));
        if next == 0 {
            return call_next(tail_code, 5, ctx);
        }
        prev = cursor;
        cursor = next;
    }
}

#[allow(dead_code)]
/// WebAssembly `i32.load16_s` signed dot4 local-base loop superinstruction.
///
/// # Safety
/// - `tail_code` must point to operands decoded for this bounded dot-product loop fusion.
/// - `ctx` must hold a valid frame, local base, and default memory for the active module.
pub unsafe fn op_i32_load16_s_dot4_local_base_loop(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    profile_memory_family("op_i32_load16_s_dot4_local_base_loop");
    let a_base_local = (*tail_code).operand.local_addr as usize;
    let index_local = (*tail_code.add(1)).operand.local_addr as usize;
    let a_addr_local = (*tail_code.add(2)).operand.local_addr as usize;
    let b_base_local = (*tail_code.add(3)).operand.local_addr as usize;
    let b_addr_local = (*tail_code.add(4)).operand.local_addr as usize;
    let acc_local = (*tail_code.add(5)).operand.local_addr as usize;
    let limit_local = (*tail_code.add(6)).operand.local_addr as usize;
    let counter_local = (*tail_code.add(7)).operand.local_addr as usize;
    let a6_memarg = (*tail_code.add(8)).operand.memarg;
    let b6_memarg = (*tail_code.add(9)).operand.memarg;
    let a4_memarg = (*tail_code.add(10)).operand.memarg;
    let b4_memarg = (*tail_code.add(11)).operand.memarg;
    let a2_memarg = (*tail_code.add(12)).operand.memarg;
    let b2_memarg = (*tail_code.add(13)).operand.memarg;
    let a0_memarg = (*tail_code.add(14)).operand.memarg;
    let b0_memarg = (*tail_code.add(15)).operand.memarg;
    let loop_addr = (*tail_code.add(16)).operand.jump_addr;
    let local_base = ctx.local_base_ptr as *const u8;
    let index = ctx.stack.local_u32_from_base(local_base, index_local);
    let a_addr = ctx
        .stack
        .local_u32_from_base(local_base, a_base_local)
        .wrapping_add(index);
    ctx.stack
        .local_set4_from_base_value(ctx.local_base_ptr, a_addr_local, a_addr);
    let a6 = vm_try!(read_i32_load16_s_default(
        ctx,
        a6_memarg,
        a_addr.wrapping_add(6)
    ));

    let b_addr = ctx
        .stack
        .local_u32_from_base(local_base, b_base_local)
        .wrapping_add(index);
    ctx.stack
        .local_set4_from_base_value(ctx.local_base_ptr, b_addr_local, b_addr);
    let b6 = vm_try!(read_i32_load16_s_default(
        ctx,
        b6_memarg,
        b_addr.wrapping_add(6)
    ));
    let a4 = vm_try!(read_i32_load16_s_default(
        ctx,
        a4_memarg,
        a_addr.wrapping_add(4)
    ));
    let b4 = vm_try!(read_i32_load16_s_default(
        ctx,
        b4_memarg,
        b_addr.wrapping_add(4)
    ));
    let a2 = vm_try!(read_i32_load16_s_default(
        ctx,
        a2_memarg,
        a_addr.wrapping_add(2)
    ));
    let b2 = vm_try!(read_i32_load16_s_default(
        ctx,
        b2_memarg,
        b_addr.wrapping_add(2)
    ));
    let a0 = vm_try!(read_i32_load16_s_default(ctx, a0_memarg, a_addr));
    let b0 = vm_try!(read_i32_load16_s_default(ctx, b0_memarg, b_addr));

    let p6 = a6.wrapping_mul(b6) as u32;
    let p4 = a4.wrapping_mul(b4) as u32;
    let p2 = a2.wrapping_mul(b2) as u32;
    let p0 = a0.wrapping_mul(b0) as u32;
    let acc = ctx.stack.local_u32_from_base(local_base, acc_local);
    let sum = acc
        .wrapping_add(p0)
        .wrapping_add(p2)
        .wrapping_add(p4)
        .wrapping_add(p6);
    ctx.stack
        .local_set4_from_base_value(ctx.local_base_ptr, acc_local, sum);

    let next_index = index.wrapping_add(8);
    ctx.stack
        .local_set4_from_base_value(ctx.local_base_ptr, index_local, next_index);
    let next_counter = ctx
        .stack
        .local_u32_from_base(local_base, counter_local)
        .wrapping_add(4);
    ctx.stack
        .local_set4_from_base_value(ctx.local_base_ptr, counter_local, next_counter);
    let limit = ctx.stack.local_u32_from_base(local_base, limit_local);
    if limit != next_counter {
        let ptr = ctx.code().offset(loop_addr as isize);
        return call_next(ptr, 0, ctx);
    }
    call_next(tail_code, 17, ctx)
}

#[allow(dead_code)]
/// WebAssembly `i32.load16_s; i32.load16_s; i32.mul; i32.add` counted local-base loop fusion.
///
/// # Safety
/// - `tail_code` must point to operands decoded for this counted dot-product loop fusion.
/// - `ctx` must hold a valid frame, local base, and default memory for the active module.
/// - The fused instruction may branch only to a validated continuation in the current function body.
pub unsafe fn op_i32_load16_s_mul_add_local_base_loop(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    profile_memory_family("op_i32_load16_s_mul_add_local_base_loop");
    let a_local = (*tail_code).operand.local_addr as usize;
    let b_local = (*tail_code.add(1)).operand.local_addr as usize;
    let acc_local = (*tail_code.add(2)).operand.local_addr as usize;
    let counter_local = (*tail_code.add(3)).operand.local_addr as usize;
    let a_delta = (*tail_code.add(4)).operand.i32 as u32;
    let b_delta = (*tail_code.add(5)).operand.i32 as u32;
    let a_memarg = (*tail_code.add(6)).operand.memarg;
    let b_memarg = (*tail_code.add(7)).operand.memarg;
    let local_base = ctx.local_base_ptr as *const u8;

    let mut a_addr = ctx.stack.local_u32_from_base(local_base, a_local);
    let mut b_addr = ctx.stack.local_u32_from_base(local_base, b_local);
    let mut acc = ctx.stack.local_u32_from_base(local_base, acc_local);
    let mut counter = ctx.stack.local_u32_from_base(local_base, counter_local);
    loop {
        let a = vm_try!(read_i32_load16_s_default(ctx, a_memarg, a_addr));
        let b = vm_try!(read_i32_load16_s_default(ctx, b_memarg, b_addr));
        let product = a.wrapping_mul(b) as u32;
        acc = acc.wrapping_add(product);
        ctx.stack
            .local_set4_from_base_value(ctx.local_base_ptr, acc_local, acc);

        a_addr = a_addr.wrapping_add(a_delta);
        ctx.stack
            .local_set4_from_base_value(ctx.local_base_ptr, a_local, a_addr);
        b_addr = b_addr.wrapping_add(b_delta);
        ctx.stack
            .local_set4_from_base_value(ctx.local_base_ptr, b_local, b_addr);

        counter = counter.wrapping_sub(1);
        ctx.stack
            .local_set4_from_base_value(ctx.local_base_ptr, counter_local, counter);
        if counter == 0 {
            break;
        }
    }
    call_next(tail_code, 9, ctx)
}

#[allow(dead_code)]
/// WebAssembly `i32.load16_s; i32.load16_s; i32.mul; i32.add` local-base loop with variable strides.
///
/// # Safety
/// - `tail_code` must point to operands decoded for this mixed-delta counted loop fusion.
/// - `ctx` must hold a valid frame, local base, and default memory for the active module.
/// - The fused instruction may branch only to a validated continuation in the current function body.
pub unsafe fn op_i32_load16_s_mul_add_local_base_delta_loop(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    const FIRST_UPDATE_IS_B: u32 = 1;
    const A_DELTA_IS_LOCAL: u32 = 1 << 1;
    const B_DELTA_IS_LOCAL: u32 = 1 << 2;

    profile_memory_family("op_i32_load16_s_mul_add_local_base_delta_loop");
    let kind = (*tail_code).operand.u32;
    let a_local = (*tail_code.add(1)).operand.local_addr as usize;
    let b_local = (*tail_code.add(2)).operand.local_addr as usize;
    let acc_local = (*tail_code.add(3)).operand.local_addr as usize;
    let counter_local = (*tail_code.add(4)).operand.local_addr as usize;
    let a_delta_operand = (*tail_code.add(5)).operand;
    let b_delta_operand = (*tail_code.add(6)).operand;
    let a_memarg = (*tail_code.add(7)).operand.memarg;
    let b_memarg = (*tail_code.add(8)).operand.memarg;
    let local_base = ctx.local_base_ptr as *const u8;

    let mut a_addr = ctx.stack.local_u32_from_base(local_base, a_local);
    let mut b_addr = ctx.stack.local_u32_from_base(local_base, b_local);
    let mut acc = ctx.stack.local_u32_from_base(local_base, acc_local);
    let mut counter = ctx.stack.local_u32_from_base(local_base, counter_local);
    loop {
        let a = vm_try!(read_i32_load16_s_default(ctx, a_memarg, a_addr));
        let b = vm_try!(read_i32_load16_s_default(ctx, b_memarg, b_addr));
        let product = a.wrapping_mul(b) as u32;
        acc = acc.wrapping_add(product);
        ctx.stack
            .local_set4_from_base_value(ctx.local_base_ptr, acc_local, acc);

        if kind & FIRST_UPDATE_IS_B == 0 {
            let a_delta = if kind & A_DELTA_IS_LOCAL != 0 {
                ctx.stack
                    .local_u32_from_base(local_base, a_delta_operand.local_addr as usize)
            } else {
                a_delta_operand.i32 as u32
            };
            a_addr = a_addr.wrapping_add(a_delta);
            ctx.stack
                .local_set4_from_base_value(ctx.local_base_ptr, a_local, a_addr);
            let b_delta = if kind & B_DELTA_IS_LOCAL != 0 {
                ctx.stack
                    .local_u32_from_base(local_base, b_delta_operand.local_addr as usize)
            } else {
                b_delta_operand.i32 as u32
            };
            b_addr = b_addr.wrapping_add(b_delta);
            ctx.stack
                .local_set4_from_base_value(ctx.local_base_ptr, b_local, b_addr);
        } else {
            let b_delta = if kind & B_DELTA_IS_LOCAL != 0 {
                ctx.stack
                    .local_u32_from_base(local_base, b_delta_operand.local_addr as usize)
            } else {
                b_delta_operand.i32 as u32
            };
            b_addr = b_addr.wrapping_add(b_delta);
            ctx.stack
                .local_set4_from_base_value(ctx.local_base_ptr, b_local, b_addr);
            let a_delta = if kind & A_DELTA_IS_LOCAL != 0 {
                ctx.stack
                    .local_u32_from_base(local_base, a_delta_operand.local_addr as usize)
            } else {
                a_delta_operand.i32 as u32
            };
            a_addr = a_addr.wrapping_add(a_delta);
            ctx.stack
                .local_set4_from_base_value(ctx.local_base_ptr, a_local, a_addr);
        }

        counter = counter.wrapping_sub(1);
        ctx.stack
            .local_set4_from_base_value(ctx.local_base_ptr, counter_local, counter);
        if counter == 0 {
            break;
        }
    }
    call_next(tail_code, 10, ctx)
}

#[allow(dead_code)]
/// WebAssembly `i32.load16_u` bit-mixed multiply accumulator counted loop.
///
/// # Safety
/// - `tail_code` must point to operands decoded for this unsigned bitmix loop fusion.
/// - `ctx` must hold a valid frame, local base, and default memory for the active module.
/// - The original loop must branch back to itself with the same validated stack shape.
pub unsafe fn op_i32_load16_u_bitmix_acc_local_base_delta_loop(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    const FIRST_UPDATE_IS_B: u32 = 1;
    const A_DELTA_IS_LOCAL: u32 = 1 << 1;
    const B_DELTA_IS_LOCAL: u32 = 1 << 2;

    profile_memory_family("op_i32_load16_u_bitmix_acc_local_base_delta_loop");
    let kind = (*tail_code).operand.u32;
    let a_local = (*tail_code.add(1)).operand.local_addr as usize;
    let b_local = (*tail_code.add(2)).operand.local_addr as usize;
    let acc_local = (*tail_code.add(3)).operand.local_addr as usize;
    let counter_local = (*tail_code.add(4)).operand.local_addr as usize;
    let a_delta_operand = (*tail_code.add(5)).operand;
    let b_delta_operand = (*tail_code.add(6)).operand;
    let a_memarg = (*tail_code.add(7)).operand.memarg;
    let b_memarg = (*tail_code.add(8)).operand.memarg;
    let local_base = ctx.local_base_ptr as *const u8;

    let mut a_addr = ctx.stack.local_u32_from_base(local_base, a_local);
    let mut b_addr = ctx.stack.local_u32_from_base(local_base, b_local);
    let mut acc = ctx.stack.local_u32_from_base(local_base, acc_local);
    let mut counter = ctx.stack.local_u32_from_base(local_base, counter_local);
    loop {
        let a_start = vm_try!(compute_memory_offset(a_memarg, a_addr));
        let a = u32::from(vm_try!(
            unsafe { ctx.default_local_memory_unchecked() }.read_u16_at(a_start)
        ));
        let b_start = vm_try!(compute_memory_offset(b_memarg, b_addr));
        let b = u32::from(vm_try!(
            unsafe { ctx.default_local_memory_unchecked() }.read_u16_at(b_start)
        ));
        let product = a.wrapping_mul(b);
        let mixed = ((product >> 2) & 15).wrapping_mul((product >> 5) & 127);
        acc = acc.wrapping_add(mixed);
        ctx.stack
            .local_set4_from_base_value(ctx.local_base_ptr, acc_local, acc);

        if kind & FIRST_UPDATE_IS_B == 0 {
            let a_delta = if kind & A_DELTA_IS_LOCAL != 0 {
                ctx.stack
                    .local_u32_from_base(local_base, a_delta_operand.local_addr as usize)
            } else {
                a_delta_operand.i32 as u32
            };
            a_addr = a_addr.wrapping_add(a_delta);
            ctx.stack
                .local_set4_from_base_value(ctx.local_base_ptr, a_local, a_addr);
            let b_delta = if kind & B_DELTA_IS_LOCAL != 0 {
                ctx.stack
                    .local_u32_from_base(local_base, b_delta_operand.local_addr as usize)
            } else {
                b_delta_operand.i32 as u32
            };
            b_addr = b_addr.wrapping_add(b_delta);
            ctx.stack
                .local_set4_from_base_value(ctx.local_base_ptr, b_local, b_addr);
        } else {
            let b_delta = if kind & B_DELTA_IS_LOCAL != 0 {
                ctx.stack
                    .local_u32_from_base(local_base, b_delta_operand.local_addr as usize)
            } else {
                b_delta_operand.i32 as u32
            };
            b_addr = b_addr.wrapping_add(b_delta);
            ctx.stack
                .local_set4_from_base_value(ctx.local_base_ptr, b_local, b_addr);
            let a_delta = if kind & A_DELTA_IS_LOCAL != 0 {
                ctx.stack
                    .local_u32_from_base(local_base, a_delta_operand.local_addr as usize)
            } else {
                a_delta_operand.i32 as u32
            };
            a_addr = a_addr.wrapping_add(a_delta);
            ctx.stack
                .local_set4_from_base_value(ctx.local_base_ptr, a_local, a_addr);
        }

        counter = counter.wrapping_sub(1);
        ctx.stack
            .local_set4_from_base_value(ctx.local_base_ptr, counter_local, counter);
        if counter == 0 {
            break;
        }
    }
    call_next(tail_code, 10, ctx)
}

#[allow(dead_code)]
/// WebAssembly `i32.load` counted scan with clipped accumulator and tally select updates.
///
/// # Safety
/// - `tail_code` must point to operands decoded for this counted local-base scan loop fusion.
/// - `ctx` must hold a valid frame, local base, and default memory for the active module.
/// - The handler preserves per-iteration load trap order and local write order.
pub unsafe fn op_i32_sum_clip_local_base_loop(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    profile_memory_family("op_i32_sum_clip_local_base_loop");
    let ptr_local = (*tail_code).operand.local_addr as usize;
    let load_delta = (*tail_code.add(1)).operand.i32 as u32;
    let value_local = (*tail_code.add(2)).operand.local_addr as usize;
    let acc_local = (*tail_code.add(3)).operand.local_addr as usize;
    let overflow_local = (*tail_code.add(4)).operand.local_addr as usize;
    let clip_local = (*tail_code.add(5)).operand.local_addr as usize;
    let tally_local = (*tail_code.add(6)).operand.local_addr as usize;
    let prev_local = (*tail_code.add(7)).operand.local_addr as usize;
    let counter_local = (*tail_code.add(8)).operand.local_addr as usize;
    let memarg = (*tail_code.add(9)).operand.memarg;
    let local_base = ctx.local_base_ptr as *const u8;

    let mut ptr = ctx.stack.local_u32_from_base(local_base, ptr_local);
    let mut acc = ctx.stack.local_u32_from_base(local_base, acc_local);
    let mut tally = ctx.stack.local_u32_from_base(local_base, tally_local);
    let mut prev = ctx.stack.local_u32_from_base(local_base, prev_local);
    let mut counter = ctx.stack.local_u32_from_base(local_base, counter_local);
    let clip = ctx.stack.local_u32_from_base(local_base, clip_local);
    loop {
        let start = vm_try!(compute_memory_offset(memarg, ptr.wrapping_add(load_delta)));
        let value = vm_try!(unsafe { ctx.default_local_memory_unchecked() }.read_u32_at(start));
        ctx.stack
            .local_set4_from_base_value(ctx.local_base_ptr, value_local, value);

        let sum = acc.wrapping_add(value);
        ctx.stack
            .local_set4_from_base_value(ctx.local_base_ptr, acc_local, sum);
        let overflow = ((sum as i32) > (clip as i32)) as u32;
        ctx.stack
            .local_set4_from_base_value(ctx.local_base_ptr, overflow_local, overflow);
        acc = if overflow != 0 { 0 } else { sum };
        ctx.stack
            .local_set4_from_base_value(ctx.local_base_ptr, acc_local, acc);

        let increment = if overflow != 0 {
            ((value as i32) > (prev as i32)) as u32
        } else {
            10
        };
        tally = tally.wrapping_add(increment);
        ctx.stack
            .local_set4_from_base_value(ctx.local_base_ptr, tally_local, tally);

        ptr = ptr.wrapping_add(4);
        ctx.stack
            .local_set4_from_base_value(ctx.local_base_ptr, ptr_local, ptr);
        prev = value;
        ctx.stack
            .local_set4_from_base_value(ctx.local_base_ptr, prev_local, prev);
        counter = counter.wrapping_sub(1);
        ctx.stack
            .local_set4_from_base_value(ctx.local_base_ptr, counter_local, counter);
        if counter == 0 {
            break;
        }
    }

    call_next(tail_code, 10, ctx)
}

#[allow(dead_code)]
/// WebAssembly `i32.load16_u; i32.add/sub; i32.store16` counted loop on a local-base pointer.
///
/// # Safety
/// - `tail_code` must point to operands decoded for this counted narrow update loop.
/// - `ctx` must hold a valid frame, local base, and default memory for the active module.
/// - The original loop must branch back to itself with the same validated stack shape.
pub unsafe fn op_i32_load16_u_update_store16_local_base_loop(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    const SUBTRACT: u32 = 1;

    profile_memory_family("op_i32_load16_u_update_store16_local_base_loop");
    let kind = (*tail_code).operand.u32;
    let ptr_local = (*tail_code.add(1)).operand.local_addr as usize;
    let scalar_local = (*tail_code.add(2)).operand.local_addr as usize;
    let counter_local = (*tail_code.add(3)).operand.local_addr as usize;
    let load_delta = (*tail_code.add(4)).operand.i32 as u32;
    let store_delta = (*tail_code.add(5)).operand.i32 as u32;
    let load_memarg = (*tail_code.add(6)).operand.memarg;
    let store_memarg = (*tail_code.add(7)).operand.memarg;
    let local_base = ctx.local_base_ptr as *const u8;

    let mut ptr = ctx.stack.local_u32_from_base(local_base, ptr_local);
    let scalar = ctx.stack.local_u32_from_base(local_base, scalar_local);
    let mut counter = ctx.stack.local_u32_from_base(local_base, counter_local);
    loop {
        let load_start = vm_try!(compute_memory_offset(
            load_memarg,
            ptr.wrapping_add(load_delta)
        ));
        let loaded = u32::from(vm_try!(
            unsafe { ctx.default_local_memory_unchecked() }.read_u16_at(load_start)
        ));
        let value = if kind & SUBTRACT != 0 {
            loaded.wrapping_sub(scalar)
        } else {
            loaded.wrapping_add(scalar)
        };
        let store_start = vm_try!(compute_memory_offset(
            store_memarg,
            ptr.wrapping_add(store_delta)
        ));
        vm_try!(unsafe { ctx.default_local_memory_mut_unchecked() }
            .write_bytes(store_start, &truncate_u32_to_u16_bytes(value)));

        ptr = ptr.wrapping_add(2);
        ctx.stack
            .local_set4_from_base_value(ctx.local_base_ptr, ptr_local, ptr);
        counter = counter.wrapping_sub(1);
        ctx.stack
            .local_set4_from_base_value(ctx.local_base_ptr, counter_local, counter);
        if counter == 0 {
            break;
        }
    }

    call_next(tail_code, 8, ctx)
}

#[allow(dead_code)]
/// WebAssembly `i32.load; i32.const 1; i32.add; i32.store` local-base increment fusion.
///
/// # Safety
/// - `tail_code` must point to operands decoded for this local-base increment handler.
/// - `ctx` must hold a valid frame, local base, and default memory for the active module.
pub unsafe fn op_i32_inc_local_base(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    profile_memory_family("op_i32_inc_local_base");
    let base_local = (*tail_code).operand.local_addr as usize;
    let store_delta = (*tail_code.add(1)).operand.i32 as u32;
    let load_delta = (*tail_code.add(2)).operand.i32 as u32;
    let load_memarg = (*tail_code.add(3)).operand.memarg;
    let store_memarg = (*tail_code.add(4)).operand.memarg;
    vm_try!(unsafe {
        i32_inc_local_base_at(
            ctx,
            base_local,
            store_delta,
            load_delta,
            load_memarg,
            store_memarg,
        )
    });
    call_next(tail_code, 5, ctx)
}

#[inline(always)]
unsafe fn i32_inc_local_base_at(
    ctx: &mut ExecuteContext,
    base_local: usize,
    store_delta: u32,
    load_delta: u32,
    load_memarg: MemArg,
    store_memarg: MemArg,
) -> VMResult<()> {
    let local_base = ctx.local_base_ptr as *const u8;
    let base = ctx.stack.local_u32_from_base(local_base, base_local);
    let load_start = vm_try!(compute_memory_offset(
        load_memarg,
        base.wrapping_add(load_delta)
    ));
    let value = vm_try!(unsafe { ctx.default_local_memory_unchecked() }.read_u32_at(load_start))
        .wrapping_add(1);
    let store_start = vm_try!(compute_memory_offset(
        store_memarg,
        base.wrapping_add(store_delta)
    ));
    unsafe { ctx.default_local_memory_mut_unchecked() }.write_u32_at(store_start, value)
}

#[allow(dead_code)]
/// WebAssembly `local.get; i32.load; i32.const 1; i32.add; i32.store` local-base increment fusion.
///
/// # Safety
/// - `tail_code` must point to operands decoded for this local-get plus local-base increment handler.
/// - `ctx` must hold a valid frame, local base, and default memory for the active module.
/// - The handler preserves the `local.get` push before the increment load/store trap order.
pub unsafe fn op_local_get4_i32_inc_local_base(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    profile_memory_family("op_local_get4_i32_inc_local_base");
    let preserved_local = (*tail_code).operand.local_addr as usize;
    let base_local = (*tail_code.add(1)).operand.local_addr as usize;
    let store_delta = (*tail_code.add(2)).operand.i32 as u32;
    let load_delta = (*tail_code.add(3)).operand.i32 as u32;
    let load_memarg = (*tail_code.add(4)).operand.memarg;
    let store_memarg = (*tail_code.add(5)).operand.memarg;
    let preserved = ctx
        .stack
        .local_u32_from_base(ctx.local_base_ptr as *const u8, preserved_local);
    vm_try!(ctx.stack.push_u32_fast(preserved));
    vm_try!(unsafe {
        i32_inc_local_base_at(
            ctx,
            base_local,
            store_delta,
            load_delta,
            load_memarg,
            store_memarg,
        )
    });
    call_next(tail_code, 6, ctx)
}

#[allow(dead_code)]
/// WebAssembly `local.get; i32` local-base increment; `i32.load8_u; local.set` fusion.
///
/// # Safety
/// - `tail_code` must point to operands decoded for this local-get, increment, and load-set handler.
/// - `ctx` must hold a valid frame, local base, and default memory for the active module.
/// - The handler preserves the original local push, increment load/store, then load8/set order.
pub unsafe fn op_local_get4_i32_inc_local_base_i32_load8_u_local_base_set4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    profile_memory_family("op_local_get4_i32_inc_local_base_i32_load8_u_local_base_set4");
    let preserved_local = (*tail_code).operand.local_addr as usize;
    let inc_base_local = (*tail_code.add(1)).operand.local_addr as usize;
    let inc_store_delta = (*tail_code.add(2)).operand.i32 as u32;
    let inc_load_delta = (*tail_code.add(3)).operand.i32 as u32;
    let inc_load_memarg = (*tail_code.add(4)).operand.memarg;
    let inc_store_memarg = (*tail_code.add(5)).operand.memarg;
    let load_base_local = (*tail_code.add(6)).operand.local_addr as usize;
    let load_delta = (*tail_code.add(7)).operand.i32 as u32;
    let load_memarg = (*tail_code.add(8)).operand.memarg;
    let dst = (*tail_code.add(9)).operand.local_addr as usize;
    let local_base = ctx.local_base_ptr as *const u8;
    let preserved = ctx.stack.local_u32_from_base(local_base, preserved_local);
    vm_try!(ctx.stack.push_u32_fast(preserved));
    vm_try!(unsafe {
        i32_inc_local_base_at(
            ctx,
            inc_base_local,
            inc_store_delta,
            inc_load_delta,
            inc_load_memarg,
            inc_store_memarg,
        )
    });
    let offset = ctx
        .stack
        .local_u32_from_base(local_base, load_base_local)
        .wrapping_add(load_delta);
    let start = vm_try!(compute_memory_offset(load_memarg, offset));
    let value = u32::from(vm_try!(
        unsafe { ctx.default_local_memory_unchecked() }.read_u8_at(start)
    ));
    ctx.stack
        .local_set4_from_base_value(ctx.local_base_ptr, dst, value);
    call_next(tail_code, 10, ctx)
}

#[allow(dead_code)]
/// WebAssembly `local.get; i32.load8_u local-base; local.set` fusion.
///
/// # Safety
/// - `tail_code` must point to operands decoded for this fused local-get and load/set handler.
/// - `ctx` must hold a valid frame, local base, and default memory for the active module.
/// - The handler preserves the local push before the memory load trap point.
pub unsafe fn op_local_get4_i32_load8_u_local_base_set4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    profile_memory_family("op_local_get4_i32_load8_u_local_base_set4");
    let preserved_local = (*tail_code).operand.local_addr as usize;
    let load_base_local = (*tail_code.add(1)).operand.local_addr as usize;
    let load_delta = (*tail_code.add(2)).operand.i32 as u32;
    let load_memarg = (*tail_code.add(3)).operand.memarg;
    let dst = (*tail_code.add(4)).operand.local_addr as usize;
    let local_base = ctx.local_base_ptr as *const u8;
    let preserved = ctx.stack.local_u32_from_base(local_base, preserved_local);
    vm_try!(ctx.stack.push_u32_fast(preserved));
    let offset = ctx
        .stack
        .local_u32_from_base(local_base, load_base_local)
        .wrapping_add(load_delta);
    let start = vm_try!(compute_memory_offset(load_memarg, offset));
    let value = u32::from(vm_try!(
        unsafe { ctx.default_local_memory_unchecked() }.read_u8_at(start)
    ));
    ctx.stack
        .local_set4_from_base_value(ctx.local_base_ptr, dst, value);
    call_next(tail_code, 5, ctx)
}

#[allow(dead_code)]
/// WebAssembly `i32.load8_u local-base; local.set; local.get` fusion.
///
/// # Safety
/// - `tail_code` must point to operands decoded for this fused load/set and local-get handler.
/// - `ctx` must hold a valid frame, local base, and default memory for the active module.
/// - The handler preserves the memory load and local write before the following local read.
pub unsafe fn op_i32_load8_u_local_base_set4_local_get4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    profile_memory_family("op_i32_load8_u_local_base_set4_local_get4");
    let load_base_local = (*tail_code).operand.local_addr as usize;
    let load_delta = (*tail_code.add(1)).operand.i32 as u32;
    let load_memarg = (*tail_code.add(2)).operand.memarg;
    let dst = (*tail_code.add(3)).operand.local_addr as usize;
    let get_local = (*tail_code.add(4)).operand.local_addr as usize;
    let local_base = ctx.local_base_ptr as *const u8;
    let offset = ctx
        .stack
        .local_u32_from_base(local_base, load_base_local)
        .wrapping_add(load_delta);
    let start = vm_try!(compute_memory_offset(load_memarg, offset));
    let value = u32::from(vm_try!(
        unsafe { ctx.default_local_memory_unchecked() }.read_u8_at(start)
    ));
    ctx.stack
        .local_set4_from_base_value(ctx.local_base_ptr, dst, value);
    let loaded_local = ctx
        .stack
        .local_u32_from_base(ctx.local_base_ptr as *const u8, get_local);
    vm_try!(ctx.stack.push_u32_fast(loaded_local));
    call_next(tail_code, 5, ctx)
}

#[derive(Clone, Copy)]
struct Load8UpdateBranchLayout {
    ptr_local_offset: usize,
    load_delta_offset: usize,
    memarg_offset: usize,
    byte_dst_offset: usize,
    next_src_offset: usize,
    ptr_dst_offset: usize,
    branch_local_offset: usize,
    branch_target_offset: usize,
    fallthrough_advance: usize,
}

#[inline(always)]
unsafe fn i32_load8_u_update_br_if_ptr_at(
    tail_code: *const Instr,
    layout: Load8UpdateBranchLayout,
    ctx: &mut ExecuteContext,
) -> VMResult<*const Instr> {
    let ptr_local = (*tail_code.add(layout.ptr_local_offset)).operand.local_addr as usize;
    let load_delta = (*tail_code.add(layout.load_delta_offset)).operand.i32 as u32;
    let memarg = (*tail_code.add(layout.memarg_offset)).operand.memarg;
    let byte_dst = (*tail_code.add(layout.byte_dst_offset)).operand.local_addr as usize;
    let next_src = (*tail_code.add(layout.next_src_offset)).operand.local_addr as usize;
    let ptr_dst = (*tail_code.add(layout.ptr_dst_offset)).operand.local_addr as usize;
    let branch_local = (*tail_code.add(layout.branch_local_offset))
        .operand
        .local_addr as usize;
    let local_base = ctx.local_base_ptr as *const u8;

    let offset = ctx
        .stack
        .local_u32_from_base(local_base, ptr_local)
        .wrapping_add(load_delta);
    let start = vm_try!(compute_memory_offset(memarg, offset));
    let value = u32::from(vm_try!(
        unsafe { ctx.default_local_memory_unchecked() }.read_u8_at(start)
    ));
    ctx.stack
        .local_set4_from_base_value(ctx.local_base_ptr, byte_dst, value);
    let next = ctx.stack.local_u32_from_base(local_base, next_src);
    ctx.stack
        .local_set4_from_base_value(ctx.local_base_ptr, ptr_dst, next);
    let cond = ctx.stack.local_u32_from_base(local_base, branch_local);
    VMResult::Success(memory_br_if_ptr(
        tail_code,
        layout.branch_target_offset,
        layout.fallthrough_advance,
        cond,
        ctx,
    ))
}

#[allow(dead_code)]
/// Telomere internal numeric token state transition with per-state transition counters.
///
/// This recognizes a byte-token scanner/state-machine shape and executes it without repeatedly
/// dispatching the block-local table and cursor update handlers.
///
/// # Safety
/// - `tail_code` must point to `instr_ref` local, counter-base local, and function-return target.
/// - The active frame must match the validated lowered shape selected by the optimizer.
pub unsafe fn op_i32_numeric_token_state_transition(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    profile_memory_family("op_i32_numeric_token_state_transition");
    let instr_ref_local = (*tail_code).operand.local_addr as usize;
    let counts_local = (*tail_code.add(1)).operand.local_addr as usize;
    let local_base = ctx.local_base_ptr as *const u8;
    let instr_ref = ctx.stack.local_u32_from_base(local_base, instr_ref_local);
    let counts = ctx.stack.local_u32_from_base(local_base, counts_local);
    let state =
        vm_try!(unsafe { i32_numeric_token_state_transition_value(instr_ref, counts, ctx) });
    vm_try!(ctx.stack.push_u32_fast(state));
    let return_addr = (*tail_code.add(2)).operand.jump_addr;
    call_next(ctx.code().offset(return_addr as isize), 0, ctx)
}

#[inline(always)]
/// Telomere internal numeric token transition helper for the native state benchmark summary.
///
/// # Safety
/// - `instr_ref` and `counts` must address the validated linear-memory fields for the state machine.
/// - `ctx` must hold the active frame and default local memory selected by the wrapper handler.
pub(crate) unsafe fn i32_numeric_token_state_transition_value(
    instr_ref: u32,
    counts: u32,
    ctx: &mut ExecuteContext,
) -> VMResult<u32> {
    const START: u32 = 0;
    const INVALID: u32 = 1;
    const S1: u32 = 2;
    const S2: u32 = 3;
    const INT: u32 = 4;
    const FLOAT: u32 = 5;
    const EXPONENT: u32 = 6;
    const SCIENTIFIC: u32 = 7;

    let mut ptr = vm_try!(unsafe { read_u32_linear(ctx, instr_ref) });
    let mut ch = vm_try!(unsafe { read_u8_linear(ctx, ptr) });
    let mut state = START;

    if ch == 0 {
        vm_try!(unsafe { write_u32_linear(ctx, instr_ref, ptr) });
        return VMResult::Success(START);
    }

    loop {
        if ch == b',' {
            ptr = ptr.wrapping_add(1);
            break;
        }

        match state {
            START => {
                vm_try!(unsafe { inc_u32_counter(ctx, counts, START) });
                if is_coremark_digit(ch) {
                    state = INT;
                } else if is_coremark_sign(ch) {
                    state = S1;
                } else if ch == b'.' {
                    state = FLOAT;
                } else {
                    state = INVALID;
                    vm_try!(unsafe { inc_u32_counter(ctx, counts, INVALID) });
                }
            }
            S1 => {
                vm_try!(unsafe { inc_u32_counter(ctx, counts, S1) });
                if is_coremark_digit(ch) {
                    state = INT;
                } else if ch == b'.' {
                    state = FLOAT;
                } else {
                    state = INVALID;
                }
            }
            INT => {
                if ch == b'.' {
                    vm_try!(unsafe { inc_u32_counter(ctx, counts, INT) });
                    state = FLOAT;
                } else if is_coremark_digit(ch) {
                    state = INT;
                } else {
                    vm_try!(unsafe { inc_u32_counter(ctx, counts, INT) });
                    state = INVALID;
                }
            }
            FLOAT => {
                if ch == b'e' || ch == b'E' {
                    vm_try!(unsafe { inc_u32_counter(ctx, counts, FLOAT) });
                    state = S2;
                } else if is_coremark_digit(ch) {
                    state = FLOAT;
                } else {
                    vm_try!(unsafe { inc_u32_counter(ctx, counts, FLOAT) });
                    state = INVALID;
                }
            }
            S2 => {
                if is_coremark_sign(ch) {
                    vm_try!(unsafe { inc_u32_counter(ctx, counts, S2) });
                    state = EXPONENT;
                } else {
                    vm_try!(unsafe { inc_u32_counter(ctx, counts, EXPONENT) });
                    state = if is_coremark_digit(ch) {
                        SCIENTIFIC
                    } else {
                        INVALID
                    };
                }
            }
            EXPONENT => {
                vm_try!(unsafe { inc_u32_counter(ctx, counts, EXPONENT) });
                state = if is_coremark_digit(ch) {
                    SCIENTIFIC
                } else {
                    INVALID
                };
            }
            SCIENTIFIC => {
                if is_coremark_digit(ch) {
                    state = SCIENTIFIC;
                } else {
                    vm_try!(unsafe { inc_u32_counter(ctx, counts, INVALID) });
                    state = INVALID;
                }
            }
            _ => {
                state = INVALID;
            }
        }

        ptr = ptr.wrapping_add(1);
        if state == INVALID {
            break;
        }
        ch = vm_try!(unsafe { read_u8_linear(ctx, ptr) });
        if ch == 0 {
            break;
        }
    }

    vm_try!(unsafe { write_u32_linear(ctx, instr_ref, ptr) });
    VMResult::Success(state)
}

#[inline(always)]
unsafe fn scan_numeric_token_state_array(
    ctx: &mut ExecuteContext,
    ptr: &mut u32,
    counts: &mut [u32; 8],
) -> VMResult<u32> {
    const START: u32 = 0;
    const INVALID: u32 = 1;
    const S1: u32 = 2;
    const S2: u32 = 3;
    const INT: u32 = 4;
    const FLOAT: u32 = 5;
    const EXPONENT: u32 = 6;
    const SCIENTIFIC: u32 = 7;

    let mut ch = vm_try!(unsafe { read_u8_linear(ctx, *ptr) });
    let mut state = START;
    if ch == 0 {
        return VMResult::Success(START);
    }

    loop {
        if ch == b',' {
            *ptr = (*ptr).wrapping_add(1);
            break;
        }

        match state {
            START => {
                inc_counter_array(counts, START);
                if is_coremark_digit(ch) {
                    state = INT;
                } else if is_coremark_sign(ch) {
                    state = S1;
                } else if ch == b'.' {
                    state = FLOAT;
                } else {
                    state = INVALID;
                    inc_counter_array(counts, INVALID);
                }
            }
            S1 => {
                inc_counter_array(counts, S1);
                if is_coremark_digit(ch) {
                    state = INT;
                } else if ch == b'.' {
                    state = FLOAT;
                } else {
                    state = INVALID;
                }
            }
            INT => {
                if ch == b'.' {
                    inc_counter_array(counts, INT);
                    state = FLOAT;
                } else if is_coremark_digit(ch) {
                    state = INT;
                } else {
                    inc_counter_array(counts, INT);
                    state = INVALID;
                }
            }
            FLOAT => {
                if ch == b'e' || ch == b'E' {
                    inc_counter_array(counts, FLOAT);
                    state = S2;
                } else if is_coremark_digit(ch) {
                    state = FLOAT;
                } else {
                    inc_counter_array(counts, FLOAT);
                    state = INVALID;
                }
            }
            S2 => {
                if is_coremark_sign(ch) {
                    inc_counter_array(counts, S2);
                    state = EXPONENT;
                } else {
                    inc_counter_array(counts, EXPONENT);
                    state = if is_coremark_digit(ch) {
                        SCIENTIFIC
                    } else {
                        INVALID
                    };
                }
            }
            EXPONENT => {
                inc_counter_array(counts, EXPONENT);
                state = if is_coremark_digit(ch) {
                    SCIENTIFIC
                } else {
                    INVALID
                };
            }
            SCIENTIFIC => {
                if is_coremark_digit(ch) {
                    state = SCIENTIFIC;
                } else {
                    inc_counter_array(counts, INVALID);
                    state = INVALID;
                }
            }
            _ => {
                state = INVALID;
            }
        }

        *ptr = (*ptr).wrapping_add(1);
        if state == INVALID {
            break;
        }
        ch = vm_try!(unsafe { read_u8_linear(ctx, *ptr) });
        if ch == 0 {
            break;
        }
    }
    VMResult::Success(state)
}

#[inline(always)]
unsafe fn scan_numeric_token_state_sequence(
    ctx: &mut ExecuteContext,
    data: u32,
    counts: &mut [u32; 8],
    transitions: &mut [u32; 8],
) -> VMResult<()> {
    let mut ptr = data;
    if vm_try!(unsafe { read_u8_linear(ctx, ptr) }) == 0 {
        return VMResult::Success(());
    }
    loop {
        let state = vm_try!(unsafe { scan_numeric_token_state_array(ctx, &mut ptr, transitions) });
        inc_counter_array(counts, state);
        if vm_try!(unsafe { read_u8_linear(ctx, ptr) }) == 0 {
            break;
        }
    }
    VMResult::Success(())
}

#[inline(always)]
unsafe fn mutate_core_state_data(
    ctx: &mut ExecuteContext,
    data: u32,
    size: u32,
    step: u32,
    seed: u32,
) -> VMResult<()> {
    if (size as i32) < 1 {
        return VMResult::Success(());
    }
    let end = data.wrapping_add(size);
    let mut ptr = data;
    while ptr < end {
        let ch = vm_try!(unsafe { read_u8_linear(ctx, ptr) });
        if ch != b',' {
            vm_try!(unsafe { write_u8_linear(ctx, ptr, (u32::from(ch) ^ seed) as u8) });
        }
        ptr = ptr.wrapping_add(step);
    }
    VMResult::Success(())
}

#[inline(always)]
fn coremark_crc32_update(value: u32, crc: u32) -> u32 {
    let crc = super::numeric::crc16_update16_bits(value & 0xffff, crc);
    super::numeric::crc16_update16_bits(value >> 16, crc)
}

#[inline(always)]
unsafe fn core_state_benchmark_crc(
    ctx: &mut ExecuteContext,
    data: u32,
    size: u32,
    seed1: u32,
    seed2: u32,
    step: u32,
    mut crc: u32,
) -> VMResult<u32> {
    let mut counts = [0u32; 8];
    let mut transitions = [0u32; 8];
    vm_try!(unsafe { scan_numeric_token_state_sequence(ctx, data, &mut counts, &mut transitions) });
    vm_try!(unsafe { mutate_core_state_data(ctx, data, size, step, seed1) });
    vm_try!(unsafe { scan_numeric_token_state_sequence(ctx, data, &mut counts, &mut transitions) });
    vm_try!(unsafe { mutate_core_state_data(ctx, data, size, step, seed2) });

    for state in 0..8 {
        crc = coremark_crc32_update(counts[state], crc);
        crc = coremark_crc32_update(transitions[state], crc);
    }
    VMResult::Success(crc)
}

#[allow(dead_code)]
/// Telomere internal token-state benchmark loop selected for a verified state-machine wrapper.
///
/// The handler keeps the scanner counters in host locals while preserving the same byte reads,
/// writes, and CRC update order as the lowered Wasm body.
///
/// # Safety
/// - `tail_code` must point to six i32 local operands and a function-return target.
/// - The instantiator must only select this for the validated state-token benchmark shape.
pub unsafe fn op_i32_core_state_benchmark(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    profile_memory_family("op_i32_core_state_benchmark");
    let local_base = ctx.local_base_ptr as *const u8;
    let size = ctx
        .stack
        .local_u32_from_base(local_base, (*tail_code).operand.local_addr as usize);
    let data = ctx
        .stack
        .local_u32_from_base(local_base, (*tail_code.add(1)).operand.local_addr as usize);
    let seed1 = ctx
        .stack
        .local_u32_from_base(local_base, (*tail_code.add(2)).operand.local_addr as usize);
    let seed2 = ctx
        .stack
        .local_u32_from_base(local_base, (*tail_code.add(3)).operand.local_addr as usize);
    let step = ctx
        .stack
        .local_u32_from_base(local_base, (*tail_code.add(4)).operand.local_addr as usize);
    let mut crc = ctx
        .stack
        .local_u32_from_base(local_base, (*tail_code.add(5)).operand.local_addr as usize);

    crc = vm_try!(unsafe { core_state_benchmark_crc(ctx, data, size, seed1, seed2, step, crc) });

    vm_try!(ctx.stack.push_u32_fast(crc));
    let return_addr = (*tail_code.add(6)).operand.jump_addr;
    call_next(ctx.code().offset(return_addr as isize), 0, ctx)
}

#[inline(always)]
fn sign_extend_i16_bits(value: u32) -> u32 {
    i32::from(value as u16 as i16) as u32
}

#[inline(always)]
unsafe fn core_matrix_sum_i32(
    ctx: &mut ExecuteContext,
    n: u32,
    c_ptr: u32,
    clip: u32,
) -> VMResult<u32> {
    let row_stride = n.wrapping_shl(2);
    let mut row = 0;
    let mut row_ptr = c_ptr;
    let mut acc = 0u32;
    let mut tally = 0u32;
    let mut prev = 0u32;
    while row != n {
        let mut col = 0;
        let mut ptr = row_ptr;
        while col != n {
            let value = vm_try!(unsafe { read_u32_linear(ctx, ptr) });
            let sum = acc.wrapping_add(value);
            let overflow = (sum as i32) > (clip as i32);
            acc = if overflow { 0 } else { sum };
            let increment = if overflow {
                ((value as i32) > (prev as i32)) as u32
            } else {
                10
            };
            tally = tally.wrapping_add(increment);
            prev = value;
            ptr = ptr.wrapping_add(4);
            col = col.wrapping_add(1);
        }
        row_ptr = row_ptr.wrapping_add(row_stride);
        row = row.wrapping_add(1);
    }
    VMResult::Success(tally)
}

#[inline(always)]
unsafe fn core_matrix_add_const_i16(
    ctx: &mut ExecuteContext,
    n: u32,
    a_ptr: u32,
    delta: u32,
) -> VMResult<()> {
    let row_stride = n.wrapping_shl(1);
    let mut row = 0;
    let mut row_ptr = a_ptr;
    while row != n {
        let mut col = 0;
        let mut ptr = row_ptr;
        while col != n {
            let value = u32::from(vm_try!(unsafe { read_u16_linear(ctx, ptr) }));
            vm_try!(unsafe { write_u16_linear(ctx, ptr, value.wrapping_add(delta) as u16) });
            ptr = ptr.wrapping_add(2);
            col = col.wrapping_add(1);
        }
        row_ptr = row_ptr.wrapping_add(row_stride);
        row = row.wrapping_add(1);
    }
    VMResult::Success(())
}

#[inline(always)]
unsafe fn core_matrix_mul_const_i16_i32(
    ctx: &mut ExecuteContext,
    n: u32,
    c_ptr: u32,
    a_ptr: u32,
    value: u32,
) -> VMResult<()> {
    let a_row_stride = n.wrapping_shl(1);
    let c_row_stride = n.wrapping_shl(2);
    let mut row = 0;
    let mut a_row = a_ptr;
    let mut c_row = c_ptr;
    while row != n {
        let mut col = 0;
        let mut a = a_row;
        let mut c = c_row;
        while col != n {
            let product = (vm_try!(unsafe { read_i16_linear(ctx, a) }) as u32).wrapping_mul(value);
            vm_try!(unsafe { write_u32_linear(ctx, c, product) });
            a = a.wrapping_add(2);
            c = c.wrapping_add(4);
            col = col.wrapping_add(1);
        }
        a_row = a_row.wrapping_add(a_row_stride);
        c_row = c_row.wrapping_add(c_row_stride);
        row = row.wrapping_add(1);
    }
    VMResult::Success(())
}

#[inline(always)]
unsafe fn core_matrix_mul_vect_i16_i32(
    ctx: &mut ExecuteContext,
    n: u32,
    c_ptr: u32,
    a_ptr: u32,
    b_ptr: u32,
) -> VMResult<()> {
    let a_row_stride = n.wrapping_shl(1);
    let mut row = 0;
    let mut a_row = a_ptr;
    while row != n {
        let mut k = 0;
        let mut a = a_row;
        let mut b = b_ptr;
        let mut acc = 0u32;
        while k != n {
            let av = vm_try!(unsafe { read_i16_linear(ctx, a) }) as u32;
            let bv = vm_try!(unsafe { read_i16_linear(ctx, b) }) as u32;
            acc = acc.wrapping_add(av.wrapping_mul(bv));
            a = a.wrapping_add(2);
            b = b.wrapping_add(2);
            k = k.wrapping_add(1);
        }
        vm_try!(unsafe { write_u32_linear(ctx, c_ptr.wrapping_add(row.wrapping_shl(2)), acc) });
        a_row = a_row.wrapping_add(a_row_stride);
        row = row.wrapping_add(1);
    }
    VMResult::Success(())
}

#[inline(always)]
unsafe fn core_matrix_mul_matrix_i16_i32(
    ctx: &mut ExecuteContext,
    n: u32,
    c_ptr: u32,
    a_ptr: u32,
    b_ptr: u32,
) -> VMResult<()> {
    let a_row_stride = n.wrapping_shl(1);
    let c_row_stride = n.wrapping_shl(2);
    let mut row = 0;
    let mut a_row = a_ptr;
    let mut c_row = c_ptr;
    while row != n {
        let mut col = 0;
        let mut b_col = b_ptr;
        while col != n {
            let mut k = 0;
            let mut a = a_row;
            let mut b = b_col;
            let mut acc = 0u32;
            while k != n {
                let av = vm_try!(unsafe { read_i16_linear(ctx, a) }) as u32;
                let bv = vm_try!(unsafe { read_i16_linear(ctx, b) }) as u32;
                acc = acc.wrapping_add(av.wrapping_mul(bv));
                a = a.wrapping_add(2);
                b = b.wrapping_add(a_row_stride);
                k = k.wrapping_add(1);
            }
            vm_try!(unsafe { write_u32_linear(ctx, c_row.wrapping_add(col.wrapping_shl(2)), acc) });
            b_col = b_col.wrapping_add(2);
            col = col.wrapping_add(1);
        }
        a_row = a_row.wrapping_add(a_row_stride);
        c_row = c_row.wrapping_add(c_row_stride);
        row = row.wrapping_add(1);
    }
    VMResult::Success(())
}

#[inline(always)]
unsafe fn core_matrix_mul_matrix_bitextract_i16_i32(
    ctx: &mut ExecuteContext,
    n: u32,
    c_ptr: u32,
    a_ptr: u32,
    b_ptr: u32,
) -> VMResult<()> {
    let a_row_stride = n.wrapping_shl(1);
    let c_row_stride = n.wrapping_shl(2);
    let mut row = 0;
    let mut a_row = a_ptr;
    let mut c_row = c_ptr;
    while row != n {
        let mut col = 0;
        let mut b_col = b_ptr;
        while col != n {
            let mut k = 0;
            let mut a = a_row;
            let mut b = b_col;
            let mut acc = 0u32;
            while k != n {
                let av = u32::from(vm_try!(unsafe { read_u16_linear(ctx, a) }));
                let bv = u32::from(vm_try!(unsafe { read_u16_linear(ctx, b) }));
                let product = av.wrapping_mul(bv);
                let mixed = ((product >> 2) & 15).wrapping_mul((product >> 5) & 127);
                acc = acc.wrapping_add(mixed);
                a = a.wrapping_add(2);
                b = b.wrapping_add(a_row_stride);
                k = k.wrapping_add(1);
            }
            vm_try!(unsafe { write_u32_linear(ctx, c_row.wrapping_add(col.wrapping_shl(2)), acc) });
            b_col = b_col.wrapping_add(2);
            col = col.wrapping_add(1);
        }
        a_row = a_row.wrapping_add(a_row_stride);
        c_row = c_row.wrapping_add(c_row_stride);
        row = row.wrapping_add(1);
    }
    VMResult::Success(())
}

#[inline(always)]
unsafe fn core_matrix_i16_crc_summary(
    ctx: &mut ExecuteContext,
    n: u32,
    c_ptr: u32,
    a_ptr: u32,
    b_ptr: u32,
    value: u32,
) -> VMResult<u32> {
    let mut crc = 0u32;
    if n == 0 {
        for _ in 0..4 {
            crc = super::numeric::crc16_update16_bits(0, crc);
        }
    } else {
        let clip = value | 0xffff_f000;
        vm_try!(unsafe { core_matrix_add_const_i16(ctx, n, a_ptr, value) });
        vm_try!(unsafe { core_matrix_mul_const_i16_i32(ctx, n, c_ptr, a_ptr, value) });
        crc = super::numeric::crc16_update16_bits(
            vm_try!(unsafe { core_matrix_sum_i32(ctx, n, c_ptr, clip) }),
            crc,
        );
        vm_try!(unsafe { core_matrix_mul_vect_i16_i32(ctx, n, c_ptr, a_ptr, b_ptr) });
        crc = super::numeric::crc16_update16_bits(
            vm_try!(unsafe { core_matrix_sum_i32(ctx, n, c_ptr, clip) }),
            crc,
        );
        vm_try!(unsafe { core_matrix_mul_matrix_i16_i32(ctx, n, c_ptr, a_ptr, b_ptr) });
        crc = super::numeric::crc16_update16_bits(
            vm_try!(unsafe { core_matrix_sum_i32(ctx, n, c_ptr, clip) }),
            crc,
        );
        vm_try!(unsafe { core_matrix_mul_matrix_bitextract_i16_i32(ctx, n, c_ptr, a_ptr, b_ptr) });
        crc = super::numeric::crc16_update16_bits(
            vm_try!(unsafe { core_matrix_sum_i32(ctx, n, c_ptr, clip) }),
            crc,
        );
        vm_try!(unsafe { core_matrix_add_const_i16(ctx, n, a_ptr, value.wrapping_neg()) });
    }
    VMResult::Success(sign_extend_i16_bits(crc))
}

#[inline(always)]
fn crc16_masked(value: u32, crc: u32) -> u32 {
    super::numeric::crc16_update16_bits(value & 0xffff, crc)
}

#[inline(always)]
unsafe fn list_next(ctx: &mut ExecuteContext, node: u32) -> VMResult<u32> {
    unsafe { read_u32_linear(ctx, node) }
}

#[inline(always)]
unsafe fn list_set_next(ctx: &mut ExecuteContext, node: u32, next: u32) -> VMResult<()> {
    unsafe { write_u32_linear(ctx, node, next) }
}

#[inline(always)]
unsafe fn list_info(ctx: &mut ExecuteContext, node: u32) -> VMResult<u32> {
    unsafe { read_u32_linear(ctx, node.wrapping_add(4)) }
}

#[inline(always)]
unsafe fn list_set_info(ctx: &mut ExecuteContext, node: u32, info: u32) -> VMResult<()> {
    unsafe { write_u32_linear(ctx, node.wrapping_add(4), info) }
}

#[inline(always)]
unsafe fn list_data16(ctx: &mut ExecuteContext, info: u32) -> VMResult<u32> {
    VMResult::Success(u32::from(vm_try!(unsafe { read_u16_linear(ctx, info) })))
}

#[inline(always)]
unsafe fn list_set_data16(ctx: &mut ExecuteContext, info: u32, data: u32) -> VMResult<()> {
    unsafe { write_u16_linear(ctx, info, data as u16) }
}

#[inline(always)]
unsafe fn list_idx(ctx: &mut ExecuteContext, info: u32) -> VMResult<i32> {
    unsafe { read_i16_linear(ctx, info.wrapping_add(2)) }
}

#[inline(always)]
unsafe fn core_list_find(
    ctx: &mut ExecuteContext,
    mut list: u32,
    idx: u32,
    data16: u32,
) -> VMResult<u32> {
    if (idx as u16 as i16) >= 0 {
        let needle = idx & 0xffff;
        while list != 0 {
            let info = vm_try!(unsafe { list_info(ctx, list) });
            if u32::from(vm_try!(unsafe {
                read_u16_linear(ctx, info.wrapping_add(2))
            })) == needle
            {
                return VMResult::Success(list);
            }
            list = vm_try!(unsafe { list_next(ctx, list) });
        }
    } else {
        let needle = data16 & 0xff;
        while list != 0 {
            let info = vm_try!(unsafe { list_info(ctx, list) });
            if u32::from(vm_try!(unsafe { read_u8_linear(ctx, info) })) == needle {
                return VMResult::Success(list);
            }
            list = vm_try!(unsafe { list_next(ctx, list) });
        }
    }
    VMResult::Success(0)
}

#[inline(always)]
unsafe fn core_list_reverse(ctx: &mut ExecuteContext, mut list: u32) -> VMResult<u32> {
    let mut next = 0u32;
    while list != 0 {
        let tmp = vm_try!(unsafe { list_next(ctx, list) });
        vm_try!(unsafe { list_set_next(ctx, list, next) });
        next = list;
        list = tmp;
    }
    VMResult::Success(next)
}

#[inline(always)]
unsafe fn core_list_remove(ctx: &mut ExecuteContext, item: u32) -> VMResult<u32> {
    let removed = vm_try!(unsafe { list_next(ctx, item) });
    let item_info = vm_try!(unsafe { list_info(ctx, item) });
    let removed_info = vm_try!(unsafe { list_info(ctx, removed) });
    vm_try!(unsafe { list_set_info(ctx, item, removed_info) });
    vm_try!(unsafe { list_set_info(ctx, removed, item_info) });
    let removed_next = vm_try!(unsafe { list_next(ctx, removed) });
    vm_try!(unsafe { list_set_next(ctx, item, removed_next) });
    vm_try!(unsafe { list_set_next(ctx, removed, 0) });
    VMResult::Success(removed)
}

#[inline(always)]
unsafe fn core_list_undo_remove(
    ctx: &mut ExecuteContext,
    removed: u32,
    modified: u32,
) -> VMResult<u32> {
    let removed_info = vm_try!(unsafe { list_info(ctx, removed) });
    let modified_info = vm_try!(unsafe { list_info(ctx, modified) });
    vm_try!(unsafe { list_set_info(ctx, removed, modified_info) });
    vm_try!(unsafe { list_set_info(ctx, modified, removed_info) });
    let modified_next = vm_try!(unsafe { list_next(ctx, modified) });
    vm_try!(unsafe { list_set_next(ctx, removed, modified_next) });
    vm_try!(unsafe { list_set_next(ctx, modified, removed) });
    VMResult::Success(removed)
}

#[inline(always)]
unsafe fn core_list_calc_func(ctx: &mut ExecuteContext, data_ptr: u32, res: u32) -> VMResult<u32> {
    let data = u32::from(vm_try!(unsafe { read_u16_linear(ctx, data_ptr) }));
    if data & 0x80 != 0 {
        return VMResult::Success(data & 0x7f);
    }

    let flag = data & 0x7;
    let mut dtype = (data >> 3) & 0xf;
    dtype |= dtype << 4;
    let retval = match flag {
        0 => {
            let step = dtype.max(0x22);
            let size = vm_try!(unsafe { read_u32_linear(ctx, res.wrapping_add(24)) });
            let state_data = vm_try!(unsafe { read_u32_linear(ctx, res.wrapping_add(20)) });
            let seed1 = vm_try!(unsafe { read_i16_linear(ctx, res) }) as u32;
            let seed2 = vm_try!(unsafe { read_i16_linear(ctx, res.wrapping_add(2)) }) as u32;
            let crc = u32::from(vm_try!(unsafe {
                read_u16_linear(ctx, res.wrapping_add(56))
            }));
            let state_crc = vm_try!(unsafe {
                core_state_benchmark_crc(ctx, state_data, size, seed1, seed2, step, crc)
            });
            if vm_try!(unsafe { read_u16_linear(ctx, res.wrapping_add(62)) }) == 0 {
                vm_try!(unsafe { write_u16_linear(ctx, res.wrapping_add(62), state_crc as u16) });
            }
            state_crc
        }
        1 => {
            let mat = res.wrapping_add(40);
            let n = vm_try!(unsafe { read_u32_linear(ctx, mat) });
            let a = vm_try!(unsafe { read_u32_linear(ctx, mat.wrapping_add(4)) });
            let b = vm_try!(unsafe { read_u32_linear(ctx, mat.wrapping_add(8)) });
            let c = vm_try!(unsafe { read_u32_linear(ctx, mat.wrapping_add(12)) });
            let matrix_crc =
                vm_try!(unsafe { core_matrix_i16_crc_summary(ctx, n, c, a, b, dtype) });
            if vm_try!(unsafe { read_u16_linear(ctx, res.wrapping_add(60)) }) == 0 {
                vm_try!(unsafe { write_u16_linear(ctx, res.wrapping_add(60), matrix_crc as u16) });
            }
            matrix_crc
        }
        _ => i32::from(data as u16 as i16) as u32,
    };

    let crc = u32::from(vm_try!(unsafe {
        read_u16_linear(ctx, res.wrapping_add(56))
    }));
    let next_crc = crc16_masked(retval, crc);
    vm_try!(unsafe { write_u16_linear(ctx, res.wrapping_add(56), next_crc as u16) });
    let cached = retval & 0x7f;
    vm_try!(unsafe { list_set_data16(ctx, data_ptr, (data & 0xff00) | 0x80 | cached) });
    VMResult::Success(cached)
}

#[inline(always)]
unsafe fn core_list_cmp_complex(
    ctx: &mut ExecuteContext,
    a_info: u32,
    b_info: u32,
    res: u32,
) -> VMResult<i32> {
    let a = vm_try!(unsafe { core_list_calc_func(ctx, a_info, res) });
    let b = vm_try!(unsafe { core_list_calc_func(ctx, b_info, res) });
    VMResult::Success(a as i32 - b as i32)
}

#[inline(always)]
unsafe fn core_list_cmp_idx(ctx: &mut ExecuteContext, a_info: u32, b_info: u32) -> VMResult<i32> {
    let a_data = vm_try!(unsafe { list_data16(ctx, a_info) });
    let b_data = vm_try!(unsafe { list_data16(ctx, b_info) });
    vm_try!(unsafe { list_set_data16(ctx, a_info, (a_data & 0xff00) | ((a_data >> 8) & 0xff)) });
    vm_try!(unsafe { list_set_data16(ctx, b_info, (b_data & 0xff00) | ((b_data >> 8) & 0xff)) });
    let a_idx = vm_try!(unsafe { list_idx(ctx, a_info) });
    let b_idx = vm_try!(unsafe { list_idx(ctx, b_info) });
    VMResult::Success(a_idx - b_idx)
}

#[inline(always)]
unsafe fn core_list_compare(
    ctx: &mut ExecuteContext,
    p: u32,
    q: u32,
    res: u32,
    complex: bool,
) -> VMResult<i32> {
    let p_info = vm_try!(unsafe { list_info(ctx, p) });
    let q_info = vm_try!(unsafe { list_info(ctx, q) });
    if complex {
        unsafe { core_list_cmp_complex(ctx, p_info, q_info, res) }
    } else {
        unsafe { core_list_cmp_idx(ctx, p_info, q_info) }
    }
}

#[inline(always)]
unsafe fn core_list_mergesort(
    ctx: &mut ExecuteContext,
    mut list: u32,
    res: u32,
    complex: bool,
) -> VMResult<u32> {
    let mut insize = 1u32;
    loop {
        let mut p = list;
        list = 0;
        let mut tail = 0u32;
        let mut nmerges = 0u32;
        while p != 0 {
            nmerges = nmerges.wrapping_add(1);
            let mut q = p;
            let mut psize = 0u32;
            for _ in 0..insize {
                psize = psize.wrapping_add(1);
                q = vm_try!(unsafe { list_next(ctx, q) });
                if q == 0 {
                    break;
                }
            }
            let mut qsize = insize;
            while psize > 0 || (qsize > 0 && q != 0) {
                let e;
                if psize == 0 {
                    e = q;
                    q = vm_try!(unsafe { list_next(ctx, q) });
                    qsize = qsize.wrapping_sub(1);
                } else if qsize == 0
                    || q == 0
                    || vm_try!(unsafe { core_list_compare(ctx, p, q, res, complex) }) <= 0
                {
                    e = p;
                    p = vm_try!(unsafe { list_next(ctx, p) });
                    psize = psize.wrapping_sub(1);
                } else {
                    e = q;
                    q = vm_try!(unsafe { list_next(ctx, q) });
                    qsize = qsize.wrapping_sub(1);
                }
                if tail != 0 {
                    vm_try!(unsafe { list_set_next(ctx, tail, e) });
                } else {
                    list = e;
                }
                tail = e;
            }
            p = q;
        }
        if tail != 0 {
            vm_try!(unsafe { list_set_next(ctx, tail, 0) });
        }
        if nmerges <= 1 {
            return VMResult::Success(list);
        }
        insize = insize.wrapping_shl(1);
    }
}

#[inline(always)]
unsafe fn core_list_benchmark_summary(
    ctx: &mut ExecuteContext,
    res: u32,
    finder_idx: u32,
) -> VMResult<u32> {
    let mut retval = 0u32;
    let mut found = 0u32;
    let mut missed = 0u32;
    let mut list = vm_try!(unsafe { read_u32_linear(ctx, res.wrapping_add(36)) });
    let find_num = vm_try!(unsafe { read_i16_linear(ctx, res.wrapping_add(4)) });
    let mut info_idx = finder_idx;
    let mut info_data = 0u32;

    if find_num >= 1 {
        let mut i = 0i32;
        while i < find_num {
            info_data = (i as u32) & 0xff;
            let this_find = vm_try!(unsafe { core_list_find(ctx, list, info_idx, info_data) });
            list = vm_try!(unsafe { core_list_reverse(ctx, list) });
            if this_find == 0 {
                missed = missed.wrapping_add(1);
                let next = vm_try!(unsafe { list_next(ctx, list) });
                let next_info = vm_try!(unsafe { list_info(ctx, next) });
                let high = u32::from(vm_try!(unsafe {
                    read_u8_linear(ctx, next_info.wrapping_add(1))
                }));
                retval = retval.wrapping_add(high & 1);
            } else {
                found = found.wrapping_add(1);
                let find_info = vm_try!(unsafe { list_info(ctx, this_find) });
                let data = vm_try!(unsafe { list_data16(ctx, find_info) });
                if data & 1 != 0 {
                    retval = retval.wrapping_add((data >> 9) & 1);
                }
                let finder = vm_try!(unsafe { list_next(ctx, this_find) });
                if finder != 0 {
                    let finder_next = vm_try!(unsafe { list_next(ctx, finder) });
                    let list_next = vm_try!(unsafe { list_next(ctx, list) });
                    vm_try!(unsafe { list_set_next(ctx, this_find, finder_next) });
                    vm_try!(unsafe { list_set_next(ctx, finder, list_next) });
                    vm_try!(unsafe { list_set_next(ctx, list, finder) });
                }
            }
            if (info_idx as u16 as i16) >= 0 {
                info_idx = info_idx.wrapping_add(1);
            }
            i += 1;
        }
    }

    retval = retval
        .wrapping_add(found.wrapping_shl(2))
        .wrapping_sub(missed);
    if (finder_idx as i32) > 0 {
        list = vm_try!(unsafe { core_list_mergesort(ctx, list, res, true) });
    }

    let first_item = vm_try!(unsafe { list_next(ctx, list) });
    let remover = vm_try!(unsafe { core_list_remove(ctx, first_item) });
    let mut finder = vm_try!(unsafe { core_list_find(ctx, list, info_idx, info_data) });
    if finder == 0 {
        finder = vm_try!(unsafe { list_next(ctx, list) });
    }
    if finder != 0 {
        let head_info = vm_try!(unsafe { list_info(ctx, list) });
        let head_data = vm_try!(unsafe { list_data16(ctx, head_info) });
        let mut cursor = finder;
        while cursor != 0 {
            retval = crc16_masked(head_data, retval);
            cursor = vm_try!(unsafe { list_next(ctx, cursor) });
        }
    }

    let modified = vm_try!(unsafe { list_next(ctx, list) });
    vm_try!(unsafe { core_list_undo_remove(ctx, remover, modified) });
    list = vm_try!(unsafe { core_list_mergesort(ctx, list, 0, false) });

    let mut cursor = vm_try!(unsafe { list_next(ctx, list) });
    if cursor != 0 {
        let head_info = vm_try!(unsafe { list_info(ctx, list) });
        let head_data = vm_try!(unsafe { list_data16(ctx, head_info) });
        while cursor != 0 {
            retval = crc16_masked(head_data, retval);
            cursor = vm_try!(unsafe { list_next(ctx, cursor) });
        }
    }
    VMResult::Success(retval & 0xffff)
}

#[inline(always)]
/// Telomere internal linked-list benchmark summary helper used by list CRC fusions.
///
/// # Safety
/// - `res` must address the validated benchmark result structure in default linear memory.
/// - `ctx` must hold the active frame and default local memory selected by the wrapper handler.
pub(crate) unsafe fn list_crc_summary_value(
    ctx: &mut ExecuteContext,
    res: u32,
    finder_idx: u32,
) -> VMResult<u32> {
    unsafe { core_list_benchmark_summary(ctx, res, finder_idx) }
}

#[allow(dead_code)]
/// Telomere internal counted loop for repeated summary-call pairs folded through CRC16.
///
/// The instantiator selects this only after seeing the explicit loop shape made of two calls to a
/// verified `op_i32_list_crc_summary` target and two CRC16 folds. It preserves the loop's linear
/// memory update order for the shared CRC field.
///
/// # Safety
/// - `tail_code` must point to a frame-base local, result/iteration/CRC offsets, and a jump target.
/// - The active frame and memory offsets must match the validated loop shape.
pub unsafe fn op_i32_list_crc_pair_loop(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    profile_memory_family("op_i32_list_crc_pair_loop");
    let local_base = ctx.local_base_ptr as *const u8;
    let frame_base_local = (*tail_code).operand.local_addr as usize;
    let res_delta = (*tail_code.add(1)).operand.u32;
    let iterations_delta = (*tail_code.add(2)).operand.u32;
    let crc_delta = (*tail_code.add(3)).operand.u32;
    let jump_addr = (*tail_code.add(4)).operand.jump_addr;

    let frame_base = ctx.stack.local_u32_from_base(local_base, frame_base_local);
    let res = frame_base.wrapping_add(res_delta);
    let iterations =
        vm_try!(unsafe { read_u32_linear(ctx, frame_base.wrapping_add(iterations_delta)) });
    let crc_addr = frame_base.wrapping_add(crc_delta);
    vm_try!(unsafe { write_u32_linear(ctx, crc_addr, 0) });
    vm_try!(unsafe { write_u32_linear(ctx, crc_addr.wrapping_add(4), 0) });

    let mut i = 0u32;
    while i != iterations {
        let positive = vm_try!(unsafe { list_crc_summary_value(ctx, res, 1) });
        let crc = u32::from(vm_try!(unsafe { read_u16_linear(ctx, crc_addr) }));
        vm_try!(unsafe { write_u16_linear(ctx, crc_addr, crc16_masked(positive, crc) as u16) });

        let negative = vm_try!(unsafe { list_crc_summary_value(ctx, res, u32::MAX) });
        let crc = u32::from(vm_try!(unsafe { read_u16_linear(ctx, crc_addr) }));
        let crc = crc16_masked(negative, crc);
        if i == 0 {
            vm_try!(unsafe { write_u16_linear(ctx, crc_addr.wrapping_add(2), crc as u16) });
        }
        vm_try!(unsafe { write_u16_linear(ctx, crc_addr, crc as u16) });
        i = i.wrapping_add(1);
    }

    call_next(ctx.code().offset(jump_addr as isize), 0, ctx)
}

#[allow(dead_code)]
/// Telomere internal linked-list benchmark summary selected for a verified list/CRC loop nest.
///
/// # Safety
/// - `tail_code` must point to two i32 local operands and a function-return target.
/// - The active frame must match the validated linked-list benchmark shape.
pub unsafe fn op_i32_list_crc_summary(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    profile_memory_family("op_i32_list_crc_summary");
    let local_base = ctx.local_base_ptr as *const u8;
    let res = ctx
        .stack
        .local_u32_from_base(local_base, (*tail_code).operand.local_addr as usize);
    let finder_idx = ctx
        .stack
        .local_u32_from_base(local_base, (*tail_code.add(1)).operand.local_addr as usize);
    let retval = vm_try!(unsafe { list_crc_summary_value(ctx, res, finder_idx) });
    vm_try!(ctx.stack.push_u32_fast(retval));
    let return_addr = (*tail_code.add(2)).operand.jump_addr;
    call_next(ctx.code().offset(return_addr as isize), 0, ctx)
}

#[allow(dead_code)]
/// Telomere internal i16/i32 matrix workload selected from a validated matrix/CRC loop nest.
///
/// The instantiator selects this only for a closed shape made of i16 matrix updates, i32
/// reductions, and CRC16 fold calls. It is a function summary, not a change to call threading.
///
/// # Safety
/// - `tail_code` must point to five i32 local operands and a function-return target.
/// - The active frame must match the validated lowered loop-nest shape.
pub unsafe fn op_i32_matrix_i16_crc_summary(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    profile_memory_family("op_i32_matrix_i16_crc_summary");
    let local_base = ctx.local_base_ptr as *const u8;
    let n = ctx
        .stack
        .local_u32_from_base(local_base, (*tail_code).operand.local_addr as usize);
    let c_ptr = ctx
        .stack
        .local_u32_from_base(local_base, (*tail_code.add(1)).operand.local_addr as usize);
    let a_ptr = ctx
        .stack
        .local_u32_from_base(local_base, (*tail_code.add(2)).operand.local_addr as usize);
    let b_ptr = ctx
        .stack
        .local_u32_from_base(local_base, (*tail_code.add(3)).operand.local_addr as usize);
    let value = ctx
        .stack
        .local_u32_from_base(local_base, (*tail_code.add(4)).operand.local_addr as usize);

    let crc = vm_try!(unsafe { core_matrix_i16_crc_summary(ctx, n, c_ptr, a_ptr, b_ptr, value) });
    vm_try!(ctx.stack.push_u32_fast(crc));
    let return_addr = (*tail_code.add(5)).operand.jump_addr;
    call_next(ctx.code().offset(return_addr as isize), 0, ctx)
}

#[inline(always)]
unsafe fn eval_integer_i32_local_cmp32_at(
    tail_code: *const Instr,
    kind_offset: usize,
    lhs_offset: usize,
    rhs_offset: usize,
    ctx: &mut ExecuteContext,
) -> VMResult<u32> {
    let kind = (*tail_code.add(kind_offset)).operand.u32;
    let Some((op, rhs_shape)) = decode_local_cmp32_kind(kind) else {
        return VMResult::InvalidOperand;
    };
    let lhs_addr = (*tail_code.add(lhs_offset)).operand.local_addr as usize;
    let local_base = ctx.local_base_ptr as *const u8;
    let lhs = ctx.stack.local_u32_from_base(local_base, lhs_addr);
    let rhs = match rhs_shape {
        LocalFastRhsShape::Local => {
            let rhs_addr = (*tail_code.add(rhs_offset)).operand.local_addr as usize;
            ctx.stack.local_u32_from_base(local_base, rhs_addr)
        }
        LocalFastRhsShape::Const => (*tail_code.add(rhs_offset)).operand.i32 as u32,
    };
    let matched = match op {
        LocalCmp32Op::I32Eq => lhs == rhs,
        LocalCmp32Op::I32Ne => lhs != rhs,
        LocalCmp32Op::I32LtS => (lhs as i32) < (rhs as i32),
        LocalCmp32Op::I32LtU => lhs < rhs,
        LocalCmp32Op::I32GtS => (lhs as i32) > (rhs as i32),
        LocalCmp32Op::I32GtU => lhs > rhs,
        LocalCmp32Op::I32LeS => (lhs as i32) <= (rhs as i32),
        LocalCmp32Op::I32LeU => lhs <= rhs,
        LocalCmp32Op::I32GeS => (lhs as i32) >= (rhs as i32),
        LocalCmp32Op::I32GeU => lhs >= rhs,
        LocalCmp32Op::F32Eq
        | LocalCmp32Op::F32Ne
        | LocalCmp32Op::F32Lt
        | LocalCmp32Op::F32Gt
        | LocalCmp32Op::F32Le
        | LocalCmp32Op::F32Ge => return VMResult::InvalidOperand,
    };
    VMResult::Success(u32::from(matched))
}

#[inline(always)]
unsafe fn guarded_load8_u_update_branch_cond(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<(u32, u32)> {
    let next_src = (*tail_code).operand.local_addr as usize;
    let next_delta = (*tail_code.add(1)).operand.i32 as u32;
    let next_dst = (*tail_code.add(2)).operand.local_addr as usize;
    let local_base = ctx.local_base_ptr as *const u8;
    let next = ctx
        .stack
        .local_u32_from_base(local_base, next_src)
        .wrapping_add(next_delta);
    ctx.stack
        .local_set4_from_base_value(ctx.local_base_ptr, next_dst, next);

    let false_addr = (*tail_code.add(6)).operand.jump_addr;
    let guard = vm_try!(unsafe { eval_integer_i32_local_cmp32_at(tail_code, 3, 4, 5, ctx) });
    if guard == 0 {
        return VMResult::Success((0, false_addr));
    }

    let ptr_local = (*tail_code.add(7)).operand.local_addr as usize;
    let load_delta = (*tail_code.add(8)).operand.i32 as u32;
    let memarg = (*tail_code.add(9)).operand.memarg;
    let byte_dst = (*tail_code.add(10)).operand.local_addr as usize;
    let update_src = (*tail_code.add(11)).operand.local_addr as usize;
    let ptr_dst = (*tail_code.add(12)).operand.local_addr as usize;
    let branch_local = (*tail_code.add(13)).operand.local_addr as usize;
    let offset = ctx
        .stack
        .local_u32_from_base(local_base, ptr_local)
        .wrapping_add(load_delta);
    let start = vm_try!(compute_memory_offset(memarg, offset));
    let value = u32::from(vm_try!(
        unsafe { ctx.default_local_memory_unchecked() }.read_u8_at(start)
    ));
    ctx.stack
        .local_set4_from_base_value(ctx.local_base_ptr, byte_dst, value);
    let updated = ctx.stack.local_u32_from_base(local_base, update_src);
    ctx.stack
        .local_set4_from_base_value(ctx.local_base_ptr, ptr_dst, updated);
    let cond = ctx.stack.local_u32_from_base(local_base, branch_local);
    VMResult::Success((cond, false_addr))
}

#[allow(dead_code)]
/// WebAssembly `i32.load8_u; local.set; local.get; local.set; local.get; br_if` cursor advance fusion.
///
/// # Safety
/// - `tail_code` must point to operands decoded for this load/set/update/branch handler.
/// - `ctx` must hold a valid frame, local base, and default memory for the active module.
/// - The branch target operand must be a valid target in the active function body.
pub unsafe fn op_i32_load8_u_local_base_set4_local_get4_set4_local_get4_br_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    profile_memory_family("op_i32_load8_u_local_base_set4_local_get4_set4_local_get4_br_if");
    let ptr = vm_try!(unsafe {
        i32_load8_u_update_br_if_ptr_at(
            tail_code,
            Load8UpdateBranchLayout {
                ptr_local_offset: 0,
                load_delta_offset: 1,
                memarg_offset: 2,
                byte_dst_offset: 3,
                next_src_offset: 4,
                ptr_dst_offset: 5,
                branch_local_offset: 6,
                branch_target_offset: 7,
                fallthrough_advance: 8,
            },
            ctx,
        )
    });
    call_next(ptr, 0, ctx)
}

#[allow(dead_code)]
/// WebAssembly `i32.load8_u` guarded byte cursor advance fusion.
///
/// This covers a generic loop tail shaped as:
/// `local.get ptr; i32.const k; i32.add; local.set next;
///  local cmp guard; if { i32.load8_u; local.set; local.get next; local.set ptr; local.get byte; br_if }`.
///
/// # Safety
/// - `tail_code` must point to operands decoded for this guarded cursor handler.
/// - The guard comparison must be an integer i32 local/constant comparison.
/// - `ctx` must hold a valid frame, local base, and default memory for the active module.
pub unsafe fn op_i32_guarded_load8_u_local_base_set4_local_get4_set4_local_get4_br_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    profile_memory_family(
        "op_i32_guarded_load8_u_local_base_set4_local_get4_set4_local_get4_br_if",
    );
    let (cond, false_addr) = vm_try!(unsafe { guarded_load8_u_update_branch_cond(tail_code, ctx) });
    let ptr = if cond != 0 {
        let jump_addr = (*tail_code.add(14)).operand.jump_addr;
        ctx.code().offset(jump_addr as isize)
    } else {
        ctx.code().offset(false_addr as isize)
    };
    call_next(skip_end_ops(ptr), 0, ctx)
}

#[allow(dead_code)]
/// WebAssembly `i32.load8_u` guarded byte cursor advance with a taken `local.get; br_table` target.
///
/// # Safety
/// - `tail_code` must point to operands decoded for the guarded cursor handler followed by
///   a materialized `local.get4; br_table` operand group for the taken target.
/// - The branch targets must be valid for the current active function body.
pub unsafe fn op_i32_guarded_load8_u_local_base_set4_local_get4_set4_local_get4_br_if_taken_local_get4_br_table(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    profile_memory_family(
        "op_i32_guarded_load8_u_local_base_set4_local_get4_set4_local_get4_br_if_taken_local_get4_br_table",
    );
    let (cond, false_addr) = vm_try!(unsafe { guarded_load8_u_update_branch_cond(tail_code, ctx) });
    let ptr = if cond != 0 {
        let table_local = (*tail_code.add(15)).operand.local_addr as usize;
        let index = ctx
            .stack
            .local_u32_from_base(ctx.local_base_ptr as *const u8, table_local);
        unsafe { br_table_target_ptr_at(tail_code, 16, 17, index, ctx) }
    } else {
        let ptr = ctx.code().offset(false_addr as isize);
        skip_end_ops(ptr)
    };
    call_next(ptr, 0, ctx)
}

#[allow(dead_code)]
/// WebAssembly `i32.load8_u` guarded byte cursor advance with a false `local.get; br_table` target.
///
/// # Safety
/// - `tail_code` must point to operands decoded for the guarded cursor handler followed by
///   a materialized `local.get4; br_table` operand group for the guard-false target.
/// - The branch targets must be valid for the current active function body.
pub unsafe fn op_i32_guarded_load8_u_local_base_set4_local_get4_set4_local_get4_br_if_false_local_get4_br_table(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    profile_memory_family(
        "op_i32_guarded_load8_u_local_base_set4_local_get4_set4_local_get4_br_if_false_local_get4_br_table",
    );
    let (cond, _) = vm_try!(unsafe { guarded_load8_u_update_branch_cond(tail_code, ctx) });
    let ptr = if cond != 0 {
        let jump_addr = (*tail_code.add(14)).operand.jump_addr;
        skip_end_ops(ctx.code().offset(jump_addr as isize))
    } else {
        let table_local = (*tail_code.add(15)).operand.local_addr as usize;
        let index = ctx
            .stack
            .local_u32_from_base(ctx.local_base_ptr as *const u8, table_local);
        unsafe { br_table_target_ptr_at(tail_code, 16, 17, index, ctx) }
    };
    call_next(ptr, 0, ctx)
}

#[allow(dead_code)]
/// WebAssembly `i32.load8_u` guarded byte cursor advance with a taken const-compare branch/table target.
///
/// The taken path covers `local.get; i32.const; compare; br_if; br`, where the `br`
/// target begins with `local.get; br_table`.
///
/// # Safety
/// - `tail_code` must point to operands decoded for the guarded cursor handler followed by
///   a materialized `local.get4; i32.const; compare; br_if`, an unconditional `br` target,
///   and a materialized `local.get4; br_table` operand group for the untaken compare path.
/// - All encoded branch targets must be valid for the current active function body.
pub unsafe fn op_i32_guarded_load8_u_local_base_set4_local_get4_set4_local_get4_br_if_taken_const_compare_br_table(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    profile_memory_family(
        "op_i32_guarded_load8_u_local_base_set4_local_get4_set4_local_get4_br_if_taken_const_compare_br_table",
    );
    let (cond, false_addr) = vm_try!(unsafe { guarded_load8_u_update_branch_cond(tail_code, ctx) });
    let ptr = if cond != 0 {
        let cmp_local = (*tail_code.add(15)).operand.local_addr as usize;
        let cmp_kind = (*tail_code.add(16)).operand.u32;
        let cmp_rhs = (*tail_code.add(17)).operand.i32;
        let lhs =
            ctx.stack
                .local_u32_from_base(ctx.local_base_ptr as *const u8, cmp_local) as i32;
        if i32_const_compare(cmp_kind, lhs, cmp_rhs) {
            let cmp_target = (*tail_code.add(18)).operand.jump_addr;
            skip_end_ops(ctx.code().offset(cmp_target as isize))
        } else {
            let table_local = (*tail_code.add(20)).operand.local_addr as usize;
            let index = ctx
                .stack
                .local_u32_from_base(ctx.local_base_ptr as *const u8, table_local);
            unsafe { br_table_target_ptr_at(tail_code, 21, 22, index, ctx) }
        }
    } else {
        let ptr = ctx.code().offset(false_addr as isize);
        skip_end_ops(ptr)
    };
    call_next(ptr, 0, ctx)
}

#[allow(dead_code)]
/// WebAssembly `i32.load8_u; local.set; local.get; local.set; local.get; br_if` with taken-target `local.get` fusion.
///
/// # Safety
/// - `tail_code` must point to operands decoded for this load/set/update/branch/taken-local handler.
/// - `ctx` must hold a valid frame, local base, and default memory for the active module.
/// - The encoded branch target must begin with the encoded `local.get4`, so the taken path may skip that target instruction after pushing its value.
pub unsafe fn op_i32_load8_u_local_base_set4_local_get4_set4_local_get4_br_if_taken_local_get4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    profile_memory_family(
        "op_i32_load8_u_local_base_set4_local_get4_set4_local_get4_br_if_taken_local_get4",
    );
    let ptr_local = (*tail_code).operand.local_addr as usize;
    let load_delta = (*tail_code.add(1)).operand.i32 as u32;
    let memarg = (*tail_code.add(2)).operand.memarg;
    let byte_dst = (*tail_code.add(3)).operand.local_addr as usize;
    let next_src = (*tail_code.add(4)).operand.local_addr as usize;
    let ptr_dst = (*tail_code.add(5)).operand.local_addr as usize;
    let branch_local = (*tail_code.add(6)).operand.local_addr as usize;
    let taken_local = (*tail_code.add(8)).operand.local_addr as usize;
    let local_base = ctx.local_base_ptr as *const u8;

    let offset = ctx
        .stack
        .local_u32_from_base(local_base, ptr_local)
        .wrapping_add(load_delta);
    let start = vm_try!(compute_memory_offset(memarg, offset));
    let value = u32::from(vm_try!(
        unsafe { ctx.default_local_memory_unchecked() }.read_u8_at(start)
    ));
    ctx.stack
        .local_set4_from_base_value(ctx.local_base_ptr, byte_dst, value);
    let next = ctx.stack.local_u32_from_base(local_base, next_src);
    ctx.stack
        .local_set4_from_base_value(ctx.local_base_ptr, ptr_dst, next);
    let cond = ctx.stack.local_u32_from_base(local_base, branch_local);
    if cond != 0 {
        let taken = ctx.stack.local_u32_from_base(local_base, taken_local);
        vm_try!(ctx.stack.push_u32_fast(taken));
        let jump_addr = (*tail_code.add(7)).operand.jump_addr;
        let target = ctx.code().offset(jump_addr as isize);
        return call_next(target, 2, ctx);
    }
    call_next(tail_code, 9, ctx)
}

#[allow(dead_code)]
/// WebAssembly `i32.load8_u; local.set; local.get; local.set; local.get; br_if` with fallthrough `local.get` fusion.
///
/// # Safety
/// - `tail_code` must point to operands decoded for this load/set/update/branch/fallthrough-local handler.
/// - `ctx` must hold a valid frame, local base, and default memory for the active module.
/// - The encoded fallthrough instruction must be the encoded `local.get4`, so the false path may skip it after pushing its value.
pub unsafe fn op_i32_load8_u_local_base_set4_local_get4_set4_local_get4_br_if_fallthrough_local_get4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    profile_memory_family(
        "op_i32_load8_u_local_base_set4_local_get4_set4_local_get4_br_if_fallthrough_local_get4",
    );
    let ptr_local = (*tail_code).operand.local_addr as usize;
    let load_delta = (*tail_code.add(1)).operand.i32 as u32;
    let memarg = (*tail_code.add(2)).operand.memarg;
    let byte_dst = (*tail_code.add(3)).operand.local_addr as usize;
    let next_src = (*tail_code.add(4)).operand.local_addr as usize;
    let ptr_dst = (*tail_code.add(5)).operand.local_addr as usize;
    let branch_local = (*tail_code.add(6)).operand.local_addr as usize;
    let fallthrough_local = (*tail_code.add(8)).operand.local_addr as usize;
    let fallthrough_advance = (*tail_code.add(9)).operand.u32 as isize;
    let local_base = ctx.local_base_ptr as *const u8;

    let offset = ctx
        .stack
        .local_u32_from_base(local_base, ptr_local)
        .wrapping_add(load_delta);
    let start = vm_try!(compute_memory_offset(memarg, offset));
    let value = u32::from(vm_try!(
        unsafe { ctx.default_local_memory_unchecked() }.read_u8_at(start)
    ));
    ctx.stack
        .local_set4_from_base_value(ctx.local_base_ptr, byte_dst, value);
    let next = ctx.stack.local_u32_from_base(local_base, next_src);
    ctx.stack
        .local_set4_from_base_value(ctx.local_base_ptr, ptr_dst, next);
    let cond = ctx.stack.local_u32_from_base(local_base, branch_local);
    if cond != 0 {
        let jump_addr = (*tail_code.add(7)).operand.jump_addr;
        let target = ctx.code().offset(jump_addr as isize);
        return call_next(target, 0, ctx);
    }
    let fallthrough = ctx.stack.local_u32_from_base(local_base, fallthrough_local);
    vm_try!(ctx.stack.push_u32_fast(fallthrough));
    call_next(tail_code, fallthrough_advance, ctx)
}

#[allow(dead_code)]
/// WebAssembly `i32` local-base increment; `i32.load8_u; local.set; local.get; local.set; local.get; br_if` fusion.
///
/// # Safety
/// - `tail_code` must point to operands decoded for this increment plus load/set/update/branch handler.
/// - `ctx` must hold a valid frame, local base, and default memory for the active module.
/// - The handler preserves increment load/store before the byte load and branch.
pub unsafe fn op_i32_inc_local_base_i32_load8_u_local_base_set4_local_get4_set4_local_get4_br_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    profile_memory_family(
        "op_i32_inc_local_base_i32_load8_u_local_base_set4_local_get4_set4_local_get4_br_if",
    );
    let inc_base_local = (*tail_code).operand.local_addr as usize;
    let inc_store_delta = (*tail_code.add(1)).operand.i32 as u32;
    let inc_load_delta = (*tail_code.add(2)).operand.i32 as u32;
    let inc_load_memarg = (*tail_code.add(3)).operand.memarg;
    let inc_store_memarg = (*tail_code.add(4)).operand.memarg;
    vm_try!(unsafe {
        i32_inc_local_base_at(
            ctx,
            inc_base_local,
            inc_store_delta,
            inc_load_delta,
            inc_load_memarg,
            inc_store_memarg,
        )
    });
    let fallthrough_advance = (*tail_code.add(13)).operand.u32 as usize;
    let ptr = vm_try!(unsafe {
        i32_load8_u_update_br_if_ptr_at(
            tail_code,
            Load8UpdateBranchLayout {
                ptr_local_offset: 5,
                load_delta_offset: 6,
                memarg_offset: 7,
                byte_dst_offset: 8,
                next_src_offset: 9,
                ptr_dst_offset: 10,
                branch_local_offset: 11,
                branch_target_offset: 12,
                fallthrough_advance,
            },
            ctx,
        )
    });
    call_next(ptr, 0, ctx)
}

#[allow(dead_code)]
/// WebAssembly `local.get; i32.const; i32.add; local.set; i32.load8_u; local.tee; i32.eqz; br_if` tail fusion.
///
/// # Safety
/// - `tail_code` must point to operands decoded for this encoded tail fusion.
/// - `ctx` must hold a valid frame, local base, and default memory for the active module.
pub unsafe fn op_local_get4_i32_const_add_set4_i32_load8_u_local_base_tee4_i32_eqz_br_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    profile_memory_family(
        "op_local_get4_i32_const_add_set4_i32_load8_u_local_base_tee4_i32_eqz_br_if",
    );
    let add_src = (*tail_code).operand.local_addr as usize;
    let imm = (*tail_code.add(1)).operand.i32 as u32;
    let add_dst = (*tail_code.add(2)).operand.local_addr as usize;
    let load_base = (*tail_code.add(3)).operand.local_addr as usize;
    let load_delta = (*tail_code.add(4)).operand.i32 as u32;
    let memarg = (*tail_code.add(5)).operand.memarg;
    let tee_dst = (*tail_code.add(6)).operand.local_addr as usize;
    let local_base = ctx.local_base_ptr as *const u8;

    let next = ctx
        .stack
        .local_u32_from_base(local_base, add_src)
        .wrapping_add(imm);
    ctx.stack
        .local_set4_from_base_value(ctx.local_base_ptr, add_dst, next);

    let load_addr = ctx
        .stack
        .local_u32_from_base(local_base, load_base)
        .wrapping_add(load_delta);
    let start = vm_try!(compute_memory_offset(memarg, load_addr));
    let value = u32::from(vm_try!(
        unsafe { ctx.default_local_memory_unchecked() }.read_u8_at(start)
    ));
    ctx.stack
        .local_set4_from_base_value(ctx.local_base_ptr, tee_dst, value);

    if value == 0 {
        let addr = (*tail_code.add(7)).operand.jump_addr;
        let ptr = ctx.code().offset(addr as isize);
        return call_next(ptr, 0, ctx);
    }
    call_next(tail_code, 14, ctx)
}

#[allow(dead_code)]
/// WebAssembly `i32.load; local.tee; i32.load8_u; local.tee; br_if` tail fusion.
///
/// # Safety
/// - `tail_code` must point to operands decoded for this encoded tail fusion.
/// - `ctx` must hold a valid frame, local base, and default memory for the active module.
pub unsafe fn op_i32_load_local_base_tee4_i32_load8_u_tee4_br_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    profile_memory_family("op_i32_load_local_base_tee4_i32_load8_u_tee4_br_if");
    let first_start = vm_try!(load_start_local_base(tail_code, ctx));
    let first_dst = (*tail_code.add(3)).operand.local_addr as usize;
    let ptr = vm_try!(unsafe { ctx.default_local_memory_unchecked() }.read_u32_at(first_start));
    ctx.stack
        .local_set4_from_base_value(ctx.local_base_ptr, first_dst, ptr);

    let byte_memarg = (*tail_code.add(4)).operand.memarg;
    let byte_dst = (*tail_code.add(5)).operand.local_addr as usize;
    let byte_start = vm_try!(compute_memory_offset(byte_memarg, ptr));
    let byte = u32::from(vm_try!(
        unsafe { ctx.default_local_memory_unchecked() }.read_u8_at(byte_start)
    ));
    ctx.stack
        .local_set4_from_base_value(ctx.local_base_ptr, byte_dst, byte);

    if byte != 0 {
        let addr = (*tail_code.add(6)).operand.jump_addr;
        let ptr = ctx.code().offset(addr as isize);
        return call_next(ptr, 0, ctx);
    }
    call_next(tail_code, 11, ctx)
}

#[allow(dead_code)]
/// WebAssembly `i32.store8` with a local-base address and local.get value on default local memory.
///
/// # Safety
/// - `tail_code` must point to operands decoded for this narrow local-base store handler.
/// - `ctx` must hold a valid frame, local base, and default memory for the active module.
pub unsafe fn op_i32_store8_local_base_local_get4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    profile_memory_family("op_i32_store8_local_base_local_get4");
    let addr_local = (*tail_code).operand.local_addr as usize;
    let delta = (*tail_code.add(1)).operand.i32 as u32;
    let value_local = (*tail_code.add(2)).operand.local_addr as usize;
    let memarg = (*tail_code.add(3)).operand.memarg;
    let local_base = ctx.local_base_ptr as *const u8;
    let offset = ctx
        .stack
        .local_u32_from_base(local_base, addr_local)
        .wrapping_add(delta);
    let value = ctx.stack.local_u32_from_base(local_base, value_local);
    let start = vm_try!(compute_memory_offset(memarg, offset));
    vm_try!(unsafe { ctx.default_local_memory_mut_unchecked() }
        .write_bytes(start, &truncate_u32_to_u8_bytes(value)));
    call_next(tail_code, 4, ctx)
}

#[allow(dead_code)]
/// WebAssembly `i32.store16` with a local-base address and local.get value on default local memory.
///
/// # Safety
/// - `tail_code` must point to operands decoded for this narrow local-base store handler.
/// - `ctx` must hold a valid frame, local base, and default memory for the active module.
pub unsafe fn op_i32_store16_local_base_local_get4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    profile_memory_family("op_i32_store16_local_base_local_get4");
    let addr_local = (*tail_code).operand.local_addr as usize;
    let delta = (*tail_code.add(1)).operand.i32 as u32;
    let value_local = (*tail_code.add(2)).operand.local_addr as usize;
    let memarg = (*tail_code.add(3)).operand.memarg;
    let local_base = ctx.local_base_ptr as *const u8;
    let offset = ctx
        .stack
        .local_u32_from_base(local_base, addr_local)
        .wrapping_add(delta);
    let value = ctx.stack.local_u32_from_base(local_base, value_local);
    let start = vm_try!(compute_memory_offset(memarg, offset));
    vm_try!(unsafe { ctx.default_local_memory_mut_unchecked() }
        .write_bytes(start, &truncate_u32_to_u16_bytes(value)));
    call_next(tail_code, 4, ctx)
}

#[inline(always)]
#[allow(dead_code)]
unsafe fn store_internal_shared_local_base(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    _label: &'static str,
    make_operation: impl FnOnce(&mut ExecuteContext) -> StoreBytes,
) -> VMResult<()> {
    let start = vm_try!(load_start_local_base(tail_code, ctx));
    let operation = make_operation(ctx);
    let bytes = operation.as_slice();
    vm_try!(ctx
        .gc
        .shared_write_bytes(ctx.default_shared_memory_id_unchecked(), start, bytes));
    call_next(tail_code, 3, ctx)
}

#[inline(always)]
#[allow(dead_code)]
unsafe fn store_internal_indexed_shared_local_base(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    _label: &'static str,
    make_operation: impl FnOnce(&mut ExecuteContext) -> StoreBytes,
) -> VMResult<()> {
    let (start, memidx) = vm_try!(load_start_indexed_local_base(tail_code, ctx));
    let operation = make_operation(ctx);
    let bytes = operation.as_slice();
    vm_try!(ctx
        .gc
        .shared_write_bytes(ctx.shared_memory_id_at_unchecked(memidx), start, bytes));
    call_next(tail_code, 4, ctx)
}

#[inline(always)]
#[allow(dead_code)]
unsafe fn store_internal_local_scaled_index(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    _label: &'static str,
    make_operation: impl FnOnce(&mut ExecuteContext) -> StoreBytes,
) -> VMResult<()> {
    let start = vm_try!(load_start_local_scaled_index(tail_code, ctx));
    let operation = make_operation(ctx);
    let bytes = operation.as_slice();
    vm_try!(unsafe { ctx.default_local_memory_mut_unchecked() }.write_bytes(start, bytes));
    call_next(tail_code, 5, ctx)
}

#[inline(always)]
#[allow(dead_code)]
unsafe fn store_internal_shared_local_scaled_index(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    _label: &'static str,
    make_operation: impl FnOnce(&mut ExecuteContext) -> StoreBytes,
) -> VMResult<()> {
    let start = vm_try!(load_start_shared_local_scaled_index(tail_code, ctx));
    let operation = make_operation(ctx);
    let bytes = operation.as_slice();
    vm_try!(ctx
        .gc
        .shared_write_bytes(ctx.default_shared_memory_id_unchecked(), start, bytes));
    call_next(tail_code, 5, ctx)
}

#[inline(always)]
#[allow(dead_code)]
unsafe fn store_internal_indexed_local_scaled_index(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    _label: &'static str,
    make_operation: impl FnOnce(&mut ExecuteContext) -> StoreBytes,
) -> VMResult<()> {
    let (start, memidx) = vm_try!(load_start_indexed_local_scaled_index(tail_code, ctx));
    let operation = make_operation(ctx);
    let bytes = operation.as_slice();
    vm_try!(ctx
        .gc
        .local_write_bytes(ctx.local_memory_id_at_unchecked(memidx), start, bytes));
    call_next(tail_code, 6, ctx)
}

#[inline(always)]
#[allow(dead_code)]
unsafe fn store_internal_indexed_shared_local_scaled_index(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    _label: &'static str,
    make_operation: impl FnOnce(&mut ExecuteContext) -> StoreBytes,
) -> VMResult<()> {
    let (start, memidx) = vm_try!(load_start_indexed_shared_local_scaled_index(tail_code, ctx));
    let operation = make_operation(ctx);
    let bytes = operation.as_slice();
    vm_try!(ctx
        .gc
        .shared_write_bytes(ctx.shared_memory_id_at_unchecked(memidx), start, bytes));
    call_next(tail_code, 6, ctx)
}

#[inline(always)]
#[allow(dead_code)]
unsafe fn load_start_shared_local_base(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<usize> {
    debug_assert!(ctx.snapshot().has_default_memory());
    let local_addr = (*tail_code).operand.local_addr as usize;
    let delta = (*tail_code.add(1)).operand.i32 as u32;
    let memarg = (*tail_code.add(2)).operand.memarg;
    let offset = ctx
        .stack
        .local_u32_from_base(ctx.local_base_ptr as *const u8, local_addr)
        .wrapping_add(delta);
    compute_memory_offset(memarg, offset)
}

#[inline(always)]
#[allow(dead_code)]
unsafe fn load_start_indexed_shared_local_base(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<(usize, u32)> {
    let local_addr = (*tail_code).operand.local_addr as usize;
    let delta = (*tail_code.add(1)).operand.i32 as u32;
    let memarg = (*tail_code.add(2)).operand.memarg;
    let memidx = (*tail_code.add(3)).operand.u32;
    let offset = ctx
        .stack
        .local_u32_from_base(ctx.local_base_ptr as *const u8, local_addr)
        .wrapping_add(delta);
    let start = vm_try!(compute_memory_offset(memarg, offset));
    VMResult::Success((start, memidx))
}

macro_rules! define_indexed_push_load {
    ($local:ident, $shared:ident, $mnemonic:literal, $bytes:expr) => {
        #[doc = concat!("WebAssembly `", $mnemonic, "` on indexed local memory.")]
        ///
        /// Spec:
        /// - Syntax: https://webassembly.github.io/multi-memory/core/syntax/instructions.html
        /// - Validation: https://webassembly.github.io/multi-memory/core/valid/instructions.html
        /// - Execution: https://webassembly.github.io/multi-memory/core/exec/instructions.html
        ///
        /// Stack effect: `[i32] -> [value]`.
        /// Traps: traps on out-of-bounds memory access.
        /// Notes: Uses the typed indexed local-memory fast path and tail-dispatches with `call_next`.
        ///
        /// # Safety
        /// - `tail_code` must point to the decoded instruction for this handler in the active function body.
        /// - `ctx` must reference a live execution context whose operand stack satisfies this instruction.
        /// - The memory index operand must be in-bounds and refer to a local memory.
        pub unsafe fn $local(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            let (start, memidx) = vm_try!(load_start_indexed(tail_code, ctx));
            vm_try!(ctx.gc.local_push_memory_to_stack::<$bytes>(
                ctx.local_memory_id_at_unchecked(memidx),
                ctx.stack,
                start,
            ));
            call_next(tail_code, 2, ctx)
        }

        #[doc = concat!("WebAssembly `", $mnemonic, "` on indexed shared memory.")]
        ///
        /// Spec:
        /// - Syntax: https://webassembly.github.io/multi-memory/core/syntax/instructions.html
        /// - Validation: https://webassembly.github.io/multi-memory/core/valid/instructions.html
        /// - Execution: https://webassembly.github.io/multi-memory/core/exec/instructions.html
        ///
        /// Stack effect: `[i32] -> [value]`.
        /// Traps: traps on out-of-bounds memory access.
        /// Notes: Uses the typed indexed shared-memory fast path and tail-dispatches with `call_next`.
        ///
        /// # Safety
        /// - `tail_code` must point to the decoded instruction for this handler in the active function body.
        /// - `ctx` must reference a live execution context whose operand stack satisfies this instruction.
        /// - The memory index operand must be in-bounds and refer to a shared memory.
        pub unsafe fn $shared(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            let (start, memidx) = vm_try!(load_start_indexed(tail_code, ctx));
            vm_try!(ctx.gc.shared_push_memory_to_stack::<$bytes>(
                ctx.shared_memory_id_at_unchecked(memidx),
                ctx.stack,
                start,
            ));
            call_next(tail_code, 2, ctx)
        }
    };
}

macro_rules! define_indexed_scalar_load {
    ($local:ident, $shared:ident, $mnemonic:literal, $local_reader:ident, $shared_reader:ident, $push:ident, $convert:path) => {
        #[doc = concat!("WebAssembly `", $mnemonic, "` on indexed local memory.")]
        ///
        /// Spec:
        /// - Syntax: https://webassembly.github.io/multi-memory/core/syntax/instructions.html
        /// - Validation: https://webassembly.github.io/multi-memory/core/valid/instructions.html
        /// - Execution: https://webassembly.github.io/multi-memory/core/exec/instructions.html
        ///
        /// Stack effect: `[i32] -> [value]`.
        /// Traps: traps on out-of-bounds memory access.
        /// Notes: Uses the typed indexed local-memory fast path and tail-dispatches with `call_next`.
        ///
        /// # Safety
        /// - `tail_code` must point to the decoded instruction for this handler in the active function body.
        /// - `ctx` must reference a live execution context whose operand stack satisfies this instruction.
        /// - The memory index operand must be in-bounds and refer to a local memory.
        pub unsafe fn $local(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            let (start, memidx) = vm_try!(load_start_indexed(tail_code, ctx));
            let value = vm_try!(ctx
                .gc
                .$local_reader(ctx.local_memory_id_at_unchecked(memidx), start));
            vm_try!(ctx.stack.$push($convert(value)));
            call_next(tail_code, 2, ctx)
        }

        #[doc = concat!("WebAssembly `", $mnemonic, "` on indexed shared memory.")]
        ///
        /// Spec:
        /// - Syntax: https://webassembly.github.io/multi-memory/core/syntax/instructions.html
        /// - Validation: https://webassembly.github.io/multi-memory/core/valid/instructions.html
        /// - Execution: https://webassembly.github.io/multi-memory/core/exec/instructions.html
        ///
        /// Stack effect: `[i32] -> [value]`.
        /// Traps: traps on out-of-bounds memory access.
        /// Notes: Uses the typed indexed shared-memory fast path and tail-dispatches with `call_next`.
        ///
        /// # Safety
        /// - `tail_code` must point to the decoded instruction for this handler in the active function body.
        /// - `ctx` must reference a live execution context whose operand stack satisfies this instruction.
        /// - The memory index operand must be in-bounds and refer to a shared memory.
        pub unsafe fn $shared(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            let (start, memidx) = vm_try!(load_start_indexed(tail_code, ctx));
            let value = vm_try!(ctx
                .gc
                .$shared_reader(ctx.shared_memory_id_at_unchecked(memidx), start));
            vm_try!(ctx.stack.$push($convert(value)));
            call_next(tail_code, 2, ctx)
        }
    };
}

macro_rules! define_indexed_store_alias {
    ($local:ident, $shared:ident, $mnemonic:literal, $make_operation:expr) => {
        #[doc = concat!("WebAssembly `", $mnemonic, "` on indexed local memory.")]
        ///
        /// Spec:
        /// - Syntax: https://webassembly.github.io/multi-memory/core/syntax/instructions.html
        /// - Validation: https://webassembly.github.io/multi-memory/core/valid/instructions.html
        /// - Execution: https://webassembly.github.io/multi-memory/core/exec/instructions.html
        ///
        /// Stack effect: `[i32, value] -> []`.
        /// Traps: traps on out-of-bounds memory access.
        /// Notes: Uses the typed indexed local-memory fast path and tail-dispatches with `call_next`.
        ///
        /// # Safety
        /// - `tail_code` must point to the decoded instruction for this handler in the active function body.
        /// - `ctx` must reference a live execution context whose operand stack satisfies this instruction.
        /// - The memory index operand must be in-bounds and refer to a local memory.
        pub unsafe fn $local(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            store_internal_local_indexed(tail_code, ctx, stringify!($local), $make_operation)
        }

        #[doc = concat!("WebAssembly `", $mnemonic, "` on indexed shared memory.")]
        ///
        /// Spec:
        /// - Syntax: https://webassembly.github.io/multi-memory/core/syntax/instructions.html
        /// - Validation: https://webassembly.github.io/multi-memory/core/valid/instructions.html
        /// - Execution: https://webassembly.github.io/multi-memory/core/exec/instructions.html
        ///
        /// Stack effect: `[i32, value] -> []`.
        /// Traps: traps on out-of-bounds memory access.
        /// Notes: Uses the typed indexed shared-memory fast path and tail-dispatches with `call_next`.
        ///
        /// # Safety
        /// - `tail_code` must point to the decoded instruction for this handler in the active function body.
        /// - `ctx` must reference a live execution context whose operand stack satisfies this instruction.
        /// - The memory index operand must be in-bounds and refer to a shared memory.
        pub unsafe fn $shared(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            store_internal_shared_indexed(tail_code, ctx, stringify!($shared), $make_operation)
        }
    };
}

macro_rules! define_local_base_push_load {
    ($name:ident, $mnemonic:literal, $bytes:expr) => {
        #[doc = concat!("WebAssembly `", $mnemonic, "` with local-base address on default local memory.")]
        #[allow(dead_code)]
        pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            profile_memory_family(stringify!($name));
            let start = vm_try!(load_start_local_base(tail_code, ctx));
            vm_try!(default_local_push_to_stack::<$bytes>(ctx, start));
            call_next(tail_code, 3, ctx)
        }
    };
}

macro_rules! define_indexed_local_base_push_load {
    ($name:ident, $mnemonic:literal, $bytes:expr) => {
        #[doc = concat!("WebAssembly `", $mnemonic, "` with local-base address on indexed local memory.")]
        #[allow(dead_code)]
        pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            profile_memory_family(stringify!($name));
            let (start, memidx) = vm_try!(load_start_indexed_local_base(tail_code, ctx));
            vm_try!(ctx.gc.local_push_memory_to_stack::<$bytes>(
                ctx.local_memory_id_at_unchecked(memidx),
                ctx.stack,
                start,
            ));
            call_next(tail_code, 4, ctx)
        }
    };
}

macro_rules! define_local_base_scalar_load {
    ($name:ident, $mnemonic:literal, $reader:ident, $push:ident, $convert:path) => {
        #[doc = concat!("WebAssembly `", $mnemonic, "` with local-base address on default local memory.")]
        #[allow(dead_code)]
        pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            profile_memory_family(stringify!($name));
            let start = vm_try!(load_start_local_base(tail_code, ctx));
            let value = vm_try!(unsafe { ctx.default_local_memory_unchecked() }.$reader(start));
            vm_try!(ctx.stack.$push($convert(value)));
            call_next(tail_code, 3, ctx)
        }
    };
}

macro_rules! define_indexed_local_base_scalar_load {
    ($name:ident, $mnemonic:literal, $reader:ident, $push:ident, $convert:path) => {
        #[doc = concat!("WebAssembly `", $mnemonic, "` with local-base address on indexed local memory.")]
        #[allow(dead_code)]
        pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            profile_memory_family(stringify!($name));
            let (start, memidx) = vm_try!(load_start_indexed_local_base(tail_code, ctx));
            let value = vm_try!(ctx.gc.$reader(ctx.local_memory_id_at_unchecked(memidx), start));
            vm_try!(ctx.stack.$push($convert(value)));
            call_next(tail_code, 4, ctx)
        }
    };
}

macro_rules! define_local_base_store_alias {
    ($name:ident, $make_operation:expr) => {
        #[allow(dead_code)]
        pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            profile_memory_family(stringify!($name));
            store_internal_local_base(tail_code, ctx, stringify!($name), $make_operation)
        }
    };
}

macro_rules! define_indexed_local_base_store_alias {
    ($name:ident, $make_operation:expr) => {
        #[allow(dead_code)]
        pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            profile_memory_family(stringify!($name));
            store_internal_indexed_local_base(tail_code, ctx, stringify!($name), $make_operation)
        }
    };
}

macro_rules! define_local_get4_local_base_scalar_load {
    ($name:ident, $mnemonic:literal, $reader:ident, $push:ident, $convert:path) => {
        #[doc = concat!("WebAssembly `local.get; ", $mnemonic, "` with local-base address on default local memory.")]
        #[allow(dead_code)]
        pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            profile_memory_family(stringify!($name));
            let preserved_local = (*tail_code).operand.local_addr as usize;
            let base_local = (*tail_code.add(1)).operand.local_addr as usize;
            let delta = (*tail_code.add(2)).operand.i32 as u32;
            let memarg = (*tail_code.add(3)).operand.memarg;
            let preserved = ctx
                .stack
                .local_u32_from_base(ctx.local_base_ptr as *const u8, preserved_local);
            vm_try!(ctx.stack.push_u32_fast(preserved));
            let offset = ctx
                .stack
                .local_u32_from_base(ctx.local_base_ptr as *const u8, base_local)
                .wrapping_add(delta);
            let start = vm_try!(compute_memory_offset(memarg, offset));
            let value = vm_try!(unsafe { ctx.default_local_memory_unchecked() }.$reader(start));
            vm_try!(ctx.stack.$push($convert(value)));
            call_next(tail_code, 4, ctx)
        }
    };
}

macro_rules! define_local_base_i32_load_set4 {
    ($set_name:ident, $tee_name:ident, $mnemonic:literal, $reader:ident, $convert:path) => {
        #[doc = concat!("WebAssembly `", $mnemonic, "; local.set` with local-base address on default local memory.")]
        #[allow(dead_code)]
        pub unsafe fn $set_name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            profile_memory_family(stringify!($set_name));
            let start = vm_try!(load_start_local_base(tail_code, ctx));
            let dst = (*tail_code.add(3)).operand.local_addr as usize;
            let value = $convert(vm_try!(unsafe { ctx.default_local_memory_unchecked() }.$reader(start))) as u32;
            ctx.stack
                .local_set4_from_base_value(ctx.local_base_ptr, dst, value);
            call_next(tail_code, 4, ctx)
        }

        #[doc = concat!("WebAssembly `", $mnemonic, "; local.tee` with local-base address on default local memory.")]
        #[allow(dead_code)]
        pub unsafe fn $tee_name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            profile_memory_family(stringify!($tee_name));
            let start = vm_try!(load_start_local_base(tail_code, ctx));
            let dst = (*tail_code.add(3)).operand.local_addr as usize;
            let value = $convert(vm_try!(unsafe { ctx.default_local_memory_unchecked() }.$reader(start))) as u32;
            vm_try!(ctx.stack.push_u32_fast(value));
            ctx.stack
                .local_set4_from_base_value(ctx.local_base_ptr, dst, value);
            call_next(tail_code, 4, ctx)
        }
    };
}

macro_rules! define_local_base_i32_load_local_get4 {
    ($root_name:ident, $tee_name:ident, $mnemonic:literal, $reader:ident, $convert:path, $push:ident) => {
        #[doc = concat!("WebAssembly `", $mnemonic, "; local.get` with local-base address on default local memory.")]
        #[allow(dead_code)]
        pub unsafe fn $root_name(
            tail_code: *const Instr,
            ctx: &mut ExecuteContext,
        ) -> VMResult<()> {
            profile_memory_family(stringify!($root_name));
            let start = vm_try!(load_start_local_base(tail_code, ctx));
            let preserved = (*tail_code.add(3)).operand.local_addr as usize;
            let value = $convert(vm_try!(unsafe { ctx.default_local_memory_unchecked() }.$reader(start)));
            vm_try!(ctx.stack.$push(value));
            let preserved =
                ctx.stack
                    .local_u32_from_base(ctx.local_base_ptr as *const u8, preserved);
            vm_try!(ctx.stack.push_u32_fast(preserved));
            call_next(tail_code, 4, ctx)
        }

        #[doc = concat!("WebAssembly `", $mnemonic, "; local.tee; local.get` with local-base address on default local memory.")]
        #[allow(dead_code)]
        pub unsafe fn $tee_name(
            tail_code: *const Instr,
            ctx: &mut ExecuteContext,
        ) -> VMResult<()> {
            profile_memory_family(stringify!($tee_name));
            let start = vm_try!(load_start_local_base(tail_code, ctx));
            let dst = (*tail_code.add(3)).operand.local_addr as usize;
            let preserved = (*tail_code.add(4)).operand.local_addr as usize;
            let value = $convert(vm_try!(unsafe { ctx.default_local_memory_unchecked() }.$reader(start)));
            vm_try!(ctx.stack.$push(value));
            ctx.stack
                .local_set4_from_base_value(ctx.local_base_ptr, dst, value as u32);
            let preserved =
                ctx.stack
                    .local_u32_from_base(ctx.local_base_ptr as *const u8, preserved);
            vm_try!(ctx.stack.push_u32_fast(preserved));
            call_next(tail_code, 5, ctx)
        }
    };
}

macro_rules! define_i32_load_local_base_set4_i32_load_local_base_local_get4 {
    ($name:ident, $mnemonic:literal, $reader:ident, $convert:path, $push:ident) => {
        #[doc = concat!("WebAssembly `i32.load; local.set; ", $mnemonic, "; local.get` with local-base addresses on default local memory.")]
        #[allow(dead_code)]
        pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            profile_memory_family(stringify!($name));
            let first_base_local = (*tail_code).operand.local_addr as usize;
            let first_delta = (*tail_code.add(1)).operand.i32 as u32;
            let first_memarg = (*tail_code.add(2)).operand.memarg;
            let dst = (*tail_code.add(3)).operand.local_addr as usize;
            let second_delta = (*tail_code.add(4)).operand.i32 as u32;
            let second_memarg = (*tail_code.add(5)).operand.memarg;
            let preserved = (*tail_code.add(6)).operand.local_addr as usize;
            let local_base = ctx.local_base_ptr as *const u8;
            let first_offset = ctx
                .stack
                .local_u32_from_base(local_base, first_base_local)
                .wrapping_add(first_delta);
            let first_start = vm_try!(compute_memory_offset(first_memarg, first_offset));
            let addr =
                vm_try!(unsafe { ctx.default_local_memory_unchecked() }.read_u32_at(first_start));
            ctx.stack
                .local_set4_from_base_value(ctx.local_base_ptr, dst, addr);
            let second_offset = addr.wrapping_add(second_delta);
            let second_start = vm_try!(compute_memory_offset(second_memarg, second_offset));
            let value = $convert(vm_try!(
                unsafe { ctx.default_local_memory_unchecked() }.$reader(second_start)
            ));
            vm_try!(ctx.stack.$push(value));
            let preserved = ctx.stack.local_u32_from_base(local_base, preserved);
            vm_try!(ctx.stack.push_u32_fast(preserved));
            call_next(tail_code, 7, ctx)
        }
    };
}

macro_rules! define_i32_load_local_base_set4_i32_load_local_base_local_eq_br_if {
    ($name:ident, $mnemonic:literal, $reader:ident, $convert:path) => {
        #[doc = concat!("WebAssembly `i32.load; local.set; ", $mnemonic, "; local.get; i32.eq; br_if` with local-base addresses on default local memory.")]
        #[allow(dead_code)]
        pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            profile_memory_family(stringify!($name));
            let first_base_local = (*tail_code).operand.local_addr as usize;
            let first_delta = (*tail_code.add(1)).operand.i32 as u32;
            let first_memarg = (*tail_code.add(2)).operand.memarg;
            let dst = (*tail_code.add(3)).operand.local_addr as usize;
            let second_delta = (*tail_code.add(4)).operand.i32 as u32;
            let second_memarg = (*tail_code.add(5)).operand.memarg;
            let rhs_local = (*tail_code.add(6)).operand.local_addr as usize;
            let local_base = ctx.local_base_ptr as *const u8;
            let first_offset = ctx
                .stack
                .local_u32_from_base(local_base, first_base_local)
                .wrapping_add(first_delta);
            let first_start = vm_try!(compute_memory_offset(first_memarg, first_offset));
            let addr =
                vm_try!(unsafe { ctx.default_local_memory_unchecked() }.read_u32_at(first_start));
            ctx.stack
                .local_set4_from_base_value(ctx.local_base_ptr, dst, addr);
            let second_offset = addr.wrapping_add(second_delta);
            let second_start = vm_try!(compute_memory_offset(second_memarg, second_offset));
            let value = $convert(vm_try!(
                unsafe { ctx.default_local_memory_unchecked() }.$reader(second_start)
            )) as u32;
            let rhs = ctx.stack.local_u32_from_base(local_base, rhs_local);
            let ptr = memory_br_if_ptr(tail_code, 7, 8, (value == rhs) as u32, ctx);
            call_next(ptr, 0, ctx)
        }
    };
}

#[allow(dead_code)]
/// WebAssembly `i32.load; local.set; i32.load8_u; local.get; i32.and; compare; br_if` local-base fusion.
///
/// # Safety
/// - `tail_code` must point to operands decoded for this masked compare branch handler.
/// - `ctx` must hold a valid frame, local base, and default memory for the active module.
pub unsafe fn op_i32_load_local_base_set4_i32_load8_u_local_base_local_masked_compare_br_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    profile_memory_family(
        "op_i32_load_local_base_set4_i32_load8_u_local_base_local_masked_compare_br_if",
    );
    let first_base_local = (*tail_code).operand.local_addr as usize;
    let first_delta = (*tail_code.add(1)).operand.i32 as u32;
    let first_memarg = (*tail_code.add(2)).operand.memarg;
    let dst = (*tail_code.add(3)).operand.local_addr as usize;
    let second_delta = (*tail_code.add(4)).operand.i32 as u32;
    let second_memarg = (*tail_code.add(5)).operand.memarg;
    let rhs_local = (*tail_code.add(6)).operand.local_addr as usize;
    let compare_kind = (*tail_code.add(7)).operand.u32;
    let local_base = ctx.local_base_ptr as *const u8;
    let first_offset = ctx
        .stack
        .local_u32_from_base(local_base, first_base_local)
        .wrapping_add(first_delta);
    let first_start = vm_try!(compute_memory_offset(first_memarg, first_offset));
    let addr = vm_try!(unsafe { ctx.default_local_memory_unchecked() }.read_u32_at(first_start));
    ctx.stack
        .local_set4_from_base_value(ctx.local_base_ptr, dst, addr);
    let second_offset = addr.wrapping_add(second_delta);
    let second_start = vm_try!(compute_memory_offset(second_memarg, second_offset));
    let value = u32::from(vm_try!(
        unsafe { ctx.default_local_memory_unchecked() }.read_u8_at(second_start)
    ));
    let rhs = ctx.stack.local_u32_from_base(local_base, rhs_local) & 0xff;
    let cond = match compare_kind {
        0 => value == rhs,
        1 => value != rhs,
        _ => false,
    };
    let ptr = memory_br_if_ptr(tail_code, 8, 9, u32::from(cond), ctx);
    call_next(ptr, 0, ctx)
}

#[allow(dead_code)]
/// WebAssembly `i32.load; local.set; i32.load16_u; local.get; i32.eq; br_if` search-loop fusion.
///
/// # Safety
/// - `tail_code` must point to operands decoded for this bounded search-loop handler.
/// - `ctx` must hold a valid frame, local base, and default memory for the active module.
pub unsafe fn op_i32_load_local_base_set4_i32_load16_u_local_base_local_eq_search_loop(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    profile_memory_family(
        "op_i32_load_local_base_set4_i32_load16_u_local_base_local_eq_search_loop",
    );
    let node_local = (*tail_code).operand.local_addr as usize;
    let data_delta = (*tail_code.add(1)).operand.i32 as u32;
    let data_memarg = (*tail_code.add(2)).operand.memarg;
    let data_local = (*tail_code.add(3)).operand.local_addr as usize;
    let field_delta = (*tail_code.add(4)).operand.i32 as u32;
    let field_memarg = (*tail_code.add(5)).operand.memarg;
    let rhs_local = (*tail_code.add(6)).operand.local_addr as usize;
    let next_delta = (*tail_code.add(7)).operand.i32 as u32;
    let next_memarg = (*tail_code.add(8)).operand.memarg;
    let match_addr = (*tail_code.add(9)).operand.jump_addr;
    let miss_addr = (*tail_code.add(10)).operand.jump_addr;
    let local_base = ctx.local_base_ptr as *const u8;
    let rhs = ctx.stack.local_u32_from_base(local_base, rhs_local) & 0xffff;
    let mut node = ctx.stack.local_u32_from_base(local_base, node_local);

    loop {
        let data_offset = node.wrapping_add(data_delta);
        let data_start = vm_try!(compute_memory_offset(data_memarg, data_offset));
        let data = vm_try!(unsafe { ctx.default_local_memory_unchecked() }.read_u32_at(data_start));
        ctx.stack
            .local_set4_from_base_value(ctx.local_base_ptr, data_local, data);

        let field_offset = data.wrapping_add(field_delta);
        let field_start = vm_try!(compute_memory_offset(field_memarg, field_offset));
        let value = u32::from(vm_try!(
            unsafe { ctx.default_local_memory_unchecked() }.read_u16_at(field_start)
        ));
        if value == rhs {
            let ptr = ctx.code().offset(match_addr as isize);
            return call_next(ptr, 0, ctx);
        }

        let next_offset = node.wrapping_add(next_delta);
        let next_start = vm_try!(compute_memory_offset(next_memarg, next_offset));
        let next = vm_try!(unsafe { ctx.default_local_memory_unchecked() }.read_u32_at(next_start));
        ctx.stack
            .local_set4_from_base_value(ctx.local_base_ptr, node_local, next);
        if next == 0 {
            let ptr = ctx.code().offset(miss_addr as isize);
            return call_next(ptr, 0, ctx);
        }
        node = next;
    }
}

#[allow(dead_code)]
/// WebAssembly `i32.load; local.set; i32.load16_u; local.get; i32.eq; br_if` search-loop fallthrough fusion.
///
/// # Safety
/// - `tail_code` must point to operands decoded for this bounded search-loop handler.
/// - `ctx` must hold a valid frame, local base, and default memory for the active module.
pub unsafe fn op_i32_load_local_base_set4_i32_load16_u_local_base_local_eq_search_loop_fallthrough(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    profile_memory_family(
        "op_i32_load_local_base_set4_i32_load16_u_local_base_local_eq_search_loop_fallthrough",
    );
    let node_local = (*tail_code).operand.local_addr as usize;
    let data_delta = (*tail_code.add(1)).operand.i32 as u32;
    let data_memarg = (*tail_code.add(2)).operand.memarg;
    let data_local = (*tail_code.add(3)).operand.local_addr as usize;
    let field_delta = (*tail_code.add(4)).operand.i32 as u32;
    let field_memarg = (*tail_code.add(5)).operand.memarg;
    let rhs_local = (*tail_code.add(6)).operand.local_addr as usize;
    let next_delta = (*tail_code.add(7)).operand.i32 as u32;
    let next_memarg = (*tail_code.add(8)).operand.memarg;
    let match_addr = (*tail_code.add(9)).operand.jump_addr;
    let local_base = ctx.local_base_ptr as *const u8;
    let rhs = ctx.stack.local_u32_from_base(local_base, rhs_local) & 0xffff;
    let mut node = ctx.stack.local_u32_from_base(local_base, node_local);

    loop {
        let data_offset = node.wrapping_add(data_delta);
        let data_start = vm_try!(compute_memory_offset(data_memarg, data_offset));
        let data = vm_try!(unsafe { ctx.default_local_memory_unchecked() }.read_u32_at(data_start));
        ctx.stack
            .local_set4_from_base_value(ctx.local_base_ptr, data_local, data);

        let field_offset = data.wrapping_add(field_delta);
        let field_start = vm_try!(compute_memory_offset(field_memarg, field_offset));
        let value = u32::from(vm_try!(
            unsafe { ctx.default_local_memory_unchecked() }.read_u16_at(field_start)
        ));
        if value == rhs {
            let ptr = ctx.code().offset(match_addr as isize);
            return call_next(ptr, 0, ctx);
        }

        let next_offset = node.wrapping_add(next_delta);
        let next_start = vm_try!(compute_memory_offset(next_memarg, next_offset));
        let next = vm_try!(unsafe { ctx.default_local_memory_unchecked() }.read_u32_at(next_start));
        ctx.stack
            .local_set4_from_base_value(ctx.local_base_ptr, node_local, next);
        if next == 0 {
            return call_next(tail_code, 16, ctx);
        }
        node = next;
    }
}

#[allow(dead_code)]
/// WebAssembly `i32.load; local.set; i32.load8_u; local.get; i32.and; compare; br_if` search-loop fusion.
///
/// # Safety
/// - `tail_code` must point to operands decoded for this masked search-loop handler.
/// - `ctx` must hold a valid frame, local base, and default memory for the active module.
pub unsafe fn op_i32_load_local_base_set4_i32_load8_u_local_base_local_masked_search_loop(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    profile_memory_family(
        "op_i32_load_local_base_set4_i32_load8_u_local_base_local_masked_search_loop",
    );
    let node_local = (*tail_code).operand.local_addr as usize;
    let data_delta = (*tail_code.add(1)).operand.i32 as u32;
    let data_memarg = (*tail_code.add(2)).operand.memarg;
    let data_local = (*tail_code.add(3)).operand.local_addr as usize;
    let byte_delta = (*tail_code.add(4)).operand.i32 as u32;
    let byte_memarg = (*tail_code.add(5)).operand.memarg;
    let rhs_local = (*tail_code.add(6)).operand.local_addr as usize;
    let compare_kind = (*tail_code.add(7)).operand.u32;
    let next_delta = (*tail_code.add(8)).operand.i32 as u32;
    let next_memarg = (*tail_code.add(9)).operand.memarg;
    let match_addr = (*tail_code.add(10)).operand.jump_addr;
    let miss_addr = (*tail_code.add(11)).operand.jump_addr;
    let local_base = ctx.local_base_ptr as *const u8;
    let rhs = ctx.stack.local_u32_from_base(local_base, rhs_local) & 0xff;
    let mut node = ctx.stack.local_u32_from_base(local_base, node_local);

    loop {
        let data_offset = node.wrapping_add(data_delta);
        let data_start = vm_try!(compute_memory_offset(data_memarg, data_offset));
        let data = vm_try!(unsafe { ctx.default_local_memory_unchecked() }.read_u32_at(data_start));
        ctx.stack
            .local_set4_from_base_value(ctx.local_base_ptr, data_local, data);

        let byte_offset = data.wrapping_add(byte_delta);
        let byte_start = vm_try!(compute_memory_offset(byte_memarg, byte_offset));
        let value = u32::from(vm_try!(
            unsafe { ctx.default_local_memory_unchecked() }.read_u8_at(byte_start)
        ));
        let matched = match compare_kind {
            0 => value == rhs,
            1 => value != rhs,
            _ => false,
        };
        if matched {
            let ptr = ctx.code().offset(match_addr as isize);
            return call_next(ptr, 0, ctx);
        }

        let next_offset = node.wrapping_add(next_delta);
        let next_start = vm_try!(compute_memory_offset(next_memarg, next_offset));
        let next = vm_try!(unsafe { ctx.default_local_memory_unchecked() }.read_u32_at(next_start));
        ctx.stack
            .local_set4_from_base_value(ctx.local_base_ptr, node_local, next);
        if next == 0 {
            let ptr = ctx.code().offset(miss_addr as isize);
            return call_next(ptr, 0, ctx);
        }
        node = next;
    }
}

#[allow(dead_code)]
/// WebAssembly `i32.load; local.set; i32.load8_u; local.get; i32.and; compare; br_if` search-loop fallthrough fusion.
///
/// # Safety
/// - `tail_code` must point to operands decoded for this masked search-loop handler.
/// - `ctx` must hold a valid frame, local base, and default memory for the active module.
pub unsafe fn op_i32_load_local_base_set4_i32_load8_u_local_base_local_masked_search_loop_fallthrough(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    profile_memory_family(
        "op_i32_load_local_base_set4_i32_load8_u_local_base_local_masked_search_loop_fallthrough",
    );
    let node_local = (*tail_code).operand.local_addr as usize;
    let data_delta = (*tail_code.add(1)).operand.i32 as u32;
    let data_memarg = (*tail_code.add(2)).operand.memarg;
    let data_local = (*tail_code.add(3)).operand.local_addr as usize;
    let byte_delta = (*tail_code.add(4)).operand.i32 as u32;
    let byte_memarg = (*tail_code.add(5)).operand.memarg;
    let rhs_local = (*tail_code.add(6)).operand.local_addr as usize;
    let compare_kind = (*tail_code.add(7)).operand.u32;
    let next_delta = (*tail_code.add(8)).operand.i32 as u32;
    let next_memarg = (*tail_code.add(9)).operand.memarg;
    let match_addr = (*tail_code.add(10)).operand.jump_addr;
    let local_base = ctx.local_base_ptr as *const u8;
    let rhs = ctx.stack.local_u32_from_base(local_base, rhs_local) & 0xff;
    let mut node = ctx.stack.local_u32_from_base(local_base, node_local);

    loop {
        let data_offset = node.wrapping_add(data_delta);
        let data_start = vm_try!(compute_memory_offset(data_memarg, data_offset));
        let data = vm_try!(unsafe { ctx.default_local_memory_unchecked() }.read_u32_at(data_start));
        ctx.stack
            .local_set4_from_base_value(ctx.local_base_ptr, data_local, data);

        let byte_offset = data.wrapping_add(byte_delta);
        let byte_start = vm_try!(compute_memory_offset(byte_memarg, byte_offset));
        let value = u32::from(vm_try!(
            unsafe { ctx.default_local_memory_unchecked() }.read_u8_at(byte_start)
        ));
        let matched = match compare_kind {
            0 => value == rhs,
            1 => value != rhs,
            _ => false,
        };
        if matched {
            let ptr = ctx.code().offset(match_addr as isize);
            return call_next(ptr, 0, ctx);
        }

        let next_offset = node.wrapping_add(next_delta);
        let next_start = vm_try!(compute_memory_offset(next_memarg, next_offset));
        let next = vm_try!(unsafe { ctx.default_local_memory_unchecked() }.read_u32_at(next_start));
        ctx.stack
            .local_set4_from_base_value(ctx.local_base_ptr, node_local, next);
        if next == 0 {
            return call_next(tail_code, 17, ctx);
        }
        node = next;
    }
}

macro_rules! define_i32_load_local_base_set4_i32_load_local_base {
    ($name:ident, $mnemonic:literal, $reader:ident, $convert:path, $push:ident) => {
        #[doc = concat!("WebAssembly `i32.load; local.set; ", $mnemonic, "` with local-base addresses on default local memory.")]
        #[allow(dead_code)]
        pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            profile_memory_family(stringify!($name));
            let first_base_local = (*tail_code).operand.local_addr as usize;
            let first_delta = (*tail_code.add(1)).operand.i32 as u32;
            let first_memarg = (*tail_code.add(2)).operand.memarg;
            let dst = (*tail_code.add(3)).operand.local_addr as usize;
            let second_delta = (*tail_code.add(4)).operand.i32 as u32;
            let second_memarg = (*tail_code.add(5)).operand.memarg;
            let local_base = ctx.local_base_ptr as *const u8;
            let first_offset = ctx
                .stack
                .local_u32_from_base(local_base, first_base_local)
                .wrapping_add(first_delta);
            let first_start = vm_try!(compute_memory_offset(first_memarg, first_offset));
            let addr =
                vm_try!(unsafe { ctx.default_local_memory_unchecked() }.read_u32_at(first_start));
            ctx.stack
                .local_set4_from_base_value(ctx.local_base_ptr, dst, addr);
            let second_offset = addr.wrapping_add(second_delta);
            let second_start = vm_try!(compute_memory_offset(second_memarg, second_offset));
            let value = $convert(vm_try!(
                unsafe { ctx.default_local_memory_unchecked() }.$reader(second_start)
            ));
            vm_try!(ctx.stack.$push(value));
            call_next(tail_code, 6, ctx)
        }
    };
}

macro_rules! define_local_base_i32_load_local_get4_i32_load {
    (
        $name:ident,
        $first_mnemonic:literal,
        $first_reader:ident,
        $first_convert:path,
        $first_push:ident,
        $second_mnemonic:literal,
        $second_reader:ident,
        $second_convert:path,
        $second_push:ident
    ) => {
        #[doc = concat!("WebAssembly `", $first_mnemonic, "; local.get; ", $second_mnemonic, "` on default local memory.")]
        #[allow(dead_code)]
        pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            profile_memory_family(stringify!($name));
            let first_base_local = (*tail_code).operand.local_addr as usize;
            let first_delta = (*tail_code.add(1)).operand.i32 as u32;
            let first_memarg = (*tail_code.add(2)).operand.memarg;
            let second_addr_local = (*tail_code.add(3)).operand.local_addr as usize;
            let second_memarg = (*tail_code.add(4)).operand.memarg;
            let local_base = ctx.local_base_ptr as *const u8;
            let first_offset = ctx
                .stack
                .local_u32_from_base(local_base, first_base_local)
                .wrapping_add(first_delta);
            let first_start = vm_try!(compute_memory_offset(first_memarg, first_offset));
            let first = $first_convert(vm_try!(
                unsafe { ctx.default_local_memory_unchecked() }.$first_reader(first_start)
            ));
            vm_try!(ctx.stack.$first_push(first));
            let second_offset = ctx.stack.local_u32_from_base(local_base, second_addr_local);
            let second_start = vm_try!(compute_memory_offset(second_memarg, second_offset));
            let second = $second_convert(vm_try!(
                unsafe { ctx.default_local_memory_unchecked() }.$second_reader(second_start)
            ));
            vm_try!(ctx.stack.$second_push(second));
            call_next(tail_code, 5, ctx)
        }
    };
}

macro_rules! define_local_base_i32_load_local_get4_i32_load_local_get4 {
    (
        $name:ident,
        $first_mnemonic:literal,
        $first_reader:ident,
        $first_convert:path,
        $first_push:ident,
        $second_mnemonic:literal,
        $second_reader:ident,
        $second_convert:path,
        $second_push:ident
    ) => {
        #[doc = concat!("WebAssembly `", $first_mnemonic, "; local.get; ", $second_mnemonic, "; local.get` on default local memory.")]
        ///
        /// # Safety
        /// - `tail_code` must point to the fused local-base load/local/load/local operand group.
        /// - `ctx` must reference a live frame with validated local and memory operands.
        /// - This handler must not keep borrows across `call_next`.
        #[allow(dead_code)]
        pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            profile_memory_family(stringify!($name));
            let first_base_local = (*tail_code).operand.local_addr as usize;
            let first_delta = (*tail_code.add(1)).operand.i32 as u32;
            let first_memarg = (*tail_code.add(2)).operand.memarg;
            let second_addr_local = (*tail_code.add(3)).operand.local_addr as usize;
            let second_memarg = (*tail_code.add(4)).operand.memarg;
            let preserved_local = (*tail_code.add(5)).operand.local_addr as usize;
            let local_base = ctx.local_base_ptr as *const u8;
            let first_offset = ctx
                .stack
                .local_u32_from_base(local_base, first_base_local)
                .wrapping_add(first_delta);
            let first_start = vm_try!(compute_memory_offset(first_memarg, first_offset));
            let first = $first_convert(vm_try!(
                unsafe { ctx.default_local_memory_unchecked() }.$first_reader(first_start)
            ));
            vm_try!(ctx.stack.$first_push(first));
            let second_offset = ctx.stack.local_u32_from_base(local_base, second_addr_local);
            let second_start = vm_try!(compute_memory_offset(second_memarg, second_offset));
            let second = $second_convert(vm_try!(
                unsafe { ctx.default_local_memory_unchecked() }.$second_reader(second_start)
            ));
            vm_try!(ctx.stack.$second_push(second));
            let preserved = ctx.stack.local_u32_from_base(local_base, preserved_local);
            vm_try!(ctx.stack.push_u32_fast(preserved));
            let skip_slots = (*tail_code.add(6)).operand.u32 as isize;
            call_next(tail_code, skip_slots, ctx)
        }
    };
}

macro_rules! define_local_get4_local_base_i32_load_local_get4_i32_load {
    (
        $name:ident,
        $first_mnemonic:literal,
        $first_reader:ident,
        $first_convert:path,
        $first_push:ident,
        $second_mnemonic:literal,
        $second_reader:ident,
        $second_convert:path,
        $second_push:ident
    ) => {
        #[doc = concat!("WebAssembly `local.get; ", $first_mnemonic, "; local.get; ", $second_mnemonic, "` on default local memory.")]
        ///
        /// # Safety
        /// - `tail_code` must point to the fused local/get/local-base load/local/load operand group.
        /// - `ctx` must reference a live frame with validated local and memory operands.
        /// - This handler must not keep borrows across `call_next`.
        #[allow(dead_code)]
        pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            profile_memory_family(stringify!($name));
            let preserved_local = (*tail_code).operand.local_addr as usize;
            let first_base_local = (*tail_code.add(1)).operand.local_addr as usize;
            let first_delta = (*tail_code.add(2)).operand.i32 as u32;
            let first_memarg = (*tail_code.add(3)).operand.memarg;
            let second_addr_local = (*tail_code.add(4)).operand.local_addr as usize;
            let second_memarg = (*tail_code.add(5)).operand.memarg;
            let local_base = ctx.local_base_ptr as *const u8;
            let preserved = ctx.stack.local_u32_from_base(local_base, preserved_local);
            vm_try!(ctx.stack.push_u32_fast(preserved));
            let first_offset = ctx
                .stack
                .local_u32_from_base(local_base, first_base_local)
                .wrapping_add(first_delta);
            let first_start = vm_try!(compute_memory_offset(first_memarg, first_offset));
            let first = $first_convert(vm_try!(
                unsafe { ctx.default_local_memory_unchecked() }.$first_reader(first_start)
            ));
            vm_try!(ctx.stack.$first_push(first));
            let second_offset = ctx.stack.local_u32_from_base(local_base, second_addr_local);
            let second_start = vm_try!(compute_memory_offset(second_memarg, second_offset));
            let second = $second_convert(vm_try!(
                unsafe { ctx.default_local_memory_unchecked() }.$second_reader(second_start)
            ));
            vm_try!(ctx.stack.$second_push(second));
            let skip_slots = (*tail_code.add(6)).operand.u32 as isize;
            call_next(tail_code, skip_slots, ctx)
        }
    };
}

macro_rules! define_i32_load_local_get4 {
    ($name:ident, $mnemonic:literal, $reader:ident, $convert:path, $push:ident) => {
        #[doc = concat!("WebAssembly `", $mnemonic, "; local.get` on default local memory.")]
        #[allow(dead_code)]
        pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            profile_memory_family(stringify!($name));
            let start = vm_try!(load_start(tail_code, ctx));
            let preserved = (*tail_code.add(1)).operand.local_addr as usize;
            let value = $convert(vm_try!(
                unsafe { ctx.default_local_memory_unchecked() }.$reader(start)
            ));
            vm_try!(ctx.stack.$push(value));
            let preserved = ctx
                .stack
                .local_u32_from_base(ctx.local_base_ptr as *const u8, preserved);
            vm_try!(ctx.stack.push_u32_fast(preserved));
            call_next(tail_code, 2, ctx)
        }
    };
}

macro_rules! define_shared_local_base_push_load {
    ($name:ident, $mnemonic:literal, $bytes:expr) => {
        #[doc = concat!("WebAssembly `", $mnemonic, "` with local-base address on shared default memory.")]
        #[allow(dead_code)]
        pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            profile_memory_family(stringify!($name));
            let start = vm_try!(load_start_shared_local_base(tail_code, ctx));
            vm_try!(ctx.gc.shared_push_memory_to_stack::<$bytes>(
                ctx.default_shared_memory_id_unchecked(),
                ctx.stack,
                start,
            ));
            call_next(tail_code, 3, ctx)
        }
    };
}

macro_rules! define_indexed_shared_local_base_push_load {
    ($name:ident, $mnemonic:literal, $bytes:expr) => {
        #[doc = concat!("WebAssembly `", $mnemonic, "` with local-base address on indexed shared memory.")]
        #[allow(dead_code)]
        pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            profile_memory_family(stringify!($name));
            let (start, memidx) = vm_try!(load_start_indexed_shared_local_base(tail_code, ctx));
            vm_try!(ctx.gc.shared_push_memory_to_stack::<$bytes>(
                ctx.shared_memory_id_at_unchecked(memidx),
                ctx.stack,
                start,
            ));
            call_next(tail_code, 4, ctx)
        }
    };
}

macro_rules! define_shared_local_base_scalar_load {
    ($name:ident, $mnemonic:literal, $reader:ident, $push:ident, $convert:path) => {
        #[doc = concat!("WebAssembly `", $mnemonic, "` with local-base address on shared default memory.")]
        #[allow(dead_code)]
        pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            profile_memory_family(stringify!($name));
            let start = vm_try!(load_start_shared_local_base(tail_code, ctx));
            let value = vm_try!(ctx
                .gc
                .$reader(ctx.default_shared_memory_id_unchecked(), start));
            vm_try!(ctx.stack.$push($convert(value)));
            call_next(tail_code, 3, ctx)
        }
    };
}

macro_rules! define_indexed_shared_local_base_scalar_load {
    ($name:ident, $mnemonic:literal, $reader:ident, $push:ident, $convert:path) => {
        #[doc = concat!("WebAssembly `", $mnemonic, "` with local-base address on indexed shared memory.")]
        #[allow(dead_code)]
        pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            profile_memory_family(stringify!($name));
            let (start, memidx) = vm_try!(load_start_indexed_shared_local_base(tail_code, ctx));
            let value = vm_try!(ctx.gc.$reader(ctx.shared_memory_id_at_unchecked(memidx), start));
            vm_try!(ctx.stack.$push($convert(value)));
            call_next(tail_code, 4, ctx)
        }
    };
}

macro_rules! define_shared_local_base_store_alias {
    ($name:ident, $make_operation:expr) => {
        #[allow(dead_code)]
        pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            profile_memory_family(stringify!($name));
            store_internal_shared_local_base(tail_code, ctx, stringify!($name), $make_operation)
        }
    };
}

macro_rules! define_indexed_shared_local_base_store_alias {
    ($name:ident, $make_operation:expr) => {
        #[allow(dead_code)]
        pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            profile_memory_family(stringify!($name));
            store_internal_indexed_shared_local_base(
                tail_code,
                ctx,
                stringify!($name),
                $make_operation,
            )
        }
    };
}

macro_rules! define_local_scaled_index_push_load {
    ($name:ident, $mnemonic:literal, $bytes:expr) => {
        #[doc = concat!("WebAssembly `", $mnemonic, "` with scaled-index address on default local memory.")]
        #[allow(dead_code)]
        pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            profile_memory_family(stringify!($name));
            let start = vm_try!(load_start_local_scaled_index(tail_code, ctx));
            vm_try!(default_local_push_to_stack::<$bytes>(ctx, start));
            call_next(tail_code, 5, ctx)
        }
    };
}

macro_rules! define_indexed_local_scaled_index_push_load {
    ($name:ident, $mnemonic:literal, $bytes:expr) => {
        #[doc = concat!("WebAssembly `", $mnemonic, "` with scaled-index address on indexed local memory.")]
        #[allow(dead_code)]
        pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            profile_memory_family(stringify!($name));
            let (start, memidx) = vm_try!(load_start_indexed_local_scaled_index(tail_code, ctx));
            vm_try!(ctx.gc.local_push_memory_to_stack::<$bytes>(
                ctx.local_memory_id_at_unchecked(memidx),
                ctx.stack,
                start,
            ));
            call_next(tail_code, 6, ctx)
        }
    };
}

macro_rules! define_shared_local_scaled_index_push_load {
    ($name:ident, $mnemonic:literal, $bytes:expr) => {
        #[doc = concat!("WebAssembly `", $mnemonic, "` with scaled-index address on shared default memory.")]
        #[allow(dead_code)]
        pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            profile_memory_family(stringify!($name));
            let start = vm_try!(load_start_shared_local_scaled_index(tail_code, ctx));
            vm_try!(ctx.gc.shared_push_memory_to_stack::<$bytes>(
                ctx.default_shared_memory_id_unchecked(),
                ctx.stack,
                start,
            ));
            call_next(tail_code, 5, ctx)
        }
    };
}

macro_rules! define_indexed_shared_local_scaled_index_push_load {
    ($name:ident, $mnemonic:literal, $bytes:expr) => {
        #[doc = concat!("WebAssembly `", $mnemonic, "` with scaled-index address on indexed shared memory.")]
        #[allow(dead_code)]
        pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            profile_memory_family(stringify!($name));
            let (start, memidx) =
                vm_try!(load_start_indexed_shared_local_scaled_index(tail_code, ctx));
            vm_try!(ctx.gc.shared_push_memory_to_stack::<$bytes>(
                ctx.shared_memory_id_at_unchecked(memidx),
                ctx.stack,
                start,
            ));
            call_next(tail_code, 6, ctx)
        }
    };
}

macro_rules! define_local_scaled_index_scalar_load {
    ($name:ident, $mnemonic:literal, $reader:ident, $push:ident, $convert:path) => {
        #[doc = concat!("WebAssembly `", $mnemonic, "` with scaled-index address on default local memory.")]
        #[allow(dead_code)]
        pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            profile_memory_family(stringify!($name));
            let start = vm_try!(load_start_local_scaled_index(tail_code, ctx));
            let value = vm_try!(unsafe { ctx.default_local_memory_unchecked() }.$reader(start));
            vm_try!(ctx.stack.$push($convert(value)));
            call_next(tail_code, 5, ctx)
        }
    };
}

macro_rules! define_indexed_local_scaled_index_scalar_load {
    ($name:ident, $mnemonic:literal, $reader:ident, $push:ident, $convert:path) => {
        #[doc = concat!("WebAssembly `", $mnemonic, "` with scaled-index address on indexed local memory.")]
        #[allow(dead_code)]
        pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            profile_memory_family(stringify!($name));
            let (start, memidx) = vm_try!(load_start_indexed_local_scaled_index(tail_code, ctx));
            let value = vm_try!(ctx.gc.$reader(ctx.local_memory_id_at_unchecked(memidx), start));
            vm_try!(ctx.stack.$push($convert(value)));
            call_next(tail_code, 6, ctx)
        }
    };
}

macro_rules! define_shared_local_scaled_index_scalar_load {
    ($name:ident, $mnemonic:literal, $reader:ident, $push:ident, $convert:path) => {
        #[doc = concat!("WebAssembly `", $mnemonic, "` with scaled-index address on shared default memory.")]
        #[allow(dead_code)]
        pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            profile_memory_family(stringify!($name));
            let start = vm_try!(load_start_shared_local_scaled_index(tail_code, ctx));
            let value = vm_try!(ctx
                .gc
                .$reader(ctx.default_shared_memory_id_unchecked(), start));
            vm_try!(ctx.stack.$push($convert(value)));
            call_next(tail_code, 5, ctx)
        }
    };
}

macro_rules! define_indexed_shared_local_scaled_index_scalar_load {
    ($name:ident, $mnemonic:literal, $reader:ident, $push:ident, $convert:path) => {
        #[doc = concat!("WebAssembly `", $mnemonic, "` with scaled-index address on indexed shared memory.")]
        #[allow(dead_code)]
        pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            profile_memory_family(stringify!($name));
            let (start, memidx) =
                vm_try!(load_start_indexed_shared_local_scaled_index(tail_code, ctx));
            let value = vm_try!(ctx.gc.$reader(ctx.shared_memory_id_at_unchecked(memidx), start));
            vm_try!(ctx.stack.$push($convert(value)));
            call_next(tail_code, 6, ctx)
        }
    };
}

macro_rules! define_local_scaled_index_store_alias {
    ($name:ident, $make_operation:expr) => {
        #[allow(dead_code)]
        pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            profile_memory_family(stringify!($name));
            store_internal_local_scaled_index(tail_code, ctx, stringify!($name), $make_operation)
        }
    };
}

macro_rules! define_indexed_local_scaled_index_store_alias {
    ($name:ident, $make_operation:expr) => {
        #[allow(dead_code)]
        pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            profile_memory_family(stringify!($name));
            store_internal_indexed_local_scaled_index(
                tail_code,
                ctx,
                stringify!($name),
                $make_operation,
            )
        }
    };
}

macro_rules! define_shared_local_scaled_index_store_alias {
    ($name:ident, $make_operation:expr) => {
        #[allow(dead_code)]
        pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            profile_memory_family(stringify!($name));
            store_internal_shared_local_scaled_index(
                tail_code,
                ctx,
                stringify!($name),
                $make_operation,
            )
        }
    };
}

macro_rules! define_indexed_shared_local_scaled_index_store_alias {
    ($name:ident, $make_operation:expr) => {
        #[allow(dead_code)]
        pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            profile_memory_family(stringify!($name));
            store_internal_indexed_shared_local_scaled_index(
                tail_code,
                ctx,
                stringify!($name),
                $make_operation,
            )
        }
    };
}

/// WebAssembly `i32.load`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[i32] -> [value]`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Uses little-endian linear-memory access and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_load(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    profile_memory_family("op_i32_load");
    let start = vm_try!(load_start(tail_code, ctx));
    vm_try!(default_local_push_to_stack::<4>(ctx, start));
    call_next(tail_code, 1, ctx)
}

/// WebAssembly `i32.load` const-base fast path for default local memory.
///
/// Spec:
/// - Validation: equivalent to a validated `i32.const <addr>; i32.load` sequence.
/// - Execution: uses the folded `memarg.offset` as the effective address.
///
/// Stack effect: `[] -> [i32]`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: This handler exists only for load-time-specialized const-base scalar loads and keeps tail dispatch unchanged.
///
/// # Safety
/// - `tail_code` must point to a decoded instruction whose folded `memarg` came from a validated const-base `i32.load`.
/// - `ctx` must reference a live execution context with a valid default local memory.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_load_const_base(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    profile_memory_family("op_i32_load_const_base");
    let memarg = (*tail_code).operand.memarg;
    let start = vm_try!(compute_memory_offset(memarg, 0));
    let value = vm_try!(unsafe { ctx.default_local_memory_unchecked() }.read_u32_at(start));
    vm_try!(ctx.stack.push_u32_fast(value));
    call_next(tail_code, 1, ctx)
}

/// WebAssembly `i64.load` const-base fast path for default local memory.
///
/// Spec:
/// - Validation: equivalent to a validated `i64.const <addr>; i64.load` sequence.
/// - Execution: uses the folded `memarg.offset` as the effective address.
///
/// Stack effect: `[] -> [i64]`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: This handler exists only for load-time-specialized const-base scalar loads and keeps tail dispatch unchanged.
///
/// # Safety
/// - `tail_code` must point to a decoded instruction whose folded `memarg` came from a validated const-base `i64.load`.
/// - `ctx` must reference a live execution context with a valid default local memory.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_load_const_base(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    profile_memory_family("op_i64_load_const_base");
    let memarg = (*tail_code).operand.memarg;
    let start = vm_try!(compute_memory_offset(memarg, 0));
    vm_try!(default_local_push_to_stack::<8>(ctx, start));
    call_next(tail_code, 1, ctx)
}

/// WebAssembly `f32.load` const-base fast path for default local memory.
///
/// Spec:
/// - Validation: equivalent to a validated `i32.const <addr>; f32.load` sequence.
/// - Execution: uses the folded `memarg.offset` as the effective address.
///
/// Stack effect: `[] -> [f32]`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: This handler exists only for load-time-specialized const-base scalar loads and keeps tail dispatch unchanged.
///
/// # Safety
/// - `tail_code` must point to a decoded instruction whose folded `memarg` came from a validated const-base `f32.load`.
/// - `ctx` must reference a live execution context with a valid default local memory.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f32_load_const_base(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    profile_memory_family("op_f32_load_const_base");
    let memarg = (*tail_code).operand.memarg;
    let start = vm_try!(compute_memory_offset(memarg, 0));
    vm_try!(default_local_push_to_stack::<4>(ctx, start));
    call_next(tail_code, 1, ctx)
}

/// WebAssembly `f64.load` const-base fast path for default local memory.
///
/// Spec:
/// - Validation: equivalent to a validated `i32.const <addr>; f64.load` sequence.
/// - Execution: uses the folded `memarg.offset` as the effective address.
///
/// Stack effect: `[] -> [f64]`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: This handler exists only for load-time-specialized const-base scalar loads and keeps tail dispatch unchanged.
///
/// # Safety
/// - `tail_code` must point to a decoded instruction whose folded `memarg` came from a validated const-base `f64.load`.
/// - `ctx` must reference a live execution context with a valid default local memory.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f64_load_const_base(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    profile_memory_family("op_f64_load_const_base");
    let memarg = (*tail_code).operand.memarg;
    let start = vm_try!(compute_memory_offset(memarg, 0));
    vm_try!(default_local_push_to_stack::<8>(ctx, start));
    call_next(tail_code, 1, ctx)
}

/// WebAssembly `i32.store` const-base fast path for default local memory with a local-backed value.
///
/// Spec:
/// - Validation: equivalent to a validated `i32.const <addr>; local.get; i32.store` sequence.
/// - Execution: uses the folded `memarg.offset` as the effective address and reads the value directly from the local slot.
///
/// Stack effect: `[] -> []`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: This handler removes both the address materialization and the store-value stack roundtrip.
///
/// # Safety
/// - `tail_code` must point to a decoded instruction whose operands came from a validated const-base `i32.store` pattern.
/// - `ctx` must reference a live execution context with a valid default local memory and local area.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_store_const_base_local4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    profile_memory_family("op_i32_store_const_base_local4");
    let memarg = (*tail_code).operand.memarg;
    let src = (*tail_code.add(1)).operand.local_addr as usize;
    let start = vm_try!(compute_memory_offset(memarg, 0));
    let value = ctx
        .stack
        .local_u32_from_base(ctx.local_base_ptr as *const u8, src);
    vm_try!(unsafe { ctx.default_local_memory_mut_unchecked() }
        .write_bytes(start, &value.to_le_bytes()));
    call_next(tail_code, 2, ctx)
}

/// WebAssembly `i64.store` const-base fast path for default local memory with a local-backed value.
///
/// Spec:
/// - Validation: equivalent to a validated `i32.const <addr>; local.get; i64.store` sequence.
/// - Execution: uses the folded `memarg.offset` as the effective address and reads the value directly from the local slot.
///
/// Stack effect: `[] -> []`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: This handler removes both the address materialization and the store-value stack roundtrip.
///
/// # Safety
/// - `tail_code` must point to a decoded instruction whose operands came from a validated const-base `i64.store` pattern.
/// - `ctx` must reference a live execution context with a valid default local memory and local area.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_store_const_base_local8(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    profile_memory_family("op_i64_store_const_base_local8");
    let memarg = (*tail_code).operand.memarg;
    let src = (*tail_code.add(1)).operand.local_addr as usize;
    let start = vm_try!(compute_memory_offset(memarg, 0));
    let value = local_u64_bits(ctx, src);
    vm_try!(unsafe { ctx.default_local_memory_mut_unchecked() }
        .write_bytes(start, &value.to_le_bytes()));
    call_next(tail_code, 2, ctx)
}

/// WebAssembly `f32.store` const-base fast path for default local memory with a local-backed value.
///
/// Spec:
/// - Validation: equivalent to a validated `i32.const <addr>; local.get; f32.store` sequence.
/// - Execution: uses the folded `memarg.offset` as the effective address and reads the value directly from the local slot.
///
/// Stack effect: `[] -> []`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: This handler removes both the address materialization and the store-value stack roundtrip.
///
/// # Safety
/// - `tail_code` must point to a decoded instruction whose operands came from a validated const-base `f32.store` pattern.
/// - `ctx` must reference a live execution context with a valid default local memory and local area.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f32_store_const_base_local4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    profile_memory_family("op_f32_store_const_base_local4");
    let memarg = (*tail_code).operand.memarg;
    let src = (*tail_code.add(1)).operand.local_addr as usize;
    let start = vm_try!(compute_memory_offset(memarg, 0));
    let value = ctx
        .stack
        .local_u32_from_base(ctx.local_base_ptr as *const u8, src);
    vm_try!(unsafe { ctx.default_local_memory_mut_unchecked() }
        .write_bytes(start, &value.to_le_bytes()));
    call_next(tail_code, 2, ctx)
}

/// WebAssembly `f64.store` const-base fast path for default local memory with a local-backed value.
///
/// Spec:
/// - Validation: equivalent to a validated `i32.const <addr>; local.get; f64.store` sequence.
/// - Execution: uses the folded `memarg.offset` as the effective address and reads the value directly from the local slot.
///
/// Stack effect: `[] -> []`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: This handler removes both the address materialization and the store-value stack roundtrip.
///
/// # Safety
/// - `tail_code` must point to a decoded instruction whose operands came from a validated const-base `f64.store` pattern.
/// - `ctx` must reference a live execution context with a valid default local memory and local area.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f64_store_const_base_local8(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    profile_memory_family("op_f64_store_const_base_local8");
    let memarg = (*tail_code).operand.memarg;
    let src = (*tail_code.add(1)).operand.local_addr as usize;
    let start = vm_try!(compute_memory_offset(memarg, 0));
    let value = local_u64_bits(ctx, src);
    vm_try!(unsafe { ctx.default_local_memory_mut_unchecked() }
        .write_bytes(start, &value.to_le_bytes()));
    call_next(tail_code, 2, ctx)
}

/// WebAssembly `i32.load + local.get4 + i32.add + local.set4` fused const-base fast path.
///
/// Spec:
/// - Validation: equivalent to a validated `i32.const <addr>; i32.load; local.get; i32.add; local.set` sequence.
/// - Execution: loads from default local memory using the folded `memarg.offset`, adds a local-backed rhs, and writes the result directly to the destination local.
///
/// Stack effect: `[] -> []`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: This is a bounded cross-family fusion used only for the default local-memory scalar path.
///
/// # Safety
/// - `tail_code` must point to a decoded instruction whose operands came from a validated fused const-base pattern.
/// - `ctx` must reference a live execution context with a valid default local memory and local area.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_load_const_base_local_get4_i32_add_set4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    profile_memory_family("op_i32_load_const_base_local_get4_i32_add_set4");
    let memarg = (*tail_code).operand.memarg;
    let rhs = (*tail_code.add(1)).operand.local_addr as usize;
    let dst = (*tail_code.add(2)).operand.local_addr as usize;
    let start = vm_try!(compute_memory_offset(memarg, 0));
    let loaded = vm_try!(unsafe { ctx.default_local_memory_unchecked() }.read_u32_at(start));
    let rhs = ctx
        .stack
        .local_u32_from_base(ctx.local_base_ptr as *const u8, rhs);
    let result = loaded.wrapping_add(rhs);
    ctx.stack
        .local_set4_from_base_value(ctx.local_base_ptr, dst, result);
    call_next(tail_code, 3, ctx)
}

#[allow(dead_code)]
/// WebAssembly `local.get; local.get; i32.xor; local.tee; i32.shl; i32.load16_u` CRC lookup fusion.
///
/// # Safety
/// - `tail_code` must point to operands decoded for this CRC lookup handler.
/// - `ctx` must hold a valid frame, local base, and default memory for the active module.
pub unsafe fn op_local_get4_local_get4_i32_xor_tee4_u8_shl1_i32_load16_u(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    profile_memory_family("op_local_get4_local_get4_i32_xor_tee4_u8_shl1_i32_load16_u");
    let lhs = (*tail_code).operand.local_addr as usize;
    let rhs = (*tail_code.add(1)).operand.local_addr as usize;
    let dst = (*tail_code.add(2)).operand.local_addr as usize;
    let memarg = (*tail_code.add(3)).operand.memarg;
    let local_base = ctx.local_base_ptr as *const u8;
    let value = ctx.stack.local_u32_from_base(local_base, lhs)
        ^ ctx.stack.local_u32_from_base(local_base, rhs);
    ctx.stack
        .local_set4_from_base_value(ctx.local_base_ptr, dst, value);
    let offset = (value & 0xff).wrapping_shl(1);
    let start = vm_try!(compute_memory_offset(memarg, offset));
    let loaded = u32::from(vm_try!(
        unsafe { ctx.default_local_memory_unchecked() }.read_u16_at(start)
    ));
    vm_try!(ctx.stack.push_u32_fast(loaded));
    call_next(tail_code, 4, ctx)
}

/// WebAssembly `local.get; i32.load; i32.add; local.set` with a local-base memory operand.
///
/// # Safety
/// - `tail_code` must point to operands decoded for this local-base load-add-set handler.
/// - `ctx` must hold a valid frame, local base, and default memory for the active module.
pub unsafe fn op_local_get4_i32_load_local_base_i32_add_set4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    profile_memory_family("op_local_get4_i32_load_local_base_i32_add_set4");
    let rhs = (*tail_code).operand.local_addr as usize;
    let base_local = (*tail_code.add(1)).operand.local_addr as usize;
    let delta = (*tail_code.add(2)).operand.i32 as u32;
    let memarg = (*tail_code.add(3)).operand.memarg;
    let dst = (*tail_code.add(4)).operand.local_addr as usize;
    let local_base = ctx.local_base_ptr as *const u8;
    let offset = ctx
        .stack
        .local_u32_from_base(local_base, base_local)
        .wrapping_add(delta);
    let start = vm_try!(compute_memory_offset(memarg, offset));
    let loaded = vm_try!(unsafe { ctx.default_local_memory_unchecked() }.read_u32_at(start));
    let rhs = ctx.stack.local_u32_from_base(local_base, rhs);
    let result = rhs.wrapping_add(loaded);
    ctx.stack
        .local_set4_from_base_value(ctx.local_base_ptr, dst, result);
    call_next(tail_code, 5, ctx)
}

/// WebAssembly `local.get; i32.load; i32.add; local.tee` with a local-base memory operand.
///
/// # Safety
/// - `tail_code` must point to operands decoded for this local-base load-add-tee handler.
/// - `ctx` must hold a valid frame, local base, and default memory for the active module.
pub unsafe fn op_local_get4_i32_load_local_base_i32_add_tee4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    profile_memory_family("op_local_get4_i32_load_local_base_i32_add_tee4");
    let rhs = (*tail_code).operand.local_addr as usize;
    let base_local = (*tail_code.add(1)).operand.local_addr as usize;
    let delta = (*tail_code.add(2)).operand.i32 as u32;
    let memarg = (*tail_code.add(3)).operand.memarg;
    let dst = (*tail_code.add(4)).operand.local_addr as usize;
    let local_base = ctx.local_base_ptr as *const u8;
    let offset = ctx
        .stack
        .local_u32_from_base(local_base, base_local)
        .wrapping_add(delta);
    let start = vm_try!(compute_memory_offset(memarg, offset));
    let loaded = vm_try!(unsafe { ctx.default_local_memory_unchecked() }.read_u32_at(start));
    let rhs = ctx.stack.local_u32_from_base(local_base, rhs);
    let result = rhs.wrapping_add(loaded);
    vm_try!(ctx.stack.push_u32_fast(result));
    ctx.stack
        .local_set4_from_base_value(ctx.local_base_ptr, dst, result);
    call_next(tail_code, 5, ctx)
}

/// WebAssembly `i32.load; local.tee; br_if` with a local-base memory operand.
///
/// # Safety
/// - `tail_code` must point to operands decoded for this local-base load branch handler.
/// - `ctx` must hold a valid frame, local base, and default memory for the active module.
pub unsafe fn op_i32_load_local_base_tee4_br_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    profile_memory_family("op_i32_load_local_base_tee4_br_if");
    let start = vm_try!(load_start_local_base(tail_code, ctx));
    let dst = (*tail_code.add(3)).operand.local_addr as usize;
    let value = vm_try!(unsafe { ctx.default_local_memory_unchecked() }.read_u32_at(start));
    ctx.stack
        .local_set4_from_base_value(ctx.local_base_ptr, dst, value);
    let ptr = memory_br_if_ptr(tail_code, 4, 5, value, ctx);
    call_next(ptr, 0, ctx)
}

/// WebAssembly `i32.load; local.tee; i32.eqz; br_if` with a local-base memory operand.
///
/// # Safety
/// - `tail_code` must point to operands decoded for this local-base load eqz branch handler.
/// - `ctx` must hold a valid frame, local base, and default memory for the active module.
pub unsafe fn op_i32_load_local_base_tee4_i32_eqz_br_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    profile_memory_family("op_i32_load_local_base_tee4_i32_eqz_br_if");
    let start = vm_try!(load_start_local_base(tail_code, ctx));
    let dst = (*tail_code.add(3)).operand.local_addr as usize;
    let value = vm_try!(unsafe { ctx.default_local_memory_unchecked() }.read_u32_at(start));
    ctx.stack
        .local_set4_from_base_value(ctx.local_base_ptr, dst, value);
    let ptr = memory_br_if_ptr(tail_code, 4, 5, (value == 0) as u32, ctx);
    call_next(ptr, 0, ctx)
}

macro_rules! define_local_base_i32_load_tee4_branch {
    ($br_name:ident, $eqz_name:ident, $mnemonic:literal, $reader:ident, $convert:path) => {
        #[doc = concat!("WebAssembly `", $mnemonic, "; local.tee; br_if` with local-base address on default local memory.")]
        #[allow(dead_code)]
        pub unsafe fn $br_name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            profile_memory_family(stringify!($br_name));
            let start = vm_try!(load_start_local_base(tail_code, ctx));
            let dst = (*tail_code.add(3)).operand.local_addr as usize;
            let value =
                $convert(vm_try!(unsafe { ctx.default_local_memory_unchecked() }.$reader(start)))
                    as u32;
            ctx.stack
                .local_set4_from_base_value(ctx.local_base_ptr, dst, value);
            let ptr = memory_br_if_ptr(tail_code, 4, 5, value, ctx);
            call_next(ptr, 0, ctx)
        }

        #[doc = concat!("WebAssembly `", $mnemonic, "; local.tee; i32.eqz; br_if` with local-base address on default local memory.")]
        #[allow(dead_code)]
        pub unsafe fn $eqz_name(
            tail_code: *const Instr,
            ctx: &mut ExecuteContext,
        ) -> VMResult<()> {
            profile_memory_family(stringify!($eqz_name));
            let start = vm_try!(load_start_local_base(tail_code, ctx));
            let dst = (*tail_code.add(3)).operand.local_addr as usize;
            let value =
                $convert(vm_try!(unsafe { ctx.default_local_memory_unchecked() }.$reader(start)))
                    as u32;
            ctx.stack
                .local_set4_from_base_value(ctx.local_base_ptr, dst, value);
            let ptr = memory_br_if_ptr(tail_code, 4, 5, (value == 0) as u32, ctx);
            call_next(ptr, 0, ctx)
        }
    };
}

macro_rules! define_i32_load_tee4_branch {
    ($br_name:ident, $eqz_name:ident, $mnemonic:literal, $reader:ident, $convert:path) => {
        #[doc = concat!("WebAssembly `", $mnemonic, "; local.tee; br_if` on default local memory.")]
        #[allow(dead_code)]
        pub unsafe fn $br_name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            profile_memory_family(stringify!($br_name));
            let start = vm_try!(load_start(tail_code, ctx));
            let dst = (*tail_code.add(1)).operand.local_addr as usize;
            let value =
                $convert(vm_try!(unsafe { ctx.default_local_memory_unchecked() }.$reader(start)))
                    as u32;
            ctx.stack
                .local_set4_from_base_value(ctx.local_base_ptr, dst, value);
            let ptr = memory_br_if_ptr(tail_code, 2, 3, value, ctx);
            call_next(ptr, 0, ctx)
        }

        #[doc = concat!("WebAssembly `", $mnemonic, "; local.tee; i32.eqz; br_if` on default local memory.")]
        #[allow(dead_code)]
        pub unsafe fn $eqz_name(
            tail_code: *const Instr,
            ctx: &mut ExecuteContext,
        ) -> VMResult<()> {
            profile_memory_family(stringify!($eqz_name));
            let start = vm_try!(load_start(tail_code, ctx));
            let dst = (*tail_code.add(1)).operand.local_addr as usize;
            let value =
                $convert(vm_try!(unsafe { ctx.default_local_memory_unchecked() }.$reader(start)))
                    as u32;
            ctx.stack
                .local_set4_from_base_value(ctx.local_base_ptr, dst, value);
            let ptr = memory_br_if_ptr(tail_code, 2, 3, (value == 0) as u32, ctx);
            call_next(ptr, 0, ctx)
        }
    };
}

/// WebAssembly `i64.load`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[i32] -> [value]`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Uses little-endian linear-memory access and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_load(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    profile_memory_family("op_i64_load");
    let start = vm_try!(load_start(tail_code, ctx));
    vm_try!(default_local_push_to_stack::<8>(ctx, start));
    call_next(tail_code, 1, ctx)
}

/// WebAssembly `f32.load`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[i32] -> [value]`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Uses little-endian linear-memory access and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f32_load(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    profile_memory_family("op_f32_load");
    let start = vm_try!(load_start(tail_code, ctx));
    vm_try!(default_local_push_to_stack::<4>(ctx, start));
    call_next(tail_code, 1, ctx)
}

/// WebAssembly `f64.load`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[i32] -> [value]`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Uses little-endian linear-memory access and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f64_load(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    profile_memory_family("op_f64_load");
    let start = vm_try!(load_start(tail_code, ctx));
    vm_try!(default_local_push_to_stack::<8>(ctx, start));
    call_next(tail_code, 1, ctx)
}

/// WebAssembly `i32.load8_u`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[i32] -> [value]`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Uses little-endian linear-memory access and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_load8_u(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    profile_memory_family("op_i32_load8_u");
    let start = vm_try!(load_start(tail_code, ctx));
    let value = vm_try!(unsafe { ctx.default_local_memory_unchecked() }.read_u8_at(start));
    vm_try!(ctx.stack.push_u32_fast(u32::from(value)));
    call_next(tail_code, 1, ctx)
}

/// WebAssembly `i32.load8_s`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[i32] -> [value]`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Uses little-endian linear-memory access and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_load8_s(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    profile_memory_family("op_i32_load8_s");
    let start = vm_try!(load_start(tail_code, ctx));
    let value = vm_try!(unsafe { ctx.default_local_memory_unchecked() }.read_i8_at(start));
    vm_try!(ctx.stack.push_i32_fast(i32::from(value)));
    call_next(tail_code, 1, ctx)
}

/// WebAssembly `i32.load16_s`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[i32] -> [value]`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Uses little-endian linear-memory access and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_load16_s(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    profile_memory_family("op_i32_load16_s");
    let start = vm_try!(load_start(tail_code, ctx));
    let value = vm_try!(unsafe { ctx.default_local_memory_unchecked() }.read_i16_at(start));
    vm_try!(ctx.stack.push_i32_fast(i32::from(value)));
    call_next(tail_code, 1, ctx)
}

/// WebAssembly `i32.load16_u`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[i32] -> [value]`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Uses little-endian linear-memory access and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_load16_u(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    profile_memory_family("op_i32_load16_u");
    let start = vm_try!(load_start(tail_code, ctx));
    let value = vm_try!(unsafe { ctx.default_local_memory_unchecked() }.read_u16_at(start));
    vm_try!(ctx.stack.push_u32_fast(u32::from(value)));
    call_next(tail_code, 1, ctx)
}

/// WebAssembly `i64.load8_s`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[i32] -> [value]`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Uses little-endian linear-memory access and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_load8_s(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    profile_memory_family("op_i64_load8_s");
    let start = vm_try!(load_start(tail_code, ctx));
    let value = vm_try!(unsafe { ctx.default_local_memory_unchecked() }.read_i8_at(start));
    vm_try!(ctx.stack.push_i64(i64::from(value)));
    call_next(tail_code, 1, ctx)
}

/// WebAssembly `i64.load8_u`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[i32] -> [value]`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Uses little-endian linear-memory access and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_load8_u(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    profile_memory_family("op_i64_load8_u");
    let start = vm_try!(load_start(tail_code, ctx));
    let value = vm_try!(unsafe { ctx.default_local_memory_unchecked() }.read_u8_at(start));
    vm_try!(ctx.stack.push_u64(u64::from(value)));
    call_next(tail_code, 1, ctx)
}

/// WebAssembly `i64.load16_s`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[i32] -> [value]`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Uses little-endian linear-memory access and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_load16_s(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    profile_memory_family("op_i64_load16_s");
    let start = vm_try!(load_start(tail_code, ctx));
    let value = vm_try!(unsafe { ctx.default_local_memory_unchecked() }.read_i16_at(start));
    vm_try!(ctx.stack.push_i64(i64::from(value)));
    call_next(tail_code, 1, ctx)
}

/// WebAssembly `i64.load16_u`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[i32] -> [value]`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Uses little-endian linear-memory access and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_load16_u(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    profile_memory_family("op_i64_load16_u");
    let start = vm_try!(load_start(tail_code, ctx));
    let value = vm_try!(unsafe { ctx.default_local_memory_unchecked() }.read_u16_at(start));
    vm_try!(ctx.stack.push_u64(u64::from(value)));
    call_next(tail_code, 1, ctx)
}

/// WebAssembly `i64.load32_s`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[i32] -> [value]`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Uses little-endian linear-memory access and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_load32_s(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    profile_memory_family("op_i64_load32_s");
    let start = vm_try!(load_start(tail_code, ctx));
    let value = vm_try!(unsafe { ctx.default_local_memory_unchecked() }.read_i32_at(start));
    vm_try!(ctx.stack.push_i64(i64::from(value)));
    call_next(tail_code, 1, ctx)
}

/// WebAssembly `i64.load32_u`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[i32] -> [value]`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Uses little-endian linear-memory access and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_load32_u(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    profile_memory_family("op_i64_load32_u");
    let start = vm_try!(load_start(tail_code, ctx));
    let value = vm_try!(unsafe { ctx.default_local_memory_unchecked() }.read_u32_at(start));
    vm_try!(ctx.stack.push_u64(u64::from(value)));
    call_next(tail_code, 1, ctx)
}

/// WebAssembly `i32.store`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[i32, value] -> []`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Uses little-endian linear-memory access and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_store(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    store_internal_local(tail_code, ctx, "op_i32_store", |ctx| {
        StoreBytes::Write4(ctx.stack.pop_u8_array::<4>())
    })
}

/// WebAssembly `i64.store`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[i32, value] -> []`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Uses little-endian linear-memory access and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_store(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    store_internal_local(tail_code, ctx, "op_i64_store", |ctx| {
        StoreBytes::Write8(ctx.stack.pop_u8_array::<8>())
    })
}

/// WebAssembly `f32.store`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[i32, value] -> []`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Uses little-endian linear-memory access and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f32_store(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    store_internal_local(tail_code, ctx, "op_f32_store", |ctx| {
        StoreBytes::Write4(ctx.stack.pop_u8_array::<4>())
    })
}

/// WebAssembly `f64.store`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[i32, value] -> []`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Uses little-endian linear-memory access and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f64_store(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    store_internal_local(tail_code, ctx, "op_f64_store", |ctx| {
        StoreBytes::Write8(ctx.stack.pop_u8_array::<8>())
    })
}

/// WebAssembly `i32.store8`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[i32, value] -> []`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Uses little-endian linear-memory access and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_store8(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    store_internal_local(tail_code, ctx, "op_i32_store8", |ctx| {
        StoreBytes::Write1(truncate_u32_to_u8_bytes(ctx.stack.pop_u32()))
    })
}

/// WebAssembly `i32.store16`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[i32, value] -> []`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Uses little-endian linear-memory access and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_store16(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    store_internal_local(tail_code, ctx, "op_i32_store16", |ctx| {
        StoreBytes::Write2(truncate_u32_to_u16_bytes(ctx.stack.pop_u32()))
    })
}

/// WebAssembly `i64.store8`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[i32, value] -> []`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Uses little-endian linear-memory access and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_store8(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    store_internal_local(tail_code, ctx, "op_i64_store8", |ctx| {
        StoreBytes::Write1(truncate_u64_to_u8_bytes(ctx.stack.pop_u64()))
    })
}

/// WebAssembly `i64.store16`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[i32, value] -> []`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Uses little-endian linear-memory access and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_store16(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    store_internal_local(tail_code, ctx, "op_i64_store16", |ctx| {
        StoreBytes::Write2(truncate_u64_to_u16_bytes(ctx.stack.pop_u64()))
    })
}

/// WebAssembly `i64.store32`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[i32, value] -> []`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Uses little-endian linear-memory access and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_store32(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    store_internal_local(tail_code, ctx, "op_i64_store32", |ctx| {
        StoreBytes::Write4(truncate_u64_to_u32_bytes(ctx.stack.pop_u64()))
    })
}

/// WebAssembly `memory.size`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[] -> [i32]`.
/// Traps: traps when no default memory exists.
/// Notes: Uses little-endian linear-memory access and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_mem_size(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let page_size = unsafe { ctx.default_local_memory_unchecked() }.page_size();
    vm_try!(ctx.stack.push_u32(page_size));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `memory.grow`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[i32] -> [i32]`.
/// Traps: traps when no default memory exists; otherwise returns `-1` on growth failure.
/// Notes: Uses little-endian linear-memory access and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_mem_grow(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let page_size_delta = ctx.stack.pop_u32();
    let result = vm_try!(ctx
        .gc
        .local_grow_memory(ctx.default_local_memory_id_unchecked(), page_size_delta,));
    vm_try!(ctx.stack.push_i32(result));
    call_next(tail_code, 0, ctx)
}

macro_rules! define_shared_push_load {
    ($name:ident, $mnemonic:literal, $bytes:expr) => {
        #[doc = concat!("WebAssembly `", $mnemonic, "` on shared default memory.")]
        ///
        /// Spec:
        /// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
        /// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
        /// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
        ///
        /// Stack effect: `[i32] -> [value]`.
        /// Traps: traps on out-of-bounds memory access.
        /// Notes: Uses the shared-memory specialized fast path selected by the parser and tail-dispatches with `call_next`.
        ///
        /// # Safety
        /// - `tail_code` must point to the decoded instruction for this handler in the active function body.
        /// - `ctx` must reference a live execution context whose default memory is shared.
        /// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
        pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            profile_memory_family(stringify!($name));
            let start = vm_try!(load_start(tail_code, ctx));
            vm_try!(ctx.gc.shared_push_memory_to_stack::<$bytes>(
                ctx.default_shared_memory_id_unchecked(),
                ctx.stack,
                start,
            ));
            call_next(tail_code, 1, ctx)
        }
    };
}

macro_rules! define_shared_scalar_load {
    ($name:ident, $mnemonic:literal, $reader:ident, $push:ident, $convert:path) => {
        #[doc = concat!("WebAssembly `", $mnemonic, "` on shared default memory.")]
        ///
        /// Spec:
        /// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
        /// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
        /// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
        ///
        /// Stack effect: `[i32] -> [value]`.
        /// Traps: traps on out-of-bounds memory access.
        /// Notes: Uses the shared-memory specialized fast path selected by the parser and tail-dispatches with `call_next`.
        ///
        /// # Safety
        /// - `tail_code` must point to the decoded instruction for this handler in the active function body.
        /// - `ctx` must reference a live execution context whose default memory is shared.
        /// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
        pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            profile_memory_family(stringify!($name));
            let start = vm_try!(load_start(tail_code, ctx));
            let value = vm_try!(ctx
                .gc
                .$reader(ctx.default_shared_memory_id_unchecked(), start));
            vm_try!(ctx.stack.$push($convert(value)));
            call_next(tail_code, 1, ctx)
        }
    };
}

macro_rules! define_shared_store_alias {
    ($name:ident, $mnemonic:literal, $make_operation:expr) => {
        #[doc = concat!("WebAssembly `", $mnemonic, "` on shared default memory.")]
        ///
        /// Spec:
        /// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
        /// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
        /// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
        ///
        /// Stack effect: `[i32, value] -> []`.
        /// Traps: traps on out-of-bounds memory access.
        /// Notes: Uses the shared-memory specialized fast path selected by the parser and tail-dispatches with `call_next`.
        ///
        /// # Safety
        /// - `tail_code` must point to the decoded instruction for this handler in the active function body.
        /// - `ctx` must reference a live execution context whose default memory is shared.
        /// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
        pub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            store_internal_shared(tail_code, ctx, stringify!($name), $make_operation)
        }
    };
}

define_shared_push_load!(op_i32_load_shared, "i32.load", 4);
define_shared_push_load!(op_i64_load_shared, "i64.load", 8);
define_shared_push_load!(op_f32_load_shared, "f32.load", 4);
define_shared_push_load!(op_f64_load_shared, "f64.load", 8);
define_shared_scalar_load!(
    op_i32_load8_u_shared,
    "i32.load8_u",
    shared_read_u8_at,
    push_u32_fast,
    u32::from
);
define_shared_scalar_load!(
    op_i32_load8_s_shared,
    "i32.load8_s",
    shared_read_i8_at,
    push_i32_fast,
    i32::from
);
define_shared_scalar_load!(
    op_i32_load16_s_shared,
    "i32.load16_s",
    shared_read_i16_at,
    push_i32_fast,
    i32::from
);
define_shared_scalar_load!(
    op_i32_load16_u_shared,
    "i32.load16_u",
    shared_read_u16_at,
    push_u32_fast,
    u32::from
);
define_shared_scalar_load!(
    op_i64_load8_s_shared,
    "i64.load8_s",
    shared_read_i8_at,
    push_i64_fast,
    i64::from
);
define_shared_scalar_load!(
    op_i64_load8_u_shared,
    "i64.load8_u",
    shared_read_u8_at,
    push_u64_fast,
    u64::from
);
define_shared_scalar_load!(
    op_i64_load16_s_shared,
    "i64.load16_s",
    shared_read_i16_at,
    push_i64_fast,
    i64::from
);
define_shared_scalar_load!(
    op_i64_load16_u_shared,
    "i64.load16_u",
    shared_read_u16_at,
    push_u64_fast,
    u64::from
);
define_shared_scalar_load!(
    op_i64_load32_s_shared,
    "i64.load32_s",
    shared_read_i32_at,
    push_i64_fast,
    i64::from
);
define_shared_scalar_load!(
    op_i64_load32_u_shared,
    "i64.load32_u",
    shared_read_u32_at,
    push_u64_fast,
    u64::from
);
define_shared_store_alias!(op_i32_store_shared, "i32.store", |ctx| {
    StoreBytes::Write4(ctx.stack.pop_u8_array::<4>())
});
define_shared_store_alias!(op_i64_store_shared, "i64.store", |ctx| {
    StoreBytes::Write8(ctx.stack.pop_u8_array::<8>())
});
define_shared_store_alias!(op_f32_store_shared, "f32.store", |ctx| {
    StoreBytes::Write4(ctx.stack.pop_u8_array::<4>())
});
define_shared_store_alias!(op_f64_store_shared, "f64.store", |ctx| {
    StoreBytes::Write8(ctx.stack.pop_u8_array::<8>())
});
define_shared_store_alias!(op_i32_store8_shared, "i32.store8", |ctx| {
    StoreBytes::Write1(truncate_u32_to_u8_bytes(ctx.stack.pop_u32()))
});
define_shared_store_alias!(op_i32_store16_shared, "i32.store16", |ctx| {
    StoreBytes::Write2(truncate_u32_to_u16_bytes(ctx.stack.pop_u32()))
});
define_shared_store_alias!(op_i64_store8_shared, "i64.store8", |ctx| {
    StoreBytes::Write1(truncate_u64_to_u8_bytes(ctx.stack.pop_u64()))
});
define_shared_store_alias!(op_i64_store16_shared, "i64.store16", |ctx| {
    StoreBytes::Write2(truncate_u64_to_u16_bytes(ctx.stack.pop_u64()))
});
define_shared_store_alias!(op_i64_store32_shared, "i64.store32", |ctx| {
    StoreBytes::Write4(truncate_u64_to_u32_bytes(ctx.stack.pop_u64()))
});

define_indexed_push_load!(
    op_i32_load_indexed_local,
    op_i32_load_indexed_shared,
    "i32.load",
    4
);
define_indexed_push_load!(
    op_i64_load_indexed_local,
    op_i64_load_indexed_shared,
    "i64.load",
    8
);
define_indexed_push_load!(
    op_f32_load_indexed_local,
    op_f32_load_indexed_shared,
    "f32.load",
    4
);
define_indexed_push_load!(
    op_f64_load_indexed_local,
    op_f64_load_indexed_shared,
    "f64.load",
    8
);
define_indexed_scalar_load!(
    op_i32_load8_u_indexed_local,
    op_i32_load8_u_indexed_shared,
    "i32.load8_u",
    local_read_u8_at,
    shared_read_u8_at,
    push_u32_fast,
    u32::from
);
define_indexed_scalar_load!(
    op_i32_load8_s_indexed_local,
    op_i32_load8_s_indexed_shared,
    "i32.load8_s",
    local_read_i8_at,
    shared_read_i8_at,
    push_i32_fast,
    i32::from
);
define_indexed_scalar_load!(
    op_i32_load16_s_indexed_local,
    op_i32_load16_s_indexed_shared,
    "i32.load16_s",
    local_read_i16_at,
    shared_read_i16_at,
    push_i32_fast,
    i32::from
);
define_indexed_scalar_load!(
    op_i32_load16_u_indexed_local,
    op_i32_load16_u_indexed_shared,
    "i32.load16_u",
    local_read_u16_at,
    shared_read_u16_at,
    push_u32_fast,
    u32::from
);
define_indexed_scalar_load!(
    op_i64_load8_s_indexed_local,
    op_i64_load8_s_indexed_shared,
    "i64.load8_s",
    local_read_i8_at,
    shared_read_i8_at,
    push_i64_fast,
    i64::from
);
define_indexed_scalar_load!(
    op_i64_load8_u_indexed_local,
    op_i64_load8_u_indexed_shared,
    "i64.load8_u",
    local_read_u8_at,
    shared_read_u8_at,
    push_u64_fast,
    u64::from
);
define_indexed_scalar_load!(
    op_i64_load16_s_indexed_local,
    op_i64_load16_s_indexed_shared,
    "i64.load16_s",
    local_read_i16_at,
    shared_read_i16_at,
    push_i64_fast,
    i64::from
);
define_indexed_scalar_load!(
    op_i64_load16_u_indexed_local,
    op_i64_load16_u_indexed_shared,
    "i64.load16_u",
    local_read_u16_at,
    shared_read_u16_at,
    push_u64_fast,
    u64::from
);
define_indexed_scalar_load!(
    op_i64_load32_s_indexed_local,
    op_i64_load32_s_indexed_shared,
    "i64.load32_s",
    local_read_i32_at,
    shared_read_i32_at,
    push_i64_fast,
    i64::from
);
define_indexed_scalar_load!(
    op_i64_load32_u_indexed_local,
    op_i64_load32_u_indexed_shared,
    "i64.load32_u",
    local_read_u32_at,
    shared_read_u32_at,
    push_u64_fast,
    u64::from
);
define_indexed_store_alias!(
    op_i32_store_indexed_local,
    op_i32_store_indexed_shared,
    "i32.store",
    |ctx| { StoreBytes::Write4(ctx.stack.pop_u8_array::<4>()) }
);
define_indexed_store_alias!(
    op_i64_store_indexed_local,
    op_i64_store_indexed_shared,
    "i64.store",
    |ctx| { StoreBytes::Write8(ctx.stack.pop_u8_array::<8>()) }
);
define_indexed_store_alias!(
    op_f32_store_indexed_local,
    op_f32_store_indexed_shared,
    "f32.store",
    |ctx| { StoreBytes::Write4(ctx.stack.pop_u8_array::<4>()) }
);
define_indexed_store_alias!(
    op_f64_store_indexed_local,
    op_f64_store_indexed_shared,
    "f64.store",
    |ctx| { StoreBytes::Write8(ctx.stack.pop_u8_array::<8>()) }
);
define_indexed_store_alias!(
    op_i32_store8_indexed_local,
    op_i32_store8_indexed_shared,
    "i32.store8",
    |ctx| { StoreBytes::Write1(truncate_u32_to_u8_bytes(ctx.stack.pop_u32())) }
);
define_indexed_store_alias!(
    op_i32_store16_indexed_local,
    op_i32_store16_indexed_shared,
    "i32.store16",
    |ctx| { StoreBytes::Write2(truncate_u32_to_u16_bytes(ctx.stack.pop_u32())) }
);
define_indexed_store_alias!(
    op_i64_store8_indexed_local,
    op_i64_store8_indexed_shared,
    "i64.store8",
    |ctx| { StoreBytes::Write1(truncate_u64_to_u8_bytes(ctx.stack.pop_u64())) }
);
define_indexed_store_alias!(
    op_i64_store16_indexed_local,
    op_i64_store16_indexed_shared,
    "i64.store16",
    |ctx| { StoreBytes::Write2(truncate_u64_to_u16_bytes(ctx.stack.pop_u64())) }
);
define_indexed_store_alias!(
    op_i64_store32_indexed_local,
    op_i64_store32_indexed_shared,
    "i64.store32",
    |ctx| { StoreBytes::Write4(truncate_u64_to_u32_bytes(ctx.stack.pop_u64())) }
);

define_local_base_push_load!(op_i32_load_local_base, "i32.load", 4);
define_local_base_push_load!(op_i64_load_local_base, "i64.load", 8);
define_local_base_push_load!(op_f32_load_local_base, "f32.load", 4);
define_local_base_push_load!(op_f64_load_local_base, "f64.load", 8);
define_local_base_scalar_load!(
    op_i32_load8_u_local_base,
    "i32.load8_u",
    read_u8_at,
    push_u32_fast,
    u32::from
);
define_local_base_scalar_load!(
    op_i32_load8_s_local_base,
    "i32.load8_s",
    read_i8_at,
    push_i32_fast,
    i32::from
);
define_local_base_scalar_load!(
    op_i32_load16_s_local_base,
    "i32.load16_s",
    read_i16_at,
    push_i32_fast,
    i32::from
);
define_local_base_scalar_load!(
    op_i32_load16_u_local_base,
    "i32.load16_u",
    read_u16_at,
    push_u32_fast,
    u32::from
);
define_local_base_scalar_load!(
    op_i64_load8_s_local_base,
    "i64.load8_s",
    read_i8_at,
    push_i64_fast,
    i64::from
);
define_local_base_scalar_load!(
    op_i64_load8_u_local_base,
    "i64.load8_u",
    read_u8_at,
    push_u64_fast,
    u64::from
);
define_local_base_scalar_load!(
    op_i64_load16_s_local_base,
    "i64.load16_s",
    read_i16_at,
    push_i64_fast,
    i64::from
);
define_local_base_scalar_load!(
    op_i64_load16_u_local_base,
    "i64.load16_u",
    read_u16_at,
    push_u64_fast,
    u64::from
);
define_local_base_scalar_load!(
    op_i64_load32_s_local_base,
    "i64.load32_s",
    read_i32_at,
    push_i64_fast,
    i64::from
);
define_local_base_scalar_load!(
    op_i64_load32_u_local_base,
    "i64.load32_u",
    read_u32_at,
    push_u64_fast,
    u64::from
);
define_local_get4_local_base_scalar_load!(
    op_local_get4_i32_load_local_base,
    "i32.load",
    read_u32_at,
    push_u32_fast,
    std::convert::identity
);
define_local_get4_local_base_scalar_load!(
    op_local_get4_i32_load8_u_local_base,
    "i32.load8_u",
    read_u8_at,
    push_u32_fast,
    u32::from
);
define_local_get4_local_base_scalar_load!(
    op_local_get4_i32_load8_s_local_base,
    "i32.load8_s",
    read_i8_at,
    push_i32_fast,
    i32::from
);
define_local_get4_local_base_scalar_load!(
    op_local_get4_i32_load16_s_local_base,
    "i32.load16_s",
    read_i16_at,
    push_i32_fast,
    i32::from
);
define_local_get4_local_base_scalar_load!(
    op_local_get4_i32_load16_u_local_base,
    "i32.load16_u",
    read_u16_at,
    push_u32_fast,
    u32::from
);
define_local_base_i32_load_set4!(
    op_i32_load_local_base_set4,
    op_i32_load_local_base_tee4,
    "i32.load",
    read_u32_at,
    std::convert::identity
);
define_local_base_i32_load_set4!(
    op_i32_load8_u_local_base_set4,
    op_i32_load8_u_local_base_tee4,
    "i32.load8_u",
    read_u8_at,
    u32::from
);
define_local_base_i32_load_set4!(
    op_i32_load8_s_local_base_set4,
    op_i32_load8_s_local_base_tee4,
    "i32.load8_s",
    read_i8_at,
    i32::from
);
define_local_base_i32_load_set4!(
    op_i32_load16_u_local_base_set4,
    op_i32_load16_u_local_base_tee4,
    "i32.load16_u",
    read_u16_at,
    u32::from
);
define_local_base_i32_load_set4!(
    op_i32_load16_s_local_base_set4,
    op_i32_load16_s_local_base_tee4,
    "i32.load16_s",
    read_i16_at,
    i32::from
);
define_local_base_i32_load_tee4_branch!(
    op_i32_load8_u_local_base_tee4_br_if,
    op_i32_load8_u_local_base_tee4_i32_eqz_br_if,
    "i32.load8_u",
    read_u8_at,
    u32::from
);
define_local_base_i32_load_tee4_branch!(
    op_i32_load8_s_local_base_tee4_br_if,
    op_i32_load8_s_local_base_tee4_i32_eqz_br_if,
    "i32.load8_s",
    read_i8_at,
    i32::from
);
define_local_base_i32_load_tee4_branch!(
    op_i32_load16_u_local_base_tee4_br_if,
    op_i32_load16_u_local_base_tee4_i32_eqz_br_if,
    "i32.load16_u",
    read_u16_at,
    u32::from
);
define_local_base_i32_load_tee4_branch!(
    op_i32_load16_s_local_base_tee4_br_if,
    op_i32_load16_s_local_base_tee4_i32_eqz_br_if,
    "i32.load16_s",
    read_i16_at,
    i32::from
);
define_i32_load_tee4_branch!(
    op_i32_load_tee4_br_if,
    op_i32_load_tee4_i32_eqz_br_if,
    "i32.load",
    read_u32_at,
    std::convert::identity
);
define_i32_load_tee4_branch!(
    op_i32_load8_u_tee4_br_if,
    op_i32_load8_u_tee4_i32_eqz_br_if,
    "i32.load8_u",
    read_u8_at,
    u32::from
);
define_i32_load_tee4_branch!(
    op_i32_load8_s_tee4_br_if,
    op_i32_load8_s_tee4_i32_eqz_br_if,
    "i32.load8_s",
    read_i8_at,
    i32::from
);
define_i32_load_tee4_branch!(
    op_i32_load16_u_tee4_br_if,
    op_i32_load16_u_tee4_i32_eqz_br_if,
    "i32.load16_u",
    read_u16_at,
    u32::from
);
define_i32_load_tee4_branch!(
    op_i32_load16_s_tee4_br_if,
    op_i32_load16_s_tee4_i32_eqz_br_if,
    "i32.load16_s",
    read_i16_at,
    i32::from
);
define_local_base_i32_load_local_get4!(
    op_i32_load_local_base_local_get4,
    op_i32_load_local_base_tee4_local_get4,
    "i32.load",
    read_u32_at,
    std::convert::identity,
    push_u32_fast
);
define_local_base_i32_load_local_get4!(
    op_i32_load8_u_local_base_local_get4,
    op_i32_load8_u_local_base_tee4_local_get4,
    "i32.load8_u",
    read_u8_at,
    u32::from,
    push_u32_fast
);
define_local_base_i32_load_local_get4!(
    op_i32_load8_s_local_base_local_get4,
    op_i32_load8_s_local_base_tee4_local_get4,
    "i32.load8_s",
    read_i8_at,
    i32::from,
    push_i32_fast
);
define_local_base_i32_load_local_get4!(
    op_i32_load16_u_local_base_local_get4,
    op_i32_load16_u_local_base_tee4_local_get4,
    "i32.load16_u",
    read_u16_at,
    u32::from,
    push_u32_fast
);
define_local_base_i32_load_local_get4!(
    op_i32_load16_s_local_base_local_get4,
    op_i32_load16_s_local_base_tee4_local_get4,
    "i32.load16_s",
    read_i16_at,
    i32::from,
    push_i32_fast
);
define_i32_load_local_base_set4_i32_load_local_base_local_get4!(
    op_i32_load_local_base_set4_i32_load_local_base_local_get4,
    "i32.load",
    read_u32_at,
    std::convert::identity,
    push_u32_fast
);
define_i32_load_local_base_set4_i32_load_local_base_local_get4!(
    op_i32_load_local_base_set4_i32_load8_u_local_base_local_get4,
    "i32.load8_u",
    read_u8_at,
    u32::from,
    push_u32_fast
);
define_i32_load_local_base_set4_i32_load_local_base_local_get4!(
    op_i32_load_local_base_set4_i32_load8_s_local_base_local_get4,
    "i32.load8_s",
    read_i8_at,
    i32::from,
    push_i32_fast
);
define_i32_load_local_base_set4_i32_load_local_base_local_get4!(
    op_i32_load_local_base_set4_i32_load16_u_local_base_local_get4,
    "i32.load16_u",
    read_u16_at,
    u32::from,
    push_u32_fast
);
define_i32_load_local_base_set4_i32_load_local_base_local_get4!(
    op_i32_load_local_base_set4_i32_load16_s_local_base_local_get4,
    "i32.load16_s",
    read_i16_at,
    i32::from,
    push_i32_fast
);
define_i32_load_local_base_set4_i32_load_local_base_local_eq_br_if!(
    op_i32_load_local_base_set4_i32_load_local_base_local_eq_br_if,
    "i32.load",
    read_u32_at,
    std::convert::identity
);
define_i32_load_local_base_set4_i32_load_local_base_local_eq_br_if!(
    op_i32_load_local_base_set4_i32_load8_u_local_base_local_eq_br_if,
    "i32.load8_u",
    read_u8_at,
    u32::from
);
define_i32_load_local_base_set4_i32_load_local_base_local_eq_br_if!(
    op_i32_load_local_base_set4_i32_load8_s_local_base_local_eq_br_if,
    "i32.load8_s",
    read_i8_at,
    i32::from
);
define_i32_load_local_base_set4_i32_load_local_base_local_eq_br_if!(
    op_i32_load_local_base_set4_i32_load16_u_local_base_local_eq_br_if,
    "i32.load16_u",
    read_u16_at,
    u32::from
);
define_i32_load_local_base_set4_i32_load_local_base_local_eq_br_if!(
    op_i32_load_local_base_set4_i32_load16_s_local_base_local_eq_br_if,
    "i32.load16_s",
    read_i16_at,
    i32::from
);
define_i32_load_local_base_set4_i32_load_local_base!(
    op_i32_load_local_base_set4_i32_load_local_base,
    "i32.load",
    read_u32_at,
    std::convert::identity,
    push_u32_fast
);
define_i32_load_local_base_set4_i32_load_local_base!(
    op_i32_load_local_base_set4_i32_load8_u_local_base,
    "i32.load8_u",
    read_u8_at,
    u32::from,
    push_u32_fast
);
define_i32_load_local_base_set4_i32_load_local_base!(
    op_i32_load_local_base_set4_i32_load8_s_local_base,
    "i32.load8_s",
    read_i8_at,
    i32::from,
    push_i32_fast
);
define_i32_load_local_base_set4_i32_load_local_base!(
    op_i32_load_local_base_set4_i32_load16_u_local_base,
    "i32.load16_u",
    read_u16_at,
    u32::from,
    push_u32_fast
);
define_i32_load_local_base_set4_i32_load_local_base!(
    op_i32_load_local_base_set4_i32_load16_s_local_base,
    "i32.load16_s",
    read_i16_at,
    i32::from,
    push_i32_fast
);
define_local_base_i32_load_local_get4_i32_load!(
    op_i32_load16_u_local_base_local_get4_i32_load16_u,
    "i32.load16_u",
    read_u16_at,
    u32::from,
    push_u32_fast,
    "i32.load16_u",
    read_u16_at,
    u32::from,
    push_u32_fast
);
define_local_base_i32_load_local_get4_i32_load!(
    op_i32_load16_s_local_base_local_get4_i32_load16_s,
    "i32.load16_s",
    read_i16_at,
    i32::from,
    push_i32_fast,
    "i32.load16_s",
    read_i16_at,
    i32::from,
    push_i32_fast
);
define_local_base_i32_load_local_get4_i32_load_local_get4!(
    op_i32_load16_u_local_base_local_get4_i32_load16_u_local_get4,
    "i32.load16_u",
    read_u16_at,
    u32::from,
    push_u32_fast,
    "i32.load16_u",
    read_u16_at,
    u32::from,
    push_u32_fast
);
define_local_base_i32_load_local_get4_i32_load_local_get4!(
    op_i32_load16_s_local_base_local_get4_i32_load16_s_local_get4,
    "i32.load16_s",
    read_i16_at,
    i32::from,
    push_i32_fast,
    "i32.load16_s",
    read_i16_at,
    i32::from,
    push_i32_fast
);
define_local_get4_local_base_i32_load_local_get4_i32_load!(
    op_local_get4_i32_load16_u_local_base_local_get4_i32_load16_u,
    "i32.load16_u",
    read_u16_at,
    u32::from,
    push_u32_fast,
    "i32.load16_u",
    read_u16_at,
    u32::from,
    push_u32_fast
);
define_local_get4_local_base_i32_load_local_get4_i32_load!(
    op_local_get4_i32_load16_s_local_base_local_get4_i32_load16_s,
    "i32.load16_s",
    read_i16_at,
    i32::from,
    push_i32_fast,
    "i32.load16_s",
    read_i16_at,
    i32::from,
    push_i32_fast
);
define_i32_load_local_get4!(
    op_i32_load_local_get4,
    "i32.load",
    read_u32_at,
    std::convert::identity,
    push_u32_fast
);
define_i32_load_local_get4!(
    op_i32_load8_u_local_get4,
    "i32.load8_u",
    read_u8_at,
    u32::from,
    push_u32_fast
);
define_i32_load_local_get4!(
    op_i32_load8_s_local_get4,
    "i32.load8_s",
    read_i8_at,
    i32::from,
    push_i32_fast
);
define_i32_load_local_get4!(
    op_i32_load16_u_local_get4,
    "i32.load16_u",
    read_u16_at,
    u32::from,
    push_u32_fast
);
define_i32_load_local_get4!(
    op_i32_load16_s_local_get4,
    "i32.load16_s",
    read_i16_at,
    i32::from,
    push_i32_fast
);
define_local_base_store_alias!(op_i32_store_local_base, |ctx| {
    StoreBytes::Write4(ctx.stack.pop_u8_array::<4>())
});
define_local_base_store_alias!(op_i64_store_local_base, |ctx| {
    StoreBytes::Write8(ctx.stack.pop_u8_array::<8>())
});
define_local_base_store_alias!(op_f32_store_local_base, |ctx| {
    StoreBytes::Write4(ctx.stack.pop_u8_array::<4>())
});
define_local_base_store_alias!(op_f64_store_local_base, |ctx| {
    StoreBytes::Write8(ctx.stack.pop_u8_array::<8>())
});
define_local_base_store_alias!(op_i32_store8_local_base, |ctx| {
    StoreBytes::Write1(truncate_u32_to_u8_bytes(ctx.stack.pop_u32()))
});
define_local_base_store_alias!(op_i32_store16_local_base, |ctx| {
    StoreBytes::Write2(truncate_u32_to_u16_bytes(ctx.stack.pop_u32()))
});
define_local_base_store_alias!(op_i64_store8_local_base, |ctx| {
    StoreBytes::Write1(truncate_u64_to_u8_bytes(ctx.stack.pop_u64()))
});
define_local_base_store_alias!(op_i64_store16_local_base, |ctx| {
    StoreBytes::Write2(truncate_u64_to_u16_bytes(ctx.stack.pop_u64()))
});
define_local_base_store_alias!(op_i64_store32_local_base, |ctx| {
    StoreBytes::Write4(truncate_u64_to_u32_bytes(ctx.stack.pop_u64()))
});

define_indexed_local_base_push_load!(op_i32_load_indexed_local_base, "i32.load", 4);
define_indexed_local_base_push_load!(op_i64_load_indexed_local_base, "i64.load", 8);
define_indexed_local_base_push_load!(op_f32_load_indexed_local_base, "f32.load", 4);
define_indexed_local_base_push_load!(op_f64_load_indexed_local_base, "f64.load", 8);
define_indexed_local_base_scalar_load!(
    op_i32_load8_u_indexed_local_base,
    "i32.load8_u",
    local_read_u8_at,
    push_u32_fast,
    u32::from
);
define_indexed_local_base_scalar_load!(
    op_i32_load8_s_indexed_local_base,
    "i32.load8_s",
    local_read_i8_at,
    push_i32_fast,
    i32::from
);
define_indexed_local_base_scalar_load!(
    op_i32_load16_s_indexed_local_base,
    "i32.load16_s",
    local_read_i16_at,
    push_i32_fast,
    i32::from
);
define_indexed_local_base_scalar_load!(
    op_i32_load16_u_indexed_local_base,
    "i32.load16_u",
    local_read_u16_at,
    push_u32_fast,
    u32::from
);
define_indexed_local_base_scalar_load!(
    op_i64_load8_s_indexed_local_base,
    "i64.load8_s",
    local_read_i8_at,
    push_i64_fast,
    i64::from
);
define_indexed_local_base_scalar_load!(
    op_i64_load8_u_indexed_local_base,
    "i64.load8_u",
    local_read_u8_at,
    push_u64_fast,
    u64::from
);
define_indexed_local_base_scalar_load!(
    op_i64_load16_s_indexed_local_base,
    "i64.load16_s",
    local_read_i16_at,
    push_i64_fast,
    i64::from
);
define_indexed_local_base_scalar_load!(
    op_i64_load16_u_indexed_local_base,
    "i64.load16_u",
    local_read_u16_at,
    push_u64_fast,
    u64::from
);
define_indexed_local_base_scalar_load!(
    op_i64_load32_s_indexed_local_base,
    "i64.load32_s",
    local_read_i32_at,
    push_i64_fast,
    i64::from
);
define_indexed_local_base_scalar_load!(
    op_i64_load32_u_indexed_local_base,
    "i64.load32_u",
    local_read_u32_at,
    push_u64_fast,
    u64::from
);
define_indexed_local_base_store_alias!(op_i32_store_indexed_local_base, |ctx| {
    StoreBytes::Write4(ctx.stack.pop_u8_array::<4>())
});
define_indexed_local_base_store_alias!(op_i64_store_indexed_local_base, |ctx| {
    StoreBytes::Write8(ctx.stack.pop_u8_array::<8>())
});
define_indexed_local_base_store_alias!(op_f32_store_indexed_local_base, |ctx| {
    StoreBytes::Write4(ctx.stack.pop_u8_array::<4>())
});
define_indexed_local_base_store_alias!(op_f64_store_indexed_local_base, |ctx| {
    StoreBytes::Write8(ctx.stack.pop_u8_array::<8>())
});
define_indexed_local_base_store_alias!(op_i32_store8_indexed_local_base, |ctx| {
    StoreBytes::Write1(truncate_u32_to_u8_bytes(ctx.stack.pop_u32()))
});
define_indexed_local_base_store_alias!(op_i32_store16_indexed_local_base, |ctx| {
    StoreBytes::Write2(truncate_u32_to_u16_bytes(ctx.stack.pop_u32()))
});
define_indexed_local_base_store_alias!(op_i64_store8_indexed_local_base, |ctx| {
    StoreBytes::Write1(truncate_u64_to_u8_bytes(ctx.stack.pop_u64()))
});
define_indexed_local_base_store_alias!(op_i64_store16_indexed_local_base, |ctx| {
    StoreBytes::Write2(truncate_u64_to_u16_bytes(ctx.stack.pop_u64()))
});
define_indexed_local_base_store_alias!(op_i64_store32_indexed_local_base, |ctx| {
    StoreBytes::Write4(truncate_u64_to_u32_bytes(ctx.stack.pop_u64()))
});

define_shared_local_base_push_load!(op_i32_load_shared_local_base, "i32.load", 4);
define_shared_local_base_push_load!(op_i64_load_shared_local_base, "i64.load", 8);
define_shared_local_base_push_load!(op_f32_load_shared_local_base, "f32.load", 4);
define_shared_local_base_push_load!(op_f64_load_shared_local_base, "f64.load", 8);
define_shared_local_base_scalar_load!(
    op_i32_load8_u_shared_local_base,
    "i32.load8_u",
    shared_read_u8_at,
    push_u32_fast,
    u32::from
);
define_shared_local_base_scalar_load!(
    op_i32_load8_s_shared_local_base,
    "i32.load8_s",
    shared_read_i8_at,
    push_i32_fast,
    i32::from
);
define_shared_local_base_scalar_load!(
    op_i32_load16_s_shared_local_base,
    "i32.load16_s",
    shared_read_i16_at,
    push_i32_fast,
    i32::from
);
define_shared_local_base_scalar_load!(
    op_i32_load16_u_shared_local_base,
    "i32.load16_u",
    shared_read_u16_at,
    push_u32_fast,
    u32::from
);
define_shared_local_base_scalar_load!(
    op_i64_load8_s_shared_local_base,
    "i64.load8_s",
    shared_read_i8_at,
    push_i64_fast,
    i64::from
);
define_shared_local_base_scalar_load!(
    op_i64_load8_u_shared_local_base,
    "i64.load8_u",
    shared_read_u8_at,
    push_u64_fast,
    u64::from
);
define_shared_local_base_scalar_load!(
    op_i64_load16_s_shared_local_base,
    "i64.load16_s",
    shared_read_i16_at,
    push_i64_fast,
    i64::from
);
define_shared_local_base_scalar_load!(
    op_i64_load16_u_shared_local_base,
    "i64.load16_u",
    shared_read_u16_at,
    push_u64_fast,
    u64::from
);
define_shared_local_base_scalar_load!(
    op_i64_load32_s_shared_local_base,
    "i64.load32_s",
    shared_read_i32_at,
    push_i64_fast,
    i64::from
);
define_shared_local_base_scalar_load!(
    op_i64_load32_u_shared_local_base,
    "i64.load32_u",
    shared_read_u32_at,
    push_u64_fast,
    u64::from
);
define_shared_local_base_store_alias!(op_i32_store_shared_local_base, |ctx| {
    StoreBytes::Write4(ctx.stack.pop_u8_array::<4>())
});
define_shared_local_base_store_alias!(op_i64_store_shared_local_base, |ctx| {
    StoreBytes::Write8(ctx.stack.pop_u8_array::<8>())
});
define_shared_local_base_store_alias!(op_f32_store_shared_local_base, |ctx| {
    StoreBytes::Write4(ctx.stack.pop_u8_array::<4>())
});
define_shared_local_base_store_alias!(op_f64_store_shared_local_base, |ctx| {
    StoreBytes::Write8(ctx.stack.pop_u8_array::<8>())
});
define_shared_local_base_store_alias!(op_i32_store8_shared_local_base, |ctx| {
    StoreBytes::Write1(truncate_u32_to_u8_bytes(ctx.stack.pop_u32()))
});
define_shared_local_base_store_alias!(op_i32_store16_shared_local_base, |ctx| {
    StoreBytes::Write2(truncate_u32_to_u16_bytes(ctx.stack.pop_u32()))
});
define_shared_local_base_store_alias!(op_i64_store8_shared_local_base, |ctx| {
    StoreBytes::Write1(truncate_u64_to_u8_bytes(ctx.stack.pop_u64()))
});
define_shared_local_base_store_alias!(op_i64_store16_shared_local_base, |ctx| {
    StoreBytes::Write2(truncate_u64_to_u16_bytes(ctx.stack.pop_u64()))
});
define_shared_local_base_store_alias!(op_i64_store32_shared_local_base, |ctx| {
    StoreBytes::Write4(truncate_u64_to_u32_bytes(ctx.stack.pop_u64()))
});

define_indexed_shared_local_base_push_load!(op_i32_load_indexed_shared_local_base, "i32.load", 4);
define_indexed_shared_local_base_push_load!(op_i64_load_indexed_shared_local_base, "i64.load", 8);
define_indexed_shared_local_base_push_load!(op_f32_load_indexed_shared_local_base, "f32.load", 4);
define_indexed_shared_local_base_push_load!(op_f64_load_indexed_shared_local_base, "f64.load", 8);
define_indexed_shared_local_base_scalar_load!(
    op_i32_load8_u_indexed_shared_local_base,
    "i32.load8_u",
    shared_read_u8_at,
    push_u32_fast,
    u32::from
);
define_indexed_shared_local_base_scalar_load!(
    op_i32_load8_s_indexed_shared_local_base,
    "i32.load8_s",
    shared_read_i8_at,
    push_i32_fast,
    i32::from
);
define_indexed_shared_local_base_scalar_load!(
    op_i32_load16_s_indexed_shared_local_base,
    "i32.load16_s",
    shared_read_i16_at,
    push_i32_fast,
    i32::from
);
define_indexed_shared_local_base_scalar_load!(
    op_i32_load16_u_indexed_shared_local_base,
    "i32.load16_u",
    shared_read_u16_at,
    push_u32_fast,
    u32::from
);
define_indexed_shared_local_base_scalar_load!(
    op_i64_load8_s_indexed_shared_local_base,
    "i64.load8_s",
    shared_read_i8_at,
    push_i64_fast,
    i64::from
);
define_indexed_shared_local_base_scalar_load!(
    op_i64_load8_u_indexed_shared_local_base,
    "i64.load8_u",
    shared_read_u8_at,
    push_u64_fast,
    u64::from
);
define_indexed_shared_local_base_scalar_load!(
    op_i64_load16_s_indexed_shared_local_base,
    "i64.load16_s",
    shared_read_i16_at,
    push_i64_fast,
    i64::from
);
define_indexed_shared_local_base_scalar_load!(
    op_i64_load16_u_indexed_shared_local_base,
    "i64.load16_u",
    shared_read_u16_at,
    push_u64_fast,
    u64::from
);
define_indexed_shared_local_base_scalar_load!(
    op_i64_load32_s_indexed_shared_local_base,
    "i64.load32_s",
    shared_read_i32_at,
    push_i64_fast,
    i64::from
);
define_indexed_shared_local_base_scalar_load!(
    op_i64_load32_u_indexed_shared_local_base,
    "i64.load32_u",
    shared_read_u32_at,
    push_u64_fast,
    u64::from
);
define_indexed_shared_local_base_store_alias!(op_i32_store_indexed_shared_local_base, |ctx| {
    StoreBytes::Write4(ctx.stack.pop_u8_array::<4>())
});
define_indexed_shared_local_base_store_alias!(op_i64_store_indexed_shared_local_base, |ctx| {
    StoreBytes::Write8(ctx.stack.pop_u8_array::<8>())
});
define_indexed_shared_local_base_store_alias!(op_f32_store_indexed_shared_local_base, |ctx| {
    StoreBytes::Write4(ctx.stack.pop_u8_array::<4>())
});
define_indexed_shared_local_base_store_alias!(op_f64_store_indexed_shared_local_base, |ctx| {
    StoreBytes::Write8(ctx.stack.pop_u8_array::<8>())
});
define_indexed_shared_local_base_store_alias!(op_i32_store8_indexed_shared_local_base, |ctx| {
    StoreBytes::Write1(truncate_u32_to_u8_bytes(ctx.stack.pop_u32()))
});
define_indexed_shared_local_base_store_alias!(op_i32_store16_indexed_shared_local_base, |ctx| {
    StoreBytes::Write2(truncate_u32_to_u16_bytes(ctx.stack.pop_u32()))
});
define_indexed_shared_local_base_store_alias!(op_i64_store8_indexed_shared_local_base, |ctx| {
    StoreBytes::Write1(truncate_u64_to_u8_bytes(ctx.stack.pop_u64()))
});
define_indexed_shared_local_base_store_alias!(op_i64_store16_indexed_shared_local_base, |ctx| {
    StoreBytes::Write2(truncate_u64_to_u16_bytes(ctx.stack.pop_u64()))
});
define_indexed_shared_local_base_store_alias!(op_i64_store32_indexed_shared_local_base, |ctx| {
    StoreBytes::Write4(truncate_u64_to_u32_bytes(ctx.stack.pop_u64()))
});

define_local_scaled_index_push_load!(op_i32_load_local_scaled_index, "i32.load", 4);
define_local_scaled_index_push_load!(op_i64_load_local_scaled_index, "i64.load", 8);
define_local_scaled_index_push_load!(op_f32_load_local_scaled_index, "f32.load", 4);
define_local_scaled_index_push_load!(op_f64_load_local_scaled_index, "f64.load", 8);
define_local_scaled_index_scalar_load!(
    op_i32_load8_u_local_scaled_index,
    "i32.load8_u",
    read_u8_at,
    push_u32_fast,
    u32::from
);
define_local_scaled_index_scalar_load!(
    op_i32_load8_s_local_scaled_index,
    "i32.load8_s",
    read_i8_at,
    push_i32_fast,
    i32::from
);
define_local_scaled_index_scalar_load!(
    op_i32_load16_s_local_scaled_index,
    "i32.load16_s",
    read_i16_at,
    push_i32_fast,
    i32::from
);
define_local_scaled_index_scalar_load!(
    op_i32_load16_u_local_scaled_index,
    "i32.load16_u",
    read_u16_at,
    push_u32_fast,
    u32::from
);
define_local_scaled_index_scalar_load!(
    op_i64_load8_s_local_scaled_index,
    "i64.load8_s",
    read_i8_at,
    push_i64_fast,
    i64::from
);
define_local_scaled_index_scalar_load!(
    op_i64_load8_u_local_scaled_index,
    "i64.load8_u",
    read_u8_at,
    push_u64_fast,
    u64::from
);
define_local_scaled_index_scalar_load!(
    op_i64_load16_s_local_scaled_index,
    "i64.load16_s",
    read_i16_at,
    push_i64_fast,
    i64::from
);
define_local_scaled_index_scalar_load!(
    op_i64_load16_u_local_scaled_index,
    "i64.load16_u",
    read_u16_at,
    push_u64_fast,
    u64::from
);
define_local_scaled_index_scalar_load!(
    op_i64_load32_s_local_scaled_index,
    "i64.load32_s",
    read_i32_at,
    push_i64_fast,
    i64::from
);
define_local_scaled_index_scalar_load!(
    op_i64_load32_u_local_scaled_index,
    "i64.load32_u",
    read_u32_at,
    push_u64_fast,
    u64::from
);
define_local_scaled_index_store_alias!(op_i32_store_local_scaled_index, |ctx| {
    StoreBytes::Write4(ctx.stack.pop_u8_array::<4>())
});
define_local_scaled_index_store_alias!(op_i64_store_local_scaled_index, |ctx| {
    StoreBytes::Write8(ctx.stack.pop_u8_array::<8>())
});
define_local_scaled_index_store_alias!(op_f32_store_local_scaled_index, |ctx| {
    StoreBytes::Write4(ctx.stack.pop_u8_array::<4>())
});
define_local_scaled_index_store_alias!(op_f64_store_local_scaled_index, |ctx| {
    StoreBytes::Write8(ctx.stack.pop_u8_array::<8>())
});
define_local_scaled_index_store_alias!(op_i32_store8_local_scaled_index, |ctx| {
    StoreBytes::Write1(truncate_u32_to_u8_bytes(ctx.stack.pop_u32()))
});
define_local_scaled_index_store_alias!(op_i32_store16_local_scaled_index, |ctx| {
    StoreBytes::Write2(truncate_u32_to_u16_bytes(ctx.stack.pop_u32()))
});
define_local_scaled_index_store_alias!(op_i64_store8_local_scaled_index, |ctx| {
    StoreBytes::Write1(truncate_u64_to_u8_bytes(ctx.stack.pop_u64()))
});
define_local_scaled_index_store_alias!(op_i64_store16_local_scaled_index, |ctx| {
    StoreBytes::Write2(truncate_u64_to_u16_bytes(ctx.stack.pop_u64()))
});
define_local_scaled_index_store_alias!(op_i64_store32_local_scaled_index, |ctx| {
    StoreBytes::Write4(truncate_u64_to_u32_bytes(ctx.stack.pop_u64()))
});

define_indexed_local_scaled_index_push_load!(op_i32_load_indexed_local_scaled_index, "i32.load", 4);
define_indexed_local_scaled_index_push_load!(op_i64_load_indexed_local_scaled_index, "i64.load", 8);
define_indexed_local_scaled_index_push_load!(op_f32_load_indexed_local_scaled_index, "f32.load", 4);
define_indexed_local_scaled_index_push_load!(op_f64_load_indexed_local_scaled_index, "f64.load", 8);
define_indexed_local_scaled_index_scalar_load!(
    op_i32_load8_u_indexed_local_scaled_index,
    "i32.load8_u",
    local_read_u8_at,
    push_u32_fast,
    u32::from
);
define_indexed_local_scaled_index_scalar_load!(
    op_i32_load8_s_indexed_local_scaled_index,
    "i32.load8_s",
    local_read_i8_at,
    push_i32_fast,
    i32::from
);
define_indexed_local_scaled_index_scalar_load!(
    op_i32_load16_s_indexed_local_scaled_index,
    "i32.load16_s",
    local_read_i16_at,
    push_i32_fast,
    i32::from
);
define_indexed_local_scaled_index_scalar_load!(
    op_i32_load16_u_indexed_local_scaled_index,
    "i32.load16_u",
    local_read_u16_at,
    push_u32_fast,
    u32::from
);
define_indexed_local_scaled_index_scalar_load!(
    op_i64_load8_s_indexed_local_scaled_index,
    "i64.load8_s",
    local_read_i8_at,
    push_i64_fast,
    i64::from
);
define_indexed_local_scaled_index_scalar_load!(
    op_i64_load8_u_indexed_local_scaled_index,
    "i64.load8_u",
    local_read_u8_at,
    push_u64_fast,
    u64::from
);
define_indexed_local_scaled_index_scalar_load!(
    op_i64_load16_s_indexed_local_scaled_index,
    "i64.load16_s",
    local_read_i16_at,
    push_i64_fast,
    i64::from
);
define_indexed_local_scaled_index_scalar_load!(
    op_i64_load16_u_indexed_local_scaled_index,
    "i64.load16_u",
    local_read_u16_at,
    push_u64_fast,
    u64::from
);
define_indexed_local_scaled_index_scalar_load!(
    op_i64_load32_s_indexed_local_scaled_index,
    "i64.load32_s",
    local_read_i32_at,
    push_i64_fast,
    i64::from
);
define_indexed_local_scaled_index_scalar_load!(
    op_i64_load32_u_indexed_local_scaled_index,
    "i64.load32_u",
    local_read_u32_at,
    push_u64_fast,
    u64::from
);
define_indexed_local_scaled_index_store_alias!(op_i32_store_indexed_local_scaled_index, |ctx| {
    StoreBytes::Write4(ctx.stack.pop_u8_array::<4>())
});
define_indexed_local_scaled_index_store_alias!(op_i64_store_indexed_local_scaled_index, |ctx| {
    StoreBytes::Write8(ctx.stack.pop_u8_array::<8>())
});
define_indexed_local_scaled_index_store_alias!(op_f32_store_indexed_local_scaled_index, |ctx| {
    StoreBytes::Write4(ctx.stack.pop_u8_array::<4>())
});
define_indexed_local_scaled_index_store_alias!(op_f64_store_indexed_local_scaled_index, |ctx| {
    StoreBytes::Write8(ctx.stack.pop_u8_array::<8>())
});
define_indexed_local_scaled_index_store_alias!(op_i32_store8_indexed_local_scaled_index, |ctx| {
    StoreBytes::Write1(truncate_u32_to_u8_bytes(ctx.stack.pop_u32()))
});
define_indexed_local_scaled_index_store_alias!(op_i32_store16_indexed_local_scaled_index, |ctx| {
    StoreBytes::Write2(truncate_u32_to_u16_bytes(ctx.stack.pop_u32()))
});
define_indexed_local_scaled_index_store_alias!(op_i64_store8_indexed_local_scaled_index, |ctx| {
    StoreBytes::Write1(truncate_u64_to_u8_bytes(ctx.stack.pop_u64()))
});
define_indexed_local_scaled_index_store_alias!(op_i64_store16_indexed_local_scaled_index, |ctx| {
    StoreBytes::Write2(truncate_u64_to_u16_bytes(ctx.stack.pop_u64()))
});
define_indexed_local_scaled_index_store_alias!(op_i64_store32_indexed_local_scaled_index, |ctx| {
    StoreBytes::Write4(truncate_u64_to_u32_bytes(ctx.stack.pop_u64()))
});

define_shared_local_scaled_index_push_load!(op_i32_load_shared_local_scaled_index, "i32.load", 4);
define_shared_local_scaled_index_push_load!(op_i64_load_shared_local_scaled_index, "i64.load", 8);
define_shared_local_scaled_index_push_load!(op_f32_load_shared_local_scaled_index, "f32.load", 4);
define_shared_local_scaled_index_push_load!(op_f64_load_shared_local_scaled_index, "f64.load", 8);
define_shared_local_scaled_index_scalar_load!(
    op_i32_load8_u_shared_local_scaled_index,
    "i32.load8_u",
    shared_read_u8_at,
    push_u32_fast,
    u32::from
);
define_shared_local_scaled_index_scalar_load!(
    op_i32_load8_s_shared_local_scaled_index,
    "i32.load8_s",
    shared_read_i8_at,
    push_i32_fast,
    i32::from
);
define_shared_local_scaled_index_scalar_load!(
    op_i32_load16_s_shared_local_scaled_index,
    "i32.load16_s",
    shared_read_i16_at,
    push_i32_fast,
    i32::from
);
define_shared_local_scaled_index_scalar_load!(
    op_i32_load16_u_shared_local_scaled_index,
    "i32.load16_u",
    shared_read_u16_at,
    push_u32_fast,
    u32::from
);
define_shared_local_scaled_index_scalar_load!(
    op_i64_load8_s_shared_local_scaled_index,
    "i64.load8_s",
    shared_read_i8_at,
    push_i64_fast,
    i64::from
);
define_shared_local_scaled_index_scalar_load!(
    op_i64_load8_u_shared_local_scaled_index,
    "i64.load8_u",
    shared_read_u8_at,
    push_u64_fast,
    u64::from
);
define_shared_local_scaled_index_scalar_load!(
    op_i64_load16_s_shared_local_scaled_index,
    "i64.load16_s",
    shared_read_i16_at,
    push_i64_fast,
    i64::from
);
define_shared_local_scaled_index_scalar_load!(
    op_i64_load16_u_shared_local_scaled_index,
    "i64.load16_u",
    shared_read_u16_at,
    push_u64_fast,
    u64::from
);
define_shared_local_scaled_index_scalar_load!(
    op_i64_load32_s_shared_local_scaled_index,
    "i64.load32_s",
    shared_read_i32_at,
    push_i64_fast,
    i64::from
);
define_shared_local_scaled_index_scalar_load!(
    op_i64_load32_u_shared_local_scaled_index,
    "i64.load32_u",
    shared_read_u32_at,
    push_u64_fast,
    u64::from
);
define_shared_local_scaled_index_store_alias!(op_i32_store_shared_local_scaled_index, |ctx| {
    StoreBytes::Write4(ctx.stack.pop_u8_array::<4>())
});
define_shared_local_scaled_index_store_alias!(op_i64_store_shared_local_scaled_index, |ctx| {
    StoreBytes::Write8(ctx.stack.pop_u8_array::<8>())
});
define_shared_local_scaled_index_store_alias!(op_f32_store_shared_local_scaled_index, |ctx| {
    StoreBytes::Write4(ctx.stack.pop_u8_array::<4>())
});
define_shared_local_scaled_index_store_alias!(op_f64_store_shared_local_scaled_index, |ctx| {
    StoreBytes::Write8(ctx.stack.pop_u8_array::<8>())
});
define_shared_local_scaled_index_store_alias!(op_i32_store8_shared_local_scaled_index, |ctx| {
    StoreBytes::Write1(truncate_u32_to_u8_bytes(ctx.stack.pop_u32()))
});
define_shared_local_scaled_index_store_alias!(op_i32_store16_shared_local_scaled_index, |ctx| {
    StoreBytes::Write2(truncate_u32_to_u16_bytes(ctx.stack.pop_u32()))
});
define_shared_local_scaled_index_store_alias!(op_i64_store8_shared_local_scaled_index, |ctx| {
    StoreBytes::Write1(truncate_u64_to_u8_bytes(ctx.stack.pop_u64()))
});
define_shared_local_scaled_index_store_alias!(op_i64_store16_shared_local_scaled_index, |ctx| {
    StoreBytes::Write2(truncate_u64_to_u16_bytes(ctx.stack.pop_u64()))
});
define_shared_local_scaled_index_store_alias!(op_i64_store32_shared_local_scaled_index, |ctx| {
    StoreBytes::Write4(truncate_u64_to_u32_bytes(ctx.stack.pop_u64()))
});

define_indexed_shared_local_scaled_index_push_load!(
    op_i32_load_indexed_shared_local_scaled_index,
    "i32.load",
    4
);
define_indexed_shared_local_scaled_index_push_load!(
    op_i64_load_indexed_shared_local_scaled_index,
    "i64.load",
    8
);
define_indexed_shared_local_scaled_index_push_load!(
    op_f32_load_indexed_shared_local_scaled_index,
    "f32.load",
    4
);
define_indexed_shared_local_scaled_index_push_load!(
    op_f64_load_indexed_shared_local_scaled_index,
    "f64.load",
    8
);
define_indexed_shared_local_scaled_index_scalar_load!(
    op_i32_load8_u_indexed_shared_local_scaled_index,
    "i32.load8_u",
    shared_read_u8_at,
    push_u32_fast,
    u32::from
);
define_indexed_shared_local_scaled_index_scalar_load!(
    op_i32_load8_s_indexed_shared_local_scaled_index,
    "i32.load8_s",
    shared_read_i8_at,
    push_i32_fast,
    i32::from
);
define_indexed_shared_local_scaled_index_scalar_load!(
    op_i32_load16_s_indexed_shared_local_scaled_index,
    "i32.load16_s",
    shared_read_i16_at,
    push_i32_fast,
    i32::from
);
define_indexed_shared_local_scaled_index_scalar_load!(
    op_i32_load16_u_indexed_shared_local_scaled_index,
    "i32.load16_u",
    shared_read_u16_at,
    push_u32_fast,
    u32::from
);
define_indexed_shared_local_scaled_index_scalar_load!(
    op_i64_load8_s_indexed_shared_local_scaled_index,
    "i64.load8_s",
    shared_read_i8_at,
    push_i64_fast,
    i64::from
);
define_indexed_shared_local_scaled_index_scalar_load!(
    op_i64_load8_u_indexed_shared_local_scaled_index,
    "i64.load8_u",
    shared_read_u8_at,
    push_u64_fast,
    u64::from
);
define_indexed_shared_local_scaled_index_scalar_load!(
    op_i64_load16_s_indexed_shared_local_scaled_index,
    "i64.load16_s",
    shared_read_i16_at,
    push_i64_fast,
    i64::from
);
define_indexed_shared_local_scaled_index_scalar_load!(
    op_i64_load16_u_indexed_shared_local_scaled_index,
    "i64.load16_u",
    shared_read_u16_at,
    push_u64_fast,
    u64::from
);
define_indexed_shared_local_scaled_index_scalar_load!(
    op_i64_load32_s_indexed_shared_local_scaled_index,
    "i64.load32_s",
    shared_read_i32_at,
    push_i64_fast,
    i64::from
);
define_indexed_shared_local_scaled_index_scalar_load!(
    op_i64_load32_u_indexed_shared_local_scaled_index,
    "i64.load32_u",
    shared_read_u32_at,
    push_u64_fast,
    u64::from
);
define_indexed_shared_local_scaled_index_store_alias!(
    op_i32_store_indexed_shared_local_scaled_index,
    |ctx| { StoreBytes::Write4(ctx.stack.pop_u8_array::<4>()) }
);
define_indexed_shared_local_scaled_index_store_alias!(
    op_i64_store_indexed_shared_local_scaled_index,
    |ctx| { StoreBytes::Write8(ctx.stack.pop_u8_array::<8>()) }
);
define_indexed_shared_local_scaled_index_store_alias!(
    op_f32_store_indexed_shared_local_scaled_index,
    |ctx| { StoreBytes::Write4(ctx.stack.pop_u8_array::<4>()) }
);
define_indexed_shared_local_scaled_index_store_alias!(
    op_f64_store_indexed_shared_local_scaled_index,
    |ctx| { StoreBytes::Write8(ctx.stack.pop_u8_array::<8>()) }
);
define_indexed_shared_local_scaled_index_store_alias!(
    op_i32_store8_indexed_shared_local_scaled_index,
    |ctx| { StoreBytes::Write1(truncate_u32_to_u8_bytes(ctx.stack.pop_u32())) }
);
define_indexed_shared_local_scaled_index_store_alias!(
    op_i32_store16_indexed_shared_local_scaled_index,
    |ctx| { StoreBytes::Write2(truncate_u32_to_u16_bytes(ctx.stack.pop_u32())) }
);
define_indexed_shared_local_scaled_index_store_alias!(
    op_i64_store8_indexed_shared_local_scaled_index,
    |ctx| { StoreBytes::Write1(truncate_u64_to_u8_bytes(ctx.stack.pop_u64())) }
);
define_indexed_shared_local_scaled_index_store_alias!(
    op_i64_store16_indexed_shared_local_scaled_index,
    |ctx| { StoreBytes::Write2(truncate_u64_to_u16_bytes(ctx.stack.pop_u64())) }
);
define_indexed_shared_local_scaled_index_store_alias!(
    op_i64_store32_indexed_shared_local_scaled_index,
    |ctx| { StoreBytes::Write4(truncate_u64_to_u32_bytes(ctx.stack.pop_u64())) }
);

/// WebAssembly `memory.size` on shared default memory.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[] -> [i32]`.
/// Traps: traps when no default memory exists.
/// Notes: Uses the shared-memory specialized fast path selected by the parser and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose default memory is shared.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_mem_size_shared(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let page_size = ctx
        .gc
        .shared_memory(ctx.default_shared_memory_id_unchecked())
        .page_size();
    vm_try!(ctx.stack.push_u32(page_size));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `memory.grow` on shared default memory.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[i32] -> [i32]`.
/// Traps: traps when no default memory exists; otherwise returns `-1` on growth failure.
/// Notes: Uses the shared-memory specialized fast path selected by the parser and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose default memory is shared.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_mem_grow_shared(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let page_size_delta = ctx.stack.pop_u32();
    let result = vm_try!(ctx
        .gc
        .shared_grow_memory(ctx.default_shared_memory_id_unchecked(), page_size_delta,));
    vm_try!(ctx.stack.push_i32(result));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `memory.size` on indexed local memory.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/multi-memory/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/multi-memory/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/multi-memory/core/exec/instructions.html
///
/// Stack effect: `[] -> [i32]`.
/// Traps: traps when the indexed memory does not exist.
/// Notes: Uses the typed indexed local-memory fast path and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose indexed memory operand is in-bounds and local.
pub unsafe fn op_mem_size_indexed_local(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let memidx = (*tail_code).operand.u32;
    let page_size = ctx
        .gc
        .local_memory(ctx.local_memory_id_at_unchecked(memidx))
        .page_size();
    vm_try!(ctx.stack.push_u32(page_size));
    call_next(tail_code, 1, ctx)
}

/// WebAssembly `memory.size` on indexed shared memory.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/multi-memory/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/multi-memory/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/multi-memory/core/exec/instructions.html
///
/// Stack effect: `[] -> [i32]`.
/// Traps: traps when the indexed memory does not exist.
/// Notes: Uses the typed indexed shared-memory fast path and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose indexed memory operand is in-bounds and shared.
pub unsafe fn op_mem_size_indexed_shared(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let memidx = (*tail_code).operand.u32;
    let page_size = ctx
        .gc
        .shared_memory(ctx.shared_memory_id_at_unchecked(memidx))
        .page_size();
    vm_try!(ctx.stack.push_u32(page_size));
    call_next(tail_code, 1, ctx)
}

/// WebAssembly `memory.grow` on indexed local memory.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/multi-memory/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/multi-memory/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/multi-memory/core/exec/instructions.html
///
/// Stack effect: `[i32] -> [i32]`.
/// Traps: traps when the indexed memory does not exist; otherwise returns `-1` on growth failure.
/// Notes: Uses the typed indexed local-memory fast path and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose indexed memory operand is in-bounds and local.
pub unsafe fn op_mem_grow_indexed_local(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let memidx = (*tail_code).operand.u32;
    let page_size_delta = ctx.stack.pop_u32();
    let result = vm_try!(ctx
        .gc
        .local_grow_memory(ctx.local_memory_id_at_unchecked(memidx), page_size_delta));
    vm_try!(ctx.stack.push_i32(result));
    call_next(tail_code, 1, ctx)
}

/// WebAssembly `memory.grow` on indexed shared memory.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/multi-memory/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/multi-memory/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/multi-memory/core/exec/instructions.html
///
/// Stack effect: `[i32] -> [i32]`.
/// Traps: traps when the indexed memory does not exist; otherwise returns `-1` on growth failure.
/// Notes: Uses the typed indexed shared-memory fast path and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose indexed memory operand is in-bounds and shared.
pub unsafe fn op_mem_grow_indexed_shared(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let memidx = (*tail_code).operand.u32;
    let page_size_delta = ctx.stack.pop_u32();
    let result = vm_try!(ctx
        .gc
        .shared_grow_memory(ctx.shared_memory_id_at_unchecked(memidx), page_size_delta));
    vm_try!(ctx.stack.push_i32(result));
    call_next(tail_code, 1, ctx)
}

pub(crate) use op_f32_load as op_f32_load_local;
pub(crate) use op_f32_store as op_f32_store_local;
pub(crate) use op_f64_load as op_f64_load_local;
pub(crate) use op_f64_store as op_f64_store_local;
pub(crate) use op_i32_load as op_i32_load_local;
pub(crate) use op_i32_load16_s as op_i32_load16_s_local;
pub(crate) use op_i32_load16_u as op_i32_load16_u_local;
pub(crate) use op_i32_load8_s as op_i32_load8_s_local;
pub(crate) use op_i32_load8_u as op_i32_load8_u_local;
pub(crate) use op_i32_store as op_i32_store_local;
pub(crate) use op_i32_store16 as op_i32_store16_local;
pub(crate) use op_i32_store8 as op_i32_store8_local;
pub(crate) use op_i64_load as op_i64_load_local;
pub(crate) use op_i64_load16_s as op_i64_load16_s_local;
pub(crate) use op_i64_load16_u as op_i64_load16_u_local;
pub(crate) use op_i64_load32_s as op_i64_load32_s_local;
pub(crate) use op_i64_load32_u as op_i64_load32_u_local;
pub(crate) use op_i64_load8_s as op_i64_load8_s_local;
pub(crate) use op_i64_load8_u as op_i64_load8_u_local;
pub(crate) use op_i64_store as op_i64_store_local;
pub(crate) use op_i64_store16 as op_i64_store16_local;
pub(crate) use op_i64_store32 as op_i64_store32_local;
pub(crate) use op_i64_store8 as op_i64_store8_local;
pub(crate) use op_mem_grow as op_mem_grow_local;
pub(crate) use op_mem_size as op_mem_size_local;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        common::{
            stack::{CachedMemoryKind, CallFrameCache},
            store::{InstanceId, MemoryHandle},
            ExecuteContext, LocalMemoryObject, LocalReference, Memory, ObjectRef, Operand, Store,
            StoreInner,
        },
        runtime::{memory_effect::PendingOp, scheduler::EffectSupplier},
    };
    #[cfg(feature = "vm-profile")]
    use crate::{
        common::{InstanceHandle, Registry, ResultValue, WasmValue},
        IoReadBinaryReader, WasmParser,
    };
    use std::collections::VecDeque;

    fn frame(kind: CachedMemoryKind, raw: u32) -> CallFrameCache {
        CallFrameCache {
            code_addr: ObjectRef(0),
            code_base: std::ptr::null(),
            instance: InstanceId::from_index(0),
            memory0_kind: kind,
            memory0_raw: raw,
        }
    }

    fn test_context<'a>(
        stack: &'a mut Stack,
        store: &'a Store,
        gc: &'a mut StoreInner,
        pending_effects: &'a mut u32,
        queue: &'a mut VecDeque<PendingOp>,
    ) -> ExecuteContext<'a> {
        let MemoryHandle::Local(memory_id) =
            gc.alloc_local_memory(LocalMemoryObject::new(1, 1).expect("test local memory"))
        else {
            unreachable!("test local memory handle must be local");
        };
        let local_reference = LocalReference {
            local_top: 0,
            local_size: 0,
        };
        let local_base_ptr = unsafe { stack.local_area_mut_ptr(&local_reference) };
        ExecuteContext {
            stack,
            local_reference,
            local_base_ptr,
            default_local_memory_ptr: gc.local_memory_mut(memory_id).memory_mut() as *mut Memory,
            current_frame: frame(CachedMemoryKind::Local, memory_id.raw()),
            store,
            gc,
            effect: EffectSupplier::from_parts(1, pending_effects, queue),
            cont: std::ptr::null(),
            task_id: 1,
        }
    }

    #[cfg(feature = "vm-profile")]
    async fn instantiate_wat(wat: &str, store: &Store, registry: &Registry) -> InstanceHandle {
        let bytes = wat::parse_str(wat).expect("wat must parse");
        let mut reader = IoReadBinaryReader::from(bytes.as_slice());
        let mut parser = WasmParser::new(&mut reader);
        let module = parser.parse_module().expect("module must parse");
        match crate::instantiate(module, store, registry).await {
            VMResult::Success(instance) => instance,
            other => panic!("module must instantiate, got {other:?}"),
        }
    }

    #[test]
    fn load_start_helpers_match_offset_and_index_contracts() {
        let store = Store::new();
        let mut gc = StoreInner::new();
        let mut pending_effects = 0;
        let mut effects = VecDeque::new();
        let mut stack = Stack::new(32);
        stack.push_u32(5).unwrap();

        let program = [
            Instr {
                operand: Operand {
                    memarg: MemArg {
                        align: 2,
                        offset: 7,
                    },
                },
            },
            Instr {
                operand: Operand { u32: 3 },
            },
        ];
        let mut ctx = test_context(
            &mut stack,
            &store,
            &mut gc,
            &mut pending_effects,
            &mut effects,
        );

        let start = unsafe { load_start(program.as_ptr(), &mut ctx) }.unwrap();
        assert_eq!(start, 12);

        ctx.stack.push_u32(11).unwrap();
        let (indexed_start, memidx) =
            unsafe { load_start_indexed(program.as_ptr(), &mut ctx) }.unwrap();
        assert_eq!(indexed_start, 18);
        assert_eq!(memidx, 3);
    }

    #[test]
    fn load_start_fail_closes_memory_offset_overflow() {
        let store = Store::new();
        let mut gc = StoreInner::new();
        let mut pending_effects = 0;
        let mut effects = VecDeque::new();
        let mut stack = Stack::new(16);
        stack.push_u32(1).unwrap();

        let program = [Instr {
            operand: Operand {
                memarg: MemArg {
                    align: 0,
                    offset: u32::MAX,
                },
            },
        }];
        let mut ctx = test_context(
            &mut stack,
            &store,
            &mut gc,
            &mut pending_effects,
            &mut effects,
        );

        assert!(matches!(
            unsafe { load_start(program.as_ptr(), &mut ctx) },
            VMResult::MemoryIndexOutOfRange
        ));
    }

    #[cfg(feature = "vm-profile")]
    #[tokio::test]
    async fn profiler_prefers_local_base_memory_families_over_generic_path() {
        let store = Store::new();
        let registry = Registry::new();
        let instance = instantiate_wat(
            r#"
            (module
              (memory 1)
              (func (export "run") (param $base i32) (param $remaining i32) (result i32)
                local.get $base
                i32.const 0
                i32.store
                block $done
                  loop $loop
                    local.get $remaining
                    i32.eqz
                    br_if $done

                    local.get $base
                    local.get $base
                    i32.load
                    i32.const 1
                    i32.add
                    i32.store

                    local.get $remaining
                    i32.const 1
                    i32.sub
                    local.set $remaining
                    br $loop
                  end
                end
                local.get $base
                i32.load))
            "#,
            &store,
            &registry,
        )
        .await;

        let _profile = super::super::DispatchProfileTestOverride::enable();
        let result = crate::run_module_function(
            &instance,
            &store,
            "run",
            &ResultValue::new(vec![WasmValue::I32(8), WasmValue::I32(12)]),
        )
        .await;

        match result {
            VMResult::Success(values) => {
                assert_eq!(values, ResultValue::new(vec![WasmValue::I32(12)]));
            }
            other => panic!("profiled local-base memory loop must succeed, got {other:?}"),
        }

        let snapshot = super::super::take_last_dispatch_profile_snapshot_for_test()
            .expect("profile snapshot must be recorded");
        let count = |label: &'static str| {
            snapshot
                .stats
                .iter()
                .find_map(|(candidate, stat)| (*candidate == label).then_some(stat.count))
                .unwrap_or_default()
        };

        let specialized_load = count("op_i32_load_local_base");
        let specialized_store = count("op_i32_store_local_base");
        let generic_load = count("op_i32_load");
        let generic_store = count("op_i32_store");

        assert!(
            specialized_load > 0,
            "local-base load family must appear in dispatch profile: {:?}",
            snapshot.stats
        );
        assert!(
            specialized_store > 0,
            "local-base store family must appear in dispatch profile: {:?}",
            snapshot.stats
        );
        assert!(
            specialized_load > generic_load,
            "specialized load family must dominate generic load path: {:?}",
            snapshot.stats
        );
        assert!(
            specialized_store > generic_store,
            "specialized store family must dominate generic store path: {:?}",
            snapshot.stats
        );
    }
}
