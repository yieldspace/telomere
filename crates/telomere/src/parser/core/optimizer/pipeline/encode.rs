use std::collections::HashMap;

use super::lower::LoweredKernelFunction;
use crate::common::{
    decode_local_binop32_kind, decode_local_binop64_kind, decode_local_cmp32_kind,
    decode_local_cmp64_kind, LocalFastRhsShape, LoweredBlockMap, LoweredFunction,
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

    fuse_search_loop_entries(&mut code);
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

fn fuse_search_loop_entries(code: &mut [LoweredOp]) {
    for cursor in 0..code.len() {
        if let Some(replacement) = fuse_i32_load_store_local_base_local_get4_pair(code, cursor) {
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
}
