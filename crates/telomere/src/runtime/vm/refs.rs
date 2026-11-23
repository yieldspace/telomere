use super::*;

/// WebAssembly `ref.null`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[] -> [ref]`.
/// Traps: none.
/// Notes: Implements the validated reference semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
#[inline(never)]
pub unsafe fn op_ref_null(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    vm_try!(ctx.stack.push_u32(0));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `ref.is.null`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[ref] -> [i32]`.
/// Traps: none.
/// Notes: Implements the validated reference semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
#[inline(never)]
pub unsafe fn op_ref_is_null(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    // #240: fn-pointer identity is load-bearing for this handler (see the
    // `Op` contract in `common.rs`). Referencing a function-local static
    // keeps this body distinct from `op_i32_eqz`, whose body is otherwise
    // byte-identical (both push whether a slot is zero), so identical-code
    // folding cannot merge the two fn items into one address.
    //
    // The reference is a volatile load, not the `black_box` of the static's
    // address used at the other barrier sites: `black_box` materialises its
    // argument in an address-taken stack slot, and in this handler that
    // slot made LLVM drop the sibling call for the `call_next` dispatch,
    // so every execution left a live frame behind and a long `ref.is_null`
    // loop overflowed the host stack (#282, the same failure as #277's
    // `op_f64_const`). A volatile load of a static that is never written
    // cannot be elided or folded to a constant, and it leaves no frame
    // state, so the dispatch stays a tail jump.
    {
        static FN_IDENTITY: u8 = 0;
        // SAFETY: `FN_IDENTITY` is a live function-local static that is
        // never written, so a volatile read of it is in-bounds and cannot
        // race; the value itself is unused.
        let _ = unsafe { std::ptr::read_volatile(&raw const FN_IDENTITY) };
    }
    let value = ctx.stack.pop_u32();
    vm_try!(ctx.stack.push_u32(u32::from(value == 0)));
    call_next(tail_code, 0, ctx)
}

/// WebAssembly `ref.func`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[] -> [funcref]`.
/// Traps: none.
/// Notes: Implements the validated reference semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
#[inline(never)]
pub unsafe fn op_ref_func(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let funcidx = (*tail_code).operand.u32;
    vm_try!(ctx
        .stack
        .push_u32(ctx.instance().funcs.as_slice()[funcidx as usize].get()));
    call_next(tail_code, 1, ctx)
}
