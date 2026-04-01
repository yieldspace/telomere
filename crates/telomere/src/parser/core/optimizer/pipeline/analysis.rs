use std::collections::HashSet;

use super::ir::{CanonBlock, CanonFunc, CanonInst};
use crate::{
    common::{LoweredOperand, MemArg, Op, Operand, ValType},
    runtime::vm,
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct EffectVersionState {
    pub(crate) memory: u32,
    pub(crate) global: u32,
    pub(crate) table: u32,
    pub(crate) calls: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum ValueExprKey {
    Unary {
        op_addr: usize,
        src: ValueExprInput,
    },
    Binary {
        op_addr: usize,
        lhs: ValueExprInput,
        rhs: ValueExprInput,
    },
    I32LoadConstBase {
        memarg: [u8; 8],
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum ValueExprInput {
    Local { width: u32, encoded: [u8; 8] },
    Const { width: u32, encoded: [u8; 8] },
}

#[derive(Debug, Clone)]
pub(crate) struct ValueExprSite {
    pub(crate) cursor: usize,
    pub(crate) key: ValueExprKey,
    pub(crate) expr_len: usize,
    pub(crate) consumed: usize,
    pub(crate) result_ty: ValType,
    pub(crate) source_locals: Vec<u32>,
    pub(crate) written_local: Option<u32>,
    pub(crate) depends_on_effects: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct PreCandidate {
    pub(crate) block_id: usize,
    pub(crate) cursor: usize,
    pub(crate) site: ValueExprSite,
}

#[derive(Debug, Clone)]
pub(crate) struct AnalysisResults {
    pub(crate) reverse_postorder: Vec<usize>,
    pub(crate) idom: Vec<Option<usize>>,
    pub(crate) dominator_tree: Vec<Vec<usize>>,
    pub(crate) loop_headers: Vec<bool>,
    pub(crate) loop_depth: Vec<usize>,
    pub(crate) loop_parents: Vec<Option<usize>>,
    pub(crate) loop_members: Vec<Vec<usize>>,
    pub(crate) loop_preheaders: Vec<Option<usize>>,
    pub(crate) loop_written_locals: Vec<Vec<u32>>,
    pub(crate) live_in: Vec<Vec<ValType>>,
    pub(crate) live_out: Vec<Vec<ValType>>,
    pub(crate) live_locals_in: Vec<Vec<u32>>,
    pub(crate) live_locals_out: Vec<Vec<u32>>,
    pub(crate) block_entry_effects: Vec<EffectVersionState>,
    pub(crate) block_exit_effects: Vec<EffectVersionState>,
    pub(crate) inst_effects: Vec<Vec<EffectVersionState>>,
    pub(crate) gvn_sites: Vec<Vec<ValueExprSite>>,
    pub(crate) pre_candidates: Vec<Option<PreCandidate>>,
    pub(crate) coalescible_local_pairs: Vec<Vec<usize>>,
}

pub(crate) fn analyze(func: &CanonFunc) -> AnalysisResults {
    let reverse_postorder = compute_reverse_postorder(func);
    let idom = compute_idom(func, &reverse_postorder);
    let dominator_tree = build_dominator_tree(&idom);
    let loop_headers = func
        .blocks
        .iter()
        .map(|block| {
            block
                .predecessors
                .iter()
                .any(|pred| dominates(*pred, block.id, &idom))
        })
        .collect::<Vec<_>>();
    let loop_depth = compute_loop_depth(func, &idom, &loop_headers);
    let loop_members = compute_loop_members(func, &idom, &loop_headers);
    let loop_parents = compute_loop_parents(&loop_headers, &loop_members);
    let loop_preheaders = compute_loop_preheaders(func, &loop_members);
    let loop_written_locals = compute_loop_written_locals(func, &loop_members);
    let live_in = func
        .blocks
        .iter()
        .map(|block| block.params.iter().map(|param| param.ty).collect())
        .collect();
    let live_out = func
        .blocks
        .iter()
        .map(|block| {
            block
                .insts
                .last()
                .map(|inst| inst.stack_after.clone())
                .unwrap_or_default()
        })
        .collect();
    let (live_locals_in, live_locals_out) = compute_local_liveness(func, &reverse_postorder);
    let (block_entry_effects, block_exit_effects, inst_effects) =
        analyze_effect_versions(func, &reverse_postorder);
    let gvn_sites = compute_gvn_sites(func);
    let pre_candidates = compute_pre_candidates(func, &loop_members, &loop_written_locals);
    let coalescible_local_pairs = compute_coalescible_local_pairs(func);
    AnalysisResults {
        reverse_postorder,
        idom,
        dominator_tree,
        loop_headers,
        loop_depth,
        loop_parents,
        loop_members,
        loop_preheaders,
        loop_written_locals,
        live_in,
        live_out,
        live_locals_in,
        live_locals_out,
        block_entry_effects,
        block_exit_effects,
        inst_effects,
        gvn_sites,
        pre_candidates,
        coalescible_local_pairs,
    }
}

impl AnalysisResults {
    pub(crate) fn verify(&self, func: &CanonFunc) -> bool {
        let reachable = reachable_blocks(func);
        let rpo_is_valid = !self.reverse_postorder.is_empty()
            && self.reverse_postorder.first() == Some(&func.entry_block)
            && self.reverse_postorder.len() == reachable.len()
            && {
                let mut seen = HashSet::new();
                self.reverse_postorder
                    .iter()
                    .all(|block_id| *block_id < func.blocks.len() && seen.insert(*block_id))
            };
        let reachable_matches = rpo_is_valid
            && reachable
                .iter()
                .all(|block_id| self.reverse_postorder.contains(block_id));
        rpo_is_valid
            && reachable_matches
            && self.idom.len() == func.blocks.len()
            && self.dominator_tree.len() == func.blocks.len()
            && self.loop_headers.len() == func.blocks.len()
            && self.loop_depth.len() == func.blocks.len()
            && self.loop_parents.len() == func.blocks.len()
            && self.loop_members.len() == func.blocks.len()
            && self.loop_preheaders.len() == func.blocks.len()
            && self.loop_written_locals.len() == func.blocks.len()
            && self.live_in.len() == func.blocks.len()
            && self.live_out.len() == func.blocks.len()
            && self.live_locals_in.len() == func.blocks.len()
            && self.live_locals_out.len() == func.blocks.len()
            && self.block_entry_effects.len() == func.blocks.len()
            && self.block_exit_effects.len() == func.blocks.len()
            && self.inst_effects.len() == func.blocks.len()
            && self.gvn_sites.len() == func.blocks.len()
            && self.pre_candidates.len() == func.blocks.len()
            && self.coalescible_local_pairs.len() == func.blocks.len()
            && self
                .inst_effects
                .iter()
                .zip(&func.blocks)
                .all(|(effects, block)| effects.len() == block.insts.len())
            && self
                .gvn_sites
                .iter()
                .zip(&func.blocks)
                .all(|(sites, block)| {
                    sites.iter().all(|site| {
                        site.cursor < block.insts.len()
                            && site.expr_len > 0
                            && site.cursor + site.consumed <= block.insts.len()
                    })
                })
            && self
                .pre_candidates
                .iter()
                .enumerate()
                .all(|(header, candidate)| match candidate {
                    None => true,
                    Some(candidate) => {
                        self.loop_headers[header]
                            && candidate.block_id < func.blocks.len()
                            && candidate.cursor < func.blocks[candidate.block_id].insts.len()
                            && self.loop_members[header].contains(&candidate.block_id)
                    }
                })
            && self
                .coalescible_local_pairs
                .iter()
                .zip(&func.blocks)
                .all(|(pairs, block)| {
                    pairs
                        .iter()
                        .all(|cursor| cursor.saturating_add(1) < block.insts.len())
                })
    }
}

fn reachable_blocks(func: &CanonFunc) -> Vec<usize> {
    fn visit(block: usize, func: &CanonFunc, seen: &mut [bool], out: &mut Vec<usize>) {
        if seen[block] {
            return;
        }
        seen[block] = true;
        out.push(block);
        for succ in &func.blocks[block].successors {
            visit(*succ, func, seen, out);
        }
    }

    let mut seen = vec![false; func.blocks.len()];
    let mut out = Vec::with_capacity(func.blocks.len());
    visit(func.entry_block, func, &mut seen, &mut out);
    out
}

fn compute_reverse_postorder(func: &CanonFunc) -> Vec<usize> {
    fn visit(block: usize, func: &CanonFunc, seen: &mut [bool], out: &mut Vec<usize>) {
        if seen[block] {
            return;
        }
        seen[block] = true;
        for succ in &func.blocks[block].successors {
            visit(*succ, func, seen, out);
        }
        out.push(block);
    }

    let mut seen = vec![false; func.blocks.len()];
    let mut out = Vec::with_capacity(func.blocks.len());
    visit(func.entry_block, func, &mut seen, &mut out);
    out.reverse();
    out
}

fn compute_idom(func: &CanonFunc, rpo: &[usize]) -> Vec<Option<usize>> {
    let mut order = vec![usize::MAX; func.blocks.len()];
    for (index, block) in rpo.iter().copied().enumerate() {
        order[block] = index;
    }
    let mut idom = vec![None; func.blocks.len()];
    idom[func.entry_block] = Some(func.entry_block);
    let mut changed = true;
    while changed {
        changed = false;
        for &block in rpo.iter().skip(1) {
            let mut preds = func.blocks[block]
                .predecessors
                .iter()
                .copied()
                .filter(|pred| idom[*pred].is_some());
            let Some(mut new_idom) = preds.next() else {
                continue;
            };
            for pred in preds {
                new_idom = intersect(pred, new_idom, &idom, &order);
            }
            if idom[block] != Some(new_idom) {
                idom[block] = Some(new_idom);
                changed = true;
            }
        }
    }
    idom[func.entry_block] = None;
    idom
}

fn intersect(mut lhs: usize, mut rhs: usize, idom: &[Option<usize>], order: &[usize]) -> usize {
    while lhs != rhs {
        while order[lhs] > order[rhs] {
            lhs = idom[lhs].expect("dominators must converge");
        }
        while order[rhs] > order[lhs] {
            rhs = idom[rhs].expect("dominators must converge");
        }
    }
    lhs
}

fn build_dominator_tree(idom: &[Option<usize>]) -> Vec<Vec<usize>> {
    let mut tree = vec![Vec::new(); idom.len()];
    for (block, parent) in idom.iter().copied().enumerate() {
        if let Some(parent) = parent {
            tree[parent].push(block);
        }
    }
    tree
}

pub(crate) fn dominates(candidate: usize, block: usize, idom: &[Option<usize>]) -> bool {
    if candidate == block {
        return true;
    }
    let mut current = idom[block];
    while let Some(parent) = current {
        if parent == candidate {
            return true;
        }
        current = idom[parent];
    }
    false
}

fn compute_loop_depth(
    func: &CanonFunc,
    idom: &[Option<usize>],
    loop_headers: &[bool],
) -> Vec<usize> {
    (0..func.blocks.len())
        .map(|block| {
            let mut depth = 0usize;
            let mut current = Some(block);
            while let Some(node) = current {
                if loop_headers[node] && dominates(node, block, idom) {
                    depth += 1;
                }
                current = idom[node];
            }
            depth
        })
        .collect()
}

fn compute_loop_members(
    func: &CanonFunc,
    idom: &[Option<usize>],
    loop_headers: &[bool],
) -> Vec<Vec<usize>> {
    let mut out = vec![Vec::new(); func.blocks.len()];
    for header in 0..func.blocks.len() {
        if !loop_headers[header] {
            continue;
        }
        let mut members = HashSet::from([header]);
        let mut worklist = func.blocks[header]
            .predecessors
            .iter()
            .copied()
            .filter(|pred| dominates(header, *pred, idom))
            .collect::<Vec<_>>();
        while let Some(block) = worklist.pop() {
            if !members.insert(block) {
                continue;
            }
            for pred in &func.blocks[block].predecessors {
                if !members.contains(pred) {
                    worklist.push(*pred);
                }
            }
        }
        let mut members = members.into_iter().collect::<Vec<_>>();
        members.sort_unstable();
        out[header] = members;
    }
    out
}

fn compute_loop_parents(loop_headers: &[bool], loop_members: &[Vec<usize>]) -> Vec<Option<usize>> {
    let mut parents = vec![None; loop_headers.len()];
    for header in 0..loop_headers.len() {
        if !loop_headers[header] {
            continue;
        }
        let mut best_parent: Option<usize> = None;
        for candidate in 0..loop_headers.len() {
            if candidate == header || !loop_headers[candidate] {
                continue;
            }
            if !loop_members[candidate].contains(&header) {
                continue;
            }
            let candidate_size = loop_members[candidate].len();
            let better = best_parent
                .map(|current: usize| candidate_size < loop_members[current].len())
                .unwrap_or(true);
            if better {
                best_parent = Some(candidate);
            }
        }
        parents[header] = best_parent;
    }
    parents
}

fn compute_loop_preheaders(func: &CanonFunc, loop_members: &[Vec<usize>]) -> Vec<Option<usize>> {
    loop_members
        .iter()
        .enumerate()
        .map(|(header, members)| {
            if members.is_empty() {
                return None;
            }
            let outside_preds = func.blocks[header]
                .predecessors
                .iter()
                .copied()
                .filter(|pred| !members.contains(pred))
                .collect::<Vec<_>>();
            match outside_preds.as_slice() {
                [preheader] => Some(*preheader),
                _ => None,
            }
        })
        .collect()
}

fn compute_loop_written_locals(func: &CanonFunc, loop_members: &[Vec<usize>]) -> Vec<Vec<u32>> {
    loop_members
        .iter()
        .map(|members| {
            let mut written = HashSet::new();
            for block in members {
                for inst in &func.blocks[*block].insts {
                    if let Some(local_addr) = written_local_addr(inst) {
                        written.insert(local_addr);
                    }
                }
            }
            let mut written = written.into_iter().collect::<Vec<_>>();
            written.sort_unstable();
            written
        })
        .collect()
}

fn compute_gvn_sites(func: &CanonFunc) -> Vec<Vec<ValueExprSite>> {
    func.blocks
        .iter()
        .map(|block| {
            let mut sites = Vec::new();
            let mut cursor = 0usize;
            while cursor < block.insts.len() {
                let Some(site) = match_value_expr(block, cursor) else {
                    cursor += 1;
                    continue;
                };
                let consumed = site.consumed.max(1);
                sites.push(site);
                cursor += consumed;
            }
            sites
        })
        .collect()
}

fn compute_pre_candidates(
    func: &CanonFunc,
    loop_members: &[Vec<usize>],
    loop_written_locals: &[Vec<u32>],
) -> Vec<Option<PreCandidate>> {
    loop_members
        .iter()
        .enumerate()
        .map(|(header, members)| {
            if members.is_empty() {
                return None;
            }
            let loop_has_effect_writes = members.iter().any(|block_id| {
                func.blocks[*block_id]
                    .insts
                    .iter()
                    .any(|inst| writes_effect_state(inst.op))
            });
            find_pre_candidate(
                func,
                header,
                members,
                &loop_written_locals[header],
                loop_has_effect_writes,
            )
        })
        .collect()
}

fn find_pre_candidate(
    func: &CanonFunc,
    header: usize,
    loop_members: &[usize],
    loop_written_locals: &[u32],
    loop_has_effect_writes: bool,
) -> Option<PreCandidate> {
    if !loop_members.contains(&header) {
        return None;
    }
    let block = &func.blocks[header];
    for site in compute_block_value_expr_sites(block) {
        let local_stable = site
            .source_locals
            .iter()
            .all(|local_addr| !loop_written_locals.contains(local_addr));
        if local_stable && (!site.depends_on_effects || !loop_has_effect_writes) {
            return Some(PreCandidate {
                block_id: header,
                cursor: site.cursor,
                site,
            });
        }
    }
    None
}

fn compute_coalescible_local_pairs(func: &CanonFunc) -> Vec<Vec<usize>> {
    func.blocks
        .iter()
        .map(|block| {
            let mut cursors = Vec::new();
            for cursor in 0..block.insts.len().saturating_sub(1) {
                if coalescible_pair(&block.insts[cursor], &block.insts[cursor + 1]) {
                    cursors.push(cursor);
                }
            }
            cursors
        })
        .collect()
}

fn compute_block_value_expr_sites(block: &CanonBlock) -> Vec<ValueExprSite> {
    let mut sites = Vec::new();
    let mut cursor = 0usize;
    while cursor < block.insts.len() {
        let Some(site) = match_value_expr(block, cursor) else {
            cursor += 1;
            continue;
        };
        let consumed = site.consumed.max(1);
        sites.push(site);
        cursor += consumed;
    }
    sites
}

fn match_value_expr(block: &CanonBlock, cursor: usize) -> Option<ValueExprSite> {
    match_cacheable_unary(block, cursor)
        .or_else(|| match_cacheable_numeric(block, cursor))
        .or_else(|| match_cacheable_const_base_load(block, cursor))
}

fn match_cacheable_unary(block: &CanonBlock, cursor: usize) -> Option<ValueExprSite> {
    let src = block.insts.get(cursor)?;
    let unary = block.insts.get(cursor + 1)?;
    let result_ty = cacheable_unary_result_ty(unary.op)?;
    let width = result_ty.stack_size().u32();
    let (local_addr, encoded) = raw_local_get_key(src, width)?;
    let consumer_len = consumer_len(block, cursor + 2, result_ty);
    Some(ValueExprSite {
        cursor,
        key: ValueExprKey::Unary {
            op_addr: unary.op as usize,
            src: ValueExprInput::Local { width, encoded },
        },
        expr_len: 2,
        consumed: 2 + consumer_len,
        result_ty,
        source_locals: vec![local_addr],
        written_local: consumer_written_local(block, cursor + 2, result_ty),
        depends_on_effects: false,
    })
}

fn match_cacheable_numeric(block: &CanonBlock, cursor: usize) -> Option<ValueExprSite> {
    let lhs = block.insts.get(cursor)?;
    let rhs = block.insts.get(cursor + 1)?;
    let numeric = block.insts.get(cursor + 2)?;
    let (input_width, result_ty) = cacheable_numeric_signature(numeric.op)?;
    let (lhs_key, lhs_local) = cacheable_numeric_input(lhs, input_width)?;
    let (rhs_key, rhs_local) = cacheable_numeric_input(rhs, input_width)?;
    let consumer_len = consumer_len(block, cursor + 3, result_ty);
    let mut source_locals = Vec::new();
    if let Some(local) = lhs_local {
        source_locals.push(local);
    }
    if let Some(local) = rhs_local {
        source_locals.push(local);
    }
    Some(ValueExprSite {
        cursor,
        key: ValueExprKey::Binary {
            op_addr: numeric.op as usize,
            lhs: lhs_key,
            rhs: rhs_key,
        },
        expr_len: 3,
        consumed: 3 + consumer_len,
        result_ty,
        source_locals,
        written_local: consumer_written_local(block, cursor + 3, result_ty),
        depends_on_effects: false,
    })
}

fn match_cacheable_const_base_load(block: &CanonBlock, cursor: usize) -> Option<ValueExprSite> {
    let base = block.insts.get(cursor)?;
    let load = block.insts.get(cursor + 1)?;
    if !base.op_eq(vm::op_i32_const as Op) || !load.op_eq(vm::op_i32_load as Op) {
        return None;
    }
    let folded = fold_const_base_memarg(base.operands.first(), load.operands.first())?;
    let consumer_len = consumer_len(block, cursor + 2, ValType::I32);
    Some(ValueExprSite {
        cursor,
        key: ValueExprKey::I32LoadConstBase { memarg: folded },
        expr_len: 2,
        consumed: 2 + consumer_len,
        result_ty: ValType::I32,
        source_locals: Vec::new(),
        written_local: consumer_written_local(block, cursor + 2, ValType::I32),
        depends_on_effects: true,
    })
}

fn cacheable_numeric_input(
    inst: &CanonInst,
    input_width: u32,
) -> Option<(ValueExprInput, Option<u32>)> {
    if let Some((local_addr, encoded)) = raw_local_get_key(inst, input_width) {
        return Some((
            ValueExprInput::Local {
                width: input_width,
                encoded,
            },
            Some(local_addr),
        ));
    }
    let operand = const_operand_for_width(inst, input_width)?;
    Some((
        ValueExprInput::Const {
            width: input_width,
            encoded: operand,
        },
        None,
    ))
}

fn consumer_len(block: &CanonBlock, consumer_cursor: usize, result_ty: ValType) -> usize {
    if let Some(consumer) = block.insts.get(consumer_cursor) {
        if consumer_matches(consumer, result_ty) {
            return 1;
        }
    }
    0
}

fn consumer_matches(inst: &CanonInst, result_ty: ValType) -> bool {
    let width = result_ty.stack_size().u32();
    matches_local_consumer(inst, width)
        || (result_ty == ValType::I32 && inst.op_eq(vm::op_br_if as Op))
}

fn consumer_written_local(
    block: &CanonBlock,
    consumer_cursor: usize,
    result_ty: ValType,
) -> Option<u32> {
    let consumer = block.insts.get(consumer_cursor)?;
    if !matches_local_consumer(consumer, result_ty.stack_size().u32()) {
        return None;
    }
    raw_local_addr(consumer.operands.first())
}

fn matches_local_consumer(inst: &CanonInst, width: u32) -> bool {
    match width {
        4 => inst.op_eq(vm::op_local_set4 as Op) || inst.op_eq(vm::op_local_tee4 as Op),
        8 => inst.op_eq(vm::op_local_set8 as Op) || inst.op_eq(vm::op_local_tee8 as Op),
        16 => inst.op_eq(vm::op_local_set16 as Op) || inst.op_eq(vm::op_local_tee16 as Op),
        _ => false,
    }
}

fn cacheable_unary_result_ty(op: Op) -> Option<ValType> {
    Some(
        if std::ptr::fn_addr_eq(op, vm::op_i32_clz as Op)
            || std::ptr::fn_addr_eq(op, vm::op_i32_ctz as Op)
            || std::ptr::fn_addr_eq(op, vm::op_i32_popcnt as Op)
        {
            ValType::I32
        } else if std::ptr::fn_addr_eq(op, vm::op_i64_clz as Op)
            || std::ptr::fn_addr_eq(op, vm::op_i64_ctz as Op)
            || std::ptr::fn_addr_eq(op, vm::op_i64_popcnt as Op)
        {
            ValType::I64
        } else if std::ptr::fn_addr_eq(op, vm::op_f32_abs as Op)
            || std::ptr::fn_addr_eq(op, vm::op_f32_neg as Op)
            || std::ptr::fn_addr_eq(op, vm::op_f32_sqrt as Op)
            || std::ptr::fn_addr_eq(op, vm::op_f32_ceil as Op)
            || std::ptr::fn_addr_eq(op, vm::op_f32_floor as Op)
            || std::ptr::fn_addr_eq(op, vm::op_f32_trunc as Op)
            || std::ptr::fn_addr_eq(op, vm::op_f32_nearest as Op)
        {
            ValType::F32
        } else if std::ptr::fn_addr_eq(op, vm::op_f64_abs as Op)
            || std::ptr::fn_addr_eq(op, vm::op_f64_neg as Op)
            || std::ptr::fn_addr_eq(op, vm::op_f64_sqrt as Op)
            || std::ptr::fn_addr_eq(op, vm::op_f64_ceil as Op)
            || std::ptr::fn_addr_eq(op, vm::op_f64_floor as Op)
            || std::ptr::fn_addr_eq(op, vm::op_f64_trunc as Op)
            || std::ptr::fn_addr_eq(op, vm::op_f64_nearest as Op)
        {
            ValType::F64
        } else {
            return None;
        },
    )
}

fn cacheable_numeric_signature(op: Op) -> Option<(u32, ValType)> {
    Some(
        if std::ptr::fn_addr_eq(op, vm::op_i32_add as Op)
            || std::ptr::fn_addr_eq(op, vm::op_i32_sub as Op)
            || std::ptr::fn_addr_eq(op, vm::op_i32_mul as Op)
            || std::ptr::fn_addr_eq(op, vm::op_i32_and as Op)
            || std::ptr::fn_addr_eq(op, vm::op_i32_or as Op)
            || std::ptr::fn_addr_eq(op, vm::op_i32_xor as Op)
            || std::ptr::fn_addr_eq(op, vm::op_i32_shl as Op)
            || std::ptr::fn_addr_eq(op, vm::op_i32_shr_s as Op)
            || std::ptr::fn_addr_eq(op, vm::op_i32_shr_u as Op)
            || std::ptr::fn_addr_eq(op, vm::op_i32_rotl as Op)
            || std::ptr::fn_addr_eq(op, vm::op_i32_rotr as Op)
        {
            (4, ValType::I32)
        } else if std::ptr::fn_addr_eq(op, vm::op_i64_add as Op)
            || std::ptr::fn_addr_eq(op, vm::op_i64_sub as Op)
            || std::ptr::fn_addr_eq(op, vm::op_i64_mul as Op)
            || std::ptr::fn_addr_eq(op, vm::op_i64_and as Op)
            || std::ptr::fn_addr_eq(op, vm::op_i64_or as Op)
            || std::ptr::fn_addr_eq(op, vm::op_i64_xor as Op)
            || std::ptr::fn_addr_eq(op, vm::op_i64_shl as Op)
            || std::ptr::fn_addr_eq(op, vm::op_i64_shr_s as Op)
            || std::ptr::fn_addr_eq(op, vm::op_i64_shr_u as Op)
            || std::ptr::fn_addr_eq(op, vm::op_i64_rotl as Op)
            || std::ptr::fn_addr_eq(op, vm::op_i64_rotr as Op)
        {
            (8, ValType::I64)
        } else if std::ptr::fn_addr_eq(op, vm::op_f32_add as Op)
            || std::ptr::fn_addr_eq(op, vm::op_f32_sub as Op)
            || std::ptr::fn_addr_eq(op, vm::op_f32_mul as Op)
            || std::ptr::fn_addr_eq(op, vm::op_f32_div as Op)
        {
            (4, ValType::F32)
        } else if std::ptr::fn_addr_eq(op, vm::op_f64_add as Op)
            || std::ptr::fn_addr_eq(op, vm::op_f64_sub as Op)
            || std::ptr::fn_addr_eq(op, vm::op_f64_mul as Op)
            || std::ptr::fn_addr_eq(op, vm::op_f64_div as Op)
        {
            (8, ValType::F64)
        } else if std::ptr::fn_addr_eq(op, vm::op_i32_eq as Op)
            || std::ptr::fn_addr_eq(op, vm::op_i32_ne as Op)
            || std::ptr::fn_addr_eq(op, vm::op_i32_lt_s as Op)
            || std::ptr::fn_addr_eq(op, vm::op_i32_lt_u as Op)
            || std::ptr::fn_addr_eq(op, vm::op_i32_gt_s as Op)
            || std::ptr::fn_addr_eq(op, vm::op_i32_gt_u as Op)
            || std::ptr::fn_addr_eq(op, vm::op_i32_le_s as Op)
            || std::ptr::fn_addr_eq(op, vm::op_i32_le_u as Op)
            || std::ptr::fn_addr_eq(op, vm::op_i32_ge_s as Op)
            || std::ptr::fn_addr_eq(op, vm::op_i32_ge_u as Op)
            || std::ptr::fn_addr_eq(op, vm::op_f32_eq as Op)
            || std::ptr::fn_addr_eq(op, vm::op_f32_ne as Op)
            || std::ptr::fn_addr_eq(op, vm::op_f32_lt as Op)
            || std::ptr::fn_addr_eq(op, vm::op_f32_gt as Op)
            || std::ptr::fn_addr_eq(op, vm::op_f32_le as Op)
            || std::ptr::fn_addr_eq(op, vm::op_f32_ge as Op)
        {
            (4, ValType::I32)
        } else if std::ptr::fn_addr_eq(op, vm::op_i64_eq as Op)
            || std::ptr::fn_addr_eq(op, vm::op_i64_ne as Op)
            || std::ptr::fn_addr_eq(op, vm::op_i64_lt_s as Op)
            || std::ptr::fn_addr_eq(op, vm::op_i64_lt_u as Op)
            || std::ptr::fn_addr_eq(op, vm::op_i64_gt_s as Op)
            || std::ptr::fn_addr_eq(op, vm::op_i64_gt_u as Op)
            || std::ptr::fn_addr_eq(op, vm::op_i64_le_s as Op)
            || std::ptr::fn_addr_eq(op, vm::op_i64_le_u as Op)
            || std::ptr::fn_addr_eq(op, vm::op_i64_ge_s as Op)
            || std::ptr::fn_addr_eq(op, vm::op_i64_ge_u as Op)
            || std::ptr::fn_addr_eq(op, vm::op_f64_eq as Op)
            || std::ptr::fn_addr_eq(op, vm::op_f64_ne as Op)
            || std::ptr::fn_addr_eq(op, vm::op_f64_lt as Op)
            || std::ptr::fn_addr_eq(op, vm::op_f64_gt as Op)
            || std::ptr::fn_addr_eq(op, vm::op_f64_le as Op)
            || std::ptr::fn_addr_eq(op, vm::op_f64_ge as Op)
        {
            (8, ValType::I32)
        } else {
            return None;
        },
    )
}

fn raw_local_get_key(inst: &CanonInst, width: u32) -> Option<(u32, [u8; 8])> {
    let expected = match width {
        4 => vm::op_local_get4 as Op,
        8 => vm::op_local_get8 as Op,
        16 => vm::op_local_get16 as Op,
        _ => return None,
    };
    if !inst.op_eq(expected) {
        return None;
    }
    let LoweredOperand::Raw(encoded) = inst.operands.first()? else {
        return None;
    };
    Some((
        unsafe { Operand { encoded: *encoded }.local_addr },
        *encoded,
    ))
}

fn const_operand_for_width(inst: &CanonInst, width: u32) -> Option<[u8; 8]> {
    let matches = match width {
        4 => inst.op_eq(vm::op_i32_const as Op) || inst.op_eq(vm::op_f32_const as Op),
        8 => inst.op_eq(vm::op_i64_const as Op) || inst.op_eq(vm::op_f64_const as Op),
        _ => false,
    };
    if !matches {
        return None;
    }
    let LoweredOperand::Raw(encoded) = inst.operands.first()? else {
        return None;
    };
    Some(*encoded)
}

fn raw_i32(operand: Option<&LoweredOperand>) -> Option<i32> {
    let LoweredOperand::Raw(encoded) = operand? else {
        return None;
    };
    Some(unsafe { Operand { encoded: *encoded }.i32 })
}

fn raw_memarg(operand: Option<&LoweredOperand>) -> Option<MemArg> {
    let LoweredOperand::Raw(encoded) = operand? else {
        return None;
    };
    Some(unsafe { Operand { encoded: *encoded }.memarg })
}

fn fold_const_base_memarg(
    base: Option<&LoweredOperand>,
    memarg: Option<&LoweredOperand>,
) -> Option<[u8; 8]> {
    let base = raw_i32(base)? as u32;
    let mut memarg = raw_memarg(memarg)?;
    memarg.offset = memarg.offset.wrapping_add(base);
    Some(unsafe { Operand { memarg }.encoded })
}

fn coalescible_pair(set: &CanonInst, get: &CanonInst) -> bool {
    let teeable = (set.op_eq(vm::op_local_set4 as Op) && get.op_eq(vm::op_local_get4 as Op))
        || (set.op_eq(vm::op_local_set8 as Op) && get.op_eq(vm::op_local_get8 as Op))
        || (set.op_eq(vm::op_local_set16 as Op) && get.op_eq(vm::op_local_get16 as Op));
    teeable
        && match (set.operands.first(), get.operands.first()) {
            (Some(lhs), Some(rhs)) => same_operand(lhs, rhs),
            _ => false,
        }
}

fn same_operand(lhs: &LoweredOperand, rhs: &LoweredOperand) -> bool {
    match (lhs, rhs) {
        (LoweredOperand::Raw(lhs), LoweredOperand::Raw(rhs)) => lhs == rhs,
        (LoweredOperand::JumpTarget(lhs), LoweredOperand::JumpTarget(rhs)) => lhs == rhs,
        (LoweredOperand::ConstPoolRef(lhs), LoweredOperand::ConstPoolRef(rhs)) => lhs == rhs,
        (LoweredOperand::CallRecipeRef(lhs), LoweredOperand::CallRecipeRef(rhs)) => {
            lhs.funcidx == rhs.funcidx && lhs.resolved_recipe_slot() == rhs.resolved_recipe_slot()
        }
        _ => false,
    }
}

fn compute_local_liveness(
    func: &CanonFunc,
    reverse_postorder: &[usize],
) -> (Vec<Vec<u32>>, Vec<Vec<u32>>) {
    let (uses, defs): (Vec<_>, Vec<_>) = func.blocks.iter().map(block_local_use_def).unzip();
    let mut live_in = vec![HashSet::<u32>::new(); func.blocks.len()];
    let mut live_out = vec![HashSet::<u32>::new(); func.blocks.len()];

    let mut changed = true;
    while changed {
        changed = false;
        for &block_id in reverse_postorder.iter().rev() {
            let mut next_out = HashSet::new();
            for succ in &func.blocks[block_id].successors {
                next_out.extend(live_in[*succ].iter().copied());
            }

            let mut next_in = uses[block_id].clone();
            next_in.extend(
                next_out
                    .iter()
                    .copied()
                    .filter(|local| !defs[block_id].contains(local)),
            );

            if live_out[block_id] != next_out || live_in[block_id] != next_in {
                live_out[block_id] = next_out;
                live_in[block_id] = next_in;
                changed = true;
            }
        }
    }

    (sort_local_sets(live_in), sort_local_sets(live_out))
}

fn block_local_use_def(block: &super::ir::CanonBlock) -> (HashSet<u32>, HashSet<u32>) {
    let mut uses = HashSet::new();
    let mut defs = HashSet::new();
    for inst in &block.insts {
        if let Some(local_addr) = read_local_addr(inst) {
            if !defs.contains(&local_addr) {
                uses.insert(local_addr);
            }
        }
        if let Some(local_addr) = written_local_addr(inst) {
            defs.insert(local_addr);
        }
    }
    (uses, defs)
}

fn sort_local_sets(sets: Vec<HashSet<u32>>) -> Vec<Vec<u32>> {
    sets.into_iter()
        .map(|set| {
            let mut locals = set.into_iter().collect::<Vec<_>>();
            locals.sort_unstable();
            locals
        })
        .collect()
}

fn read_local_addr(inst: &CanonInst) -> Option<u32> {
    if inst.op_eq(vm::op_local_get4 as Op)
        || inst.op_eq(vm::op_local_get8 as Op)
        || inst.op_eq(vm::op_local_get16 as Op)
    {
        return raw_local_addr(inst.operands.first());
    }
    None
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

fn raw_local_addr(operand: Option<&crate::common::LoweredOperand>) -> Option<u32> {
    let crate::common::LoweredOperand::Raw(encoded) = operand? else {
        return None;
    };
    Some(unsafe { crate::common::Operand { encoded: *encoded }.local_addr })
}

fn analyze_effect_versions(
    func: &CanonFunc,
    reverse_postorder: &[usize],
) -> (
    Vec<EffectVersionState>,
    Vec<EffectVersionState>,
    Vec<Vec<EffectVersionState>>,
) {
    let mut entry = vec![EffectVersionState::default(); func.blocks.len()];
    let mut exit = vec![EffectVersionState::default(); func.blocks.len()];
    for &block_id in reverse_postorder {
        let joined = join_predecessor_effects(block_id, func, &exit);
        entry[block_id] = joined;
        exit[block_id] = func.blocks[block_id]
            .insts
            .iter()
            .fold(joined, |state, inst| advance_effect_state(state, inst.op));
    }

    let inst_effects = func
        .blocks
        .iter()
        .map(|block| {
            let mut state = entry[block.id];
            block
                .insts
                .iter()
                .map(|inst| {
                    state = advance_effect_state(state, inst.op);
                    state
                })
                .collect()
        })
        .collect();

    (entry, exit, inst_effects)
}

fn join_predecessor_effects(
    block_id: usize,
    func: &CanonFunc,
    exit_effects: &[EffectVersionState],
) -> EffectVersionState {
    let Some((&first, rest)) = func.blocks[block_id].predecessors.split_first() else {
        return EffectVersionState::default();
    };
    rest.iter()
        .fold(exit_effects[first], |state, pred| EffectVersionState {
            memory: state.memory.max(exit_effects[*pred].memory),
            global: state.global.max(exit_effects[*pred].global),
            table: state.table.max(exit_effects[*pred].table),
            calls: state.calls.max(exit_effects[*pred].calls),
        })
}

fn advance_effect_state(
    mut state: EffectVersionState,
    op: crate::common::Op,
) -> EffectVersionState {
    if is_memory_store(op) {
        state.memory = state.memory.saturating_add(1);
    }
    if is_global_write(op) {
        state.global = state.global.saturating_add(1);
    }
    if is_table_write(op) {
        state.table = state.table.saturating_add(1);
    }
    if is_call(op) {
        state.calls = state.calls.saturating_add(1);
    }
    state
}

fn writes_effect_state(op: crate::common::Op) -> bool {
    is_memory_store(op) || is_global_write(op) || is_table_write(op) || is_call(op)
}

fn is_memory_store(op: crate::common::Op) -> bool {
    let labels = [
        vm::op_i32_store as crate::common::Op,
        vm::op_i32_store8 as crate::common::Op,
        vm::op_i32_store16 as crate::common::Op,
        vm::op_i64_store as crate::common::Op,
        vm::op_i64_store8 as crate::common::Op,
        vm::op_i64_store16 as crate::common::Op,
        vm::op_i64_store32 as crate::common::Op,
        vm::op_f32_store as crate::common::Op,
        vm::op_f64_store as crate::common::Op,
    ];
    labels
        .iter()
        .any(|candidate| std::ptr::fn_addr_eq(*candidate, op))
}

fn is_global_write(op: crate::common::Op) -> bool {
    let labels = [
        vm::op_global_set4 as crate::common::Op,
        vm::op_global_set8 as crate::common::Op,
        vm::op_global_set16 as crate::common::Op,
    ];
    labels
        .iter()
        .any(|candidate| std::ptr::fn_addr_eq(*candidate, op))
}

fn is_table_write(op: crate::common::Op) -> bool {
    let labels = [
        vm::op_table_set as crate::common::Op,
        vm::op_table_init as crate::common::Op,
        vm::op_table_copy as crate::common::Op,
        vm::op_table_fill as crate::common::Op,
    ];
    labels
        .iter()
        .any(|candidate| std::ptr::fn_addr_eq(*candidate, op))
}

fn is_call(op: crate::common::Op) -> bool {
    let labels = [
        vm::op_call as crate::common::Op,
        vm::op_call_import as crate::common::Op,
        vm::op_return_call as crate::common::Op,
        vm::op_return_call_import as crate::common::Op,
        vm::op_call_indirect as crate::common::Op,
        vm::op_return_call_indirect as crate::common::Op,
    ];
    labels
        .iter()
        .any(|candidate| std::ptr::fn_addr_eq(*candidate, op))
}

trait CanonInstExt {
    fn op_eq(&self, candidate: Op) -> bool;
}

impl CanonInstExt for CanonInst {
    fn op_eq(&self, candidate: Op) -> bool {
        std::ptr::fn_addr_eq(self.op, candidate)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::{FuncIdx, FuncType, LoweredOperand};
    use crate::parser::core::optimizer::pipeline::ir::{
        BlockParam, EffectId, InstId, StorageClass, ValueId,
    };

    fn raw_local_operand(local_addr: u32) -> LoweredOperand {
        LoweredOperand::Raw(unsafe { Operand { local_addr }.encoded })
    }

    fn raw_i32_operand(value: i32) -> LoweredOperand {
        LoweredOperand::Raw(unsafe { Operand { i32: value }.encoded })
    }

    fn inst(
        id: usize,
        op: Op,
        operands: Vec<LoweredOperand>,
        stack_before: Vec<ValType>,
        stack_after: Vec<ValType>,
    ) -> CanonInst {
        CanonInst {
            id: InstId(id),
            op,
            operands,
            stack_before,
            stack_after,
            preserved_prefix_len: 0,
            fresh_result_count: 1,
            effect: EffectId(id),
        }
    }

    fn empty_param(index: usize, ty: ValType) -> BlockParam {
        BlockParam {
            id: ValueId(index),
            index,
            ty,
            storage: StorageClass::BlockParam,
        }
    }

    #[test]
    fn analysis_emits_gvn_sites_and_coalesce_pairs() {
        let func = CanonFunc {
            funcidx: FuncIdx(0),
            functype: FuncType::new(vec![], vec![]),
            locals_size: 12,
            entry_block: 0,
            blocks: vec![super::CanonBlock {
                id: 0,
                params: vec![],
                predecessors: vec![],
                successors: vec![],
                insts: vec![
                    inst(
                        0,
                        vm::op_local_get4 as Op,
                        vec![raw_local_operand(0)],
                        vec![],
                        vec![ValType::I32],
                    ),
                    inst(
                        1,
                        vm::op_local_get4 as Op,
                        vec![raw_local_operand(4)],
                        vec![ValType::I32],
                        vec![ValType::I32, ValType::I32],
                    ),
                    inst(
                        2,
                        vm::op_i32_add as Op,
                        vec![],
                        vec![ValType::I32, ValType::I32],
                        vec![ValType::I32],
                    ),
                    inst(
                        3,
                        vm::op_local_get4 as Op,
                        vec![raw_local_operand(0)],
                        vec![ValType::I32],
                        vec![ValType::I32, ValType::I32],
                    ),
                    inst(
                        4,
                        vm::op_local_get4 as Op,
                        vec![raw_local_operand(4)],
                        vec![ValType::I32, ValType::I32],
                        vec![ValType::I32, ValType::I32, ValType::I32],
                    ),
                    inst(
                        5,
                        vm::op_i32_add as Op,
                        vec![],
                        vec![ValType::I32, ValType::I32, ValType::I32],
                        vec![ValType::I32, ValType::I32],
                    ),
                    inst(
                        6,
                        vm::op_local_set4 as Op,
                        vec![raw_local_operand(8)],
                        vec![ValType::I32, ValType::I32],
                        vec![ValType::I32],
                    ),
                    inst(
                        7,
                        vm::op_local_get4 as Op,
                        vec![raw_local_operand(8)],
                        vec![ValType::I32],
                        vec![ValType::I32, ValType::I32],
                    ),
                ],
            }],
        };

        let analysis = analyze(&func);
        assert_eq!(analysis.gvn_sites[0].len(), 2);
        assert_eq!(analysis.coalescible_local_pairs[0], vec![6]);
    }

    #[test]
    fn analysis_emits_loop_pre_candidate() {
        let func = CanonFunc {
            funcidx: FuncIdx(0),
            functype: FuncType::new(vec![ValType::I32], vec![]),
            locals_size: 4,
            entry_block: 0,
            blocks: vec![
                super::CanonBlock {
                    id: 0,
                    params: vec![],
                    predecessors: vec![],
                    successors: vec![1],
                    insts: vec![],
                },
                super::CanonBlock {
                    id: 1,
                    params: vec![empty_param(0, ValType::I32)],
                    predecessors: vec![0, 1],
                    successors: vec![1],
                    insts: vec![
                        inst(
                            0,
                            vm::op_local_get4 as Op,
                            vec![raw_local_operand(0)],
                            vec![],
                            vec![ValType::I32],
                        ),
                        inst(
                            1,
                            vm::op_i32_const as Op,
                            vec![raw_i32_operand(1)],
                            vec![ValType::I32],
                            vec![ValType::I32, ValType::I32],
                        ),
                        inst(
                            2,
                            vm::op_i32_add as Op,
                            vec![],
                            vec![ValType::I32, ValType::I32],
                            vec![ValType::I32],
                        ),
                        inst(
                            3,
                            vm::op_br_if as Op,
                            vec![LoweredOperand::JumpTarget(1)],
                            vec![ValType::I32],
                            vec![],
                        ),
                    ],
                },
            ],
        };

        let analysis = analyze(&func);
        let candidate = analysis.pre_candidates[1]
            .as_ref()
            .expect("loop header must have a hoist candidate");
        assert_eq!(candidate.block_id, 1);
        assert_eq!(candidate.cursor, 0);
        assert_eq!(analysis.loop_preheaders[1], Some(0));
    }
}
