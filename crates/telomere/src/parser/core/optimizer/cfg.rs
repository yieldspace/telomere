use std::collections::{BTreeSet, HashMap};

use crate::{
    common::{Instr, MemArg, Op, Operand},
    parser::core::type_checker::StackSnapshot,
    runtime::vm,
};

#[derive(Debug, Clone)]
pub(crate) struct InstructionMeta {
    pub(crate) start: usize,
    pub(crate) len: usize,
    pub(crate) stack_before: StackSnapshot,
    pub(crate) stack_after: StackSnapshot,
    pub(crate) preserved_prefix_len: usize,
    pub(crate) fresh_result_count: usize,
}

#[derive(Clone)]
pub(crate) struct DecodedInstr {
    pub(crate) old_start: usize,
    pub(crate) op: Op,
    pub(crate) operands: Vec<Operand>,
    pub(crate) stack_before: StackSnapshot,
    pub(crate) stack_after: StackSnapshot,
    pub(crate) preserved_prefix_len: usize,
    pub(crate) fresh_result_count: usize,
}

impl DecodedInstr {
    pub(crate) fn operand_u32(&self, idx: usize) -> u32 {
        unsafe { self.operands[idx].u32 }
    }

    pub(crate) fn operand_i32(&self, idx: usize) -> i32 {
        unsafe { self.operands[idx].i32 }
    }

    pub(crate) fn operand_i64(&self, idx: usize) -> i64 {
        unsafe { self.operands[idx].i64 }
    }

    pub(crate) fn operand_f32(&self, idx: usize) -> f32 {
        unsafe { self.operands[idx].f32 }
    }

    pub(crate) fn operand_f64(&self, idx: usize) -> f64 {
        unsafe { self.operands[idx].f64 }
    }

    pub(crate) fn operand_select(&self) -> u32 {
        unsafe { self.operands[0].select }
    }

    pub(crate) fn operand_local_addr(&self) -> u32 {
        unsafe { self.operands[0].local_addr }
    }

    pub(crate) fn operand_memarg(&self, idx: usize) -> MemArg {
        unsafe { self.operands[idx].memarg }
    }

    pub(crate) fn operand_jump_addr(&self, idx: usize) -> usize {
        unsafe { self.operands[idx].jump_addr as usize }
    }

    pub(crate) fn op_eq(&self, candidate: Op) -> bool {
        std::ptr::fn_addr_eq(self.op, candidate)
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct BasicBlock {
    pub(crate) id: usize,
    pub(crate) start: usize,
    pub(crate) end: usize,
}

#[derive(Clone)]
pub(crate) struct BasicBlockProgram {
    pub(crate) records: Vec<DecodedInstr>,
    pub(crate) blocks: Vec<BasicBlock>,
    pub(crate) old_start_to_block: HashMap<usize, usize>,
    pub(crate) successors: Vec<Vec<usize>>,
    pub(crate) predecessors: Vec<Vec<usize>>,
}

impl BasicBlockProgram {
    pub(crate) fn block(&self, block_id: usize) -> BasicBlock {
        self.blocks[block_id]
    }

    pub(crate) fn block_for_old_start(&self, old_start: usize) -> Option<usize> {
        self.old_start_to_block.get(&old_start).copied()
    }

    pub(crate) fn next_block_id(&self, block_id: usize) -> Option<usize> {
        (block_id + 1 < self.blocks.len()).then_some(block_id + 1)
    }
}

pub(crate) fn build_program(
    instrs: &[Instr],
    meta: Vec<InstructionMeta>,
) -> Option<BasicBlockProgram> {
    let mut records = Vec::with_capacity(meta.len());
    let mut start_to_record = HashMap::with_capacity(meta.len());
    for (record_idx, meta) in meta.into_iter().enumerate() {
        if meta.start + meta.len > instrs.len() || meta.len == 0 {
            return None;
        }
        let op = unsafe { instrs[meta.start].op };
        let mut operands = Vec::with_capacity(meta.len.saturating_sub(1));
        for operand_idx in 1..meta.len {
            operands.push(unsafe { instrs[meta.start + operand_idx].operand });
        }
        start_to_record.insert(meta.start, record_idx);
        records.push(DecodedInstr {
            old_start: meta.start,
            op,
            operands,
            stack_before: meta.stack_before,
            stack_after: meta.stack_after,
            preserved_prefix_len: meta.preserved_prefix_len,
            fresh_result_count: meta.fresh_result_count,
        });
    }
    let (blocks, old_start_to_block) = build_blocks(&records, &start_to_record);
    let (successors, predecessors) = build_edges(&records, &blocks, &old_start_to_block);
    Some(BasicBlockProgram {
        records,
        blocks,
        old_start_to_block,
        successors,
        predecessors,
    })
}

fn build_blocks(
    records: &[DecodedInstr],
    start_to_record: &HashMap<usize, usize>,
) -> (Vec<BasicBlock>, HashMap<usize, usize>) {
    if records.is_empty() {
        return (Vec::new(), HashMap::new());
    }
    let mut leaders = BTreeSet::from([0usize]);
    for (idx, record) in records.iter().enumerate() {
        if idx > 0 && starts_basic_block(record) {
            leaders.insert(idx);
        }
        for target in control_targets(record) {
            if let Some(target_idx) = start_to_record.get(&target) {
                leaders.insert(*target_idx);
            }
        }
        if ends_basic_block(record) && idx + 1 < records.len() {
            leaders.insert(idx + 1);
        }
    }

    let leaders = leaders.into_iter().collect::<Vec<_>>();
    let mut blocks = Vec::with_capacity(leaders.len());
    let mut old_start_to_block = HashMap::with_capacity(leaders.len());
    for (block_id, start) in leaders.iter().copied().enumerate() {
        let end = leaders.get(block_id + 1).copied().unwrap_or(records.len());
        old_start_to_block.insert(records[start].old_start, block_id);
        blocks.push(BasicBlock {
            id: block_id,
            start,
            end,
        });
    }
    (blocks, old_start_to_block)
}

fn build_edges(
    records: &[DecodedInstr],
    blocks: &[BasicBlock],
    old_start_to_block: &HashMap<usize, usize>,
) -> (Vec<Vec<usize>>, Vec<Vec<usize>>) {
    let mut successors = vec![Vec::new(); blocks.len()];
    let mut predecessors = vec![Vec::new(); blocks.len()];
    for block in blocks {
        let mut succs = block_successors(records, *block, old_start_to_block, blocks.len());
        succs.sort_unstable();
        succs.dedup();
        for succ in &succs {
            predecessors[*succ].push(block.id);
        }
        successors[block.id] = succs;
    }
    for preds in &mut predecessors {
        preds.sort_unstable();
        preds.dedup();
    }
    (successors, predecessors)
}

fn block_successors(
    records: &[DecodedInstr],
    block: BasicBlock,
    old_start_to_block: &HashMap<usize, usize>,
    block_count: usize,
) -> Vec<usize> {
    if block.start >= block.end {
        return Vec::new();
    }
    let terminal = &records[block.end - 1];
    let fallthrough = (block.id + 1 < block_count).then_some(block.id + 1);

    if terminal.op_eq(vm::op_br) || terminal.op_eq(vm::op_else) || terminal.op_eq(vm::op_return) {
        return target_block_ids(terminal, old_start_to_block);
    }
    if terminal.op_eq(vm::op_if) || terminal.op_eq(vm::op_br_if) {
        let mut succs = target_block_ids(terminal, old_start_to_block);
        if let Some(next) = fallthrough {
            succs.push(next);
        }
        return succs;
    }
    if terminal.op_eq(vm::op_br_table) {
        return target_block_ids(terminal, old_start_to_block);
    }
    if terminal.op_eq(vm::special_function_return) {
        return Vec::new();
    }
    if terminal.op_eq(vm::special_block_return) {
        return fallthrough.into_iter().collect();
    }
    if let Some(next) = fallthrough {
        return vec![next];
    }
    Vec::new()
}

fn target_block_ids(
    record: &DecodedInstr,
    old_start_to_block: &HashMap<usize, usize>,
) -> Vec<usize> {
    control_targets(record)
        .into_iter()
        .filter_map(|target| old_start_to_block.get(&target).copied())
        .collect()
}

fn starts_basic_block(record: &DecodedInstr) -> bool {
    record.op_eq(vm::op_loop)
        || record.op_eq(vm::op_if)
        || record.op_eq(vm::op_else)
        || record.op_eq(vm::op_end)
        || record.op_eq(vm::special_function_return)
        || record.op_eq(vm::special_block_return)
}

fn control_targets(record: &DecodedInstr) -> Vec<usize> {
    if record.op_eq(vm::op_if) || record.op_eq(vm::op_else) || record.op_eq(vm::op_br) {
        return vec![record.operand_jump_addr(0)];
    }
    if record.op_eq(vm::op_br_if) || record.op_eq(vm::op_return) {
        return vec![record.operand_jump_addr(0)];
    }
    if record.op_eq(vm::op_br_table) {
        return (1..=record.operand_u32(0) as usize + 1)
            .map(|idx| record.operand_jump_addr(idx))
            .collect();
    }
    Vec::new()
}

fn ends_basic_block(record: &DecodedInstr) -> bool {
    record.op_eq(vm::op_if)
        || record.op_eq(vm::op_br)
        || record.op_eq(vm::op_br_if)
        || record.op_eq(vm::op_br_table)
        || record.op_eq(vm::op_return)
        || record.op_eq(vm::op_else)
        || record.op_eq(vm::op_end)
        || record.op_eq(vm::special_function_return)
        || record.op_eq(vm::special_block_return)
}
