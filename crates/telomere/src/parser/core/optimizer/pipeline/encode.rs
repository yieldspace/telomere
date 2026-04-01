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
