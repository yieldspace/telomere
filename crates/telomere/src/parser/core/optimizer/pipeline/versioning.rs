use std::collections::HashMap;

use super::{
    analysis::AnalysisResults,
    ir::{CanonBlock, CanonFunc, CanonInst},
    select::{
        scalar_const_base_load_family, scalar_const_base_store_family,
        scalar_local_base_load_family, scalar_local_base_store_family,
        scalar_local_scaled_index_load_family, scalar_local_scaled_index_store_family,
        scalar_memory_store_type, BlockVersion, BlockVersionKind, KernelBlock, KernelFunction,
        KernelOp, VersionFact, VersionKey,
    },
};
use crate::{
    common::{
        decode_local_binop32_kind, LocalBinop32Op, LocalFastRhsShape, LoweredOperand, MemArg, Op,
        Operand,
    },
    runtime::vm,
};

const MAX_SPECIALIZED_CLONES_PER_BLOCK: usize = 2;

pub(crate) fn apply(
    mut kernel: KernelFunction,
    func: &CanonFunc,
    _analysis: &AnalysisResults,
) -> KernelFunction {
    let selected_keys = select_specialization_keys(func, &kernel);
    let mut clone_for_key = HashMap::<(usize, VersionKey), usize>::new();
    let mut extra_blocks = Vec::new();

    for (block_id, keys) in selected_keys {
        for key in keys {
            let mut block = kernel.blocks[block_id].clone();
            block.block_id = kernel.blocks.len() + extra_blocks.len();
            block.label = block.block_id;
            block.version = BlockVersion {
                kind: BlockVersionKind::Specialized,
                key: key.clone(),
            };
            let fallthrough = fallthrough_target(func, block_id);
            block.ops = specialize_block_ops(&block.ops, &key, fallthrough);
            if let Some(fallthrough) = fallthrough {
                block.ops.push(branch_op(fallthrough));
            }
            relabel_first_op(&mut block);
            clone_for_key.insert((block_id, key), block.label);
            extra_blocks.push(block);
        }
    }

    for block in &mut kernel.blocks {
        rewrite_explicit_targets(block, func, &clone_for_key);
        fuse_search_loop_ops(block);
        relabel_first_op(block);
    }

    for block in &mut extra_blocks {
        fuse_search_loop_ops(block);
        relabel_first_op(block);
    }

    kernel.blocks.extend(extra_blocks);
    kernel
}

pub(crate) fn verify(kernel: &KernelFunction) -> bool {
    !kernel.blocks.is_empty()
        && kernel
            .blocks
            .iter()
            .enumerate()
            .all(|(expected, block)| block.block_id == expected && block.label == expected)
        && verify_clone_budget(kernel)
}

fn select_specialization_keys(
    func: &CanonFunc,
    kernel: &KernelFunction,
) -> Vec<(usize, Vec<VersionKey>)> {
    let mut candidates = Vec::new();
    for target in 0..func.blocks.len() {
        if !versionable_target_block(&kernel.blocks[target], fallthrough_target(func, target)) {
            continue;
        }
        let mut counts = HashMap::<VersionKey, usize>::new();
        for pred in &func.blocks[target].predecessors {
            let Some(key) = explicit_edge_key(func, *pred, target) else {
                continue;
            };
            if !key_legal_for_target_block(func, &kernel.blocks[target], &key) {
                continue;
            }
            *counts.entry(key).or_default() += 1;
        }
        let mut ranked = counts.into_iter().collect::<Vec<_>>();
        ranked.sort_by(|(lhs_key, lhs_count), (rhs_key, rhs_count)| {
            rhs_count
                .cmp(lhs_count)
                .then_with(|| rhs_key.facts.len().cmp(&lhs_key.facts.len()))
        });
        let keys = ranked
            .into_iter()
            .take(MAX_SPECIALIZED_CLONES_PER_BLOCK)
            .map(|(key, _)| key)
            .collect::<Vec<_>>();
        if !keys.is_empty() {
            candidates.push((target, keys));
        }
    }
    candidates
}

fn key_legal_for_target_block(func: &CanonFunc, block: &KernelBlock, key: &VersionKey) -> bool {
    let Some(fact) = key.facts.first() else {
        return true;
    };
    match fact {
        VersionFact::LocalZero { local_addr }
        | VersionFact::LocalNonZero { local_addr }
        | VersionFact::ConstLocal { local_addr, .. } => {
            local_value_fact_legal_for_target_block(func, block, *local_addr)
        }
        VersionFact::AddressConstBase { offset } => {
            specialize_const_base_block_ops(&block.ops, *offset).is_some()
        }
        VersionFact::AddressLocalBase { local_addr, delta } => {
            specialize_local_base_block_ops(&block.ops, *local_addr, *delta).is_some()
        }
        VersionFact::AddressLocalScaledIndex {
            base_local_addr,
            index_local_addr,
            scale_log2,
            delta,
        } => specialize_local_scaled_index_block_ops(
            &block.ops,
            *base_local_addr,
            *index_local_addr,
            *scale_log2,
            *delta,
        )
        .is_some(),
        VersionFact::DirectCallTargetClass { .. } => block.ops.iter().any(is_direct_call_kernel_op),
    }
}

fn local_value_fact_legal_for_target_block(
    func: &CanonFunc,
    block: &KernelBlock,
    local_addr: u32,
) -> bool {
    let Some(last) = block.ops.last() else {
        return false;
    };
    let suffix_len = if is_local_get_br_if(last, local_addr) {
        2
    } else if is_local_get_eqz_br_if(last, local_addr)
        || local_get_const_add_imm(last, local_addr).is_some()
        || local_get_const_compare_operands(last, local_addr).is_some()
    {
        3
    } else {
        return false;
    };

    let canon_block = &func.blocks[block.original_block_id];
    canon_block.insts.len() >= suffix_len
        && canon_block.insts[..canon_block.insts.len() - suffix_len]
            .iter()
            .all(|inst| written_local_addr(inst) != Some(local_addr))
}

fn versionable_target_block(block: &KernelBlock, fallthrough: Option<usize>) -> bool {
    if block.ops.iter().any(is_direct_call_kernel_op) {
        return true;
    }
    if versionable_const_base_prefix(&block.ops) {
        return true;
    }
    let Some(last) = block.ops.last() else {
        return false;
    };
    (std::ptr::fn_addr_eq(last.op, vm::op_local_get4_br_if as Op)
        || std::ptr::fn_addr_eq(last.op, vm::op_local_get4_i32_eqz_br_if as Op)
        || std::ptr::fn_addr_eq(last.op, vm::op_local_get4_i32_const_add_br_if as Op)
        || std::ptr::fn_addr_eq(last.op, vm::op_local_get4_i32_const_compare_br_if as Op))
        && (fallthrough.is_some() || op_jump_target(last).is_some())
}

fn explicit_edge_key(func: &CanonFunc, pred: usize, target: usize) -> Option<VersionKey> {
    let block = &func.blocks[pred];
    let len = block.insts.len();
    if let Some((offset, _)) = match_edge_const_base(block, target) {
        return Some(VersionKey {
            facts: vec![VersionFact::AddressConstBase { offset }],
        });
    }
    if let Some((local_addr, delta, _)) = match_edge_local_base(block, target) {
        return Some(VersionKey {
            facts: vec![VersionFact::AddressLocalBase { local_addr, delta }],
        });
    }
    if let Some((base_local_addr, index_local_addr, scale_log2, delta, _)) =
        match_edge_local_scaled_index(block, target)
    {
        return Some(VersionKey {
            facts: vec![VersionFact::AddressLocalScaledIndex {
                base_local_addr,
                index_local_addr,
                scale_log2,
                delta,
            }],
        });
    }
    if len >= 2
        && is_direct_call_op(block.insts[len - 2].op)
        && block.insts[len - 1].op_eq(vm::op_br as Op)
        && branch_target(&block.insts[len - 1]) == Some(target)
    {
        return Some(VersionKey {
            facts: vec![VersionFact::DirectCallTargetClass {
                imported: is_import_direct_call(block.insts[len - 2].op),
            }],
        });
    }
    if len >= 2
        && block.insts[len - 2].op_eq(vm::op_local_get4 as Op)
        && block.insts[len - 1].op_eq(vm::op_br_if as Op)
        && branch_target(&block.insts[len - 1]) == Some(target)
    {
        let local_addr = raw_local_addr(block.insts[len - 2].operands.first())?;
        return Some(VersionKey {
            facts: vec![VersionFact::LocalNonZero { local_addr }],
        });
    }
    if len >= 3
        && block.insts[len - 3].op_eq(vm::op_local_get4 as Op)
        && block.insts[len - 2].op_eq(vm::op_i32_eqz as Op)
        && block.insts[len - 1].op_eq(vm::op_br_if as Op)
        && branch_target(&block.insts[len - 1]) == Some(target)
    {
        let local_addr = raw_local_addr(block.insts[len - 3].operands.first())?;
        return Some(VersionKey {
            facts: vec![VersionFact::LocalZero { local_addr }],
        });
    }
    if len >= 3
        && block.insts[len - 3].op_eq(vm::op_i32_const as Op)
        && block.insts[len - 2].op_eq(vm::op_local_set4 as Op)
        && branch_target(&block.insts[len - 1]) == Some(target)
    {
        let local_addr = raw_local_addr(block.insts[len - 2].operands.first())?;
        let value = raw_i32(block.insts[len - 3].operands.first())? as u32;
        return Some(VersionKey {
            facts: vec![VersionFact::ConstLocal { local_addr, value }],
        });
    }
    if len >= 2
        && is_direct_call(block.insts[len - 2].op)
        && branch_target(&block.insts[len - 1]) == Some(target)
    {
        return Some(VersionKey {
            facts: vec![VersionFact::DirectCallTargetClass {
                imported: is_import_call(block.insts[len - 2].op),
            }],
        });
    }
    None
}

fn rewrite_explicit_targets(
    block: &mut KernelBlock,
    func: &CanonFunc,
    clone_for_key: &HashMap<(usize, VersionKey), usize>,
) {
    if matches!(block.version.kind, BlockVersionKind::Specialized) {
        return;
    }
    for target in explicit_edge_targets(func, block.original_block_id) {
        if let Some(key) = explicit_edge_key(func, block.original_block_id, target).clone() {
            if let Some(label) = clone_for_key.get(&(target, key.clone())) {
                if rewrite_specialized_edge(block, &key, *label) {
                    continue;
                }
                rewrite_target_label(block, target, *label);
            }
        }
    }
}

fn explicit_edge_targets(func: &CanonFunc, pred: usize) -> Vec<usize> {
    let Some(last) = func.blocks[pred].insts.last() else {
        return Vec::new();
    };
    if last.op_eq(vm::op_br as Op)
        || last.op_eq(vm::op_br_if as Op)
        || last.op_eq(vm::op_if as Op)
        || last.op_eq(vm::op_else as Op)
        || last.op_eq(vm::op_return as Op)
    {
        return branch_target(last).into_iter().collect();
    }
    if last.op_eq(vm::op_br_table as Op) {
        return last
            .operands
            .iter()
            .skip(1)
            .filter_map(|operand| {
                let LoweredOperand::JumpTarget(target) = operand else {
                    return None;
                };
                Some(*target)
            })
            .collect();
    }
    Vec::new()
}

fn rewrite_target_label(block: &mut KernelBlock, original_target: usize, new_label: usize) {
    for op in &mut block.ops {
        for operand in &mut op.operands {
            if let LoweredOperand::JumpTarget(target) = operand {
                if *target == original_target {
                    *target = new_label;
                }
            }
        }
    }
}

fn specialize_block_ops(
    ops: &[KernelOp],
    key: &VersionKey,
    fallthrough: Option<usize>,
) -> Vec<KernelOp> {
    let Some(fact) = key.facts.first() else {
        return ops.to_vec();
    };
    let mut ops = ops.to_vec();
    let Some(last) = ops.last().cloned() else {
        return ops;
    };

    match fact {
        VersionFact::LocalZero { local_addr } => {
            if is_local_get_br_if(&last, *local_addr) {
                if let Some(fallthrough) = fallthrough {
                    *ops.last_mut().expect("block has a last op") = branch_op(fallthrough);
                }
            } else if is_local_get_eqz_br_if(&last, *local_addr) {
                if let Some(taken) = op_jump_target(&last) {
                    *ops.last_mut().expect("block has a last op") = branch_op(taken);
                }
            }
        }
        VersionFact::LocalNonZero { local_addr } => {
            if is_local_get_br_if(&last, *local_addr) {
                if let Some(taken) = op_jump_target(&last) {
                    *ops.last_mut().expect("block has a last op") = branch_op(taken);
                }
            } else if is_local_get_eqz_br_if(&last, *local_addr) {
                if let Some(fallthrough) = fallthrough {
                    *ops.last_mut().expect("block has a last op") = branch_op(fallthrough);
                }
            }
        }
        VersionFact::ConstLocal { local_addr, value } => {
            if is_local_get_br_if(&last, *local_addr) {
                let target = if *value != 0 {
                    op_jump_target(&last)
                } else {
                    fallthrough
                };
                if let Some(target) = target {
                    *ops.last_mut().expect("block has a last op") = branch_op(target);
                }
            } else if is_local_get_eqz_br_if(&last, *local_addr) {
                let target = if *value == 0 {
                    op_jump_target(&last)
                } else {
                    fallthrough
                };
                if let Some(target) = target {
                    *ops.last_mut().expect("block has a last op") = branch_op(target);
                }
            } else if let Some((kind, imm)) = local_get_const_compare_operands(&last, *local_addr) {
                let target = if eval_i32_compare(kind, *value as i32, imm) {
                    op_jump_target(&last)
                } else {
                    fallthrough
                };
                if let Some(target) = target {
                    *ops.last_mut().expect("block has a last op") = branch_op(target);
                }
            } else if let Some(imm) = local_get_const_add_imm(&last, *local_addr) {
                let target = if (*value as i32).wrapping_add(imm) != 0 {
                    op_jump_target(&last)
                } else {
                    fallthrough
                };
                if let Some(target) = target {
                    *ops.last_mut().expect("block has a last op") = branch_op(target);
                }
            }
        }
        VersionFact::AddressConstBase { offset } => {
            if let Some(specialized) = specialize_const_base_block_ops(&ops, *offset) {
                ops = specialized;
            }
        }
        VersionFact::AddressLocalBase { local_addr, delta } => {
            if let Some(specialized) = specialize_local_base_block_ops(&ops, *local_addr, *delta) {
                ops = specialized;
            }
        }
        VersionFact::AddressLocalScaledIndex {
            base_local_addr,
            index_local_addr,
            scale_log2,
            delta,
        } => {
            if let Some(specialized) = specialize_local_scaled_index_block_ops(
                &ops,
                *base_local_addr,
                *index_local_addr,
                *scale_log2,
                *delta,
            ) {
                ops = specialized;
            }
        }
        VersionFact::DirectCallTargetClass { imported } => {
            for op in &mut ops {
                if *imported {
                    if std::ptr::fn_addr_eq(op.op, vm::op_call as Op) {
                        op.op = vm::op_call_import as Op;
                    } else if std::ptr::fn_addr_eq(op.op, vm::op_return_call as Op) {
                        op.op = vm::op_return_call_import as Op;
                    }
                } else if std::ptr::fn_addr_eq(op.op, vm::op_call_import as Op) {
                    op.op = vm::op_call as Op;
                } else if std::ptr::fn_addr_eq(op.op, vm::op_return_call_import as Op) {
                    op.op = vm::op_return_call as Op;
                }
            }
        }
    }

    ops
}

fn branch_op(target: usize) -> KernelOp {
    KernelOp {
        label: None,
        op: vm::op_br as Op,
        operands: vec![LoweredOperand::JumpTarget(target)],
        family: "versioned-branch",
    }
}

fn verify_clone_budget(kernel: &KernelFunction) -> bool {
    let mut generic = HashMap::<usize, usize>::new();
    let mut specialized = HashMap::<usize, usize>::new();
    for block in &kernel.blocks {
        match block.version.kind {
            BlockVersionKind::Generic => *generic.entry(block.original_block_id).or_default() += 1,
            BlockVersionKind::Specialized => {
                *specialized.entry(block.original_block_id).or_default() += 1
            }
        }
    }
    generic.values().all(|count| *count <= 1)
        && specialized
            .values()
            .all(|count| *count <= MAX_SPECIALIZED_CLONES_PER_BLOCK)
}

fn fuse_search_loop_ops(block: &mut KernelBlock) {
    let mut fused = Vec::with_capacity(block.ops.len());
    let mut cursor = 0usize;
    while cursor < block.ops.len() {
        if let Some(op) = fuse_i32_load16_u_local_base_eq_search_loop(&block.ops, cursor) {
            fused.push(op);
            cursor += 3;
            continue;
        }
        if let Some(op) = fuse_i32_load8_u_local_base_masked_search_loop(&block.ops, cursor) {
            fused.push(op);
            cursor += 3;
            continue;
        }
        fused.push(block.ops[cursor].clone());
        cursor += 1;
    }
    block.ops = fused;
}

fn fuse_i32_load16_u_local_base_eq_search_loop(
    ops: &[KernelOp],
    cursor: usize,
) -> Option<KernelOp> {
    let [compare, next, miss] = ops.get(cursor..cursor + 3)? else {
        return None;
    };
    if !std::ptr::fn_addr_eq(
        compare.op,
        vm::op_i32_load_local_base_set4_i32_load16_u_local_base_local_eq_br_if as Op,
    ) || !std::ptr::fn_addr_eq(next.op, vm::op_i32_load_local_base_tee4_br_if as Op)
        || !std::ptr::fn_addr_eq(miss.op, vm::op_br as Op)
        || op_jump_target(next).is_none()
    {
        return None;
    }
    if raw_local_addr(compare.operands.first()) != raw_local_addr(next.operands.first())
        || raw_local_addr(compare.operands.first()) != raw_local_addr(next.operands.get(3))
    {
        return None;
    }
    let match_target = op_jump_target(compare)?;
    let miss_target = op_jump_target(miss)?;
    let operands = vec![
        compare.operands.first()?.clone(),
        compare.operands.get(1)?.clone(),
        compare.operands.get(2)?.clone(),
        compare.operands.get(3)?.clone(),
        compare.operands.get(4)?.clone(),
        compare.operands.get(5)?.clone(),
        compare.operands.get(6)?.clone(),
        next.operands.get(1)?.clone(),
        next.operands.get(2)?.clone(),
        LoweredOperand::JumpTarget(match_target),
        LoweredOperand::JumpTarget(miss_target),
    ];
    Some(KernelOp {
        label: compare.label,
        op: vm::op_i32_load_local_base_set4_i32_load16_u_local_base_local_eq_search_loop as Op,
        operands,
        family: "versioned-i32_load16_u_local_base_eq_search_loop",
    })
}

fn fuse_i32_load8_u_local_base_masked_search_loop(
    ops: &[KernelOp],
    cursor: usize,
) -> Option<KernelOp> {
    let [compare, next, miss] = ops.get(cursor..cursor + 3)? else {
        return None;
    };
    if !std::ptr::fn_addr_eq(
        compare.op,
        vm::op_i32_load_local_base_set4_i32_load8_u_local_base_local_masked_compare_br_if as Op,
    ) || !std::ptr::fn_addr_eq(next.op, vm::op_i32_load_local_base_tee4_br_if as Op)
        || !std::ptr::fn_addr_eq(miss.op, vm::op_br as Op)
        || op_jump_target(next).is_none()
    {
        return None;
    }
    if raw_local_addr(compare.operands.first()) != raw_local_addr(next.operands.first())
        || raw_local_addr(compare.operands.first()) != raw_local_addr(next.operands.get(3))
    {
        return None;
    }
    let match_target = op_jump_target(compare)?;
    let miss_target = op_jump_target(miss)?;
    let operands = vec![
        compare.operands.first()?.clone(),
        compare.operands.get(1)?.clone(),
        compare.operands.get(2)?.clone(),
        compare.operands.get(3)?.clone(),
        compare.operands.get(4)?.clone(),
        compare.operands.get(5)?.clone(),
        compare.operands.get(6)?.clone(),
        compare.operands.get(7)?.clone(),
        next.operands.get(1)?.clone(),
        next.operands.get(2)?.clone(),
        LoweredOperand::JumpTarget(match_target),
        LoweredOperand::JumpTarget(miss_target),
    ];
    Some(KernelOp {
        label: compare.label,
        op: vm::op_i32_load_local_base_set4_i32_load8_u_local_base_local_masked_search_loop as Op,
        operands,
        family: "versioned-i32_load8_u_local_base_masked_search_loop",
    })
}

fn versionable_const_base_prefix(ops: &[KernelOp]) -> bool {
    if let Some(first) = ops.first() {
        if scalar_const_base_load_family(first.op).is_some() {
            return true;
        }
    }
    if let Some(store) = ops.get(1) {
        if let Some(scalar) = scalar_memory_store_type(store.op) {
            if scalar_const_base_store_family(store.op).is_some()
                && raw_local_get_kernel(&ops[0], scalar.width()).is_some()
            {
                return true;
            }
        }
    }
    ops.len() >= 4
        && std::ptr::fn_addr_eq(ops[0].op, vm::op_i32_load as Op)
        && std::ptr::fn_addr_eq(ops[1].op, vm::op_local_get4 as Op)
        && std::ptr::fn_addr_eq(ops[2].op, vm::op_i32_add as Op)
        && std::ptr::fn_addr_eq(ops[3].op, vm::op_local_set4 as Op)
}

fn rewrite_const_base_edge(block: &mut KernelBlock, new_label: usize) -> bool {
    let len = block.ops.len();
    if len < 2
        || !std::ptr::fn_addr_eq(block.ops[len - 2].op, vm::op_i32_const as Op)
        || !std::ptr::fn_addr_eq(block.ops[len - 1].op, vm::op_br as Op)
    {
        return false;
    }
    block.ops.pop();
    block.ops.pop();
    block.ops.push(branch_op(new_label));
    true
}

fn rewrite_specialized_edge(block: &mut KernelBlock, key: &VersionKey, new_label: usize) -> bool {
    match key.facts.first() {
        Some(VersionFact::AddressConstBase { .. }) => rewrite_const_base_edge(block, new_label),
        Some(VersionFact::AddressLocalBase { local_addr, delta }) => {
            rewrite_local_base_edge(block, *local_addr, *delta, new_label)
        }
        Some(VersionFact::AddressLocalScaledIndex {
            base_local_addr,
            index_local_addr,
            scale_log2,
            delta,
        }) => rewrite_local_scaled_index_edge(
            block,
            *base_local_addr,
            *index_local_addr,
            *scale_log2,
            *delta,
            new_label,
        ),
        _ => false,
    }
}

fn rewrite_suffix_edge(
    block: &mut KernelBlock,
    consumed_before_br: usize,
    new_label: usize,
) -> bool {
    let consumed = consumed_before_br.saturating_add(1);
    if block.ops.len() < consumed {
        return false;
    }
    block.ops.truncate(block.ops.len() - consumed);
    block.ops.push(branch_op(new_label));
    true
}

fn rewrite_local_base_edge(
    block: &mut KernelBlock,
    local_addr: u32,
    delta: i32,
    new_label: usize,
) -> bool {
    let len = block.ops.len();
    if len < 2 || !std::ptr::fn_addr_eq(block.ops[len - 1].op, vm::op_br as Op) {
        return false;
    }
    if delta == 0 && is_local_get_op(&block.ops[len - 2], local_addr) {
        return rewrite_suffix_edge(block, 1, new_label);
    }
    if is_local_get_const_add_op(&block.ops[len - 2], local_addr, delta) {
        return rewrite_suffix_edge(block, 1, new_label);
    }
    if len >= 4
        && is_local_get_op(&block.ops[len - 4], local_addr)
        && is_i32_const_op(&block.ops[len - 3], delta)
        && std::ptr::fn_addr_eq(block.ops[len - 2].op, vm::op_i32_add as Op)
    {
        return rewrite_suffix_edge(block, 3, new_label);
    }
    false
}

fn rewrite_local_scaled_index_edge(
    block: &mut KernelBlock,
    base_local_addr: u32,
    index_local_addr: u32,
    scale_log2: u32,
    delta: i32,
    new_label: usize,
) -> bool {
    let len = block.ops.len();
    if len < 2 || !std::ptr::fn_addr_eq(block.ops[len - 1].op, vm::op_br as Op) {
        return false;
    }
    if scale_log2 == 0
        && delta == 0
        && is_local_get_local_get_add_op(&block.ops[len - 2], base_local_addr, index_local_addr)
    {
        return rewrite_suffix_edge(block, 1, new_label);
    }
    if scale_log2 == 0
        && len >= 4
        && is_local_get_local_get_add_op(&block.ops[len - 4], base_local_addr, index_local_addr)
        && is_i32_const_op(&block.ops[len - 3], delta)
        && std::ptr::fn_addr_eq(block.ops[len - 2].op, vm::op_i32_add as Op)
    {
        return rewrite_suffix_edge(block, 3, new_label);
    }
    match_generic_local_scaled_index_edge(
        &block.ops,
        base_local_addr,
        index_local_addr,
        scale_log2,
        delta,
    )
    .is_some_and(|consumed_before_br| rewrite_suffix_edge(block, consumed_before_br, new_label))
}

fn match_generic_local_scaled_index_edge(
    ops: &[KernelOp],
    base_local_addr: u32,
    index_local_addr: u32,
    scale_log2: u32,
    delta: i32,
) -> Option<usize> {
    let len = ops.len();
    if len < 2 || !std::ptr::fn_addr_eq(ops[len - 1].op, vm::op_br as Op) {
        return None;
    }
    for &consumed_before_br in &[7usize, 5, 3] {
        if len < consumed_before_br + 1 {
            continue;
        }
        let start = len - 1 - consumed_before_br;
        if generic_local_scaled_index_suffix_matches(
            &ops[start..len - 1],
            base_local_addr,
            index_local_addr,
            scale_log2,
            delta,
        ) {
            return Some(consumed_before_br);
        }
    }
    None
}

fn generic_local_scaled_index_suffix_matches(
    ops: &[KernelOp],
    base_local_addr: u32,
    index_local_addr: u32,
    scale_log2: u32,
    delta: i32,
) -> bool {
    if ops.len() < 3 || !is_local_get_op(&ops[0], base_local_addr) {
        return false;
    }

    let mut cursor = 1usize;
    if scale_log2 == 0 {
        if !ops
            .get(cursor)
            .is_some_and(|op| is_local_get_op(op, index_local_addr))
        {
            return false;
        }
        cursor += 1;
    } else if ops
        .get(cursor)
        .is_some_and(|op| is_local_binop32_const_shl_op(op, index_local_addr, scale_log2))
    {
        cursor += 1;
    } else if ops.len() >= cursor + 3
        && is_local_get_op(&ops[cursor], index_local_addr)
        && is_i32_const_op(
            &ops[cursor + 1],
            i32::try_from(scale_log2).ok().unwrap_or_default(),
        )
        && std::ptr::fn_addr_eq(ops[cursor + 2].op, vm::op_i32_shl as Op)
    {
        cursor += 3;
    } else {
        return false;
    }

    if !ops
        .get(cursor)
        .is_some_and(|op| std::ptr::fn_addr_eq(op.op, vm::op_i32_add as Op))
    {
        return false;
    }
    cursor += 1;
    let mut seen_delta = 0i32;
    if ops.len() >= cursor + 2
        && is_i32_const_op(&ops[cursor], delta)
        && std::ptr::fn_addr_eq(ops[cursor + 1].op, vm::op_i32_add as Op)
    {
        seen_delta = delta;
        cursor += 2;
    }
    cursor == ops.len() && seen_delta == delta
}

fn specialize_const_base_block_ops(ops: &[KernelOp], base: u32) -> Option<Vec<KernelOp>> {
    if ops.is_empty() {
        return None;
    }
    if std::ptr::fn_addr_eq(ops[0].op, vm::op_i32_load as Op) {
        if ops.len() >= 4
            && std::ptr::fn_addr_eq(ops[1].op, vm::op_local_get4 as Op)
            && std::ptr::fn_addr_eq(ops[2].op, vm::op_i32_add as Op)
            && std::ptr::fn_addr_eq(ops[3].op, vm::op_local_set4 as Op)
        {
            let folded = fold_const_base_memarg(base, ops[0].operands.first())?;
            let mut out = Vec::with_capacity(ops.len() - 3);
            out.push(KernelOp {
                label: ops[0].label,
                op: vm::op_i32_load_const_base_local_get4_i32_add_set4 as Op,
                operands: vec![
                    folded,
                    ops[1].operands.first()?.clone(),
                    ops[3].operands.first()?.clone(),
                ],
                family: "versioned-i32_load_const_base_local_get4_i32_add_set4",
            });
            out.extend_from_slice(&ops[4..]);
            return Some(out);
        }

        let folded = fold_const_base_memarg(base, ops[0].operands.first())?;
        let mut out = Vec::with_capacity(ops.len());
        out.push(KernelOp {
            label: ops[0].label,
            op: vm::op_i32_load_const_base as Op,
            operands: vec![folded],
            family: "versioned-i32_load_const_base",
        });
        out.extend_from_slice(&ops[1..]);
        return Some(out);
    }

    if let Some(op) = scalar_const_base_load_family(ops[0].op) {
        let folded = fold_const_base_memarg(base, ops[0].operands.first())?;
        let mut out = Vec::with_capacity(ops.len());
        out.push(KernelOp {
            label: ops[0].label,
            op,
            operands: vec![folded],
            family: "versioned-memory_const_base_load",
        });
        out.extend_from_slice(&ops[1..]);
        return Some(out);
    }

    if let Some(store) = ops.get(1) {
        if let Some(scalar) = scalar_memory_store_type(store.op) {
            if let Some(op) = scalar_const_base_store_family(store.op) {
                let src = raw_local_get_kernel(&ops[0], scalar.width())?;
                let folded = fold_const_base_memarg(base, store.operands.first())?;
                let mut out = Vec::with_capacity(ops.len() - 1);
                out.push(KernelOp {
                    label: ops[0].label,
                    op,
                    operands: vec![folded, src],
                    family: "versioned-memory_const_base_store_local",
                });
                out.extend_from_slice(&ops[2..]);
                return Some(out);
            }
        }
    }

    if ops.len() >= 2
        && std::ptr::fn_addr_eq(ops[0].op, vm::op_local_get4 as Op)
        && std::ptr::fn_addr_eq(ops[1].op, vm::op_i32_store as Op)
    {
        let folded = fold_const_base_memarg(base, ops[1].operands.first())?;
        let mut out = Vec::with_capacity(ops.len() - 1);
        out.push(KernelOp {
            label: ops[0].label,
            op: vm::op_i32_store_const_base_local4 as Op,
            operands: vec![folded, ops[0].operands.first()?.clone()],
            family: "versioned-i32_store_const_base_local4",
        });
        out.extend_from_slice(&ops[2..]);
        return Some(out);
    }

    None
}

fn specialize_local_base_block_ops(
    ops: &[KernelOp],
    local_addr: u32,
    delta: i32,
) -> Option<Vec<KernelOp>> {
    if ops.is_empty() {
        return None;
    }
    if let Some(op) = scalar_local_base_load_family(ops[0].op) {
        let mut out = Vec::with_capacity(ops.len());
        out.push(KernelOp {
            label: ops[0].label,
            op,
            operands: vec![
                raw_local_operand(local_addr),
                raw_i32_operand(delta),
                ops[0].operands.first()?.clone(),
            ],
            family: "versioned-memory_local_base",
        });
        out.extend_from_slice(&ops[1..]);
        return Some(out);
    }

    if let Some(store) = ops.get(1) {
        if let Some(scalar) = scalar_memory_store_type(store.op) {
            let src = raw_local_get_kernel(&ops[0], scalar.width())?;
            let op = scalar_local_base_store_family(store.op)?;
            let mut out = Vec::with_capacity(ops.len());
            out.push(KernelOp {
                label: ops[0].label,
                op: local_get_op_for_width(scalar.width()),
                operands: vec![src],
                family: ops[0].family,
            });
            out.push(KernelOp {
                label: None,
                op,
                operands: {
                    let mut operands = vec![raw_local_operand(local_addr), raw_i32_operand(delta)];
                    operands.extend(store.operands.clone());
                    operands
                },
                family: "versioned-memory_local_base",
            });
            out.extend_from_slice(&ops[2..]);
            return Some(out);
        }
    }

    if ops.len() >= 2
        && std::ptr::fn_addr_eq(ops[0].op, vm::op_local_get4 as Op)
        && std::ptr::fn_addr_eq(ops[1].op, vm::op_i32_store as Op)
    {
        let mut out = Vec::with_capacity(ops.len());
        out.push(ops[0].clone());
        out.push(KernelOp {
            label: None,
            op: vm::op_i32_store_local_base as Op,
            operands: vec![
                raw_local_operand(local_addr),
                raw_i32_operand(delta),
                ops[1].operands.first()?.clone(),
            ],
            family: "versioned-i32_store_local_base",
        });
        out.extend_from_slice(&ops[2..]);
        return Some(out);
    }

    None
}

fn specialize_local_scaled_index_block_ops(
    ops: &[KernelOp],
    base_local_addr: u32,
    index_local_addr: u32,
    scale_log2: u32,
    delta: i32,
) -> Option<Vec<KernelOp>> {
    if ops.is_empty() {
        return None;
    }
    if let Some(op) = scalar_local_scaled_index_load_family(ops[0].op) {
        let mut out = Vec::with_capacity(ops.len());
        out.push(KernelOp {
            label: ops[0].label,
            op,
            operands: vec![
                raw_local_operand(base_local_addr),
                raw_local_operand(index_local_addr),
                raw_u32_operand(scale_log2),
                raw_i32_operand(delta),
                ops[0].operands.first()?.clone(),
            ],
            family: "versioned-memory_local_scaled_index",
        });
        out.extend_from_slice(&ops[1..]);
        return Some(out);
    }

    if let Some(store) = ops.get(1) {
        if let Some(scalar) = scalar_memory_store_type(store.op) {
            let src = raw_local_get_kernel(&ops[0], scalar.width())?;
            let op = scalar_local_scaled_index_store_family(store.op)?;
            let mut out = Vec::with_capacity(ops.len());
            out.push(KernelOp {
                label: ops[0].label,
                op: local_get_op_for_width(scalar.width()),
                operands: vec![src],
                family: ops[0].family,
            });
            out.push(KernelOp {
                label: None,
                op,
                operands: {
                    let mut operands = vec![
                        raw_local_operand(base_local_addr),
                        raw_local_operand(index_local_addr),
                        raw_u32_operand(scale_log2),
                        raw_i32_operand(delta),
                    ];
                    operands.extend(store.operands.clone());
                    operands
                },
                family: "versioned-memory_local_scaled_index",
            });
            out.extend_from_slice(&ops[2..]);
            return Some(out);
        }
    }

    if ops.len() >= 2
        && std::ptr::fn_addr_eq(ops[0].op, vm::op_local_get4 as Op)
        && std::ptr::fn_addr_eq(ops[1].op, vm::op_i32_store as Op)
    {
        let mut out = Vec::with_capacity(ops.len());
        out.push(ops[0].clone());
        out.push(KernelOp {
            label: None,
            op: vm::op_i32_store_local_scaled_index as Op,
            operands: vec![
                raw_local_operand(base_local_addr),
                raw_local_operand(index_local_addr),
                raw_u32_operand(scale_log2),
                raw_i32_operand(delta),
                ops[1].operands.first()?.clone(),
            ],
            family: "versioned-i32_store_local_scaled_index",
        });
        out.extend_from_slice(&ops[2..]);
        return Some(out);
    }

    None
}

fn fold_const_base_memarg(base: u32, memarg: Option<&LoweredOperand>) -> Option<LoweredOperand> {
    let mut memarg = raw_memarg(memarg)?;
    memarg.offset = memarg.offset.wrapping_add(base);
    Some(LoweredOperand::Raw(unsafe { Operand { memarg }.encoded }))
}

fn relabel_first_op(block: &mut KernelBlock) {
    if let Some(first) = block.ops.first_mut() {
        first.label = Some(block.label);
    }
}

fn fallthrough_target(func: &CanonFunc, block_id: usize) -> Option<usize> {
    let explicit = func.blocks[block_id].insts.last().and_then(branch_target);
    func.blocks[block_id]
        .successors
        .iter()
        .copied()
        .find(|succ| Some(*succ) != explicit)
}

fn branch_target(inst: &super::ir::CanonInst) -> Option<usize> {
    let LoweredOperand::JumpTarget(target) = inst.operands.first()? else {
        return None;
    };
    Some(*target)
}

fn op_jump_target(op: &KernelOp) -> Option<usize> {
    let LoweredOperand::JumpTarget(target) = op.operands.last()? else {
        return None;
    };
    Some(*target)
}

fn raw_local_addr(operand: Option<&LoweredOperand>) -> Option<u32> {
    let LoweredOperand::Raw(encoded) = operand? else {
        return None;
    };
    Some(unsafe { crate::common::Operand { encoded: *encoded }.local_addr })
}

fn raw_i32(operand: Option<&LoweredOperand>) -> Option<i32> {
    let LoweredOperand::Raw(encoded) = operand? else {
        return None;
    };
    Some(unsafe { crate::common::Operand { encoded: *encoded }.i32 })
}

fn raw_memarg(operand: Option<&LoweredOperand>) -> Option<MemArg> {
    let LoweredOperand::Raw(encoded) = operand? else {
        return None;
    };
    Some(unsafe { Operand { encoded: *encoded }.memarg })
}

fn raw_local_operand(local_addr: u32) -> LoweredOperand {
    LoweredOperand::Raw(unsafe { Operand { local_addr }.encoded })
}

fn raw_i32_operand(i32: i32) -> LoweredOperand {
    LoweredOperand::Raw(unsafe { Operand { i32 }.encoded })
}

fn raw_u32_operand(u32: u32) -> LoweredOperand {
    LoweredOperand::Raw(unsafe { Operand { u32 }.encoded })
}

fn match_edge_const_base(block: &CanonBlock, target: usize) -> Option<(u32, usize)> {
    let len = block.insts.len();
    if len < 2
        || !block.insts[len - 2].op_eq(vm::op_i32_const as Op)
        || !block.insts[len - 1].op_eq(vm::op_br as Op)
        || branch_target(&block.insts[len - 1]) != Some(target)
    {
        return None;
    }
    let offset = raw_i32(block.insts[len - 2].operands.first())? as u32;
    Some((offset, 1))
}

fn match_edge_local_base(block: &CanonBlock, target: usize) -> Option<(u32, i32, usize)> {
    let len = block.insts.len();
    if len < 2
        || !block.insts[len - 1].op_eq(vm::op_br as Op)
        || branch_target(&block.insts[len - 1]) != Some(target)
    {
        return None;
    }
    if len >= 4
        && block.insts[len - 4].op_eq(vm::op_local_get4 as Op)
        && block.insts[len - 3].op_eq(vm::op_i32_const as Op)
        && block.insts[len - 2].op_eq(vm::op_i32_add as Op)
    {
        return Some((
            raw_local_addr(block.insts[len - 4].operands.first())?,
            raw_i32(block.insts[len - 3].operands.first())?,
            3,
        ));
    }
    if block.insts[len - 2].op_eq(vm::op_local_get4 as Op) {
        return Some((raw_local_addr(block.insts[len - 2].operands.first())?, 0, 1));
    }
    None
}

fn match_edge_local_scaled_index(
    block: &CanonBlock,
    target: usize,
) -> Option<(u32, u32, u32, i32, usize)> {
    let len = block.insts.len();
    if len < 4
        || !block.insts[len - 1].op_eq(vm::op_br as Op)
        || branch_target(&block.insts[len - 1]) != Some(target)
    {
        return None;
    }
    for &consumed_before_br in &[7usize, 5, 3] {
        if len < consumed_before_br + 1 {
            continue;
        }
        let start = len - 1 - consumed_before_br;
        let suffix = &block.insts[start..len - 1];
        if let Some((base_local_addr, index_local_addr, scale_log2, delta)) =
            match_local_scaled_index_suffix(suffix)
        {
            return Some((
                base_local_addr,
                index_local_addr,
                scale_log2,
                delta,
                consumed_before_br,
            ));
        }
    }
    None
}

fn match_local_scaled_index_suffix(insts: &[CanonInst]) -> Option<(u32, u32, u32, i32)> {
    let base_local_addr = raw_local_addr(insts.first()?.operands.first())?;
    let index_local_addr = raw_local_addr(insts.get(1)?.operands.first())?;
    if !insts.first()?.op_eq(vm::op_local_get4 as Op)
        || !insts.get(1)?.op_eq(vm::op_local_get4 as Op)
    {
        return None;
    }
    let mut cursor = 2usize;
    let mut scale_log2 = 0u32;
    if insts.len() >= cursor + 2
        && insts
            .get(cursor)
            .is_some_and(|inst| inst.op_eq(vm::op_i32_const as Op))
        && insts
            .get(cursor + 1)
            .is_some_and(|inst| inst.op_eq(vm::op_i32_shl as Op))
    {
        let scale = raw_i32(insts.get(cursor)?.operands.first())?;
        if !(0..=3).contains(&scale) {
            return None;
        }
        scale_log2 = u32::try_from(scale).ok()?;
        cursor += 2;
    }
    if !insts
        .get(cursor)
        .is_some_and(|inst| inst.op_eq(vm::op_i32_add as Op))
    {
        return None;
    }
    cursor += 1;
    let mut delta = 0i32;
    if insts.len() >= cursor + 2
        && insts
            .get(cursor)
            .is_some_and(|inst| inst.op_eq(vm::op_i32_const as Op))
        && insts
            .get(cursor + 1)
            .is_some_and(|inst| inst.op_eq(vm::op_i32_add as Op))
    {
        delta = raw_i32(insts.get(cursor)?.operands.first())?;
        cursor += 2;
    }
    (cursor == insts.len()).then_some((base_local_addr, index_local_addr, scale_log2, delta))
}

fn is_local_get_br_if(op: &KernelOp, expected_local_addr: u32) -> bool {
    std::ptr::fn_addr_eq(op.op, vm::op_local_get4_br_if as Op)
        && raw_local_addr(op.operands.first()) == Some(expected_local_addr)
}

fn is_local_get_op(op: &KernelOp, expected_local_addr: u32) -> bool {
    std::ptr::fn_addr_eq(op.op, vm::op_local_get4 as Op)
        && raw_local_addr(op.operands.first()) == Some(expected_local_addr)
}

fn raw_local_get_kernel(op: &KernelOp, width: u32) -> Option<LoweredOperand> {
    if std::ptr::fn_addr_eq(op.op, local_get_op_for_width(width)) {
        return op.operands.first().cloned();
    }
    None
}

fn local_get_op_for_width(width: u32) -> Op {
    match width {
        4 => vm::op_local_get4 as Op,
        8 => vm::op_local_get8 as Op,
        16 => vm::op_local_get16 as Op,
        other => panic!("unsupported local.get width for versioning: {other}"),
    }
}

fn is_local_get_const_add_op(op: &KernelOp, expected_local_addr: u32, expected_delta: i32) -> bool {
    std::ptr::fn_addr_eq(op.op, vm::op_local_get4_i32_const_add as Op)
        && raw_local_addr(op.operands.first()) == Some(expected_local_addr)
        && raw_i32(op.operands.get(1)) == Some(expected_delta)
}

fn is_local_get_local_get_add_op(
    op: &KernelOp,
    expected_base_local_addr: u32,
    expected_index_local_addr: u32,
) -> bool {
    std::ptr::fn_addr_eq(op.op, vm::op_local_get4_local_get4_i32_add as Op)
        && raw_local_addr(op.operands.first()) == Some(expected_base_local_addr)
        && raw_local_addr(op.operands.get(1)) == Some(expected_index_local_addr)
}

fn is_local_binop32_const_shl_op(
    op: &KernelOp,
    expected_local_addr: u32,
    expected_scale_log2: u32,
) -> bool {
    std::ptr::fn_addr_eq(op.op, vm::op_local_binop32 as Op)
        && raw_u32(op.operands.first())
            .and_then(decode_local_binop32_kind)
            .is_some_and(|(kind, rhs_shape)| {
                kind == LocalBinop32Op::I32Shl && rhs_shape == LocalFastRhsShape::Const
            })
        && raw_local_addr(op.operands.get(1)) == Some(expected_local_addr)
        && raw_u32(op.operands.get(2)) == Some(expected_scale_log2)
}

fn is_i32_const_op(op: &KernelOp, expected: i32) -> bool {
    std::ptr::fn_addr_eq(op.op, vm::op_i32_const as Op)
        && raw_i32(op.operands.first()) == Some(expected)
}

fn is_local_get_eqz_br_if(op: &KernelOp, expected_local_addr: u32) -> bool {
    std::ptr::fn_addr_eq(op.op, vm::op_local_get4_i32_eqz_br_if as Op)
        && raw_local_addr(op.operands.first()) == Some(expected_local_addr)
}

fn local_get_const_add_imm(op: &KernelOp, expected_local_addr: u32) -> Option<i32> {
    if !std::ptr::fn_addr_eq(op.op, vm::op_local_get4_i32_const_add_br_if as Op)
        || raw_local_addr(op.operands.first()) != Some(expected_local_addr)
    {
        return None;
    }
    raw_i32(op.operands.get(1))
}

fn local_get_const_compare_operands(op: &KernelOp, expected_local_addr: u32) -> Option<(u32, i32)> {
    if !std::ptr::fn_addr_eq(op.op, vm::op_local_get4_i32_const_compare_br_if as Op)
        || raw_local_addr(op.operands.first()) != Some(expected_local_addr)
    {
        return None;
    }
    Some((raw_u32(op.operands.get(1))?, raw_i32(op.operands.get(2))?))
}

fn raw_u32(operand: Option<&LoweredOperand>) -> Option<u32> {
    let LoweredOperand::Raw(encoded) = operand? else {
        return None;
    };
    Some(unsafe { crate::common::Operand { encoded: *encoded }.u32 })
}

fn written_local_addr(inst: &CanonInst) -> Option<u32> {
    if inst.op_eq(vm::op_local_set4 as Op)
        || inst.op_eq(vm::op_local_set8 as Op)
        || inst.op_eq(vm::op_local_set16 as Op)
        || inst.op_eq(vm::op_local_tee4 as Op)
        || inst.op_eq(vm::op_local_tee8 as Op)
        || inst.op_eq(vm::op_local_tee16 as Op)
    {
        return raw_local_addr(inst.operands.first());
    }
    None
}

fn eval_i32_compare(kind: u32, lhs: i32, rhs: i32) -> bool {
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

fn is_direct_call(op: Op) -> bool {
    std::ptr::fn_addr_eq(op, vm::op_call as Op)
        || std::ptr::fn_addr_eq(op, vm::op_call_import as Op)
        || std::ptr::fn_addr_eq(op, vm::op_return_call as Op)
        || std::ptr::fn_addr_eq(op, vm::op_return_call_import as Op)
}

fn is_import_call(op: Op) -> bool {
    std::ptr::fn_addr_eq(op, vm::op_call_import as Op)
        || std::ptr::fn_addr_eq(op, vm::op_return_call_import as Op)
}

fn is_direct_call_kernel_op(op: &KernelOp) -> bool {
    is_direct_call(op.op)
}

fn is_direct_call_op(op: Op) -> bool {
    std::ptr::fn_addr_eq(op, vm::op_call as Op)
        || std::ptr::fn_addr_eq(op, vm::op_call_import as Op)
        || std::ptr::fn_addr_eq(op, vm::op_return_call as Op)
        || std::ptr::fn_addr_eq(op, vm::op_return_call_import as Op)
}

fn is_import_direct_call(op: Op) -> bool {
    std::ptr::fn_addr_eq(op, vm::op_call_import as Op)
        || std::ptr::fn_addr_eq(op, vm::op_return_call_import as Op)
}

trait CanonInstExt {
    fn op_eq(&self, candidate: Op) -> bool;
}

impl CanonInstExt for super::ir::CanonInst {
    fn op_eq(&self, candidate: Op) -> bool {
        std::ptr::fn_addr_eq(self.op, candidate)
    }
}
