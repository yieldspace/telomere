use super::*;

#[derive(Clone, Copy)]
pub(super) enum ControlBranchKind {
    BrIf,
    If,
}

#[derive(Clone, Copy)]
pub(super) enum NarrowCopyKind {
    Load8Store8,
    Load16Store16,
}

pub(super) fn collect_jump_targets(
    decoded: &[DecodedInstruction],
    raw_len: usize,
    raw_instrs: &[Instr],
) -> JumpTargetBitmap {
    let mut targets = JumpTargetBitmap::with_raw_len(raw_len);
    for instruction in decoded {
        let raw = instruction.raw(raw_instrs);
        let op = unsafe { raw[0].op };
        if raw.len() >= 2
            && (std::ptr::fn_addr_eq(op, vm::op_br as crate::common::Op)
                || std::ptr::fn_addr_eq(op, vm::op_br_if as crate::common::Op)
                || std::ptr::fn_addr_eq(op, vm::op_if as crate::common::Op)
                || std::ptr::fn_addr_eq(op, vm::op_else as crate::common::Op)
                || std::ptr::fn_addr_eq(op, vm::op_return as crate::common::Op))
        {
            targets.mark(unsafe { raw[1].operand.jump_addr as usize });
            continue;
        }
        if raw.len() >= 3 && std::ptr::fn_addr_eq(op, vm::op_br_table as crate::common::Op) {
            let table_size = unsafe { raw[1].operand.u32 as usize };
            for target in &raw[2..=table_size + 2] {
                targets.mark(unsafe { target.operand.jump_addr as usize });
            }
        }
    }
    targets
}

pub(super) fn fuse_superinstructions(
    decoded: Vec<DecodedInstruction>,
    jump_targets: &JumpTargetBitmap,
) -> Vec<OptimizedInstruction> {
    let mut optimized = Vec::with_capacity(decoded.len());
    let mut index = 0;

    while index < decoded.len() {
        if let Some((consumed, fused)) = try_matchers(
            PRODUCER_IMM_AND_BRANCH_MATCHERS,
            decoded.as_slice(),
            index,
            jump_targets,
        ) {
            optimized.push(fused);
            index += consumed;
            continue;
        }
        if let Some((consumed, fused)) = try_matchers(
            PRODUCER_COMPARE_BRANCH_MATCHERS,
            decoded.as_slice(),
            index,
            jump_targets,
        ) {
            optimized.push(fused);
            index += consumed;
            continue;
        }
        if let Some((consumed, fused)) = try_matchers(
            PRODUCER_COMPARE_SELECT_MATCHERS,
            decoded.as_slice(),
            index,
            jump_targets,
        ) {
            optimized.push(fused);
            index += consumed;
            continue;
        }
        if let Some((consumed, fused)) = try_matchers(
            PRODUCER_IMM_SCALAR_SET_TEE_MATCHERS,
            decoded.as_slice(),
            index,
            jump_targets,
        ) {
            optimized.push(fused);
            index += consumed;
            continue;
        }
        if let Some((consumed, fused)) = try_matchers(
            PRODUCER_TEE_EQZ_BRANCH_MATCHERS,
            decoded.as_slice(),
            index,
            jump_targets,
        ) {
            optimized.push(fused);
            index += consumed;
            continue;
        }
        if let Some((consumed, fused)) = try_matchers(
            PRODUCER_TEE_IMM_COMPARE_BRANCH_MATCHERS,
            decoded.as_slice(),
            index,
            jump_targets,
        ) {
            optimized.push(fused);
            index += consumed;
            continue;
        }
        if let Some((consumed, fused)) = try_matchers(
            PRODUCER_TEE_IMM_SCALAR_SET_TEE_MATCHERS,
            decoded.as_slice(),
            index,
            jump_targets,
        ) {
            optimized.push(fused);
            index += consumed;
            continue;
        }
        if let Some((consumed, fused)) = try_matchers(
            PRODUCER_TEE_SELECT_MATCHERS,
            decoded.as_slice(),
            index,
            jump_targets,
        ) {
            optimized.push(fused);
            index += consumed;
            continue;
        }
        if let Some((consumed, fused)) = try_matchers(
            COMPARE_TEE_SELECT_MATCHERS,
            decoded.as_slice(),
            index,
            jump_targets,
        ) {
            optimized.push(fused);
            index += consumed;
            continue;
        }
        if let Some((consumed, fused)) = try_matchers(
            LOCAL_IMM_SET_TEE_MATCHERS,
            decoded.as_slice(),
            index,
            jump_targets,
        ) {
            optimized.push(fused);
            index += consumed;
            continue;
        }
        if let Some((consumed, fused)) = try_matchers(
            LOCAL_LOCAL_SET_TEE_MATCHERS,
            decoded.as_slice(),
            index,
            jump_targets,
        ) {
            optimized.push(fused);
            index += consumed;
            continue;
        }
        if let Some((consumed, fused)) = try_matchers(
            COMPARE_SET_TEE_MATCHERS,
            decoded.as_slice(),
            index,
            jump_targets,
        ) {
            optimized.push(fused);
            index += consumed;
            continue;
        }
        if let Some((consumed, fused)) = try_matchers(
            COMPARE_SELECT_MATCHERS,
            decoded.as_slice(),
            index,
            jump_targets,
        ) {
            optimized.push(fused);
            index += consumed;
            continue;
        }
        if let Some((consumed, fused)) = try_matchers(
            LOAD_MASK_BRANCH_MATCHERS,
            decoded.as_slice(),
            index,
            jump_targets,
        ) {
            optimized.push(fused);
            index += consumed;
            continue;
        }
        if let Some((consumed, fused)) =
            try_matchers(BRANCH_MATCHERS, decoded.as_slice(), index, jump_targets)
        {
            optimized.push(fused);
            index += consumed;
            continue;
        }
        if let Some((consumed, fused)) = try_matchers(
            CONST_SET_TEE_MATCHERS,
            decoded.as_slice(),
            index,
            jump_targets,
        ) {
            optimized.push(fused);
            index += consumed;
            continue;
        }
        if let Some((consumed, fused)) =
            try_matchers(LOCAL_COPY_MATCHERS, decoded.as_slice(), index, jump_targets)
        {
            optimized.push(fused);
            index += consumed;
            continue;
        }
        if let Some((consumed, fused)) =
            try_matchers(CONST_ADDR_MATCHERS, decoded.as_slice(), index, jump_targets)
        {
            optimized.push(fused);
            index += consumed;
            continue;
        }
        if let Some((consumed, fused)) = try_matchers(
            LOAD_MODIFY_STORE_MATCHERS,
            decoded.as_slice(),
            index,
            jump_targets,
        ) {
            optimized.push(fused);
            index += consumed;
            continue;
        }
        if let Some((consumed, fused)) = try_matchers(
            LOAD_STORE_NARROW_COPY_MATCHERS,
            decoded.as_slice(),
            index,
            jump_targets,
        ) {
            optimized.push(fused);
            index += consumed;
            continue;
        }
        if let Some((consumed, fused)) = try_matchers(
            LOCAL_ADDR_LOAD_MATCHERS,
            decoded.as_slice(),
            index,
            jump_targets,
        ) {
            optimized.push(fused);
            index += consumed;
            continue;
        }
        if let Some((consumed, fused)) = try_matchers(
            LOCAL_LOCAL_STORE_MATCHERS,
            decoded.as_slice(),
            index,
            jump_targets,
        ) {
            optimized.push(fused);
            index += consumed;
            continue;
        }
        if let Some((consumed, fused)) = try_matchers(
            LOCAL_IMM_PUSH_MATCHERS,
            decoded.as_slice(),
            index,
            jump_targets,
        ) {
            optimized.push(fused);
            index += consumed;
            continue;
        }
        if let Some((consumed, fused)) = try_matchers(
            LOCAL_LOCAL_PUSH_MATCHERS,
            decoded.as_slice(),
            index,
            jump_targets,
        ) {
            optimized.push(fused);
            index += consumed;
            continue;
        }

        optimized.push(OptimizedInstruction::raw(InstructionSpan::from_old_range(
            &decoded[index].old_range,
        )));
        index += 1;
    }

    optimized
}

fn try_matchers(
    matchers: &[Matcher],
    decoded: &[DecodedInstruction],
    index: usize,
    jump_targets: &JumpTargetBitmap,
) -> Option<MatchOutcome> {
    for matcher in matchers {
        if let Some(outcome) = matcher(decoded, index, jump_targets) {
            return Some(outcome);
        }
    }
    None
}

fn sequence_crosses_jump_targets(
    decoded: &[DecodedInstruction],
    jump_targets: &JumpTargetBitmap,
    mut range: Range<usize>,
) -> bool {
    range.any(|idx| jump_targets.contains_raw(decoded[idx].old_range.start as u32))
}

pub(super) fn same_width(lhs: ValueSize, rhs: ValueSize) -> bool {
    matches!(
        (lhs, rhs),
        (ValueSize::Byte4, ValueSize::Byte4)
            | (ValueSize::Byte8, ValueSize::Byte8)
            | (ValueSize::Byte16, ValueSize::Byte16)
    )
}

pub(super) fn local_get(kind: DecodedKind) -> Option<(ValueSize, u32)> {
    match kind {
        DecodedKind::LocalGet(width, local_addr) => Some((width, local_addr)),
        _ => None,
    }
}

pub(super) fn local_set_tee(kind: DecodedKind) -> Option<(ValueSize, u32, bool)> {
    match kind {
        DecodedKind::LocalSet(width, local_addr) => Some((width, local_addr, false)),
        DecodedKind::LocalTee(width, local_addr) => Some((width, local_addr, true)),
        _ => None,
    }
}

pub(super) fn select_width(kind: DecodedKind) -> Option<ValueSize> {
    match kind {
        DecodedKind::Select(width) => Some(width),
        _ => None,
    }
}

pub(super) fn scalar_matches_const(op: TypedScalarOp, value: TypedConst) -> bool {
    matches!(
        (op, value),
        (TypedScalarOp::I32(_), TypedConst::I32(_))
            | (TypedScalarOp::I64(_), TypedConst::I64(_))
            | (TypedScalarOp::F32(_), TypedConst::F32(_))
            | (TypedScalarOp::F64(_), TypedConst::F64(_))
    )
}

pub(super) fn compare_matches_const(op: TypedCompareOp, value: TypedConst) -> bool {
    matches!(
        (op, value),
        (TypedCompareOp::I32(_), TypedConst::I32(_))
            | (TypedCompareOp::I64(_), TypedConst::I64(_))
            | (TypedCompareOp::F32(_), TypedConst::F32(_))
            | (TypedCompareOp::F64(_), TypedConst::F64(_))
    )
}

pub(super) fn is_existing_i32_local_imm_fastpath(op: TypedScalarOp) -> bool {
    matches!(
        op,
        TypedScalarOp::I32(
            I32ScalarKind::Add
                | I32ScalarKind::Sub
                | I32ScalarKind::And
                | I32ScalarKind::Shl
                | I32ScalarKind::ShrU
        )
    )
}

pub(super) fn is_existing_i32_local_local_fastpath(op: TypedScalarOp) -> bool {
    matches!(op, TypedScalarOp::I32(I32ScalarKind::Add))
}

pub(super) fn is_integer_scalar(op: TypedScalarOp) -> bool {
    matches!(op, TypedScalarOp::I32(_) | TypedScalarOp::I64(_))
}

fn is_supported_tee_consumer_scalar(op: TypedScalarOp) -> bool {
    matches!(
        op,
        TypedScalarOp::I32(
            I32ScalarKind::Add
                | I32ScalarKind::Sub
                | I32ScalarKind::And
                | I32ScalarKind::Or
                | I32ScalarKind::Xor
                | I32ScalarKind::Shl
                | I32ScalarKind::ShrS
                | I32ScalarKind::ShrU
        ) | TypedScalarOp::I64(
            I64ScalarKind::Add
                | I64ScalarKind::Sub
                | I64ScalarKind::And
                | I64ScalarKind::Or
                | I64ScalarKind::Xor
                | I64ScalarKind::Shl
                | I64ScalarKind::ShrS
                | I64ScalarKind::ShrU
        )
    )
}

fn is_integer_compare(op: TypedCompareOp) -> bool {
    matches!(op, TypedCompareOp::I32(_) | TypedCompareOp::I64(_))
}

pub(super) fn match_producer_imm_and_branch(
    decoded: &[DecodedInstruction],
    index: usize,
    jump_targets: &JumpTargetBitmap,
) -> Option<MatchOutcome> {
    let seed_match = match_producer_seed(decoded, index)?;
    if !has_nontrivial_seed(&seed_match) {
        return None;
    }
    let const_index = index + seed_match.consumed;
    let scalar_index = const_index + 1;
    let branch_or_eqz = decoded.get(scalar_index + 1)?;
    let rhs_const = match decoded.get(const_index)?.kind {
        DecodedKind::Const(value) => value,
        _ => return None,
    };
    let scalar_op = match decoded.get(scalar_index)?.kind {
        DecodedKind::Scalar(op) => op,
        _ => return None,
    };
    let width = seed_match.seed.width();
    if !same_width(width, rhs_const.width())
        || !same_width(width, scalar_op.width())
        || !scalar_matches_const(scalar_op, rhs_const)
        || !matches!(
            scalar_op,
            TypedScalarOp::I32(I32ScalarKind::And) | TypedScalarOp::I64(I64ScalarKind::And)
        )
    {
        return None;
    }

    let (zero_test, branch_kind, target_old, branch_index, end) = match branch_or_eqz.kind {
        DecodedKind::Eqz(eqz_width) => {
            let branch = decoded.get(scalar_index + 2)?;
            if !same_width(width, eqz_width) {
                return None;
            }
            let (branch_kind, target_old) = match branch.kind {
                DecodedKind::BrIf(target) => (ControlBranchKind::BrIf, target),
                DecodedKind::If(target) => (ControlBranchKind::If, target),
                _ => return None,
            };
            (
                true,
                branch_kind,
                target_old,
                scalar_index + 2,
                branch.old_range.end,
            )
        }
        DecodedKind::BrIf(target) => (
            false,
            ControlBranchKind::BrIf,
            target,
            scalar_index + 1,
            branch_or_eqz.old_range.end,
        ),
        DecodedKind::If(target) => (
            false,
            ControlBranchKind::If,
            target,
            scalar_index + 1,
            branch_or_eqz.old_range.end,
        ),
        _ => return None,
    };
    if sequence_crosses_jump_targets(decoded, jump_targets, index + 1..branch_index + 1) {
        return None;
    }
    Some((
        branch_index - index + 1,
        OptimizedInstruction::ProducerImmAndBranch {
            span: InstructionSpan::new(decoded[index].old_range.start, end),
            seed: seed_match.seed,
            rhs_const,
            width,
            target_old,
            zero_test,
            branch_kind,
        },
    ))
}

pub(super) fn match_producer_imm_scalar_set_tee(
    decoded: &[DecodedInstruction],
    index: usize,
    jump_targets: &JumpTargetBitmap,
) -> Option<MatchOutcome> {
    let seed_match = match_producer_seed(decoded, index)?;
    if !has_nontrivial_seed(&seed_match) {
        return None;
    }
    let const_index = index + seed_match.consumed;
    let scalar_index = const_index + 1;
    let set_tee_index = const_index + 2;
    let rhs_const = match decoded.get(const_index)?.kind {
        DecodedKind::Const(value) => value,
        _ => return None,
    };
    let scalar_op = match decoded.get(scalar_index)?.kind {
        DecodedKind::Scalar(op) => op,
        _ => return None,
    };
    let set_tee = decoded.get(set_tee_index)?;
    if sequence_crosses_jump_targets(decoded, jump_targets, index + 1..set_tee_index + 1) {
        return None;
    }
    let (dst_width, dst_local, dst_tee) = local_set_tee(set_tee.kind)?;
    let width = seed_match.seed.width();
    if !same_width(width, rhs_const.width())
        || !same_width(width, scalar_op.width())
        || !same_width(width, dst_width)
        || !scalar_matches_const(scalar_op, rhs_const)
        || !is_supported_tee_consumer_scalar(scalar_op)
    {
        return None;
    }
    Some((
        set_tee_index - index + 1,
        OptimizedInstruction::ProducerImmScalarSetTee {
            span: InstructionSpan::new(decoded[index].old_range.start, set_tee.old_range.end),
            seed: seed_match.seed,
            rhs_const,
            dst_local,
            dst_tee,
            op: scalar_op,
        },
    ))
}

pub(super) fn match_producer_local_compare_branch(
    decoded: &[DecodedInstruction],
    index: usize,
    jump_targets: &JumpTargetBitmap,
) -> Option<MatchOutcome> {
    let seed_match = match_producer_seed(decoded, index)?;
    if !has_nontrivial_seed(&seed_match) {
        return None;
    }
    let rhs_index = index + seed_match.consumed;
    let compare_index = rhs_index + 1;
    let branch_index = rhs_index + 2;
    let (rhs_width, rhs_local_addr) = local_get(decoded.get(rhs_index)?.kind)?;
    let compare_op = match decoded.get(compare_index)?.kind {
        DecodedKind::Compare(op) => op,
        _ => return None,
    };
    let branch = decoded.get(branch_index)?;
    let (branch_kind, target_old) = match branch.kind {
        DecodedKind::BrIf(target) => (ControlBranchKind::BrIf, target),
        DecodedKind::If(target) => (ControlBranchKind::If, target),
        _ => return None,
    };
    let seed = seed_match.seed;
    if sequence_crosses_jump_targets(decoded, jump_targets, index + 1..branch_index + 1) {
        return None;
    }
    if !same_width(seed.width(), rhs_width)
        || !same_width(seed.width(), compare_op.width())
        || (is_float_compare(compare_op) && !is_float_load_seed_for_compare(seed, compare_op))
    {
        return None;
    }
    Some((
        branch_index - index + 1,
        OptimizedInstruction::ProducerCompareBranchLocal {
            span: InstructionSpan::new(decoded[index].old_range.start, branch.old_range.end),
            seed,
            rhs_local_addr,
            target_old,
            op: compare_op,
            branch_kind,
        },
    ))
}

pub(super) fn match_producer_const_compare_branch(
    decoded: &[DecodedInstruction],
    index: usize,
    jump_targets: &JumpTargetBitmap,
) -> Option<MatchOutcome> {
    let seed_match = match_producer_seed(decoded, index)?;
    if !has_nontrivial_seed(&seed_match) {
        return None;
    }
    let const_index = index + seed_match.consumed;
    let compare_index = const_index + 1;
    let branch_index = const_index + 2;
    let rhs_const = match decoded.get(const_index)?.kind {
        DecodedKind::Const(value) => value,
        _ => return None,
    };
    let compare_op = match decoded.get(compare_index)?.kind {
        DecodedKind::Compare(op) => op,
        _ => return None,
    };
    let branch = decoded.get(branch_index)?;
    let (branch_kind, target_old) = match branch.kind {
        DecodedKind::BrIf(target) => (ControlBranchKind::BrIf, target),
        DecodedKind::If(target) => (ControlBranchKind::If, target),
        _ => return None,
    };
    let seed = seed_match.seed;
    if sequence_crosses_jump_targets(decoded, jump_targets, index + 1..branch_index + 1) {
        return None;
    }
    if !same_width(seed.width(), rhs_const.width())
        || !same_width(seed.width(), compare_op.width())
        || !compare_matches_const(compare_op, rhs_const)
        || (is_float_compare(compare_op) && !is_float_load_seed_for_compare(seed, compare_op))
    {
        return None;
    }
    Some((
        branch_index - index + 1,
        OptimizedInstruction::ProducerCompareBranchConst {
            span: InstructionSpan::new(decoded[index].old_range.start, branch.old_range.end),
            seed,
            rhs_const,
            target_old,
            op: compare_op,
            branch_kind,
        },
    ))
}

pub(super) fn match_producer_local_compare_select(
    decoded: &[DecodedInstruction],
    index: usize,
    jump_targets: &JumpTargetBitmap,
) -> Option<MatchOutcome> {
    let seed_match = match_producer_seed(decoded, index)?;
    if !has_nontrivial_seed(&seed_match) {
        return None;
    }
    let rhs_index = index + seed_match.consumed;
    let compare_index = rhs_index + 1;
    let select_index = rhs_index + 2;
    let (rhs_width, rhs_local_addr) = local_get(decoded.get(rhs_index)?.kind)?;
    let compare_op = match decoded.get(compare_index)?.kind {
        DecodedKind::Compare(op) => op,
        _ => return None,
    };
    let select_width = select_width(decoded.get(select_index)?.kind)?;
    let seed = seed_match.seed;
    if sequence_crosses_jump_targets(decoded, jump_targets, index + 1..select_index + 1) {
        return None;
    }
    if !same_width(seed.width(), rhs_width)
        || !same_width(seed.width(), compare_op.width())
        || !matches!(select_width, ValueSize::Byte4 | ValueSize::Byte8)
        || (is_float_compare(compare_op) && !is_float_load_seed_for_compare(seed, compare_op))
    {
        return None;
    }
    Some((
        select_index - index + 1,
        OptimizedInstruction::ProducerCompareSelectLocal {
            span: InstructionSpan::new(
                decoded[index].old_range.start,
                decoded[select_index].old_range.end,
            ),
            seed,
            rhs_local_addr,
            select_width,
            op: compare_op,
        },
    ))
}

pub(super) fn match_producer_const_compare_select(
    decoded: &[DecodedInstruction],
    index: usize,
    jump_targets: &JumpTargetBitmap,
) -> Option<MatchOutcome> {
    let seed_match = match_producer_seed(decoded, index)?;
    if !has_nontrivial_seed(&seed_match) {
        return None;
    }
    let const_index = index + seed_match.consumed;
    let compare_index = const_index + 1;
    let select_index = const_index + 2;
    let rhs_const = match decoded.get(const_index)?.kind {
        DecodedKind::Const(value) => value,
        _ => return None,
    };
    let compare_op = match decoded.get(compare_index)?.kind {
        DecodedKind::Compare(op) => op,
        _ => return None,
    };
    let select_width = select_width(decoded.get(select_index)?.kind)?;
    let seed = seed_match.seed;
    if sequence_crosses_jump_targets(decoded, jump_targets, index + 1..select_index + 1) {
        return None;
    }
    if !same_width(seed.width(), rhs_const.width())
        || !same_width(seed.width(), compare_op.width())
        || !compare_matches_const(compare_op, rhs_const)
        || !matches!(select_width, ValueSize::Byte4 | ValueSize::Byte8)
        || (is_float_compare(compare_op) && !is_float_load_seed_for_compare(seed, compare_op))
    {
        return None;
    }
    Some((
        select_index - index + 1,
        OptimizedInstruction::ProducerCompareSelectConst {
            span: InstructionSpan::new(
                decoded[index].old_range.start,
                decoded[select_index].old_range.end,
            ),
            seed,
            rhs_const,
            select_width,
            op: compare_op,
        },
    ))
}

pub(super) fn match_producer_tee_eqz_branch(
    decoded: &[DecodedInstruction],
    index: usize,
    jump_targets: &JumpTargetBitmap,
) -> Option<MatchOutcome> {
    let seed_match = match_producer_seed(decoded, index)?;
    let tee_index = index + seed_match.consumed;
    let eqz_index = tee_index + 1;
    let branch_index = tee_index + 2;
    let tee = decoded.get(tee_index)?;
    let eqz = decoded.get(eqz_index)?;
    let branch = decoded.get(branch_index)?;
    if sequence_crosses_jump_targets(decoded, jump_targets, index + 1..branch_index + 1) {
        return None;
    }
    let (tee_width, tee_local_addr, tee) = local_set_tee(tee.kind)?;
    if !tee || !same_width(seed_match.seed.width(), tee_width) {
        return None;
    }
    let eqz_width = match eqz.kind {
        DecodedKind::Eqz(width) => width,
        _ => return None,
    };
    if !same_width(tee_width, eqz_width) {
        return None;
    }
    let (branch_kind, target_old) = match branch.kind {
        DecodedKind::BrIf(target) => (ControlBranchKind::BrIf, target),
        DecodedKind::If(target) => (ControlBranchKind::If, target),
        _ => return None,
    };
    Some((
        seed_match.consumed + 3,
        OptimizedInstruction::ProducerTeeEqzBranch {
            span: InstructionSpan::new(decoded[index].old_range.start, branch.old_range.end),
            seed: seed_match.seed,
            tee_local_addr,
            target_old,
            width: tee_width,
            branch_kind,
        },
    ))
}

pub(super) fn match_producer_tee_imm_compare_branch(
    decoded: &[DecodedInstruction],
    index: usize,
    jump_targets: &JumpTargetBitmap,
) -> Option<MatchOutcome> {
    let seed_match = match_producer_seed(decoded, index)?;
    let tee_index = index + seed_match.consumed;
    let const_index = tee_index + 1;
    let compare_index = tee_index + 2;
    let branch_index = tee_index + 3;
    let tee = decoded.get(tee_index)?;
    let rhs_const = match decoded.get(const_index)?.kind {
        DecodedKind::Const(value) => value,
        _ => return None,
    };
    let compare_op = match decoded.get(compare_index)?.kind {
        DecodedKind::Compare(op) => op,
        _ => return None,
    };
    let branch = decoded.get(branch_index)?;
    if sequence_crosses_jump_targets(decoded, jump_targets, index + 1..branch_index + 1) {
        return None;
    }
    let (tee_width, tee_local_addr, tee) = local_set_tee(tee.kind)?;
    if !tee
        || !same_width(seed_match.seed.width(), tee_width)
        || !same_width(tee_width, rhs_const.width())
        || !same_width(tee_width, compare_op.width())
        || !is_integer_compare(compare_op)
        || !compare_matches_const(compare_op, rhs_const)
    {
        return None;
    }
    let (branch_kind, target_old) = match branch.kind {
        DecodedKind::BrIf(target) => (ControlBranchKind::BrIf, target),
        DecodedKind::If(target) => (ControlBranchKind::If, target),
        _ => return None,
    };
    Some((
        seed_match.consumed + 4,
        OptimizedInstruction::ProducerTeeImmCompareBranch {
            span: InstructionSpan::new(decoded[index].old_range.start, branch.old_range.end),
            seed: seed_match.seed,
            tee_local_addr,
            rhs_const,
            target_old,
            op: compare_op,
            branch_kind,
        },
    ))
}

pub(super) fn match_producer_tee_imm_scalar_set_tee(
    decoded: &[DecodedInstruction],
    index: usize,
    jump_targets: &JumpTargetBitmap,
) -> Option<MatchOutcome> {
    let seed_match = match_producer_seed(decoded, index)?;
    let tee_index = index + seed_match.consumed;
    let const_index = tee_index + 1;
    let scalar_index = tee_index + 2;
    let set_tee_index = tee_index + 3;
    let tee = decoded.get(tee_index)?;
    let rhs_const = match decoded.get(const_index)?.kind {
        DecodedKind::Const(value) => value,
        _ => return None,
    };
    let scalar_op = match decoded.get(scalar_index)?.kind {
        DecodedKind::Scalar(op) => op,
        _ => return None,
    };
    let set_tee = decoded.get(set_tee_index)?;
    if sequence_crosses_jump_targets(decoded, jump_targets, index + 1..set_tee_index + 1) {
        return None;
    }
    let (tee_width, tee_local_addr, tee) = local_set_tee(tee.kind)?;
    let (dst_width, dst_local, dst_tee) = local_set_tee(set_tee.kind)?;
    if !tee
        || !same_width(seed_match.seed.width(), tee_width)
        || !same_width(tee_width, rhs_const.width())
        || !same_width(tee_width, scalar_op.width())
        || !same_width(tee_width, dst_width)
        || !is_supported_tee_consumer_scalar(scalar_op)
        || !scalar_matches_const(scalar_op, rhs_const)
    {
        return None;
    }
    Some((
        seed_match.consumed + 4,
        OptimizedInstruction::ProducerTeeImmScalarSetTee {
            span: InstructionSpan::new(
                decoded[index].old_range.start,
                decoded[set_tee_index].old_range.end,
            ),
            seed: seed_match.seed,
            tee_local_addr,
            rhs_const,
            dst_local,
            dst_tee,
            op: scalar_op,
        },
    ))
}

pub(super) fn match_producer_tee_const_self_select(
    decoded: &[DecodedInstruction],
    index: usize,
    jump_targets: &JumpTargetBitmap,
) -> Option<MatchOutcome> {
    let seed_match = match_producer_seed(decoded, index)?;
    let tee_index = index + seed_match.consumed;
    let const_index = tee_index + 1;
    let get_index = tee_index + 2;
    let select_index = tee_index + 3;
    let tee = decoded.get(tee_index)?;
    let rhs_const = match decoded.get(const_index)?.kind {
        DecodedKind::Const(value) => value,
        _ => return None,
    };
    let (tee_width, tee_local_addr, tee) = local_set_tee(tee.kind)?;
    let (self_width, self_local_addr) = local_get(decoded.get(get_index)?.kind)?;
    let select_width = select_width(decoded.get(select_index)?.kind)?;
    if sequence_crosses_jump_targets(decoded, jump_targets, index + 1..select_index + 1) {
        return None;
    }
    if !tee
        || tee_local_addr != self_local_addr
        || !same_width(seed_match.seed.width(), tee_width)
        || !same_width(tee_width, rhs_const.width())
        || !same_width(tee_width, self_width)
        || !same_width(tee_width, select_width)
    {
        return None;
    }
    Some((
        seed_match.consumed + 4,
        OptimizedInstruction::ProducerTeeConstSelfSelect {
            span: InstructionSpan::new(
                decoded[index].old_range.start,
                decoded[select_index].old_range.end,
            ),
            seed: seed_match.seed,
            tee_local_addr,
            rhs_const,
            width: tee_width,
        },
    ))
}

pub(super) fn match_local_local_compare_tee_select(
    decoded: &[DecodedInstruction],
    index: usize,
    jump_targets: &JumpTargetBitmap,
) -> Option<MatchOutcome> {
    let [first, second, third, fourth, fifth] = decoded.get(index..index + 5)? else {
        return None;
    };
    if sequence_crosses_jump_targets(decoded, jump_targets, index + 1..index + 5) {
        return None;
    }
    let (lhs_width, lhs_local_addr) = local_get(first.kind)?;
    let (rhs_width, rhs_local_addr) = local_get(second.kind)?;
    let compare_op = match third.kind {
        DecodedKind::Compare(op) => op,
        _ => return None,
    };
    let (tee_width, tee_local_addr, tee) = local_set_tee(fourth.kind)?;
    let select_width = select_width(fifth.kind)?;
    if !tee
        || !same_width(lhs_width, rhs_width)
        || !same_width(lhs_width, compare_op.width())
        || !same_width(tee_width, ValueSize::Byte4)
        || !matches!(lhs_width, ValueSize::Byte4 | ValueSize::Byte8)
        || !matches!(select_width, ValueSize::Byte4 | ValueSize::Byte8)
        || !is_integer_compare(compare_op)
    {
        return None;
    }
    Some((
        5,
        OptimizedInstruction::CompareTeeSelectLocal {
            span: InstructionSpan::new(first.old_range.start, fifth.old_range.end),
            lhs_local_addr,
            rhs_local_addr,
            tee_local_addr,
            select_width,
            op: compare_op,
        },
    ))
}

pub(super) fn match_local_const_compare_tee_select(
    decoded: &[DecodedInstruction],
    index: usize,
    jump_targets: &JumpTargetBitmap,
) -> Option<MatchOutcome> {
    let [first, second, third, fourth, fifth] = decoded.get(index..index + 5)? else {
        return None;
    };
    if sequence_crosses_jump_targets(decoded, jump_targets, index + 1..index + 5) {
        return None;
    }
    let (lhs_width, lhs_local_addr) = local_get(first.kind)?;
    let rhs_const = match second.kind {
        DecodedKind::Const(value) => value,
        _ => return None,
    };
    let compare_op = match third.kind {
        DecodedKind::Compare(op) => op,
        _ => return None,
    };
    let (tee_width, tee_local_addr, tee) = local_set_tee(fourth.kind)?;
    let select_width = select_width(fifth.kind)?;
    if !tee
        || !same_width(lhs_width, compare_op.width())
        || !same_width(lhs_width, rhs_const.width())
        || !same_width(tee_width, ValueSize::Byte4)
        || !matches!(lhs_width, ValueSize::Byte4 | ValueSize::Byte8)
        || !matches!(select_width, ValueSize::Byte4 | ValueSize::Byte8)
        || !is_integer_compare(compare_op)
        || !compare_matches_const(compare_op, rhs_const)
    {
        return None;
    }
    Some((
        5,
        OptimizedInstruction::CompareTeeSelectConst {
            span: InstructionSpan::new(first.old_range.start, fifth.old_range.end),
            lhs_local_addr,
            rhs_const,
            tee_local_addr,
            select_width,
            op: compare_op,
        },
    ))
}

pub(super) fn match_local_imm_scalar_set_tee(
    decoded: &[DecodedInstruction],
    index: usize,
    jump_targets: &JumpTargetBitmap,
) -> Option<MatchOutcome> {
    let [first, second, third, fourth] = decoded.get(index..index + 4)? else {
        return None;
    };
    if sequence_crosses_jump_targets(decoded, jump_targets, index + 1..index + 4) {
        return None;
    }
    let (src_width, src_local) = local_get(first.kind)?;
    if !matches!(src_width, ValueSize::Byte4 | ValueSize::Byte8) {
        return None;
    }
    let imm = match second.kind {
        DecodedKind::Const(value) => value,
        _ => return None,
    };
    let op = match third.kind {
        DecodedKind::Scalar(op) => op,
        _ => return None,
    };
    let (dst_width, dst_local, tee) = local_set_tee(fourth.kind)?;

    if !same_width(src_width, imm.width())
        || !same_width(src_width, op.width())
        || !same_width(src_width, dst_width)
        || !scalar_matches_const(op, imm)
    {
        return None;
    }

    Some((
        4,
        OptimizedInstruction::LocalImmSetTee {
            span: InstructionSpan::new(first.old_range.start, fourth.old_range.end),
            src_local,
            imm,
            dst_local,
            tee,
            op,
        },
    ))
}

pub(super) fn match_local_copy(
    decoded: &[DecodedInstruction],
    index: usize,
    jump_targets: &JumpTargetBitmap,
) -> Option<MatchOutcome> {
    let [first, second] = decoded.get(index..index + 2)? else {
        return None;
    };
    if sequence_crosses_jump_targets(decoded, jump_targets, index + 1..index + 2) {
        return None;
    }
    let (src_width, src_local) = local_get(first.kind)?;
    let (dst_width, dst_local, tee) = local_set_tee(second.kind)?;
    if !matches!(src_width, ValueSize::Byte4 | ValueSize::Byte8)
        || !same_width(src_width, dst_width)
    {
        return None;
    }

    Some((
        2,
        OptimizedInstruction::LocalCopy {
            span: InstructionSpan::new(first.old_range.start, second.old_range.end),
            src_local,
            dst_local,
            width: src_width,
            tee,
        },
    ))
}

pub(super) fn match_const_set_tee(
    decoded: &[DecodedInstruction],
    index: usize,
    jump_targets: &JumpTargetBitmap,
) -> Option<MatchOutcome> {
    let [first, second] = decoded.get(index..index + 2)? else {
        return None;
    };
    if sequence_crosses_jump_targets(decoded, jump_targets, index + 1..index + 2) {
        return None;
    }
    let value = match first.kind {
        DecodedKind::Const(value) => value,
        _ => return None,
    };
    let (dst_width, dst_local, tee) = local_set_tee(second.kind)?;
    if !matches!(dst_width, ValueSize::Byte4 | ValueSize::Byte8)
        || !same_width(value.width(), dst_width)
    {
        return None;
    }

    Some((
        2,
        OptimizedInstruction::ConstSetTee {
            span: InstructionSpan::new(first.old_range.start, second.old_range.end),
            value,
            dst_local,
            tee,
        },
    ))
}

pub(super) fn match_local_imm_scalar_push(
    decoded: &[DecodedInstruction],
    index: usize,
    jump_targets: &JumpTargetBitmap,
) -> Option<MatchOutcome> {
    let [first, second, third] = decoded.get(index..index + 3)? else {
        return None;
    };
    if sequence_crosses_jump_targets(decoded, jump_targets, index + 1..index + 3) {
        return None;
    }
    let (lhs_width, src_local) = local_get(first.kind)?;
    let imm = match second.kind {
        DecodedKind::Const(value) => value,
        _ => return None,
    };
    let op = match third.kind {
        DecodedKind::Scalar(op) => op,
        _ => return None,
    };

    if !same_width(lhs_width, ValueSize::Byte4)
        || !same_width(lhs_width, imm.width())
        || !same_width(lhs_width, op.width())
        || !matches!(imm, TypedConst::I32(_))
        || !matches!(op, TypedScalarOp::I32(_))
    {
        return None;
    }

    Some((
        3,
        OptimizedInstruction::LocalImmPush {
            span: InstructionSpan::new(first.old_range.start, third.old_range.end),
            src_local,
            imm,
            op,
        },
    ))
}

pub(super) fn match_local_local_scalar_set_tee(
    decoded: &[DecodedInstruction],
    index: usize,
    jump_targets: &JumpTargetBitmap,
) -> Option<MatchOutcome> {
    let [first, second, third, fourth] = decoded.get(index..index + 4)? else {
        return None;
    };
    if sequence_crosses_jump_targets(decoded, jump_targets, index + 1..index + 4) {
        return None;
    }
    let (lhs_width, lhs_local_addr) = local_get(first.kind)?;
    let (rhs_width, rhs_local_addr) = local_get(second.kind)?;
    let op = match third.kind {
        DecodedKind::Scalar(op) => op,
        _ => return None,
    };
    let (dst_width, dst_local, tee) = local_set_tee(fourth.kind)?;

    if !matches!(lhs_width, ValueSize::Byte4 | ValueSize::Byte8)
        || !same_width(lhs_width, rhs_width)
        || !same_width(lhs_width, op.width())
        || !same_width(lhs_width, dst_width)
    {
        return None;
    }

    Some((
        4,
        OptimizedInstruction::LocalLocalSetTee {
            span: InstructionSpan::new(first.old_range.start, fourth.old_range.end),
            lhs_local_addr,
            rhs_local_addr,
            dst_local,
            tee,
            op,
        },
    ))
}

pub(super) fn match_local_local_scalar_push(
    decoded: &[DecodedInstruction],
    index: usize,
    jump_targets: &JumpTargetBitmap,
) -> Option<MatchOutcome> {
    let [first, second, third] = decoded.get(index..index + 3)? else {
        return None;
    };
    if sequence_crosses_jump_targets(decoded, jump_targets, index + 1..index + 3) {
        return None;
    }
    let (lhs_width, lhs_local_addr) = local_get(first.kind)?;
    let (rhs_width, rhs_local_addr) = local_get(second.kind)?;
    let op = match third.kind {
        DecodedKind::Scalar(op) => op,
        _ => return None,
    };

    if !same_width(lhs_width, ValueSize::Byte4)
        || !same_width(lhs_width, rhs_width)
        || !same_width(lhs_width, op.width())
        || !matches!(op, TypedScalarOp::I32(_))
    {
        return None;
    }

    Some((
        3,
        OptimizedInstruction::LocalLocalPush {
            span: InstructionSpan::new(first.old_range.start, third.old_range.end),
            lhs_local_addr,
            rhs_local_addr,
            op,
        },
    ))
}

pub(super) fn match_i32_local_and_imm_branch(
    decoded: &[DecodedInstruction],
    index: usize,
    jump_targets: &JumpTargetBitmap,
) -> Option<MatchOutcome> {
    let [first, second, third, fourth] = decoded.get(index..index + 4)? else {
        return None;
    };
    let (width, local_addr) = local_get(first.kind)?;
    let imm = match second.kind {
        DecodedKind::Const(TypedConst::I32(value)) => value,
        _ => return None,
    };
    if !same_width(width, ValueSize::Byte4)
        || !matches!(
            third.kind,
            DecodedKind::Scalar(TypedScalarOp::I32(I32ScalarKind::And))
        )
    {
        return None;
    }

    let (zero_test, branch_kind, target_old, end, consumed) = match fourth.kind {
        DecodedKind::Eqz(ValueSize::Byte4) => {
            let fifth = decoded.get(index + 4)?;
            if sequence_crosses_jump_targets(decoded, jump_targets, index + 1..index + 5) {
                return None;
            }
            match fifth.kind {
                DecodedKind::BrIf(target) => (
                    true,
                    ControlBranchKind::BrIf,
                    target,
                    fifth.old_range.end,
                    5,
                ),
                DecodedKind::If(target) => {
                    (true, ControlBranchKind::If, target, fifth.old_range.end, 5)
                }
                _ => return None,
            }
        }
        DecodedKind::BrIf(target) => {
            if sequence_crosses_jump_targets(decoded, jump_targets, index + 1..index + 4) {
                return None;
            }
            (
                false,
                ControlBranchKind::BrIf,
                target,
                fourth.old_range.end,
                4,
            )
        }
        DecodedKind::If(target) => {
            if sequence_crosses_jump_targets(decoded, jump_targets, index + 1..index + 4) {
                return None;
            }
            (
                false,
                ControlBranchKind::If,
                target,
                fourth.old_range.end,
                4,
            )
        }
        _ => return None,
    };

    Some((
        consumed,
        OptimizedInstruction::I32LocalAndImmBranch {
            span: InstructionSpan::new(first.old_range.start, end),
            local_addr,
            imm,
            target_old,
            zero_test,
            branch_kind,
        },
    ))
}

pub(super) fn match_i32_local_addr_load8_u_and_imm_eqz_branch(
    decoded: &[DecodedInstruction],
    index: usize,
    jump_targets: &JumpTargetBitmap,
) -> Option<MatchOutcome> {
    let [first, second, third, fourth, fifth, sixth] = decoded.get(index..index + 6)? else {
        return None;
    };
    if sequence_crosses_jump_targets(decoded, jump_targets, index + 1..index + 6) {
        return None;
    }
    let (addr_width, local_addr) = local_get(first.kind)?;
    let (load_op, memarg) = match second.kind {
        DecodedKind::Load(op, memarg) => (op, memarg),
        _ => return None,
    };
    let imm = match third.kind {
        DecodedKind::Const(TypedConst::I32(value)) => value,
        _ => return None,
    };
    if !same_width(addr_width, ValueSize::Byte4)
        || !matches!(load_op, TypedLoadOp::Bits4(Load4Kind::I32Load8U))
        || !matches!(
            fourth.kind,
            DecodedKind::Scalar(TypedScalarOp::I32(I32ScalarKind::And))
        )
        || !matches!(fifth.kind, DecodedKind::Eqz(ValueSize::Byte4))
    {
        return None;
    }
    let (branch_kind, target_old) = match sixth.kind {
        DecodedKind::BrIf(target) => (ControlBranchKind::BrIf, target),
        DecodedKind::If(target) => (ControlBranchKind::If, target),
        _ => return None,
    };

    Some((
        6,
        OptimizedInstruction::I32LocalAddrLoad8UAndImmEqzBranch {
            span: InstructionSpan::new(first.old_range.start, sixth.old_range.end),
            local_addr,
            memarg,
            imm,
            target_old,
            branch_kind,
        },
    ))
}

pub(super) fn match_local_branch(
    decoded: &[DecodedInstruction],
    index: usize,
    jump_targets: &JumpTargetBitmap,
) -> Option<MatchOutcome> {
    let [first, second] = decoded.get(index..index + 2)? else {
        return None;
    };
    let (width, local_addr) = local_get(first.kind)?;
    if !matches!(width, ValueSize::Byte4 | ValueSize::Byte8) {
        return None;
    }

    let (zero_test, branch_kind, target_old, end, consumed) = match second.kind {
        DecodedKind::Eqz(eqz_width) => {
            let third = decoded.get(index + 2)?;
            if !same_width(width, eqz_width)
                || sequence_crosses_jump_targets(decoded, jump_targets, index + 1..index + 3)
            {
                return None;
            }
            match third.kind {
                DecodedKind::BrIf(target) => (
                    true,
                    ControlBranchKind::BrIf,
                    target,
                    third.old_range.end,
                    3,
                ),
                DecodedKind::If(target) => {
                    (true, ControlBranchKind::If, target, third.old_range.end, 3)
                }
                _ => return None,
            }
        }
        DecodedKind::BrIf(target) => {
            if sequence_crosses_jump_targets(decoded, jump_targets, index + 1..index + 2) {
                return None;
            }
            (
                false,
                ControlBranchKind::BrIf,
                target,
                second.old_range.end,
                2,
            )
        }
        DecodedKind::If(target) => {
            if sequence_crosses_jump_targets(decoded, jump_targets, index + 1..index + 2) {
                return None;
            }
            (
                false,
                ControlBranchKind::If,
                target,
                second.old_range.end,
                2,
            )
        }
        _ => return None,
    };

    Some((
        consumed,
        OptimizedInstruction::LocalBranch {
            span: InstructionSpan::new(first.old_range.start, end),
            local_addr,
            target_old,
            width,
            zero_test,
            branch_kind,
        },
    ))
}

pub(super) fn match_local_local_ge_u_br_if(
    decoded: &[DecodedInstruction],
    index: usize,
    jump_targets: &JumpTargetBitmap,
) -> Option<MatchOutcome> {
    let [first, second, third, fourth] = decoded.get(index..index + 4)? else {
        return None;
    };
    if sequence_crosses_jump_targets(decoded, jump_targets, index + 1..index + 4) {
        return None;
    }
    let (lhs_width, lhs_local_addr) = local_get(first.kind)?;
    let (rhs_width, rhs_local_addr) = local_get(second.kind)?;
    let target_old = match fourth.kind {
        DecodedKind::BrIf(target) => target,
        _ => return None,
    };
    if !same_width(lhs_width, ValueSize::Byte4) || !same_width(rhs_width, ValueSize::Byte4) {
        return None;
    }
    if !matches!(
        third.kind,
        DecodedKind::Compare(TypedCompareOp::I32(IntCompareKind::GeU))
    ) {
        return None;
    }

    Some((
        4,
        OptimizedInstruction::I32LocalLocalGeUBrIf {
            span: InstructionSpan::new(first.old_range.start, fourth.old_range.end),
            lhs_local_addr,
            rhs_local_addr,
            target_old,
        },
    ))
}

pub(super) fn match_local_local_compare_set_tee(
    decoded: &[DecodedInstruction],
    index: usize,
    jump_targets: &JumpTargetBitmap,
) -> Option<MatchOutcome> {
    let [first, second, third, fourth] = decoded.get(index..index + 4)? else {
        return None;
    };
    if sequence_crosses_jump_targets(decoded, jump_targets, index + 1..index + 4) {
        return None;
    }
    let (lhs_width, lhs_local_addr) = local_get(first.kind)?;
    let (rhs_width, rhs_local_addr) = local_get(second.kind)?;
    let op = match third.kind {
        DecodedKind::Compare(op) => op,
        _ => return None,
    };
    let (dst_width, dst_local, tee) = local_set_tee(fourth.kind)?;
    if !same_width(lhs_width, rhs_width)
        || !same_width(lhs_width, op.width())
        || !same_width(dst_width, ValueSize::Byte4)
        || !matches!(lhs_width, ValueSize::Byte4 | ValueSize::Byte8)
    {
        return None;
    }

    Some((
        4,
        OptimizedInstruction::CompareSetTeeLocal {
            span: InstructionSpan::new(first.old_range.start, fourth.old_range.end),
            lhs_local_addr,
            rhs_local_addr,
            dst_local,
            tee,
            op,
        },
    ))
}

pub(super) fn match_local_const_compare_set_tee(
    decoded: &[DecodedInstruction],
    index: usize,
    jump_targets: &JumpTargetBitmap,
) -> Option<MatchOutcome> {
    let [first, second, third, fourth] = decoded.get(index..index + 4)? else {
        return None;
    };
    if sequence_crosses_jump_targets(decoded, jump_targets, index + 1..index + 4) {
        return None;
    }
    let (lhs_width, lhs_local_addr) = local_get(first.kind)?;
    let rhs_const = match second.kind {
        DecodedKind::Const(value) => value,
        _ => return None,
    };
    let op = match third.kind {
        DecodedKind::Compare(op) => op,
        _ => return None,
    };
    let (dst_width, dst_local, tee) = local_set_tee(fourth.kind)?;
    if !same_width(lhs_width, op.width())
        || !same_width(lhs_width, rhs_const.width())
        || !same_width(dst_width, ValueSize::Byte4)
        || !matches!(lhs_width, ValueSize::Byte4 | ValueSize::Byte8)
        || !compare_matches_const(op, rhs_const)
    {
        return None;
    }

    Some((
        4,
        OptimizedInstruction::CompareSetTeeConst {
            span: InstructionSpan::new(first.old_range.start, fourth.old_range.end),
            lhs_local_addr,
            rhs_const,
            dst_local,
            tee,
            op,
        },
    ))
}

pub(super) fn match_local_local_compare_br_if(
    decoded: &[DecodedInstruction],
    index: usize,
    jump_targets: &JumpTargetBitmap,
) -> Option<MatchOutcome> {
    let [first, second, third, fourth] = decoded.get(index..index + 4)? else {
        return None;
    };
    if sequence_crosses_jump_targets(decoded, jump_targets, index + 1..index + 4) {
        return None;
    }
    let (lhs_width, lhs_local_addr) = local_get(first.kind)?;
    let (rhs_width, rhs_local_addr) = local_get(second.kind)?;
    let op = match third.kind {
        DecodedKind::Compare(op) => op,
        _ => return None,
    };
    let target_old = match fourth.kind {
        DecodedKind::BrIf(target) => target,
        _ => return None,
    };
    if !same_width(lhs_width, rhs_width)
        || !same_width(lhs_width, op.width())
        || !matches!(lhs_width, ValueSize::Byte4 | ValueSize::Byte8)
    {
        return None;
    }

    Some((
        4,
        OptimizedInstruction::CompareBrIfLocal {
            span: InstructionSpan::new(first.old_range.start, fourth.old_range.end),
            lhs_local_addr,
            rhs_local_addr,
            target_old,
            op,
        },
    ))
}

pub(super) fn match_local_const_compare_br_if(
    decoded: &[DecodedInstruction],
    index: usize,
    jump_targets: &JumpTargetBitmap,
) -> Option<MatchOutcome> {
    let [first, second, third, fourth] = decoded.get(index..index + 4)? else {
        return None;
    };
    if sequence_crosses_jump_targets(decoded, jump_targets, index + 1..index + 4) {
        return None;
    }
    let (lhs_width, lhs_local_addr) = local_get(first.kind)?;
    let rhs_const = match second.kind {
        DecodedKind::Const(value) => value,
        _ => return None,
    };
    let op = match third.kind {
        DecodedKind::Compare(op) => op,
        _ => return None,
    };
    let target_old = match fourth.kind {
        DecodedKind::BrIf(target) => target,
        _ => return None,
    };
    if !same_width(lhs_width, op.width())
        || !same_width(lhs_width, rhs_const.width())
        || !matches!(lhs_width, ValueSize::Byte4 | ValueSize::Byte8)
        || !compare_matches_const(op, rhs_const)
    {
        return None;
    }

    Some((
        4,
        OptimizedInstruction::CompareBrIfConst {
            span: InstructionSpan::new(first.old_range.start, fourth.old_range.end),
            lhs_local_addr,
            rhs_const,
            target_old,
            op,
        },
    ))
}

pub(super) fn match_local_local_compare_select(
    decoded: &[DecodedInstruction],
    index: usize,
    jump_targets: &JumpTargetBitmap,
) -> Option<MatchOutcome> {
    let [first, second, third, fourth] = decoded.get(index..index + 4)? else {
        return None;
    };
    if sequence_crosses_jump_targets(decoded, jump_targets, index + 1..index + 4) {
        return None;
    }
    let (lhs_width, lhs_local_addr) = local_get(first.kind)?;
    let (rhs_width, rhs_local_addr) = local_get(second.kind)?;
    let op = match third.kind {
        DecodedKind::Compare(op) => op,
        _ => return None,
    };
    let select_width = select_width(fourth.kind)?;
    if !same_width(lhs_width, rhs_width)
        || !same_width(lhs_width, op.width())
        || !matches!(lhs_width, ValueSize::Byte4 | ValueSize::Byte8)
        || !matches!(select_width, ValueSize::Byte4 | ValueSize::Byte8)
    {
        return None;
    }

    Some((
        4,
        OptimizedInstruction::CompareSelectLocal {
            span: InstructionSpan::new(first.old_range.start, fourth.old_range.end),
            lhs_local_addr,
            rhs_local_addr,
            select_width,
            op,
        },
    ))
}

pub(super) fn match_local_const_compare_select(
    decoded: &[DecodedInstruction],
    index: usize,
    jump_targets: &JumpTargetBitmap,
) -> Option<MatchOutcome> {
    let [first, second, third, fourth] = decoded.get(index..index + 4)? else {
        return None;
    };
    if sequence_crosses_jump_targets(decoded, jump_targets, index + 1..index + 4) {
        return None;
    }
    let (lhs_width, lhs_local_addr) = local_get(first.kind)?;
    let rhs_const = match second.kind {
        DecodedKind::Const(value) => value,
        _ => return None,
    };
    let op = match third.kind {
        DecodedKind::Compare(op) => op,
        _ => return None,
    };
    let select_width = select_width(fourth.kind)?;
    if !same_width(lhs_width, op.width())
        || !same_width(lhs_width, rhs_const.width())
        || !matches!(lhs_width, ValueSize::Byte4 | ValueSize::Byte8)
        || !matches!(select_width, ValueSize::Byte4 | ValueSize::Byte8)
        || !compare_matches_const(op, rhs_const)
    {
        return None;
    }

    Some((
        4,
        OptimizedInstruction::CompareSelectConst {
            span: InstructionSpan::new(first.old_range.start, fourth.old_range.end),
            lhs_local_addr,
            rhs_const,
            select_width,
            op,
        },
    ))
}

pub(super) fn match_const_load(
    decoded: &[DecodedInstruction],
    index: usize,
    jump_targets: &JumpTargetBitmap,
) -> Option<MatchOutcome> {
    let [first, second] = decoded.get(index..index + 2)? else {
        return None;
    };
    if sequence_crosses_jump_targets(decoded, jump_targets, index + 1..index + 2) {
        return None;
    }
    let addr = match first.kind {
        DecodedKind::Const(TypedConst::I32(value)) => value,
        _ => return None,
    };
    let (op, memarg) = match second.kind {
        DecodedKind::Load(op, memarg) => (op, memarg),
        _ => return None,
    };
    let start = match compute_memory_offset(memarg, addr as u32) {
        VMResult::Success(start) => u32::try_from(start).ok()?,
        _ => return None,
    };

    Some((
        2,
        OptimizedInstruction::LoadConstLocal {
            span: InstructionSpan::new(first.old_range.start, second.old_range.end),
            start,
            op,
        },
    ))
}

pub(super) fn match_const_local_store(
    decoded: &[DecodedInstruction],
    index: usize,
    jump_targets: &JumpTargetBitmap,
) -> Option<MatchOutcome> {
    let [first, second, third] = decoded.get(index..index + 3)? else {
        return None;
    };
    if sequence_crosses_jump_targets(decoded, jump_targets, index + 1..index + 3) {
        return None;
    }
    let addr = match first.kind {
        DecodedKind::Const(TypedConst::I32(value)) => value,
        _ => return None,
    };
    let (value_width, value_local_addr) = local_get(second.kind)?;
    let (op, memarg) = match third.kind {
        DecodedKind::Store(op, memarg) => (op, memarg),
        _ => return None,
    };
    if !same_width(value_width, op.value_width()) {
        return None;
    }
    let start = match compute_memory_offset(memarg, addr as u32) {
        VMResult::Success(start) => u32::try_from(start).ok()?,
        _ => return None,
    };

    Some((
        3,
        OptimizedInstruction::StoreConstLocal {
            span: InstructionSpan::new(first.old_range.start, third.old_range.end),
            start,
            value_local_addr,
            op,
        },
    ))
}

pub(super) fn match_local_addr_load(
    decoded: &[DecodedInstruction],
    index: usize,
    jump_targets: &JumpTargetBitmap,
) -> Option<MatchOutcome> {
    let [first, second] = decoded.get(index..index + 2)? else {
        return None;
    };
    if sequence_crosses_jump_targets(decoded, jump_targets, index + 1..index + 2) {
        return None;
    }
    let (addr_width, local_addr) = local_get(first.kind)?;
    let (op, memarg) = match second.kind {
        DecodedKind::Load(op, memarg) => (op, memarg),
        _ => return None,
    };
    if !same_width(addr_width, ValueSize::Byte4) {
        return None;
    }

    Some((
        2,
        OptimizedInstruction::LocalAddrLoad {
            span: InstructionSpan::new(first.old_range.start, second.old_range.end),
            local_addr,
            memarg,
            op,
        },
    ))
}

pub(super) fn match_local_imm_addr_load(
    decoded: &[DecodedInstruction],
    index: usize,
    jump_targets: &JumpTargetBitmap,
) -> Option<MatchOutcome> {
    let [first, second, third, fourth] = decoded.get(index..index + 4)? else {
        return None;
    };
    if sequence_crosses_jump_targets(decoded, jump_targets, index + 1..index + 4) {
        return None;
    }
    let (addr_width, local_addr) = local_get(first.kind)?;
    let imm = match second.kind {
        DecodedKind::Const(TypedConst::I32(value)) => value,
        _ => return None,
    };
    if !matches!(
        third.kind,
        DecodedKind::Scalar(TypedScalarOp::I32(I32ScalarKind::Add))
    ) {
        return None;
    }
    let (op, memarg) = match fourth.kind {
        DecodedKind::Load(op, memarg) => (op, memarg),
        _ => return None,
    };
    if !same_width(addr_width, ValueSize::Byte4) || !op.uses_dedicated_local_addr() {
        return None;
    }

    Some((
        4,
        OptimizedInstruction::LocalImmAddrLoad {
            span: InstructionSpan::new(first.old_range.start, fourth.old_range.end),
            local_addr,
            imm,
            memarg,
            op,
        },
    ))
}

pub(super) fn match_i32_local_local_load_tee_add_imm_store(
    decoded: &[DecodedInstruction],
    index: usize,
    jump_targets: &JumpTargetBitmap,
) -> Option<MatchOutcome> {
    let [first, second, third, fourth, fifth, sixth, seventh] = decoded.get(index..index + 7)?
    else {
        return None;
    };
    if sequence_crosses_jump_targets(decoded, jump_targets, index + 1..index + 7) {
        return None;
    }
    let (store_addr_width, store_addr_local_addr) = local_get(first.kind)?;
    let (load_addr_width, load_addr_local_addr) = local_get(second.kind)?;
    let (load_op, load_memarg) = match third.kind {
        DecodedKind::Load(op, memarg) => (op, memarg),
        _ => return None,
    };
    let (tee_width, tee_local_addr, tee) = local_set_tee(fourth.kind)?;
    let imm = match fifth.kind {
        DecodedKind::Const(TypedConst::I32(value)) => value,
        _ => return None,
    };
    let (store_op, store_memarg) = match seventh.kind {
        DecodedKind::Store(op, memarg) => (op, memarg),
        _ => return None,
    };
    if !same_width(store_addr_width, ValueSize::Byte4)
        || !same_width(load_addr_width, ValueSize::Byte4)
        || !matches!(load_op, TypedLoadOp::Bits4(Load4Kind::I32))
        || !same_width(tee_width, ValueSize::Byte4)
        || !tee
        || !matches!(
            sixth.kind,
            DecodedKind::Scalar(TypedScalarOp::I32(I32ScalarKind::Add))
        )
        || !matches!(store_op, TypedStoreOp::Bits4(Store4Kind::I32))
    {
        return None;
    }

    Some((
        7,
        OptimizedInstruction::I32LocalLocalLoadTeeAddImmStore {
            span: InstructionSpan::new(first.old_range.start, seventh.old_range.end),
            store_addr_local_addr,
            load_addr_local_addr,
            tee_local_addr,
            imm,
            load_memarg,
            store_memarg,
        },
    ))
}

pub(super) fn match_local_local_store(
    decoded: &[DecodedInstruction],
    index: usize,
    jump_targets: &JumpTargetBitmap,
) -> Option<MatchOutcome> {
    let [first, second, third] = decoded.get(index..index + 3)? else {
        return None;
    };
    if sequence_crosses_jump_targets(decoded, jump_targets, index + 1..index + 3) {
        return None;
    }
    let (addr_width, addr_local_addr) = local_get(first.kind)?;
    let (value_width, value_local_addr) = local_get(second.kind)?;
    let (op, memarg) = match third.kind {
        DecodedKind::Store(op, memarg) => (op, memarg),
        _ => return None,
    };
    if !same_width(addr_width, ValueSize::Byte4) || !same_width(value_width, op.value_width()) {
        return None;
    }

    Some((
        3,
        OptimizedInstruction::LocalLocalStore {
            span: InstructionSpan::new(first.old_range.start, third.old_range.end),
            addr_local_addr,
            value_local_addr,
            memarg,
            op,
        },
    ))
}

pub(super) fn match_i32_local_local_load8_u_store8_copy(
    decoded: &[DecodedInstruction],
    index: usize,
    jump_targets: &JumpTargetBitmap,
) -> Option<MatchOutcome> {
    match_i32_local_local_narrow_copy(decoded, index, jump_targets, NarrowCopyKind::Load8Store8)
}

pub(super) fn match_i32_local_local_load16_u_store16_copy(
    decoded: &[DecodedInstruction],
    index: usize,
    jump_targets: &JumpTargetBitmap,
) -> Option<MatchOutcome> {
    match_i32_local_local_narrow_copy(decoded, index, jump_targets, NarrowCopyKind::Load16Store16)
}

pub(super) fn match_i32_local_local_narrow_copy(
    decoded: &[DecodedInstruction],
    index: usize,
    jump_targets: &JumpTargetBitmap,
    kind: NarrowCopyKind,
) -> Option<MatchOutcome> {
    let [first, second, third, fourth] = decoded.get(index..index + 4)? else {
        return None;
    };
    if sequence_crosses_jump_targets(decoded, jump_targets, index + 1..index + 4) {
        return None;
    }
    let (dst_width, dst_local_addr) = local_get(first.kind)?;
    let (src_width, src_local_addr) = local_get(second.kind)?;
    let (load_op, load_memarg) = match third.kind {
        DecodedKind::Load(op, memarg) => (op, memarg),
        _ => return None,
    };
    let (store_op, store_memarg) = match fourth.kind {
        DecodedKind::Store(op, memarg) => (op, memarg),
        _ => return None,
    };

    let op_matches = matches!(
        (kind, load_op, store_op),
        (
            NarrowCopyKind::Load8Store8,
            TypedLoadOp::Bits4(Load4Kind::I32Load8U),
            TypedStoreOp::Bits4(Store4Kind::I32Store8),
        ) | (
            NarrowCopyKind::Load16Store16,
            TypedLoadOp::Bits4(Load4Kind::I32Load16U),
            TypedStoreOp::Bits4(Store4Kind::I32Store16),
        )
    );

    if !same_width(dst_width, ValueSize::Byte4)
        || !same_width(src_width, ValueSize::Byte4)
        || !op_matches
    {
        return None;
    }

    Some((
        4,
        OptimizedInstruction::I32LocalLocalNarrowCopy {
            span: InstructionSpan::new(first.old_range.start, fourth.old_range.end),
            dst_local_addr,
            src_local_addr,
            load_memarg,
            store_memarg,
            kind,
        },
    ))
}

pub(super) fn match_local_imm_local_store(
    decoded: &[DecodedInstruction],
    index: usize,
    jump_targets: &JumpTargetBitmap,
) -> Option<MatchOutcome> {
    let [first, second, third, fourth, fifth] = decoded.get(index..index + 5)? else {
        return None;
    };
    if sequence_crosses_jump_targets(decoded, jump_targets, index + 1..index + 5) {
        return None;
    }
    let (addr_width, addr_local_addr) = local_get(first.kind)?;
    let imm = match second.kind {
        DecodedKind::Const(TypedConst::I32(value)) => value,
        _ => return None,
    };
    if !matches!(
        third.kind,
        DecodedKind::Scalar(TypedScalarOp::I32(I32ScalarKind::Add))
    ) {
        return None;
    }
    let (value_width, value_local_addr) = local_get(fourth.kind)?;
    let (op, memarg) = match fifth.kind {
        DecodedKind::Store(op, memarg) => (op, memarg),
        _ => return None,
    };
    if !same_width(addr_width, ValueSize::Byte4)
        || !same_width(value_width, op.value_width())
        || !op.uses_dedicated_local_local()
    {
        return None;
    }

    Some((
        5,
        OptimizedInstruction::LocalImmLocalStore {
            span: InstructionSpan::new(first.old_range.start, fifth.old_range.end),
            addr_local_addr,
            imm,
            value_local_addr,
            memarg,
            op,
        },
    ))
}
