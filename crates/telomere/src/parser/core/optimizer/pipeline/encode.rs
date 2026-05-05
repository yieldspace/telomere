use std::collections::HashMap;

use super::lower::LoweredKernelFunction;
use crate::common::{
    decode_local_binop32_kind, decode_local_binop64_kind, decode_local_cmp32_kind,
    decode_local_cmp64_kind, LocalCmp32Op, LocalFastRhsShape, LoweredBlockMap, LoweredFunction,
    LoweredJumpTarget, LoweredOp, LoweredOperand, Op, Operand,
};
use crate::runtime::vm;

pub(crate) fn encode(kernel: LoweredKernelFunction) -> LoweredFunction {
    let mut code = Vec::new();
    let mut call_recipes = Vec::new();
    let mut jump_table = Vec::new();
    let mut block_map = Vec::new();

    for block in kernel.blocks {
        block_map.push(LoweredBlockMap {
            block_id: block.block_id,
            label: block.label,
            code_index: code.len(),
        });
        jump_table.push(LoweredJumpTarget {
            label: block.label,
            block_id: block.block_id,
        });
        for op in block.ops {
            for operand in &op.operands {
                if let LoweredOperand::CallRecipeRef(target) = operand {
                    call_recipes.push(*target);
                }
            }
            code.push(LoweredOp {
                label: op.label,
                op: op.op,
                operands: op.operands,
            });
        }
    }

    fuse_search_loop_entries(&mut code, &block_map);
    let const_pool = build_const_pool(&mut code);
    call_recipes.sort_unstable_by_key(|target| (target.funcidx, target.resolved_recipe_slot()));
    call_recipes.dedup();

    LoweredFunction {
        code,
        const_pool,
        call_recipes,
        jump_table,
        block_map,
        materialized_preview: None,
    }
}

pub(crate) fn verify(lowered: &LoweredFunction) -> bool {
    if lowered.code.is_empty() || lowered.block_map.is_empty() || lowered.jump_table.is_empty() {
        return false;
    }

    let mut labels = HashMap::new();
    for block in &lowered.block_map {
        if block.code_index >= lowered.code.len() {
            return false;
        }
        if lowered.code[block.code_index].label != Some(block.label) {
            return false;
        }
        if labels.insert(block.label, block.block_id).is_some() {
            return false;
        }
    }

    lowered
        .jump_table
        .iter()
        .all(|target| labels.get(&target.label) == Some(&target.block_id))
        && lowered.code.iter().all(|op| {
            op.operands.iter().all(|operand| match operand {
                LoweredOperand::Raw(_) | LoweredOperand::CallRecipeRef(_) => true,
                LoweredOperand::ConstPoolRef(index) => (*index as usize) < lowered.const_pool.len(),
                LoweredOperand::JumpTarget(label) => labels.contains_key(label),
            })
        })
}

fn fuse_search_loop_entries(code: &mut [LoweredOp], block_map: &[LoweredBlockMap]) {
    let label_code_indices = block_map
        .iter()
        .map(|block| (block.label, block.code_index))
        .collect::<HashMap<_, _>>();
    for cursor in 0..code.len() {
        if let Some(replacement) = fuse_local_get4_run_skip(code, cursor) {
            code[cursor] = replacement;
            continue;
        }
        if let Some(replacement) = fuse_local_get4x3_add_const_binop_add_consumer(code, cursor) {
            code[cursor] = replacement;
            continue;
        }
        if let Some(replacement) = fuse_i32_load_store_local_base_local_get4_pair(code, cursor) {
            code[cursor] = replacement;
            continue;
        }
        if let Some(replacement) = fuse_local_get4_local_base_i32_load_pair(code, cursor) {
            code[cursor] = replacement;
            continue;
        }
        if let Some(replacement) =
            fuse_local_base_i32_load_pair_fallthrough_local_get4(code, cursor)
        {
            code[cursor] = replacement;
            continue;
        }
        if let Some(replacement) = fuse_local_get4_br_table(code, cursor) {
            code[cursor] = replacement;
            continue;
        }
        if let Some(replacement) = fuse_local_get4_i32_const_add_br_table(code, cursor) {
            code[cursor] = replacement;
            continue;
        }
        if let Some(replacement) = fuse_i32_inc_load8_update_branch_tail(code, cursor) {
            code[cursor] = replacement;
            continue;
        }
        if let Some(replacement) =
            fuse_guarded_load8_update_branch_tail_taken_const_compare_br_table(
                code,
                cursor,
                &label_code_indices,
            )
        {
            code[cursor] = replacement;
            continue;
        }
        if let Some(replacement) = fuse_guarded_load8_update_branch_tail_false_local_get4_br_table(
            code,
            cursor,
            &label_code_indices,
        ) {
            code[cursor] = replacement;
            continue;
        }
        if let Some(replacement) = fuse_guarded_load8_update_branch_tail_taken_local_get4_br_table(
            code,
            cursor,
            &label_code_indices,
        ) {
            code[cursor] = replacement;
            continue;
        }
        if let Some(replacement) = fuse_guarded_load8_update_branch_tail(code, cursor) {
            code[cursor] = replacement;
            continue;
        }
        if let Some(replacement) = fuse_local_base_load_tee_load8_branch_tail(code, cursor) {
            code[cursor] = replacement;
            continue;
        }
        if let Some(replacement) = fuse_local_add_set_load8_eqz_branch_tail(code, cursor) {
            code[cursor] = replacement;
            continue;
        }
        if let Some(replacement) =
            fuse_load8_update_branch_taken_local_get4(code, cursor, &label_code_indices)
        {
            code[cursor] = replacement;
            continue;
        }
        if let Some(replacement) = fuse_load8_update_branch_fallthrough_local_get4(code, cursor) {
            code[cursor] = replacement;
            continue;
        }
        if let Some(replacement) = fuse_i32_load16_u_local_base_eq_search_loop_entry(code, cursor) {
            code[cursor] = replacement;
            continue;
        }
        if let Some(replacement) =
            fuse_i32_load8_u_local_base_masked_search_loop_entry(code, cursor)
        {
            code[cursor] = replacement;
        }
    }
}

fn fuse_local_get4_run_skip(code: &[LoweredOp], cursor: usize) -> Option<LoweredOp> {
    let first = code.get(cursor)?;
    let mut locals = local_get4_run_operands(first)?;
    let mut consumed_tail_slots = 0usize;
    let mut next = cursor.checked_add(1)?;
    while locals.len() < 16 {
        let Some(op) = code.get(next) else {
            break;
        };
        if op.label.is_some() {
            break;
        }
        let Some(mut next_locals) = local_get4_run_operands(op) else {
            break;
        };
        if locals.len() + next_locals.len() > 16 {
            break;
        }
        consumed_tail_slots = consumed_tail_slots.saturating_add(1 + op.operands.len());
        locals.append(&mut next_locals);
        next += 1;
    }
    if locals.len() < 4 || consumed_tail_slots == 0 {
        return None;
    }
    let mut operands = Vec::with_capacity(locals.len() + 2);
    operands.push(raw_u32_operand(
        u32::try_from(locals.len()).expect("local.get run count exceeds u32::MAX"),
    ));
    operands.extend(locals);
    let skip_slots = operands
        .len()
        .saturating_add(1)
        .saturating_add(consumed_tail_slots);
    operands.push(raw_u32_operand(
        u32::try_from(skip_slots).expect("local.get run skip exceeds u32::MAX"),
    ));
    Some(LoweredOp {
        label: first.label,
        op: vm::op_local_get4_run_skip as Op,
        operands,
    })
}

fn local_get4_run_operands(op: &LoweredOp) -> Option<Vec<LoweredOperand>> {
    if std::ptr::fn_addr_eq(op.op, vm::op_local_get4 as Op) {
        return Some(vec![op.operands.first()?.clone()]);
    }
    if std::ptr::fn_addr_eq(op.op, vm::op_local_get4_local_get4 as Op) {
        return Some(vec![
            op.operands.first()?.clone(),
            op.operands.get(1)?.clone(),
        ]);
    }
    if std::ptr::fn_addr_eq(op.op, vm::op_local_get4_local_get4_local_get4 as Op) {
        return Some(vec![
            op.operands.first()?.clone(),
            op.operands.get(1)?.clone(),
            op.operands.get(2)?.clone(),
        ]);
    }
    if !std::ptr::fn_addr_eq(op.op, vm::op_local_get4_run as Op) {
        return None;
    }
    let LoweredOperand::Raw(encoded_count) = op.operands.first()? else {
        return None;
    };
    let count = unsafe {
        Operand {
            encoded: *encoded_count,
        }
        .u32 as usize
    };
    if count == 0 || op.operands.len() < 1 + count {
        return None;
    }
    Some(op.operands[1..1 + count].to_vec())
}

fn fuse_local_get4x3_add_const_binop_add_consumer(
    code: &[LoweredOp],
    cursor: usize,
) -> Option<LoweredOp> {
    let get3 = code.get(cursor)?;
    let add_inner = code.get(cursor + 1)?;
    let const_binop = code.get(cursor + 2)?;
    let add_outer = code.get(cursor + 3)?;
    let consumer = code.get(cursor + 4)?;
    if [add_inner, const_binop, add_outer, consumer]
        .iter()
        .any(|op| op.label.is_some())
    {
        return None;
    }
    if !std::ptr::fn_addr_eq(get3.op, vm::op_local_get4_local_get4_local_get4 as Op)
        || !std::ptr::fn_addr_eq(add_inner.op, vm::op_i32_add as Op)
        || !std::ptr::fn_addr_eq(const_binop.op, vm::op_i32_const_binop as Op)
        || !std::ptr::fn_addr_eq(add_outer.op, vm::op_i32_add as Op)
    {
        return None;
    }
    if std::ptr::fn_addr_eq(consumer.op, vm::op_local_tee4 as Op) {
        let store_value = code.get(cursor + 5);
        let store = code.get(cursor + 6);
        if store_value.is_some_and(|op| {
            op.label.is_none() && std::ptr::fn_addr_eq(op.op, vm::op_i32_const as Op)
        }) && store.is_some_and(|op| {
            op.label.is_none() && std::ptr::fn_addr_eq(op.op, vm::op_i32_store as Op)
        }) {
            let store_value = store_value?;
            let store = store?;
            if get3.operands.len() != 3
                || const_binop.operands.len() != 2
                || consumer.operands.len() != 1
                || store_value.operands.len() != 1
                || store.operands.len() != 1
            {
                return None;
            }

            let consumed_tail_slots = code[cursor + 1..=cursor + 6]
                .iter()
                .map(|op| 1 + op.operands.len())
                .sum::<usize>();
            let mut operands = Vec::with_capacity(9);
            operands.extend(get3.operands.iter().cloned());
            operands.extend(const_binop.operands.iter().cloned());
            operands.push(consumer.operands[0].clone());
            operands.push(store_value.operands[0].clone());
            operands.push(store.operands[0].clone());
            let skip = operands
                .len()
                .saturating_add(1)
                .saturating_add(consumed_tail_slots);
            operands.push(raw_u32_operand(
                u32::try_from(skip).expect("local arithmetic store fusion skip exceeds u32::MAX"),
            ));

            return Some(LoweredOp {
                label: get3.label,
                op: vm::op_local_get4x3_i32_add_const_binop_i32_add_tee4_i32_const_store as Op,
                operands,
            });
        }
    }

    let (op, dst) = if std::ptr::fn_addr_eq(consumer.op, vm::op_local_set4 as Op) {
        (
            vm::op_local_get4x3_i32_add_const_binop_i32_add_set4 as Op,
            consumer.operands.first()?.clone(),
        )
    } else if std::ptr::fn_addr_eq(consumer.op, vm::op_local_tee4 as Op) {
        (
            vm::op_local_get4x3_i32_add_const_binop_i32_add_tee4 as Op,
            consumer.operands.first()?.clone(),
        )
    } else {
        return None;
    };
    if get3.operands.len() != 3 || const_binop.operands.len() != 2 || consumer.operands.len() != 1 {
        return None;
    }

    let consumed_tail_slots = code[cursor + 1..=cursor + 4]
        .iter()
        .map(|op| 1 + op.operands.len())
        .sum::<usize>();
    let mut operands = Vec::with_capacity(7);
    operands.extend(get3.operands.iter().cloned());
    operands.extend(const_binop.operands.iter().cloned());
    operands.push(dst);
    let skip = operands
        .len()
        .saturating_add(1)
        .saturating_add(consumed_tail_slots);
    operands.push(raw_u32_operand(
        u32::try_from(skip).expect("local arithmetic fusion skip exceeds u32::MAX"),
    ));

    Some(LoweredOp {
        label: get3.label,
        op,
        operands,
    })
}

fn fuse_load8_update_branch_taken_local_get4(
    code: &[LoweredOp],
    cursor: usize,
    label_code_indices: &HashMap<usize, usize>,
) -> Option<LoweredOp> {
    let branch = code.get(cursor)?;
    if !std::ptr::fn_addr_eq(
        branch.op,
        vm::op_i32_load8_u_local_base_set4_local_get4_set4_local_get4_br_if as Op,
    ) {
        return None;
    }
    let target = *label_code_indices.get(&op_jump_target(branch)?)?;
    if target == cursor {
        return None;
    }
    let target_op = code.get(target)?;
    if !std::ptr::fn_addr_eq(target_op.op, vm::op_local_get4 as Op) {
        return None;
    }
    let mut operands = branch.operands.clone();
    operands.push(target_op.operands.first()?.clone());
    Some(LoweredOp {
        label: branch.label,
        op: vm::op_i32_load8_u_local_base_set4_local_get4_set4_local_get4_br_if_taken_local_get4
            as Op,
        operands,
    })
}

fn fuse_load8_update_branch_fallthrough_local_get4(
    code: &[LoweredOp],
    cursor: usize,
) -> Option<LoweredOp> {
    let [branch, fallthrough] = code.get(cursor..cursor + 2)? else {
        return None;
    };
    if !std::ptr::fn_addr_eq(
        branch.op,
        vm::op_i32_load8_u_local_base_set4_local_get4_set4_local_get4_br_if as Op,
    ) || !std::ptr::fn_addr_eq(fallthrough.op, vm::op_local_get4 as Op)
    {
        return None;
    }
    let mut operands = branch.operands.clone();
    operands.push(fallthrough.operands.first()?.clone());
    let skip_slots = u32::try_from(operands.len() + 1 + 1 + fallthrough.operands.len())
        .expect("fused load8 branch fallthrough skip exceeds u32::MAX");
    operands.push(raw_u32_operand(skip_slots));
    Some(LoweredOp {
        label: branch.label,
        op: vm::op_i32_load8_u_local_base_set4_local_get4_set4_local_get4_br_if_fallthrough_local_get4
            as Op,
        operands,
    })
}

fn fuse_i32_load_store_local_base_local_get4_pair(
    code: &[LoweredOp],
    cursor: usize,
) -> Option<LoweredOp> {
    let [load, store] = code.get(cursor..cursor + 2)? else {
        return None;
    };
    let load_kind = lowered_i32_scalar_load_kind(load.op)?;
    let store_kind = lowered_i32_local_base_store_local_get4_kind(store.op)?;
    let mut operands = vec![
        raw_u32_operand(load_kind | (store_kind << 8)),
        load.operands.first()?.clone(),
        store.operands.first()?.clone(),
        store.operands.get(1)?.clone(),
        store.operands.get(2)?.clone(),
        store.operands.get(3)?.clone(),
    ];
    let skip_slots = u32::try_from(operands.len() + 1 + 1 + store.operands.len())
        .expect("fused load/store skip exceeds u32::MAX");
    operands.push(raw_u32_operand(skip_slots));
    Some(LoweredOp {
        label: load.label,
        op: vm::op_i32_load_store_local_base_local_get4 as Op,
        operands,
    })
}

fn fuse_local_base_i32_load_pair_fallthrough_local_get4(
    code: &[LoweredOp],
    cursor: usize,
) -> Option<LoweredOp> {
    let [pair, get] = code.get(cursor..cursor + 2)? else {
        return None;
    };
    if !std::ptr::fn_addr_eq(get.op, vm::op_local_get4 as Op) {
        return None;
    }
    let op = local_base_i32_load_pair_local_get4_family(pair.op)?;
    let mut operands = pair.operands.clone();
    operands.push(get.operands.first()?.clone());
    let skip_slots = u32::try_from(operands.len() + 1 + 1 + get.operands.len())
        .expect("fused local-base load pair/local.get skip exceeds u32::MAX");
    operands.push(raw_u32_operand(skip_slots));
    Some(LoweredOp {
        label: pair.label,
        op,
        operands,
    })
}

fn fuse_local_get4_local_base_i32_load_pair(
    code: &[LoweredOp],
    cursor: usize,
) -> Option<LoweredOp> {
    let [get, pair] = code.get(cursor..cursor + 2)? else {
        return None;
    };
    if !std::ptr::fn_addr_eq(get.op, vm::op_local_get4 as Op) {
        return None;
    }
    let op = local_get4_local_base_i32_load_pair_family(pair.op)?;
    let mut operands = Vec::with_capacity(8);
    operands.push(get.operands.first()?.clone());
    operands.extend(pair.operands.iter().cloned());
    let skip_slots = u32::try_from(operands.len() + 1 + 1 + pair.operands.len())
        .expect("fused local.get/local-base load pair skip exceeds u32::MAX");
    operands.push(raw_u32_operand(skip_slots));
    Some(LoweredOp {
        label: get.label,
        op,
        operands,
    })
}

fn local_base_i32_load_pair_local_get4_family(op: Op) -> Option<Op> {
    if std::ptr::fn_addr_eq(
        op,
        vm::op_i32_load16_u_local_base_local_get4_i32_load16_u as Op,
    ) {
        return Some(vm::op_i32_load16_u_local_base_local_get4_i32_load16_u_local_get4 as Op);
    }
    if std::ptr::fn_addr_eq(
        op,
        vm::op_i32_load16_s_local_base_local_get4_i32_load16_s as Op,
    ) {
        return Some(vm::op_i32_load16_s_local_base_local_get4_i32_load16_s_local_get4 as Op);
    }
    None
}

fn local_get4_local_base_i32_load_pair_family(op: Op) -> Option<Op> {
    if std::ptr::fn_addr_eq(
        op,
        vm::op_i32_load16_u_local_base_local_get4_i32_load16_u as Op,
    ) {
        return Some(vm::op_local_get4_i32_load16_u_local_base_local_get4_i32_load16_u as Op);
    }
    if std::ptr::fn_addr_eq(
        op,
        vm::op_i32_load16_s_local_base_local_get4_i32_load16_s as Op,
    ) {
        return Some(vm::op_local_get4_i32_load16_s_local_base_local_get4_i32_load16_s as Op);
    }
    None
}

fn fuse_local_get4_br_table(code: &[LoweredOp], cursor: usize) -> Option<LoweredOp> {
    let [get, table] = code.get(cursor..cursor + 2)? else {
        return None;
    };
    if !std::ptr::fn_addr_eq(get.op, vm::op_local_get4 as Op)
        || !std::ptr::fn_addr_eq(table.op, vm::op_br_table as Op)
    {
        return None;
    }
    let mut operands = vec![get.operands.first()?.clone()];
    operands.extend(table.operands.iter().cloned());
    Some(LoweredOp {
        label: get.label,
        op: vm::op_local_get4_br_table as Op,
        operands,
    })
}

fn fuse_local_get4_i32_const_add_br_table(code: &[LoweredOp], cursor: usize) -> Option<LoweredOp> {
    let [add, table] = code.get(cursor..cursor + 2)? else {
        return None;
    };
    if !std::ptr::fn_addr_eq(add.op, vm::op_local_get4_i32_const_add as Op)
        || !std::ptr::fn_addr_eq(table.op, vm::op_br_table as Op)
    {
        return None;
    }
    let mut operands = vec![add.operands.first()?.clone(), add.operands.get(1)?.clone()];
    operands.extend(table.operands.iter().cloned());
    Some(LoweredOp {
        label: add.label,
        op: vm::op_local_get4_i32_const_add_br_table as Op,
        operands,
    })
}

fn fuse_i32_inc_load8_update_branch_tail(code: &[LoweredOp], cursor: usize) -> Option<LoweredOp> {
    let [inc, branch] = code.get(cursor..cursor + 2)? else {
        return None;
    };
    if !std::ptr::fn_addr_eq(inc.op, vm::op_i32_inc_local_base as Op)
        || !std::ptr::fn_addr_eq(
            branch.op,
            vm::op_i32_load8_u_local_base_set4_local_get4_set4_local_get4_br_if as Op,
        )
    {
        return None;
    }
    let mut operands = vec![
        inc.operands.first()?.clone(),
        inc.operands.get(1)?.clone(),
        inc.operands.get(2)?.clone(),
        inc.operands.get(3)?.clone(),
        inc.operands.get(4)?.clone(),
        branch.operands.first()?.clone(),
        branch.operands.get(1)?.clone(),
        branch.operands.get(2)?.clone(),
        branch.operands.get(3)?.clone(),
        branch.operands.get(4)?.clone(),
        branch.operands.get(5)?.clone(),
        branch.operands.get(6)?.clone(),
        LoweredOperand::JumpTarget(op_jump_target(branch)?),
    ];
    let skip_slots = u32::try_from(operands.len() + 1 + 1 + branch.operands.len())
        .expect("fused increment/load8 branch skip exceeds u32::MAX");
    operands.push(raw_u32_operand(skip_slots));
    Some(LoweredOp {
        label: inc.label,
        op: vm::op_i32_inc_local_base_i32_load8_u_local_base_set4_local_get4_set4_local_get4_br_if
            as Op,
        operands,
    })
}

fn fuse_guarded_load8_update_branch_tail(code: &[LoweredOp], cursor: usize) -> Option<LoweredOp> {
    let [next_set, guard_cmp, guard_if, branch] = code.get(cursor..cursor + 4)? else {
        return None;
    };
    if !std::ptr::fn_addr_eq(next_set.op, vm::op_local_get4_i32_const_add_set4 as Op)
        || !std::ptr::fn_addr_eq(guard_cmp.op, vm::op_local_cmp32 as Op)
        || !std::ptr::fn_addr_eq(guard_if.op, vm::op_if as Op)
        || !std::ptr::fn_addr_eq(
            branch.op,
            vm::op_i32_load8_u_local_base_set4_local_get4_set4_local_get4_br_if as Op,
        )
    {
        return None;
    }
    if !is_integer_i32_cmp32(guard_cmp.operands.first()?) {
        return None;
    }
    let operands = vec![
        next_set.operands.first()?.clone(),
        next_set.operands.get(1)?.clone(),
        next_set.operands.get(2)?.clone(),
        guard_cmp.operands.first()?.clone(),
        guard_cmp.operands.get(1)?.clone(),
        guard_cmp.operands.get(2)?.clone(),
        LoweredOperand::JumpTarget(op_jump_target(guard_if)?),
        branch.operands.first()?.clone(),
        branch.operands.get(1)?.clone(),
        branch.operands.get(2)?.clone(),
        branch.operands.get(3)?.clone(),
        branch.operands.get(4)?.clone(),
        branch.operands.get(5)?.clone(),
        branch.operands.get(6)?.clone(),
        LoweredOperand::JumpTarget(op_jump_target(branch)?),
    ];
    Some(LoweredOp {
        label: next_set.label,
        op: vm::op_i32_guarded_load8_u_local_base_set4_local_get4_set4_local_get4_br_if as Op,
        operands,
    })
}

fn fuse_guarded_load8_update_branch_tail_taken_local_get4_br_table(
    code: &[LoweredOp],
    cursor: usize,
    label_code_indices: &HashMap<usize, usize>,
) -> Option<LoweredOp> {
    let mut replacement = fuse_guarded_load8_update_branch_tail(code, cursor)?;
    let target = *label_code_indices.get(&op_jump_target(&replacement)?)?;
    replacement
        .operands
        .extend(local_get4_br_table_operands_at(code, target)?);
    replacement.op =
        vm::op_i32_guarded_load8_u_local_base_set4_local_get4_set4_local_get4_br_if_taken_local_get4_br_table
            as Op;
    Some(replacement)
}

fn fuse_guarded_load8_update_branch_tail_false_local_get4_br_table(
    code: &[LoweredOp],
    cursor: usize,
    label_code_indices: &HashMap<usize, usize>,
) -> Option<LoweredOp> {
    let mut replacement = fuse_guarded_load8_update_branch_tail(code, cursor)?;
    let false_target = guarded_load8_false_target(&replacement)?;
    let target = *label_code_indices.get(&false_target)?;
    replacement
        .operands
        .extend(local_get4_br_table_operands_at(code, target)?);
    replacement.op =
        vm::op_i32_guarded_load8_u_local_base_set4_local_get4_set4_local_get4_br_if_false_local_get4_br_table
            as Op;
    Some(replacement)
}

fn fuse_guarded_load8_update_branch_tail_taken_const_compare_br_table(
    code: &[LoweredOp],
    cursor: usize,
    label_code_indices: &HashMap<usize, usize>,
) -> Option<LoweredOp> {
    let mut replacement = fuse_guarded_load8_update_branch_tail(code, cursor)?;
    let target = *label_code_indices.get(&op_jump_target(&replacement)?)?;
    let compare_index = skip_noop_ops_at(code, target)?;
    let [compare, branch] = code.get(compare_index..compare_index + 2)? else {
        return None;
    };
    if !std::ptr::fn_addr_eq(compare.op, vm::op_local_get4_i32_const_compare_br_if as Op)
        || !std::ptr::fn_addr_eq(branch.op, vm::op_br as Op)
    {
        return None;
    }
    let branch_target = op_jump_target(branch)?;
    let table_operands =
        local_get4_br_table_operands_at(code, *label_code_indices.get(&branch_target)?)?;
    replacement
        .operands
        .extend(compare.operands.iter().cloned());
    replacement
        .operands
        .push(LoweredOperand::JumpTarget(branch_target));
    replacement.operands.extend(table_operands);
    replacement.op =
        vm::op_i32_guarded_load8_u_local_base_set4_local_get4_set4_local_get4_br_if_taken_const_compare_br_table
            as Op;
    Some(replacement)
}

fn local_get4_br_table_operands_at(
    code: &[LoweredOp],
    target: usize,
) -> Option<Vec<LoweredOperand>> {
    let target = skip_noop_ops_at(code, target)?;
    let op = code.get(target)?;
    if std::ptr::fn_addr_eq(op.op, vm::op_local_get4_br_table as Op) {
        return Some(op.operands.clone());
    }
    let [get, table] = code.get(target..target + 2)? else {
        return None;
    };
    if !std::ptr::fn_addr_eq(get.op, vm::op_local_get4 as Op)
        || !std::ptr::fn_addr_eq(table.op, vm::op_br_table as Op)
    {
        return None;
    }
    let mut operands = vec![get.operands.first()?.clone()];
    operands.extend(table.operands.iter().cloned());
    Some(operands)
}

fn skip_noop_ops_at(code: &[LoweredOp], mut target: usize) -> Option<usize> {
    while lowered_op_is_noop(code.get(target)?) {
        target = target.checked_add(1)?;
    }
    Some(target)
}

fn lowered_op_is_noop(op: &LoweredOp) -> bool {
    if std::ptr::fn_addr_eq(op.op, vm::op_end as Op) {
        return true;
    }
    if !std::ptr::fn_addr_eq(op.op, vm::op_drop as Op) {
        return false;
    }
    let Some(LoweredOperand::Raw(encoded)) = op.operands.first() else {
        return false;
    };
    unsafe { Operand { encoded: *encoded }.drop_size == 0 }
}

fn fuse_local_base_load_tee_load8_branch_tail(
    code: &[LoweredOp],
    cursor: usize,
) -> Option<LoweredOp> {
    let [load_ptr, load_branch] = code.get(cursor..cursor + 2)? else {
        return None;
    };
    if !std::ptr::fn_addr_eq(load_ptr.op, vm::op_i32_load_local_base_tee4 as Op)
        || !std::ptr::fn_addr_eq(load_branch.op, vm::op_i32_load8_u_tee4_br_if as Op)
    {
        return None;
    }
    Some(LoweredOp {
        label: load_ptr.label,
        op: vm::op_i32_load_local_base_tee4_i32_load8_u_tee4_br_if as Op,
        operands: vec![
            load_ptr.operands.first()?.clone(),
            load_ptr.operands.get(1)?.clone(),
            load_ptr.operands.get(2)?.clone(),
            load_ptr.operands.get(3)?.clone(),
            load_branch.operands.first()?.clone(),
            load_branch.operands.get(1)?.clone(),
            LoweredOperand::JumpTarget(op_jump_target(load_branch)?),
        ],
    })
}

fn fuse_local_add_set_load8_eqz_branch_tail(
    code: &[LoweredOp],
    cursor: usize,
) -> Option<LoweredOp> {
    let [add_set, load_branch] = code.get(cursor..cursor + 2)? else {
        return None;
    };
    if !std::ptr::fn_addr_eq(add_set.op, vm::op_local_get4_i32_const_add_set4 as Op)
        || !std::ptr::fn_addr_eq(
            load_branch.op,
            vm::op_i32_load8_u_local_base_tee4_i32_eqz_br_if as Op,
        )
    {
        return None;
    }
    Some(LoweredOp {
        label: add_set.label,
        op: vm::op_local_get4_i32_const_add_set4_i32_load8_u_local_base_tee4_i32_eqz_br_if as Op,
        operands: vec![
            add_set.operands.first()?.clone(),
            add_set.operands.get(1)?.clone(),
            add_set.operands.get(2)?.clone(),
            load_branch.operands.first()?.clone(),
            load_branch.operands.get(1)?.clone(),
            load_branch.operands.get(2)?.clone(),
            load_branch.operands.get(3)?.clone(),
            LoweredOperand::JumpTarget(op_jump_target(load_branch)?),
        ],
    })
}

fn fuse_i32_load16_u_local_base_eq_search_loop_entry(
    code: &[LoweredOp],
    cursor: usize,
) -> Option<LoweredOp> {
    let [compare, next] = code.get(cursor..cursor + 2)? else {
        return None;
    };
    if !std::ptr::fn_addr_eq(
        compare.op,
        vm::op_i32_load_local_base_set4_i32_load16_u_local_base_local_eq_br_if as Op,
    ) || !std::ptr::fn_addr_eq(next.op, vm::op_i32_load_local_base_tee4_br_if as Op)
        || op_jump_target(next).is_none()
    {
        return None;
    }
    if !same_local_operand(compare.operands.first(), next.operands.first())
        || !same_local_operand(compare.operands.first(), next.operands.get(3))
    {
        return None;
    }
    let mut operands = vec![
        compare.operands.first()?.clone(),
        compare.operands.get(1)?.clone(),
        compare.operands.get(2)?.clone(),
        compare.operands.get(3)?.clone(),
        compare.operands.get(4)?.clone(),
        compare.operands.get(5)?.clone(),
        compare.operands.get(6)?.clone(),
        next.operands.get(1)?.clone(),
        next.operands.get(2)?.clone(),
        LoweredOperand::JumpTarget(op_jump_target(compare)?),
    ];
    let op = if code
        .get(cursor + 2)
        .is_some_and(|miss| std::ptr::fn_addr_eq(miss.op, vm::op_br as Op))
    {
        operands.push(LoweredOperand::JumpTarget(op_jump_target(
            code.get(cursor + 2)?,
        )?));
        vm::op_i32_load_local_base_set4_i32_load16_u_local_base_local_eq_search_loop as Op
    } else {
        vm::op_i32_load_local_base_set4_i32_load16_u_local_base_local_eq_search_loop_fallthrough
            as Op
    };
    Some(LoweredOp {
        label: compare.label,
        op,
        operands,
    })
}

fn fuse_i32_load8_u_local_base_masked_search_loop_entry(
    code: &[LoweredOp],
    cursor: usize,
) -> Option<LoweredOp> {
    let [compare, next] = code.get(cursor..cursor + 2)? else {
        return None;
    };
    if !std::ptr::fn_addr_eq(
        compare.op,
        vm::op_i32_load_local_base_set4_i32_load8_u_local_base_local_masked_compare_br_if as Op,
    ) || !std::ptr::fn_addr_eq(next.op, vm::op_i32_load_local_base_tee4_br_if as Op)
        || op_jump_target(next).is_none()
    {
        return None;
    }
    if !same_local_operand(compare.operands.first(), next.operands.first())
        || !same_local_operand(compare.operands.first(), next.operands.get(3))
    {
        return None;
    }
    let mut operands = vec![
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
        LoweredOperand::JumpTarget(op_jump_target(compare)?),
    ];
    let op = if code
        .get(cursor + 2)
        .is_some_and(|miss| std::ptr::fn_addr_eq(miss.op, vm::op_br as Op))
    {
        operands.push(LoweredOperand::JumpTarget(op_jump_target(
            code.get(cursor + 2)?,
        )?));
        vm::op_i32_load_local_base_set4_i32_load8_u_local_base_local_masked_search_loop as Op
    } else {
        vm::op_i32_load_local_base_set4_i32_load8_u_local_base_local_masked_search_loop_fallthrough
            as Op
    };
    Some(LoweredOp {
        label: compare.label,
        op,
        operands,
    })
}

fn same_local_operand(lhs: Option<&LoweredOperand>, rhs: Option<&LoweredOperand>) -> bool {
    raw_local_addr(lhs).is_some_and(|lhs| Some(lhs) == raw_local_addr(rhs))
}

fn raw_local_addr(operand: Option<&LoweredOperand>) -> Option<u32> {
    let LoweredOperand::Raw(encoded) = operand? else {
        return None;
    };
    Some(unsafe { Operand { encoded: *encoded }.local_addr })
}

fn raw_u32_operand(value: u32) -> LoweredOperand {
    LoweredOperand::Raw(unsafe { Operand { u32: value }.encoded })
}

fn lowered_i32_scalar_load_kind(op: Op) -> Option<u32> {
    if std::ptr::fn_addr_eq(op, vm::op_i32_load as Op) {
        Some(0)
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_load8_s as Op) {
        Some(1)
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_load8_u as Op) {
        Some(2)
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_load16_s as Op) {
        Some(3)
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_load16_u as Op) {
        Some(4)
    } else {
        None
    }
}

fn guarded_load8_false_target(op: &LoweredOp) -> Option<usize> {
    let LoweredOperand::JumpTarget(target) = op.operands.get(6)? else {
        return None;
    };
    Some(*target)
}

fn lowered_i32_local_base_store_local_get4_kind(op: Op) -> Option<u32> {
    if std::ptr::fn_addr_eq(op, vm::op_i32_store_local_base_local_get4 as Op) {
        Some(0)
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_store8_local_base_local_get4 as Op) {
        Some(1)
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_store16_local_base_local_get4 as Op) {
        Some(2)
    } else {
        None
    }
}

fn is_integer_i32_cmp32(kind_operand: &LoweredOperand) -> bool {
    let LoweredOperand::Raw(encoded) = kind_operand else {
        return false;
    };
    let kind = unsafe { Operand { encoded: *encoded }.u32 };
    let Some((op, _)) = decode_local_cmp32_kind(kind) else {
        return false;
    };
    matches!(
        op,
        LocalCmp32Op::I32Eq
            | LocalCmp32Op::I32Ne
            | LocalCmp32Op::I32LtS
            | LocalCmp32Op::I32LtU
            | LocalCmp32Op::I32GtS
            | LocalCmp32Op::I32GtU
            | LocalCmp32Op::I32LeS
            | LocalCmp32Op::I32LeU
            | LocalCmp32Op::I32GeS
            | LocalCmp32Op::I32GeU
    )
}

fn op_jump_target(op: &LoweredOp) -> Option<usize> {
    let LoweredOperand::JumpTarget(target) = op.operands.last()? else {
        return None;
    };
    Some(*target)
}

fn build_const_pool(code: &mut [LoweredOp]) -> Vec<[u8; 8]> {
    let mut counts = HashMap::<[u8; 8], usize>::new();
    for op in code.iter() {
        for (operand_index, operand) in op.operands.iter().enumerate() {
            if let LoweredOperand::Raw(encoded) = operand {
                if !poolable_raw_operand(op.op, operand_index, &op.operands) {
                    continue;
                }
                *counts.entry(*encoded).or_default() += 1;
            }
        }
    }

    let mut const_pool = Vec::new();
    let mut indices = HashMap::<[u8; 8], u32>::new();
    for op in code.iter_mut() {
        let raw_operand_mask = (0..op.operands.len())
            .map(|operand_index| poolable_raw_operand(op.op, operand_index, &op.operands))
            .collect::<Vec<_>>();
        for (operand_index, operand) in op.operands.iter_mut().enumerate() {
            let LoweredOperand::Raw(encoded) = operand else {
                continue;
            };
            if !raw_operand_mask[operand_index] {
                continue;
            }
            if counts.get(encoded).copied().unwrap_or_default() < 2 {
                continue;
            }
            let index = *indices.entry(*encoded).or_insert_with(|| {
                let index =
                    u32::try_from(const_pool.len()).expect("const pool length exceeds u32::MAX");
                const_pool.push(*encoded);
                index
            });
            *operand = LoweredOperand::ConstPoolRef(index);
        }
    }
    const_pool
}

fn poolable_raw_operand(op: Op, operand_index: usize, operands: &[LoweredOperand]) -> bool {
    if operand_index == 0
        && (std::ptr::fn_addr_eq(op, vm::op_i32_const as Op)
            || std::ptr::fn_addr_eq(op, vm::op_i64_const as Op)
            || std::ptr::fn_addr_eq(op, vm::op_f32_const as Op)
            || std::ptr::fn_addr_eq(op, vm::op_f64_const as Op))
    {
        return true;
    }

    if operand_index == 1
        && (std::ptr::fn_addr_eq(op, vm::op_local_get4_i32_const_add as Op)
            || std::ptr::fn_addr_eq(op, vm::op_local_get4_i32_const_add_set4 as Op)
            || std::ptr::fn_addr_eq(op, vm::op_local_get4_i32_const_add_tee4 as Op)
            || std::ptr::fn_addr_eq(op, vm::op_local_get4_i32_const_add_br_if as Op)
            || std::ptr::fn_addr_eq(op, vm::op_local_get4_i32_const_add_tee4_br_if as Op))
    {
        return true;
    }

    if operand_index == 2
        && std::ptr::fn_addr_eq(op, vm::op_local_get4_i32_const_compare_br_if as Op)
    {
        return true;
    }

    if operand_index != 2 {
        return false;
    }

    let Some(LoweredOperand::Raw(kind_raw)) = operands.first() else {
        return false;
    };
    let kind = unsafe { Operand { encoded: *kind_raw }.u32 };
    if std::ptr::fn_addr_eq(op, vm::op_local_binop32 as Op)
        || std::ptr::fn_addr_eq(op, vm::op_local_binop32_set4 as Op)
        || std::ptr::fn_addr_eq(op, vm::op_local_binop32_tee4 as Op)
        || std::ptr::fn_addr_eq(op, vm::op_local_binop32_br_if as Op)
    {
        if let Some((_, rhs_shape)) = decode_local_binop32_kind(kind) {
            return rhs_shape == LocalFastRhsShape::Const;
        }
    }

    if std::ptr::fn_addr_eq(op, vm::op_local_binop64 as Op)
        || std::ptr::fn_addr_eq(op, vm::op_local_binop64_set8 as Op)
        || std::ptr::fn_addr_eq(op, vm::op_local_binop64_tee8 as Op)
    {
        if let Some((_, rhs_shape)) = decode_local_binop64_kind(kind) {
            return rhs_shape == LocalFastRhsShape::Const;
        }
    }

    if std::ptr::fn_addr_eq(op, vm::op_local_cmp32 as Op)
        || std::ptr::fn_addr_eq(op, vm::op_local_cmp32_br_if as Op)
    {
        if let Some((_, rhs_shape)) = decode_local_cmp32_kind(kind) {
            return rhs_shape == LocalFastRhsShape::Const;
        }
    }

    if std::ptr::fn_addr_eq(op, vm::op_local_cmp64 as Op)
        || std::ptr::fn_addr_eq(op, vm::op_local_cmp64_br_if as Op)
    {
        if let Some((_, rhs_shape)) = decode_local_cmp64_kind(kind) {
            return rhs_shape == LocalFastRhsShape::Const;
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        common::MemArg,
        parser::core::optimizer::pipeline::lower::{
            LoweredKernelBlock, LoweredKernelFunction, LoweredKernelOp,
        },
    };

    fn raw_local(local_addr: u32) -> LoweredOperand {
        LoweredOperand::Raw(unsafe { Operand { local_addr }.encoded })
    }

    fn raw_i32(value: i32) -> LoweredOperand {
        LoweredOperand::Raw(unsafe { Operand { i32: value }.encoded })
    }

    fn raw_u32(value: u32) -> LoweredOperand {
        LoweredOperand::Raw(unsafe { Operand { u32: value }.encoded })
    }

    fn raw_memarg(offset: u32) -> LoweredOperand {
        LoweredOperand::Raw(unsafe {
            Operand {
                memarg: MemArg { align: 0, offset },
            }
            .encoded
        })
    }

    #[test]
    fn fuses_i32_load_store_pair_when_store_starts_labeled_block() {
        let lowered = encode(LoweredKernelFunction {
            blocks: vec![
                LoweredKernelBlock {
                    block_id: 0,
                    label: 0,
                    ops: vec![LoweredKernelOp {
                        label: Some(0),
                        op: vm::op_i32_load as Op,
                        operands: vec![raw_memarg(0)],
                        family: "test.load",
                    }],
                },
                LoweredKernelBlock {
                    block_id: 1,
                    label: 1,
                    ops: vec![LoweredKernelOp {
                        label: Some(1),
                        op: vm::op_i32_store_local_base_local_get4 as Op,
                        operands: vec![raw_local(0), raw_i32(0), raw_local(1), raw_memarg(0)],
                        family: "test.store",
                    }],
                },
            ],
        });

        assert!(std::ptr::fn_addr_eq(
            lowered.code[0].op,
            vm::op_i32_load_store_local_base_local_get4 as Op
        ));
        assert_eq!(lowered.code[1].label, Some(1));
        assert!(std::ptr::fn_addr_eq(
            lowered.code[1].op,
            vm::op_i32_store_local_base_local_get4 as Op
        ));
        let materialized = lowered.materialize();
        assert!(std::ptr::fn_addr_eq(
            unsafe { materialized.instrs[0].op },
            vm::op_i32_load_store_local_base_local_get4 as Op
        ));
        assert!(verify(&lowered));
    }

    #[test]
    fn fuses_local_base_load_pair_followed_by_local_get4() {
        let lowered = encode(LoweredKernelFunction {
            blocks: vec![
                LoweredKernelBlock {
                    block_id: 0,
                    label: 0,
                    ops: vec![LoweredKernelOp {
                        label: Some(0),
                        op: vm::op_i32_load16_s_local_base_local_get4_i32_load16_s as Op,
                        operands: vec![
                            raw_local(0),
                            raw_i32(2),
                            raw_memarg(0),
                            raw_local(4),
                            raw_memarg(6),
                        ],
                        family: "test.load_pair",
                    }],
                },
                LoweredKernelBlock {
                    block_id: 1,
                    label: 1,
                    ops: vec![LoweredKernelOp {
                        label: Some(1),
                        op: vm::op_local_get4 as Op,
                        operands: vec![raw_local(8)],
                        family: "test.get",
                    }],
                },
            ],
        });

        assert!(std::ptr::fn_addr_eq(
            lowered.code[0].op,
            vm::op_i32_load16_s_local_base_local_get4_i32_load16_s_local_get4 as Op
        ));
        assert!(std::ptr::fn_addr_eq(
            lowered.code[1].op,
            vm::op_local_get4 as Op
        ));
        assert_eq!(lowered.code[1].label, Some(1));
        let materialized = lowered.materialize();
        assert_eq!(materialized.op_lens[0], 8);
        assert_eq!(
            unsafe { materialized.instrs[7].operand.u32 },
            9,
            "fused handler must skip its operands and the consumed local.get"
        );
        assert!(verify(&lowered));
    }

    #[test]
    fn fuses_local_get4_followed_by_local_base_load_pair() {
        let lowered = encode(LoweredKernelFunction {
            blocks: vec![
                LoweredKernelBlock {
                    block_id: 0,
                    label: 0,
                    ops: vec![LoweredKernelOp {
                        label: Some(0),
                        op: vm::op_local_get4 as Op,
                        operands: vec![raw_local(8)],
                        family: "test.get",
                    }],
                },
                LoweredKernelBlock {
                    block_id: 1,
                    label: 1,
                    ops: vec![LoweredKernelOp {
                        label: Some(1),
                        op: vm::op_i32_load16_s_local_base_local_get4_i32_load16_s as Op,
                        operands: vec![
                            raw_local(0),
                            raw_i32(2),
                            raw_memarg(0),
                            raw_local(4),
                            raw_memarg(6),
                        ],
                        family: "test.load_pair",
                    }],
                },
            ],
        });

        assert!(std::ptr::fn_addr_eq(
            lowered.code[0].op,
            vm::op_local_get4_i32_load16_s_local_base_local_get4_i32_load16_s as Op
        ));
        assert_eq!(lowered.code[1].label, Some(1));
        let materialized = lowered.materialize();
        assert_eq!(materialized.op_lens[0], 8);
        assert_eq!(
            unsafe { materialized.instrs[7].operand.u32 },
            13,
            "fused handler must skip its operands and the consumed load pair"
        );
        assert!(verify(&lowered));
    }

    #[test]
    fn fuses_consecutive_local_get4_groups_into_skip_run() {
        let lowered = encode(LoweredKernelFunction {
            blocks: vec![LoweredKernelBlock {
                block_id: 0,
                label: 0,
                ops: vec![
                    LoweredKernelOp {
                        label: Some(0),
                        op: vm::op_local_get4_local_get4_local_get4 as Op,
                        operands: vec![raw_local(0), raw_local(4), raw_local(8)],
                        family: "test.get3",
                    },
                    LoweredKernelOp {
                        label: None,
                        op: vm::op_local_get4_local_get4_local_get4 as Op,
                        operands: vec![raw_local(12), raw_local(16), raw_local(20)],
                        family: "test.get3",
                    },
                ],
            }],
        });

        assert!(std::ptr::fn_addr_eq(
            lowered.code[0].op,
            vm::op_local_get4_run_skip as Op
        ));
        assert_eq!(lowered.code[0].operands.len(), 8);
        let Some(LoweredOperand::Raw(count)) = lowered.code[0].operands.first() else {
            panic!("run must encode count")
        };
        assert_eq!(unsafe { Operand { encoded: *count }.u32 }, 6);
        let Some(LoweredOperand::Raw(skip)) = lowered.code[0].operands.last() else {
            panic!("run must encode skip")
        };
        assert_eq!(unsafe { Operand { encoded: *skip }.u32 }, 12);
        assert!(verify(&lowered));
    }

    #[test]
    fn fuses_i32_inc_load8_update_branch_tail_after_lowering() {
        let lowered = encode(LoweredKernelFunction {
            blocks: vec![LoweredKernelBlock {
                block_id: 0,
                label: 0,
                ops: vec![
                    LoweredKernelOp {
                        label: Some(0),
                        op: vm::op_i32_inc_local_base as Op,
                        operands: vec![
                            raw_local(0),
                            raw_i32(0),
                            raw_i32(4),
                            raw_memarg(0),
                            raw_memarg(0),
                        ],
                        family: "test.inc",
                    },
                    LoweredKernelOp {
                        label: None,
                        op: vm::op_i32_load8_u_local_base_set4_local_get4_set4_local_get4_br_if
                            as Op,
                        operands: vec![
                            raw_local(1),
                            raw_i32(0),
                            raw_memarg(0),
                            raw_local(2),
                            raw_local(3),
                            raw_local(1),
                            raw_local(2),
                            LoweredOperand::JumpTarget(0),
                        ],
                        family: "test.branch",
                    },
                ],
            }],
        });

        assert!(std::ptr::fn_addr_eq(
            lowered.code[0].op,
            vm::op_i32_inc_local_base_i32_load8_u_local_base_set4_local_get4_set4_local_get4_br_if
                as Op
        ));
        assert!(std::ptr::fn_addr_eq(
            lowered.code[1].op,
            vm::op_i32_load8_u_local_base_set4_local_get4_set4_local_get4_br_if as Op
        ));
        let Some(LoweredOperand::Raw(skip)) = lowered.code[0].operands.get(13) else {
            panic!("fused op must encode a raw skip slot")
        };
        assert_eq!(unsafe { Operand { encoded: *skip }.u32 }, 23);
        let materialized = lowered.materialize();
        assert!(std::ptr::fn_addr_eq(
            unsafe { materialized.instrs[0].op },
            vm::op_i32_inc_local_base_i32_load8_u_local_base_set4_local_get4_set4_local_get4_br_if
                as Op
        ));
        assert!(verify(&lowered));
    }

    #[test]
    fn fuses_guarded_load8_update_branch_tail_after_lowering() {
        let lowered = encode(LoweredKernelFunction {
            blocks: vec![
                LoweredKernelBlock {
                    block_id: 0,
                    label: 0,
                    ops: vec![
                        LoweredKernelOp {
                            label: Some(0),
                            op: vm::op_local_get4_i32_const_add_set4 as Op,
                            operands: vec![raw_local(1), raw_i32(1), raw_local(3)],
                            family: "test.next",
                        },
                        LoweredKernelOp {
                            label: None,
                            op: vm::op_local_cmp32 as Op,
                            operands: vec![raw_u32(3), raw_local(2), raw_i32(1)],
                            family: "test.guard",
                        },
                        LoweredKernelOp {
                            label: Some(1),
                            op: vm::op_if as Op,
                            operands: vec![LoweredOperand::JumpTarget(2)],
                            family: "test.if",
                        },
                        LoweredKernelOp {
                            label: Some(3),
                            op: vm::op_i32_load8_u_local_base_set4_local_get4_set4_local_get4_br_if
                                as Op,
                            operands: vec![
                                raw_local(1),
                                raw_i32(0),
                                raw_memarg(0),
                                raw_local(4),
                                raw_local(3),
                                raw_local(1),
                                raw_local(4),
                                LoweredOperand::JumpTarget(0),
                            ],
                            family: "test.branch",
                        },
                    ],
                },
                LoweredKernelBlock {
                    block_id: 1,
                    label: 2,
                    ops: vec![LoweredKernelOp {
                        label: Some(2),
                        op: vm::op_end as Op,
                        operands: vec![],
                        family: "test.end",
                    }],
                },
            ],
        });

        assert!(std::ptr::fn_addr_eq(
            lowered.code[0].op,
            vm::op_i32_guarded_load8_u_local_base_set4_local_get4_set4_local_get4_br_if as Op
        ));
        assert!(std::ptr::fn_addr_eq(
            lowered.code[1].op,
            vm::op_local_cmp32 as Op
        ));
        assert_eq!(lowered.code[0].operands.len(), 15);
        let materialized = lowered.materialize();
        assert!(std::ptr::fn_addr_eq(
            unsafe { materialized.instrs[0].op },
            vm::op_i32_guarded_load8_u_local_base_set4_local_get4_set4_local_get4_br_if as Op
        ));
        assert!(verify(&lowered));
    }

    #[test]
    fn fuses_guarded_load8_update_branch_tail_taken_br_table_after_lowering() {
        let lowered = encode(LoweredKernelFunction {
            blocks: vec![
                LoweredKernelBlock {
                    block_id: 0,
                    label: 0,
                    ops: vec![
                        LoweredKernelOp {
                            label: Some(0),
                            op: vm::op_end as Op,
                            operands: vec![],
                            family: "test.target.end",
                        },
                        LoweredKernelOp {
                            label: None,
                            op: vm::op_local_get4 as Op,
                            operands: vec![raw_local(2)],
                            family: "test.table.get",
                        },
                        LoweredKernelOp {
                            label: None,
                            op: vm::op_br_table as Op,
                            operands: vec![
                                raw_u32(1),
                                LoweredOperand::JumpTarget(1),
                                LoweredOperand::JumpTarget(2),
                            ],
                            family: "test.table",
                        },
                    ],
                },
                LoweredKernelBlock {
                    block_id: 1,
                    label: 1,
                    ops: vec![
                        LoweredKernelOp {
                            label: Some(1),
                            op: vm::op_local_get4_i32_const_add_set4 as Op,
                            operands: vec![raw_local(1), raw_i32(1), raw_local(3)],
                            family: "test.next",
                        },
                        LoweredKernelOp {
                            label: None,
                            op: vm::op_local_cmp32 as Op,
                            operands: vec![raw_u32(3), raw_local(2), raw_i32(1)],
                            family: "test.guard",
                        },
                        LoweredKernelOp {
                            label: None,
                            op: vm::op_if as Op,
                            operands: vec![LoweredOperand::JumpTarget(2)],
                            family: "test.if",
                        },
                        LoweredKernelOp {
                            label: None,
                            op: vm::op_i32_load8_u_local_base_set4_local_get4_set4_local_get4_br_if
                                as Op,
                            operands: vec![
                                raw_local(1),
                                raw_i32(0),
                                raw_memarg(0),
                                raw_local(4),
                                raw_local(3),
                                raw_local(1),
                                raw_local(4),
                                LoweredOperand::JumpTarget(0),
                            ],
                            family: "test.branch",
                        },
                    ],
                },
                LoweredKernelBlock {
                    block_id: 2,
                    label: 2,
                    ops: vec![LoweredKernelOp {
                        label: Some(2),
                        op: vm::op_end as Op,
                        operands: vec![],
                        family: "test.end",
                    }],
                },
            ],
        });

        assert!(std::ptr::fn_addr_eq(
            lowered.code[1].op,
            vm::op_local_get4_br_table as Op
        ));
        assert!(std::ptr::fn_addr_eq(
            lowered.code[3].op,
            vm::op_i32_guarded_load8_u_local_base_set4_local_get4_set4_local_get4_br_if_taken_local_get4_br_table as Op
        ));
        assert_eq!(lowered.code[3].operands.len(), 19);
        assert!(verify(&lowered));
    }

    #[test]
    fn fuses_guarded_load8_update_branch_tail_false_br_table_after_lowering() {
        let lowered = encode(LoweredKernelFunction {
            blocks: vec![
                LoweredKernelBlock {
                    block_id: 0,
                    label: 0,
                    ops: vec![
                        LoweredKernelOp {
                            label: Some(0),
                            op: vm::op_end as Op,
                            operands: vec![],
                            family: "test.target.end",
                        },
                        LoweredKernelOp {
                            label: None,
                            op: vm::op_local_get4 as Op,
                            operands: vec![raw_local(2)],
                            family: "test.table.get",
                        },
                        LoweredKernelOp {
                            label: None,
                            op: vm::op_br_table as Op,
                            operands: vec![
                                raw_u32(1),
                                LoweredOperand::JumpTarget(1),
                                LoweredOperand::JumpTarget(2),
                            ],
                            family: "test.table",
                        },
                    ],
                },
                LoweredKernelBlock {
                    block_id: 1,
                    label: 1,
                    ops: vec![
                        LoweredKernelOp {
                            label: Some(1),
                            op: vm::op_local_get4_i32_const_add_set4 as Op,
                            operands: vec![raw_local(1), raw_i32(1), raw_local(3)],
                            family: "test.next",
                        },
                        LoweredKernelOp {
                            label: None,
                            op: vm::op_local_cmp32 as Op,
                            operands: vec![raw_u32(3), raw_local(2), raw_i32(1)],
                            family: "test.guard",
                        },
                        LoweredKernelOp {
                            label: None,
                            op: vm::op_if as Op,
                            operands: vec![LoweredOperand::JumpTarget(0)],
                            family: "test.if",
                        },
                        LoweredKernelOp {
                            label: None,
                            op: vm::op_i32_load8_u_local_base_set4_local_get4_set4_local_get4_br_if
                                as Op,
                            operands: vec![
                                raw_local(1),
                                raw_i32(0),
                                raw_memarg(0),
                                raw_local(4),
                                raw_local(3),
                                raw_local(1),
                                raw_local(4),
                                LoweredOperand::JumpTarget(2),
                            ],
                            family: "test.branch",
                        },
                    ],
                },
                LoweredKernelBlock {
                    block_id: 2,
                    label: 2,
                    ops: vec![LoweredKernelOp {
                        label: Some(2),
                        op: vm::op_end as Op,
                        operands: vec![],
                        family: "test.end",
                    }],
                },
            ],
        });

        assert!(std::ptr::fn_addr_eq(
            lowered.code[1].op,
            vm::op_local_get4_br_table as Op
        ));
        assert!(std::ptr::fn_addr_eq(
            lowered.code[3].op,
            vm::op_i32_guarded_load8_u_local_base_set4_local_get4_set4_local_get4_br_if_false_local_get4_br_table as Op
        ));
        assert_eq!(lowered.code[3].operands.len(), 19);
        assert!(verify(&lowered));
    }

    #[test]
    fn fuses_guarded_load8_update_branch_tail_taken_const_compare_br_table_after_lowering() {
        let lowered = encode(LoweredKernelFunction {
            blocks: vec![
                LoweredKernelBlock {
                    block_id: 0,
                    label: 0,
                    ops: vec![
                        LoweredKernelOp {
                            label: Some(0),
                            op: vm::op_local_get4 as Op,
                            operands: vec![raw_local(2)],
                            family: "test.table.get",
                        },
                        LoweredKernelOp {
                            label: None,
                            op: vm::op_br_table as Op,
                            operands: vec![
                                raw_u32(1),
                                LoweredOperand::JumpTarget(3),
                                LoweredOperand::JumpTarget(2),
                            ],
                            family: "test.table",
                        },
                    ],
                },
                LoweredKernelBlock {
                    block_id: 1,
                    label: 1,
                    ops: vec![
                        LoweredKernelOp {
                            label: Some(1),
                            op: vm::op_drop as Op,
                            operands: vec![raw_u32(0)],
                            family: "test.noop.drop",
                        },
                        LoweredKernelOp {
                            label: None,
                            op: vm::op_local_get4_i32_const_compare_br_if as Op,
                            operands: vec![
                                raw_local(4),
                                raw_u32(0),
                                raw_i32(0),
                                LoweredOperand::JumpTarget(2),
                            ],
                            family: "test.compare",
                        },
                        LoweredKernelOp {
                            label: None,
                            op: vm::op_br as Op,
                            operands: vec![LoweredOperand::JumpTarget(0)],
                            family: "test.loop.br",
                        },
                    ],
                },
                LoweredKernelBlock {
                    block_id: 2,
                    label: 2,
                    ops: vec![LoweredKernelOp {
                        label: Some(2),
                        op: vm::op_end as Op,
                        operands: vec![],
                        family: "test.exit",
                    }],
                },
                LoweredKernelBlock {
                    block_id: 3,
                    label: 3,
                    ops: vec![
                        LoweredKernelOp {
                            label: Some(3),
                            op: vm::op_local_get4_i32_const_add_set4 as Op,
                            operands: vec![raw_local(1), raw_i32(1), raw_local(3)],
                            family: "test.next",
                        },
                        LoweredKernelOp {
                            label: None,
                            op: vm::op_local_cmp32 as Op,
                            operands: vec![raw_u32(3), raw_local(2), raw_i32(1)],
                            family: "test.guard",
                        },
                        LoweredKernelOp {
                            label: None,
                            op: vm::op_if as Op,
                            operands: vec![LoweredOperand::JumpTarget(2)],
                            family: "test.if",
                        },
                        LoweredKernelOp {
                            label: None,
                            op: vm::op_i32_load8_u_local_base_set4_local_get4_set4_local_get4_br_if
                                as Op,
                            operands: vec![
                                raw_local(1),
                                raw_i32(0),
                                raw_memarg(0),
                                raw_local(4),
                                raw_local(3),
                                raw_local(1),
                                raw_local(4),
                                LoweredOperand::JumpTarget(1),
                            ],
                            family: "test.branch",
                        },
                    ],
                },
            ],
        });

        assert!(std::ptr::fn_addr_eq(
            lowered.code[6].op,
            vm::op_i32_guarded_load8_u_local_base_set4_local_get4_set4_local_get4_br_if_taken_const_compare_br_table as Op
        ));
        assert_eq!(lowered.code[6].operands.len(), 24);
        assert!(verify(&lowered));
    }

    #[test]
    fn fuses_local_get4_br_table_after_lowering() {
        let lowered = encode(LoweredKernelFunction {
            blocks: vec![LoweredKernelBlock {
                block_id: 0,
                label: 0,
                ops: vec![
                    LoweredKernelOp {
                        label: Some(0),
                        op: vm::op_local_get4 as Op,
                        operands: vec![raw_local(1)],
                        family: "test.get",
                    },
                    LoweredKernelOp {
                        label: None,
                        op: vm::op_br_table as Op,
                        operands: vec![
                            raw_u32(1),
                            LoweredOperand::JumpTarget(0),
                            LoweredOperand::JumpTarget(0),
                        ],
                        family: "test.table",
                    },
                ],
            }],
        });

        assert!(std::ptr::fn_addr_eq(
            lowered.code[0].op,
            vm::op_local_get4_br_table as Op
        ));
        assert_eq!(lowered.code[0].operands.len(), 4);
        assert!(std::ptr::fn_addr_eq(
            lowered.code[1].op,
            vm::op_br_table as Op
        ));
        assert!(verify(&lowered));
    }

    #[test]
    fn fuses_local_get4_i32_const_add_br_table_after_lowering() {
        let lowered = encode(LoweredKernelFunction {
            blocks: vec![LoweredKernelBlock {
                block_id: 0,
                label: 0,
                ops: vec![
                    LoweredKernelOp {
                        label: Some(0),
                        op: vm::op_local_get4_i32_const_add as Op,
                        operands: vec![raw_local(1), raw_i32(-43)],
                        family: "test.add",
                    },
                    LoweredKernelOp {
                        label: None,
                        op: vm::op_br_table as Op,
                        operands: vec![
                            raw_u32(2),
                            LoweredOperand::JumpTarget(0),
                            LoweredOperand::JumpTarget(0),
                            LoweredOperand::JumpTarget(0),
                        ],
                        family: "test.table",
                    },
                ],
            }],
        });

        assert!(std::ptr::fn_addr_eq(
            lowered.code[0].op,
            vm::op_local_get4_i32_const_add_br_table as Op
        ));
        assert_eq!(lowered.code[0].operands.len(), 6);
        assert!(verify(&lowered));
    }

    #[test]
    fn fuses_taken_local_get4_into_load8_update_branch() {
        let lowered = encode(LoweredKernelFunction {
            blocks: vec![
                LoweredKernelBlock {
                    block_id: 0,
                    label: 0,
                    ops: vec![LoweredKernelOp {
                        label: Some(0),
                        op: vm::op_i32_load8_u_local_base_set4_local_get4_set4_local_get4_br_if
                            as Op,
                        operands: vec![
                            raw_local(1),
                            raw_i32(0),
                            raw_memarg(0),
                            raw_local(2),
                            raw_local(3),
                            raw_local(1),
                            raw_local(2),
                            LoweredOperand::JumpTarget(1),
                        ],
                        family: "test.branch",
                    }],
                },
                LoweredKernelBlock {
                    block_id: 1,
                    label: 1,
                    ops: vec![LoweredKernelOp {
                        label: Some(1),
                        op: vm::op_local_get4 as Op,
                        operands: vec![raw_local(4)],
                        family: "test.local_get",
                    }],
                },
            ],
        });

        assert!(std::ptr::fn_addr_eq(
            lowered.code[0].op,
            vm::op_i32_load8_u_local_base_set4_local_get4_set4_local_get4_br_if_taken_local_get4
                as Op
        ));
        assert_eq!(lowered.code[0].operands.len(), 9);
        let Some(LoweredOperand::Raw(local)) = lowered.code[0].operands.get(8) else {
            panic!("fused branch must append taken local")
        };
        assert_eq!(unsafe { Operand { encoded: *local }.local_addr }, 4);
        let materialized = lowered.materialize();
        assert!(std::ptr::fn_addr_eq(
            unsafe { materialized.instrs[0].op },
            vm::op_i32_load8_u_local_base_set4_local_get4_set4_local_get4_br_if_taken_local_get4
                as Op
        ));
        assert!(verify(&lowered));
    }

    #[test]
    fn fuses_fallthrough_local_get4_into_load8_update_branch() {
        let lowered = encode(LoweredKernelFunction {
            blocks: vec![LoweredKernelBlock {
                block_id: 0,
                label: 0,
                ops: vec![
                    LoweredKernelOp {
                        label: Some(0),
                        op: vm::op_i32_load8_u_local_base_set4_local_get4_set4_local_get4_br_if
                            as Op,
                        operands: vec![
                            raw_local(1),
                            raw_i32(0),
                            raw_memarg(0),
                            raw_local(2),
                            raw_local(3),
                            raw_local(1),
                            raw_local(2),
                            LoweredOperand::JumpTarget(0),
                        ],
                        family: "test.branch",
                    },
                    LoweredKernelOp {
                        label: None,
                        op: vm::op_local_get4 as Op,
                        operands: vec![raw_local(4)],
                        family: "test.local_get",
                    },
                ],
            }],
        });

        assert!(std::ptr::fn_addr_eq(
            lowered.code[0].op,
            vm::op_i32_load8_u_local_base_set4_local_get4_set4_local_get4_br_if_fallthrough_local_get4
                as Op
        ));
        assert_eq!(lowered.code[0].operands.len(), 10);
        let Some(LoweredOperand::Raw(local)) = lowered.code[0].operands.get(8) else {
            panic!("fused branch must append fallthrough local")
        };
        assert_eq!(unsafe { Operand { encoded: *local }.local_addr }, 4);
        let Some(LoweredOperand::Raw(skip)) = lowered.code[0].operands.get(9) else {
            panic!("fused branch must append fallthrough skip")
        };
        assert_eq!(unsafe { Operand { encoded: *skip }.u32 }, 12);
        assert!(verify(&lowered));
    }
}
