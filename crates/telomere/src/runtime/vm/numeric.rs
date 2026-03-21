use super::*;
use crate::common::stack::StackOperation;
use std::ops::BitXor;
use vstd::prelude::*;

verus! {

pub open spec fn spec_numeric_const_result(
    view: crate::common::formal::StackView,
    bytes: Seq<u8>,
) -> crate::common::formal::StackView {
    crate::common::formal::stack_const_bytes(view, bytes)
}

pub open spec fn spec_numeric_unary_result(
    view: crate::common::formal::StackView,
    operand_width: nat,
    result_bytes: Seq<u8>,
) -> crate::common::formal::StackView {
    crate::common::formal::stack_unary_result(view, operand_width, result_bytes)
}

pub open spec fn spec_numeric_binary_result(
    view: crate::common::formal::StackView,
    operand_width: nat,
    result_bytes: Seq<u8>,
) -> crate::common::formal::StackView {
    crate::common::formal::stack_binary_result(view, operand_width, result_bytes)
}

pub open spec fn spec_numeric_compare_result(
    view: crate::common::formal::StackView,
    operand_width: nat,
    result: u32,
) -> crate::common::formal::StackView {
    crate::common::formal::stack_compare_result(view, operand_width, result)
}

pub open spec fn numeric_continue_cont(step: crate::common::formal::NumericStep) -> nat {
    match step {
        crate::common::formal::NumericStep::PushConst { next_cont, .. } => next_cont,
        crate::common::formal::NumericStep::ReplaceTop { next_cont, .. } => next_cont,
    }
}

pub proof fn lemma_numeric_family_refines_spec_step(
    before: crate::common::formal::CoreStepState,
    step: crate::common::formal::NumericStep,
)
    ensures
        crate::common::formal::spec_step(
            before,
            crate::common::formal::CoreStepInstr::Numeric(step),
        ) == crate::common::formal::spec_step_numeric(before, step),
        crate::common::formal::task_id_preserved(
            before,
            crate::common::formal::spec_step_numeric(before, step).0,
        ),
        crate::common::formal::current_default_memory_of(
            crate::common::formal::spec_step_numeric(before, step).0,
        ) == crate::common::formal::current_default_memory_of(before),
        crate::common::formal::caller_default_memory_of(
            crate::common::formal::spec_step_numeric(before, step).0,
        ) == crate::common::formal::caller_default_memory_of(before),
        if crate::common::formal::outcome_is_trap(
            crate::common::formal::spec_step_numeric(before, step).1,
        ) {
            crate::common::formal::spec_step_numeric(before, step).0.context.cont_addr
                == before.context.cont_addr
        } else {
            crate::common::formal::spec_step_numeric(before, step).0.context.cont_addr
                == numeric_continue_cont(step)
        },
{
}

#[inline(always)]
fn bool_to_u32(value: bool) -> (result: u32)
    ensures
        result == if value { 1u32 } else { 0u32 },
{
    if value { 1u32 } else { 0u32 }
}

} // verus!

#[inline(always)]
unsafe fn unary_stack_op<T, F>(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    f: F,
) -> VMResult<()>
where
    Stack: StackOperation<T>,
    F: FnOnce(T) -> T,
{
    let mut facade = ExecuteContextFacade::new(ctx);
    let value: T = facade.pop();
    vm_try!(facade.push(f(value)));
    facade_call_next(tail_code, 0, &mut facade)
}

#[inline(always)]
unsafe fn unary_stack_into<T, U, F>(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    f: F,
) -> VMResult<()>
where
    Stack: StackOperation<T> + StackOperation<U>,
    F: FnOnce(T) -> U,
{
    let mut facade = ExecuteContextFacade::new(ctx);
    let value: T = facade.pop();
    vm_try!(facade.push(f(value)));
    facade_call_next(tail_code, 0, &mut facade)
}

#[inline(always)]
unsafe fn try_unary_stack_into<T, U, F>(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    f: F,
) -> VMResult<()>
where
    Stack: StackOperation<T> + StackOperation<U>,
    F: FnOnce(T) -> VMResult<U>,
{
    let mut facade = ExecuteContextFacade::new(ctx);
    let value: T = facade.pop();
    let result = vm_try!(f(value));
    vm_try!(facade.push(result));
    facade_call_next(tail_code, 0, &mut facade)
}

#[inline(always)]
unsafe fn const_stack_op<T>(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    value: T,
    skip: isize,
) -> VMResult<()>
where
    Stack: StackOperation<T>,
{
    let mut facade = ExecuteContextFacade::new(ctx);
    vm_try!(facade.push(value));
    facade_call_next(tail_code, skip, &mut facade)
}

#[inline(always)]
unsafe fn binary_stack_op<T, F>(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    f: F,
) -> VMResult<()>
where
    Stack: StackOperation<T>,
    F: FnOnce(T, T) -> T,
{
    let mut facade = ExecuteContextFacade::new(ctx);
    let rhs: T = facade.pop();
    let lhs: T = facade.pop();
    vm_try!(facade.push(f(lhs, rhs)));
    facade_call_next(tail_code, 0, &mut facade)
}

#[inline(always)]
unsafe fn try_binary_stack_op<T, F>(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    f: F,
) -> VMResult<()>
where
    Stack: StackOperation<T>,
    F: FnOnce(T, T) -> VMResult<T>,
{
    let mut facade = ExecuteContextFacade::new(ctx);
    let rhs: T = facade.pop();
    let lhs: T = facade.pop();
    let result = vm_try!(f(lhs, rhs));
    vm_try!(facade.push(result));
    facade_call_next(tail_code, 0, &mut facade)
}

#[inline(always)]
unsafe fn binary_compare_op<T, F>(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    f: F,
) -> VMResult<()>
where
    Stack: StackOperation<T>,
    F: FnOnce(T, T) -> bool,
{
    let mut facade = ExecuteContextFacade::new(ctx);
    let rhs: T = facade.pop();
    let lhs: T = facade.pop();
    vm_try!(facade.push_u32(bool_to_u32(f(lhs, rhs))));
    facade_call_next(tail_code, 0, &mut facade)
}

#[inline(always)]
unsafe fn unary_predicate_op<T, F>(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    f: F,
) -> VMResult<()>
where
    Stack: StackOperation<T>,
    F: FnOnce(T) -> bool,
{
    let mut facade = ExecuteContextFacade::new(ctx);
    let value: T = facade.pop();
    vm_try!(facade.push_u32(bool_to_u32(f(value))));
    facade_call_next(tail_code, 0, &mut facade)
}

/// WebAssembly `i32.const`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[] -> [value]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_const(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let v = (*tail_code).operand.i32;
    trace!("op_i32_const: {v}");
    const_stack_op(tail_code, ctx, v, 1)
}

/// WebAssembly `i32.add`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_add(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    binary_stack_op(tail_code, ctx, |lhs: i32, rhs: i32| {
        let result = lhs.wrapping_add(rhs);
        trace!("op_i32_add: {lhs} + {rhs} => {result}");
        result
    })
}

/// WebAssembly `i32.sub`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_sub(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    binary_stack_op(tail_code, ctx, |lhs: i32, rhs: i32| {
        let result = lhs.wrapping_sub(rhs);
        trace!("op_i32_sub: lhs={lhs} rhs={rhs} => {result}");
        result
    })
}

/// WebAssembly `i64.clz`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_clz(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    unary_stack_op::<i64, _>(tail_code, ctx, |a| a.leading_zeros().into())
}

/// WebAssembly `i64.ctz`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_ctz(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    unary_stack_op::<i64, _>(tail_code, ctx, |a| a.trailing_zeros().into())
}

/// WebAssembly `i64.popcnt`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_popcnt(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    unary_stack_op::<i64, _>(tail_code, ctx, |a| a.count_ones().into())
}

/// WebAssembly `i64.sub`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_sub(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    binary_stack_op(tail_code, ctx, |lhs: i64, rhs: i64| {
        let result = lhs.wrapping_sub(rhs);
        trace!("op_i64_sub: lhs={lhs} rhs={rhs} => {result}");
        result
    })
}

/// WebAssembly `i64.const`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[] -> [value]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_const(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    trace!("op_i64_const");
    const_stack_op(tail_code, ctx, (*tail_code).operand.i64, 1)
}

/// WebAssembly `f32.const`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[] -> [value]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f32_const(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    trace!("op_f32_const");
    const_stack_op(tail_code, ctx, (*tail_code).operand.f32, 1)
}

/// WebAssembly `f64.const`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[] -> [value]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f64_const(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    trace!("op_f64_const");
    const_stack_op(tail_code, ctx, (*tail_code).operand.f64, 1)
}

/// WebAssembly `f32.lt`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [i32]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f32_lt(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    trace!("op_f32_lt");
    binary_compare_op(tail_code, ctx, |lhs: f32, rhs: f32| lhs < rhs)
}

/// WebAssembly `f32.gt`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [i32]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f32_gt(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    trace!("op_f32_gt");
    binary_compare_op(tail_code, ctx, |lhs: f32, rhs: f32| lhs > rhs)
}

/// WebAssembly `f32.sqrt`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f32_sqrt(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    unary_stack_op::<f32, _>(tail_code, ctx, |a| a.sqrt())
}

/// WebAssembly `f32.add`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f32_add(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    trace!("op_f32_add");
    binary_stack_op(tail_code, ctx, |lhs: f32, rhs: f32| lhs + rhs)
}

/// WebAssembly `f32.sub`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f32_sub(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    trace!("op_f32_sub");
    binary_stack_op(tail_code, ctx, |lhs: f32, rhs: f32| lhs - rhs)
}

/// WebAssembly `f32.mul`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f32_mul(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    trace!("op_f32_mul");
    binary_stack_op(tail_code, ctx, |lhs: f32, rhs: f32| lhs * rhs)
}

/// WebAssembly `f32.div`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f32_div(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    trace!("op_f32_div");
    binary_stack_op(tail_code, ctx, |lhs: f32, rhs: f32| lhs / rhs)
}

/// WebAssembly `f32.min`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f32_min(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    binary_stack_op(tail_code, ctx, |lhs: f32, rhs: f32| {
        if lhs.is_nan() || rhs.is_nan() {
            f32::NAN
        } else if lhs == 0.0 && rhs == 0.0 && (lhs.is_sign_negative() || rhs.is_sign_negative()) {
            -0.0
        } else {
            f32::min(lhs, rhs)
        }
    })
}

/// WebAssembly `f32.max`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f32_max(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    binary_stack_op(tail_code, ctx, |lhs: f32, rhs: f32| {
        if lhs.is_nan() || rhs.is_nan() {
            f32::NAN
        } else if lhs == 0.0 && rhs == 0.0 && (lhs.is_sign_positive() || rhs.is_sign_positive()) {
            0.0
        } else {
            f32::max(lhs, rhs)
        }
    })
}

/// WebAssembly `f32.copysign`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f32_copysign(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    binary_stack_op(tail_code, ctx, |lhs: f32, rhs: f32| lhs.copysign(rhs))
}

/// WebAssembly `f64.add`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f64_add(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    trace!("op_f64_add");
    binary_stack_op(tail_code, ctx, |lhs: f64, rhs: f64| lhs + rhs)
}

/// WebAssembly `f64.sub`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f64_sub(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    trace!("op_f64_sub");
    binary_stack_op(tail_code, ctx, |lhs: f64, rhs: f64| lhs - rhs)
}

/// WebAssembly `f64.mul`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f64_mul(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    trace!("op_f64_mul");
    binary_stack_op(tail_code, ctx, |lhs: f64, rhs: f64| lhs * rhs)
}

/// WebAssembly `f64.div`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f64_div(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    trace!("op_f64_div");
    binary_stack_op(tail_code, ctx, |lhs: f64, rhs: f64| lhs / rhs)
}

/// WebAssembly `f64.min`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f64_min(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    binary_stack_op(tail_code, ctx, |lhs: f64, rhs: f64| {
        if lhs.is_nan() || rhs.is_nan() {
            f64::NAN
        } else if lhs == 0.0 && rhs == 0.0 && (lhs.is_sign_negative() || rhs.is_sign_negative()) {
            -0.0
        } else {
            f64::min(lhs, rhs)
        }
    })
}

/// WebAssembly `f64.max`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f64_max(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    binary_stack_op(tail_code, ctx, |lhs: f64, rhs: f64| {
        if lhs.is_nan() || rhs.is_nan() {
            f64::NAN
        } else if lhs == 0.0 && rhs == 0.0 && (lhs.is_sign_positive() || rhs.is_sign_positive()) {
            0.0
        } else {
            f64::max(lhs, rhs)
        }
    })
}

/// WebAssembly `f64.copysign`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f64_copysign(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    binary_stack_op(tail_code, ctx, |lhs: f64, rhs: f64| lhs.copysign(rhs))
}

/// WebAssembly `i32.wrap_i64`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_wrap_i64(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    unary_stack_into::<i64, i32, _>(tail_code, ctx, |value| value as i32)
}

/// WebAssembly `i32.trunc_f32_s`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: traps on NaN or out-of-range conversion.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_trunc_f32_s(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    try_unary_stack_into::<f32, i32, _>(tail_code, ctx, |value| {
        if (i32::MIN as f32) <= value && value < (i32::MAX as f32) {
            VMResult::Success(value as i32)
        } else {
            VMResult::InvalidOperand
        }
    })
}

/// WebAssembly `i32.trunc_f32_u`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: traps on NaN or out-of-range conversion.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_trunc_f32_u(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    try_unary_stack_into::<f32, u32, _>(tail_code, ctx, |value| {
        if -1.0 < value && value < (u32::MAX as f32) {
            VMResult::Success(value.trunc() as u32)
        } else {
            VMResult::InvalidOperand
        }
    })
}

/// WebAssembly `i32.trunc_f64_s`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: traps on NaN or out-of-range conversion.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_trunc_f64_s(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    try_unary_stack_into::<f64, i32, _>(tail_code, ctx, |value| {
        let truncated = value.trunc();
        if (i32::MIN as f64) <= truncated && truncated <= (i32::MAX as f64) {
            VMResult::Success(truncated as i32)
        } else {
            VMResult::InvalidOperand
        }
    })
}

/// WebAssembly `i32.trunc_f64_u`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: traps on NaN or out-of-range conversion.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_trunc_f64_u(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    try_unary_stack_into::<f64, u32, _>(tail_code, ctx, |value| {
        let truncated = value.trunc();
        if -1.0 < truncated && truncated <= (u32::MAX as f64) {
            VMResult::Success(truncated as u32)
        } else {
            VMResult::InvalidOperand
        }
    })
}

/// WebAssembly `i64.trunc_f32_s`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: traps on NaN or out-of-range conversion.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_trunc_f32_s(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    try_unary_stack_into::<f32, i64, _>(tail_code, ctx, |value| {
        if (i64::MIN as f32) <= value && value < (i64::MAX as f32) {
            VMResult::Success(value as i64)
        } else {
            VMResult::InvalidOperand
        }
    })
}

/// WebAssembly `i64.trunc_f32_u`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: traps on NaN or out-of-range conversion.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_trunc_f32_u(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    try_unary_stack_into::<f32, u64, _>(tail_code, ctx, |value| {
        if -1.0 < value && value < (u64::MAX as f32) {
            VMResult::Success(value.trunc() as u64)
        } else {
            VMResult::InvalidOperand
        }
    })
}

/// WebAssembly `i64.trunc_f64_s`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: traps on NaN or out-of-range conversion.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_trunc_f64_s(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    try_unary_stack_into::<f64, i64, _>(tail_code, ctx, |value| {
        if (i64::MIN as f64) <= value && value < (i64::MAX as f64) {
            VMResult::Success(value as i64)
        } else {
            VMResult::InvalidOperand
        }
    })
}

/// WebAssembly `i64.trunc_f64_u`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: traps on NaN or out-of-range conversion.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_trunc_f64_u(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    try_unary_stack_into::<f64, u64, _>(tail_code, ctx, |value| {
        if -1.0 < value && value < (u64::MAX as f64) {
            VMResult::Success(value as u64)
        } else {
            VMResult::InvalidOperand
        }
    })
}

/// WebAssembly `i32.trunc_sat_f32_s`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_trunc_sat_f32_s(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    unary_stack_into::<f32, i32, _>(tail_code, ctx, |value| value.trunc() as i32)
}

/// WebAssembly `i32.trunc_sat_f32_u`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_trunc_sat_f32_u(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    unary_stack_into::<f32, u32, _>(tail_code, ctx, |value| value.trunc() as u32)
}

/// WebAssembly `i32.trunc_sat_f64_s`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_trunc_sat_f64_s(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    unary_stack_into::<f64, i32, _>(tail_code, ctx, |value| value.trunc() as i32)
}

/// WebAssembly `i32.trunc_sat_f64_u`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_trunc_sat_f64_u(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    unary_stack_into::<f64, u32, _>(tail_code, ctx, |value| value.trunc() as u32)
}

/// WebAssembly `i64.trunc_sat_f32_s`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_trunc_sat_f32_s(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    unary_stack_into::<f32, i64, _>(tail_code, ctx, |value| value.trunc() as i64)
}

/// WebAssembly `i64.trunc_sat_f32_u`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_trunc_sat_f32_u(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    unary_stack_into::<f32, u64, _>(tail_code, ctx, |value| value.trunc() as u64)
}

/// WebAssembly `i64.trunc_sat_f64_s`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_trunc_sat_f64_s(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    unary_stack_into::<f64, i64, _>(tail_code, ctx, |value| value.trunc() as i64)
}

/// WebAssembly `i64.trunc_sat_f64_u`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_trunc_sat_f64_u(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    unary_stack_into::<f64, u64, _>(tail_code, ctx, |value| value.trunc() as u64)
}

/// WebAssembly `i64.add`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_add(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    binary_stack_op(tail_code, ctx, |lhs: i64, rhs: i64| lhs.wrapping_add(rhs))
}

/// WebAssembly `i64.extend_i32_s`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_extend_i32_s(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    unary_stack_into::<i32, i64, _>(tail_code, ctx, Into::into)
}

/// WebAssembly `i64.extend_i32_u`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_extend_i32_u(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    unary_stack_into::<u32, i64, _>(tail_code, ctx, Into::into)
}

/// WebAssembly `f32.convert_i32_s`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f32_convert_i32_s(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    unary_stack_into::<i32, f32, _>(tail_code, ctx, |value| value as f32)
}

/// WebAssembly `f32.convert_i32_u`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f32_convert_i32_u(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    unary_stack_into::<u32, f32, _>(tail_code, ctx, |value| value as f32)
}

/// WebAssembly `f32.convert_i64_s`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f32_convert_i64_s(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    unary_stack_into::<i64, f32, _>(tail_code, ctx, |value| value as f32)
}

/// WebAssembly `f32.convert_i64_u`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f32_convert_i64_u(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    unary_stack_into::<u64, f32, _>(tail_code, ctx, |value| value as f32)
}

/// WebAssembly `f32.demote_f64`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f32_demote_f64(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    unary_stack_into::<f64, f32, _>(tail_code, ctx, |value| value as f32)
}

/// WebAssembly `f64.convert_i32_s`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f64_convert_i32_s(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    unary_stack_into::<i32, f64, _>(tail_code, ctx, |value| value as f64)
}

/// WebAssembly `f64.convert_i32_u`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f64_convert_i32_u(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    unary_stack_into::<u32, f64, _>(tail_code, ctx, |value| value as f64)
}

/// WebAssembly `f64.convert_i64_s`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f64_convert_i64_s(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    unary_stack_into::<i64, f64, _>(tail_code, ctx, |value| value as f64)
}

/// WebAssembly `f64.convert_i64_u`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f64_convert_i64_u(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    unary_stack_into::<u64, f64, _>(tail_code, ctx, |value| value as f64)
}

/// WebAssembly `f64.promote_f32`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f64_promote_f32(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    unary_stack_into::<f32, f64, _>(tail_code, ctx, Into::into)
}

/// WebAssembly `f32.abs`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f32_abs(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    unary_stack_op::<f32, _>(tail_code, ctx, |a| a.abs())
}

/// WebAssembly `f32.neg`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f32_neg(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    unary_stack_op::<f32, _>(tail_code, ctx, |a| -a)
}

/// WebAssembly `f32.ceil`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f32_ceil(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    unary_stack_op::<f32, _>(tail_code, ctx, |a| a.ceil())
}

/// WebAssembly `f32.floor`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f32_floor(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    unary_stack_op::<f32, _>(tail_code, ctx, |a| a.floor())
}

/// WebAssembly `f32.trunc`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f32_trunc(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    unary_stack_op::<f32, _>(tail_code, ctx, |a| a.trunc())
}

/// WebAssembly `f32.nearest`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f32_nearest(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    unary_stack_op::<f32, _>(tail_code, ctx, |a| a.round_ties_even())
}

/// WebAssembly `f64.ceil`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f64_ceil(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    unary_stack_op::<f64, _>(tail_code, ctx, |a| a.ceil())
}

/// WebAssembly `f64.floor`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f64_floor(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    unary_stack_op::<f64, _>(tail_code, ctx, |a| a.floor())
}

/// WebAssembly `f64.trunc`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f64_trunc(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    unary_stack_op::<f64, _>(tail_code, ctx, |a| a.trunc())
}

/// WebAssembly `f64.nearest`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f64_nearest(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    unary_stack_op::<f64, _>(tail_code, ctx, |a| a.round_ties_even())
}

/// WebAssembly `f64.abs`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f64_abs(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    unary_stack_op::<f64, _>(tail_code, ctx, |a| a.abs())
}

/// WebAssembly `f64.neg`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f64_neg(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    unary_stack_op::<f64, _>(tail_code, ctx, |a| -a)
}

/// WebAssembly `f64.sqrt`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f64_sqrt(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    unary_stack_op::<f64, _>(tail_code, ctx, |a| a.sqrt())
}

/// WebAssembly `f32.eq`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [i32]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f32_eq(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    binary_compare_op(tail_code, ctx, |lhs: f32, rhs: f32| lhs == rhs)
}

/// WebAssembly `f32.ne`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [i32]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f32_ne(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    binary_compare_op(tail_code, ctx, |lhs: f32, rhs: f32| lhs != rhs)
}

/// WebAssembly `f32.le`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [i32]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f32_le(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    binary_compare_op(tail_code, ctx, |lhs: f32, rhs: f32| lhs <= rhs)
}

/// WebAssembly `f32.ge`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [i32]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f32_ge(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    binary_compare_op(tail_code, ctx, |lhs: f32, rhs: f32| lhs >= rhs)
}

/// WebAssembly `f64.eq`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [i32]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f64_eq(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    binary_compare_op(tail_code, ctx, |lhs: f64, rhs: f64| lhs == rhs)
}

/// WebAssembly `f64.ne`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [i32]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f64_ne(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    binary_compare_op(tail_code, ctx, |lhs: f64, rhs: f64| lhs != rhs)
}

/// WebAssembly `f64.lt`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [i32]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f64_lt(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    binary_compare_op(tail_code, ctx, |lhs: f64, rhs: f64| lhs < rhs)
}

/// WebAssembly `f64.gt`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [i32]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f64_gt(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    binary_compare_op(tail_code, ctx, |lhs: f64, rhs: f64| lhs > rhs)
}

/// WebAssembly `f64.le`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [i32]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f64_le(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    binary_compare_op(tail_code, ctx, |lhs: f64, rhs: f64| lhs <= rhs)
}

/// WebAssembly `f64.ge`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [i32]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_f64_ge(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    binary_compare_op(tail_code, ctx, |lhs: f64, rhs: f64| lhs >= rhs)
}

/// WebAssembly `i32.ctz`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_ctz(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    unary_stack_op::<u32, _>(tail_code, ctx, |a| a.trailing_zeros())
}

/// WebAssembly `i32.clz`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_clz(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    unary_stack_op::<u32, _>(tail_code, ctx, |a| a.leading_zeros())
}

/// WebAssembly `i32.popcnt`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_popcnt(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    unary_stack_op::<u32, _>(tail_code, ctx, |a| a.count_ones())
}

/// WebAssembly `i32.mul`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_mul(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    binary_stack_op(tail_code, ctx, |lhs: i32, rhs: i32| lhs.wrapping_mul(rhs))
}

/// WebAssembly `i32.div_s`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: traps on division by zero and signed overflow.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_div_s(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    try_binary_stack_op(tail_code, ctx, |lhs: i32, rhs: i32| {
        VMResult::from_option(lhs.checked_div(rhs), || VMResult::InvalidOperand)
    })
}

/// WebAssembly `i32.div_u`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: traps on division by zero.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_div_u(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    try_binary_stack_op(tail_code, ctx, |lhs: u32, rhs: u32| {
        VMResult::from_option(lhs.checked_div(rhs), || VMResult::InvalidOperand)
    })
}

/// WebAssembly `i64.div_s`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: traps on division by zero and signed overflow.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_div_s(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    try_binary_stack_op(tail_code, ctx, |lhs: i64, rhs: i64| {
        VMResult::from_option(lhs.checked_div(rhs), || VMResult::InvalidOperand)
    })
}

/// WebAssembly `i64.div_u`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: traps on division by zero.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_div_u(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    try_binary_stack_op(tail_code, ctx, |lhs: u64, rhs: u64| {
        VMResult::from_option(lhs.checked_div(rhs), || VMResult::InvalidOperand)
    })
}

/// WebAssembly `i64.and`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_and(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    binary_stack_op(tail_code, ctx, |lhs: u64, rhs: u64| lhs & rhs)
}

/// WebAssembly `i64.or`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_or(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    binary_stack_op(tail_code, ctx, |lhs: u64, rhs: u64| lhs | rhs)
}

/// WebAssembly `i64.xor`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_xor(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    binary_stack_op(tail_code, ctx, |lhs: u64, rhs: u64| lhs.bitxor(rhs))
}

/// WebAssembly `i64.mul`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_mul(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    binary_stack_op(tail_code, ctx, |lhs: i64, rhs: i64| lhs.wrapping_mul(rhs))
}

/// WebAssembly `i32.rem_s`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [result]`.
/// Traps: traps on division by zero and signed overflow.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_rem_s(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    try_binary_stack_op(tail_code, ctx, |lhs: i32, rhs: i32| {
        if rhs == 0 {
            VMResult::InvalidOperand
        } else {
            VMResult::Success(lhs.wrapping_rem(rhs))
        }
    })
}

/// WebAssembly `i32.rem_u`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [result]`.
/// Traps: traps on division by zero.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_rem_u(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    try_binary_stack_op(tail_code, ctx, |lhs: u32, rhs: u32| {
        VMResult::from_option(lhs.checked_rem(rhs), || VMResult::InvalidOperand)
    })
}

/// WebAssembly `i64.rem_s`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [result]`.
/// Traps: traps on division by zero and signed overflow.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_rem_s(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    try_binary_stack_op(tail_code, ctx, |lhs: i64, rhs: i64| {
        if rhs == 0 {
            VMResult::InvalidOperand
        } else {
            VMResult::Success(lhs.wrapping_rem(rhs))
        }
    })
}

/// WebAssembly `i64.rem_u`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [result]`.
/// Traps: traps on division by zero.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_rem_u(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    try_binary_stack_op(tail_code, ctx, |lhs: u64, rhs: u64| {
        VMResult::from_option(lhs.checked_rem(rhs), || VMResult::InvalidOperand)
    })
}

/// WebAssembly `i32.and`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_and(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    binary_stack_op(tail_code, ctx, |lhs: u32, rhs: u32| lhs & rhs)
}

/// WebAssembly `i32.or`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_or(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    binary_stack_op(tail_code, ctx, |lhs: u32, rhs: u32| lhs | rhs)
}

/// WebAssembly `i32.xor`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_xor(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    binary_stack_op(tail_code, ctx, |lhs: u32, rhs: u32| lhs.bitxor(rhs))
}

/// WebAssembly `i32.shl`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_shl(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    binary_stack_op(tail_code, ctx, |lhs: i32, rhs: i32| wasm_i32_shl(lhs, rhs))
}

/// WebAssembly `i32.shr_s`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_shr_s(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    binary_stack_op(tail_code, ctx, |lhs: i32, rhs: i32| {
        wasm_i32_shr_s(lhs, rhs)
    })
}

/// WebAssembly `i32.shr_u`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_shr_u(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    binary_stack_op(tail_code, ctx, |lhs: u32, rhs: u32| {
        wasm_i32_shr_u(lhs, rhs)
    })
}

/// WebAssembly `i32.rotl`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_rotl(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    binary_stack_op(tail_code, ctx, |lhs: u32, rhs: u32| lhs.rotate_left(rhs))
}

/// WebAssembly `i32.rotr`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_rotr(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    binary_stack_op(tail_code, ctx, |lhs: u32, rhs: u32| lhs.rotate_right(rhs))
}

/// WebAssembly `i64.rotl`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_rotl(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    binary_stack_op(tail_code, ctx, |lhs: u64, rhs: u64| {
        lhs.rotate_left(rhs as u32)
    })
}

/// WebAssembly `i64.rotr`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_rotr(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    binary_stack_op(tail_code, ctx, |lhs: u64, rhs: u64| {
        lhs.rotate_right(rhs as u32)
    })
}

/// WebAssembly `i64.shl`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_shl(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    binary_stack_op(tail_code, ctx, |lhs: i64, rhs: i64| wasm_i64_shl(lhs, rhs))
}

/// WebAssembly `i64.shr_s`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_shr_s(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    binary_stack_op(tail_code, ctx, |lhs: i64, rhs: i64| {
        wasm_i64_shr_s(lhs, rhs)
    })
}

/// WebAssembly `i64.shr_u`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_shr_u(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    binary_stack_op(tail_code, ctx, |lhs: u64, rhs: u64| {
        wasm_i64_shr_u(lhs, rhs)
    })
}

/// WebAssembly `i32.eqz`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [i32]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_eqz(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    unary_predicate_op::<u32, _>(tail_code, ctx, |value| value == 0)
}

/// WebAssembly `i64.eqz`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [i32]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_eqz(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    unary_stack_into::<u64, u32, _>(tail_code, ctx, |a| bool_to_u32(a == 0))
}

/// WebAssembly `i64.eq`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [i32]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_eq(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    binary_compare_op(tail_code, ctx, |lhs: u64, rhs: u64| lhs == rhs)
}

/// WebAssembly `i64.ne`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [i32]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_ne(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    binary_compare_op(tail_code, ctx, |lhs: u64, rhs: u64| lhs != rhs)
}

/// WebAssembly `i64.lt_s`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [i32]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_lt_s(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    binary_compare_op(tail_code, ctx, |lhs: i64, rhs: i64| lhs < rhs)
}

/// WebAssembly `i64.lt_u`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [i32]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_lt_u(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    binary_compare_op(tail_code, ctx, |lhs: u64, rhs: u64| lhs < rhs)
}

/// WebAssembly `i64.gt_s`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [i32]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_gt_s(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    binary_compare_op(tail_code, ctx, |lhs: i64, rhs: i64| lhs > rhs)
}

/// WebAssembly `i64.gt_u`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [i32]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_gt_u(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    binary_compare_op(tail_code, ctx, |lhs: u64, rhs: u64| lhs > rhs)
}

/// WebAssembly `i64.le_s`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [i32]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_le_s(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    binary_compare_op(tail_code, ctx, |lhs: i64, rhs: i64| lhs <= rhs)
}

/// WebAssembly `i64.le_u`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [i32]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_le_u(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    binary_compare_op(tail_code, ctx, |lhs: u64, rhs: u64| lhs <= rhs)
}

/// WebAssembly `i64.ge_s`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [i32]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_ge_s(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    binary_compare_op(tail_code, ctx, |lhs: i64, rhs: i64| lhs >= rhs)
}

/// WebAssembly `i64.ge_u`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [i32]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_ge_u(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    binary_compare_op(tail_code, ctx, |lhs: u64, rhs: u64| lhs >= rhs)
}

/// WebAssembly `i32.eq`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [i32]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_eq(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    binary_compare_op(tail_code, ctx, |lhs: u32, rhs: u32| lhs == rhs)
}

/// WebAssembly `i32.ne`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [i32]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_ne(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    binary_compare_op(tail_code, ctx, |lhs: u32, rhs: u32| lhs != rhs)
}

/// WebAssembly `i32.le_s`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [i32]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_le_s(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    binary_compare_op(tail_code, ctx, |lhs: i32, rhs: i32| lhs <= rhs)
}

/// WebAssembly `i32.le_u`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [i32]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_le_u(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    binary_compare_op(tail_code, ctx, |lhs: u32, rhs: u32| lhs <= rhs)
}

/// WebAssembly `i32.lt_s`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [i32]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_lt_s(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    binary_compare_op(tail_code, ctx, |lhs: i32, rhs: i32| lhs < rhs)
}

/// WebAssembly `i32.lt_u`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [i32]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_lt_u(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    binary_compare_op(tail_code, ctx, |lhs: u32, rhs: u32| lhs < rhs)
}

/// WebAssembly `i32.gt_s`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [i32]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_gt_s(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    binary_compare_op(tail_code, ctx, |lhs: i32, rhs: i32| lhs > rhs)
}

/// WebAssembly `i32.gt_u`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [i32]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_gt_u(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    binary_compare_op(tail_code, ctx, |lhs: u32, rhs: u32| lhs > rhs)
}

/// WebAssembly `i32.ge_s`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [i32]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_ge_s(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    binary_compare_op(tail_code, ctx, |lhs: i32, rhs: i32| lhs >= rhs)
}

/// WebAssembly `i32.ge_u`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[lhs, rhs] -> [i32]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_ge_u(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    binary_compare_op(tail_code, ctx, |lhs: u32, rhs: u32| lhs >= rhs)
}

/// WebAssembly `i32.extend8_s`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_extend8_s(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    unary_stack_into::<u32, i32, _>(tail_code, ctx, |v| i8::from_le_bytes([v as u8]).into())
}

/// WebAssembly `i32.extend16_s`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i32_extend16_s(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    unary_stack_into::<u32, i32, _>(tail_code, ctx, |v| {
        i16::from_le_bytes([v as u8, (v >> 8) as u8]).into()
    })
}

/// WebAssembly `i64.extend8_s`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_extend8_s(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    unary_stack_into::<u64, i64, _>(tail_code, ctx, |v| i8::from_le_bytes([v as u8]).into())
}

/// WebAssembly `i64.extend16_s`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_extend16_s(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    unary_stack_into::<u64, i64, _>(tail_code, ctx, |v| {
        i16::from_le_bytes([v as u8, (v >> 8) as u8]).into()
    })
}

/// WebAssembly `i64.extend32_s`.
///
/// Spec:
/// - Syntax: https://webassembly.github.io/spec/core/syntax/instructions.html
/// - Validation: https://webassembly.github.io/spec/core/valid/instructions.html
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[value] -> [result]`.
/// Traps: none.
/// Notes: Implements the validated numeric semantics and tail-dispatches with `call_next`.
///
/// # Safety
/// - `tail_code` must point to the decoded instruction for this handler in the active function body.
/// - `ctx` must reference a live execution context whose validated operand stack, locals, and default memory/table state satisfy this instruction.
/// - This handler must not keep borrows, locks, or guards alive across `call_next` or `call_code`.
pub unsafe fn op_i64_extend32_s(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    unary_stack_into::<u64, i64, _>(tail_code, ctx, |v| {
        i32::from_le_bytes([v as u8, (v >> 8) as u8, (v >> 16) as u8, (v >> 24) as u8]).into()
    })
}
