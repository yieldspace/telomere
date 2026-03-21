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

pub open spec fn spec_load_start_indexed_result(
    default_memory_present: bool,
    memarg_offset: u32,
    offset: u32,
    memidx: u32,
) -> Option<(int, int)> {
    match crate::runtime::vm::spec_load_start_result(default_memory_present, memarg_offset, offset) {
        Some(start) => Some((start, memidx as int)),
        None => None,
    }
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
unsafe fn load_start(
    tail_code: *const Instr,
    facade: &mut ExecuteContextFacade<'_, '_>,
) -> VMResult<usize> {
    debug_assert!(facade.has_default_memory());
    let memarg = (*tail_code).operand.memarg;
    let offset = facade.pop_u32();
    trace!("memory access: {:?} {}", memarg, offset);
    compute_memory_offset(memarg, offset)
}

#[inline(always)]
unsafe fn load_start_indexed(
    tail_code: *const Instr,
    facade: &mut ExecuteContextFacade<'_, '_>,
) -> VMResult<(usize, u32)> {
    let memarg = (*tail_code).operand.memarg;
    let memidx = (*tail_code.add(1)).operand.u32;
    let offset = facade.pop_u32();
    trace!(
        "indexed memory access: {:?} {} memidx={}",
        memarg,
        offset,
        memidx
    );
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
            let mut facade = ExecuteContextFacade::new(ctx);
            let (start, memidx) = vm_try!(load_start_indexed(tail_code, &mut facade));
            vm_try!(facade.push_memory_to_stack_local_indexed::<$bytes>(memidx, start));
            facade_call_next(tail_code, 2, &mut facade)
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
            let mut facade = ExecuteContextFacade::new(ctx);
            let (start, memidx) = vm_try!(load_start_indexed(tail_code, &mut facade));
            vm_try!(facade.push_memory_to_stack_shared_indexed::<$bytes>(memidx, start));
            facade_call_next(tail_code, 2, &mut facade)
        }
    };
}

macro_rules! define_indexed_scalar_load {
    ($local:ident, $shared:ident, $mnemonic:literal, $local_reader:ident, $shared_reader:ident, $push:ident, $convert:ident) => {
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
            let mut facade = ExecuteContextFacade::new(ctx);
            let (start, memidx) = vm_try!(load_start_indexed(tail_code, &mut facade));
            let value = vm_try!(facade.$local_reader(memidx, start));
            vm_try!(facade.$push($convert(value)));
            facade_call_next(tail_code, 2, &mut facade)
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
            let mut facade = ExecuteContextFacade::new(ctx);
            let (start, memidx) = vm_try!(load_start_indexed(tail_code, &mut facade));
            let value = vm_try!(facade.$shared_reader(memidx, start));
            vm_try!(facade.$push($convert(value)));
            facade_call_next(tail_code, 2, &mut facade)
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
            store_internal_local_indexed(tail_code, ctx, $make_operation)
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
            store_internal_shared_indexed(tail_code, ctx, $make_operation)
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
    let mut facade = ExecuteContextFacade::new(ctx);
    let start = vm_try!(load_start(tail_code, &mut facade));
    vm_try!(facade.push_memory_to_stack::<4>(start));
    facade_call_next(tail_code, 1, &mut facade)
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
    let mut facade = ExecuteContextFacade::new(ctx);
    let start = vm_try!(load_start(tail_code, &mut facade));
    vm_try!(facade.push_memory_to_stack::<8>(start));
    facade_call_next(tail_code, 1, &mut facade)
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
    let mut facade = ExecuteContextFacade::new(ctx);
    let start = vm_try!(load_start(tail_code, &mut facade));
    vm_try!(facade.push_memory_to_stack::<4>(start));
    facade_call_next(tail_code, 1, &mut facade)
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
    let mut facade = ExecuteContextFacade::new(ctx);
    let start = vm_try!(load_start(tail_code, &mut facade));
    vm_try!(facade.push_memory_to_stack::<8>(start));
    facade_call_next(tail_code, 1, &mut facade)
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
    let mut facade = ExecuteContextFacade::new(ctx);
    let start = vm_try!(load_start(tail_code, &mut facade));
    let value = vm_try!(facade.read_memory_u8(start));
    vm_try!(facade.push_u32(widen_u8_to_u32(value)));
    facade_call_next(tail_code, 1, &mut facade)
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
    let mut facade = ExecuteContextFacade::new(ctx);
    let start = vm_try!(load_start(tail_code, &mut facade));
    let value = vm_try!(facade.read_memory_i8(start));
    vm_try!(facade.push_i32(widen_i8_to_i32(value)));
    facade_call_next(tail_code, 1, &mut facade)
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
    let mut facade = ExecuteContextFacade::new(ctx);
    let start = vm_try!(load_start(tail_code, &mut facade));
    let value = vm_try!(facade.read_memory_i16(start));
    vm_try!(facade.push_i32(widen_i16_to_i32(value)));
    facade_call_next(tail_code, 1, &mut facade)
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
    let mut facade = ExecuteContextFacade::new(ctx);
    let start = vm_try!(load_start(tail_code, &mut facade));
    let value = vm_try!(facade.read_memory_u16(start));
    vm_try!(facade.push_u32(widen_u16_to_u32(value)));
    facade_call_next(tail_code, 1, &mut facade)
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
    let mut facade = ExecuteContextFacade::new(ctx);
    let start = vm_try!(load_start(tail_code, &mut facade));
    let value = vm_try!(facade.read_memory_i8(start));
    vm_try!(facade.push_i64(widen_i8_to_i64(value)));
    facade_call_next(tail_code, 1, &mut facade)
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
    let mut facade = ExecuteContextFacade::new(ctx);
    let start = vm_try!(load_start(tail_code, &mut facade));
    let value = vm_try!(facade.read_memory_u8(start));
    vm_try!(facade.push_u64(widen_u8_to_u64(value)));
    facade_call_next(tail_code, 1, &mut facade)
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
    let mut facade = ExecuteContextFacade::new(ctx);
    let start = vm_try!(load_start(tail_code, &mut facade));
    let value = vm_try!(facade.read_memory_i16(start));
    vm_try!(facade.push_i64(widen_i16_to_i64(value)));
    facade_call_next(tail_code, 1, &mut facade)
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
    let mut facade = ExecuteContextFacade::new(ctx);
    let start = vm_try!(load_start(tail_code, &mut facade));
    let value = vm_try!(facade.read_memory_u16(start));
    vm_try!(facade.push_u64(widen_u16_to_u64(value)));
    facade_call_next(tail_code, 1, &mut facade)
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
    let mut facade = ExecuteContextFacade::new(ctx);
    let start = vm_try!(load_start(tail_code, &mut facade));
    let value = vm_try!(facade.read_memory_i32(start));
    vm_try!(facade.push_i64(widen_i32_to_i64(value)));
    facade_call_next(tail_code, 1, &mut facade)
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
    let mut facade = ExecuteContextFacade::new(ctx);
    let start = vm_try!(load_start(tail_code, &mut facade));
    let value = vm_try!(facade.read_memory_u32(start));
    vm_try!(facade.push_u64(widen_u32_to_u64(value)));
    facade_call_next(tail_code, 1, &mut facade)
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
    store_internal_local(
        tail_code,
        ctx,
        |facade: &mut ExecuteContextFacade<'_, '_>| StoreBytes::Write4(facade.pop_u8_array::<4>()),
    )
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
    store_internal_local(
        tail_code,
        ctx,
        |facade: &mut ExecuteContextFacade<'_, '_>| StoreBytes::Write8(facade.pop_u8_array::<8>()),
    )
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
    store_internal_local(
        tail_code,
        ctx,
        |facade: &mut ExecuteContextFacade<'_, '_>| StoreBytes::Write4(facade.pop_u8_array::<4>()),
    )
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
    store_internal_local(
        tail_code,
        ctx,
        |facade: &mut ExecuteContextFacade<'_, '_>| StoreBytes::Write8(facade.pop_u8_array::<8>()),
    )
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
    store_internal_local(
        tail_code,
        ctx,
        |facade: &mut ExecuteContextFacade<'_, '_>| {
            StoreBytes::Write1(truncate_u32_to_u8_bytes(facade.pop_u32()))
        },
    )
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
    store_internal_local(
        tail_code,
        ctx,
        |facade: &mut ExecuteContextFacade<'_, '_>| {
            StoreBytes::Write2(truncate_u32_to_u16_bytes(facade.pop_u32()))
        },
    )
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
    store_internal_local(
        tail_code,
        ctx,
        |facade: &mut ExecuteContextFacade<'_, '_>| {
            StoreBytes::Write1(truncate_u64_to_u8_bytes(facade.pop_u64()))
        },
    )
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
    store_internal_local(
        tail_code,
        ctx,
        |facade: &mut ExecuteContextFacade<'_, '_>| {
            StoreBytes::Write2(truncate_u64_to_u16_bytes(facade.pop_u64()))
        },
    )
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
    store_internal_local(
        tail_code,
        ctx,
        |facade: &mut ExecuteContextFacade<'_, '_>| {
            StoreBytes::Write4(truncate_u64_to_u32_bytes(facade.pop_u64()))
        },
    )
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
    let mut facade = ExecuteContextFacade::new(ctx);
    let page_size = facade.memory_page_size().unwrap_or_default();
    vm_try!(facade.push_u32(page_size));
    facade_call_next(tail_code, 0, &mut facade)
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
    let mut facade = ExecuteContextFacade::new(ctx);
    let page_size_delta = facade.pop_u32();
    let result = vm_try!(facade.grow_memory(page_size_delta));
    vm_try!(facade.push_i32(result));
    facade_call_next(tail_code, 0, &mut facade)
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
            let mut facade = ExecuteContextFacade::new(ctx);
            let start = vm_try!(load_start(tail_code, &mut facade));
            vm_try!(facade.push_memory_to_stack::<$bytes>(start));
            facade_call_next(tail_code, 1, &mut facade)
        }
    };
}

macro_rules! define_shared_scalar_load {
    ($name:ident, $mnemonic:literal, $reader:ident, $push:ident, $convert:ident) => {
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
            let mut facade = ExecuteContextFacade::new(ctx);
            let start = vm_try!(load_start(tail_code, &mut facade));
            let value = vm_try!(facade.$reader(start));
            vm_try!(facade.$push($convert(value)));
            facade_call_next(tail_code, 1, &mut facade)
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
            store_internal_shared(tail_code, ctx, $make_operation)
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
    read_memory_u8,
    push_u32,
    widen_u8_to_u32
);
define_shared_scalar_load!(
    op_i32_load8_s_shared,
    "i32.load8_s",
    read_memory_i8,
    push_i32,
    widen_i8_to_i32
);
define_shared_scalar_load!(
    op_i32_load16_s_shared,
    "i32.load16_s",
    read_memory_i16,
    push_i32,
    widen_i16_to_i32
);
define_shared_scalar_load!(
    op_i32_load16_u_shared,
    "i32.load16_u",
    read_memory_u16,
    push_u32,
    widen_u16_to_u32
);
define_shared_scalar_load!(
    op_i64_load8_s_shared,
    "i64.load8_s",
    read_memory_i8,
    push_i64,
    widen_i8_to_i64
);
define_shared_scalar_load!(
    op_i64_load8_u_shared,
    "i64.load8_u",
    read_memory_u8,
    push_u64,
    widen_u8_to_u64
);
define_shared_scalar_load!(
    op_i64_load16_s_shared,
    "i64.load16_s",
    read_memory_i16,
    push_i64,
    widen_i16_to_i64
);
define_shared_scalar_load!(
    op_i64_load16_u_shared,
    "i64.load16_u",
    read_memory_u16,
    push_u64,
    widen_u16_to_u64
);
define_shared_scalar_load!(
    op_i64_load32_s_shared,
    "i64.load32_s",
    read_memory_i32,
    push_i64,
    widen_i32_to_i64
);
define_shared_scalar_load!(
    op_i64_load32_u_shared,
    "i64.load32_u",
    read_memory_u32,
    push_u64,
    widen_u32_to_u64
);
define_shared_store_alias!(
    op_i32_store_shared,
    "i32.store",
    |facade: &mut ExecuteContextFacade<'_, '_>| { StoreBytes::Write4(facade.pop_u8_array::<4>()) }
);
define_shared_store_alias!(
    op_i64_store_shared,
    "i64.store",
    |facade: &mut ExecuteContextFacade<'_, '_>| { StoreBytes::Write8(facade.pop_u8_array::<8>()) }
);
define_shared_store_alias!(
    op_f32_store_shared,
    "f32.store",
    |facade: &mut ExecuteContextFacade<'_, '_>| { StoreBytes::Write4(facade.pop_u8_array::<4>()) }
);
define_shared_store_alias!(
    op_f64_store_shared,
    "f64.store",
    |facade: &mut ExecuteContextFacade<'_, '_>| { StoreBytes::Write8(facade.pop_u8_array::<8>()) }
);
define_shared_store_alias!(
    op_i32_store8_shared,
    "i32.store8",
    |facade: &mut ExecuteContextFacade<'_, '_>| {
        StoreBytes::Write1(truncate_u32_to_u8_bytes(facade.pop_u32()))
    }
);
define_shared_store_alias!(
    op_i32_store16_shared,
    "i32.store16",
    |facade: &mut ExecuteContextFacade<'_, '_>| {
        StoreBytes::Write2(truncate_u32_to_u16_bytes(facade.pop_u32()))
    }
);
define_shared_store_alias!(
    op_i64_store8_shared,
    "i64.store8",
    |facade: &mut ExecuteContextFacade<'_, '_>| {
        StoreBytes::Write1(truncate_u64_to_u8_bytes(facade.pop_u64()))
    }
);
define_shared_store_alias!(
    op_i64_store16_shared,
    "i64.store16",
    |facade: &mut ExecuteContextFacade<'_, '_>| {
        StoreBytes::Write2(truncate_u64_to_u16_bytes(facade.pop_u64()))
    }
);
define_shared_store_alias!(
    op_i64_store32_shared,
    "i64.store32",
    |facade: &mut ExecuteContextFacade<'_, '_>| {
        StoreBytes::Write4(truncate_u64_to_u32_bytes(facade.pop_u64()))
    }
);

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
    read_u8_at_local_indexed,
    read_u8_at_shared_indexed,
    push_u32,
    widen_u8_to_u32
);
define_indexed_scalar_load!(
    op_i32_load8_s_indexed_local,
    op_i32_load8_s_indexed_shared,
    "i32.load8_s",
    read_i8_at_local_indexed,
    read_i8_at_shared_indexed,
    push_i32,
    widen_i8_to_i32
);
define_indexed_scalar_load!(
    op_i32_load16_s_indexed_local,
    op_i32_load16_s_indexed_shared,
    "i32.load16_s",
    read_i16_at_local_indexed,
    read_i16_at_shared_indexed,
    push_i32,
    widen_i16_to_i32
);
define_indexed_scalar_load!(
    op_i32_load16_u_indexed_local,
    op_i32_load16_u_indexed_shared,
    "i32.load16_u",
    read_u16_at_local_indexed,
    read_u16_at_shared_indexed,
    push_u32,
    widen_u16_to_u32
);
define_indexed_scalar_load!(
    op_i64_load8_s_indexed_local,
    op_i64_load8_s_indexed_shared,
    "i64.load8_s",
    read_i8_at_local_indexed,
    read_i8_at_shared_indexed,
    push_i64,
    widen_i8_to_i64
);
define_indexed_scalar_load!(
    op_i64_load8_u_indexed_local,
    op_i64_load8_u_indexed_shared,
    "i64.load8_u",
    read_u8_at_local_indexed,
    read_u8_at_shared_indexed,
    push_u64,
    widen_u8_to_u64
);
define_indexed_scalar_load!(
    op_i64_load16_s_indexed_local,
    op_i64_load16_s_indexed_shared,
    "i64.load16_s",
    read_i16_at_local_indexed,
    read_i16_at_shared_indexed,
    push_i64,
    widen_i16_to_i64
);
define_indexed_scalar_load!(
    op_i64_load16_u_indexed_local,
    op_i64_load16_u_indexed_shared,
    "i64.load16_u",
    read_u16_at_local_indexed,
    read_u16_at_shared_indexed,
    push_u64,
    widen_u16_to_u64
);
define_indexed_scalar_load!(
    op_i64_load32_s_indexed_local,
    op_i64_load32_s_indexed_shared,
    "i64.load32_s",
    read_i32_at_local_indexed,
    read_i32_at_shared_indexed,
    push_i64,
    widen_i32_to_i64
);
define_indexed_scalar_load!(
    op_i64_load32_u_indexed_local,
    op_i64_load32_u_indexed_shared,
    "i64.load32_u",
    read_u32_at_local_indexed,
    read_u32_at_shared_indexed,
    push_u64,
    widen_u32_to_u64
);
define_indexed_store_alias!(
    op_i32_store_indexed_local,
    op_i32_store_indexed_shared,
    "i32.store",
    |facade: &mut ExecuteContextFacade<'_, '_>| { StoreBytes::Write4(facade.pop_u8_array::<4>()) }
);
define_indexed_store_alias!(
    op_i64_store_indexed_local,
    op_i64_store_indexed_shared,
    "i64.store",
    |facade: &mut ExecuteContextFacade<'_, '_>| { StoreBytes::Write8(facade.pop_u8_array::<8>()) }
);
define_indexed_store_alias!(
    op_f32_store_indexed_local,
    op_f32_store_indexed_shared,
    "f32.store",
    |facade: &mut ExecuteContextFacade<'_, '_>| { StoreBytes::Write4(facade.pop_u8_array::<4>()) }
);
define_indexed_store_alias!(
    op_f64_store_indexed_local,
    op_f64_store_indexed_shared,
    "f64.store",
    |facade: &mut ExecuteContextFacade<'_, '_>| { StoreBytes::Write8(facade.pop_u8_array::<8>()) }
);
define_indexed_store_alias!(
    op_i32_store8_indexed_local,
    op_i32_store8_indexed_shared,
    "i32.store8",
    |facade: &mut ExecuteContextFacade<'_, '_>| {
        StoreBytes::Write1(truncate_u32_to_u8_bytes(facade.pop_u32()))
    }
);
define_indexed_store_alias!(
    op_i32_store16_indexed_local,
    op_i32_store16_indexed_shared,
    "i32.store16",
    |facade: &mut ExecuteContextFacade<'_, '_>| {
        StoreBytes::Write2(truncate_u32_to_u16_bytes(facade.pop_u32()))
    }
);
define_indexed_store_alias!(
    op_i64_store8_indexed_local,
    op_i64_store8_indexed_shared,
    "i64.store8",
    |facade: &mut ExecuteContextFacade<'_, '_>| {
        StoreBytes::Write1(truncate_u64_to_u8_bytes(facade.pop_u64()))
    }
);
define_indexed_store_alias!(
    op_i64_store16_indexed_local,
    op_i64_store16_indexed_shared,
    "i64.store16",
    |facade: &mut ExecuteContextFacade<'_, '_>| {
        StoreBytes::Write2(truncate_u64_to_u16_bytes(facade.pop_u64()))
    }
);
define_indexed_store_alias!(
    op_i64_store32_indexed_local,
    op_i64_store32_indexed_shared,
    "i64.store32",
    |facade: &mut ExecuteContextFacade<'_, '_>| {
        StoreBytes::Write4(truncate_u64_to_u32_bytes(facade.pop_u64()))
    }
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
    let mut facade = ExecuteContextFacade::new(ctx);
    let page_size = facade.memory_page_size().unwrap_or_default();
    vm_try!(facade.push_u32(page_size));
    facade_call_next(tail_code, 0, &mut facade)
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
    let mut facade = ExecuteContextFacade::new(ctx);
    let page_size_delta = facade.pop_u32();
    let result = vm_try!(facade.grow_memory(page_size_delta));
    vm_try!(facade.push_i32(result));
    facade_call_next(tail_code, 0, &mut facade)
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
    let mut facade = ExecuteContextFacade::new(ctx);
    let memidx = (*tail_code).operand.u32;
    let page_size = facade.memory_page_size_local_indexed(memidx);
    vm_try!(facade.push_u32(page_size));
    facade_call_next(tail_code, 1, &mut facade)
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
    let mut facade = ExecuteContextFacade::new(ctx);
    let memidx = (*tail_code).operand.u32;
    let page_size = facade.memory_page_size_shared_indexed(memidx);
    vm_try!(facade.push_u32(page_size));
    facade_call_next(tail_code, 1, &mut facade)
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
    let mut facade = ExecuteContextFacade::new(ctx);
    let memidx = (*tail_code).operand.u32;
    let page_size_delta = facade.pop_u32();
    let result = vm_try!(facade.grow_memory_local_indexed(memidx, page_size_delta));
    vm_try!(facade.push_i32(result));
    facade_call_next(tail_code, 1, &mut facade)
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
    let mut facade = ExecuteContextFacade::new(ctx);
    let memidx = (*tail_code).operand.u32;
    let page_size_delta = facade.pop_u32();
    let result = vm_try!(facade.grow_memory_shared_indexed(memidx, page_size_delta));
    vm_try!(facade.push_i32(result));
    facade_call_next(tail_code, 1, &mut facade)
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
            store::InstanceId,
            ExecuteContext, GcRef, LocalReference, Operand, Store, StoreInner,
        },
        runtime::{memory_effect::PendingOp, scheduler::PendingOpEmitter},
    };
    use std::collections::VecDeque;

    fn frame(kind: CachedMemoryKind, raw: u32) -> CallFrameCache {
        CallFrameCache {
            code_addr: GcRef(0),
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
        pending_ops: &'a mut VecDeque<PendingOp>,
    ) -> ExecuteContext<'a> {
        ExecuteContext::new(
            stack,
            LocalReference {
                local_top: 0,
                local_size: 0,
            },
            frame(CachedMemoryKind::Local, 1),
            store,
            gc,
            PendingOpEmitter::from_parts(1, pending_effects, pending_ops),
            std::ptr::null(),
            1,
        )
    }

    #[test]
    fn load_start_helpers_match_offset_and_index_contracts() {
        let store = Store::new();
        let mut gc = StoreInner::new();
        let mut pending_effects = 0;
        let mut pending_ops = VecDeque::new();
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
            &mut pending_ops,
        );

        let mut facade = ExecuteContextFacade::new(&mut ctx);
        let start = unsafe { load_start(program.as_ptr(), &mut facade) }.unwrap();
        assert_eq!(start, 12);

        facade.push_u32(11).unwrap();
        let (indexed_start, memidx) =
            unsafe { load_start_indexed(program.as_ptr(), &mut facade) }.unwrap();
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

        let mut facade = ExecuteContextFacade::new(&mut ctx);
        assert!(matches!(
            unsafe { load_start(program.as_ptr(), &mut facade) },
            VMResult::MemoryIndexOutOfRange
        ));
    }
}
