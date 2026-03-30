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
        block_entry_states_equal, merge_overlay_entry_state, replay_block_body_for_overlay,
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
    usage: BindingUsage,
}

#[derive(Clone, Debug, Default)]
struct BindingUsage {
    memory_address: bool,
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
    pred: usize,
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
    pub(super) entries: Vec<BlockEntryState>,
    pub(super) exits: Vec<BlockEntryState>,
    pub(super) specialized_block_count: usize,
    pub(super) generic_fallback_edges: usize,
    pub(super) specialized_to_specialized_edges: usize,
    pub(super) selected_cfg_specialized_edges: usize,
    pub(super) selected_original_routable_edges: usize,
    pub(super) fixpoint_iterations: usize,
    pub(super) route_rewrites: usize,
    pub(super) fixpoint_fallback: bool,
    pub(super) version_key_fact_breakdown: BTreeMap<&'static str, usize>,
    pub(super) versionable_candidate_blocks: usize,
    pub(super) blocks_with_entry_bindings: usize,
    pub(super) blocks_with_version_candidates: usize,
}

const LARGE_FUNCTION_SCALAR_ONLY_VERSIONING_BLOCK_LIMIT: usize = 8;
const SELECTED_PREDECESSOR_CHAIN_BONUS: u32 = 2;

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
            entries: vec![BlockEntryState::default(); program.blocks.len()],
            exits: vec![BlockEntryState::default(); program.blocks.len()],
            specialized_block_count: 0,
            generic_fallback_edges: 0,
            specialized_to_specialized_edges: 0,
            selected_cfg_specialized_edges: 0,
            selected_original_routable_edges: 0,
            fixpoint_iterations: 0,
            route_rewrites: 0,
            fixpoint_fallback: false,
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

    for block in &mut blocks {
        if block.version.kind == BlockVersionKind::Specialized {
            retag_specialized_source_starts(block);
        }
    }

    let binding_cache = build_entry_binding_cache(entries, graph, base_bodies);
    let selected_cfg_specialized_edges = count_selected_cfg_specialized_edges(program, &selection);
    let selected_original_routable_edges =
        count_selected_original_routable_edges(program, exits, graph, &selection, &binding_cache);
    let mut edge_targets = build_initial_edge_targets(
        program,
        graph,
        exits,
        &selection,
        &binding_cache,
        &blocks,
        &generic_indices,
        &specialized_indices,
    );
    let mut overlay = VersionedRewriteOverlay {
        blocks,
        reachable: Vec::new(),
        entries: vec![BlockEntryState::default(); program.blocks.len() + selected_block_count],
        exits: vec![BlockEntryState::default(); program.blocks.len() + selected_block_count],
        specialized_block_count: 0,
        generic_fallback_edges: 0,
        specialized_to_specialized_edges: 0,
        selected_cfg_specialized_edges,
        selected_original_routable_edges,
        fixpoint_iterations: 0,
        route_rewrites: 0,
        fixpoint_fallback: false,
        version_key_fact_breakdown: fact_breakdown,
        versionable_candidate_blocks: selection.versionable_candidate_blocks,
        blocks_with_entry_bindings: selection.blocks_with_entry_bindings,
        blocks_with_version_candidates: selection.blocks_with_version_candidates,
    };
    apply_edge_targets(program, base_bodies, &mut overlay, &edge_targets);
    analyze_overlay_fixpoint(
        program,
        entries,
        exits,
        graph,
        base_bodies,
        &binding_cache,
        &selection,
        &generic_indices,
        &specialized_indices,
        &mut overlay,
        &mut edge_targets,
    );
    finalize_overlay_jump_targets(program, base_bodies, &mut overlay, &edge_targets);
    let specialized_block_count = overlay
        .blocks
        .iter()
        .filter(|block| block.version.kind == BlockVersionKind::Specialized)
        .count();
    overlay.specialized_block_count = specialized_block_count;
    overlay
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

fn count_selected_cfg_specialized_edges(
    program: &BasicBlockProgram,
    selection: &VersionSelectionPlan,
) -> usize {
    program
        .successors
        .iter()
        .enumerate()
        .filter(|(block_id, _)| selection.selected[*block_id].is_some())
        .map(|(_, succs)| {
            succs
                .iter()
                .filter(|succ| selection.selected[**succ].is_some())
                .count()
        })
        .sum()
}

fn count_selected_original_routable_edges(
    program: &BasicBlockProgram,
    exits: &[BlockEntryState],
    graph: &ValueGraph,
    selection: &VersionSelectionPlan,
    binding_cache: &[Vec<EntryBinding>],
) -> usize {
    program
        .successors
        .iter()
        .enumerate()
        .filter(|(block_id, _)| selection.selected[*block_id].is_some())
        .map(|(block_id, succs)| {
            succs
                .iter()
                .filter(|succ| {
                    let Some(selected) = selection.selected[**succ].as_ref() else {
                        return false;
                    };
                    build_candidate_for_state(&exits[block_id], graph, &binding_cache[**succ])
                        .map(|candidate| version_key_satisfies(&candidate.key, &selected.key))
                        .unwrap_or(false)
                })
                .count()
        })
        .sum()
}

fn build_entry_binding_cache(
    entries: &[BlockEntryState],
    graph: &ValueGraph,
    base_bodies: &[BlockBody],
) -> Vec<Vec<EntryBinding>> {
    base_bodies
        .iter()
        .enumerate()
        .map(|(block_id, body)| collect_entry_bindings(entries, graph, block_id, body))
        .collect()
}

fn build_initial_edge_targets(
    program: &BasicBlockProgram,
    graph: &ValueGraph,
    exits: &[BlockEntryState],
    selection: &VersionSelectionPlan,
    binding_cache: &[Vec<EntryBinding>],
    blocks: &[VersionedBlock],
    generic_indices: &[usize],
    specialized_indices: &[Option<usize>],
) -> HashMap<(usize, usize), usize> {
    let mut edge_targets = HashMap::new();
    for block in blocks {
        let original_block = block.version.original_block;
        for succ in rewritten_successor_blocks(program, original_block, &block.body) {
            let pred_state = exits.get(original_block).cloned().unwrap_or_default();
            let (target, _) = select_overlay_successor_target(
                graph,
                &pred_state,
                None,
                block.version.kind,
                succ,
                selection,
                binding_cache,
                generic_indices[succ],
                specialized_indices[succ],
                block.version.kind == BlockVersionKind::Generic,
            );
            edge_targets.insert((block.id, succ), target);
        }
    }
    edge_targets
}

fn analyze_overlay_fixpoint(
    program: &BasicBlockProgram,
    original_entries: &[BlockEntryState],
    original_exits: &[BlockEntryState],
    graph: &mut ValueGraph,
    base_bodies: &[BlockBody],
    binding_cache: &[Vec<EntryBinding>],
    selection: &VersionSelectionPlan,
    generic_indices: &[usize],
    specialized_indices: &[Option<usize>],
    overlay: &mut VersionedRewriteOverlay,
    edge_targets: &mut HashMap<(usize, usize), usize>,
) {
    let max_iterations = overlay.blocks.len().saturating_mul(2).saturating_add(1);
    analyze_overlay_fixpoint_with_limit(
        program,
        original_entries,
        original_exits,
        graph,
        base_bodies,
        binding_cache,
        selection,
        generic_indices,
        specialized_indices,
        overlay,
        edge_targets,
        max_iterations,
    );
}

fn analyze_overlay_fixpoint_with_limit(
    program: &BasicBlockProgram,
    original_entries: &[BlockEntryState],
    original_exits: &[BlockEntryState],
    graph: &mut ValueGraph,
    base_bodies: &[BlockBody],
    binding_cache: &[Vec<EntryBinding>],
    selection: &VersionSelectionPlan,
    generic_indices: &[usize],
    specialized_indices: &[Option<usize>],
    overlay: &mut VersionedRewriteOverlay,
    edge_targets: &mut HashMap<(usize, usize), usize>,
    max_iterations: usize,
) {
    let initial_edge_targets = edge_targets.clone();
    let mut route_rewrites = 0usize;
    let mut iterations = 0usize;
    let mut converged = false;

    for iter in 0..max_iterations {
        iterations = iter + 1;
        apply_edge_targets(program, base_bodies, overlay, edge_targets);
        let (entries, exits, replay_failed) =
            compute_overlay_states(program, original_entries, original_exits, graph, overlay);
        let states_changed = overlay_state_vectors_changed(graph, &overlay.entries, &entries)
            || overlay_state_vectors_changed(graph, &overlay.exits, &exits);
        overlay.entries = entries;
        overlay.exits = exits;

        let mut next_edge_targets = edge_targets.clone();
        let mut changed_edges = 0usize;
        for block in &overlay.blocks {
            if !overlay.reachable[block.id] {
                continue;
            }
            let pred_state = if replay_failed[block.id] {
                &original_exits[block.version.original_block]
            } else {
                &overlay.exits[block.id]
            };
            for succ in rewritten_successor_blocks(
                program,
                block.version.original_block,
                &base_bodies[block.version.original_block],
            ) {
                let allow_specialized =
                    !replay_failed[block.id] || block.version.kind == BlockVersionKind::Specialized;
                let (target, _) = select_overlay_successor_target(
                    graph,
                    pred_state,
                    Some(&original_exits[block.version.original_block]),
                    block.version.kind,
                    succ,
                    selection,
                    binding_cache,
                    generic_indices[succ],
                    specialized_indices[succ],
                    allow_specialized,
                );
                let entry = next_edge_targets
                    .entry((block.id, succ))
                    .or_insert(generic_indices[succ]);
                if *entry != target {
                    *entry = target;
                    changed_edges += 1;
                }
            }
        }
        route_rewrites = route_rewrites.saturating_add(changed_edges);
        if changed_edges == 0 && !states_changed {
            *edge_targets = next_edge_targets;
            converged = true;
            break;
        }
        *edge_targets = next_edge_targets;
    }

    overlay.fixpoint_iterations = iterations;
    overlay.route_rewrites = route_rewrites;
    if !converged {
        overlay.fixpoint_fallback = true;
        *edge_targets = initial_edge_targets;
        apply_edge_targets(program, base_bodies, overlay, edge_targets);
        let (entries, exits, _) =
            compute_overlay_states(program, original_entries, original_exits, graph, overlay);
        overlay.entries = entries;
        overlay.exits = exits;
    } else {
        apply_edge_targets(program, base_bodies, overlay, edge_targets);
    }
}

fn compute_overlay_states(
    program: &BasicBlockProgram,
    original_entries: &[BlockEntryState],
    original_exits: &[BlockEntryState],
    graph: &mut ValueGraph,
    overlay: &VersionedRewriteOverlay,
) -> (Vec<BlockEntryState>, Vec<BlockEntryState>, Vec<bool>) {
    let block_count = overlay.blocks.len();
    let predecessors = overlay_predecessors(overlay);
    let mut entries = vec![BlockEntryState::default(); block_count];
    let mut exits = vec![BlockEntryState::default(); block_count];
    let mut replay_failed = vec![false; block_count];
    let mut worklist = overlay
        .reachable
        .iter()
        .enumerate()
        .filter_map(|(block_id, reachable)| reachable.then_some(block_id))
        .collect::<VecDeque<_>>();
    let mut queued = vec![false; block_count];
    for block_id in worklist.iter().copied() {
        queued[block_id] = true;
    }

    while let Some(block_id) = worklist.pop_front() {
        queued[block_id] = false;
        if !overlay.reachable[block_id] {
            continue;
        }
        let block = &overlay.blocks[block_id];
        let mut incoming = Vec::new();
        if block.version.original_block == 0 {
            if let Some(entry) = original_entries.first().filter(|entry| entry.reachable) {
                incoming.push(entry.clone());
            }
        }
        for pred in &predecessors[block_id] {
            if exits[*pred].reachable {
                incoming.push(exits[*pred].clone());
            }
        }
        let Some(entry) = merge_overlay_entry_state(
            program,
            graph,
            block.version.original_block,
            overlay_analysis_block_id(program, block_id),
            &incoming,
        ) else {
            if original_exits[block.version.original_block].reachable {
                let seeded_entry = original_entries
                    .get(block.version.original_block)
                    .cloned()
                    .unwrap_or_default();
                let entry_changed =
                    !block_entry_states_equal(graph, &entries[block_id], &seeded_entry);
                if entry_changed {
                    entries[block_id] = seeded_entry;
                }
                let exit_changed = !block_entry_states_equal(
                    graph,
                    &exits[block_id],
                    &original_exits[block.version.original_block],
                );
                if exit_changed {
                    exits[block_id] = original_exits[block.version.original_block].clone();
                    for succ in &overlay.blocks[block_id].successors {
                        if overlay.reachable[*succ] && !queued[*succ] {
                            queued[*succ] = true;
                            worklist.push_back(*succ);
                        }
                    }
                }
            }
            continue;
        };
        let entry_changed = !block_entry_states_equal(graph, &entries[block_id], &entry);
        if entry_changed {
            entries[block_id] = entry.clone();
        }
        let value_map = build_overlay_entry_value_map(
            graph,
            original_entries,
            &entry,
            block.version.original_block,
            &block.body,
        );
        let replayed = replay_block_body_for_overlay(
            graph,
            overlay_analysis_block_id(program, block_id),
            &entry,
            &block.body,
            &value_map,
        );
        replay_failed[block_id] = replayed.is_none();
        let exit = replayed
            .map(|exit| {
                overlay_complete_exit_with_original(
                    graph,
                    exit,
                    &original_exits[block.version.original_block],
                )
            })
            .unwrap_or_else(|| original_exits[block.version.original_block].clone());
        let exit_changed = !block_entry_states_equal(graph, &exits[block_id], &exit);
        if exit_changed {
            exits[block_id] = exit;
        }
        if entry_changed || exit_changed {
            for succ in &overlay.blocks[block_id].successors {
                if overlay.reachable[*succ] && !queued[*succ] {
                    queued[*succ] = true;
                    worklist.push_back(*succ);
                }
            }
        }
    }

    (entries, exits, replay_failed)
}

fn build_overlay_entry_value_map(
    graph: &ValueGraph,
    original_entries: &[BlockEntryState],
    entry: &BlockEntryState,
    original_block: usize,
    body: &BlockBody,
) -> HashMap<ValueRef, ValueRef> {
    let mut value_map = HashMap::new();
    for binding in collect_entry_bindings(original_entries, graph, original_block, body) {
        let value = match binding.id {
            EntryBindingId::Stack(ordinal) => entry.stack.get(ordinal).copied(),
            EntryBindingId::Local(slot) => entry.locals.get(&slot).copied(),
            EntryBindingId::Alias(ref alias) => entry.aliases.get(alias).copied(),
        };
        if let Some(value) = value {
            value_map.insert(binding.block_value, value);
        }
    }
    value_map
}

fn overlay_predecessors(overlay: &VersionedRewriteOverlay) -> Vec<Vec<usize>> {
    let mut predecessors = vec![Vec::new(); overlay.blocks.len()];
    for block in &overlay.blocks {
        if !overlay.reachable[block.id] {
            continue;
        }
        for succ in &block.successors {
            predecessors[*succ].push(block.id);
        }
    }
    predecessors
}

fn overlay_state_vectors_changed(
    graph: &ValueGraph,
    lhs: &[BlockEntryState],
    rhs: &[BlockEntryState],
) -> bool {
    lhs.len() != rhs.len()
        || lhs
            .iter()
            .zip(rhs.iter())
            .any(|(lhs, rhs)| !block_entry_states_equal(graph, lhs, rhs))
}

fn overlay_complete_exit_with_original(
    graph: &ValueGraph,
    mut exit: BlockEntryState,
    original: &BlockEntryState,
) -> BlockEntryState {
    if !original.reachable {
        return exit;
    }
    exit.reachable |= original.reachable;
    exit.stack = (0..original.stack.len())
        .map(|index| {
            let original_value = original.stack[index];
            exit.stack
                .get(index)
                .copied()
                .map(|current| overlay_prefer_richer_value(graph, current, original_value))
                .unwrap_or(original_value)
        })
        .collect();
    for (slot, value) in &original.locals {
        exit.locals
            .entry(*slot)
            .and_modify(|current| *current = overlay_prefer_richer_value(graph, *current, *value))
            .or_insert(*value);
    }
    for (alias, value) in &original.aliases {
        exit.aliases
            .entry(alias.clone())
            .and_modify(|current| *current = overlay_prefer_richer_value(graph, *current, *value))
            .or_insert(*value);
    }
    exit
}

fn overlay_prefer_richer_value(
    graph: &ValueGraph,
    current: ValueRef,
    original: ValueRef,
) -> ValueRef {
    if overlay_value_metadata_score(graph, original) > overlay_value_metadata_score(graph, current)
    {
        original
    } else {
        current
    }
}

fn overlay_value_metadata_score(graph: &ValueGraph, value: ValueRef) -> u8 {
    let node = &graph[value.0];
    u8::from(node.const_value.is_some())
        .saturating_add(u8::from(
            stable_slot_shape(node.slot_shape.clone()).is_some(),
        ))
        .saturating_add(u8::from(node.loop_value_shape.is_some()))
        .saturating_add(u8::from(stable_address_shape(node.address_shape).is_some()))
}

fn overlay_analysis_block_id(program: &BasicBlockProgram, versioned_block: usize) -> usize {
    program.blocks.len().saturating_add(versioned_block)
}

fn select_overlay_successor_target(
    graph: &ValueGraph,
    pred_state: &BlockEntryState,
    fallback_pred_state: Option<&BlockEntryState>,
    pred_kind: BlockVersionKind,
    succ_block: usize,
    selection: &VersionSelectionPlan,
    binding_cache: &[Vec<EntryBinding>],
    generic_target: usize,
    specialized_target: Option<usize>,
    allow_specialized: bool,
) -> (usize, bool) {
    let Some(selected) = selection.selected[succ_block].as_ref() else {
        return (generic_target, false);
    };
    let Some(specialized_target) = specialized_target else {
        return (generic_target, false);
    };
    if !allow_specialized {
        return (generic_target, false);
    }
    if state_matches_selected_key(pred_state, graph, &binding_cache[succ_block], &selected.key)
        || fallback_pred_state.is_some_and(|fallback| {
            state_matches_selected_key(fallback, graph, &binding_cache[succ_block], &selected.key)
        })
    {
        (specialized_target, false)
    } else {
        (
            generic_target,
            pred_kind == BlockVersionKind::Generic || pred_kind == BlockVersionKind::Specialized,
        )
    }
}

fn state_matches_selected_key(
    state: &BlockEntryState,
    graph: &ValueGraph,
    bindings: &[EntryBinding],
    selected_key: &VersionKey,
) -> bool {
    build_candidate_for_state(state, graph, bindings)
        .map(|candidate| version_key_satisfies(&candidate.key, selected_key))
        .unwrap_or(false)
}

fn version_key_satisfies(edge_key: &VersionKey, selected_key: &VersionKey) -> bool {
    selected_key
        .facts
        .iter()
        .all(|fact| edge_key.facts.contains(fact))
}

fn apply_edge_targets(
    program: &BasicBlockProgram,
    base_bodies: &[BlockBody],
    overlay: &mut VersionedRewriteOverlay,
    edge_targets: &HashMap<(usize, usize), usize>,
) {
    let mut specialized_exists = vec![false; program.blocks.len()];
    for block in &overlay.blocks {
        if block.version.kind == BlockVersionKind::Specialized {
            specialized_exists[block.version.original_block] = true;
        }
    }

    let mut generic_fallback_edges = 0usize;
    let mut specialized_to_specialized_edges = 0usize;
    for versioned in 0..overlay.blocks.len() {
        let original_block = overlay.blocks[versioned].version.original_block;
        overlay.blocks[versioned].fallthrough =
            fallthrough_successor_block(program, original_block, &base_bodies[original_block])
                .and_then(|succ| edge_targets.get(&(versioned, succ)).copied());
        let mut successors = Vec::new();
        for succ in
            rewritten_successor_blocks(program, original_block, &base_bodies[original_block])
        {
            let Some(target) = edge_targets.get(&(versioned, succ)).copied() else {
                continue;
            };
            if specialized_exists[succ]
                && overlay.blocks[versioned].version.kind == BlockVersionKind::Generic
                && overlay.blocks[target].version.kind == BlockVersionKind::Generic
            {
                generic_fallback_edges = generic_fallback_edges.saturating_add(1);
            }
            if overlay.blocks[versioned].version.kind == BlockVersionKind::Specialized
                && overlay.blocks[target].version.kind == BlockVersionKind::Specialized
            {
                specialized_to_specialized_edges =
                    specialized_to_specialized_edges.saturating_add(1);
            }
            successors.push(target);
        }
        successors.sort_unstable();
        successors.dedup();
        overlay.blocks[versioned].successors = successors;
    }
    overlay.generic_fallback_edges = generic_fallback_edges;
    overlay.specialized_to_specialized_edges = specialized_to_specialized_edges;
    overlay.reachable = compute_overlay_reachability(&overlay.blocks);
}

fn finalize_overlay_jump_targets(
    program: &BasicBlockProgram,
    base_bodies: &[BlockBody],
    overlay: &mut VersionedRewriteOverlay,
    edge_targets: &HashMap<(usize, usize), usize>,
) {
    let mut edge_target_labels = HashMap::new();
    for block in &overlay.blocks {
        let original_block = block.version.original_block;
        for succ in
            rewritten_successor_blocks(program, original_block, &base_bodies[original_block])
        {
            let Some(target) = edge_targets.get(&(block.id, succ)).copied() else {
                continue;
            };
            edge_target_labels.insert((block.id, succ), overlay.blocks[target].block_label);
        }
    }
    for block in &mut overlay.blocks {
        rewrite_jump_targets(
            program,
            block.id,
            block.version.original_block,
            &mut block.body,
            &base_bodies[block.version.original_block],
            &edge_target_labels,
        );
    }
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
    let mut allowed_candidates = vec![Vec::new(); program.blocks.len()];
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
        candidates.retain(|candidate| allows_selected_version(program, &candidate.key));
        if candidates.is_empty() {
            continue;
        }
        allowed_candidates[block.id] = candidates;
    }

    let max_iterations = program.blocks.len().saturating_mul(2).saturating_add(1);
    for _ in 0..max_iterations {
        let mut changed = false;
        for block in &program.blocks {
            let candidates = &allowed_candidates[block.id];
            let Some(selected) = select_preferred_candidate(candidates, &plan.selected) else {
                continue;
            };
            let should_replace = plan.selected[block.id]
                .as_ref()
                .is_none_or(|current| current.pred != selected.pred || current.key != selected.key);
            if should_replace {
                plan.selected[block.id] = Some(SelectedVersion {
                    pred: selected.pred,
                    key: selected.key.clone(),
                    bindings: selected.bindings.clone(),
                });
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    plan
}

fn select_preferred_candidate<'a>(
    candidates: &'a [VersionCandidate],
    selected: &[Option<SelectedVersion>],
) -> Option<&'a VersionCandidate> {
    candidates
        .iter()
        .min_by(|lhs, rhs| compare_selected_candidates(lhs, rhs, selected))
}

fn compare_selected_candidates(
    lhs: &VersionCandidate,
    rhs: &VersionCandidate,
    selected: &[Option<SelectedVersion>],
) -> std::cmp::Ordering {
    let lhs_effective = lhs
        .key
        .score
        .saturating_add(selected_predecessor_chain_bonus(lhs, selected));
    let rhs_effective = rhs
        .key
        .score
        .saturating_add(selected_predecessor_chain_bonus(rhs, selected));
    rhs_effective
        .cmp(&lhs_effective)
        .then_with(|| rhs.key.score.cmp(&lhs.key.score))
        .then_with(|| lhs.key.canonical.cmp(&rhs.key.canonical))
        .then_with(|| lhs.pred.cmp(&rhs.pred))
}

fn selected_predecessor_chain_bonus(
    candidate: &VersionCandidate,
    selected: &[Option<SelectedVersion>],
) -> u32 {
    selected
        .get(candidate.pred)
        .and_then(|selected| selected.as_ref())
        .is_some()
        .then_some(SELECTED_PREDECESSOR_CHAIN_BONUS)
        .unwrap_or(0)
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
                usage: BindingUsage::default(),
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
                usage: BindingUsage::default(),
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
                usage: BindingUsage::default(),
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
    let memory_address_bindings = collect_memory_address_bindings(
        graph,
        body,
        block_id,
        &binding_by_local,
        &binding_by_block_argument_ordinal,
    );
    for value in collect_body_values(body) {
        let origin = graph[value.0].origin;
        if origin.kind == ExprOriginKind::BlockArgument && origin.block_id == block_id {
            let Some(binding) = binding_by_block_argument_ordinal
                .get(&origin.ordinal)
                .cloned()
                .map(|binding| apply_binding_usage(binding, &memory_address_bindings))
            else {
                continue;
            };
            if seen.insert(binding.id.clone()) {
                ordered.push(binding);
            }
            continue;
        }

        for slot in referenced_entry_local_slots(&graph[value.0]) {
            let Some(binding) = binding_by_local
                .get(&slot)
                .cloned()
                .map(|binding| apply_binding_usage(binding, &memory_address_bindings))
            else {
                continue;
            };
            if seen.insert(binding.id.clone()) {
                ordered.push(binding);
            }
        }
    }
    ordered
}

fn collect_memory_address_bindings(
    graph: &ValueGraph,
    body: &BlockBody,
    block_id: usize,
    binding_by_local: &HashMap<LocalSlot, EntryBinding>,
    binding_by_block_argument_ordinal: &HashMap<usize, EntryBinding>,
) -> BTreeSet<EntryBindingId> {
    let mut bindings = BTreeSet::new();
    for op in &body.ops {
        if !matches!(op.kind, BlockOpKind::MemoryLoad | BlockOpKind::MemoryStore) {
            continue;
        }
        let Some(address) = op.inputs.first().copied() else {
            continue;
        };
        let origin = graph[address.0].origin;
        if origin.kind == ExprOriginKind::BlockArgument && origin.block_id == block_id {
            if let Some(binding) = binding_by_block_argument_ordinal.get(&origin.ordinal) {
                bindings.insert(binding.id.clone());
            }
        }
        for slot in referenced_entry_local_slots(&graph[address.0]) {
            if let Some(binding) = binding_by_local.get(&slot) {
                bindings.insert(binding.id.clone());
            }
        }
    }
    bindings
}

fn apply_binding_usage(
    mut binding: EntryBinding,
    memory_address_bindings: &BTreeSet<EntryBindingId>,
) -> EntryBinding {
    binding.usage.memory_address = memory_address_bindings.contains(&binding.id);
    binding
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
    build_candidate_for_state(state, graph, bindings).map(|mut candidate| {
        candidate.pred = pred;
        candidate
    })
}

fn build_candidate_for_state(
    state: &BlockEntryState,
    graph: &ValueGraph,
    bindings: &[EntryBinding],
) -> Option<VersionCandidate> {
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
        let address_shape = stable_address_shape(node.address_shape)
            .or_else(|| fallback_binding_address_shape(binding));
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
        pred: usize::MAX,
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

fn fallback_binding_address_shape(binding: &EntryBinding) -> Option<AddressShape> {
    if !binding.usage.memory_address {
        return None;
    }
    match binding.id {
        EntryBindingId::Local(slot) => Some(AddressShape::base_offset(
            AddressBaseKind::EntryLocal(slot),
            0,
        )),
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

    fn two_stage_join_program(
        first_pred_starts: &[usize],
        first_join_start: usize,
        second_pred_start: usize,
        second_join_start: usize,
    ) -> BasicBlockProgram {
        let mut records = first_pred_starts
            .iter()
            .map(|start| decoded_branch(*start, first_join_start))
            .collect::<Vec<_>>();
        records.push(decoded_branch(first_join_start, second_join_start));
        records.push(decoded_branch(second_pred_start, second_join_start));
        records.push(decoded_end(second_join_start));

        let mut blocks = first_pred_starts
            .iter()
            .enumerate()
            .map(|(id, _)| BasicBlock {
                id,
                start: id,
                end: id + 1,
            })
            .collect::<Vec<_>>();
        let first_join = BasicBlock {
            id: first_pred_starts.len(),
            start: first_pred_starts.len(),
            end: first_pred_starts.len() + 1,
        };
        let second_pred = BasicBlock {
            id: first_pred_starts.len() + 1,
            start: first_pred_starts.len() + 1,
            end: first_pred_starts.len() + 2,
        };
        let second_join = BasicBlock {
            id: first_pred_starts.len() + 2,
            start: first_pred_starts.len() + 2,
            end: first_pred_starts.len() + 3,
        };
        blocks.push(first_join);
        blocks.push(second_pred);
        blocks.push(second_join);

        let mut old_start_to_block = first_pred_starts
            .iter()
            .enumerate()
            .map(|(id, start)| (*start, id))
            .collect::<HashMap<_, _>>();
        old_start_to_block.insert(first_join_start, first_join.id);
        old_start_to_block.insert(second_pred_start, second_pred.id);
        old_start_to_block.insert(second_join_start, second_join.id);

        let mut successors = vec![Vec::new(); blocks.len()];
        let mut predecessors = vec![Vec::new(); blocks.len()];
        for pred in 0..first_pred_starts.len() {
            successors[pred].push(first_join.id);
            predecessors[first_join.id].push(pred);
        }
        successors[first_join.id].push(second_join.id);
        predecessors[second_join.id].push(first_join.id);
        successors[second_pred.id].push(second_join.id);
        predecessors[second_join.id].push(second_pred.id);

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
    fn version_key_satisfies_accepts_superset_edge_facts() {
        let slot = LocalSlot::new(12, 4);
        let slot_shape = SlotShape {
            slot: Some(crate::parser::core::optimizer::expr::SlotRef::entry_local(
                slot,
            )),
            address: None,
            loop_value: None,
        };
        let selected = VersionKey {
            facts: vec![VersionFact::Slot(
                EntryBindingId::Local(slot),
                slot_shape.clone(),
            )],
            canonical: String::new(),
            score: 1,
        };
        let edge = VersionKey {
            facts: vec![
                VersionFact::Const(EntryBindingId::Local(slot), ConstValue::I32(7)),
                VersionFact::Slot(EntryBindingId::Local(slot), slot_shape),
            ],
            canonical: String::new(),
            score: 2,
        };
        assert!(version_key_satisfies(&edge, &selected));
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
            usage: BindingUsage::default(),
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
    fn build_candidate_for_pred_uses_memory_address_fallback_for_entry_local_slot() {
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
            usage: BindingUsage {
                memory_address: true,
            },
        };

        let candidate =
            build_candidate_for_pred(&exits, &graph, 0, &[binding]).expect("candidate must exist");
        assert!(candidate.key.facts.iter().any(|fact| {
            matches!(
                fact,
                VersionFact::Address(EntryBindingId::Local(found), shape)
                    if *found == slot
                        && *shape
                            == AddressShape::base_offset(AddressBaseKind::EntryLocal(slot), 0)
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
            usage: BindingUsage::default(),
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
    fn build_versioned_overlay_routes_specialized_block_into_specialized_successor() {
        let program = two_stage_join_program(&[0, 10], 20, 30, 40);
        let mut graph = ValueGraph::default();
        let slot = LocalSlot::new(0, 4);
        let stage1_arg =
            graph.ensure_block_argument(2, 0, ValType::I32, None, None, None, None, None);
        let stage2_local = graph.ensure_block_argument(
            4,
            local_binding_ordinal(slot),
            ValType::I32,
            None,
            None,
            None,
            None,
            None,
        );
        let stage2_result =
            graph.push_synthetic_specialized_value(4, 0, ValType::I32, None, None, None, None);
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
        let other_pred_value = graph.push_synthetic_specialized_value(
            3,
            0,
            ValType::I32,
            Some(ConstValue::I32(2)),
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
        let stage2_local_read = graph.push_synthetic_specialized_value(
            4,
            1,
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

        let mut entries = vec![BlockEntryState::default(); 5];
        entries[2].reachable = true;
        entries[2].stack.push(stage1_arg);
        entries[4].reachable = true;
        entries[4].locals.insert(slot, stage2_local);

        let mut exits = vec![BlockEntryState::default(); 5];
        exits[0].reachable = true;
        exits[0].stack.push(pred0_value);
        exits[1].reachable = true;
        exits[1].stack.push(pred1_value);
        exits[2].reachable = true;
        exits[2].locals.insert(slot, pred0_value);
        exits[3].reachable = true;
        exits[3].locals.insert(slot, other_pred_value);

        let base_bodies = vec![
            branch_body(0, 20),
            branch_body(10, 20),
            BlockBody {
                ops: vec![crate::parser::core::optimizer::pass::BlockOp {
                    source_start: Some(20),
                    op: vm::op_local_set4 as Op,
                    kind: BlockOpKind::LocalSet,
                    operands: vec![BlockOperand::LocalAddr(slot.addr)],
                    inputs: vec![stage1_arg],
                    values: Vec::new(),
                }],
                terminator: Some(crate::parser::core::optimizer::pass::BlockTerminator {
                    source_start: Some(21),
                    op: vm::op_br as Op,
                    kind: BlockTerminatorKind::Br,
                    operands: vec![BlockOperand::JumpTarget(40)],
                    inputs: Vec::new(),
                    values: Vec::new(),
                }),
            },
            branch_body(30, 40),
            BlockBody {
                ops: vec![crate::parser::core::optimizer::pass::BlockOp {
                    source_start: Some(40),
                    op: vm::op_i32_eqz as Op,
                    kind: BlockOpKind::PureUnary(PureOpKind::I32Eqz),
                    operands: Vec::new(),
                    inputs: vec![stage2_local_read],
                    values: vec![stage2_result],
                }],
                terminator: None,
            },
        ];

        let overlay = build_versioned_overlay(&program, &entries, &exits, &mut graph, &base_bodies);
        assert_eq!(overlay.specialized_block_count, 2);
        assert_eq!(overlay.specialized_to_specialized_edges, 1);
        assert_eq!(overlay.blocks[0].successors, vec![5]);
        assert_eq!(overlay.blocks[1].successors, vec![2]);
        assert_eq!(overlay.blocks[5].successors, vec![6]);
        assert_eq!(overlay.blocks[3].successors, vec![4]);
    }

    #[test]
    fn analyze_overlay_fixpoint_falls_back_to_initial_routes_when_budget_is_zero() {
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

        let selection = build_selection_plan(&program, &entries, &exits, &graph, &base_bodies);
        let mut blocks = Vec::new();
        let mut generic_indices = vec![0usize; program.blocks.len()];
        let mut specialized_indices = vec![None; program.blocks.len()];
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
                block_label: block_label_for(
                    &program,
                    block.id,
                    generic_idx,
                    BlockVersionKind::Generic,
                ),
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
            blocks.push(VersionedBlock {
                id: specialized_idx,
                version: VersionedBlockId {
                    original_block: block.id,
                    kind: BlockVersionKind::Specialized,
                },
                body: build_specialized_body(
                    &mut graph,
                    block.id,
                    &base_bodies[block.id],
                    &selected.bindings,
                ),
                block_label: block_label_for(
                    &program,
                    block.id,
                    specialized_idx,
                    BlockVersionKind::Specialized,
                ),
                fallthrough: None,
                successors: Vec::new(),
            });
        }
        for block in &mut blocks {
            if block.version.kind == BlockVersionKind::Specialized {
                retag_specialized_source_starts(block);
            }
        }

        let binding_cache = build_entry_binding_cache(&entries, &graph, &base_bodies);
        let mut edge_targets = build_initial_edge_targets(
            &program,
            &graph,
            &exits,
            &selection,
            &binding_cache,
            &blocks,
            &generic_indices,
            &specialized_indices,
        );
        let mut overlay = VersionedRewriteOverlay {
            blocks,
            reachable: Vec::new(),
            entries: vec![BlockEntryState::default(); 4],
            exits: vec![BlockEntryState::default(); 4],
            specialized_block_count: 1,
            generic_fallback_edges: 0,
            specialized_to_specialized_edges: 0,
            selected_cfg_specialized_edges: 1,
            selected_original_routable_edges: 1,
            fixpoint_iterations: 0,
            route_rewrites: 0,
            fixpoint_fallback: false,
            version_key_fact_breakdown: BTreeMap::new(),
            versionable_candidate_blocks: selection.versionable_candidate_blocks,
            blocks_with_entry_bindings: selection.blocks_with_entry_bindings,
            blocks_with_version_candidates: selection.blocks_with_version_candidates,
        };
        apply_edge_targets(&program, &base_bodies, &mut overlay, &edge_targets);
        analyze_overlay_fixpoint_with_limit(
            &program,
            &entries,
            &exits,
            &mut graph,
            &base_bodies,
            &binding_cache,
            &selection,
            &generic_indices,
            &specialized_indices,
            &mut overlay,
            &mut edge_targets,
            0,
        );
        assert!(overlay.fixpoint_fallback);
        assert_eq!(overlay.blocks[0].successors, vec![3]);
        assert_eq!(overlay.blocks[1].successors, vec![2]);
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
