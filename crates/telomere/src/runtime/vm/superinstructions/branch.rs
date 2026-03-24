use super::compare_select::{
    f32_compare_eval, f64_compare_eval, i32_compare_eval, i64_compare_eval,
};
use super::producer_seed::{producer_seed_u32, producer_seed_u64};
use super::*;

enum ControlBranchKind {
    BrIf,
    If,
}

#[inline(always)]
unsafe fn branch_target_relative(
    tail_code: *const Instr,
    ctx: &ExecuteContext,
    jump_slot: usize,
    fallthrough_operands: isize,
    branch_kind: ControlBranchKind,
    taken: bool,
) -> *const Instr {
    let target = (*tail_code.add(jump_slot)).operand.jump_addr;
    match (branch_kind, taken) {
        (ControlBranchKind::BrIf, true) => ctx.code().offset(target as isize),
        (ControlBranchKind::BrIf, false) => tail_code.offset(fallthrough_operands),
        (ControlBranchKind::If, true) => tail_code.offset(fallthrough_operands),
        (ControlBranchKind::If, false) => ctx.code().offset(target as isize),
    }
}

#[inline(always)]
unsafe fn branch_target_ptr(
    tail_code: *const Instr,
    jump_slot: usize,
    fallthrough_operands: isize,
    branch_kind: ControlBranchKind,
    taken: bool,
) -> *const Instr {
    let target = (*tail_code.add(jump_slot)).operand.code_ptr as *const Instr;
    match (branch_kind, taken) {
        (ControlBranchKind::BrIf, true) => target,
        (ControlBranchKind::BrIf, false) => tail_code.offset(fallthrough_operands),
        (ControlBranchKind::If, true) => tail_code.offset(fallthrough_operands),
        (ControlBranchKind::If, false) => target,
    }
}

#[inline(always)]
unsafe fn op_local_branch_u32(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    zero_test: bool,
    branch_kind: ControlBranchKind,
) -> VMResult<()> {
    let local_addr = (*tail_code).operand.local_addr;
    let cond = local_u32(ctx.stack, &ctx.local_reference(), local_addr) == 0;
    let taken = if zero_test { cond } else { !cond };
    let ptr = branch_target_relative(tail_code, ctx, 1, 2, branch_kind, taken);
    call_next(ptr, 0, ctx)
}

#[inline(always)]
unsafe fn op_local_branch_u32_ptr(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    zero_test: bool,
    branch_kind: ControlBranchKind,
) -> VMResult<()> {
    let local_addr = (*tail_code).operand.local_addr;
    let cond = local_u32(ctx.stack, &ctx.local_reference(), local_addr) == 0;
    let taken = if zero_test { cond } else { !cond };
    let ptr = branch_target_ptr(tail_code, 1, 2, branch_kind, taken);
    call_next(ptr, 0, ctx)
}

pub unsafe fn op_i32_local_br_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_local_branch_u32(tail_code, ctx, false, ControlBranchKind::BrIf)
}

pub unsafe fn op_i32_local_eqz_br_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_local_branch_u32(tail_code, ctx, true, ControlBranchKind::BrIf)
}

pub unsafe fn op_i32_local_if(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    op_local_branch_u32(tail_code, ctx, false, ControlBranchKind::If)
}

pub unsafe fn op_i32_local_eqz_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_local_branch_u32(tail_code, ctx, true, ControlBranchKind::If)
}

/// Telomere runtime helper `op_i32_local_br_if_ptr`.
///
/// Stack effect: `internal local branch dispatch`.
/// # Safety
/// - `tail_code` must point at the pointer-bearing operands for this specialized branch.
/// - `ctx` must reference a live execution context for the same validated frame and store.
pub unsafe fn op_i32_local_br_if_ptr(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_local_branch_u32_ptr(tail_code, ctx, false, ControlBranchKind::BrIf)
}

/// Telomere runtime helper `op_i32_local_eqz_br_if_ptr`.
///
/// Stack effect: `internal local branch dispatch`.
/// # Safety
/// - `tail_code` must point at the pointer-bearing operands for this specialized branch.
/// - `ctx` must reference a live execution context for the same validated frame and store.
pub unsafe fn op_i32_local_eqz_br_if_ptr(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_local_branch_u32_ptr(tail_code, ctx, true, ControlBranchKind::BrIf)
}

/// Telomere runtime helper `op_i32_local_if_ptr`.
///
/// Stack effect: `internal local branch dispatch`.
/// # Safety
/// - `tail_code` must point at the pointer-bearing operands for this specialized branch.
/// - `ctx` must reference a live execution context for the same validated frame and store.
pub unsafe fn op_i32_local_if_ptr(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_local_branch_u32_ptr(tail_code, ctx, false, ControlBranchKind::If)
}

/// Telomere runtime helper `op_i32_local_eqz_if_ptr`.
///
/// Stack effect: `internal local branch dispatch`.
/// # Safety
/// - `tail_code` must point at the pointer-bearing operands for this specialized branch.
/// - `ctx` must reference a live execution context for the same validated frame and store.
pub unsafe fn op_i32_local_eqz_if_ptr(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_local_branch_u32_ptr(tail_code, ctx, true, ControlBranchKind::If)
}

#[inline(always)]
unsafe fn op_local_branch_u64(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    zero_test: bool,
    branch_kind: ControlBranchKind,
) -> VMResult<()> {
    let local_addr = (*tail_code).operand.local_addr;
    let cond = local_u64(ctx.stack, &ctx.local_reference(), local_addr) == 0;
    let taken = if zero_test { cond } else { !cond };
    let ptr = branch_target_relative(tail_code, ctx, 1, 2, branch_kind, taken);
    call_next(ptr, 0, ctx)
}

#[inline(always)]
unsafe fn op_local_branch_u64_ptr(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    zero_test: bool,
    branch_kind: ControlBranchKind,
) -> VMResult<()> {
    let local_addr = (*tail_code).operand.local_addr;
    let cond = local_u64(ctx.stack, &ctx.local_reference(), local_addr) == 0;
    let taken = if zero_test { cond } else { !cond };
    let ptr = branch_target_ptr(tail_code, 1, 2, branch_kind, taken);
    call_next(ptr, 0, ctx)
}

pub unsafe fn op_i64_local_br_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_local_branch_u64(tail_code, ctx, false, ControlBranchKind::BrIf)
}

pub unsafe fn op_i64_local_eqz_br_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_local_branch_u64(tail_code, ctx, true, ControlBranchKind::BrIf)
}

pub unsafe fn op_i64_local_if(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    op_local_branch_u64(tail_code, ctx, false, ControlBranchKind::If)
}

pub unsafe fn op_i64_local_eqz_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_local_branch_u64(tail_code, ctx, true, ControlBranchKind::If)
}

/// Telomere runtime helper `op_i64_local_br_if_ptr`.
///
/// Stack effect: `internal local branch dispatch`.
/// # Safety
/// - `tail_code` must point at the pointer-bearing operands for this specialized branch.
/// - `ctx` must reference a live execution context for the same validated frame and store.
pub unsafe fn op_i64_local_br_if_ptr(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_local_branch_u64_ptr(tail_code, ctx, false, ControlBranchKind::BrIf)
}

/// Telomere runtime helper `op_i64_local_eqz_br_if_ptr`.
///
/// Stack effect: `internal local branch dispatch`.
/// # Safety
/// - `tail_code` must point at the pointer-bearing operands for this specialized branch.
/// - `ctx` must reference a live execution context for the same validated frame and store.
pub unsafe fn op_i64_local_eqz_br_if_ptr(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_local_branch_u64_ptr(tail_code, ctx, true, ControlBranchKind::BrIf)
}

/// Telomere runtime helper `op_i64_local_if_ptr`.
///
/// Stack effect: `internal local branch dispatch`.
/// # Safety
/// - `tail_code` must point at the pointer-bearing operands for this specialized branch.
/// - `ctx` must reference a live execution context for the same validated frame and store.
pub unsafe fn op_i64_local_if_ptr(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_local_branch_u64_ptr(tail_code, ctx, false, ControlBranchKind::If)
}

/// Telomere runtime helper `op_i64_local_eqz_if_ptr`.
///
/// Stack effect: `internal local branch dispatch`.
/// # Safety
/// - `tail_code` must point at the pointer-bearing operands for this specialized branch.
/// - `ctx` must reference a live execution context for the same validated frame and store.
pub unsafe fn op_i64_local_eqz_if_ptr(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_local_branch_u64_ptr(tail_code, ctx, true, ControlBranchKind::If)
}

#[inline(always)]
unsafe fn op_i32_local_and_imm_branch(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    zero_test: bool,
    branch_kind: ControlBranchKind,
) -> VMResult<()> {
    let local_addr = (*tail_code).operand.local_addr;
    let imm = (*tail_code.add(1)).operand.i32 as u32;
    let cond = (local_u32(ctx.stack, &ctx.local_reference(), local_addr) & imm) == 0;
    let taken = if zero_test { cond } else { !cond };
    let ptr = branch_target_relative(tail_code, ctx, 2, 3, branch_kind, taken);
    call_next(ptr, 0, ctx)
}

#[inline(always)]
unsafe fn op_i32_local_and_imm_branch_ptr(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    zero_test: bool,
    branch_kind: ControlBranchKind,
) -> VMResult<()> {
    let local_addr = (*tail_code).operand.local_addr;
    let imm = (*tail_code.add(1)).operand.i32 as u32;
    let cond = (local_u32(ctx.stack, &ctx.local_reference(), local_addr) & imm) == 0;
    let taken = if zero_test { cond } else { !cond };
    let ptr = branch_target_ptr(tail_code, 2, 3, branch_kind, taken);
    call_next(ptr, 0, ctx)
}

pub unsafe fn op_i32_local_and_imm_br_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_i32_local_and_imm_branch(tail_code, ctx, false, ControlBranchKind::BrIf)
}

pub unsafe fn op_i32_local_and_imm_eqz_br_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_i32_local_and_imm_branch(tail_code, ctx, true, ControlBranchKind::BrIf)
}

pub unsafe fn op_i32_local_and_imm_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_i32_local_and_imm_branch(tail_code, ctx, false, ControlBranchKind::If)
}

pub unsafe fn op_i32_local_and_imm_eqz_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_i32_local_and_imm_branch(tail_code, ctx, true, ControlBranchKind::If)
}

/// Telomere runtime helper `op_i32_local_and_imm_br_if_ptr`.
///
/// Stack effect: `internal local+imm branch dispatch`.
/// # Safety
/// - `tail_code` must point at the pointer-bearing operands for this specialized branch.
/// - `ctx` must reference a live execution context for the same validated frame and store.
pub unsafe fn op_i32_local_and_imm_br_if_ptr(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_i32_local_and_imm_branch_ptr(tail_code, ctx, false, ControlBranchKind::BrIf)
}

/// Telomere runtime helper `op_i32_local_and_imm_eqz_br_if_ptr`.
///
/// Stack effect: `internal local+imm branch dispatch`.
/// # Safety
/// - `tail_code` must point at the pointer-bearing operands for this specialized branch.
/// - `ctx` must reference a live execution context for the same validated frame and store.
pub unsafe fn op_i32_local_and_imm_eqz_br_if_ptr(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_i32_local_and_imm_branch_ptr(tail_code, ctx, true, ControlBranchKind::BrIf)
}

/// Telomere runtime helper `op_i32_local_and_imm_if_ptr`.
///
/// Stack effect: `internal local+imm branch dispatch`.
/// # Safety
/// - `tail_code` must point at the pointer-bearing operands for this specialized branch.
/// - `ctx` must reference a live execution context for the same validated frame and store.
pub unsafe fn op_i32_local_and_imm_if_ptr(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_i32_local_and_imm_branch_ptr(tail_code, ctx, false, ControlBranchKind::If)
}

/// Telomere runtime helper `op_i32_local_and_imm_eqz_if_ptr`.
///
/// Stack effect: `internal local+imm branch dispatch`.
/// # Safety
/// - `tail_code` must point at the pointer-bearing operands for this specialized branch.
/// - `ctx` must reference a live execution context for the same validated frame and store.
pub unsafe fn op_i32_local_and_imm_eqz_if_ptr(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_i32_local_and_imm_branch_ptr(tail_code, ctx, true, ControlBranchKind::If)
}

#[inline(always)]
unsafe fn op_i32_local_addr_load8_u_and_imm_eqz_branch(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    branch_kind: ControlBranchKind,
) -> VMResult<()> {
    let local_addr = (*tail_code).operand.local_addr;
    let memarg = (*tail_code.add(1)).operand.memarg;
    let imm = (*tail_code.add(2)).operand.i32 as u32;
    let start = vm_try!(local_mem_start_from_local(ctx, local_addr, memarg));
    let loaded = u32::from(vm_try!(ctx
        .gc
        .local_read_u8_at(ctx.default_local_memory_id_unchecked(), start)));
    let taken = (loaded & imm) == 0;
    let ptr = branch_target_relative(tail_code, ctx, 3, 4, branch_kind, taken);
    call_next(ptr, 0, ctx)
}

#[inline(always)]
unsafe fn op_i32_local_addr_load8_u_and_imm_eqz_branch_ptr(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    branch_kind: ControlBranchKind,
) -> VMResult<()> {
    let local_addr = (*tail_code).operand.local_addr;
    let memarg = (*tail_code.add(1)).operand.memarg;
    let imm = (*tail_code.add(2)).operand.i32 as u32;
    let start = vm_try!(local_mem_start_from_local(ctx, local_addr, memarg));
    let loaded = u32::from(vm_try!(ctx
        .gc
        .local_read_u8_at(ctx.default_local_memory_id_unchecked(), start)));
    let taken = (loaded & imm) == 0;
    let ptr = branch_target_ptr(tail_code, 3, 4, branch_kind, taken);
    call_next(ptr, 0, ctx)
}

#[cold]
#[inline(never)]
pub unsafe fn op_i32_local_addr_load8_u_and_imm_eqz_br_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_i32_local_addr_load8_u_and_imm_eqz_branch(tail_code, ctx, ControlBranchKind::BrIf)
}

#[cold]
#[inline(never)]
pub unsafe fn op_i32_local_addr_load8_u_and_imm_eqz_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_i32_local_addr_load8_u_and_imm_eqz_branch(tail_code, ctx, ControlBranchKind::If)
}

#[cold]
#[inline(never)]
/// Telomere runtime helper `op_i32_local_addr_load8_u_and_imm_eqz_br_if_ptr`.
///
/// Stack effect: `internal load+mask branch dispatch`.
/// # Safety
/// - `tail_code` must point at the pointer-bearing operands for this specialized branch.
/// - `ctx` must reference a live execution context for the same validated frame and store.
pub unsafe fn op_i32_local_addr_load8_u_and_imm_eqz_br_if_ptr(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_i32_local_addr_load8_u_and_imm_eqz_branch_ptr(tail_code, ctx, ControlBranchKind::BrIf)
}

#[cold]
#[inline(never)]
/// Telomere runtime helper `op_i32_local_addr_load8_u_and_imm_eqz_if_ptr`.
///
/// Stack effect: `internal load+mask branch dispatch`.
/// # Safety
/// - `tail_code` must point at the pointer-bearing operands for this specialized branch.
/// - `ctx` must reference a live execution context for the same validated frame and store.
pub unsafe fn op_i32_local_addr_load8_u_and_imm_eqz_if_ptr(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_i32_local_addr_load8_u_and_imm_eqz_branch_ptr(tail_code, ctx, ControlBranchKind::If)
}

#[inline(always)]
unsafe fn op_seed_tee_eqz_branch_u32<const PTR_TARGET: bool>(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    branch_kind: ControlBranchKind,
) -> VMResult<()> {
    let value = vm_try!(producer_seed_u32(tail_code, ctx));
    write_local_u32(
        ctx.stack,
        &ctx.local_reference(),
        (*tail_code.add(5)).operand.local_addr,
        value,
    );
    let ptr = if PTR_TARGET {
        branch_target_ptr(tail_code, 6, 7, branch_kind, value == 0)
    } else {
        branch_target_relative(tail_code, ctx, 6, 7, branch_kind, value == 0)
    };
    call_next(ptr, 0, ctx)
}

#[inline(always)]
unsafe fn op_seed_tee_eqz_branch_u64<const PTR_TARGET: bool>(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    branch_kind: ControlBranchKind,
) -> VMResult<()> {
    let value = vm_try!(producer_seed_u64(tail_code, ctx));
    write_local_u64(
        ctx.stack,
        &ctx.local_reference(),
        (*tail_code.add(5)).operand.local_addr,
        value,
    );
    let ptr = if PTR_TARGET {
        branch_target_ptr(tail_code, 6, 7, branch_kind, value == 0)
    } else {
        branch_target_relative(tail_code, ctx, 6, 7, branch_kind, value == 0)
    };
    call_next(ptr, 0, ctx)
}

#[inline(always)]
unsafe fn op_seed_tee_imm_compare_branch_u32<const PTR_TARGET: bool>(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    branch_kind: ControlBranchKind,
) -> VMResult<()> {
    let value = vm_try!(producer_seed_u32(tail_code, ctx));
    write_local_u32(
        ctx.stack,
        &ctx.local_reference(),
        (*tail_code.add(5)).operand.local_addr,
        value,
    );
    let taken = i32_compare_eval(
        value,
        (*tail_code.add(6)).operand.u32,
        IntCompareKind::from_raw((*tail_code.add(8)).operand.u32),
    ) != 0;
    let ptr = if PTR_TARGET {
        branch_target_ptr(tail_code, 7, 9, branch_kind, taken)
    } else {
        branch_target_relative(tail_code, ctx, 7, 9, branch_kind, taken)
    };
    call_next(ptr, 0, ctx)
}

#[inline(always)]
unsafe fn op_seed_tee_imm_compare_branch_u64<const PTR_TARGET: bool>(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    branch_kind: ControlBranchKind,
) -> VMResult<()> {
    let value = vm_try!(producer_seed_u64(tail_code, ctx));
    write_local_u64(
        ctx.stack,
        &ctx.local_reference(),
        (*tail_code.add(5)).operand.local_addr,
        value,
    );
    let taken = i64_compare_eval(
        value,
        (*tail_code.add(6)).operand.u64,
        IntCompareKind::from_raw((*tail_code.add(8)).operand.u32),
    ) != 0;
    let ptr = if PTR_TARGET {
        branch_target_ptr(tail_code, 7, 9, branch_kind, taken)
    } else {
        branch_target_relative(tail_code, ctx, 7, 9, branch_kind, taken)
    };
    call_next(ptr, 0, ctx)
}

pub unsafe fn op_i32_seed_tee_eqz_br_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_seed_tee_eqz_branch_u32::<false>(tail_code, ctx, ControlBranchKind::BrIf)
}

pub unsafe fn op_i32_seed_tee_eqz_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_seed_tee_eqz_branch_u32::<false>(tail_code, ctx, ControlBranchKind::If)
}

pub unsafe fn op_i32_seed_tee_eqz_br_if_ptr(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_seed_tee_eqz_branch_u32::<true>(tail_code, ctx, ControlBranchKind::BrIf)
}

pub unsafe fn op_i32_seed_tee_eqz_if_ptr(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_seed_tee_eqz_branch_u32::<true>(tail_code, ctx, ControlBranchKind::If)
}

pub unsafe fn op_i64_seed_tee_eqz_br_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_seed_tee_eqz_branch_u64::<false>(tail_code, ctx, ControlBranchKind::BrIf)
}

pub unsafe fn op_i64_seed_tee_eqz_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_seed_tee_eqz_branch_u64::<false>(tail_code, ctx, ControlBranchKind::If)
}

pub unsafe fn op_i64_seed_tee_eqz_br_if_ptr(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_seed_tee_eqz_branch_u64::<true>(tail_code, ctx, ControlBranchKind::BrIf)
}

pub unsafe fn op_i64_seed_tee_eqz_if_ptr(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_seed_tee_eqz_branch_u64::<true>(tail_code, ctx, ControlBranchKind::If)
}

pub unsafe fn op_i32_seed_tee_imm_compare_br_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_seed_tee_imm_compare_branch_u32::<false>(tail_code, ctx, ControlBranchKind::BrIf)
}

pub unsafe fn op_i32_seed_tee_imm_compare_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_seed_tee_imm_compare_branch_u32::<false>(tail_code, ctx, ControlBranchKind::If)
}

pub unsafe fn op_i32_seed_tee_imm_compare_br_if_ptr(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_seed_tee_imm_compare_branch_u32::<true>(tail_code, ctx, ControlBranchKind::BrIf)
}

pub unsafe fn op_i32_seed_tee_imm_compare_if_ptr(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_seed_tee_imm_compare_branch_u32::<true>(tail_code, ctx, ControlBranchKind::If)
}

pub unsafe fn op_i64_seed_tee_imm_compare_br_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_seed_tee_imm_compare_branch_u64::<false>(tail_code, ctx, ControlBranchKind::BrIf)
}

pub unsafe fn op_i64_seed_tee_imm_compare_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_seed_tee_imm_compare_branch_u64::<false>(tail_code, ctx, ControlBranchKind::If)
}

pub unsafe fn op_i64_seed_tee_imm_compare_br_if_ptr(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_seed_tee_imm_compare_branch_u64::<true>(tail_code, ctx, ControlBranchKind::BrIf)
}

pub unsafe fn op_i64_seed_tee_imm_compare_if_ptr(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_seed_tee_imm_compare_branch_u64::<true>(tail_code, ctx, ControlBranchKind::If)
}

#[inline(always)]
unsafe fn op_seed_imm_and_branch_u32<const ZERO_TEST: bool, const PTR_TARGET: bool>(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    branch_kind: ControlBranchKind,
) -> VMResult<()> {
    let value = vm_try!(producer_seed_u32(tail_code, ctx));
    let imm = (*tail_code.add(5)).operand.u32;
    let cond = (value & imm) == 0;
    let taken = if ZERO_TEST { cond } else { !cond };
    let ptr = if PTR_TARGET {
        branch_target_ptr(tail_code, 6, 7, branch_kind, taken)
    } else {
        branch_target_relative(tail_code, ctx, 6, 7, branch_kind, taken)
    };
    call_next(ptr, 0, ctx)
}

#[inline(always)]
unsafe fn op_seed_imm_and_branch_u64<const ZERO_TEST: bool, const PTR_TARGET: bool>(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    branch_kind: ControlBranchKind,
) -> VMResult<()> {
    let value = vm_try!(producer_seed_u64(tail_code, ctx));
    let imm = (*tail_code.add(5)).operand.u64;
    let cond = (value & imm) == 0;
    let taken = if ZERO_TEST { cond } else { !cond };
    let ptr = if PTR_TARGET {
        branch_target_ptr(tail_code, 6, 7, branch_kind, taken)
    } else {
        branch_target_relative(tail_code, ctx, 6, 7, branch_kind, taken)
    };
    call_next(ptr, 0, ctx)
}

pub unsafe fn op_i32_seed_imm_and_br_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_seed_imm_and_branch_u32::<false, false>(tail_code, ctx, ControlBranchKind::BrIf)
}

pub unsafe fn op_i32_seed_imm_and_eqz_br_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_seed_imm_and_branch_u32::<true, false>(tail_code, ctx, ControlBranchKind::BrIf)
}

pub unsafe fn op_i32_seed_imm_and_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_seed_imm_and_branch_u32::<false, false>(tail_code, ctx, ControlBranchKind::If)
}

pub unsafe fn op_i32_seed_imm_and_eqz_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_seed_imm_and_branch_u32::<true, false>(tail_code, ctx, ControlBranchKind::If)
}

/// Telomere runtime helper `op_i32_seed_imm_and_br_if_ptr`.
///
/// Stack effect: `internal producer+mask branch dispatch`.
/// # Safety
/// - `tail_code` must point at the pointer-bearing operands for this specialized branch.
/// - `ctx` must reference a live execution context for the same validated frame and store.
pub unsafe fn op_i32_seed_imm_and_br_if_ptr(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_seed_imm_and_branch_u32::<false, true>(tail_code, ctx, ControlBranchKind::BrIf)
}

/// Telomere runtime helper `op_i32_seed_imm_and_eqz_br_if_ptr`.
///
/// Stack effect: `internal producer+mask branch dispatch`.
/// # Safety
/// - `tail_code` must point at the pointer-bearing operands for this specialized branch.
/// - `ctx` must reference a live execution context for the same validated frame and store.
pub unsafe fn op_i32_seed_imm_and_eqz_br_if_ptr(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_seed_imm_and_branch_u32::<true, true>(tail_code, ctx, ControlBranchKind::BrIf)
}

/// Telomere runtime helper `op_i32_seed_imm_and_if_ptr`.
///
/// Stack effect: `internal producer+mask branch dispatch`.
/// # Safety
/// - `tail_code` must point at the pointer-bearing operands for this specialized branch.
/// - `ctx` must reference a live execution context for the same validated frame and store.
pub unsafe fn op_i32_seed_imm_and_if_ptr(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_seed_imm_and_branch_u32::<false, true>(tail_code, ctx, ControlBranchKind::If)
}

/// Telomere runtime helper `op_i32_seed_imm_and_eqz_if_ptr`.
///
/// Stack effect: `internal producer+mask branch dispatch`.
/// # Safety
/// - `tail_code` must point at the pointer-bearing operands for this specialized branch.
/// - `ctx` must reference a live execution context for the same validated frame and store.
pub unsafe fn op_i32_seed_imm_and_eqz_if_ptr(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_seed_imm_and_branch_u32::<true, true>(tail_code, ctx, ControlBranchKind::If)
}

pub unsafe fn op_i64_seed_imm_and_br_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_seed_imm_and_branch_u64::<false, false>(tail_code, ctx, ControlBranchKind::BrIf)
}

pub unsafe fn op_i64_seed_imm_and_eqz_br_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_seed_imm_and_branch_u64::<true, false>(tail_code, ctx, ControlBranchKind::BrIf)
}

pub unsafe fn op_i64_seed_imm_and_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_seed_imm_and_branch_u64::<false, false>(tail_code, ctx, ControlBranchKind::If)
}

pub unsafe fn op_i64_seed_imm_and_eqz_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_seed_imm_and_branch_u64::<true, false>(tail_code, ctx, ControlBranchKind::If)
}

/// Telomere runtime helper `op_i64_seed_imm_and_br_if_ptr`.
///
/// Stack effect: `internal producer+mask branch dispatch`.
/// # Safety
/// - `tail_code` must point at the pointer-bearing operands for this specialized branch.
/// - `ctx` must reference a live execution context for the same validated frame and store.
pub unsafe fn op_i64_seed_imm_and_br_if_ptr(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_seed_imm_and_branch_u64::<false, true>(tail_code, ctx, ControlBranchKind::BrIf)
}

/// Telomere runtime helper `op_i64_seed_imm_and_eqz_br_if_ptr`.
///
/// Stack effect: `internal producer+mask branch dispatch`.
/// # Safety
/// - `tail_code` must point at the pointer-bearing operands for this specialized branch.
/// - `ctx` must reference a live execution context for the same validated frame and store.
pub unsafe fn op_i64_seed_imm_and_eqz_br_if_ptr(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_seed_imm_and_branch_u64::<true, true>(tail_code, ctx, ControlBranchKind::BrIf)
}

/// Telomere runtime helper `op_i64_seed_imm_and_if_ptr`.
///
/// Stack effect: `internal producer+mask branch dispatch`.
/// # Safety
/// - `tail_code` must point at the pointer-bearing operands for this specialized branch.
/// - `ctx` must reference a live execution context for the same validated frame and store.
pub unsafe fn op_i64_seed_imm_and_if_ptr(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_seed_imm_and_branch_u64::<false, true>(tail_code, ctx, ControlBranchKind::If)
}

/// Telomere runtime helper `op_i64_seed_imm_and_eqz_if_ptr`.
///
/// Stack effect: `internal producer+mask branch dispatch`.
/// # Safety
/// - `tail_code` must point at the pointer-bearing operands for this specialized branch.
/// - `ctx` must reference a live execution context for the same validated frame and store.
pub unsafe fn op_i64_seed_imm_and_eqz_if_ptr(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    op_seed_imm_and_branch_u64::<true, true>(tail_code, ctx, ControlBranchKind::If)
}

pub unsafe fn op_i32_local_local_ge_u_br_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lhs_local_addr = (*tail_code).operand.local_addr;
    let rhs_local_addr = (*tail_code.add(1)).operand.local_addr;
    let ptr = if local_u32(ctx.stack, &ctx.local_reference(), lhs_local_addr)
        >= local_u32(ctx.stack, &ctx.local_reference(), rhs_local_addr)
    {
        branch_target_relative(tail_code, ctx, 2, 3, ControlBranchKind::BrIf, true)
    } else {
        tail_code.offset(3)
    };
    call_next(ptr, 0, ctx)
}

/// Telomere runtime helper `op_i32_local_local_ge_u_br_if_ptr`.
///
/// Stack effect: `internal compare branch dispatch`.
/// # Safety
/// - `tail_code` must point at the pointer-bearing operands for this specialized branch.
/// - `ctx` must reference a live execution context for the same validated frame and store.
pub unsafe fn op_i32_local_local_ge_u_br_if_ptr(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lhs_local_addr = (*tail_code).operand.local_addr;
    let rhs_local_addr = (*tail_code.add(1)).operand.local_addr;
    let ptr = if local_u32(ctx.stack, &ctx.local_reference(), lhs_local_addr)
        >= local_u32(ctx.stack, &ctx.local_reference(), rhs_local_addr)
    {
        branch_target_ptr(tail_code, 2, 3, ControlBranchKind::BrIf, true)
    } else {
        tail_code.offset(3)
    };
    call_next(ptr, 0, ctx)
}

pub unsafe fn op_i32_local_local_compare_br_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lhs_local = (*tail_code).operand.local_addr;
    let rhs_local = (*tail_code.add(1)).operand.local_addr;
    let kind = IntCompareKind::from_raw((*tail_code.add(3)).operand.u32);
    let ptr = if i32_compare_eval(
        local_u32(ctx.stack, &ctx.local_reference(), lhs_local),
        local_u32(ctx.stack, &ctx.local_reference(), rhs_local),
        kind,
    ) != 0
    {
        branch_target_relative(tail_code, ctx, 2, 4, ControlBranchKind::BrIf, true)
    } else {
        tail_code.offset(4)
    };
    call_next(ptr, 0, ctx)
}

pub unsafe fn op_i32_local_local_compare_br_if_ptr(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lhs_local = (*tail_code).operand.local_addr;
    let rhs_local = (*tail_code.add(1)).operand.local_addr;
    let kind = IntCompareKind::from_raw((*tail_code.add(3)).operand.u32);
    let ptr = if i32_compare_eval(
        local_u32(ctx.stack, &ctx.local_reference(), lhs_local),
        local_u32(ctx.stack, &ctx.local_reference(), rhs_local),
        kind,
    ) != 0
    {
        branch_target_ptr(tail_code, 2, 4, ControlBranchKind::BrIf, true)
    } else {
        tail_code.offset(4)
    };
    call_next(ptr, 0, ctx)
}

/// Telomere runtime helper `op_i32_local_const_compare_br_if`.
///
/// Stack effect: `internal compare branch dispatch`.
/// # Safety
/// - `tail_code` must point at the pointer-bearing operands for this specialized branch.
/// - `ctx` must reference a live execution context for the same validated frame and store.
pub unsafe fn op_i32_local_const_compare_br_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lhs_local = (*tail_code).operand.local_addr;
    let rhs = (*tail_code.add(1)).operand.i32 as u32;
    let kind = IntCompareKind::from_raw((*tail_code.add(3)).operand.u32);
    let ptr = if i32_compare_eval(
        local_u32(ctx.stack, &ctx.local_reference(), lhs_local),
        rhs,
        kind,
    ) != 0
    {
        branch_target_relative(tail_code, ctx, 2, 4, ControlBranchKind::BrIf, true)
    } else {
        tail_code.offset(4)
    };
    call_next(ptr, 0, ctx)
}

/// Telomere runtime helper `op_i32_local_const_compare_br_if_ptr`.
///
/// Stack effect: `internal compare branch dispatch`.
/// # Safety
/// - `tail_code` must point at the pointer-bearing operands for this specialized branch.
/// - `ctx` must reference a live execution context for the same validated frame and store.
pub unsafe fn op_i32_local_const_compare_br_if_ptr(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lhs_local = (*tail_code).operand.local_addr;
    let rhs = (*tail_code.add(1)).operand.i32 as u32;
    let kind = IntCompareKind::from_raw((*tail_code.add(3)).operand.u32);
    let ptr = if i32_compare_eval(
        local_u32(ctx.stack, &ctx.local_reference(), lhs_local),
        rhs,
        kind,
    ) != 0
    {
        branch_target_ptr(tail_code, 2, 4, ControlBranchKind::BrIf, true)
    } else {
        tail_code.offset(4)
    };
    call_next(ptr, 0, ctx)
}

pub unsafe fn op_i64_local_local_compare_br_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lhs_local = (*tail_code).operand.local_addr;
    let rhs_local = (*tail_code.add(1)).operand.local_addr;
    let kind = IntCompareKind::from_raw((*tail_code.add(3)).operand.u32);
    let ptr = if i64_compare_eval(
        local_u64(ctx.stack, &ctx.local_reference(), lhs_local),
        local_u64(ctx.stack, &ctx.local_reference(), rhs_local),
        kind,
    ) != 0
    {
        branch_target_relative(tail_code, ctx, 2, 4, ControlBranchKind::BrIf, true)
    } else {
        tail_code.offset(4)
    };
    call_next(ptr, 0, ctx)
}

/// Telomere runtime helper `op_i64_local_local_compare_br_if_ptr`.
///
/// Stack effect: `internal compare branch dispatch`.
/// # Safety
/// - `tail_code` must point at the pointer-bearing operands for this specialized branch.
/// - `ctx` must reference a live execution context for the same validated frame and store.
pub unsafe fn op_i64_local_local_compare_br_if_ptr(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lhs_local = (*tail_code).operand.local_addr;
    let rhs_local = (*tail_code.add(1)).operand.local_addr;
    let kind = IntCompareKind::from_raw((*tail_code.add(3)).operand.u32);
    let ptr = if i64_compare_eval(
        local_u64(ctx.stack, &ctx.local_reference(), lhs_local),
        local_u64(ctx.stack, &ctx.local_reference(), rhs_local),
        kind,
    ) != 0
    {
        branch_target_ptr(tail_code, 2, 4, ControlBranchKind::BrIf, true)
    } else {
        tail_code.offset(4)
    };
    call_next(ptr, 0, ctx)
}

pub unsafe fn op_i64_local_const_compare_br_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lhs_local = (*tail_code).operand.local_addr;
    let rhs = (*tail_code.add(1)).operand.u64;
    let kind = IntCompareKind::from_raw((*tail_code.add(3)).operand.u32);
    let ptr = if i64_compare_eval(
        local_u64(ctx.stack, &ctx.local_reference(), lhs_local),
        rhs,
        kind,
    ) != 0
    {
        branch_target_relative(tail_code, ctx, 2, 4, ControlBranchKind::BrIf, true)
    } else {
        tail_code.offset(4)
    };
    call_next(ptr, 0, ctx)
}

/// Telomere runtime helper `op_i64_local_const_compare_br_if_ptr`.
///
/// Stack effect: `internal compare branch dispatch`.
/// # Safety
/// - `tail_code` must point at the pointer-bearing operands for this specialized branch.
/// - `ctx` must reference a live execution context for the same validated frame and store.
pub unsafe fn op_i64_local_const_compare_br_if_ptr(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lhs_local = (*tail_code).operand.local_addr;
    let rhs = (*tail_code.add(1)).operand.u64;
    let kind = IntCompareKind::from_raw((*tail_code.add(3)).operand.u32);
    let ptr = if i64_compare_eval(
        local_u64(ctx.stack, &ctx.local_reference(), lhs_local),
        rhs,
        kind,
    ) != 0
    {
        branch_target_ptr(tail_code, 2, 4, ControlBranchKind::BrIf, true)
    } else {
        tail_code.offset(4)
    };
    call_next(ptr, 0, ctx)
}

pub unsafe fn op_f32_local_local_compare_br_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lhs_local = (*tail_code).operand.local_addr;
    let rhs_local = (*tail_code.add(1)).operand.local_addr;
    let kind = FloatCompareKind::from_raw((*tail_code.add(3)).operand.u32);
    let ptr = if f32_compare_eval(
        local_u32(ctx.stack, &ctx.local_reference(), lhs_local),
        local_u32(ctx.stack, &ctx.local_reference(), rhs_local),
        kind,
    ) != 0
    {
        branch_target_relative(tail_code, ctx, 2, 4, ControlBranchKind::BrIf, true)
    } else {
        tail_code.offset(4)
    };
    call_next(ptr, 0, ctx)
}

/// Telomere runtime helper `op_f32_local_local_compare_br_if_ptr`.
///
/// Stack effect: `internal compare branch dispatch`.
/// # Safety
/// - `tail_code` must point at the pointer-bearing operands for this specialized branch.
/// - `ctx` must reference a live execution context for the same validated frame and store.
pub unsafe fn op_f32_local_local_compare_br_if_ptr(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lhs_local = (*tail_code).operand.local_addr;
    let rhs_local = (*tail_code.add(1)).operand.local_addr;
    let kind = FloatCompareKind::from_raw((*tail_code.add(3)).operand.u32);
    let ptr = if f32_compare_eval(
        local_u32(ctx.stack, &ctx.local_reference(), lhs_local),
        local_u32(ctx.stack, &ctx.local_reference(), rhs_local),
        kind,
    ) != 0
    {
        branch_target_ptr(tail_code, 2, 4, ControlBranchKind::BrIf, true)
    } else {
        tail_code.offset(4)
    };
    call_next(ptr, 0, ctx)
}

pub unsafe fn op_f32_local_const_compare_br_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lhs_local = (*tail_code).operand.local_addr;
    let rhs = (*tail_code.add(1)).operand.f32.to_bits();
    let kind = FloatCompareKind::from_raw((*tail_code.add(3)).operand.u32);
    let ptr = if f32_compare_eval(
        local_u32(ctx.stack, &ctx.local_reference(), lhs_local),
        rhs,
        kind,
    ) != 0
    {
        branch_target_relative(tail_code, ctx, 2, 4, ControlBranchKind::BrIf, true)
    } else {
        tail_code.offset(4)
    };
    call_next(ptr, 0, ctx)
}

/// Telomere runtime helper `op_f32_local_const_compare_br_if_ptr`.
///
/// Stack effect: `internal compare branch dispatch`.
/// # Safety
/// - `tail_code` must point at the pointer-bearing operands for this specialized branch.
/// - `ctx` must reference a live execution context for the same validated frame and store.
pub unsafe fn op_f32_local_const_compare_br_if_ptr(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lhs_local = (*tail_code).operand.local_addr;
    let rhs = (*tail_code.add(1)).operand.f32.to_bits();
    let kind = FloatCompareKind::from_raw((*tail_code.add(3)).operand.u32);
    let ptr = if f32_compare_eval(
        local_u32(ctx.stack, &ctx.local_reference(), lhs_local),
        rhs,
        kind,
    ) != 0
    {
        branch_target_ptr(tail_code, 2, 4, ControlBranchKind::BrIf, true)
    } else {
        tail_code.offset(4)
    };
    call_next(ptr, 0, ctx)
}

pub unsafe fn op_f64_local_local_compare_br_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lhs_local = (*tail_code).operand.local_addr;
    let rhs_local = (*tail_code.add(1)).operand.local_addr;
    let kind = FloatCompareKind::from_raw((*tail_code.add(3)).operand.u32);
    let ptr = if f64_compare_eval(
        local_u64(ctx.stack, &ctx.local_reference(), lhs_local),
        local_u64(ctx.stack, &ctx.local_reference(), rhs_local),
        kind,
    ) != 0
    {
        branch_target_relative(tail_code, ctx, 2, 4, ControlBranchKind::BrIf, true)
    } else {
        tail_code.offset(4)
    };
    call_next(ptr, 0, ctx)
}

/// Telomere runtime helper `op_f64_local_local_compare_br_if_ptr`.
///
/// Stack effect: `internal compare branch dispatch`.
/// # Safety
/// - `tail_code` must point at the pointer-bearing operands for this specialized branch.
/// - `ctx` must reference a live execution context for the same validated frame and store.
pub unsafe fn op_f64_local_local_compare_br_if_ptr(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lhs_local = (*tail_code).operand.local_addr;
    let rhs_local = (*tail_code.add(1)).operand.local_addr;
    let kind = FloatCompareKind::from_raw((*tail_code.add(3)).operand.u32);
    let ptr = if f64_compare_eval(
        local_u64(ctx.stack, &ctx.local_reference(), lhs_local),
        local_u64(ctx.stack, &ctx.local_reference(), rhs_local),
        kind,
    ) != 0
    {
        branch_target_ptr(tail_code, 2, 4, ControlBranchKind::BrIf, true)
    } else {
        tail_code.offset(4)
    };
    call_next(ptr, 0, ctx)
}

pub unsafe fn op_f64_local_const_compare_br_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lhs_local = (*tail_code).operand.local_addr;
    let rhs = (*tail_code.add(1)).operand.f64.to_bits();
    let kind = FloatCompareKind::from_raw((*tail_code.add(3)).operand.u32);
    let ptr = if f64_compare_eval(
        local_u64(ctx.stack, &ctx.local_reference(), lhs_local),
        rhs,
        kind,
    ) != 0
    {
        branch_target_relative(tail_code, ctx, 2, 4, ControlBranchKind::BrIf, true)
    } else {
        tail_code.offset(4)
    };
    call_next(ptr, 0, ctx)
}

/// Telomere runtime helper `op_f64_local_const_compare_br_if_ptr`.
///
/// Stack effect: `internal compare branch dispatch`.
/// # Safety
/// - `tail_code` must point at the pointer-bearing operands for this specialized branch.
/// - `ctx` must reference a live execution context for the same validated frame and store.
pub unsafe fn op_f64_local_const_compare_br_if_ptr(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lhs_local = (*tail_code).operand.local_addr;
    let rhs = (*tail_code.add(1)).operand.f64.to_bits();
    let kind = FloatCompareKind::from_raw((*tail_code.add(3)).operand.u32);
    let ptr = if f64_compare_eval(
        local_u64(ctx.stack, &ctx.local_reference(), lhs_local),
        rhs,
        kind,
    ) != 0
    {
        branch_target_ptr(tail_code, 2, 4, ControlBranchKind::BrIf, true)
    } else {
        tail_code.offset(4)
    };
    call_next(ptr, 0, ctx)
}
