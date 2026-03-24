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
        AliasAddress, AliasKey, AliasSpace, BlockParam, ConstValue, EffectBarrier, EffectEpoch,
        ExprId, ExprOrigin, ExprOriginKind, ExprState, HeapVersion, LocalSlot, PureOpKind,
        ValueGraph, ValueKey,
    },
    sink::{RecordEmit, RewriteSink},
};

trait LocalPass {
    fn run_block(
        &mut self,
        program: &BasicBlockProgram,
        block: BasicBlock,
        entry: &BlockEntryState,
    ) -> BlockRunResult;
}

#[derive(Clone, Debug)]
struct AbstractValue {
    ty: ValType,
    origin: ExprOrigin,
    block_param: Option<BlockParam>,
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

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct LoopInvariantSet {
    pure_origins: BTreeSet<ExprOrigin>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum JoinAliasAddress {
    Const(u32),
    EntryLocal(usize),
    BlockParam(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct JoinAliasKey {
    space: AliasSpace,
    index: u32,
    width: u8,
    address: JoinAliasAddress,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct RecordGraphInfo {
    origin: Option<ExprOrigin>,
    use_count: usize,
    block_param: bool,
}

#[derive(Clone, Default)]
struct BlockRunResult {
    exit: BlockEntryState,
    records: Vec<RecordEmit>,
    record_graph: Vec<RecordGraphInfo>,
    loop_invariants: LoopInvariantSet,
}

#[derive(Default)]
struct RelowerState {
    records_by_block: Vec<Vec<RecordEmit>>,
    record_graphs_by_block: Vec<Vec<RecordGraphInfo>>,
    loop_invariants: Vec<LoopInvariantSet>,
}

#[derive(Default)]
struct FunctionRewrite {
    entries: Vec<BlockEntryState>,
    exits: Vec<BlockEntryState>,
    relower: RelowerState,
}

const UNKNOWN_HEAP_VERSION: u32 = u32::MAX;
const INSTR_RESULT_ORIGIN_STRIDE: usize = 256;

pub(crate) fn optimize_function(
    _funcidx: FuncIdx,
    _functype: &FuncType,
    locals: &mut LocalsData,
    instrs: Vec<Instr>,
    meta: Vec<InstructionMeta>,
) -> Vec<Instr> {
    let Some(program) = build_program(&instrs, meta) else {
        return instrs;
    };
    let rewrite = rewrite_program(&program);
    let mut per_block_records = rewrite.relower.records_by_block.to_vec();
    let licm_modified = apply_licm(&program, &rewrite, locals, &mut per_block_records);
    let per_block_records =
        select_superinstructions(&program, &rewrite, &per_block_records, &licm_modified);
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

fn rewrite_program(program: &BasicBlockProgram) -> FunctionRewrite {
    let mut rewrite = FunctionRewrite {
        entries: vec![BlockEntryState::default(); program.blocks.len()],
        exits: vec![BlockEntryState::default(); program.blocks.len()],
        relower: RelowerState {
            records_by_block: vec![Vec::new(); program.blocks.len()],
            record_graphs_by_block: vec![Vec::new(); program.blocks.len()],
            loop_invariants: vec![LoopInvariantSet::default(); program.blocks.len()],
        },
    };
    let mut pass = BlockOptimizer::default();
    let mut worklist = VecDeque::new();
    let mut queued = vec![false; program.blocks.len()];
    worklist.push_back(0usize);
    queued[0] = true;

    while let Some(block_id) = worklist.pop_front() {
        queued[block_id] = false;
        let Some(entry) = compute_entry_state(program, &rewrite, block_id) else {
            if clear_block_rewrite(&mut rewrite, block_id) {
                enqueue_successors(program, block_id, &mut worklist, &mut queued);
            }
            continue;
        };
        let entry_changed = !same_state(&rewrite.entries[block_id], &entry);
        if entry_changed {
            rewrite.entries[block_id] = entry.clone();
        }
        let result = pass.run_block(program, program.block(block_id), &entry);
        let exit_changed = !same_state(&rewrite.exits[block_id], &result.exit);
        let records_changed =
            !same_records(&rewrite.relower.records_by_block[block_id], &result.records);
        let graph_changed = rewrite.relower.record_graphs_by_block[block_id] != result.record_graph;
        let invariants_changed =
            rewrite.relower.loop_invariants[block_id] != result.loop_invariants;
        if exit_changed {
            rewrite.exits[block_id] = result.exit;
        }
        if records_changed {
            rewrite.relower.records_by_block[block_id] = result.records;
        }
        if graph_changed {
            rewrite.relower.record_graphs_by_block[block_id] = result.record_graph;
        }
        if invariants_changed {
            rewrite.relower.loop_invariants[block_id] = result.loop_invariants;
        }
        if entry_changed || exit_changed || records_changed || graph_changed || invariants_changed {
            enqueue_successors(program, block_id, &mut worklist, &mut queued);
        }
    }

    rewrite
}

fn compute_entry_state(
    program: &BasicBlockProgram,
    rewrite: &FunctionRewrite,
    block_id: usize,
) -> Option<BlockEntryState> {
    let block = program.block(block_id);
    let first = program.records.get(block.start)?;
    let mut incoming = Vec::new();
    if block_id == 0 {
        incoming.push(default_entry_state(block_id, first));
    }
    for pred in &program.predecessors[block_id] {
        let pred_state = &rewrite.exits[*pred];
        if pred_state.reachable {
            incoming.push(pred_state.clone());
        }
    }
    if incoming.is_empty() {
        return None;
    }
    Some(merge_states(block_id, first, &incoming))
}

fn clear_block_rewrite(rewrite: &mut FunctionRewrite, block_id: usize) -> bool {
    let entry_changed = !same_state(&rewrite.entries[block_id], &BlockEntryState::default());
    let exit_changed = !same_state(&rewrite.exits[block_id], &BlockEntryState::default());
    let records_changed = !rewrite.relower.records_by_block[block_id].is_empty();
    let graph_changed = !rewrite.relower.record_graphs_by_block[block_id].is_empty();
    let invariants_changed =
        rewrite.relower.loop_invariants[block_id] != LoopInvariantSet::default();
    if entry_changed {
        rewrite.entries[block_id] = BlockEntryState::default();
    }
    if exit_changed {
        rewrite.exits[block_id] = BlockEntryState::default();
    }
    if records_changed {
        rewrite.relower.records_by_block[block_id].clear();
    }
    if graph_changed {
        rewrite.relower.record_graphs_by_block[block_id].clear();
    }
    if invariants_changed {
        rewrite.relower.loop_invariants[block_id] = LoopInvariantSet::default();
    }
    entry_changed || exit_changed || records_changed || graph_changed || invariants_changed
}

fn enqueue_successors(
    program: &BasicBlockProgram,
    block_id: usize,
    worklist: &mut VecDeque<usize>,
    queued: &mut [bool],
) {
    for succ in &program.successors[block_id] {
        if !queued[*succ] {
            queued[*succ] = true;
            worklist.push_back(*succ);
        }
    }
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
                block_param: None,
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

    merge_aliases(block_id, incoming, &mut state);

    state
}

fn merge_aliases(block_id: usize, incoming: &[BlockEntryState], state: &mut BlockEntryState) {
    let mut exact_keys = if let Some(first_entry) = incoming.first() {
        first_entry.aliases.keys().copied().collect::<BTreeSet<_>>()
    } else {
        BTreeSet::new()
    };
    for entry in incoming.iter().skip(1) {
        exact_keys.retain(|key| entry.aliases.contains_key(key));
    }
    for key in exact_keys {
        if !space_version_stable(key.space, incoming, state.heap) {
            continue;
        }
        merge_alias_value(
            block_id,
            key,
            incoming
                .iter()
                .map(|entry| entry.aliases.get(&key))
                .collect::<Vec<_>>(),
            state,
        );
    }

    let mut join_keys = BTreeSet::new();
    for entry in incoming {
        for key in entry.aliases.keys().copied() {
            if let Some(join_key) = join_alias_key(key) {
                join_keys.insert(join_key);
            }
        }
    }
    for join_key in join_keys {
        if !space_version_stable(join_key.space, incoming, state.heap) {
            continue;
        }
        let merged_key = alias_key_from_join(block_id, join_key);
        if state.aliases.contains_key(&merged_key) {
            continue;
        }
        let mut values = Vec::with_capacity(incoming.len());
        let mut ambiguous = false;
        for entry in incoming {
            let matches = entry
                .aliases
                .iter()
                .filter_map(|(key, value)| {
                    (join_alias_key(*key) == Some(join_key)).then_some(value)
                })
                .collect::<Vec<_>>();
            if matches.len() != 1 {
                ambiguous = true;
                break;
            }
            values.push(Some(matches[0]));
        }
        if ambiguous {
            continue;
        }
        merge_alias_value(block_id, merged_key, values, state);
    }
}

fn merge_alias_value(
    block_id: usize,
    key: AliasKey,
    values: Vec<Option<&AbstractValue>>,
    state: &mut BlockEntryState,
) {
    let Some(first_value) = values.first().and_then(|value| *value) else {
        return;
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
            block_param: Some(BlockParam {
                block_id,
                ordinal,
                ty,
            }),
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
        block_param: Some(BlockParam {
            block_id,
            ordinal,
            ty,
        }),
        const_value,
        key,
    }
}

fn alias_ordinal(key: AliasKey) -> usize {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    key.hash(&mut hasher);
    hasher.finish() as usize
}

fn join_alias_key(key: AliasKey) -> Option<JoinAliasKey> {
    let address = match key.address {
        AliasAddress::Const(value) => JoinAliasAddress::Const(value),
        AliasAddress::Origin(origin) if origin.kind == ExprOriginKind::EntryLocal => {
            JoinAliasAddress::EntryLocal(origin.ordinal)
        }
        AliasAddress::Origin(origin) if origin.kind == ExprOriginKind::BlockParam => {
            JoinAliasAddress::BlockParam(origin.ordinal)
        }
        _ => return None,
    };
    Some(JoinAliasKey {
        space: key.space,
        index: key.index,
        width: key.width,
        address,
    })
}

fn alias_key_from_join(block_id: usize, key: JoinAliasKey) -> AliasKey {
    let address = match key.address {
        JoinAliasAddress::Const(value) => AliasAddress::Const(value),
        JoinAliasAddress::EntryLocal(ordinal) => AliasAddress::Origin(ExprOrigin {
            block_id,
            ordinal,
            kind: ExprOriginKind::EntryLocal,
        }),
        JoinAliasAddress::BlockParam(ordinal) => AliasAddress::Origin(ExprOrigin {
            block_id,
            ordinal,
            kind: ExprOriginKind::BlockParam,
        }),
    };
    AliasKey {
        space: key.space,
        index: key.index,
        width: key.width,
        address,
    }
}
fn instr_result_origin_ordinal(ordinal: usize, result_index: usize) -> usize {
    ordinal
        .saturating_mul(INSTR_RESULT_ORIGIN_STRIDE)
        .saturating_add(result_index)
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
        && lhs.block_param == rhs.block_param
        && lhs.const_value == rhs.const_value
        && lhs.key == rhs.key
}

#[derive(Default)]
struct BlockOptimizer {
    block_id: usize,
    effect_epoch: EffectEpoch,
    sink: RewriteSink,
    exprs: ValueGraph,
    stack: Vec<ExprId>,
    locals: HashMap<LocalSlot, ExprId>,
    origin_locals: HashMap<ExprOrigin, LocalSlot>,
    cse: HashMap<ValueKey, CseEntry>,
    aliases: HashMap<AliasKey, ExprId>,
    last_local_write: Option<LocalWrite>,
    last_store: HashMap<AliasKey, StoreWrite>,
    heap: HeapVersion,
    loop_invariants: LoopInvariantSet,
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
    ) -> BlockRunResult {
        self.reset(block, entry);
        for record_idx in block.start..block.end {
            let record = &program.records[record_idx];
            let ordinal = record_idx - block.start;
            self.visit_record(record, ordinal);
        }
        BlockRunResult {
            exit: self.snapshot_exit_state(),
            records: self.sink.clone().into_live_records(),
            record_graph: self.build_record_graph(),
            loop_invariants: self.loop_invariants.clone(),
        }
    }
}

impl BlockOptimizer {
    fn reset(&mut self, block: BasicBlock, entry: &BlockEntryState) {
        self.block_id = block.id;
        self.effect_epoch = 0;
        self.sink = RewriteSink::default();
        self.exprs.nodes.clear();
        self.exprs.latest_by_origin.clear();
        self.stack.clear();
        self.locals.clear();
        self.origin_locals.clear();
        self.cse.clear();
        self.aliases.clear();
        self.last_local_write = None;
        self.last_store.clear();
        self.heap = entry.heap;
        self.loop_invariants = LoopInvariantSet::default();

        let mut locals = entry.locals.iter().collect::<Vec<_>>();
        locals.sort_by_key(|(slot, _)| (slot.addr, slot.size));
        for (slot, value) in locals {
            let expr = self.seed_value(value, false);
            self.bind_local(*slot, expr);
            self.seed_cse(expr);
            self.maybe_mark_loop_invariant(expr);
        }

        for value in &entry.stack {
            let expr = self.seed_value(value, false);
            self.push_stack(expr);
            self.seed_cse(expr);
            self.maybe_mark_loop_invariant(expr);
        }

        let mut aliases = entry.aliases.iter().collect::<Vec<_>>();
        aliases.sort_by_key(|(key, _)| (key.space as u8, key.index, key.width));
        for (key, value) in aliases {
            let expr = self.seed_value(value, false);
            self.aliases.insert(*key, expr);
            self.maybe_mark_loop_invariant(expr);
        }
    }

    fn seed_value(&mut self, value: &AbstractValue, removable: bool) -> ExprId {
        let id = ExprId(self.exprs.nodes.len());
        self.exprs.nodes.push(ExprState {
            ty: value.ty,
            origin: value.origin,
            block_param: value.block_param,
            const_value: value.const_value,
            key: value.key,
            producer_record: None,
            materialized_record: None,
            use_count: 0,
            ref_count: 0,
            removable,
        });
        self.exprs.latest_by_origin.insert(value.origin, id);
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
                source_state.block_param,
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
            None,
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
            None,
            Some(key),
            Some(record_idx),
            true,
        );
        self.maybe_mark_loop_invariant(expr);
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
            None,
            Some(key),
            Some(record_idx),
            true,
        );
        self.maybe_mark_loop_invariant(expr);
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
        let Some(address) = self.canonical_alias_address(index) else {
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
            None,
            Some(ValueKey::TableGet {
                tableidx,
                index: self.exprs[index.0].origin,
            }),
            Some(record_idx),
            false,
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
        if let Some(address) = self.canonical_alias_address(index) {
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
        let Some(key) = self.memory_alias_key(access, address) else {
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
            None,
            Some(ValueKey::MemoryLoad(key)),
            Some(record_idx),
            false,
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
        if let Some(key) = self.memory_alias_key(access, address) {
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
            None,
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
        self.exprs[expr.0].use_count = self.exprs[expr.0].use_count.saturating_add(1);
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
        let state = &self.exprs[expr.0];
        state.const_value.is_some()
            || self.origin_locals.contains_key(&state.origin)
            || self.can_materialize_key(state.key)
    }

    fn bump_effect_epoch(&mut self) {
        self.effect_epoch += 1;
        self.cse.clear();
    }

    fn snapshot_exit_state(&self) -> BlockEntryState {
        let mut state = BlockEntryState {
            reachable: true,
            heap: self.heap,
            ..BlockEntryState::default()
        };

        let mut locals = self.locals.iter().collect::<Vec<_>>();
        locals.sort_by_key(|(slot, _)| (slot.addr, slot.size));
        for (slot, expr) in locals {
            state.locals.insert(*slot, self.snapshot_value(*expr));
        }

        for expr in &self.stack {
            state.stack.push(self.snapshot_value(*expr));
        }

        let mut aliases = self.aliases.iter().collect::<Vec<_>>();
        aliases.sort_by_key(|(key, _)| (key.space as u8, key.index, key.width));
        for (key, expr) in aliases {
            state.aliases.insert(*key, self.snapshot_value(*expr));
        }

        state
    }

    fn build_record_graph(&self) -> Vec<RecordGraphInfo> {
        let mut expr_by_record = HashMap::new();
        for (expr_idx, expr) in self.exprs.nodes.iter().enumerate() {
            if let Some(record_idx) = expr.materialized_record {
                expr_by_record.entry(record_idx).or_insert(ExprId(expr_idx));
            }
        }
        self.sink
            .live_indices()
            .into_iter()
            .map(|record_idx| {
                expr_by_record
                    .get(&record_idx)
                    .map(|expr| {
                        let expr = &self.exprs[expr.0];
                        RecordGraphInfo {
                            origin: Some(expr.origin),
                            use_count: expr.use_count,
                            block_param: expr.block_param.is_some(),
                        }
                    })
                    .unwrap_or_default()
            })
            .collect()
    }

    fn snapshot_value(&self, expr: ExprId) -> AbstractValue {
        let state = &self.exprs[expr.0];
        AbstractValue {
            ty: state.ty,
            origin: state.origin,
            block_param: state.block_param,
            const_value: state.const_value,
            key: state.key,
        }
    }

    fn can_materialize_key(&self, key: Option<ValueKey>) -> bool {
        let Some(key) = key else {
            return false;
        };
        match key {
            ValueKey::Unary { input, .. } => self
                .exprs
                .latest_by_origin
                .get(&input)
                .copied()
                .is_some_and(|expr| self.can_materialize(expr)),
            ValueKey::Binary { lhs, rhs, .. } => {
                self.exprs
                    .latest_by_origin
                    .get(&lhs)
                    .copied()
                    .is_some_and(|expr| self.can_materialize(expr))
                    && self
                        .exprs
                        .latest_by_origin
                        .get(&rhs)
                        .copied()
                        .is_some_and(|expr| self.can_materialize(expr))
            }
            ValueKey::MemoryLoad(_) | ValueKey::GlobalGet { .. } | ValueKey::TableGet { .. } => {
                false
            }
        }
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
                None,
                false,
            );
            self.push_stack(expr);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn new_expr_with_origin(
        &mut self,
        ty: ValType,
        origin: ExprOrigin,
        block_param: Option<BlockParam>,
        const_value: Option<ConstValue>,
        key: Option<ValueKey>,
        producer_record: Option<usize>,
        removable: bool,
    ) -> ExprId {
        let id = ExprId(self.exprs.nodes.len());
        self.exprs.nodes.push(ExprState {
            ty,
            origin,
            block_param,
            const_value,
            key,
            producer_record,
            materialized_record: producer_record,
            use_count: 0,
            ref_count: 0,
            removable,
        });
        self.exprs.latest_by_origin.insert(origin, id);
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
        if let Some(slot) = self
            .origin_locals
            .get(&self.exprs[source.0].origin)
            .copied()
        {
            let op = local_get_op(slot.size);
            let record_idx = self.sink.push(
                Some(source_start),
                op,
                vec![Operand {
                    local_addr: slot.addr,
                }],
            );
            let source_state = self.exprs[source.0].clone();
            return Some(self.new_expr_with_origin(
                source_state.ty,
                source_state.origin,
                source_state.block_param,
                source_state.const_value,
                source_state.key,
                Some(record_idx),
                true,
            ));
        }
        self.try_materialize_pure_value(source_start, source)
    }

    fn try_materialize_pure_value(
        &mut self,
        source_start: usize,
        source: ExprId,
    ) -> Option<ExprId> {
        let source_state = self.exprs[source.0].clone();
        match source_state.key? {
            ValueKey::Unary { op, input } => {
                let input_expr = self.exprs.latest_by_origin.get(&input).copied()?;
                let _ = self.try_materialize_value(source_start, input_expr)?;
                let record_idx = self
                    .sink
                    .push(Some(source_start), unary_op(op)?, Vec::new());
                Some(self.new_expr_with_origin(
                    source_state.ty,
                    source_state.origin,
                    source_state.block_param,
                    source_state.const_value,
                    source_state.key,
                    Some(record_idx),
                    true,
                ))
            }
            ValueKey::Binary { op, lhs, rhs } => {
                let lhs_expr = self.exprs.latest_by_origin.get(&lhs).copied()?;
                let rhs_expr = self.exprs.latest_by_origin.get(&rhs).copied()?;
                let _ = self.try_materialize_value(source_start, lhs_expr)?;
                let _ = self.try_materialize_value(source_start, rhs_expr)?;
                let record_idx = self
                    .sink
                    .push(Some(source_start), binary_op(op)?, Vec::new());
                Some(self.new_expr_with_origin(
                    source_state.ty,
                    source_state.origin,
                    source_state.block_param,
                    source_state.const_value,
                    source_state.key,
                    Some(record_idx),
                    true,
                ))
            }
            ValueKey::MemoryLoad(_) | ValueKey::GlobalGet { .. } | ValueKey::TableGet { .. } => {
                None
            }
        }
    }

    fn maybe_mark_loop_invariant(&mut self, expr: ExprId) {
        if self.expr_is_loop_invariant(expr) {
            self.loop_invariants
                .pure_origins
                .insert(self.exprs[expr.0].origin);
        }
    }

    fn expr_is_loop_invariant(&self, expr: ExprId) -> bool {
        let state = &self.exprs[expr.0];
        if state.block_param.is_some() {
            return false;
        }
        if state.const_value.is_some() {
            return true;
        }
        match state.origin.kind {
            ExprOriginKind::EntryLocal | ExprOriginKind::SyntheticConst => return true,
            ExprOriginKind::EntryStack | ExprOriginKind::BlockParam => return false,
            _ => {}
        }
        match state.key {
            Some(ValueKey::Unary { input, .. }) => self
                .exprs
                .latest_by_origin
                .get(&input)
                .copied()
                .is_some_and(|input| self.expr_is_loop_invariant(input)),
            Some(ValueKey::Binary { lhs, rhs, .. }) => {
                self.exprs
                    .latest_by_origin
                    .get(&lhs)
                    .copied()
                    .is_some_and(|lhs| self.expr_is_loop_invariant(lhs))
                    && self
                        .exprs
                        .latest_by_origin
                        .get(&rhs)
                        .copied()
                        .is_some_and(|rhs| self.expr_is_loop_invariant(rhs))
            }
            Some(ValueKey::MemoryLoad(_))
            | Some(ValueKey::GlobalGet { .. })
            | Some(ValueKey::TableGet { .. })
            | None => false,
        }
    }

    fn canonical_alias_address(&self, expr: ExprId) -> Option<AliasAddress> {
        let value = &self.exprs[expr.0];
        value
            .const_value
            .and_then(|value| match value {
                ConstValue::I32(value) => Some(AliasAddress::Const(value as u32)),
                _ => None,
            })
            .or_else(|| {
                self.origin_locals
                    .get(&value.origin)
                    .map(|slot| AliasAddress::Origin(local_alias_origin(self.block_id, *slot)))
            })
            .or_else(|| {
                value.block_param.map(|param| {
                    AliasAddress::Origin(ExprOrigin {
                        block_id: param.block_id,
                        ordinal: param.ordinal,
                        kind: ExprOriginKind::BlockParam,
                    })
                })
            })
            .or(Some(AliasAddress::Origin(value.origin)))
    }

    fn memory_alias_key(&self, access: MemoryAccess, address: ExprId) -> Option<AliasKey> {
        Some(AliasKey {
            space: AliasSpace::Memory,
            index: access.memidx,
            width: access.width,
            address: self.canonical_alias_address(address)?,
        })
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
        && lhs.block_param == rhs.block_param
        && lhs.const_value == rhs.const_value
        && lhs.key == rhs.key
}

fn same_records(lhs: &[RecordEmit], rhs: &[RecordEmit]) -> bool {
    lhs.len() == rhs.len()
        && lhs.iter().zip(rhs.iter()).all(|(lhs, rhs)| {
            lhs.source_start == rhs.source_start
                && lhs.alive == rhs.alive
                && std::ptr::fn_addr_eq(lhs.op, rhs.op)
                && lhs.operands.len() == rhs.operands.len()
                && lhs
                    .operands
                    .iter()
                    .zip(rhs.operands.iter())
                    .all(|(lhs, rhs)| unsafe { lhs.encoded == rhs.encoded })
        })
}

#[derive(Clone)]
struct NaturalLoop {
    header: usize,
    preheader: usize,
    blocks: BTreeSet<usize>,
}

#[derive(Default)]
struct LoopEffects {
    local_writes: BTreeSet<LocalSlot>,
    global_writes: BTreeSet<LocalSlot>,
    has_memory_mutation: bool,
    has_call_barrier: bool,
}

#[derive(Clone, Copy)]
struct LicmCandidate {
    start: usize,
    end: usize,
    result_size: u32,
    source_start: Option<usize>,
}

fn apply_licm(
    program: &BasicBlockProgram,
    rewrite: &FunctionRewrite,
    locals: &mut LocalsData,
    records_by_block: &mut [Vec<RecordEmit>],
) -> Vec<bool> {
    let loops = collect_natural_loops(program);
    let mut modified = vec![false; program.blocks.len()];
    for loop_info in loops {
        let effects = summarize_loop_effects(program, &loop_info.blocks);
        let header_records = records_by_block[loop_info.header].clone();
        let default_invariants = LoopInvariantSet::default();
        let loop_invariants = rewrite
            .relower
            .loop_invariants
            .get(loop_info.header)
            .unwrap_or(&default_invariants);
        let candidates = collect_licm_candidates(
            &header_records,
            &effects,
            loop_invariants,
            rewrite
                .relower
                .record_graphs_by_block
                .get(loop_info.header)
                .map(Vec::as_slice)
                .unwrap_or(&[]),
        );
        if candidates.is_empty() {
            continue;
        }

        let mut preheader_insert = Vec::new();
        let mut new_header = Vec::with_capacity(header_records.len());
        let mut cursor = 0usize;
        while cursor < header_records.len() {
            if let Some(candidate) = candidates
                .iter()
                .find(|candidate| candidate.start == cursor)
            {
                let temp = LocalSlot::new(
                    locals.allocate_temp_slot(type_from_slot(candidate.result_size)),
                    candidate.result_size,
                );
                preheader_insert.extend(emit_licm_candidate(candidate, temp, &header_records));
                new_header.push(RecordEmit {
                    source_start: candidate.source_start,
                    op: local_get_op(candidate.result_size),
                    operands: vec![Operand {
                        local_addr: temp.addr,
                    }],
                    alive: true,
                });
                cursor = candidate.end;
                modified[loop_info.header] = true;
                modified[loop_info.preheader] = true;
                continue;
            }
            new_header.push(header_records[cursor].clone());
            cursor += 1;
        }

        if preheader_insert.is_empty() {
            continue;
        }
        insert_before_terminal(&mut records_by_block[loop_info.preheader], preheader_insert);
        records_by_block[loop_info.header] = new_header;
    }
    modified
}

fn collect_natural_loops(program: &BasicBlockProgram) -> Vec<NaturalLoop> {
    let mut seen = BTreeSet::new();
    let mut loops = Vec::new();
    for (pred, successors) in program.successors.iter().enumerate() {
        for succ in successors {
            if *succ > pred {
                continue;
            }
            let blocks = natural_loop_blocks(program, pred, *succ);
            let outside_preds = program.predecessors[*succ]
                .iter()
                .copied()
                .filter(|candidate| !blocks.contains(candidate))
                .collect::<Vec<_>>();
            if outside_preds.len() != 1 {
                continue;
            }
            let preheader = outside_preds[0];
            if program.successors[preheader].as_slice() != [*succ] {
                continue;
            }
            if seen.insert((*succ, preheader)) {
                loops.push(NaturalLoop {
                    header: *succ,
                    preheader,
                    blocks,
                });
            }
        }
    }
    loops.sort_by_key(|loop_info| (loop_info.header, loop_info.preheader));
    loops
}

fn natural_loop_blocks(
    program: &BasicBlockProgram,
    latch: usize,
    header: usize,
) -> BTreeSet<usize> {
    let mut blocks = BTreeSet::from([header, latch]);
    let mut queue = VecDeque::from([latch]);
    while let Some(block_id) = queue.pop_front() {
        for pred in &program.predecessors[block_id] {
            if blocks.insert(*pred) && *pred != header {
                queue.push_back(*pred);
            }
        }
    }
    blocks
}

fn summarize_loop_effects(program: &BasicBlockProgram, blocks: &BTreeSet<usize>) -> LoopEffects {
    let mut effects = LoopEffects::default();
    for block_id in blocks {
        let block = program.block(*block_id);
        for record in &program.records[block.start..block.end] {
            if let Some(slot) = decode_local_set(record).or_else(|| decode_local_tee(record)) {
                effects.local_writes.insert(slot);
            }
            if let Some(slot) = decode_global_set(record) {
                effects.global_writes.insert(slot);
            }
            match effect_barrier(record) {
                EffectBarrier::Call => effects.has_call_barrier = true,
                EffectBarrier::Memory => effects.has_memory_mutation = true,
                _ => {}
            }
            if decode_memory_store(record).is_some() {
                effects.has_memory_mutation = true;
            }
        }
    }
    effects
}

fn collect_licm_candidates(
    records: &[RecordEmit],
    effects: &LoopEffects,
    loop_invariants: &LoopInvariantSet,
    record_graph: &[RecordGraphInfo],
) -> Vec<LicmCandidate> {
    let mut candidates = Vec::new();
    let mut cursor = 0usize;
    while cursor < records.len() {
        if record_is(&records[cursor], vm::op_loop as Op) {
            cursor += 1;
            continue;
        }
        if record_is_control_like(&records[cursor]) {
            break;
        }
        if let Some(candidate) =
            match_licm_candidate(records, record_graph, loop_invariants, cursor, effects)
        {
            cursor = candidate.end;
            candidates.push(candidate);
            continue;
        }
        cursor += 1;
    }
    candidates
}

fn match_licm_candidate(
    records: &[RecordEmit],
    record_graph: &[RecordGraphInfo],
    loop_invariants: &LoopInvariantSet,
    cursor: usize,
    effects: &LoopEffects,
) -> Option<LicmCandidate> {
    if let Some(slot) = record_local_get_slot(records.get(cursor)?) {
        if records
            .get(cursor + 1)
            .and_then(record_i32_const)
            .zip(records.get(cursor + 2))
            .is_some()
        {
            let op = records.get(cursor + 2)?;
            if record_is(op, vm::op_i32_add as Op) || record_is(op, vm::op_i32_sub as Op) {
                if effects.local_writes.contains(&slot)
                    || !graph_info_single_use(record_graph, cursor)
                    || !graph_info_single_use(record_graph, cursor + 1)
                    || !graph_info_single_use(record_graph, cursor + 2)
                    || !record_has_invariant_origin(record_graph, loop_invariants, cursor + 2)
                {
                    return None;
                }
                return Some(LicmCandidate {
                    start: cursor,
                    end: cursor + 3,
                    result_size: 4,
                    source_start: records[cursor].source_start,
                });
            }
        }
        if let Some(rhs) = records.get(cursor + 1).and_then(record_local_get_slot) {
            if records
                .get(cursor + 2)
                .is_some_and(|record| record_is(record, vm::op_i32_add as Op))
            {
                if effects.local_writes.contains(&slot)
                    || effects.local_writes.contains(&rhs)
                    || !graph_info_single_use(record_graph, cursor)
                    || !graph_info_single_use(record_graph, cursor + 1)
                    || !graph_info_single_use(record_graph, cursor + 2)
                    || !record_has_invariant_origin(record_graph, loop_invariants, cursor + 2)
                {
                    return None;
                }
                return Some(LicmCandidate {
                    start: cursor,
                    end: cursor + 3,
                    result_size: 4,
                    source_start: records[cursor].source_start,
                });
            }
        }
    }

    if let Some(slot) = record_global_get_slot(records.get(cursor)?) {
        if effects.has_call_barrier || effects.global_writes.contains(&slot) {
            return None;
        }
        return Some(LicmCandidate {
            start: cursor,
            end: cursor + 1,
            result_size: slot.size,
            source_start: records[cursor].source_start,
        });
    }

    if effects.has_call_barrier || effects.has_memory_mutation {
        return None;
    }
    let address = records.get(cursor)?;
    let load = records
        .get(cursor + 1)
        .and_then(record_memory_load_access)?;
    let _address = if let Some(slot) = record_local_get_slot(address) {
        if effects.local_writes.contains(&slot) {
            return None;
        }
        AliasAddress::Origin(ExprOrigin {
            block_id: 0,
            ordinal: slot.addr as usize,
            kind: ExprOriginKind::EntryLocal,
        })
    } else if let Some(value) = record_i32_const(address) {
        AliasAddress::Const(value as u32)
    } else {
        return None;
    };
    Some(LicmCandidate {
        start: cursor,
        end: cursor + 2,
        result_size: load.ty.stack_size().u32(),
        source_start: records[cursor].source_start,
    })
}

fn emit_licm_candidate(
    candidate: &LicmCandidate,
    temp: LocalSlot,
    header_records: &[RecordEmit],
) -> Vec<RecordEmit> {
    let mut out = header_records[candidate.start..candidate.end]
        .iter()
        .cloned()
        .map(|mut record| {
            record.source_start = None;
            record
        })
        .collect::<Vec<_>>();
    out.push(RecordEmit {
        source_start: None,
        op: local_set_op(temp.size),
        operands: vec![Operand {
            local_addr: temp.addr,
        }],
        alive: true,
    });
    out
}

fn insert_before_terminal(records: &mut Vec<RecordEmit>, mut insert: Vec<RecordEmit>) {
    let insert_at = records
        .last()
        .filter(|record| record_ends_basic_block(record))
        .map(|_| records.len().saturating_sub(1))
        .unwrap_or(records.len());
    records.splice(insert_at..insert_at, insert.drain(..));
}

fn graph_info_single_use(record_graph: &[RecordGraphInfo], idx: usize) -> bool {
    record_graph
        .get(idx)
        .is_some_and(|info| info.origin.is_some() && info.use_count <= 1 && !info.block_param)
}

fn record_has_invariant_origin(
    record_graph: &[RecordGraphInfo],
    loop_invariants: &LoopInvariantSet,
    idx: usize,
) -> bool {
    record_graph
        .get(idx)
        .and_then(|info| info.origin)
        .is_some_and(|origin| loop_invariants.pure_origins.contains(&origin))
}

fn record_ends_basic_block(record: &RecordEmit) -> bool {
    record_is_control_like(record)
        || record_is(record, vm::special_function_return as Op)
        || record_is(record, vm::special_block_return as Op)
}

fn record_is_control_like(record: &RecordEmit) -> bool {
    record_is(record, vm::op_if as Op)
        || record_is(record, vm::op_else as Op)
        || record_is(record, vm::op_br as Op)
        || record_is(record, vm::op_br_if as Op)
        || record_is(record, vm::op_br_table as Op)
        || record_is(record, vm::op_return as Op)
        || record_is(record, vm::op_loop as Op)
        || record_is(record, vm::op_end as Op)
}

fn record_local_get_slot(record: &RecordEmit) -> Option<LocalSlot> {
    if record_is(record, vm::op_local_get4 as Op) {
        return Some(LocalSlot::new(unsafe { record.operands[0].local_addr }, 4));
    }
    if record_is(record, vm::op_local_get8 as Op) {
        return Some(LocalSlot::new(unsafe { record.operands[0].local_addr }, 8));
    }
    if record_is(record, vm::op_local_get16 as Op) {
        return Some(LocalSlot::new(unsafe { record.operands[0].local_addr }, 16));
    }
    None
}

fn record_global_get_slot(record: &RecordEmit) -> Option<LocalSlot> {
    if record_is(record, vm::op_global_get4 as Op) {
        return Some(LocalSlot::new(unsafe { record.operands[0].u32 }, 4));
    }
    if record_is(record, vm::op_global_get8 as Op) {
        return Some(LocalSlot::new(unsafe { record.operands[0].u32 }, 8));
    }
    if record_is(record, vm::op_global_get16 as Op) {
        return Some(LocalSlot::new(unsafe { record.operands[0].u32 }, 16));
    }
    None
}

fn record_i32_const(record: &RecordEmit) -> Option<i32> {
    record_is(record, vm::op_i32_const as Op).then(|| unsafe { record.operands[0].i32 })
}

fn record_memory_load_access(record: &RecordEmit) -> Option<MemoryAccess> {
    if record_is(record, vm::op_i32_load_local as Op) || record_is(record, vm::op_i32_load as Op) {
        return Some(MemoryAccess {
            memidx: 0,
            width: 4,
            ty: ValType::I32,
        });
    }
    if record_is(record, vm::op_i64_load_local as Op) || record_is(record, vm::op_i64_load as Op) {
        return Some(MemoryAccess {
            memidx: 0,
            width: 8,
            ty: ValType::I64,
        });
    }
    if record_is(record, vm::op_f32_load_local as Op) || record_is(record, vm::op_f32_load as Op) {
        return Some(MemoryAccess {
            memidx: 0,
            width: 4,
            ty: ValType::F32,
        });
    }
    if record_is(record, vm::op_f64_load_local as Op) || record_is(record, vm::op_f64_load as Op) {
        return Some(MemoryAccess {
            memidx: 0,
            width: 8,
            ty: ValType::F64,
        });
    }
    None
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

fn select_superinstructions(
    program: &BasicBlockProgram,
    rewrite: &FunctionRewrite,
    records_by_block: &[Vec<RecordEmit>],
    licm_modified: &[bool],
) -> Vec<Vec<RecordEmit>> {
    let mut selected = vec![Vec::new(); program.blocks.len()];
    for block in &program.blocks {
        let records = &records_by_block[block.id];
        if licm_modified.get(block.id).copied().unwrap_or(false) {
            selected[block.id] = records.clone();
            continue;
        }
        let record_graph = rewrite
            .relower
            .record_graphs_by_block
            .get(block.id)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        selected[block.id] = select_block_superinstructions(records, record_graph);
    }
    selected
}

fn select_block_superinstructions(
    records: &[RecordEmit],
    record_graph: &[RecordGraphInfo],
) -> Vec<RecordEmit> {
    let mut out = Vec::with_capacity(records.len());
    let mut cursor = 0usize;
    while cursor < records.len() {
        if let Some((fused, consumed)) =
            match_selector_pattern(&records[cursor..], &record_graph[cursor..])
        {
            out.push(fused);
            cursor += consumed;
            continue;
        }
        out.push(records[cursor].clone());
        cursor += 1;
    }
    out
}

fn match_selector_pattern(
    records: &[RecordEmit],
    record_graph: &[RecordGraphInfo],
) -> Option<(RecordEmit, usize)> {
    if records.len() >= 4
        && record_is(&records[0], vm::op_local_get4 as Op)
        && record_is(&records[1], vm::op_i32_const as Op)
        && (record_is(&records[2], vm::op_i32_add as Op)
            || record_is(&records[2], vm::op_i32_sub as Op))
        && record_is(&records[3], vm::op_local_set4 as Op)
        && graph_info_single_use(record_graph, 0)
        && graph_info_single_use(record_graph, 1)
        && graph_info_single_use(record_graph, 2)
        && !next_record_is_call_like(records, 4)
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
        && graph_info_single_use(record_graph, 0)
        && graph_info_single_use(record_graph, 1)
        && graph_info_single_use(record_graph, 2)
        && !next_record_is_call_like(records, 3)
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
        && graph_info_single_use(record_graph, 0)
        && graph_info_single_use(record_graph, 1)
        && graph_info_single_use(record_graph, 2)
        && !next_record_is_call_like(records, 4)
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
        && graph_info_single_use(record_graph, 0)
        && graph_info_single_use(record_graph, 1)
        && graph_info_single_use(record_graph, 2)
        && !next_record_is_call_like(records, 4)
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
        && graph_info_single_use(record_graph, 0)
        && graph_info_single_use(record_graph, 1)
        && graph_info_single_use(record_graph, 2)
        && !next_record_is_call_like(records, 3)
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
        && graph_info_single_use(record_graph, 0)
        && graph_info_single_use(record_graph, 1)
        && graph_info_single_use(record_graph, 2)
        && !next_record_is_call_like(records, 4)
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

fn next_record_is_call_like(records: &[RecordEmit], consumed: usize) -> bool {
    records.get(consumed).is_some_and(|record| {
        record_is(record, vm::op_call as Op)
            || record_is(record, vm::op_call_import as Op)
            || record_is(record, vm::op_return_call as Op)
            || record_is(record, vm::op_return_call_import as Op)
            || record_is(record, vm::op_call_indirect as Op)
            || record_is(record, vm::op_return_call_indirect as Op)
    })
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

fn unary_op(op: PureOpKind) -> Option<Op> {
    match op {
        PureOpKind::I32Eqz => Some(vm::op_i32_eqz as Op),
        PureOpKind::I64Eqz => Some(vm::op_i64_eqz as Op),
        _ => None,
    }
}

fn binary_op(op: PureOpKind) -> Option<Op> {
    match op {
        PureOpKind::I32Eqz | PureOpKind::I64Eqz => None,
        PureOpKind::I32Add => Some(vm::op_i32_add as Op),
        PureOpKind::I32Sub => Some(vm::op_i32_sub as Op),
        PureOpKind::I32Mul => Some(vm::op_i32_mul as Op),
        PureOpKind::I32And => Some(vm::op_i32_and as Op),
        PureOpKind::I32Or => Some(vm::op_i32_or as Op),
        PureOpKind::I32Xor => Some(vm::op_i32_xor as Op),
        PureOpKind::I32Eq => Some(vm::op_i32_eq as Op),
        PureOpKind::I32Ne => Some(vm::op_i32_ne as Op),
        PureOpKind::I32LtS => Some(vm::op_i32_lt_s as Op),
        PureOpKind::I32LtU => Some(vm::op_i32_lt_u as Op),
        PureOpKind::I32GtS => Some(vm::op_i32_gt_s as Op),
        PureOpKind::I32GtU => Some(vm::op_i32_gt_u as Op),
        PureOpKind::I32LeS => Some(vm::op_i32_le_s as Op),
        PureOpKind::I32LeU => Some(vm::op_i32_le_u as Op),
        PureOpKind::I32GeS => Some(vm::op_i32_ge_s as Op),
        PureOpKind::I32GeU => Some(vm::op_i32_ge_u as Op),
        PureOpKind::I64Add => Some(vm::op_i64_add as Op),
        PureOpKind::I64Sub => Some(vm::op_i64_sub as Op),
        PureOpKind::F32Add => Some(vm::op_f32_add as Op),
        PureOpKind::F32Sub => Some(vm::op_f32_sub as Op),
        PureOpKind::F32Mul => Some(vm::op_f32_mul as Op),
        PureOpKind::F32Div => Some(vm::op_f32_div as Op),
        PureOpKind::F32Eq => Some(vm::op_f32_eq as Op),
        PureOpKind::F32Ne => Some(vm::op_f32_ne as Op),
        PureOpKind::F32Lt => Some(vm::op_f32_lt as Op),
        PureOpKind::F32Gt => Some(vm::op_f32_gt as Op),
        PureOpKind::F32Le => Some(vm::op_f32_le as Op),
        PureOpKind::F32Ge => Some(vm::op_f32_ge as Op),
        PureOpKind::F64Add => Some(vm::op_f64_add as Op),
        PureOpKind::F64Sub => Some(vm::op_f64_sub as Op),
        PureOpKind::F64Mul => Some(vm::op_f64_mul as Op),
        PureOpKind::F64Div => Some(vm::op_f64_div as Op),
        PureOpKind::F64Eq => Some(vm::op_f64_eq as Op),
        PureOpKind::F64Ne => Some(vm::op_f64_ne as Op),
        PureOpKind::F64Lt => Some(vm::op_f64_lt as Op),
        PureOpKind::F64Gt => Some(vm::op_f64_gt as Op),
        PureOpKind::F64Le => Some(vm::op_f64_le as Op),
        PureOpKind::F64Ge => Some(vm::op_f64_ge as Op),
    }
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

fn local_set_op(size: u32) -> Op {
    match size {
        4 => vm::op_local_set4 as Op,
        8 => vm::op_local_set8 as Op,
        16 => vm::op_local_set16 as Op,
        _ => vm::op_local_set4 as Op,
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

fn local_alias_origin(block_id: usize, slot: LocalSlot) -> ExprOrigin {
    ExprOrigin {
        block_id,
        ordinal: slot.addr as usize,
        kind: ExprOriginKind::EntryLocal,
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::core::type_checker::StackSnapshot;

    fn empty_snapshot() -> StackSnapshot {
        StackSnapshot {
            reachable: true,
            types: Vec::new(),
        }
    }

    #[test]
    fn merge_states_preserves_entry_local_memory_alias_across_join() {
        let first = DecodedInstr {
            old_start: 0,
            op: vm::op_end as Op,
            operands: Vec::new(),
            stack_before: empty_snapshot(),
            stack_after: empty_snapshot(),
        };
        let value_origin_lhs = ExprOrigin {
            block_id: 10,
            ordinal: 1,
            kind: ExprOriginKind::MemoryValue,
        };
        let value_origin_rhs = ExprOrigin {
            block_id: 11,
            ordinal: 1,
            kind: ExprOriginKind::MemoryValue,
        };
        let key_lhs = AliasKey {
            space: AliasSpace::Memory,
            index: 0,
            width: 4,
            address: AliasAddress::Origin(ExprOrigin {
                block_id: 10,
                ordinal: 0,
                kind: ExprOriginKind::EntryLocal,
            }),
        };
        let key_rhs = AliasKey {
            space: AliasSpace::Memory,
            index: 0,
            width: 4,
            address: AliasAddress::Origin(ExprOrigin {
                block_id: 11,
                ordinal: 0,
                kind: ExprOriginKind::EntryLocal,
            }),
        };
        let mut lhs = BlockEntryState {
            reachable: true,
            heap: HeapVersion {
                memory: 1,
                global: 0,
                table: 0,
            },
            ..BlockEntryState::default()
        };
        lhs.aliases.insert(
            key_lhs,
            AbstractValue {
                ty: ValType::I32,
                origin: value_origin_lhs,
                block_param: None,
                const_value: Some(ConstValue::I32(42)),
                key: None,
            },
        );
        let mut rhs = BlockEntryState {
            reachable: true,
            heap: HeapVersion {
                memory: 1,
                global: 0,
                table: 0,
            },
            ..BlockEntryState::default()
        };
        rhs.aliases.insert(
            key_rhs,
            AbstractValue {
                ty: ValType::I32,
                origin: value_origin_rhs,
                block_param: None,
                const_value: Some(ConstValue::I32(42)),
                key: None,
            },
        );

        let merged = merge_states(7, &first, &[lhs, rhs]);
        let merged_key = AliasKey {
            space: AliasSpace::Memory,
            index: 0,
            width: 4,
            address: AliasAddress::Origin(ExprOrigin {
                block_id: 7,
                ordinal: 0,
                kind: ExprOriginKind::EntryLocal,
            }),
        };
        let merged_value = merged
            .aliases
            .get(&merged_key)
            .expect("entry-local alias should survive the join");
        assert_eq!(merged_value.const_value, Some(ConstValue::I32(42)));
    }
}
