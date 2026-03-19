use super::*;
use vstd::prelude::*;

verus! {

#[inline(always)]
fn widen_u8_to_u32(value: u8) -> (result: u32)
    ensures
        result == value as u32,
{
    value as u32
}

#[inline(always)]
fn widen_i8_to_i32(value: i8) -> (result: i32)
    ensures
        result == value as i32,
{
    value as i32
}

#[inline(always)]
fn widen_u16_to_u32(value: u16) -> (result: u32)
    ensures
        result == value as u32,
{
    value as u32
}

#[inline(always)]
fn widen_i16_to_i32(value: i16) -> (result: i32)
    ensures
        result == value as i32,
{
    value as i32
}

#[inline(always)]
fn widen_u8_to_u64(value: u8) -> (result: u64)
    ensures
        result == value as u64,
{
    value as u64
}

#[inline(always)]
fn widen_i8_to_i64(value: i8) -> (result: i64)
    ensures
        result == value as i64,
{
    value as i64
}

#[inline(always)]
fn widen_u16_to_u64(value: u16) -> (result: u64)
    ensures
        result == value as u64,
{
    value as u64
}

#[inline(always)]
fn widen_i16_to_i64(value: i16) -> (result: i64)
    ensures
        result == value as i64,
{
    value as i64
}

#[inline(always)]
fn widen_u32_to_u64(value: u32) -> (result: u64)
    ensures
        result == value as u64,
{
    value as u64
}

#[inline(always)]
fn widen_i32_to_i64(value: i32) -> (result: i64)
    ensures
        result == value as i64,
{
    value as i64
}

#[inline(always)]
fn truncate_u32_to_u8_bytes(value: u32) -> (result: [u8; 1])
    ensures
        result@.len() == 1,
        result@[0] == (value & 0xff) as u8,
{
    [(value & 0xff) as u8]
}

#[inline(always)]
fn truncate_u32_to_u16_bytes(value: u32) -> (result: [u8; 2])
    ensures
        result@.len() == 2,
        result@[0] == (value & 0xff) as u8,
        result@[1] == ((value >> 8) & 0xff) as u8,
{
    [(value & 0xff) as u8, ((value >> 8) & 0xff) as u8]
}

#[inline(always)]
fn truncate_u64_to_u8_bytes(value: u64) -> (result: [u8; 1])
    ensures
        result@.len() == 1,
        result@[0] == (value & 0xff) as u8,
{
    [(value & 0xff) as u8]
}

#[inline(always)]
fn truncate_u64_to_u16_bytes(value: u64) -> (result: [u8; 2])
    ensures
        result@.len() == 2,
        result@[0] == (value & 0xff) as u8,
        result@[1] == ((value >> 8) & 0xff) as u8,
{
    [(value & 0xff) as u8, ((value >> 8) & 0xff) as u8]
}

#[inline(always)]
fn truncate_u64_to_u32_bytes(value: u64) -> (result: [u8; 4])
    ensures
        result@.len() == 4,
        result@[0] == (value & 0xff) as u8,
        result@[1] == ((value >> 8) & 0xff) as u8,
        result@[2] == ((value >> 16) & 0xff) as u8,
        result@[3] == ((value >> 24) & 0xff) as u8,
{
    [
        (value & 0xff) as u8,
        ((value >> 8) & 0xff) as u8,
        ((value >> 16) & 0xff) as u8,
        ((value >> 24) & 0xff) as u8,
    ]
}

} // verus!

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
    let offset = ctx.stack.pop_u32();
    trace!("memory access: {:?} {}", memarg, offset);
    compute_memory_offset(memarg, offset)
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
    let start = vm_try!(load_start(tail_code, ctx));
    vm_try!(ctx.push_memory_to_stack::<4>(start));
    call_next(tail_code, 1, ctx)
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
    let start = vm_try!(load_start(tail_code, ctx));
    vm_try!(ctx.push_memory_to_stack::<8>(start));
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
    let start = vm_try!(load_start(tail_code, ctx));
    vm_try!(ctx.push_memory_to_stack::<4>(start));
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
    let start = vm_try!(load_start(tail_code, ctx));
    vm_try!(ctx.push_memory_to_stack::<8>(start));
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
    let start = vm_try!(load_start(tail_code, ctx));
    let value = vm_try!(ctx.read_memory_u8(start));
    vm_try!(ctx.stack.push_u32(widen_u8_to_u32(value)));
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
    let start = vm_try!(load_start(tail_code, ctx));
    let value = vm_try!(ctx.read_memory_i8(start));
    vm_try!(ctx.stack.push_i32(widen_i8_to_i32(value)));
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
    let start = vm_try!(load_start(tail_code, ctx));
    let value = vm_try!(ctx.read_memory_i16(start));
    vm_try!(ctx.stack.push_i32(widen_i16_to_i32(value)));
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
    let start = vm_try!(load_start(tail_code, ctx));
    let value = vm_try!(ctx.read_memory_u16(start));
    vm_try!(ctx.stack.push_u32(widen_u16_to_u32(value)));
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
    let start = vm_try!(load_start(tail_code, ctx));
    let value = vm_try!(ctx.read_memory_i8(start));
    vm_try!(ctx.stack.push_i64(widen_i8_to_i64(value)));
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
    let start = vm_try!(load_start(tail_code, ctx));
    let value = vm_try!(ctx.read_memory_u8(start));
    vm_try!(ctx.stack.push_u64(widen_u8_to_u64(value)));
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
    let start = vm_try!(load_start(tail_code, ctx));
    let value = vm_try!(ctx.read_memory_i16(start));
    vm_try!(ctx.stack.push_i64(widen_i16_to_i64(value)));
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
    let start = vm_try!(load_start(tail_code, ctx));
    let value = vm_try!(ctx.read_memory_u16(start));
    vm_try!(ctx.stack.push_u64(widen_u16_to_u64(value)));
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
    let start = vm_try!(load_start(tail_code, ctx));
    let value = vm_try!(ctx.read_memory_i32(start));
    vm_try!(ctx.stack.push_i64(widen_i32_to_i64(value)));
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
    let start = vm_try!(load_start(tail_code, ctx));
    let value = vm_try!(ctx.read_memory_u32(start));
    vm_try!(ctx.stack.push_u64(widen_u32_to_u64(value)));
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
    store_internal(tail_code, ctx, |ctx| {
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
    store_internal(tail_code, ctx, |ctx| {
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
    store_internal(tail_code, ctx, |ctx| {
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
    store_internal(tail_code, ctx, |ctx| {
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
    store_internal(tail_code, ctx, |ctx| {
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
    store_internal(tail_code, ctx, |ctx| {
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
    store_internal(tail_code, ctx, |ctx| {
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
    store_internal(tail_code, ctx, |ctx| {
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
    store_internal(tail_code, ctx, |ctx| {
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
    let page_size = if let Some(page_size) = ctx.memory_page_size() {
        page_size
    } else {
        return VMResult::MemoryIndexOutOfRange;
    };
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
    let result = vm_try!(ctx.grow_memory(page_size_delta));
    vm_try!(ctx.stack.push_i32(result));
    call_next(tail_code, 0, ctx)
}
