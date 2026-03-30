use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::hash::{Hash, Hasher};

use crate::{common::ValType, runtime::vm};

use super::{
    cfg::BasicBlockProgram,
    expr::{
        AddressBaseKind, AddressShape, AliasKey, AliasSpace, ConstValue, ExprOriginKind, LocalSlot,
        LoopValueShape, SlotClass, SlotShape, ValueGraph, ValueRef,
    },
    pass::{
        BlockBody, BlockEntryState, BlockOpKind, BlockOperand, BlockTerminatorKind,
        UNKNOWN_HEAP_VERSION,
    },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum BlockVersionKind {
    Generic,
    Specialized,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct VersionedBlockId {
    pub(super) original_block: usize,
    pub(super) kind: BlockVersionKind,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum EntryBindingId {
    Stack(usize),
    Local(LocalSlot),
    Alias(AliasKey),
}

#[derive(Clone, Debug)]
struct EntryBinding {
    id: EntryBindingId,
    ty: ValType,
    block_value: ValueRef,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum VersionFact {
    Const(EntryBindingId, ConstValue),
    Slot(EntryBindingId, SlotShape),
    Loop(EntryBindingId, LoopValueShape),
    Address(EntryBindingId, AddressShape),
    Alias(EntryBindingId, AliasKey),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct VersionKey {
    facts: Vec<VersionFact>,
    canonical: String,
    score: u32,
}

#[derive(Clone)]
struct SelectedBindingFacts {
    block_value: ValueRef,
    ty: ValType,
    const_value: Option<ConstValue>,
    slot_shape: Option<SlotShape>,
    loop_value_shape: Option<LoopValueShape>,
    address_shape: Option<AddressShape>,
}

#[derive(Clone)]
struct VersionCandidate {
    pred: usize,
    key: VersionKey,
    bindings: Vec<SelectedBindingFacts>,
}

#[derive(Clone)]
struct SelectedVersion {
    key: VersionKey,
    bindings: Vec<SelectedBindingFacts>,
}

#[derive(Clone, Default)]
struct VersionSelectionPlan {
    selected: Vec<Option<SelectedVersion>>,
    versionable_candidate_blocks: usize,
    blocks_with_entry_bindings: usize,
    blocks_with_version_candidates: usize,
}

#[derive(Clone)]
pub(super) struct VersionedBlock {
    pub(super) id: usize,
    pub(super) version: VersionedBlockId,
    pub(super) body: BlockBody,
    pub(super) block_label: usize,
    pub(super) fallthrough: Option<usize>,
    pub(super) successors: Vec<usize>,
}

#[derive(Clone, Default)]
pub(super) struct VersionedRewriteOverlay {
    pub(super) blocks: Vec<VersionedBlock>,
    pub(super) reachable: Vec<bool>,
    pub(super) specialized_block_count: usize,
    pub(super) generic_fallback_edges: usize,
    pub(super) version_key_fact_breakdown: BTreeMap<&'static str, usize>,
    pub(super) versionable_candidate_blocks: usize,
    pub(super) blocks_with_entry_bindings: usize,
    pub(super) blocks_with_version_candidates: usize,
}

const LARGE_FUNCTION_SCALAR_ONLY_VERSIONING_BLOCK_LIMIT: usize = 8;

pub(super) fn build_versioned_overlay(
    program: &BasicBlockProgram,
    entries: &[BlockEntryState],
    exits: &[BlockEntryState],
    graph: &mut ValueGraph,
    base_bodies: &[BlockBody],
) -> VersionedRewriteOverlay {
    let selection = build_selection_plan(program, entries, exits, graph, base_bodies);
    let selected_block_count = selection.selected.iter().flatten().count();
    if selected_block_count == 0 {
        let blocks = build_generic_blocks(program, base_bodies);
        let reachable = compute_overlay_reachability(&blocks);
        return VersionedRewriteOverlay {
            blocks,
            reachable,
            specialized_block_count: 0,
            generic_fallback_edges: 0,
            version_key_fact_breakdown: BTreeMap::new(),
            versionable_candidate_blocks: selection.versionable_candidate_blocks,
            blocks_with_entry_bindings: selection.blocks_with_entry_bindings,
            blocks_with_version_candidates: selection.blocks_with_version_candidates,
        };
    }

    let mut blocks = Vec::with_capacity(program.blocks.len() + selected_block_count);
    let mut generic_indices = vec![0usize; program.blocks.len()];
    let mut specialized_indices = vec![None; program.blocks.len()];
    let mut fact_breakdown = BTreeMap::new();
    let mut generic_fallback_edges = 0usize;

    for block in &program.blocks {
        let generic_idx = blocks.len();
        generic_indices[block.id] = generic_idx;
        blocks.push(VersionedBlock {
            id: generic_idx,
            version: VersionedBlockId {
                original_block: block.id,
                kind: BlockVersionKind::Generic,
            },
            body: base_bodies[block.id].clone(),
            block_label: block_label_for(program, block.id, generic_idx, BlockVersionKind::Generic),
            fallthrough: None,
            successors: Vec::new(),
        });
    }

    for block in &program.blocks {
        let Some(selected) = selection.selected[block.id].as_ref() else {
            continue;
        };
        let specialized_idx = blocks.len();
        specialized_indices[block.id] = Some(specialized_idx);
        for fact in &selected.key.facts {
            *fact_breakdown.entry(fact_kind_label(fact)).or_insert(0) += 1;
        }
        blocks.push(VersionedBlock {
            id: specialized_idx,
            version: VersionedBlockId {
                original_block: block.id,
                kind: BlockVersionKind::Specialized,
            },
            body: build_specialized_body(
                graph,
                block.id,
                &base_bodies[block.id],
                &selected.bindings,
            ),
            block_label: block_label_for(
                program,
                block.id,
                specialized_idx,
                BlockVersionKind::Specialized,
            ),
            fallthrough: None,
            successors: Vec::new(),
        });
    }

    let mut edge_target_indices = HashMap::new();
    let mut edge_target_labels = HashMap::new();
    for block in &blocks {
        let original_block = block.version.original_block;
        for succ in
            rewritten_successor_blocks(program, original_block, &base_bodies[original_block])
        {
            let (target, used_generic_fallback) = match block.version.kind {
                BlockVersionKind::Specialized => (generic_indices[succ], false),
                BlockVersionKind::Generic => select_generic_successor_target(
                    entries,
                    exits,
                    graph,
                    &selection,
                    &base_bodies[succ],
                    generic_indices[succ],
                    specialized_indices[succ],
                    original_block,
                    succ,
                ),
            };
            if used_generic_fallback {
                generic_fallback_edges += 1;
            }
            edge_target_indices.insert((block.id, succ), target);
            edge_target_labels.insert((block.id, succ), blocks[target].block_label);
        }
    }

    for versioned in 0..blocks.len() {
        let original_block = blocks[versioned].version.original_block;
        blocks[versioned].fallthrough =
            fallthrough_successor_block(program, original_block, &base_bodies[original_block])
                .and_then(|succ| edge_target_indices.get(&(versioned, succ)).copied());
        let mut successors = Vec::new();
        for succ in
            rewritten_successor_blocks(program, original_block, &base_bodies[original_block])
        {
            let target = edge_target_indices
                .get(&(versioned, succ))
                .copied()
                .unwrap_or_else(|| generic_indices[succ]);
            successors.push(target);
        }
        successors.sort_unstable();
        successors.dedup();
        blocks[versioned].successors = successors;
    }

    for block in &mut blocks {
        if block.version.kind == BlockVersionKind::Specialized {
            retag_specialized_source_starts(block);
        }
        rewrite_jump_targets(
            program,
            block.id,
            block.version.original_block,
            &mut block.body,
            &base_bodies[block.version.original_block],
            &edge_target_labels,
        );
    }

    let reachable = compute_overlay_reachability(&blocks);
    let specialized_block_count = blocks
        .iter()
        .filter(|block| block.version.kind == BlockVersionKind::Specialized)
        .count();
    VersionedRewriteOverlay {
        blocks,
        reachable,
        specialized_block_count,
        generic_fallback_edges,
        version_key_fact_breakdown: fact_breakdown,
        versionable_candidate_blocks: selection.versionable_candidate_blocks,
        blocks_with_entry_bindings: selection.blocks_with_entry_bindings,
        blocks_with_version_candidates: selection.blocks_with_version_candidates,
    }
}

fn select_generic_successor_target(
    entries: &[BlockEntryState],
    exits: &[BlockEntryState],
    graph: &ValueGraph,
    selection: &VersionSelectionPlan,
    succ_body: &BlockBody,
    generic_target: usize,
    specialized_target: Option<usize>,
    pred_block: usize,
    succ_block: usize,
) -> (usize, bool) {
    let Some(selected) = selection.selected[succ_block].as_ref() else {
        return (generic_target, false);
    };
    let Some(specialized_target) = specialized_target else {
        return (generic_target, false);
    };
    let bindings = collect_entry_bindings(entries, graph, succ_block, succ_body);
    let edge_key = build_candidate_for_pred(exits, graph, pred_block, &bindings)
        .map(|candidate| candidate.key);
    if edge_key.as_ref() == Some(&selected.key) {
        (specialized_target, false)
    } else {
        (generic_target, true)
    }
}

fn build_generic_blocks(
    program: &BasicBlockProgram,
    base_bodies: &[BlockBody],
) -> Vec<VersionedBlock> {
    program
        .blocks
        .iter()
        .map(|block| VersionedBlock {
            id: block.id,
            version: VersionedBlockId {
                original_block: block.id,
                kind: BlockVersionKind::Generic,
            },
            body: base_bodies[block.id].clone(),
            block_label: block_label_for(program, block.id, block.id, BlockVersionKind::Generic),
            fallthrough: fallthrough_successor_block(program, block.id, &base_bodies[block.id]),
            successors: rewritten_successor_blocks(program, block.id, &base_bodies[block.id]),
        })
        .collect()
}

fn build_selection_plan(
    program: &BasicBlockProgram,
    entries: &[BlockEntryState],
    exits: &[BlockEntryState],
    graph: &ValueGraph,
    base_bodies: &[BlockBody],
) -> VersionSelectionPlan {
    let mut plan = VersionSelectionPlan {
        selected: vec![None; program.blocks.len()],
        versionable_candidate_blocks: 0,
        blocks_with_entry_bindings: 0,
        blocks_with_version_candidates: 0,
    };
    for block in &program.blocks {
        if !is_versionable_candidate(program, base_bodies, graph, block.id) {
            continue;
        }
        plan.versionable_candidate_blocks += 1;
        let bindings = collect_entry_bindings(entries, graph, block.id, &base_bodies[block.id]);
        if bindings.is_empty() {
            continue;
        }
        plan.blocks_with_entry_bindings += 1;
        let mut candidates = program.predecessors[block.id]
            .iter()
            .copied()
            .filter(|pred| exits[*pred].reachable)
            .filter_map(|pred| build_candidate_for_pred(exits, graph, pred, &bindings))
            .collect::<Vec<_>>();
        if candidates.is_empty() {
            continue;
        }
        plan.blocks_with_version_candidates += 1;
        candidates.sort_by(|lhs, rhs| {
            rhs.key
                .score
                .cmp(&lhs.key.score)
                .then_with(|| lhs.key.canonical.cmp(&rhs.key.canonical))
                .then_with(|| lhs.pred.cmp(&rhs.pred))
        });
        let Some(selected) = candidates
            .into_iter()
            .find(|candidate| allows_selected_version(program, &candidate.key))
        else {
            continue;
        };
        plan.selected[block.id] = Some(SelectedVersion {
            key: selected.key,
            bindings: selected.bindings,
        });
    }
    plan
}

fn is_versionable_candidate(
    program: &BasicBlockProgram,
    base_bodies: &[BlockBody],
    graph: &ValueGraph,
    block_id: usize,
) -> bool {
    let is_join = program.predecessors[block_id].len() >= 2;
    let is_loop_header = is_reducible_loop_header(program, block_id);
    (is_join || is_loop_header)
        && body_has_specializable_consumer(&base_bodies[block_id])
        && body_consumes_entry_binding(graph, &base_bodies[block_id], block_id)
}

fn is_reducible_loop_header(program: &BasicBlockProgram, block_id: usize) -> bool {
    let Some(block) = program.blocks.get(block_id) else {
        return false;
    };
    program
        .records
        .get(block.start)
        .is_some_and(|record| record.op_eq(vm::op_loop))
        && program.predecessors[block_id]
            .iter()
            .any(|pred| *pred >= block_id)
}

fn body_has_specializable_consumer(body: &BlockBody) -> bool {
    body.ops.iter().any(|op| {
        matches!(
            op.kind,
            BlockOpKind::MemoryLoad
                | BlockOpKind::MemoryStore
                | BlockOpKind::CallLike
                | BlockOpKind::TableGet
                | BlockOpKind::LocalSet
                | BlockOpKind::LocalTee
                | BlockOpKind::Select
                | BlockOpKind::PureUnary(_)
                | BlockOpKind::PureBinary(_)
        )
    }) || body.terminator.as_ref().is_some_and(|term| {
        matches!(
            term.kind,
            BlockTerminatorKind::If | BlockTerminatorKind::BrIf | BlockTerminatorKind::BrTable
        )
    })
}

fn body_consumes_entry_binding(graph: &ValueGraph, body: &BlockBody, block_id: usize) -> bool {
    collect_body_values(body).into_iter().any(|value| {
        let origin = graph[value.0].origin;
        (origin.kind == ExprOriginKind::BlockArgument && origin.block_id == block_id)
            || !referenced_entry_local_slots(&graph[value.0]).is_empty()
    })
}

fn collect_entry_bindings(
    entries: &[BlockEntryState],
    graph: &ValueGraph,
    block_id: usize,
    body: &BlockBody,
) -> Vec<EntryBinding> {
    let Some(entry) = entries.get(block_id) else {
        return Vec::new();
    };
    if !entry.reachable {
        return Vec::new();
    }
    let mut binding_by_value: HashMap<ValueRef, EntryBinding> = HashMap::new();
    for (ordinal, value) in entry.stack.iter().copied().enumerate() {
        binding_by_value.insert(
            value,
            EntryBinding {
                id: EntryBindingId::Stack(ordinal),
                ty: graph[value.0].ty,
                block_value: value,
            },
        );
    }
    for (slot, value) in &entry.locals {
        binding_by_value.insert(
            *value,
            EntryBinding {
                id: EntryBindingId::Local(*slot),
                ty: graph[value.0].ty,
                block_value: *value,
            },
        );
    }
    for (alias, value) in &entry.aliases {
        let alias = alias.clone();
        binding_by_value.insert(
            *value,
            EntryBinding {
                id: EntryBindingId::Alias(alias),
                ty: graph[value.0].ty,
                block_value: *value,
            },
        );
    }

    let mut ordered = Vec::new();
    let mut seen = BTreeSet::new();
    let mut binding_by_local = HashMap::new();
    let mut binding_by_block_argument_ordinal = HashMap::new();
    for binding in binding_by_value.values() {
        match binding.id {
            EntryBindingId::Stack(ordinal) => {
                binding_by_block_argument_ordinal.insert(ordinal, binding.clone());
            }
            EntryBindingId::Local(slot) => {
                binding_by_local.insert(slot, binding.clone());
                binding_by_block_argument_ordinal
                    .insert(local_binding_ordinal(slot), binding.clone());
            }
            EntryBindingId::Alias(ref alias) => {
                binding_by_block_argument_ordinal
                    .insert(alias_binding_ordinal(alias), binding.clone());
            }
        }
    }
    for value in collect_body_values(body) {
        let origin = graph[value.0].origin;
        if origin.kind == ExprOriginKind::BlockArgument && origin.block_id == block_id {
            let Some(binding) = binding_by_block_argument_ordinal
                .get(&origin.ordinal)
                .cloned()
            else {
                continue;
            };
            if seen.insert(binding.id.clone()) {
                ordered.push(binding);
            }
            continue;
        }

        for slot in referenced_entry_local_slots(&graph[value.0]) {
            let Some(binding) = binding_by_local.get(&slot).cloned() else {
                continue;
            };
            if seen.insert(binding.id.clone()) {
                ordered.push(binding);
            }
        }
    }
    ordered
}

fn local_binding_ordinal(slot: LocalSlot) -> usize {
    1024 + slot.addr as usize
}

fn alias_binding_ordinal(key: &AliasKey) -> usize {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    key.hash(&mut hasher);
    hasher.finish() as usize
}

fn referenced_entry_local_slots(
    node: &crate::parser::core::optimizer::expr::ValueNode,
) -> Vec<LocalSlot> {
    let mut slots = Vec::new();
    if let Some(slot) = node
        .slot_shape
        .as_ref()
        .and_then(|shape| shape.slot)
        .and_then(entry_local_slot_ref)
    {
        slots.push(slot);
    }
    if let Some(shape) = node.address_shape {
        collect_entry_local_slots_from_address_shape(shape, &mut slots);
    }
    if let Some(shape) = node.loop_value_shape.as_ref() {
        collect_entry_local_slots_from_loop_shape(shape, &mut slots);
    }
    slots.sort_unstable_by_key(|slot| (slot.addr, slot.size));
    slots.dedup();
    slots
}

fn entry_local_slot_ref(slot: crate::parser::core::optimizer::expr::SlotRef) -> Option<LocalSlot> {
    (slot.class == SlotClass::EntryLocal).then_some(slot.slot)
}

fn collect_entry_local_slots_from_address_shape(shape: AddressShape, slots: &mut Vec<LocalSlot>) {
    if let Some(slot) = entry_local_address_base(shape.base) {
        slots.push(slot);
    }
    if let Some(slot) = shape.index.and_then(entry_local_address_base) {
        slots.push(slot);
    }
}

fn entry_local_address_base(base: AddressBaseKind) -> Option<LocalSlot> {
    match base {
        AddressBaseKind::EntryLocal(slot) => Some(slot),
        AddressBaseKind::TempLocal(_) | AddressBaseKind::SpillLocal(_) => None,
    }
}

fn collect_entry_local_slots_from_loop_shape(shape: &LoopValueShape, slots: &mut Vec<LocalSlot>) {
    match shape {
        LoopValueShape::Local4(slot) => slots.push(*slot),
        LoopValueShape::Local4ConstAdd { base, .. } => slots.push(*base),
        LoopValueShape::Local4Local4Add { lhs, rhs } => {
            slots.push(*lhs);
            slots.push(*rhs);
        }
        LoopValueShape::CompareEqz { input } => {
            collect_entry_local_slots_from_loop_shape(input, slots);
        }
        LoopValueShape::CompareConstI32 { lhs, .. } => {
            collect_entry_local_slots_from_loop_shape(lhs, slots);
        }
        LoopValueShape::CompareLocal4 { lhs, rhs, .. } => {
            slots.push(*lhs);
            slots.push(*rhs);
        }
    }
}

fn collect_body_values(body: &BlockBody) -> Vec<ValueRef> {
    let mut values = Vec::new();
    for op in &body.ops {
        values.extend(op.inputs.iter().copied());
        values.extend(op.operands.iter().filter_map(spill_value_operand));
    }
    if let Some(terminator) = &body.terminator {
        values.extend(terminator.inputs.iter().copied());
        values.extend(terminator.operands.iter().filter_map(spill_value_operand));
    }
    values
}

fn spill_value_operand(operand: &BlockOperand) -> Option<ValueRef> {
    match operand {
        BlockOperand::SpillValue(value) => Some(*value),
        _ => None,
    }
}

fn build_candidate_for_pred(
    exits: &[BlockEntryState],
    graph: &ValueGraph,
    pred: usize,
    bindings: &[EntryBinding],
) -> Option<VersionCandidate> {
    let state = exits.get(pred)?;
    let mut facts = Vec::new();
    let mut selected_bindings = Vec::new();
    for binding in bindings {
        let value = match &binding.id {
            EntryBindingId::Stack(ordinal) => state.stack.get(*ordinal).copied()?,
            EntryBindingId::Local(slot) => *state.locals.get(slot)?,
            EntryBindingId::Alias(alias) => *state.aliases.get(alias)?,
        };
        let node = &graph[value.0];
        let const_value = node.const_value;
        let slot_shape = stable_slot_shape(node.slot_shape.clone())
            .or_else(|| fallback_binding_slot_shape(binding));
        let loop_value_shape = node.loop_value_shape.clone();
        let address_shape = stable_address_shape(node.address_shape);
        if let Some(const_value) = const_value {
            facts.push(VersionFact::Const(binding.id.clone(), const_value));
        }
        if let Some(slot_shape) = slot_shape.clone() {
            facts.push(VersionFact::Slot(binding.id.clone(), slot_shape));
        }
        if let Some(loop_value_shape) = loop_value_shape.clone() {
            facts.push(VersionFact::Loop(binding.id.clone(), loop_value_shape));
        }
        if let Some(address_shape) = address_shape {
            facts.push(VersionFact::Address(binding.id.clone(), address_shape));
        }
        if let EntryBindingId::Alias(alias) = &binding.id {
            if alias_space_is_stable(alias.space, state) {
                facts.push(VersionFact::Alias(binding.id.clone(), alias.clone()));
            }
        }
        if const_value.is_some()
            || slot_shape.is_some()
            || loop_value_shape.is_some()
            || address_shape.is_some()
        {
            selected_bindings.push(SelectedBindingFacts {
                block_value: binding.block_value,
                ty: binding.ty,
                const_value,
                slot_shape,
                loop_value_shape,
                address_shape,
            });
        }
    }
    if facts.is_empty() {
        return None;
    }
    facts.sort_by_key(version_fact_sort_key);
    facts.dedup();
    let score = facts.iter().map(fact_score).sum();
    let canonical = facts
        .iter()
        .map(version_fact_sort_key)
        .collect::<Vec<_>>()
        .join("|");
    Some(VersionCandidate {
        pred,
        key: VersionKey {
            facts,
            canonical,
            score,
        },
        bindings: selected_bindings,
    })
}

fn fallback_binding_slot_shape(binding: &EntryBinding) -> Option<SlotShape> {
    match binding.id {
        EntryBindingId::Local(slot) => Some(SlotShape {
            slot: Some(crate::parser::core::optimizer::expr::SlotRef::entry_local(
                slot,
            )),
            address: None,
            loop_value: None,
        }),
        EntryBindingId::Stack(_) | EntryBindingId::Alias(_) => None,
    }
}

fn stable_slot_shape(shape: Option<SlotShape>) -> Option<SlotShape> {
    let shape = shape?;
    match shape.slot {
        Some(slot)
            if matches!(
                slot.class,
                SlotClass::TempLocal | SlotClass::SpillLocal | SlotClass::VirtualStack
            ) =>
        {
            None
        }
        _ => Some(shape),
    }
}

fn stable_address_shape(shape: Option<AddressShape>) -> Option<AddressShape> {
    let shape = shape?;
    if !stable_address_base(shape.base) {
        return None;
    }
    if let Some(index) = shape.index {
        if !stable_address_base(index) {
            return None;
        }
    }
    Some(shape)
}

fn stable_address_base(base: AddressBaseKind) -> bool {
    matches!(base, AddressBaseKind::EntryLocal(_))
}

fn alias_space_is_stable(space: AliasSpace, state: &BlockEntryState) -> bool {
    match space {
        AliasSpace::Memory => state.heap.memory != UNKNOWN_HEAP_VERSION,
        AliasSpace::Global => state.heap.global != UNKNOWN_HEAP_VERSION,
        AliasSpace::Table => state.heap.table != UNKNOWN_HEAP_VERSION,
    }
}

fn fact_score(fact: &VersionFact) -> u32 {
    match fact {
        VersionFact::Alias(EntryBindingId::Alias(alias), _) if alias.space == AliasSpace::Table => {
            4
        }
        VersionFact::Alias(_, alias) if alias.space == AliasSpace::Table => 4,
        VersionFact::Alias(_, _) | VersionFact::Address(_, _) => 3,
        VersionFact::Loop(_, _) => 2,
        VersionFact::Const(_, _) | VersionFact::Slot(_, _) => 1,
    }
}

fn fact_kind_label(fact: &VersionFact) -> &'static str {
    match fact {
        VersionFact::Const(_, _) => "const",
        VersionFact::Slot(_, _) => "slot",
        VersionFact::Loop(_, _) => "loop",
        VersionFact::Address(_, _) => "address",
        VersionFact::Alias(_, _) => "alias",
    }
}

fn allows_selected_version(program: &BasicBlockProgram, key: &VersionKey) -> bool {
    program.blocks.len() <= LARGE_FUNCTION_SCALAR_ONLY_VERSIONING_BLOCK_LIMIT
        || !version_key_is_scalar_only(key)
}

fn version_key_is_scalar_only(key: &VersionKey) -> bool {
    !key.facts.is_empty()
        && key
            .facts
            .iter()
            .all(|fact| matches!(fact, VersionFact::Const(_, _) | VersionFact::Slot(_, _)))
}

fn version_fact_sort_key(fact: &VersionFact) -> String {
    match fact {
        VersionFact::Const(binding, value) => {
            format!("const:{}:{value:?}", binding_sort_key(binding))
        }
        VersionFact::Slot(binding, shape) => {
            format!("slot:{}:{shape:?}", binding_sort_key(binding))
        }
        VersionFact::Loop(binding, shape) => {
            format!("loop:{}:{shape:?}", binding_sort_key(binding))
        }
        VersionFact::Address(binding, shape) => {
            format!("address:{}:{shape:?}", binding_sort_key(binding))
        }
        VersionFact::Alias(binding, alias) => {
            format!("alias:{}:{alias:?}", binding_sort_key(binding))
        }
    }
}

fn binding_sort_key(binding: &EntryBindingId) -> String {
    match binding {
        EntryBindingId::Stack(ordinal) => format!("stack:{ordinal}"),
        EntryBindingId::Local(slot) => format!("local:{}:{}", slot.addr, slot.size),
        EntryBindingId::Alias(alias) => format!("alias:{alias:?}"),
    }
}

fn build_specialized_body(
    graph: &mut ValueGraph,
    block_id: usize,
    base_body: &BlockBody,
    bindings: &[SelectedBindingFacts],
) -> BlockBody {
    let mut body = base_body.clone();
    let replacements = bindings
        .iter()
        .enumerate()
        .map(|(ordinal, binding)| {
            (
                binding.block_value,
                graph.push_synthetic_specialized_value(
                    block_id,
                    usize::MAX.saturating_sub(ordinal + 1),
                    binding.ty,
                    binding.const_value,
                    binding.address_shape,
                    binding.loop_value_shape.clone(),
                    binding.slot_shape.clone(),
                ),
            )
        })
        .collect::<HashMap<_, _>>();

    rewrite_body_binding_inputs(&mut body, &replacements);
    body
}

fn rewrite_body_binding_inputs(body: &mut BlockBody, replacements: &HashMap<ValueRef, ValueRef>) {
    for op in &mut body.ops {
        for input in &mut op.inputs {
            if let Some(replacement) = replacements.get(input) {
                *input = *replacement;
            }
        }
        for operand in &mut op.operands {
            if let BlockOperand::SpillValue(value) = *operand {
                if let Some(replacement) = replacements.get(&value) {
                    *operand = BlockOperand::SpillValue(*replacement);
                }
            }
        }
    }
    if let Some(terminator) = &mut body.terminator {
        for input in &mut terminator.inputs {
            if let Some(replacement) = replacements.get(input) {
                *input = *replacement;
            }
        }
        for operand in &mut terminator.operands {
            if let BlockOperand::SpillValue(value) = *operand {
                if let Some(replacement) = replacements.get(&value) {
                    *operand = BlockOperand::SpillValue(*replacement);
                }
            }
        }
    }
}

fn retag_specialized_source_starts(block: &mut VersionedBlock) {
    for op in &mut block.body.ops {
        if op.source_start.is_some() {
            op.source_start = Some(block.block_label);
        }
    }
    if let Some(terminator) = &mut block.body.terminator {
        if terminator.source_start.is_some() {
            terminator.source_start = Some(block.block_label);
        }
    }
}

fn rewrite_jump_targets(
    program: &BasicBlockProgram,
    source_block: usize,
    original_block: usize,
    body: &mut BlockBody,
    original_body: &BlockBody,
    edge_target_labels: &HashMap<(usize, usize), usize>,
) {
    let mut target_labels = HashMap::new();
    for succ in rewritten_successor_blocks(program, original_block, original_body) {
        let Some(label) = edge_target_labels.get(&(source_block, succ)).copied() else {
            continue;
        };
        let block_start = program.records[program.blocks[succ].start].old_start;
        target_labels.insert(block_start, label);
    }
    rewrite_body_jump_targets(body, &target_labels);
}

fn rewrite_body_jump_targets(body: &mut BlockBody, target_labels: &HashMap<usize, usize>) {
    for op in &mut body.ops {
        rewrite_jump_operands(&mut op.operands, target_labels);
    }
    if let Some(terminator) = &mut body.terminator {
        rewrite_jump_operands(&mut terminator.operands, target_labels);
    }
}

fn rewrite_jump_operands(operands: &mut [BlockOperand], target_labels: &HashMap<usize, usize>) {
    for operand in operands {
        if let BlockOperand::JumpTarget(target) = operand {
            if let Some(rewritten) = target_labels.get(target).copied() {
                *operand = BlockOperand::JumpTarget(rewritten);
            }
        }
    }
}

fn rewritten_successor_blocks(
    program: &BasicBlockProgram,
    block_id: usize,
    body: &BlockBody,
) -> Vec<usize> {
    let fallthrough = program.next_block_id(block_id);
    let Some(last) = &body.terminator else {
        return fallthrough.into_iter().collect();
    };
    match last.kind {
        BlockTerminatorKind::Br | BlockTerminatorKind::Else | BlockTerminatorKind::Return => {
            single_target_block(program, last).into_iter().collect()
        }
        BlockTerminatorKind::BrIf | BlockTerminatorKind::If => {
            let mut succs = Vec::new();
            if let Some(target) = single_target_block(program, last) {
                succs.push(target);
            }
            if let Some(next) = fallthrough {
                succs.push(next);
            }
            succs.sort_unstable();
            succs.dedup();
            succs
        }
        BlockTerminatorKind::BrTable => table_target_blocks(program, last),
        BlockTerminatorKind::SpecialFunctionReturn | BlockTerminatorKind::Unreachable => Vec::new(),
        BlockTerminatorKind::SpecialBlockReturn => fallthrough.into_iter().collect(),
        _ => fallthrough.into_iter().collect(),
    }
}

fn fallthrough_successor_block(
    program: &BasicBlockProgram,
    block_id: usize,
    body: &BlockBody,
) -> Option<usize> {
    let fallthrough = program.next_block_id(block_id)?;
    match body.terminator.as_ref().map(|terminator| terminator.kind) {
        Some(
            BlockTerminatorKind::Br
            | BlockTerminatorKind::Else
            | BlockTerminatorKind::BrTable
            | BlockTerminatorKind::Return
            | BlockTerminatorKind::SpecialFunctionReturn
            | BlockTerminatorKind::Unreachable,
        ) => None,
        _ => Some(fallthrough),
    }
}

fn single_target_block(
    program: &BasicBlockProgram,
    terminator: &super::pass::BlockTerminator,
) -> Option<usize> {
    let BlockOperand::JumpTarget(target) = *terminator.operands.first()? else {
        return None;
    };
    program.block_for_old_start(target)
}

fn table_target_blocks(
    program: &BasicBlockProgram,
    terminator: &super::pass::BlockTerminator,
) -> Vec<usize> {
    let Some(BlockOperand::U32(table_len)) = terminator.operands.first() else {
        return Vec::new();
    };
    let table_len = *table_len as usize;
    (1..=table_len + 1)
        .filter_map(|idx| {
            let BlockOperand::JumpTarget(target) = terminator.operands[idx] else {
                return None;
            };
            program.block_for_old_start(target)
        })
        .collect()
}

fn compute_overlay_reachability(blocks: &[VersionedBlock]) -> Vec<bool> {
    if blocks.is_empty() {
        return Vec::new();
    }
    let mut reachable = vec![false; blocks.len()];
    let mut worklist = VecDeque::from([0usize]);
    reachable[0] = true;
    while let Some(block_id) = worklist.pop_front() {
        for succ in &blocks[block_id].successors {
            if !reachable[*succ] {
                reachable[*succ] = true;
                worklist.push_back(*succ);
            }
        }
    }
    reachable
}

fn block_label_for(
    program: &BasicBlockProgram,
    original_block: usize,
    ordinal: usize,
    kind: BlockVersionKind,
) -> usize {
    let base = program
        .records
        .last()
        .map(|record| record.old_start)
        .unwrap_or(0)
        .saturating_add(1);
    match kind {
        BlockVersionKind::Generic => {
            program.records[program.blocks[original_block].start].old_start
        }
        BlockVersionKind::Specialized => base.saturating_add(ordinal),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        common::{MemArg, Op, Operand, ValType},
        parser::core::{
            optimizer::{
                cfg::{BasicBlock, BasicBlockProgram, DecodedInstr},
                expr::{HeapVersion, PureOpKind},
            },
            type_checker::StackSnapshot,
        },
        runtime::vm,
    };
    use std::collections::HashMap;

    fn empty_snapshot() -> StackSnapshot {
        StackSnapshot {
            reachable: true,
            types: Vec::new(),
        }
    }

    fn decoded_branch(old_start: usize, target: usize) -> DecodedInstr {
        DecodedInstr {
            old_start,
            op: vm::op_br as Op,
            operands: vec![Operand {
                jump_addr: target as u32,
            }],
            stack_before: empty_snapshot(),
            stack_after: empty_snapshot(),
            preserved_prefix_len: 0,
            fresh_result_count: 0,
        }
    }

    fn decoded_end(old_start: usize) -> DecodedInstr {
        DecodedInstr {
            old_start,
            op: vm::op_end as Op,
            operands: Vec::new(),
            stack_before: empty_snapshot(),
            stack_after: empty_snapshot(),
            preserved_prefix_len: 0,
            fresh_result_count: 0,
        }
    }

    fn decoded_loop(old_start: usize) -> DecodedInstr {
        DecodedInstr {
            old_start,
            op: vm::op_loop as Op,
            operands: Vec::new(),
            stack_before: empty_snapshot(),
            stack_after: empty_snapshot(),
            preserved_prefix_len: 0,
            fresh_result_count: 0,
        }
    }

    fn join_program(pred_starts: &[usize], join_start: usize) -> BasicBlockProgram {
        let mut records = pred_starts
            .iter()
            .map(|start| decoded_branch(*start, join_start))
            .collect::<Vec<_>>();
        records.push(decoded_end(join_start));

        let mut blocks = pred_starts
            .iter()
            .enumerate()
            .map(|(id, _)| BasicBlock {
                id,
                start: id,
                end: id + 1,
            })
            .collect::<Vec<_>>();
        let join_block = BasicBlock {
            id: pred_starts.len(),
            start: pred_starts.len(),
            end: pred_starts.len() + 1,
        };
        blocks.push(join_block);

        let mut old_start_to_block = pred_starts
            .iter()
            .enumerate()
            .map(|(id, start)| (*start, id))
            .collect::<HashMap<_, _>>();
        old_start_to_block.insert(join_start, join_block.id);

        let mut successors = vec![Vec::new(); blocks.len()];
        let mut predecessors = vec![Vec::new(); blocks.len()];
        for pred in 0..pred_starts.len() {
            successors[pred].push(join_block.id);
            predecessors[join_block.id].push(pred);
        }

        BasicBlockProgram {
            records,
            blocks,
            old_start_to_block,
            successors,
            predecessors,
        }
    }

    fn loop_header_program(header_start: usize, latch_start: usize) -> BasicBlockProgram {
        let records = vec![
            decoded_branch(0, header_start),
            decoded_loop(header_start),
            decoded_branch(latch_start, header_start),
        ];
        let blocks = vec![
            BasicBlock {
                id: 0,
                start: 0,
                end: 1,
            },
            BasicBlock {
                id: 1,
                start: 1,
                end: 2,
            },
            BasicBlock {
                id: 2,
                start: 2,
                end: 3,
            },
        ];
        let old_start_to_block =
            HashMap::from([(0usize, 0usize), (header_start, 1), (latch_start, 2)]);
        let successors = vec![vec![1], vec![2], vec![1]];
        let predecessors = vec![Vec::new(), vec![0, 2], vec![1]];
        BasicBlockProgram {
            records,
            blocks,
            old_start_to_block,
            successors,
            predecessors,
        }
    }

    fn branch_body(source_start: usize, target: usize) -> BlockBody {
        BlockBody {
            ops: Vec::new(),
            terminator: Some(crate::parser::core::optimizer::pass::BlockTerminator {
                source_start: Some(source_start),
                op: vm::op_br as Op,
                kind: BlockTerminatorKind::Br,
                operands: vec![BlockOperand::JumpTarget(target)],
                inputs: Vec::new(),
                values: Vec::new(),
            }),
        }
    }

    #[test]
    fn version_key_canonicalizes_fact_order() {
        let key = VersionKey {
            facts: vec![
                VersionFact::Slot(EntryBindingId::Stack(0), SlotShape::default()),
                VersionFact::Const(EntryBindingId::Stack(0), ConstValue::I32(7)),
            ],
            canonical: vec![
                version_fact_sort_key(&VersionFact::Const(
                    EntryBindingId::Stack(0),
                    ConstValue::I32(7),
                )),
                version_fact_sort_key(&VersionFact::Slot(
                    EntryBindingId::Stack(0),
                    SlotShape::default(),
                )),
            ]
            .join("|"),
            score: 2,
        };
        assert!(key.canonical.starts_with("const:"));
        assert!(key.canonical.contains("|slot:"));
    }

    #[test]
    fn stable_address_shape_rejects_temp_local_bases() {
        let shape = AddressShape::base_offset(AddressBaseKind::TempLocal(LocalSlot::new(4, 4)), 0);
        assert!(stable_address_shape(Some(shape)).is_none());
    }

    #[test]
    fn stable_slot_shape_rejects_temp_local_slots() {
        let shape = SlotShape {
            slot: Some(crate::parser::core::optimizer::expr::SlotRef::temp_local(
                LocalSlot::new(8, 4),
            )),
            address: None,
            loop_value: None,
        };
        assert!(stable_slot_shape(Some(shape)).is_none());
    }

    #[test]
    fn rewrite_body_binding_inputs_replaces_matching_block_argument_inputs() {
        let value = crate::parser::core::optimizer::expr::ExprId(0);
        let replacement = crate::parser::core::optimizer::expr::ExprId(1);
        let mut body = BlockBody {
            ops: vec![],
            terminator: Some(crate::parser::core::optimizer::pass::BlockTerminator {
                source_start: Some(0),
                op: crate::runtime::vm::op_br_if,
                kind: BlockTerminatorKind::BrIf,
                operands: vec![],
                inputs: vec![value],
                values: vec![],
            }),
        };
        let replacements = HashMap::from([(value, replacement)]);
        rewrite_body_binding_inputs(&mut body, &replacements);
        assert_eq!(body.terminator.unwrap().inputs, vec![replacement]);
    }

    #[test]
    fn body_consumes_entry_binding_detects_spill_operands() {
        let mut graph = ValueGraph::default();
        let block_arg =
            graph.ensure_block_argument(1, 0, ValType::I32, None, None, None, None, None);
        let body = BlockBody {
            ops: vec![crate::parser::core::optimizer::pass::BlockOp {
                source_start: Some(10),
                op: vm::op_drop as Op,
                kind: BlockOpKind::Drop,
                operands: vec![BlockOperand::SpillValue(block_arg)],
                inputs: vec![],
                values: vec![],
            }],
            terminator: None,
        };
        assert!(body_consumes_entry_binding(&graph, &body, 1));
    }

    #[test]
    fn body_consumes_entry_binding_detects_entry_local_slot_reads() {
        let mut graph = ValueGraph::default();
        let slot = LocalSlot::new(4, 4);
        let local_value = graph.push_synthetic_specialized_value(
            1,
            0,
            ValType::I32,
            None,
            None,
            None,
            Some(SlotShape {
                slot: Some(crate::parser::core::optimizer::expr::SlotRef::entry_local(
                    slot,
                )),
                address: None,
                loop_value: None,
            }),
        );
        let body = BlockBody {
            ops: vec![crate::parser::core::optimizer::pass::BlockOp {
                source_start: Some(10),
                op: vm::op_i32_eqz as Op,
                kind: BlockOpKind::PureUnary(PureOpKind::I32Eqz),
                operands: vec![],
                inputs: vec![local_value],
                values: vec![],
            }],
            terminator: None,
        };
        assert!(body_consumes_entry_binding(&graph, &body, 1));
    }

    #[test]
    fn collect_entry_bindings_includes_spill_operands() {
        let mut graph = ValueGraph::default();
        let block_arg =
            graph.ensure_block_argument(1, 0, ValType::I32, None, None, None, None, None);
        let entry_value =
            graph.push_synthetic_specialized_value(0, 0, ValType::I32, None, None, None, None);
        let mut entries = vec![BlockEntryState::default(); 2];
        entries[1].reachable = true;
        entries[1].stack.push(entry_value);
        let body = BlockBody {
            ops: vec![crate::parser::core::optimizer::pass::BlockOp {
                source_start: Some(10),
                op: vm::op_drop as Op,
                kind: BlockOpKind::Drop,
                operands: vec![BlockOperand::SpillValue(block_arg)],
                inputs: vec![],
                values: vec![],
            }],
            terminator: None,
        };
        let bindings = collect_entry_bindings(&entries, &graph, 1, &body);
        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0].block_value, entry_value);
        assert_eq!(bindings[0].id, EntryBindingId::Stack(0));
    }

    #[test]
    fn collect_entry_bindings_includes_entry_local_slot_backed_values() {
        let mut graph = ValueGraph::default();
        let slot = LocalSlot::new(8, 4);
        let entry_value = graph.push_synthetic_specialized_value(
            0,
            0,
            ValType::I32,
            Some(ConstValue::I32(7)),
            None,
            None,
            Some(SlotShape {
                slot: Some(crate::parser::core::optimizer::expr::SlotRef::entry_local(
                    slot,
                )),
                address: None,
                loop_value: None,
            }),
        );
        let local_read = graph.push_synthetic_specialized_value(
            1,
            0,
            ValType::I32,
            None,
            None,
            None,
            Some(SlotShape {
                slot: Some(crate::parser::core::optimizer::expr::SlotRef::entry_local(
                    slot,
                )),
                address: None,
                loop_value: None,
            }),
        );

        let mut entries = vec![BlockEntryState::default(); 2];
        entries[1].reachable = true;
        entries[1].locals.insert(slot, entry_value);
        let body = BlockBody {
            ops: vec![crate::parser::core::optimizer::pass::BlockOp {
                source_start: Some(10),
                op: vm::op_i32_eqz as Op,
                kind: BlockOpKind::PureUnary(PureOpKind::I32Eqz),
                operands: vec![],
                inputs: vec![local_read],
                values: vec![],
            }],
            terminator: None,
        };
        let bindings = collect_entry_bindings(&entries, &graph, 1, &body);
        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0].block_value, entry_value);
        assert_eq!(bindings[0].id, EntryBindingId::Local(slot));
    }

    #[test]
    fn build_candidate_for_pred_uses_entry_local_slot_when_value_has_no_slot_shape() {
        let mut graph = ValueGraph::default();
        let slot = LocalSlot::new(12, 4);
        let pred_value =
            graph.push_synthetic_specialized_value(0, 0, ValType::I32, None, None, None, None);
        let mut exits = vec![BlockEntryState::default(); 1];
        exits[0].reachable = true;
        exits[0].locals.insert(slot, pred_value);
        let binding = EntryBinding {
            id: EntryBindingId::Local(slot),
            ty: ValType::I32,
            block_value: pred_value,
        };

        let candidate =
            build_candidate_for_pred(&exits, &graph, 0, &[binding]).expect("candidate must exist");
        assert!(candidate.key.facts.iter().any(|fact| {
            matches!(
                fact,
                VersionFact::Slot(EntryBindingId::Local(found), shape)
                    if *found == slot
                        && shape.slot
                            == Some(crate::parser::core::optimizer::expr::SlotRef::entry_local(
                                slot
                            ))
            )
        }));
    }

    #[test]
    fn build_candidate_for_pred_keeps_alias_only_facts_out_of_body_replacements() {
        let mut graph = ValueGraph::default();
        let alias_value =
            graph.push_synthetic_specialized_value(0, 0, ValType::I32, None, None, None, None);
        let alias = AliasKey {
            space: AliasSpace::Memory,
            index: 0,
            offset: 0,
            width: 4,
            address: crate::parser::core::optimizer::expr::AliasAddress::Const(0),
        };
        let mut exits = vec![BlockEntryState::default(); 1];
        exits[0].reachable = true;
        exits[0].heap.memory = 1;
        exits[0].aliases.insert(alias.clone(), alias_value);
        let binding = EntryBinding {
            id: EntryBindingId::Alias(alias.clone()),
            ty: ValType::I32,
            block_value: alias_value,
        };

        let candidate =
            build_candidate_for_pred(&exits, &graph, 0, &[binding]).expect("candidate must exist");
        assert!(candidate.key.facts.iter().any(|fact| {
            matches!(
                fact,
                VersionFact::Alias(EntryBindingId::Alias(found), present)
                    if *found == alias && *present == alias
            )
        }));
        assert!(
            candidate.bindings.is_empty(),
            "alias-only facts must not synthesize body replacements"
        );
    }

    #[test]
    fn build_versioned_overlay_specializes_join_when_consumer_reads_entry_local_slot() {
        let program = join_program(&[0, 10], 20);
        let mut graph = ValueGraph::default();
        let slot = LocalSlot::new(0, 4);
        let eqz_result =
            graph.push_synthetic_specialized_value(2, 1, ValType::I32, None, None, None, None);
        let pred0_value = graph.push_synthetic_specialized_value(
            0,
            0,
            ValType::I32,
            Some(ConstValue::I32(0)),
            None,
            None,
            Some(SlotShape {
                slot: Some(crate::parser::core::optimizer::expr::SlotRef::entry_local(
                    slot,
                )),
                address: None,
                loop_value: None,
            }),
        );
        let pred1_value = graph.push_synthetic_specialized_value(
            1,
            0,
            ValType::I32,
            Some(ConstValue::I32(1)),
            None,
            None,
            Some(SlotShape {
                slot: Some(crate::parser::core::optimizer::expr::SlotRef::entry_local(
                    slot,
                )),
                address: None,
                loop_value: None,
            }),
        );
        let local_read = graph.push_synthetic_specialized_value(
            2,
            0,
            ValType::I32,
            None,
            None,
            None,
            Some(SlotShape {
                slot: Some(crate::parser::core::optimizer::expr::SlotRef::entry_local(
                    slot,
                )),
                address: None,
                loop_value: None,
            }),
        );

        let mut entries = vec![BlockEntryState::default(); 3];
        entries[2].reachable = true;
        entries[2].locals.insert(slot, pred0_value);

        let mut exits = vec![BlockEntryState::default(); 3];
        exits[0].reachable = true;
        exits[0].locals.insert(slot, pred0_value);
        exits[1].reachable = true;
        exits[1].locals.insert(slot, pred1_value);

        let base_bodies = vec![
            branch_body(0, 20),
            branch_body(10, 20),
            BlockBody {
                ops: vec![crate::parser::core::optimizer::pass::BlockOp {
                    source_start: Some(20),
                    op: vm::op_i32_eqz as Op,
                    kind: BlockOpKind::PureUnary(PureOpKind::I32Eqz),
                    operands: vec![],
                    inputs: vec![local_read],
                    values: vec![eqz_result],
                }],
                terminator: None,
            },
        ];

        let overlay = build_versioned_overlay(&program, &entries, &exits, &mut graph, &base_bodies);
        assert_eq!(overlay.specialized_block_count, 1);
        assert_eq!(overlay.blocks.len(), 4);
        assert_eq!(overlay.generic_fallback_edges, 1);
    }

    #[test]
    fn build_versioned_overlay_specializes_join_memory_input_and_routes_fallback_edges() {
        let program = join_program(&[0, 10], 20);
        let mut graph = ValueGraph::default();
        let join_arg =
            graph.ensure_block_argument(2, 0, ValType::I32, None, None, None, None, None);
        let load_result =
            graph.push_synthetic_specialized_value(2, 1, ValType::I32, None, None, None, None);
        let pred0_value = graph.push_synthetic_specialized_value(
            0,
            0,
            ValType::I32,
            None,
            Some(AddressShape::base_offset(
                AddressBaseKind::EntryLocal(LocalSlot::new(0, 4)),
                8,
            )),
            None,
            None,
        );
        let pred1_value =
            graph.push_synthetic_specialized_value(1, 0, ValType::I32, None, None, None, None);

        let mut entries = vec![BlockEntryState::default(); 3];
        entries[2].reachable = true;
        entries[2].stack.push(join_arg);

        let mut exits = vec![BlockEntryState::default(); 3];
        exits[0].reachable = true;
        exits[0].stack.push(pred0_value);
        exits[1].reachable = true;
        exits[1].stack.push(pred1_value);

        let base_bodies = vec![
            branch_body(0, 20),
            branch_body(10, 20),
            BlockBody {
                ops: vec![crate::parser::core::optimizer::pass::BlockOp {
                    source_start: Some(20),
                    op: vm::op_i32_load as Op,
                    kind: BlockOpKind::MemoryLoad,
                    operands: vec![BlockOperand::Raw(Operand {
                        memarg: MemArg {
                            align: 2,
                            offset: 0,
                        },
                    })],
                    inputs: vec![join_arg],
                    values: vec![load_result],
                }],
                terminator: None,
            },
        ];

        let overlay = build_versioned_overlay(&program, &entries, &exits, &mut graph, &base_bodies);
        assert_eq!(overlay.specialized_block_count, 1);
        assert_eq!(overlay.blocks.len(), 4);
        assert_eq!(overlay.generic_fallback_edges, 1);
        assert_eq!(overlay.blocks[0].successors, vec![3]);
        assert_eq!(overlay.blocks[1].successors, vec![2]);
        assert!(graph[overlay.blocks[2].body.ops[0].inputs[0].0]
            .address_shape
            .is_none());
        assert_eq!(
            graph[overlay.blocks[3].body.ops[0].inputs[0].0].address_shape,
            Some(AddressShape::base_offset(
                AddressBaseKind::EntryLocal(LocalSlot::new(0, 4)),
                8,
            )),
        );
    }

    #[test]
    fn build_versioned_overlay_specializes_reducible_loop_header_memory_input() {
        let program = loop_header_program(10, 20);
        let mut graph = ValueGraph::default();
        let header_arg =
            graph.ensure_block_argument(1, 0, ValType::I32, None, None, None, None, None);
        let load_result =
            graph.push_synthetic_specialized_value(1, 1, ValType::I32, None, None, None, None);
        let preheader_value = graph.push_synthetic_specialized_value(
            0,
            0,
            ValType::I32,
            Some(ConstValue::I32(0)),
            None,
            None,
            None,
        );
        let backedge_value = graph.push_synthetic_specialized_value(
            2,
            0,
            ValType::I32,
            None,
            Some(AddressShape::base_offset(
                AddressBaseKind::EntryLocal(LocalSlot::new(0, 4)),
                16,
            )),
            Some(LoopValueShape::Local4(LocalSlot::new(0, 4))),
            Some(SlotShape {
                slot: Some(crate::parser::core::optimizer::expr::SlotRef::entry_local(
                    LocalSlot::new(0, 4),
                )),
                address: None,
                loop_value: None,
            }),
        );

        let mut entries = vec![BlockEntryState::default(); 3];
        entries[1].reachable = true;
        entries[1].stack.push(header_arg);

        let mut exits = vec![BlockEntryState::default(); 3];
        exits[0].reachable = true;
        exits[0].stack.push(preheader_value);
        exits[2].reachable = true;
        exits[2].stack.push(backedge_value);

        let base_bodies = vec![
            branch_body(0, 10),
            BlockBody {
                ops: vec![crate::parser::core::optimizer::pass::BlockOp {
                    source_start: Some(10),
                    op: vm::op_i32_load as Op,
                    kind: BlockOpKind::MemoryLoad,
                    operands: vec![BlockOperand::Raw(Operand {
                        memarg: MemArg {
                            align: 2,
                            offset: 0,
                        },
                    })],
                    inputs: vec![header_arg],
                    values: vec![load_result],
                }],
                terminator: None,
            },
            branch_body(20, 10),
        ];

        let overlay = build_versioned_overlay(&program, &entries, &exits, &mut graph, &base_bodies);
        assert_eq!(overlay.specialized_block_count, 1);
        assert_eq!(overlay.generic_fallback_edges, 1);
        assert_eq!(overlay.blocks.len(), 4);
        assert_eq!(overlay.blocks[0].successors, vec![1]);
        assert_eq!(overlay.blocks[2].successors, vec![3]);
        assert_eq!(
            graph[overlay.blocks[3].body.ops[0].inputs[0].0].address_shape,
            Some(AddressShape::base_offset(
                AddressBaseKind::EntryLocal(LocalSlot::new(0, 4)),
                16,
            )),
        );
        assert_eq!(
            graph[overlay.blocks[3].body.ops[0].inputs[0].0].loop_value_shape,
            Some(LoopValueShape::Local4(LocalSlot::new(0, 4))),
        );
    }

    #[test]
    fn build_versioned_overlay_falls_back_when_alias_heap_version_is_unknown() {
        let program = join_program(&[0, 10], 20);
        let mut graph = ValueGraph::default();
        let join_arg =
            graph.ensure_block_argument(2, 0, ValType::I32, None, None, None, None, None);
        let pred0_value =
            graph.push_synthetic_specialized_value(0, 0, ValType::I32, None, None, None, None);
        let pred1_value =
            graph.push_synthetic_specialized_value(1, 0, ValType::I32, None, None, None, None);
        let table_result =
            graph.push_synthetic_specialized_value(2, 1, ValType::I32, None, None, None, None);
        let alias = AliasKey {
            space: AliasSpace::Table,
            index: 0,
            offset: 0,
            width: 4,
            address: crate::parser::core::optimizer::expr::AliasAddress::Origin(
                crate::parser::core::optimizer::expr::ExprOrigin {
                    block_id: 2,
                    ordinal: 0,
                    kind: crate::parser::core::optimizer::expr::ExprOriginKind::BlockArgument,
                },
            ),
        };

        let mut entries = vec![BlockEntryState::default(); 3];
        entries[2].reachable = true;
        entries[2].aliases.insert(alias.clone(), join_arg);

        let mut exits = vec![BlockEntryState::default(); 3];
        exits[0].reachable = true;
        exits[0].heap = HeapVersion {
            memory: 0,
            global: 0,
            table: UNKNOWN_HEAP_VERSION,
        };
        exits[0].aliases.insert(alias.clone(), pred0_value);
        exits[1].reachable = true;
        exits[1].heap = HeapVersion {
            memory: 0,
            global: 0,
            table: UNKNOWN_HEAP_VERSION,
        };
        exits[1].aliases.insert(alias.clone(), pred1_value);

        let base_bodies = vec![
            branch_body(0, 20),
            branch_body(10, 20),
            BlockBody {
                ops: vec![crate::parser::core::optimizer::pass::BlockOp {
                    source_start: Some(20),
                    op: vm::op_table_get as Op,
                    kind: BlockOpKind::TableGet,
                    operands: vec![BlockOperand::U32(0)],
                    inputs: vec![join_arg],
                    values: vec![table_result],
                }],
                terminator: None,
            },
        ];

        let overlay = build_versioned_overlay(&program, &entries, &exits, &mut graph, &base_bodies);
        assert_eq!(overlay.specialized_block_count, 0);
        assert_eq!(overlay.blocks.len(), 3);
    }

    #[test]
    fn build_versioned_overlay_caps_versions_to_generic_plus_one_specialized_clone() {
        let program = join_program(&[0, 10, 20], 30);
        let mut graph = ValueGraph::default();
        let join_arg =
            graph.ensure_block_argument(3, 0, ValType::I32, None, None, None, None, None);
        let call_result =
            graph.push_synthetic_specialized_value(3, 1, ValType::I32, None, None, None, None);
        let pred0_value = graph.push_synthetic_specialized_value(
            0,
            0,
            ValType::I32,
            Some(ConstValue::I32(1)),
            None,
            None,
            None,
        );
        let pred1_value = graph.push_synthetic_specialized_value(
            1,
            0,
            ValType::I32,
            Some(ConstValue::I32(2)),
            None,
            None,
            None,
        );
        let pred2_value = graph.push_synthetic_specialized_value(
            2,
            0,
            ValType::I32,
            Some(ConstValue::I32(3)),
            None,
            None,
            None,
        );

        let mut entries = vec![BlockEntryState::default(); 4];
        entries[3].reachable = true;
        entries[3].stack.push(join_arg);

        let mut exits = vec![BlockEntryState::default(); 4];
        exits[0].reachable = true;
        exits[0].stack.push(pred0_value);
        exits[1].reachable = true;
        exits[1].stack.push(pred1_value);
        exits[2].reachable = true;
        exits[2].stack.push(pred2_value);

        let base_bodies = vec![
            branch_body(0, 30),
            branch_body(10, 30),
            branch_body(20, 30),
            BlockBody {
                ops: vec![crate::parser::core::optimizer::pass::BlockOp {
                    source_start: Some(30),
                    op: vm::op_call_import as Op,
                    kind: BlockOpKind::CallLike,
                    operands: vec![BlockOperand::U32(7)],
                    inputs: vec![join_arg],
                    values: vec![call_result],
                }],
                terminator: None,
            },
        ];

        let overlay = build_versioned_overlay(&program, &entries, &exits, &mut graph, &base_bodies);
        assert_eq!(overlay.specialized_block_count, 1);
        assert_eq!(overlay.blocks.len(), 5);
        assert_eq!(overlay.generic_fallback_edges, 2);
        assert_eq!(overlay.blocks[0].successors, vec![4]);
        assert_eq!(overlay.blocks[1].successors, vec![3]);
        assert_eq!(overlay.blocks[2].successors, vec![3]);
        assert!(std::ptr::fn_addr_eq(
            overlay.blocks[4].body.ops[0].op,
            vm::op_call_import as Op,
        ));
    }

    #[test]
    fn build_selection_plan_picks_best_allowed_candidate_on_large_function() {
        let pred_starts = [0, 10, 20, 30, 40, 50, 60, 70, 80];
        let join_start = 90;
        let program = join_program(&pred_starts, join_start);
        let mut graph = ValueGraph::default();
        let slot1 = LocalSlot::new(0, 4);
        let slot2 = LocalSlot::new(4, 4);
        let slot3 = LocalSlot::new(8, 4);
        let join_stack =
            graph.ensure_block_argument(9, 0, ValType::I32, None, None, None, None, None);
        let join_local1 = graph.ensure_block_argument(
            9,
            local_binding_ordinal(slot1),
            ValType::I32,
            None,
            None,
            None,
            None,
            None,
        );
        let join_local2 = graph.ensure_block_argument(
            9,
            local_binding_ordinal(slot2),
            ValType::I32,
            None,
            None,
            None,
            None,
            None,
        );
        let join_local3 = graph.ensure_block_argument(
            9,
            local_binding_ordinal(slot3),
            ValType::I32,
            None,
            None,
            None,
            None,
            None,
        );
        let load_result =
            graph.push_synthetic_specialized_value(9, 0, ValType::I32, None, None, None, None);
        let scalar_stack = graph.push_synthetic_specialized_value(
            0,
            0,
            ValType::I32,
            Some(ConstValue::I32(1)),
            None,
            None,
            None,
        );
        let scalar_local1 = graph.push_synthetic_specialized_value(
            0,
            1,
            ValType::I32,
            Some(ConstValue::I32(2)),
            None,
            None,
            None,
        );
        let scalar_local2 = graph.push_synthetic_specialized_value(
            0,
            2,
            ValType::I32,
            Some(ConstValue::I32(3)),
            None,
            None,
            None,
        );
        let scalar_local3 = graph.push_synthetic_specialized_value(
            0,
            3,
            ValType::I32,
            Some(ConstValue::I32(4)),
            None,
            None,
            None,
        );
        let address_stack = graph.push_synthetic_specialized_value(
            1,
            0,
            ValType::I32,
            None,
            Some(AddressShape::base_offset(
                AddressBaseKind::EntryLocal(slot1),
                8,
            )),
            None,
            None,
        );
        let generic_local1 =
            graph.push_synthetic_specialized_value(1, 1, ValType::I32, None, None, None, None);
        let generic_local2 =
            graph.push_synthetic_specialized_value(1, 2, ValType::I32, None, None, None, None);
        let generic_local3 =
            graph.push_synthetic_specialized_value(1, 3, ValType::I32, None, None, None, None);

        let mut entries = vec![BlockEntryState::default(); 10];
        entries[9].reachable = true;
        entries[9].stack.push(join_stack);
        entries[9].locals.insert(slot1, join_local1);
        entries[9].locals.insert(slot2, join_local2);
        entries[9].locals.insert(slot3, join_local3);

        let mut exits = vec![BlockEntryState::default(); 10];
        exits[0].reachable = true;
        exits[0].stack.push(scalar_stack);
        exits[0].locals.insert(slot1, scalar_local1);
        exits[0].locals.insert(slot2, scalar_local2);
        exits[0].locals.insert(slot3, scalar_local3);
        exits[1].reachable = true;
        exits[1].stack.push(address_stack);
        exits[1].locals.insert(slot1, generic_local1);
        exits[1].locals.insert(slot2, generic_local2);
        exits[1].locals.insert(slot3, generic_local3);

        let mut base_bodies = pred_starts
            .iter()
            .map(|start| branch_body(*start, join_start))
            .collect::<Vec<_>>();
        base_bodies.push(BlockBody {
            ops: vec![
                crate::parser::core::optimizer::pass::BlockOp {
                    source_start: Some(join_start),
                    op: vm::op_i32_load as Op,
                    kind: BlockOpKind::MemoryLoad,
                    operands: vec![BlockOperand::Raw(Operand {
                        memarg: MemArg {
                            align: 2,
                            offset: 0,
                        },
                    })],
                    inputs: vec![join_stack],
                    values: vec![load_result],
                },
                crate::parser::core::optimizer::pass::BlockOp {
                    source_start: Some(join_start + 1),
                    op: vm::op_local_set4 as Op,
                    kind: BlockOpKind::LocalSet,
                    operands: vec![BlockOperand::LocalAddr(slot1.addr)],
                    inputs: vec![join_local1],
                    values: Vec::new(),
                },
                crate::parser::core::optimizer::pass::BlockOp {
                    source_start: Some(join_start + 2),
                    op: vm::op_local_set4 as Op,
                    kind: BlockOpKind::LocalSet,
                    operands: vec![BlockOperand::LocalAddr(slot2.addr)],
                    inputs: vec![join_local2],
                    values: Vec::new(),
                },
                crate::parser::core::optimizer::pass::BlockOp {
                    source_start: Some(join_start + 3),
                    op: vm::op_local_set4 as Op,
                    kind: BlockOpKind::LocalSet,
                    operands: vec![BlockOperand::LocalAddr(slot3.addr)],
                    inputs: vec![join_local3],
                    values: Vec::new(),
                },
            ],
            terminator: None,
        });

        let overlay = build_versioned_overlay(&program, &entries, &exits, &mut graph, &base_bodies);
        assert_eq!(overlay.specialized_block_count, 1);
        assert_eq!(overlay.blocks[0].successors, vec![9]);
        assert_eq!(overlay.blocks[1].successors, vec![10]);
        assert_eq!(
            graph[overlay.blocks[10].body.ops[0].inputs[0].0].address_shape,
            Some(AddressShape::base_offset(
                AddressBaseKind::EntryLocal(slot1),
                8
            )),
        );
    }
}
