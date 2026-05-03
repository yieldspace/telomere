use super::super::cfg::{BasicBlockProgram, DecodedInstr};
use crate::{
    common::{FuncIdx, FuncType, LoweredOperand, Op, ValType},
    runtime::vm,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct ValueId(pub(crate) usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct EffectId(pub(crate) usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct InstId(pub(crate) usize);

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StorageClass {
    BlockParam,
    Local,
    Immediate,
    Effect,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) struct BlockParam {
    pub(crate) id: ValueId,
    pub(crate) index: usize,
    pub(crate) ty: ValType,
    pub(crate) storage: StorageClass,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) struct CanonInst {
    pub(crate) id: InstId,
    pub(crate) op: Op,
    pub(crate) operands: Vec<LoweredOperand>,
    pub(crate) stack_before: Vec<ValType>,
    pub(crate) stack_after: Vec<ValType>,
    pub(crate) preserved_prefix_len: usize,
    pub(crate) fresh_result_count: usize,
    pub(crate) effect: EffectId,
}

#[derive(Debug, Clone)]
pub(crate) struct CanonBlock {
    pub(crate) id: usize,
    pub(crate) params: Vec<BlockParam>,
    pub(crate) insts: Vec<CanonInst>,
    pub(crate) predecessors: Vec<usize>,
    pub(crate) successors: Vec<usize>,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) struct CanonFunc {
    pub(crate) funcidx: FuncIdx,
    pub(crate) functype: FuncType,
    pub(crate) locals_size: u32,
    pub(crate) entry_block: usize,
    pub(crate) blocks: Vec<CanonBlock>,
}

impl CanonFunc {
    pub(crate) fn from_program(
        funcidx: FuncIdx,
        functype: FuncType,
        locals_size: u32,
        program: &BasicBlockProgram,
    ) -> Self {
        let mut next_value = 0usize;
        let mut next_inst = 0usize;
        let mut next_effect = 0usize;
        let blocks = program
            .blocks
            .iter()
            .map(|block| {
                let params = program.records[block.start]
                    .stack_before
                    .types
                    .iter()
                    .enumerate()
                    .map(|(index, ty)| {
                        let id = ValueId(next_value);
                        next_value += 1;
                        BlockParam {
                            id,
                            index,
                            ty: *ty,
                            storage: StorageClass::BlockParam,
                        }
                    })
                    .collect::<Vec<_>>();
                let insts = program.records[block.start..block.end]
                    .iter()
                    .map(|record| {
                        let id = InstId(next_inst);
                        next_inst += 1;
                        let effect = EffectId(next_effect);
                        next_effect += 1;
                        CanonInst {
                            id,
                            op: record.op,
                            operands: lower_operands(record, program),
                            stack_before: record.stack_before.types.clone(),
                            stack_after: record.stack_after.types.clone(),
                            preserved_prefix_len: record.preserved_prefix_len,
                            fresh_result_count: record.fresh_result_count,
                            effect,
                        }
                    })
                    .collect::<Vec<_>>();
                CanonBlock {
                    id: block.id,
                    params,
                    insts,
                    predecessors: program.predecessors[block.id].clone(),
                    successors: program.successors[block.id].clone(),
                }
            })
            .collect();
        Self {
            funcidx,
            functype,
            locals_size,
            entry_block: 0,
            blocks,
        }
    }

    pub(crate) fn verify(&self) -> bool {
        if self.blocks.is_empty() {
            return false;
        }
        self.blocks.iter().enumerate().all(|(expected, block)| {
            block.id == expected
                && block
                    .successors
                    .iter()
                    .all(|succ| *succ < self.blocks.len())
        })
    }
}

fn lower_operands(record: &DecodedInstr, program: &BasicBlockProgram) -> Vec<LoweredOperand> {
    record
        .operands
        .iter()
        .enumerate()
        .map(|(idx, operand)| lower_operand(record, idx, operand, program))
        .collect()
}

fn lower_operand(
    record: &DecodedInstr,
    idx: usize,
    operand: &crate::common::Operand,
    program: &BasicBlockProgram,
) -> LoweredOperand {
    if is_direct_call_op(record.op) && idx == 0 {
        return LoweredOperand::CallRecipeRef(unsafe { operand.call_recipe_ref });
    }
    if let Some(target) = control_target_operand(record, idx) {
        if let Some(block_id) = program.block_for_old_start(target) {
            return LoweredOperand::JumpTarget(block_id);
        }
    }
    LoweredOperand::Raw(unsafe { operand.encoded })
}

fn control_target_operand(record: &DecodedInstr, idx: usize) -> Option<usize> {
    if (record.op_eq(vm::op_if)
        || record.op_eq(vm::op_else)
        || record.op_eq(vm::op_br)
        || record.op_eq(vm::op_br_if)
        || record.op_eq(vm::op_return))
        && idx == 0
    {
        return Some(record.operand_jump_addr(0));
    }
    if record.op_eq(vm::op_br_table) && idx > 0 {
        return Some(record.operand_jump_addr(idx));
    }
    None
}

fn is_direct_call_op(op: Op) -> bool {
    std::ptr::fn_addr_eq(op, vm::op_call as Op)
        || std::ptr::fn_addr_eq(op, vm::op_call_import as Op)
        || std::ptr::fn_addr_eq(op, vm::op_return_call as Op)
        || std::ptr::fn_addr_eq(op, vm::op_return_call_import as Op)
}
