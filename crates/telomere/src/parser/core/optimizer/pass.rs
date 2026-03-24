use std::{
    collections::{BTreeSet, HashMap, VecDeque},
    hash::{Hash, Hasher},
};

use crate::{
    common::{FuncIdx, FuncType, Instr, LocalsData, Op, Operand, ValType},
    runtime::vm,
};

use super::{
    cfg::{build_program, BasicBlock, BasicBlockProgram, DecodedInstr, InstructionMeta},
    expr::{
        AliasAddress, AliasKey, AliasSpace, ConstValue, EffectBarrier, EffectEpoch, ExprId,
        ExprOrigin, ExprOriginKind, ExprState, HeapVersion, LocalSlot, PureOpKind, ValueKey,
    },
    sink::{RecordEmit, RewriteSink},
};

pub(crate) trait LocalPass {
    fn run_block(
        &mut self,
        program: &BasicBlockProgram,
        block: BasicBlock,
        entry: &BlockEntryState,
    ) -> Vec<RecordEmit>;
}

#[derive(Clone, Debug)]
struct AbstractValue {
    ty: ValType,
    origin: ExprOrigin,
    const_value: Option<ConstValue>,
    key: Option<ValueKey>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct BlockEntryState {
    reachable: bool,
    locals: HashMap<LocalSlot, AbstractValue>,
    stack: Vec<AbstractValue>,
    heap: HeapVersion,
    aliases: HashMap<AliasKey, AbstractValue>,
}

#[derive(Default)]
struct FunctionAnalysis {
    entries: Vec<BlockEntryState>,
    exits: Vec<BlockEntryState>,
}

const UNKNOWN_HEAP_VERSION: u32 = u32::MAX;
const INSTR_RESULT_ORIGIN_STRIDE: usize = 256;

pub(crate) fn optimize_function(
    funcidx: FuncIdx,
    _functype: &FuncType,
    _locals: &LocalsData,
    instrs: Vec<Instr>,
    meta: Vec<InstructionMeta>,
) -> Vec<Instr> {
    let Some(program) = build_program(&instrs, meta) else {
        return instrs;
    };
    if program.records.iter().any(|record| {
        (record.op_eq(vm::op_call) || record.op_eq(vm::op_return_call))
            && record.operand_u32(0) == funcidx.0
    }) {
        return instrs;
    }
    let analysis = analyze_program(&program);
    let mut pass = BlockOptimizer::default();
    let mut per_block_records = vec![Vec::new(); program.blocks.len()];
    for block in &program.blocks {
        let entry = &analysis.entries[block.id];
        if !entry.reachable {
            continue;
        }
        let rewritten = pass.run_block(&program, *block, entry);
        per_block_records[block.id] = select_superinstructions(rewritten);
    }
    let reachable = reachable_blocks(&program, &per_block_records);
    let mut records = Vec::new();
    for block in &program.blocks {
        if reachable[block.id] {
            records.extend(per_block_records[block.id].clone());
        }
    }
    if patch_jump_targets(&mut records).is_err() {
        return instrs;
    }
    RewriteSink::flatten(&records)
}

fn analyze_program(program: &BasicBlockProgram) -> FunctionAnalysis {
    let mut analysis = FunctionAnalysis {
        entries: vec![BlockEntryState::default(); program.blocks.len()],
        exits: vec![BlockEntryState::default(); program.blocks.len()],
    };
    let mut worklist = VecDeque::new();
    let mut queued = vec![false; program.blocks.len()];
    worklist.push_back(0usize);
    queued[0] = true;

    while let Some(block_id) = worklist.pop_front() {
        queued[block_id] = false;
        let Some(entry) = compute_entry_state(program, &analysis, block_id) else {
            continue;
        };
        let entry_changed = !same_state(&analysis.entries[block_id], &entry);
        if entry_changed {
            analysis.entries[block_id] = entry.clone();
        }
        let exit = transfer_block(program, program.block(block_id), entry);
        let exit_changed = !same_state(&analysis.exits[block_id], &exit);
        if exit_changed {
            analysis.exits[block_id] = exit;
        }
        if entry_changed || exit_changed {
            for succ in &program.successors[block_id] {
                if !queued[*succ] {
                    queued[*succ] = true;
                    worklist.push_back(*succ);
                }
            }
            if block_id == 0 {
                for pred in &program.predecessors[block_id] {
                    if !queued[*pred] {
                        queued[*pred] = true;
                        worklist.push_back(*pred);
                    }
                }
            }
        }
    }

    analysis
}

fn compute_entry_state(
    program: &BasicBlockProgram,
    analysis: &FunctionAnalysis,
    block_id: usize,
) -> Option<BlockEntryState> {
    let block = program.block(block_id);
    let first = program.records.get(block.start)?;
    let mut incoming = Vec::new();
    if block_id == 0 {
        incoming.push(default_entry_state(block_id, first));
    }
    for pred in &program.predecessors[block_id] {
        let pred_state = &analysis.exits[*pred];
        if pred_state.reachable {
            incoming.push(pred_state.clone());
        }
    }
    if incoming.is_empty() {
        return None;
    }
    Some(merge_states(block_id, first, &incoming))
}

fn default_entry_state(block_id: usize, first: &DecodedInstr) -> BlockEntryState {
    BlockEntryState {
        reachable: true,
        stack: first
            .stack_before
            .types
            .iter()
            .enumerate()
            .map(|(ordinal, ty)| AbstractValue {
                ty: *ty,
                origin: ExprOrigin {
                    block_id,
                    ordinal,
                    kind: ExprOriginKind::EntryStack,
                },
                const_value: None,
                key: None,
            })
            .collect(),
        ..BlockEntryState::default()
    }
}

fn merge_states(
    block_id: usize,
    first: &DecodedInstr,
    incoming: &[BlockEntryState],
) -> BlockEntryState {
    let mut state = BlockEntryState {
        reachable: true,
        stack: Vec::with_capacity(first.stack_before.types.len()),
        heap: merge_heap_versions(incoming),
        ..BlockEntryState::default()
    };

    for (ordinal, ty) in first.stack_before.types.iter().enumerate() {
        let values = incoming
            .iter()
            .map(|entry| entry.stack.get(ordinal))
            .collect::<Vec<_>>();
        state.stack.push(merge_value_candidates(
            block_id,
            ordinal,
            *ty,
            &values,
            ExprOriginKind::BlockParam,
        ));
    }

    let mut local_slots = BTreeSet::new();
    for entry in incoming {
        local_slots.extend(entry.locals.keys().copied());
    }
    for slot in local_slots {
        let values = incoming
            .iter()
            .map(|entry| entry.locals.get(&slot))
            .collect::<Vec<_>>();
        state.locals.insert(
            slot,
            merge_value_candidates(
                block_id,
                1024 + slot.addr as usize,
                type_from_slot(slot.size),
                &values,
                ExprOriginKind::BlockParam,
            ),
        );
    }

    let mut alias_keys = if let Some(first_entry) = incoming.first() {
        first_entry.aliases.keys().copied().collect::<BTreeSet<_>>()
    } else {
        BTreeSet::new()
    };
    for entry in incoming.iter().skip(1) {
        alias_keys.retain(|key| entry.aliases.contains_key(key));
    }
    for key in alias_keys {
        if !space_version_stable(key.space, incoming, state.heap) {
            continue;
        }
        let values = incoming
            .iter()
            .map(|entry| entry.aliases.get(&key))
            .collect::<Vec<_>>();
        let Some(first_value) = values.first().and_then(|value| *value) else {
            continue;
        };
        let merged = merge_value_candidates(
            block_id,
            alias_ordinal(key),
            first_value.ty,
            &values,
            ExprOriginKind::BlockParam,
        );
        state.aliases.insert(key, merged);
    }

    state
}

fn merge_heap_versions(incoming: &[BlockEntryState]) -> HeapVersion {
    let memory = join_version(incoming.iter().map(|state| state.heap.memory));
    let global = join_version(incoming.iter().map(|state| state.heap.global));
    let table = join_version(incoming.iter().map(|state| state.heap.table));
    HeapVersion {
        memory,
        global,
        table,
    }
}

fn join_version(values: impl Iterator<Item = u32>) -> u32 {
    let values = values.collect::<Vec<_>>();
    let Some(first) = values.first().copied() else {
        return 0;
    };
    if values.iter().all(|value| *value == first) {
        return first;
    }
    UNKNOWN_HEAP_VERSION
}

fn space_version_stable(
    space: AliasSpace,
    incoming: &[BlockEntryState],
    merged: HeapVersion,
) -> bool {
    incoming.iter().all(|state| match space {
        AliasSpace::Memory => state.heap.memory == merged.memory,
        AliasSpace::Global => state.heap.global == merged.global,
        AliasSpace::Table => state.heap.table == merged.table,
    })
}

fn merge_value_candidates(
    block_id: usize,
    ordinal: usize,
    ty: ValType,
    values: &[Option<&AbstractValue>],
    kind: ExprOriginKind,
) -> AbstractValue {
    let Some(first) = values.first().and_then(|value| *value) else {
        return AbstractValue {
            ty,
            origin: ExprOrigin {
                block_id,
                ordinal,
                kind,
            },
            const_value: None,
            key: None,
        };
    };
    if values
        .iter()
        .all(|value| value.is_some_and(|candidate| same_value(candidate, first)))
    {
        return first.clone();
    }
    let const_value = values
        .iter()
        .map(|value| value.and_then(|value| value.const_value))
        .reduce(|lhs, rhs| if lhs == rhs { lhs } else { None })
        .flatten();
    let key = values
        .iter()
        .map(|value| value.and_then(|value| value.key))
        .reduce(|lhs, rhs| if lhs == rhs { lhs } else { None })
        .flatten();
    AbstractValue {
        ty,
        origin: ExprOrigin {
            block_id,
            ordinal,
            kind,
        },
        const_value,
        key,
    }
}

fn alias_ordinal(key: AliasKey) -> usize {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    key.hash(&mut hasher);
    hasher.finish() as usize
}

fn transfer_block(
    program: &BasicBlockProgram,
    block: BasicBlock,
    mut state: BlockEntryState,
) -> BlockEntryState {
    for record_idx in block.start..block.end {
        let record = &program.records[record_idx];
        let ordinal = record_idx - block.start;
        transfer_record(&mut state, block.id, ordinal, record);
    }
    state
}

fn transfer_record(
    state: &mut BlockEntryState,
    block_id: usize,
    ordinal: usize,
    record: &DecodedInstr,
) {
    if let Some((ty, value)) = decode_const(record) {
        state.stack.push(AbstractValue {
            ty,
            origin: ExprOrigin {
                block_id,
                ordinal,
                kind: ExprOriginKind::SyntheticConst,
            },
            const_value: Some(value),
            key: None,
        });
        return;
    }
    if let Some(slot) = decode_local_get(record) {
        let value = state.locals.get(&slot).cloned().unwrap_or(AbstractValue {
            ty: type_from_slot(slot.size),
            origin: ExprOrigin {
                block_id,
                ordinal: slot.addr as usize,
                kind: ExprOriginKind::EntryLocal,
            },
            const_value: None,
            key: None,
        });
        state.stack.push(value);
        return;
    }
    if let Some(slot) = decode_local_set(record) {
        if let Some(value) = state.stack.pop() {
            state.locals.insert(slot, value);
        }
        return;
    }
    if let Some(slot) = decode_local_tee(record) {
        if let Some(value) = state.stack.pop() {
            state.locals.insert(slot, value.clone());
            state.stack.push(value);
        }
        return;
    }
    if record.op_eq(vm::op_drop) {
        let _ = state.stack.pop();
        return;
    }
    if record.op_eq(vm::op_select) {
        transfer_select(state, block_id, ordinal);
        return;
    }
    if let Some(op) = decode_pure_unary(record) {
        transfer_unary(state, block_id, ordinal, op);
        return;
    }
    if let Some(op) = decode_pure_binary(record) {
        transfer_binary(state, block_id, ordinal, op);
        return;
    }
    if let Some(slot) = decode_global_get(record) {
        let key = global_alias_key(slot);
        let value = state.aliases.get(&key).cloned().unwrap_or(AbstractValue {
            ty: type_from_slot(slot.size),
            origin: ExprOrigin {
                block_id,
                ordinal,
                kind: ExprOriginKind::GlobalValue,
            },
            const_value: None,
            key: Some(ValueKey::GlobalGet { slot }),
        });
        state.aliases.insert(key, value.clone());
        state.stack.push(value);
        return;
    }
    if let Some(slot) = decode_global_set(record) {
        if let Some(value) = state.stack.pop() {
            state.heap.global = state.heap.global.saturating_add(1);
            clear_alias_space(state, AliasSpace::Global);
            state.aliases.insert(global_alias_key(slot), value);
        }
        return;
    }
    if let Some(tableidx) = decode_table_get(record) {
        if let Some(index) = state.stack.pop() {
            if let Some(address) = canonical_alias_address(&index) {
                let key = AliasKey {
                    space: AliasSpace::Table,
                    index: tableidx,
                    width: 4,
                    address,
                };
                let value = state.aliases.get(&key).cloned().unwrap_or(AbstractValue {
                    ty: ValType::FuncRef,
                    origin: ExprOrigin {
                        block_id,
                        ordinal,
                        kind: ExprOriginKind::TableValue,
                    },
                    const_value: None,
                    key: Some(ValueKey::TableGet {
                        tableidx,
                        index: index.origin,
                    }),
                });
                state.aliases.insert(key, value.clone());
                state.stack.push(value);
            } else {
                state.stack.push(unknown_value(
                    block_id,
                    ordinal,
                    ValType::FuncRef,
                    ExprOriginKind::TableValue,
                ));
            }
        }
        return;
    }
    if let Some(tableidx) = decode_table_set(record) {
        let _value = state.stack.pop();
        let index = state.stack.pop();
        state.heap.table = state.heap.table.saturating_add(1);
        clear_alias_space(state, AliasSpace::Table);
        let address = index.as_ref().and_then(canonical_alias_address);
        if let (Some(_index), Some(address)) = (index, address) {
            if let Some(value) = _value {
                state.aliases.insert(
                    AliasKey {
                        space: AliasSpace::Table,
                        index: tableidx,
                        width: 4,
                        address,
                    },
                    value,
                );
            }
        }
        return;
    }
    if let Some(access) = decode_memory_load(record) {
        if let Some(address_value) = state.stack.pop() {
            if let Some(key) = memory_alias_key_seed(access, &address_value) {
                let value = state.aliases.get(&key).cloned().unwrap_or(AbstractValue {
                    ty: access.ty,
                    origin: ExprOrigin {
                        block_id,
                        ordinal,
                        kind: ExprOriginKind::MemoryValue,
                    },
                    const_value: None,
                    key: Some(ValueKey::MemoryLoad(key)),
                });
                state.aliases.insert(key, value.clone());
                state.stack.push(value);
            } else {
                state.stack.push(unknown_value(
                    block_id,
                    ordinal,
                    access.ty,
                    ExprOriginKind::MemoryValue,
                ));
            }
        }
        return;
    }
    if let Some(access) = decode_memory_store(record) {
        let value = state.stack.pop();
        let address = state.stack.pop();
        state.heap.memory = state.heap.memory.saturating_add(1);
        clear_alias_space(state, AliasSpace::Memory);
        if let (Some(value), Some(address)) = (
            value,
            address.and_then(|value| memory_alias_key_seed(access, &value)),
        ) {
            state.aliases.insert(address, value);
        }
        return;
    }

    match effect_barrier(record) {
        EffectBarrier::Control | EffectBarrier::TrapSensitive => {
            reset_stack_from_snapshot(state, block_id, ordinal, &record.stack_after);
        }
        EffectBarrier::Memory => {
            state.heap.memory = state.heap.memory.saturating_add(1);
            clear_alias_space(state, AliasSpace::Memory);
            reset_stack_from_snapshot(state, block_id, ordinal, &record.stack_after);
        }
        EffectBarrier::Global => {
            state.heap.global = state.heap.global.saturating_add(1);
            clear_alias_space(state, AliasSpace::Global);
            reset_stack_from_snapshot(state, block_id, ordinal, &record.stack_after);
        }
        EffectBarrier::Table => {
            state.heap.table = state.heap.table.saturating_add(1);
            clear_alias_space(state, AliasSpace::Table);
            reset_stack_from_snapshot(state, block_id, ordinal, &record.stack_after);
        }
        EffectBarrier::Call => {
            state.heap.memory = state.heap.memory.saturating_add(1);
            state.heap.global = state.heap.global.saturating_add(1);
            state.heap.table = state.heap.table.saturating_add(1);
            state.aliases.clear();
            reset_stack_from_snapshot(state, block_id, ordinal, &record.stack_after);
        }
    }
}

fn transfer_select(state: &mut BlockEntryState, block_id: usize, ordinal: usize) {
    let Some(cond) = state.stack.pop() else {
        return;
    };
    let Some(rhs) = state.stack.pop() else {
        state.stack.push(cond);
        return;
    };
    let Some(lhs) = state.stack.pop() else {
        state.stack.push(rhs);
        state.stack.push(cond);
        return;
    };
    let value = match cond.const_value {
        Some(ConstValue::I32(0)) => rhs,
        Some(ConstValue::I32(_)) => lhs,
        _ if same_value(&lhs, &rhs) => lhs,
        _ => AbstractValue {
            ty: lhs.ty,
            origin: ExprOrigin {
                block_id,
                ordinal: instr_result_origin_ordinal(ordinal, 0),
                kind: ExprOriginKind::InstrResult,
            },
            const_value: if lhs.const_value == rhs.const_value {
                lhs.const_value
            } else {
                None
            },
            key: None,
        },
    };
    state.stack.push(value);
}

fn transfer_unary(state: &mut BlockEntryState, block_id: usize, ordinal: usize, op: PureOpKind) {
    let Some(value) = state.stack.pop() else {
        return;
    };
    let const_value = value.const_value.and_then(|value| fold_unary(op, value));
    state.stack.push(AbstractValue {
        ty: unary_output_type(op),
        origin: ExprOrigin {
            block_id,
            ordinal: instr_result_origin_ordinal(ordinal, 0),
            kind: if const_value.is_some() {
                ExprOriginKind::SyntheticConst
            } else {
                ExprOriginKind::InstrResult
            },
        },
        const_value,
        key: const_value.is_none().then_some(ValueKey::Unary {
            op,
            input: value.origin,
        }),
    });
}

fn transfer_binary(state: &mut BlockEntryState, block_id: usize, ordinal: usize, op: PureOpKind) {
    let Some(rhs) = state.stack.pop() else {
        return;
    };
    let Some(lhs) = state.stack.pop() else {
        state.stack.push(rhs);
        return;
    };
    if let Some((keep, _remove)) = simplify_identity_seed(op, &lhs, &rhs) {
        state.stack.push(keep);
        return;
    }
    let const_value = match (lhs.const_value, rhs.const_value) {
        (Some(lhs), Some(rhs)) => fold_binary(op, lhs, rhs),
        _ => None,
    };
    let (lhs_origin, rhs_origin) = canonicalize_binary_origins(op, lhs.origin, rhs.origin);
    state.stack.push(AbstractValue {
        ty: binary_output_type(op),
        origin: ExprOrigin {
            block_id,
            ordinal: instr_result_origin_ordinal(ordinal, 0),
            kind: if const_value.is_some() {
                ExprOriginKind::SyntheticConst
            } else {
                ExprOriginKind::InstrResult
            },
        },
        const_value,
        key: const_value.is_none().then_some(ValueKey::Binary {
            op,
            lhs: lhs_origin,
            rhs: rhs_origin,
        }),
    });
}

fn reset_stack_from_snapshot(
    state: &mut BlockEntryState,
    block_id: usize,
    ordinal: usize,
    snapshot: &crate::parser::core::type_checker::StackSnapshot,
) {
    state.stack = snapshot
        .types
        .iter()
        .enumerate()
        .map(|(result_idx, ty)| {
            unknown_value(
                block_id,
                instr_result_origin_ordinal(ordinal, result_idx),
                *ty,
                ExprOriginKind::InstrResult,
            )
        })
        .collect();
}

fn clear_alias_space(state: &mut BlockEntryState, space: AliasSpace) {
    state.aliases.retain(|key, _| key.space != space);
}

fn instr_result_origin_ordinal(ordinal: usize, result_index: usize) -> usize {
    ordinal
        .saturating_mul(INSTR_RESULT_ORIGIN_STRIDE)
        .saturating_add(result_index)
}

fn unknown_value(
    block_id: usize,
    ordinal: usize,
    ty: ValType,
    kind: ExprOriginKind,
) -> AbstractValue {
    AbstractValue {
        ty,
        origin: ExprOrigin {
            block_id,
            ordinal,
            kind,
        },
        const_value: None,
        key: None,
    }
}

fn same_state(lhs: &BlockEntryState, rhs: &BlockEntryState) -> bool {
    lhs.reachable == rhs.reachable
        && lhs.heap == rhs.heap
        && same_value_vec(&lhs.stack, &rhs.stack)
        && same_value_map(&lhs.locals, &rhs.locals)
        && same_value_map(&lhs.aliases, &rhs.aliases)
}

fn same_value_vec(lhs: &[AbstractValue], rhs: &[AbstractValue]) -> bool {
    lhs.len() == rhs.len()
        && lhs
            .iter()
            .zip(rhs.iter())
            .all(|(lhs, rhs)| same_value(lhs, rhs))
}

fn same_value_map<K: Eq + std::hash::Hash + Copy>(
    lhs: &HashMap<K, AbstractValue>,
    rhs: &HashMap<K, AbstractValue>,
) -> bool {
    lhs.len() == rhs.len()
        && lhs.iter().all(|(key, lhs_value)| {
            rhs.get(key)
                .is_some_and(|rhs_value| same_value(lhs_value, rhs_value))
        })
}

fn same_value(lhs: &AbstractValue, rhs: &AbstractValue) -> bool {
    lhs.ty == rhs.ty
        && lhs.origin == rhs.origin
        && lhs.const_value == rhs.const_value
        && lhs.key == rhs.key
}

#[derive(Default)]
struct BlockOptimizer {
    block_id: usize,
    effect_epoch: EffectEpoch,
    sink: RewriteSink,
    exprs: Vec<ExprState>,
    stack: Vec<ExprId>,
    locals: HashMap<LocalSlot, ExprId>,
    origin_locals: HashMap<ExprOrigin, LocalSlot>,
    cse: HashMap<ValueKey, CseEntry>,
    aliases: HashMap<AliasKey, ExprId>,
    last_local_write: Option<LocalWrite>,
    last_store: HashMap<AliasKey, StoreWrite>,
    heap: HeapVersion,
}

#[derive(Clone, Copy)]
struct LocalWrite {
    slot: LocalSlot,
    record_idx: usize,
    value: ExprId,
}

#[derive(Clone, Copy)]
struct StoreWrite {
    record_idx: usize,
}

#[derive(Clone, Copy)]
struct CseEntry {
    expr: ExprId,
    epoch: EffectEpoch,
}

impl LocalPass for BlockOptimizer {
    fn run_block(
        &mut self,
        program: &BasicBlockProgram,
        block: BasicBlock,
        entry: &BlockEntryState,
    ) -> Vec<RecordEmit> {
        self.reset(block, entry);
        for record_idx in block.start..block.end {
            let record = &program.records[record_idx];
            let ordinal = record_idx - block.start;
            self.visit_record(record, ordinal);
        }
        self.sink.clone().into_live_records()
    }
}

impl BlockOptimizer {
    fn reset(&mut self, block: BasicBlock, entry: &BlockEntryState) {
        self.block_id = block.id;
        self.effect_epoch = 0;
        self.sink = RewriteSink::default();
        self.exprs.clear();
        self.stack.clear();
        self.locals.clear();
        self.origin_locals.clear();
        self.cse.clear();
        self.aliases.clear();
        self.last_local_write = None;
        self.last_store.clear();
        self.heap = entry.heap;

        let mut locals = entry.locals.iter().collect::<Vec<_>>();
        locals.sort_by_key(|(slot, _)| (slot.addr, slot.size));
        for (slot, value) in locals {
            let expr = self.seed_value(value, false);
            self.bind_local(*slot, expr);
            self.seed_cse(expr);
        }

        for value in &entry.stack {
            let expr = self.seed_value(value, false);
            self.push_stack(expr);
            self.seed_cse(expr);
        }

        let mut aliases = entry.aliases.iter().collect::<Vec<_>>();
        aliases.sort_by_key(|(key, _)| (key.space as u8, key.index, key.width));
        for (key, value) in aliases {
            let expr = self.seed_value(value, false);
            self.aliases.insert(*key, expr);
        }
    }

    fn seed_value(&mut self, value: &AbstractValue, removable: bool) -> ExprId {
        let id = ExprId(self.exprs.len());
        self.exprs.push(ExprState {
            ty: value.ty,
            origin: value.origin,
            const_value: value.const_value,
            key: value.key,
            producer_record: None,
            ref_count: 0,
            removable,
        });
        id
    }

    fn seed_cse(&mut self, expr: ExprId) {
        let Some(key) = self.exprs[expr.0].key else {
            return;
        };
        if !self.can_materialize(expr) {
            return;
        }
        self.cse.insert(
            key,
            CseEntry {
                expr,
                epoch: self.effect_epoch,
            },
        );
    }

    fn visit_record(&mut self, record: &DecodedInstr, ordinal: usize) {
        if let Some((ty, value)) = decode_const(record) {
            self.last_local_write = None;
            self.emit_const(record.old_start, ty, value, ordinal);
            return;
        }
        if let Some(slot) = decode_local_get(record) {
            self.visit_local_get(record, slot, ordinal);
            return;
        }
        if let Some(slot) = decode_local_set(record) {
            self.visit_local_set(record, slot, false, ordinal);
            return;
        }
        if let Some(slot) = decode_local_tee(record) {
            self.visit_local_set(record, slot, true, ordinal);
            return;
        }
        if record.op_eq(vm::op_drop) {
            self.visit_drop(record);
            return;
        }
        if record.op_eq(vm::op_select) {
            self.visit_select(record, ordinal);
            return;
        }
        if let Some(op) = decode_pure_unary(record) {
            self.visit_unary(record, op, ordinal);
            return;
        }
        if let Some(op) = decode_pure_binary(record) {
            self.visit_binary(record, op, ordinal);
            return;
        }
        if record.op_eq(vm::op_if) {
            self.visit_if(record, ordinal);
            return;
        }
        if record.op_eq(vm::op_br_if) {
            self.visit_br_if(record, ordinal);
            return;
        }
        if let Some(slot) = decode_global_get(record) {
            self.visit_global_get(record, slot, ordinal);
            return;
        }
        if let Some(slot) = decode_global_set(record) {
            self.visit_global_set(record, slot);
            return;
        }
        if let Some(tableidx) = decode_table_get(record) {
            self.visit_table_get(record, tableidx, ordinal);
            return;
        }
        if let Some(tableidx) = decode_table_set(record) {
            self.visit_table_set(record, tableidx);
            return;
        }
        if let Some(access) = decode_memory_load(record) {
            self.visit_memory_load(record, access, ordinal);
            return;
        }
        if let Some(access) = decode_memory_store(record) {
            self.visit_memory_store(record, access, ordinal);
            return;
        }
        self.emit_barrier(record, ordinal);
    }

    fn visit_local_get(&mut self, record: &DecodedInstr, slot: LocalSlot, _ordinal: usize) {
        if let Some(write) = self.last_local_write {
            if write.slot == slot && self.sink.last_alive_index() == Some(write.record_idx) {
                if let Some(last) = self.sink.record_mut(write.record_idx) {
                    if let Some(tee_op) = set_to_tee(last.op, slot.size) {
                        last.op = tee_op;
                        let source = write.value;
                        self.push_stack(source);
                        self.last_local_write = Some(LocalWrite {
                            slot,
                            record_idx: write.record_idx,
                            value: source,
                        });
                        return;
                    }
                }
            }
        }

        if let Some(source) = self.locals.get(&slot).copied() {
            if let Some(materialized) = self.try_materialize_value(record.old_start, source) {
                self.last_local_write = None;
                self.push_stack(materialized);
                return;
            }
        }

        let record_idx = self.push_original(record);
        self.last_local_write = None;
        let expr = if let Some(source) = self.locals.get(&slot).copied() {
            let source_state = self.exprs[source.0].clone();
            self.new_expr_with_origin(
                source_state.ty,
                source_state.origin,
                source_state.const_value,
                source_state.key,
                Some(record_idx),
                true,
            )
        } else {
            self.new_expr_with_origin(
                type_from_slot(slot.size),
                ExprOrigin {
                    block_id: self.block_id,
                    ordinal: slot.addr as usize,
                    kind: ExprOriginKind::EntryLocal,
                },
                None,
                None,
                Some(record_idx),
                true,
            )
        };
        self.push_stack(expr);
    }

    fn visit_local_set(
        &mut self,
        record: &DecodedInstr,
        slot: LocalSlot,
        is_tee: bool,
        _ordinal: usize,
    ) {
        let Some(value) = self.pop_stack() else {
            self.emit_barrier(record, 0);
            return;
        };
        if self
            .locals
            .get(&slot)
            .is_some_and(|current| same_expr(&self.exprs[current.0], &self.exprs[value.0]))
        {
            self.last_local_write = None;
            if is_tee {
                self.push_stack(value);
            } else {
                let _ = self.try_remove_expr(value);
            }
            return;
        }
        let record_idx = self.push_original(record);
        self.bind_local(slot, value);
        self.last_local_write = Some(LocalWrite {
            slot,
            record_idx,
            value,
        });
        if is_tee {
            self.push_stack(value);
        }
    }

    fn visit_drop(&mut self, record: &DecodedInstr) {
        let Some(value) = self.pop_stack() else {
            self.emit_barrier(record, 0);
            return;
        };
        self.last_local_write = None;
        if self.try_remove_expr(value) {
            return;
        }
        self.push_original(record);
    }

    fn visit_select(&mut self, record: &DecodedInstr, ordinal: usize) {
        let Some(cond) = self.pop_stack() else {
            self.emit_barrier(record, ordinal);
            return;
        };
        let Some(rhs) = self.pop_stack() else {
            self.incref(cond);
            self.push_stack(cond);
            self.emit_barrier(record, ordinal);
            return;
        };
        let Some(lhs) = self.pop_stack() else {
            self.incref(rhs);
            self.push_stack(rhs);
            self.incref(cond);
            self.push_stack(cond);
            self.emit_barrier(record, ordinal);
            return;
        };

        self.last_local_write = None;
        let select_size = record.operand_select();
        let chosen = match self.exprs[cond.0].const_value {
            Some(ConstValue::I32(0)) => Some(rhs),
            Some(ConstValue::I32(_)) => Some(lhs),
            _ if same_expr(&self.exprs[lhs.0], &self.exprs[rhs.0]) => Some(lhs),
            _ => None,
        };
        if let Some(chosen) = chosen {
            let cond_removed = self.try_remove_expr(cond);
            let dropped = if chosen == lhs {
                self.try_remove_expr(rhs)
            } else {
                self.try_remove_expr(lhs)
            };
            if cond_removed && dropped {
                self.push_stack(chosen);
                self.incref(chosen);
                return;
            }
        }

        let record_idx = self.push_original(record);
        let key = if self.exprs[lhs.0].key == self.exprs[rhs.0].key {
            self.exprs[lhs.0].key
        } else {
            None
        };
        let const_value = if self.exprs[lhs.0].const_value == self.exprs[rhs.0].const_value {
            self.exprs[lhs.0].const_value
        } else {
            None
        };
        let expr = self.new_expr_with_origin(
            type_from_slot(select_size),
            ExprOrigin {
                block_id: self.block_id,
                ordinal: instr_result_origin_ordinal(ordinal, 0),
                kind: ExprOriginKind::InstrResult,
            },
            const_value,
            key,
            Some(record_idx),
            false,
        );
        self.push_stack(expr);
    }

    fn visit_unary(&mut self, record: &DecodedInstr, op: PureOpKind, ordinal: usize) {
        let Some(value) = self.pop_stack() else {
            self.emit_barrier(record, ordinal);
            return;
        };
        self.last_local_write = None;
        if let Some(const_value) = self.exprs[value.0]
            .const_value
            .and_then(|value| fold_unary(op, value))
        {
            if self.try_remove_expr(value) {
                self.emit_const(
                    record.old_start,
                    const_value_type(const_value),
                    const_value,
                    ordinal,
                );
                return;
            }
        }

        let key = ValueKey::Unary {
            op,
            input: self.exprs[value.0].origin,
        };
        if self.can_remove_expr(value) {
            if let Some(source) = self.lookup_cse_source(key) {
                self.try_remove_expr(value);
                if let Some(materialized) = self.try_materialize_value(record.old_start, source) {
                    self.push_stack(materialized);
                    return;
                }
            }
        }
        let record_idx = self.push_original(record);
        let expr = self.new_expr_with_origin(
            unary_output_type(op),
            ExprOrigin {
                block_id: self.block_id,
                ordinal: instr_result_origin_ordinal(ordinal, 0),
                kind: ExprOriginKind::InstrResult,
            },
            None,
            Some(key),
            Some(record_idx),
            true,
        );
        self.cse.insert(
            key,
            CseEntry {
                expr,
                epoch: self.effect_epoch,
            },
        );
        self.push_stack(expr);
    }

    fn visit_binary(&mut self, record: &DecodedInstr, op: PureOpKind, ordinal: usize) {
        let Some(rhs) = self.pop_stack() else {
            self.emit_barrier(record, ordinal);
            return;
        };
        let Some(lhs) = self.pop_stack() else {
            self.incref(rhs);
            self.push_stack(rhs);
            self.emit_barrier(record, ordinal);
            return;
        };
        self.last_local_write = None;

        if let Some((keep, remove)) = simplify_identity(op, lhs, rhs, &self.exprs) {
            if self.try_remove_expr(remove) {
                self.push_stack(keep);
                self.incref(keep);
                return;
            }
        }

        if let (Some(lhs_const), Some(rhs_const)) =
            (self.exprs[lhs.0].const_value, self.exprs[rhs.0].const_value)
        {
            if let Some(value) = fold_binary(op, lhs_const, rhs_const) {
                if self.try_remove_expr(lhs) && self.try_remove_expr(rhs) {
                    self.emit_const(record.old_start, const_value_type(value), value, ordinal);
                    return;
                }
            }
        }
        let (lhs_origin, rhs_origin) =
            canonicalize_binary_origins(op, self.exprs[lhs.0].origin, self.exprs[rhs.0].origin);
        let key = ValueKey::Binary {
            op,
            lhs: lhs_origin,
            rhs: rhs_origin,
        };
        if self.can_remove_expr(lhs) && self.can_remove_expr(rhs) {
            if let Some(source) = self.lookup_cse_source(key) {
                self.try_remove_expr(lhs);
                self.try_remove_expr(rhs);
                if let Some(materialized) = self.try_materialize_value(record.old_start, source) {
                    self.push_stack(materialized);
                    return;
                }
            }
        }

        let record_idx = self.push_original(record);
        let expr = self.new_expr_with_origin(
            binary_output_type(op),
            ExprOrigin {
                block_id: self.block_id,
                ordinal: instr_result_origin_ordinal(ordinal, 0),
                kind: ExprOriginKind::InstrResult,
            },
            None,
            Some(key),
            Some(record_idx),
            true,
        );
        self.cse.insert(
            key,
            CseEntry {
                expr,
                epoch: self.effect_epoch,
            },
        );
        self.push_stack(expr);
    }

    fn visit_if(&mut self, record: &DecodedInstr, ordinal: usize) {
        let Some(cond) = self.pop_stack() else {
            self.emit_barrier(record, 0);
            return;
        };
        self.last_local_write = None;
        if let Some(ConstValue::I32(value)) = self.exprs[cond.0].const_value {
            if self.try_remove_expr(cond) {
                if value == 0 {
                    self.sink.push(
                        Some(record.old_start),
                        vm::op_br,
                        vec![Operand {
                            jump_addr: record.operand_jump_addr(0) as u32,
                        }],
                    );
                }
                return;
            }
        }
        self.push_original(record);
        self.reset_stack_from_snapshot(ordinal, &record.stack_after);
    }

    fn visit_br_if(&mut self, record: &DecodedInstr, ordinal: usize) {
        let Some(cond) = self.pop_stack() else {
            self.emit_barrier(record, 0);
            return;
        };
        self.last_local_write = None;
        if let Some(ConstValue::I32(value)) = self.exprs[cond.0].const_value {
            if self.try_remove_expr(cond) {
                if value != 0 {
                    self.sink.push(
                        Some(record.old_start),
                        vm::op_br,
                        vec![Operand {
                            jump_addr: record.operand_jump_addr(0) as u32,
                        }],
                    );
                }
                return;
            }
        }
        self.push_original(record);
        self.reset_stack_from_snapshot(ordinal, &record.stack_after);
    }

    fn visit_global_get(&mut self, record: &DecodedInstr, slot: LocalSlot, ordinal: usize) {
        self.last_local_write = None;
        self.bump_effect_epoch();
        clear_store_space_on_load(&mut self.last_store, AliasSpace::Global);
        let key = global_alias_key(slot);
        if let Some(source) = self.aliases.get(&key).copied() {
            if let Some(materialized) = self.try_materialize_value(record.old_start, source) {
                self.push_stack(materialized);
                return;
            }
        }
        let record_idx = self.push_original(record);
        let expr = self.new_expr_with_origin(
            type_from_slot(slot.size),
            ExprOrigin {
                block_id: self.block_id,
                ordinal,
                kind: ExprOriginKind::GlobalValue,
            },
            None,
            Some(ValueKey::GlobalGet { slot }),
            Some(record_idx),
            true,
        );
        self.aliases.insert(key, expr);
        self.push_stack(expr);
    }

    fn visit_global_set(&mut self, record: &DecodedInstr, slot: LocalSlot) {
        let Some(value) = self.pop_stack() else {
            self.emit_barrier(record, 0);
            return;
        };
        self.last_local_write = None;
        self.bump_effect_epoch();
        let key = global_alias_key(slot);
        if self
            .aliases
            .get(&key)
            .is_some_and(|current| same_expr(&self.exprs[current.0], &self.exprs[value.0]))
        {
            let _ = self.try_remove_expr(value);
            return;
        }
        clear_alias_space_rewrite(&mut self.aliases, &mut self.last_store, AliasSpace::Global);
        let record_idx = self.push_original(record);
        if let Some(previous) = self.last_store.insert(key, StoreWrite { record_idx }) {
            self.sink.remove(previous.record_idx);
        }
        self.aliases.insert(key, value);
        self.heap.global = self.heap.global.saturating_add(1);
    }

    fn visit_table_get(&mut self, record: &DecodedInstr, tableidx: u32, ordinal: usize) {
        let Some(index) = self.pop_stack() else {
            self.emit_barrier(record, ordinal);
            return;
        };
        self.last_local_write = None;
        self.bump_effect_epoch();
        clear_store_space_on_load(&mut self.last_store, AliasSpace::Table);
        let Some(address) = canonical_alias_address_from_expr(&self.exprs[index.0]) else {
            let record_idx = self.push_original(record);
            let expr = self.new_expr_with_origin(
                ValType::FuncRef,
                ExprOrigin {
                    block_id: self.block_id,
                    ordinal,
                    kind: ExprOriginKind::TableValue,
                },
                None,
                None,
                Some(record_idx),
                true,
            );
            self.push_stack(expr);
            return;
        };
        let key = AliasKey {
            space: AliasSpace::Table,
            index: tableidx,
            width: 4,
            address,
        };
        if let Some(source) = self.aliases.get(&key).copied() {
            if let Some(materialized) = self.try_materialize_value(record.old_start, source) {
                self.push_stack(materialized);
                return;
            }
        }
        let record_idx = self.push_original(record);
        let expr = self.new_expr_with_origin(
            ValType::FuncRef,
            ExprOrigin {
                block_id: self.block_id,
                ordinal,
                kind: ExprOriginKind::TableValue,
            },
            None,
            Some(ValueKey::TableGet {
                tableidx,
                index: self.exprs[index.0].origin,
            }),
            Some(record_idx),
            true,
        );
        self.aliases.insert(key, expr);
        self.push_stack(expr);
    }

    fn visit_table_set(&mut self, record: &DecodedInstr, tableidx: u32) {
        let Some(value) = self.pop_stack() else {
            self.emit_barrier(record, 0);
            return;
        };
        let Some(index) = self.pop_stack() else {
            self.incref(value);
            self.push_stack(value);
            self.emit_barrier(record, 0);
            return;
        };
        self.last_local_write = None;
        self.bump_effect_epoch();
        clear_alias_space_rewrite(&mut self.aliases, &mut self.last_store, AliasSpace::Table);
        let record_idx = self.push_original(record);
        if let Some(address) = canonical_alias_address_from_expr(&self.exprs[index.0]) {
            let key = AliasKey {
                space: AliasSpace::Table,
                index: tableidx,
                width: 4,
                address,
            };
            if let Some(previous) = self.last_store.insert(key, StoreWrite { record_idx }) {
                self.sink.remove(previous.record_idx);
            }
            self.aliases.insert(key, value);
        }
        self.heap.table = self.heap.table.saturating_add(1);
    }

    fn visit_memory_load(&mut self, record: &DecodedInstr, access: MemoryAccess, ordinal: usize) {
        let Some(address) = self.pop_stack() else {
            self.emit_barrier(record, ordinal);
            return;
        };
        self.last_local_write = None;
        self.bump_effect_epoch();
        let Some(key) = memory_alias_key(access, &self.exprs[address.0]) else {
            let record_idx = self.push_original(record);
            let expr = self.new_expr_with_origin(
                access.ty,
                ExprOrigin {
                    block_id: self.block_id,
                    ordinal,
                    kind: ExprOriginKind::MemoryValue,
                },
                None,
                None,
                Some(record_idx),
                true,
            );
            self.push_stack(expr);
            clear_store_space_on_load(&mut self.last_store, AliasSpace::Memory);
            return;
        };
        clear_store_space_on_load(&mut self.last_store, AliasSpace::Memory);
        if let Some(source) = self.aliases.get(&key).copied() {
            if let Some(materialized) = self.try_materialize_value(record.old_start, source) {
                self.push_stack(materialized);
                return;
            }
        }
        let record_idx = self.push_original(record);
        let expr = self.new_expr_with_origin(
            access.ty,
            ExprOrigin {
                block_id: self.block_id,
                ordinal,
                kind: ExprOriginKind::MemoryValue,
            },
            None,
            Some(ValueKey::MemoryLoad(key)),
            Some(record_idx),
            true,
        );
        self.aliases.insert(key, expr);
        self.push_stack(expr);
    }

    fn visit_memory_store(&mut self, record: &DecodedInstr, access: MemoryAccess, _ordinal: usize) {
        let Some(value) = self.pop_stack() else {
            self.emit_barrier(record, 0);
            return;
        };
        let Some(address) = self.pop_stack() else {
            self.incref(value);
            self.push_stack(value);
            self.emit_barrier(record, 0);
            return;
        };
        self.last_local_write = None;
        self.bump_effect_epoch();
        clear_alias_space_rewrite(&mut self.aliases, &mut self.last_store, AliasSpace::Memory);
        let record_idx = self.push_original(record);
        if let Some(key) = memory_alias_key(access, &self.exprs[address.0]) {
            if self
                .aliases
                .get(&key)
                .is_some_and(|current| same_expr(&self.exprs[current.0], &self.exprs[value.0]))
            {
                self.sink.remove(record_idx);
                let _ = self.try_remove_expr(value);
                return;
            }
            if let Some(previous) = self.last_store.insert(key, StoreWrite { record_idx }) {
                self.sink.remove(previous.record_idx);
            }
            self.aliases.insert(key, value);
        }
        self.heap.memory = self.heap.memory.saturating_add(1);
    }

    fn emit_barrier(&mut self, record: &DecodedInstr, ordinal: usize) {
        self.last_local_write = None;
        let barrier = effect_barrier(record);
        self.push_original(record);
        self.effect_epoch += 1;
        self.cse.clear();
        match barrier {
            EffectBarrier::Memory => {
                clear_alias_space_rewrite(
                    &mut self.aliases,
                    &mut self.last_store,
                    AliasSpace::Memory,
                );
                self.heap.memory = self.heap.memory.saturating_add(1);
            }
            EffectBarrier::Global => {
                clear_alias_space_rewrite(
                    &mut self.aliases,
                    &mut self.last_store,
                    AliasSpace::Global,
                );
                self.heap.global = self.heap.global.saturating_add(1);
            }
            EffectBarrier::Table => {
                clear_alias_space_rewrite(
                    &mut self.aliases,
                    &mut self.last_store,
                    AliasSpace::Table,
                );
                self.heap.table = self.heap.table.saturating_add(1);
            }
            EffectBarrier::Call => {
                self.aliases.clear();
                self.last_store.clear();
                self.heap.memory = self.heap.memory.saturating_add(1);
                self.heap.global = self.heap.global.saturating_add(1);
                self.heap.table = self.heap.table.saturating_add(1);
            }
            EffectBarrier::Control | EffectBarrier::TrapSensitive => {}
        }
        self.reset_stack_from_snapshot(ordinal, &record.stack_after);
    }

    fn emit_const(&mut self, source_start: usize, ty: ValType, value: ConstValue, ordinal: usize) {
        let (op, operand) = match value {
            ConstValue::I32(value) => (vm::op_i32_const as Op, Operand { i32: value }),
            ConstValue::I64(value) => (vm::op_i64_const as Op, Operand { i64: value }),
            ConstValue::F32(value) => (vm::op_f32_const as Op, Operand { f32: value }),
            ConstValue::F64(value) => (vm::op_f64_const as Op, Operand { f64: value }),
        };
        let record_idx = self.sink.push(Some(source_start), op, vec![operand]);
        let expr = self.new_expr_with_origin(
            ty,
            ExprOrigin {
                block_id: self.block_id,
                ordinal,
                kind: ExprOriginKind::SyntheticConst,
            },
            Some(value),
            None,
            Some(record_idx),
            true,
        );
        self.push_stack(expr);
    }

    fn push_original(&mut self, record: &DecodedInstr) -> usize {
        self.sink
            .push(Some(record.old_start), record.op, record.operands.clone())
    }

    fn bind_local(&mut self, slot: LocalSlot, expr: ExprId) {
        if let Some(previous) = self.locals.insert(slot, expr) {
            self.decref(previous);
            if self.origin_locals.get(&self.exprs[previous.0].origin) == Some(&slot) {
                self.origin_locals.remove(&self.exprs[previous.0].origin);
            }
        }
        self.origin_locals.insert(self.exprs[expr.0].origin, slot);
        self.incref(expr);
    }

    fn push_stack(&mut self, expr: ExprId) {
        self.stack.push(expr);
        self.incref(expr);
    }

    fn pop_stack(&mut self) -> Option<ExprId> {
        let expr = self.stack.pop()?;
        self.decref(expr);
        Some(expr)
    }

    fn incref(&mut self, expr: ExprId) {
        self.exprs[expr.0].ref_count += 1;
    }

    fn decref(&mut self, expr: ExprId) {
        self.exprs[expr.0].ref_count = self.exprs[expr.0].ref_count.saturating_sub(1);
    }

    fn try_remove_expr(&mut self, expr: ExprId) -> bool {
        if !self.can_remove_expr(expr) {
            return false;
        }
        let state = &self.exprs[expr.0];
        let Some(record_idx) = state.producer_record else {
            return false;
        };
        self.sink.remove(record_idx);
        true
    }

    fn can_remove_expr(&self, expr: ExprId) -> bool {
        let state = &self.exprs[expr.0];
        state.ref_count == 0 && state.removable && state.producer_record.is_some()
    }

    fn can_materialize(&self, expr: ExprId) -> bool {
        self.exprs[expr.0].const_value.is_some()
            || self.origin_locals.contains_key(&self.exprs[expr.0].origin)
    }

    fn bump_effect_epoch(&mut self) {
        self.effect_epoch += 1;
        self.cse.clear();
    }

    fn reset_stack_from_snapshot(
        &mut self,
        ordinal: usize,
        snapshot: &crate::parser::core::type_checker::StackSnapshot,
    ) {
        let drained = self.stack.drain(..).collect::<Vec<_>>();
        for expr in drained {
            self.decref(expr);
        }
        for (result_idx, ty) in snapshot.types.iter().enumerate() {
            let expr = self.new_expr_with_origin(
                *ty,
                ExprOrigin {
                    block_id: self.block_id,
                    ordinal: instr_result_origin_ordinal(ordinal, result_idx),
                    kind: ExprOriginKind::InstrResult,
                },
                None,
                None,
                None,
                false,
            );
            self.push_stack(expr);
        }
    }

    fn new_expr_with_origin(
        &mut self,
        ty: ValType,
        origin: ExprOrigin,
        const_value: Option<ConstValue>,
        key: Option<ValueKey>,
        producer_record: Option<usize>,
        removable: bool,
    ) -> ExprId {
        let id = ExprId(self.exprs.len());
        self.exprs.push(ExprState {
            ty,
            origin,
            const_value,
            key,
            producer_record,
            ref_count: 0,
            removable,
        });
        id
    }

    fn lookup_cse_source(&self, key: ValueKey) -> Option<ExprId> {
        let entry = self.cse.get(&key).copied()?;
        (entry.epoch == self.effect_epoch).then_some(entry.expr)
    }

    fn try_materialize_value(&mut self, source_start: usize, source: ExprId) -> Option<ExprId> {
        if let Some(value) = self.exprs[source.0].const_value {
            let ordinal = self.exprs.len();
            self.emit_const(source_start, const_value_type(value), value, ordinal);
            return self.stack.pop().inspect(|expr| {
                self.decref(*expr);
            });
        }
        let slot = *self.origin_locals.get(&self.exprs[source.0].origin)?;
        let op = local_get_op(slot.size);
        let record_idx = self.sink.push(
            Some(source_start),
            op,
            vec![Operand {
                local_addr: slot.addr,
            }],
        );
        let source_state = self.exprs[source.0].clone();
        Some(self.new_expr_with_origin(
            source_state.ty,
            source_state.origin,
            source_state.const_value,
            source_state.key,
            Some(record_idx),
            true,
        ))
    }
}

fn clear_alias_space_rewrite(
    aliases: &mut HashMap<AliasKey, ExprId>,
    stores: &mut HashMap<AliasKey, StoreWrite>,
    space: AliasSpace,
) {
    aliases.retain(|key, _| key.space != space);
    stores.retain(|key, _| key.space != space);
}

fn clear_store_space_on_load(stores: &mut HashMap<AliasKey, StoreWrite>, space: AliasSpace) {
    stores.retain(|key, _| key.space != space);
}

fn same_expr(lhs: &ExprState, rhs: &ExprState) -> bool {
    lhs.ty == rhs.ty
        && lhs.origin == rhs.origin
        && lhs.const_value == rhs.const_value
        && lhs.key == rhs.key
}

#[derive(Clone, Copy)]
struct MemoryAccess {
    memidx: u32,
    width: u8,
    ty: ValType,
}

#[derive(Clone, Copy)]
enum SelectorPattern {
    LocalGet4I32ConstAdd,
    LocalGet4I32ConstAddSet4,
    LocalGet4I32ConstAddTee4,
    LocalGet4LocalGet4I32Add,
    LocalGet4LocalGet4I32AddSet4,
    LocalGet4LocalGet4I32AddTee4,
}

fn select_superinstructions(records: Vec<RecordEmit>) -> Vec<RecordEmit> {
    let mut out = Vec::with_capacity(records.len());
    let mut cursor = 0usize;
    while cursor < records.len() {
        if let Some((fused, consumed)) = match_selector_pattern(&records[cursor..]) {
            out.push(fused);
            cursor += consumed;
            continue;
        }
        out.push(records[cursor].clone());
        cursor += 1;
    }
    out
}

fn match_selector_pattern(records: &[RecordEmit]) -> Option<(RecordEmit, usize)> {
    if records.len() >= 4
        && record_is(&records[0], vm::op_local_get4 as Op)
        && record_is(&records[1], vm::op_i32_const as Op)
        && (record_is(&records[2], vm::op_i32_add as Op)
            || record_is(&records[2], vm::op_i32_sub as Op))
        && record_is(&records[3], vm::op_local_set4 as Op)
    {
        let imm = if record_is(&records[2], vm::op_i32_sub as Op) {
            unsafe { records[1].operands[0].i32 }.wrapping_neg()
        } else {
            unsafe { records[1].operands[0].i32 }
        };
        return Some((
            fused_record(
                SelectorPattern::LocalGet4I32ConstAddSet4,
                records,
                vm::op_local_get4_i32_const_add_set4 as Op,
                vec![
                    records[0].operands[0],
                    Operand { i32: imm },
                    records[3].operands[0],
                ],
            ),
            4,
        ));
    }
    if records.len() >= 3
        && record_is(&records[0], vm::op_local_get4 as Op)
        && record_is(&records[1], vm::op_i32_const as Op)
        && record_is(&records[2], vm::op_i32_add as Op)
    {
        return Some((
            fused_record(
                SelectorPattern::LocalGet4I32ConstAdd,
                records,
                vm::op_local_get4_i32_const_add as Op,
                vec![records[0].operands[0], records[1].operands[0]],
            ),
            3,
        ));
    }
    if records.len() >= 4
        && record_is(&records[0], vm::op_local_get4 as Op)
        && record_is(&records[1], vm::op_i32_const as Op)
        && (record_is(&records[2], vm::op_i32_add as Op)
            || record_is(&records[2], vm::op_i32_sub as Op))
        && record_is(&records[3], vm::op_local_tee4 as Op)
    {
        let imm = if record_is(&records[2], vm::op_i32_sub as Op) {
            unsafe { records[1].operands[0].i32 }.wrapping_neg()
        } else {
            unsafe { records[1].operands[0].i32 }
        };
        return Some((
            fused_record(
                SelectorPattern::LocalGet4I32ConstAddTee4,
                records,
                vm::op_local_get4_i32_const_add_tee4 as Op,
                vec![
                    records[0].operands[0],
                    Operand { i32: imm },
                    records[3].operands[0],
                ],
            ),
            4,
        ));
    }
    if records.len() >= 4
        && record_is(&records[0], vm::op_local_get4 as Op)
        && record_is(&records[1], vm::op_local_get4 as Op)
        && record_is(&records[2], vm::op_i32_add as Op)
        && record_is(&records[3], vm::op_local_set4 as Op)
    {
        return Some((
            fused_record(
                SelectorPattern::LocalGet4LocalGet4I32AddSet4,
                records,
                vm::op_local_get4_local_get4_i32_add_set4 as Op,
                vec![
                    records[0].operands[0],
                    records[1].operands[0],
                    records[3].operands[0],
                ],
            ),
            4,
        ));
    }
    if records.len() >= 3
        && record_is(&records[0], vm::op_local_get4 as Op)
        && record_is(&records[1], vm::op_local_get4 as Op)
        && record_is(&records[2], vm::op_i32_add as Op)
    {
        return Some((
            fused_record(
                SelectorPattern::LocalGet4LocalGet4I32Add,
                records,
                vm::op_local_get4_local_get4_i32_add as Op,
                vec![records[0].operands[0], records[1].operands[0]],
            ),
            3,
        ));
    }
    if records.len() >= 4
        && record_is(&records[0], vm::op_local_get4 as Op)
        && record_is(&records[1], vm::op_local_get4 as Op)
        && record_is(&records[2], vm::op_i32_add as Op)
        && record_is(&records[3], vm::op_local_tee4 as Op)
    {
        return Some((
            fused_record(
                SelectorPattern::LocalGet4LocalGet4I32AddTee4,
                records,
                vm::op_local_get4_local_get4_i32_add_tee4 as Op,
                vec![
                    records[0].operands[0],
                    records[1].operands[0],
                    records[3].operands[0],
                ],
            ),
            4,
        ));
    }
    None
}

fn fused_record(
    _pattern: SelectorPattern,
    records: &[RecordEmit],
    op: Op,
    operands: Vec<Operand>,
) -> RecordEmit {
    RecordEmit {
        source_start: records[0].source_start,
        op,
        operands,
        alive: true,
    }
}

fn record_is(record: &RecordEmit, op: Op) -> bool {
    std::ptr::fn_addr_eq(record.op, op)
}

fn reachable_blocks(program: &BasicBlockProgram, records: &[Vec<RecordEmit>]) -> Vec<bool> {
    let mut reachable = vec![false; program.blocks.len()];
    let mut queue = VecDeque::from([0usize]);
    while let Some(block_id) = queue.pop_front() {
        if reachable[block_id] {
            continue;
        }
        reachable[block_id] = true;
        for succ in rewritten_successors(program, block_id, &records[block_id]) {
            if !reachable[succ] {
                queue.push_back(succ);
            }
        }
    }
    reachable
}

fn rewritten_successors(
    program: &BasicBlockProgram,
    block_id: usize,
    records: &[RecordEmit],
) -> Vec<usize> {
    let fallthrough = program.next_block_id(block_id);
    let Some(last) = records.last() else {
        return fallthrough.into_iter().collect();
    };
    if record_is(last, vm::op_br as Op)
        || record_is(last, vm::op_else as Op)
        || record_is(last, vm::op_return as Op)
    {
        return single_target(program, last).into_iter().collect();
    }
    if record_is(last, vm::op_br_if as Op) || record_is(last, vm::op_if as Op) {
        let mut succs = Vec::new();
        if let Some(target) = single_target(program, last) {
            succs.push(target);
        }
        if let Some(next) = fallthrough {
            succs.push(next);
        }
        succs.sort_unstable();
        succs.dedup();
        return succs;
    }
    if record_is(last, vm::op_br_table as Op) {
        return table_targets(program, last);
    }
    if record_is(last, vm::special_function_return as Op) {
        return Vec::new();
    }
    if record_is(last, vm::special_block_return as Op) {
        return fallthrough.into_iter().collect();
    }
    fallthrough.into_iter().collect()
}

fn single_target(program: &BasicBlockProgram, record: &RecordEmit) -> Option<usize> {
    let target = unsafe { record.operands[0].jump_addr as usize };
    program.block_for_old_start(target)
}

fn table_targets(program: &BasicBlockProgram, record: &RecordEmit) -> Vec<usize> {
    let table_len = unsafe { record.operands[0].u32 as usize };
    (1..=table_len + 1)
        .filter_map(|idx| {
            let target = unsafe { record.operands[idx].jump_addr as usize };
            program.block_for_old_start(target)
        })
        .collect()
}

pub(crate) fn patch_jump_targets(records: &mut [RecordEmit]) -> Result<(), ()> {
    let mut old_to_new = HashMap::new();
    let mut cursor = 0usize;
    for record in records.iter() {
        if let Some(old_start) = record.source_start {
            old_to_new.insert(old_start, cursor);
        }
        cursor += record.len();
    }
    for record in records.iter_mut() {
        if std::ptr::fn_addr_eq(record.op, vm::op_if as Op)
            || std::ptr::fn_addr_eq(record.op, vm::op_else as Op)
            || std::ptr::fn_addr_eq(record.op, vm::op_br as Op)
            || std::ptr::fn_addr_eq(record.op, vm::op_br_if as Op)
            || std::ptr::fn_addr_eq(record.op, vm::op_return as Op)
        {
            let target = unsafe { record.operands[0].jump_addr as usize };
            let patched = *old_to_new.get(&target).ok_or(())?;
            record.operands[0] = Operand {
                jump_addr: patched as u32,
            };
        } else if std::ptr::fn_addr_eq(record.op, vm::op_br_table as Op) {
            let table_len = unsafe { record.operands[0].u32 as usize };
            for idx in 1..=table_len + 1 {
                let target = unsafe { record.operands[idx].jump_addr as usize };
                let patched = *old_to_new.get(&target).ok_or(())?;
                record.operands[idx] = Operand {
                    jump_addr: patched as u32,
                };
            }
        }
    }
    Ok(())
}

fn decode_const(record: &DecodedInstr) -> Option<(ValType, ConstValue)> {
    if record.op_eq(vm::op_i32_const) {
        return Some((ValType::I32, ConstValue::I32(record.operand_i32(0))));
    }
    if record.op_eq(vm::op_i64_const) {
        return Some((ValType::I64, ConstValue::I64(record.operand_i64(0))));
    }
    if record.op_eq(vm::op_f32_const) {
        return Some((ValType::F32, ConstValue::F32(record.operand_f32(0))));
    }
    if record.op_eq(vm::op_f64_const) {
        return Some((ValType::F64, ConstValue::F64(record.operand_f64(0))));
    }
    None
}

fn decode_local_get(record: &DecodedInstr) -> Option<LocalSlot> {
    if record.op_eq(vm::op_local_get4) {
        return Some(LocalSlot::new(record.operand_local_addr(), 4));
    }
    if record.op_eq(vm::op_local_get8) {
        return Some(LocalSlot::new(record.operand_local_addr(), 8));
    }
    if record.op_eq(vm::op_local_get16) {
        return Some(LocalSlot::new(record.operand_local_addr(), 16));
    }
    None
}

fn decode_local_set(record: &DecodedInstr) -> Option<LocalSlot> {
    if record.op_eq(vm::op_local_set4) {
        return Some(LocalSlot::new(record.operand_local_addr(), 4));
    }
    if record.op_eq(vm::op_local_set8) {
        return Some(LocalSlot::new(record.operand_local_addr(), 8));
    }
    if record.op_eq(vm::op_local_set16) {
        return Some(LocalSlot::new(record.operand_local_addr(), 16));
    }
    None
}

fn decode_local_tee(record: &DecodedInstr) -> Option<LocalSlot> {
    if record.op_eq(vm::op_local_tee4) {
        return Some(LocalSlot::new(record.operand_local_addr(), 4));
    }
    if record.op_eq(vm::op_local_tee8) {
        return Some(LocalSlot::new(record.operand_local_addr(), 8));
    }
    if record.op_eq(vm::op_local_tee16) {
        return Some(LocalSlot::new(record.operand_local_addr(), 16));
    }
    None
}

fn decode_global_get(record: &DecodedInstr) -> Option<LocalSlot> {
    if record.op_eq(vm::op_global_get4) {
        return Some(LocalSlot::new(record.operand_u32(0), 4));
    }
    if record.op_eq(vm::op_global_get8) {
        return Some(LocalSlot::new(record.operand_u32(0), 8));
    }
    if record.op_eq(vm::op_global_get16) {
        return Some(LocalSlot::new(record.operand_u32(0), 16));
    }
    None
}

fn decode_global_set(record: &DecodedInstr) -> Option<LocalSlot> {
    if record.op_eq(vm::op_global_set4) {
        return Some(LocalSlot::new(record.operand_u32(0), 4));
    }
    if record.op_eq(vm::op_global_set8) {
        return Some(LocalSlot::new(record.operand_u32(0), 8));
    }
    if record.op_eq(vm::op_global_set16) {
        return Some(LocalSlot::new(record.operand_u32(0), 16));
    }
    None
}

fn decode_table_get(record: &DecodedInstr) -> Option<u32> {
    record
        .op_eq(vm::op_table_get)
        .then(|| record.operand_u32(0))
}

fn decode_table_set(record: &DecodedInstr) -> Option<u32> {
    record
        .op_eq(vm::op_table_set)
        .then(|| record.operand_u32(0))
}

fn decode_memory_load(record: &DecodedInstr) -> Option<MemoryAccess> {
    if record.op_eq(vm::op_i32_load as Op)
        || record.op_eq(vm::op_i32_load_shared as Op)
        || record.op_eq(vm::op_i32_load_indexed_local as Op)
        || record.op_eq(vm::op_i32_load_indexed_shared as Op)
    {
        return Some(MemoryAccess {
            memidx: memory_index(record),
            width: 4,
            ty: ValType::I32,
        });
    }
    if record.op_eq(vm::op_i64_load as Op)
        || record.op_eq(vm::op_i64_load_shared as Op)
        || record.op_eq(vm::op_i64_load_indexed_local as Op)
        || record.op_eq(vm::op_i64_load_indexed_shared as Op)
    {
        return Some(MemoryAccess {
            memidx: memory_index(record),
            width: 8,
            ty: ValType::I64,
        });
    }
    if record.op_eq(vm::op_f32_load as Op)
        || record.op_eq(vm::op_f32_load_shared as Op)
        || record.op_eq(vm::op_f32_load_indexed_local as Op)
        || record.op_eq(vm::op_f32_load_indexed_shared as Op)
    {
        return Some(MemoryAccess {
            memidx: memory_index(record),
            width: 4,
            ty: ValType::F32,
        });
    }
    if record.op_eq(vm::op_f64_load as Op)
        || record.op_eq(vm::op_f64_load_shared as Op)
        || record.op_eq(vm::op_f64_load_indexed_local as Op)
        || record.op_eq(vm::op_f64_load_indexed_shared as Op)
    {
        return Some(MemoryAccess {
            memidx: memory_index(record),
            width: 8,
            ty: ValType::F64,
        });
    }
    None
}

fn decode_memory_store(record: &DecodedInstr) -> Option<MemoryAccess> {
    if record.op_eq(vm::op_i32_store as Op)
        || record.op_eq(vm::op_i32_store_shared as Op)
        || record.op_eq(vm::op_i32_store_indexed_local as Op)
        || record.op_eq(vm::op_i32_store_indexed_shared as Op)
    {
        return Some(MemoryAccess {
            memidx: memory_index(record),
            width: 4,
            ty: ValType::I32,
        });
    }
    if record.op_eq(vm::op_i64_store as Op)
        || record.op_eq(vm::op_i64_store_shared as Op)
        || record.op_eq(vm::op_i64_store_indexed_local as Op)
        || record.op_eq(vm::op_i64_store_indexed_shared as Op)
    {
        return Some(MemoryAccess {
            memidx: memory_index(record),
            width: 8,
            ty: ValType::I64,
        });
    }
    if record.op_eq(vm::op_f32_store as Op)
        || record.op_eq(vm::op_f32_store_shared as Op)
        || record.op_eq(vm::op_f32_store_indexed_local as Op)
        || record.op_eq(vm::op_f32_store_indexed_shared as Op)
    {
        return Some(MemoryAccess {
            memidx: memory_index(record),
            width: 4,
            ty: ValType::F32,
        });
    }
    if record.op_eq(vm::op_f64_store as Op)
        || record.op_eq(vm::op_f64_store_shared as Op)
        || record.op_eq(vm::op_f64_store_indexed_local as Op)
        || record.op_eq(vm::op_f64_store_indexed_shared as Op)
    {
        return Some(MemoryAccess {
            memidx: memory_index(record),
            width: 8,
            ty: ValType::F64,
        });
    }
    None
}

fn memory_index(record: &DecodedInstr) -> u32 {
    if record.operands.len() > 1 {
        record.operand_u32(1)
    } else {
        0
    }
}

fn set_to_tee(op: Op, size: u32) -> Option<Op> {
    match size {
        4 if std::ptr::fn_addr_eq(op, vm::op_local_set4 as Op) => Some(vm::op_local_tee4 as Op),
        8 if std::ptr::fn_addr_eq(op, vm::op_local_set8 as Op) => Some(vm::op_local_tee8 as Op),
        16 if std::ptr::fn_addr_eq(op, vm::op_local_set16 as Op) => Some(vm::op_local_tee16 as Op),
        _ => None,
    }
}

fn decode_pure_unary(record: &DecodedInstr) -> Option<PureOpKind> {
    if record.op_eq(vm::op_i32_eqz) {
        return Some(PureOpKind::I32Eqz);
    }
    if record.op_eq(vm::op_i64_eqz) {
        return Some(PureOpKind::I64Eqz);
    }
    None
}

fn decode_pure_binary(record: &DecodedInstr) -> Option<PureOpKind> {
    if record.op_eq(vm::op_i32_add) {
        return Some(PureOpKind::I32Add);
    }
    if record.op_eq(vm::op_i32_sub) {
        return Some(PureOpKind::I32Sub);
    }
    if record.op_eq(vm::op_i32_mul) {
        return Some(PureOpKind::I32Mul);
    }
    if record.op_eq(vm::op_i32_and) {
        return Some(PureOpKind::I32And);
    }
    if record.op_eq(vm::op_i32_or) {
        return Some(PureOpKind::I32Or);
    }
    if record.op_eq(vm::op_i32_xor) {
        return Some(PureOpKind::I32Xor);
    }
    if record.op_eq(vm::op_i32_eq) {
        return Some(PureOpKind::I32Eq);
    }
    if record.op_eq(vm::op_i32_ne) {
        return Some(PureOpKind::I32Ne);
    }
    if record.op_eq(vm::op_i32_lt_s) {
        return Some(PureOpKind::I32LtS);
    }
    if record.op_eq(vm::op_i32_lt_u) {
        return Some(PureOpKind::I32LtU);
    }
    if record.op_eq(vm::op_i32_gt_s) {
        return Some(PureOpKind::I32GtS);
    }
    if record.op_eq(vm::op_i32_gt_u) {
        return Some(PureOpKind::I32GtU);
    }
    if record.op_eq(vm::op_i32_le_s) {
        return Some(PureOpKind::I32LeS);
    }
    if record.op_eq(vm::op_i32_le_u) {
        return Some(PureOpKind::I32LeU);
    }
    if record.op_eq(vm::op_i32_ge_s) {
        return Some(PureOpKind::I32GeS);
    }
    if record.op_eq(vm::op_i32_ge_u) {
        return Some(PureOpKind::I32GeU);
    }
    if record.op_eq(vm::op_i64_add) {
        return Some(PureOpKind::I64Add);
    }
    if record.op_eq(vm::op_i64_sub) {
        return Some(PureOpKind::I64Sub);
    }
    if record.op_eq(vm::op_f32_add) {
        return Some(PureOpKind::F32Add);
    }
    if record.op_eq(vm::op_f32_sub) {
        return Some(PureOpKind::F32Sub);
    }
    if record.op_eq(vm::op_f32_mul) {
        return Some(PureOpKind::F32Mul);
    }
    if record.op_eq(vm::op_f32_div) {
        return Some(PureOpKind::F32Div);
    }
    if record.op_eq(vm::op_f32_eq) {
        return Some(PureOpKind::F32Eq);
    }
    if record.op_eq(vm::op_f32_ne) {
        return Some(PureOpKind::F32Ne);
    }
    if record.op_eq(vm::op_f32_lt) {
        return Some(PureOpKind::F32Lt);
    }
    if record.op_eq(vm::op_f32_gt) {
        return Some(PureOpKind::F32Gt);
    }
    if record.op_eq(vm::op_f32_le) {
        return Some(PureOpKind::F32Le);
    }
    if record.op_eq(vm::op_f32_ge) {
        return Some(PureOpKind::F32Ge);
    }
    if record.op_eq(vm::op_f64_add) {
        return Some(PureOpKind::F64Add);
    }
    if record.op_eq(vm::op_f64_sub) {
        return Some(PureOpKind::F64Sub);
    }
    if record.op_eq(vm::op_f64_mul) {
        return Some(PureOpKind::F64Mul);
    }
    if record.op_eq(vm::op_f64_div) {
        return Some(PureOpKind::F64Div);
    }
    if record.op_eq(vm::op_f64_eq) {
        return Some(PureOpKind::F64Eq);
    }
    if record.op_eq(vm::op_f64_ne) {
        return Some(PureOpKind::F64Ne);
    }
    if record.op_eq(vm::op_f64_lt) {
        return Some(PureOpKind::F64Lt);
    }
    if record.op_eq(vm::op_f64_gt) {
        return Some(PureOpKind::F64Gt);
    }
    if record.op_eq(vm::op_f64_le) {
        return Some(PureOpKind::F64Le);
    }
    if record.op_eq(vm::op_f64_ge) {
        return Some(PureOpKind::F64Ge);
    }
    None
}

fn effect_barrier(record: &DecodedInstr) -> EffectBarrier {
    if record.op_eq(vm::op_call)
        || record.op_eq(vm::op_call_import)
        || record.op_eq(vm::op_return_call)
        || record.op_eq(vm::op_return_call_import)
        || record.op_eq(vm::op_call_indirect)
        || record.op_eq(vm::op_return_call_indirect)
    {
        return EffectBarrier::Call;
    }
    if decode_memory_load(record).is_some() {
        return EffectBarrier::TrapSensitive;
    }
    if decode_memory_store(record).is_some()
        || record.op_eq(vm::op_mem_init_local as Op)
        || record.op_eq(vm::op_mem_init_shared as Op)
        || record.op_eq(vm::op_mem_init_indexed_local as Op)
        || record.op_eq(vm::op_mem_init_indexed_shared as Op)
        || record.op_eq(vm::op_mem_copy_local as Op)
        || record.op_eq(vm::op_mem_copy_shared as Op)
        || record.op_eq(vm::op_mem_copy_indexed_local_local as Op)
        || record.op_eq(vm::op_mem_copy_indexed_local_shared as Op)
        || record.op_eq(vm::op_mem_copy_indexed_shared_local as Op)
        || record.op_eq(vm::op_mem_copy_indexed_shared_shared as Op)
        || record.op_eq(vm::op_mem_fill_local as Op)
        || record.op_eq(vm::op_mem_fill_shared as Op)
        || record.op_eq(vm::op_mem_fill_indexed_local as Op)
        || record.op_eq(vm::op_mem_fill_indexed_shared as Op)
        || record.op_eq(vm::op_data_drop as Op)
        || record.op_eq(vm::op_mem_grow_local as Op)
        || record.op_eq(vm::op_mem_grow_shared as Op)
        || record.op_eq(vm::op_mem_grow_indexed_local as Op)
        || record.op_eq(vm::op_mem_grow_indexed_shared as Op)
    {
        return EffectBarrier::Memory;
    }
    #[cfg(feature = "threads")]
    if record.op_eq(vm::op_memory_atomic_notify_unshared as Op)
        || record.op_eq(vm::op_memory_atomic_notify_shared as Op)
        || record.op_eq(vm::op_memory_atomic_notify_indexed_unshared as Op)
        || record.op_eq(vm::op_memory_atomic_notify_indexed_shared as Op)
        || record.op_eq(vm::op_memory_atomic_wait32_unshared as Op)
        || record.op_eq(vm::op_memory_atomic_wait32_shared as Op)
        || record.op_eq(vm::op_memory_atomic_wait32_indexed_unshared as Op)
        || record.op_eq(vm::op_memory_atomic_wait32_indexed_shared as Op)
        || record.op_eq(vm::op_memory_atomic_wait64_unshared as Op)
        || record.op_eq(vm::op_memory_atomic_wait64_shared as Op)
        || record.op_eq(vm::op_memory_atomic_wait64_indexed_unshared as Op)
        || record.op_eq(vm::op_memory_atomic_wait64_indexed_shared as Op)
        || record.op_eq(vm::op_atomic_fence_local as Op)
        || record.op_eq(vm::op_atomic_fence_shared as Op)
    {
        return EffectBarrier::Memory;
    }
    if decode_global_get(record).is_some() {
        return EffectBarrier::TrapSensitive;
    }
    if decode_global_set(record).is_some() {
        return EffectBarrier::Global;
    }
    if decode_table_get(record).is_some() {
        return EffectBarrier::TrapSensitive;
    }
    if decode_table_set(record).is_some()
        || record.op_eq(vm::op_table_init as Op)
        || record.op_eq(vm::op_table_copy as Op)
        || record.op_eq(vm::op_elem_drop as Op)
        || record.op_eq(vm::op_table_fill as Op)
    {
        return EffectBarrier::Table;
    }
    if record.op_eq(vm::op_if)
        || record.op_eq(vm::op_else)
        || record.op_eq(vm::op_br)
        || record.op_eq(vm::op_br_if)
        || record.op_eq(vm::op_br_table)
        || record.op_eq(vm::op_return)
        || record.op_eq(vm::op_loop)
        || record.op_eq(vm::op_end)
        || record.op_eq(vm::special_block_return)
        || record.op_eq(vm::special_function_return)
        || record.op_eq(vm::op_unreachable)
    {
        return EffectBarrier::Control;
    }
    EffectBarrier::TrapSensitive
}

fn type_from_slot(size: u32) -> ValType {
    match size {
        4 => ValType::I32,
        8 => ValType::I64,
        16 => ValType::V128,
        _ => ValType::I32,
    }
}

fn local_get_op(size: u32) -> Op {
    match size {
        4 => vm::op_local_get4 as Op,
        8 => vm::op_local_get8 as Op,
        16 => vm::op_local_get16 as Op,
        _ => vm::op_local_get4 as Op,
    }
}

fn global_alias_key(slot: LocalSlot) -> AliasKey {
    AliasKey {
        space: AliasSpace::Global,
        index: slot.addr,
        width: slot.size as u8,
        address: AliasAddress::Const(0),
    }
}

fn canonical_alias_address(value: &AbstractValue) -> Option<AliasAddress> {
    value
        .const_value
        .and_then(|value| match value {
            ConstValue::I32(value) => Some(AliasAddress::Const(value as u32)),
            _ => None,
        })
        .or(Some(AliasAddress::Origin(value.origin)))
}

fn canonical_alias_address_from_expr(value: &ExprState) -> Option<AliasAddress> {
    value
        .const_value
        .and_then(|value| match value {
            ConstValue::I32(value) => Some(AliasAddress::Const(value as u32)),
            _ => None,
        })
        .or(Some(AliasAddress::Origin(value.origin)))
}

fn memory_alias_key_seed(access: MemoryAccess, address: &AbstractValue) -> Option<AliasKey> {
    Some(AliasKey {
        space: AliasSpace::Memory,
        index: access.memidx,
        width: access.width,
        address: canonical_alias_address(address)?,
    })
}

fn memory_alias_key(access: MemoryAccess, address: &ExprState) -> Option<AliasKey> {
    Some(AliasKey {
        space: AliasSpace::Memory,
        index: access.memidx,
        width: access.width,
        address: canonical_alias_address_from_expr(address)?,
    })
}

fn const_value_type(value: ConstValue) -> ValType {
    match value {
        ConstValue::I32(_) => ValType::I32,
        ConstValue::I64(_) => ValType::I64,
        ConstValue::F32(_) => ValType::F32,
        ConstValue::F64(_) => ValType::F64,
    }
}

fn unary_output_type(op: PureOpKind) -> ValType {
    match op {
        PureOpKind::I32Eqz | PureOpKind::I64Eqz => ValType::I32,
        _ => ValType::I32,
    }
}

fn binary_output_type(op: PureOpKind) -> ValType {
    match op {
        PureOpKind::I32Add
        | PureOpKind::I32Sub
        | PureOpKind::I32Mul
        | PureOpKind::I32And
        | PureOpKind::I32Or
        | PureOpKind::I32Xor => ValType::I32,
        PureOpKind::I32Eq
        | PureOpKind::I32Ne
        | PureOpKind::I32LtS
        | PureOpKind::I32LtU
        | PureOpKind::I32GtS
        | PureOpKind::I32GtU
        | PureOpKind::I32LeS
        | PureOpKind::I32LeU
        | PureOpKind::I32GeS
        | PureOpKind::I32GeU
        | PureOpKind::F32Eq
        | PureOpKind::F32Ne
        | PureOpKind::F32Lt
        | PureOpKind::F32Gt
        | PureOpKind::F32Le
        | PureOpKind::F32Ge
        | PureOpKind::F64Eq
        | PureOpKind::F64Ne
        | PureOpKind::F64Lt
        | PureOpKind::F64Gt
        | PureOpKind::F64Le
        | PureOpKind::F64Ge => ValType::I32,
        PureOpKind::I64Add | PureOpKind::I64Sub => ValType::I64,
        PureOpKind::F32Add | PureOpKind::F32Sub | PureOpKind::F32Mul | PureOpKind::F32Div => {
            ValType::F32
        }
        PureOpKind::F64Add | PureOpKind::F64Sub | PureOpKind::F64Mul | PureOpKind::F64Div => {
            ValType::F64
        }
        _ => ValType::I32,
    }
}

fn fold_unary(op: PureOpKind, value: ConstValue) -> Option<ConstValue> {
    match (op, value) {
        (PureOpKind::I32Eqz, ConstValue::I32(value)) => Some(ConstValue::I32((value == 0) as i32)),
        (PureOpKind::I64Eqz, ConstValue::I64(value)) => Some(ConstValue::I32((value == 0) as i32)),
        _ => None,
    }
}

fn fold_binary(op: PureOpKind, lhs: ConstValue, rhs: ConstValue) -> Option<ConstValue> {
    match (op, lhs, rhs) {
        (PureOpKind::I32Add, ConstValue::I32(lhs), ConstValue::I32(rhs)) => {
            Some(ConstValue::I32(lhs.wrapping_add(rhs)))
        }
        (PureOpKind::I32Sub, ConstValue::I32(lhs), ConstValue::I32(rhs)) => {
            Some(ConstValue::I32(lhs.wrapping_sub(rhs)))
        }
        (PureOpKind::I32Mul, ConstValue::I32(lhs), ConstValue::I32(rhs)) => {
            Some(ConstValue::I32(lhs.wrapping_mul(rhs)))
        }
        (PureOpKind::I32And, ConstValue::I32(lhs), ConstValue::I32(rhs)) => {
            Some(ConstValue::I32(lhs & rhs))
        }
        (PureOpKind::I32Or, ConstValue::I32(lhs), ConstValue::I32(rhs)) => {
            Some(ConstValue::I32(lhs | rhs))
        }
        (PureOpKind::I32Xor, ConstValue::I32(lhs), ConstValue::I32(rhs)) => {
            Some(ConstValue::I32(lhs ^ rhs))
        }
        (PureOpKind::I32Eq, ConstValue::I32(lhs), ConstValue::I32(rhs)) => {
            Some(ConstValue::I32((lhs == rhs) as i32))
        }
        (PureOpKind::I32Ne, ConstValue::I32(lhs), ConstValue::I32(rhs)) => {
            Some(ConstValue::I32((lhs != rhs) as i32))
        }
        (PureOpKind::I32LtS, ConstValue::I32(lhs), ConstValue::I32(rhs)) => {
            Some(ConstValue::I32((lhs < rhs) as i32))
        }
        (PureOpKind::I32LtU, ConstValue::I32(lhs), ConstValue::I32(rhs)) => {
            Some(ConstValue::I32(((lhs as u32) < (rhs as u32)) as i32))
        }
        (PureOpKind::I32GtS, ConstValue::I32(lhs), ConstValue::I32(rhs)) => {
            Some(ConstValue::I32((lhs > rhs) as i32))
        }
        (PureOpKind::I32GtU, ConstValue::I32(lhs), ConstValue::I32(rhs)) => {
            Some(ConstValue::I32(((lhs as u32) > (rhs as u32)) as i32))
        }
        (PureOpKind::I32LeS, ConstValue::I32(lhs), ConstValue::I32(rhs)) => {
            Some(ConstValue::I32((lhs <= rhs) as i32))
        }
        (PureOpKind::I32LeU, ConstValue::I32(lhs), ConstValue::I32(rhs)) => {
            Some(ConstValue::I32(((lhs as u32) <= (rhs as u32)) as i32))
        }
        (PureOpKind::I32GeS, ConstValue::I32(lhs), ConstValue::I32(rhs)) => {
            Some(ConstValue::I32((lhs >= rhs) as i32))
        }
        (PureOpKind::I32GeU, ConstValue::I32(lhs), ConstValue::I32(rhs)) => {
            Some(ConstValue::I32(((lhs as u32) >= (rhs as u32)) as i32))
        }
        (PureOpKind::I64Add, ConstValue::I64(lhs), ConstValue::I64(rhs)) => {
            Some(ConstValue::I64(lhs.wrapping_add(rhs)))
        }
        (PureOpKind::I64Sub, ConstValue::I64(lhs), ConstValue::I64(rhs)) => {
            Some(ConstValue::I64(lhs.wrapping_sub(rhs)))
        }
        (PureOpKind::F32Add, ConstValue::F32(lhs), ConstValue::F32(rhs)) => {
            Some(ConstValue::F32(lhs + rhs))
        }
        (PureOpKind::F32Sub, ConstValue::F32(lhs), ConstValue::F32(rhs)) => {
            Some(ConstValue::F32(lhs - rhs))
        }
        (PureOpKind::F32Mul, ConstValue::F32(lhs), ConstValue::F32(rhs)) => {
            Some(ConstValue::F32(lhs * rhs))
        }
        (PureOpKind::F32Div, ConstValue::F32(lhs), ConstValue::F32(rhs)) => {
            Some(ConstValue::F32(lhs / rhs))
        }
        (PureOpKind::F32Eq, ConstValue::F32(lhs), ConstValue::F32(rhs)) => {
            Some(ConstValue::I32((lhs == rhs) as i32))
        }
        (PureOpKind::F32Ne, ConstValue::F32(lhs), ConstValue::F32(rhs)) => {
            Some(ConstValue::I32((lhs != rhs) as i32))
        }
        (PureOpKind::F32Lt, ConstValue::F32(lhs), ConstValue::F32(rhs)) => {
            Some(ConstValue::I32((lhs < rhs) as i32))
        }
        (PureOpKind::F32Gt, ConstValue::F32(lhs), ConstValue::F32(rhs)) => {
            Some(ConstValue::I32((lhs > rhs) as i32))
        }
        (PureOpKind::F32Le, ConstValue::F32(lhs), ConstValue::F32(rhs)) => {
            Some(ConstValue::I32((lhs <= rhs) as i32))
        }
        (PureOpKind::F32Ge, ConstValue::F32(lhs), ConstValue::F32(rhs)) => {
            Some(ConstValue::I32((lhs >= rhs) as i32))
        }
        (PureOpKind::F64Add, ConstValue::F64(lhs), ConstValue::F64(rhs)) => {
            Some(ConstValue::F64(lhs + rhs))
        }
        (PureOpKind::F64Sub, ConstValue::F64(lhs), ConstValue::F64(rhs)) => {
            Some(ConstValue::F64(lhs - rhs))
        }
        (PureOpKind::F64Mul, ConstValue::F64(lhs), ConstValue::F64(rhs)) => {
            Some(ConstValue::F64(lhs * rhs))
        }
        (PureOpKind::F64Div, ConstValue::F64(lhs), ConstValue::F64(rhs)) => {
            Some(ConstValue::F64(lhs / rhs))
        }
        (PureOpKind::F64Eq, ConstValue::F64(lhs), ConstValue::F64(rhs)) => {
            Some(ConstValue::I32((lhs == rhs) as i32))
        }
        (PureOpKind::F64Ne, ConstValue::F64(lhs), ConstValue::F64(rhs)) => {
            Some(ConstValue::I32((lhs != rhs) as i32))
        }
        (PureOpKind::F64Lt, ConstValue::F64(lhs), ConstValue::F64(rhs)) => {
            Some(ConstValue::I32((lhs < rhs) as i32))
        }
        (PureOpKind::F64Gt, ConstValue::F64(lhs), ConstValue::F64(rhs)) => {
            Some(ConstValue::I32((lhs > rhs) as i32))
        }
        (PureOpKind::F64Le, ConstValue::F64(lhs), ConstValue::F64(rhs)) => {
            Some(ConstValue::I32((lhs <= rhs) as i32))
        }
        (PureOpKind::F64Ge, ConstValue::F64(lhs), ConstValue::F64(rhs)) => {
            Some(ConstValue::I32((lhs >= rhs) as i32))
        }
        _ => None,
    }
}

fn canonicalize_binary_origins(
    op: PureOpKind,
    lhs: ExprOrigin,
    rhs: ExprOrigin,
) -> (ExprOrigin, ExprOrigin) {
    if is_commutative(op) && rhs < lhs {
        (rhs, lhs)
    } else {
        (lhs, rhs)
    }
}

fn is_commutative(op: PureOpKind) -> bool {
    matches!(
        op,
        PureOpKind::I32Add
            | PureOpKind::I32Mul
            | PureOpKind::I32And
            | PureOpKind::I32Or
            | PureOpKind::I32Xor
            | PureOpKind::I32Eq
            | PureOpKind::I32Ne
            | PureOpKind::I64Add
            | PureOpKind::F32Add
            | PureOpKind::F32Mul
            | PureOpKind::F32Eq
            | PureOpKind::F32Ne
            | PureOpKind::F64Add
            | PureOpKind::F64Mul
            | PureOpKind::F64Eq
            | PureOpKind::F64Ne
    )
}

fn simplify_identity_seed(
    op: PureOpKind,
    lhs: &AbstractValue,
    rhs: &AbstractValue,
) -> Option<(AbstractValue, AbstractValue)> {
    match (op, lhs.const_value, rhs.const_value) {
        (PureOpKind::I32Add, _, Some(ConstValue::I32(0)))
        | (PureOpKind::I32Sub, _, Some(ConstValue::I32(0)))
        | (PureOpKind::I32Or, _, Some(ConstValue::I32(0)))
        | (PureOpKind::I32Xor, _, Some(ConstValue::I32(0))) => Some((lhs.clone(), rhs.clone())),
        (PureOpKind::I32Add, Some(ConstValue::I32(0)), _)
        | (PureOpKind::I32Or, Some(ConstValue::I32(0)), _)
        | (PureOpKind::I32Xor, Some(ConstValue::I32(0)), _) => Some((rhs.clone(), lhs.clone())),
        (PureOpKind::I32Mul, _, Some(ConstValue::I32(1)))
        | (PureOpKind::I32And, _, Some(ConstValue::I32(-1))) => Some((lhs.clone(), rhs.clone())),
        (PureOpKind::I32Mul, Some(ConstValue::I32(1)), _)
        | (PureOpKind::I32And, Some(ConstValue::I32(-1)), _) => Some((rhs.clone(), lhs.clone())),
        (PureOpKind::I64Add, _, Some(ConstValue::I64(0)))
        | (PureOpKind::I64Sub, _, Some(ConstValue::I64(0))) => Some((lhs.clone(), rhs.clone())),
        (PureOpKind::I64Add, Some(ConstValue::I64(0)), _) => Some((rhs.clone(), lhs.clone())),
        _ => None,
    }
}

fn simplify_identity(
    op: PureOpKind,
    lhs: ExprId,
    rhs: ExprId,
    exprs: &[ExprState],
) -> Option<(ExprId, ExprId)> {
    match (op, exprs[lhs.0].const_value, exprs[rhs.0].const_value) {
        (PureOpKind::I32Add, _, Some(ConstValue::I32(0)))
        | (PureOpKind::I32Sub, _, Some(ConstValue::I32(0)))
        | (PureOpKind::I32Or, _, Some(ConstValue::I32(0)))
        | (PureOpKind::I32Xor, _, Some(ConstValue::I32(0))) => Some((lhs, rhs)),
        (PureOpKind::I32Add, Some(ConstValue::I32(0)), _)
        | (PureOpKind::I32Or, Some(ConstValue::I32(0)), _)
        | (PureOpKind::I32Xor, Some(ConstValue::I32(0)), _) => Some((rhs, lhs)),
        (PureOpKind::I32Mul, _, Some(ConstValue::I32(1)))
        | (PureOpKind::I32And, _, Some(ConstValue::I32(-1))) => Some((lhs, rhs)),
        (PureOpKind::I32Mul, Some(ConstValue::I32(1)), _)
        | (PureOpKind::I32And, Some(ConstValue::I32(-1)), _) => Some((rhs, lhs)),
        (PureOpKind::I64Add, _, Some(ConstValue::I64(0)))
        | (PureOpKind::I64Sub, _, Some(ConstValue::I64(0))) => Some((lhs, rhs)),
        (PureOpKind::I64Add, Some(ConstValue::I64(0)), _) => Some((rhs, lhs)),
        _ => None,
    }
}
