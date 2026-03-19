#![allow(clippy::missing_safety_doc)]

use super::*;

#[inline(always)]
unsafe fn load_start(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<usize> {
    let memarg = (*tail_code).operand.memarg;
    let offset = ctx.stack.pop_u32();
    trace!("memory access: {:?} {}", memarg, offset);
    compute_memory_offset(memarg, offset)
}

pub unsafe fn op_i32_load(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let start = vm_try!(load_start(tail_code, ctx));
    vm_try!(ctx.push_memory_to_stack::<4>(start));
    call_next(tail_code, 1, ctx)
}

pub unsafe fn op_i64_load(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let start = vm_try!(load_start(tail_code, ctx));
    vm_try!(ctx.push_memory_to_stack::<8>(start));
    call_next(tail_code, 1, ctx)
}

pub unsafe fn op_f32_load(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let start = vm_try!(load_start(tail_code, ctx));
    vm_try!(ctx.push_memory_to_stack::<4>(start));
    call_next(tail_code, 1, ctx)
}

pub unsafe fn op_f64_load(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let start = vm_try!(load_start(tail_code, ctx));
    vm_try!(ctx.push_memory_to_stack::<8>(start));
    call_next(tail_code, 1, ctx)
}

pub unsafe fn op_i32_load8_u(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let start = vm_try!(load_start(tail_code, ctx));
    let value = vm_try!(ctx.read_memory_u8(start));
    vm_try!(ctx.stack.push_u32(value as u32));
    call_next(tail_code, 1, ctx)
}

pub unsafe fn op_i32_load8_s(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let start = vm_try!(load_start(tail_code, ctx));
    let value = vm_try!(ctx.read_memory_i8(start));
    vm_try!(ctx.stack.push_i32(value as i32));
    call_next(tail_code, 1, ctx)
}

pub unsafe fn op_i32_load16_s(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let start = vm_try!(load_start(tail_code, ctx));
    let value = vm_try!(ctx.read_memory_i16(start));
    vm_try!(ctx.stack.push_i32(value as i32));
    call_next(tail_code, 1, ctx)
}

pub unsafe fn op_i32_load16_u(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let start = vm_try!(load_start(tail_code, ctx));
    let value = vm_try!(ctx.read_memory_u16(start));
    vm_try!(ctx.stack.push_u32(value as u32));
    call_next(tail_code, 1, ctx)
}

pub unsafe fn op_i64_load8_s(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let start = vm_try!(load_start(tail_code, ctx));
    let value = vm_try!(ctx.read_memory_i8(start));
    vm_try!(ctx.stack.push_i64(value as i64));
    call_next(tail_code, 1, ctx)
}

pub unsafe fn op_i64_load8_u(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let start = vm_try!(load_start(tail_code, ctx));
    let value = vm_try!(ctx.read_memory_u8(start));
    vm_try!(ctx.stack.push_u64(value as u64));
    call_next(tail_code, 1, ctx)
}

pub unsafe fn op_i64_load16_s(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let start = vm_try!(load_start(tail_code, ctx));
    let value = vm_try!(ctx.read_memory_i16(start));
    vm_try!(ctx.stack.push_i64(value as i64));
    call_next(tail_code, 1, ctx)
}

pub unsafe fn op_i64_load16_u(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let start = vm_try!(load_start(tail_code, ctx));
    let value = vm_try!(ctx.read_memory_u16(start));
    vm_try!(ctx.stack.push_u64(value as u64));
    call_next(tail_code, 1, ctx)
}

pub unsafe fn op_i64_load32_s(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let start = vm_try!(load_start(tail_code, ctx));
    let value = vm_try!(ctx.read_memory_i32(start));
    vm_try!(ctx.stack.push_i64(value as i64));
    call_next(tail_code, 1, ctx)
}

pub unsafe fn op_i64_load32_u(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let start = vm_try!(load_start(tail_code, ctx));
    let value = vm_try!(ctx.read_memory_u32(start));
    vm_try!(ctx.stack.push_u64(value as u64));
    call_next(tail_code, 1, ctx)
}

pub unsafe fn op_i32_store(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    store_internal(tail_code, ctx, |ctx| {
        StoreBytes::Write4(ctx.stack.pop_u8_array::<4>())
    })
}

pub unsafe fn op_i64_store(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    store_internal(tail_code, ctx, |ctx| {
        StoreBytes::Write8(ctx.stack.pop_u8_array::<8>())
    })
}

pub unsafe fn op_f32_store(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    store_internal(tail_code, ctx, |ctx| {
        StoreBytes::Write4(ctx.stack.pop_u8_array::<4>())
    })
}

pub unsafe fn op_f64_store(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    store_internal(tail_code, ctx, |ctx| {
        StoreBytes::Write8(ctx.stack.pop_u8_array::<8>())
    })
}

pub unsafe fn op_i32_store8(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    store_internal(tail_code, ctx, |ctx| {
        StoreBytes::Write1([ctx.stack.pop_u32().to_le_bytes()[0]])
    })
}

pub unsafe fn op_i32_store16(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    store_internal(tail_code, ctx, |ctx| {
        let value = ctx.stack.pop_u32().to_le_bytes();
        StoreBytes::Write2([value[0], value[1]])
    })
}

pub unsafe fn op_i64_store8(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    store_internal(tail_code, ctx, |ctx| {
        let value = ctx.stack.pop_u64().to_le_bytes();
        StoreBytes::Write1([value[0]])
    })
}

pub unsafe fn op_i64_store16(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    store_internal(tail_code, ctx, |ctx| {
        let value = ctx.stack.pop_u64().to_le_bytes();
        StoreBytes::Write2([value[0], value[1]])
    })
}

pub unsafe fn op_i64_store32(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    store_internal(tail_code, ctx, |ctx| {
        let value = ctx.stack.pop_u64().to_le_bytes();
        StoreBytes::Write4([value[0], value[1], value[2], value[3]])
    })
}

pub unsafe fn op_mem_size(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let page_size = if let Some(page_size) = ctx.memory_page_size() {
        page_size
    } else {
        return VMResult::MemoryIndexOutOfRange;
    };
    vm_try!(ctx.stack.push_u32(page_size));
    call_next(tail_code, 0, ctx)
}

pub unsafe fn op_mem_grow(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let page_size_delta = ctx.stack.pop_u32();
    let result = vm_try!(ctx.grow_memory(page_size_delta));
    vm_try!(ctx.stack.push_i32(result));
    call_next(tail_code, 0, ctx)
}
