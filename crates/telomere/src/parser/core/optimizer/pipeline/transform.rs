use std::collections::{HashMap, HashSet};

use super::{
    analysis::{self, AnalysisResults, PreCandidate, ValueExprKey, ValueExprSite},
    ir::CanonFunc,
    select::{
        is_i32_memory_load_root_op, scalar_memory_load_type, scalar_memory_store_type, ScalarType,
    },
};
use crate::{
    common::{LocalsData, LoweredOperand, Op, Operand, ValType},
    runtime::vm,
};

const I32_SELECT_BIT_STEP_MASK_SHIFTED: u32 = 1 << 0;
const I32_SELECT_BIT_STEP_EQ_CONDITION: u32 = 1 << 1;
const I32_SELECT_BIT_STEP_TEE_DST: u32 = 1 << 2;

#[derive(Debug, Clone)]
pub(crate) struct TransformResult {
    pub(crate) func: CanonFunc,
    pub(crate) preserved_entry: bool,
    pub(crate) verified_cfg: bool,
    pub(crate) verified_stack: bool,
    #[allow(dead_code)]
    pub(crate) canonicalized_branches: usize,
    #[allow(dead_code)]
    pub(crate) folded_consts: usize,
    #[allow(dead_code)]
    pub(crate) cached_exprs: usize,
    #[allow(dead_code)]
    pub(crate) coalesced_slots: usize,
    #[allow(dead_code)]
    pub(crate) pre_hoists: usize,
    #[allow(dead_code)]
    pub(crate) buffered_memory_roots: usize,
}

pub(crate) fn run(
    mut func: CanonFunc,
    locals: &mut LocalsData,
    _analysis: &AnalysisResults,
) -> TransformResult {
    let mut canonicalized_branches = 0usize;
    let mut folded_consts = 0usize;
    let mut cached_exprs = 0usize;
    let mut coalesced_slots = 0usize;
    let mut pre_hoists = 0usize;
    for block in &mut func.blocks {
        folded_consts += fold_const_ops(block);
        canonicalized_branches += fold_const_branches(block);
        fuse_i32_select_bit_steps(block);
    }
    normalize_after_cfg_rewrites(&mut func);
    pre_hoists += hoist_pre_candidates(&mut func, locals);
    let analysis = analysis::analyze(&func);
    for (block, sites) in func.blocks.iter_mut().zip(&analysis.gvn_sites) {
        cached_exprs += cache_redundant_local_exprs(block, locals, sites);
    }
    for (block, pair_cursors) in func
        .blocks
        .iter_mut()
        .zip(&analysis.coalescible_local_pairs)
    {
        coalesced_slots += coalesce_local_set_get(block, pair_cursors);
    }
    let mut buffered_memory_roots = 0usize;
    for block in &mut func.blocks {
        buffered_memory_roots += buffer_same_block_memory_derived_address_roots(block, locals);
    }
    func.locals_size = u32::try_from(locals.byte_size()).expect("locals size exceeds u32::MAX");
    normalize_after_cfg_rewrites(&mut func);
    let verified_stack = verify_block_stacks(&func);
    TransformResult {
        preserved_entry: func.entry_block == 0,
        verified_cfg: func.verify(),
        verified_stack,
        func,
        canonicalized_branches,
        folded_consts,
        cached_exprs,
        coalesced_slots,
        pre_hoists,
        buffered_memory_roots,
    }
}

impl TransformResult {
    pub(crate) fn verify(&self) -> bool {
        self.preserved_entry && self.verified_cfg && self.verified_stack && self.func.verify()
    }
}

#[derive(Debug, Clone)]
struct AvailableExpr {
    def_cursor: usize,
    source_locals: Vec<u32>,
    depends_on_effects: bool,
}

fn fold_const_branches(block: &mut super::ir::CanonBlock) -> usize {
    let len = block.insts.len();
    if len >= 2
        && block.insts[len - 2].op_eq(vm::op_i32_const as Op)
        && block.insts[len - 1].op_eq(vm::op_br_if as Op)
    {
        let condition = raw_i32(block.insts[len - 2].operands.first());
        let target = jump_target(block.insts[len - 1].operands.first());
        if let (Some(condition), Some(target)) = (condition, target) {
            block.insts.pop();
            block.insts.pop();
            if condition != 0 {
                block.insts.push(make_br(target));
            }
            return 1;
        }
    }
    if len >= 3
        && block.insts[len - 3].op_eq(vm::op_i32_const as Op)
        && block.insts[len - 2].op_eq(vm::op_i32_eqz as Op)
        && block.insts[len - 1].op_eq(vm::op_br_if as Op)
    {
        let condition = raw_i32(block.insts[len - 3].operands.first());
        let target = jump_target(block.insts[len - 1].operands.first());
        if let (Some(condition), Some(target)) = (condition, target) {
            block.insts.pop();
            block.insts.pop();
            block.insts.pop();
            if condition == 0 {
                block.insts.push(make_br(target));
            }
            return 1;
        }
    }
    0
}

fn fold_const_ops(block: &mut super::ir::CanonBlock) -> usize {
    let mut folded = 0usize;
    let mut rewritten = Vec::with_capacity(block.insts.len());
    let mut cursor = 0usize;
    while cursor < block.insts.len() {
        if let Some((inst, consumed)) = fold_i32_const_window(&block.insts, cursor) {
            rewritten.push(inst);
            folded += 1;
            cursor += consumed;
            continue;
        }
        if let Some((inst, consumed)) = fold_i64_const_window(&block.insts, cursor) {
            rewritten.push(inst);
            folded += 1;
            cursor += consumed;
            continue;
        }
        rewritten.push(block.insts[cursor].clone());
        cursor += 1;
    }
    block.insts = rewritten;
    folded
}

fn cache_redundant_local_exprs(
    block: &mut super::ir::CanonBlock,
    locals: &mut LocalsData,
    sites: &[ValueExprSite],
) -> usize {
    let mut available = HashMap::<ValueExprKey, AvailableExpr>::new();
    let mut defs = HashMap::<usize, ValueExprSite>::new();
    let mut uses = HashMap::<usize, usize>::new();
    let site_by_cursor = sites
        .iter()
        .cloned()
        .map(|site| (site.cursor, site))
        .collect::<HashMap<_, _>>();

    let mut cursor = 0usize;
    while cursor < block.insts.len() {
        if let Some(site) = site_by_cursor.get(&cursor) {
            if let Some(available_expr) = available.get(&site.key) {
                uses.insert(cursor, available_expr.def_cursor);
            } else {
                available.insert(
                    site.key.clone(),
                    AvailableExpr {
                        def_cursor: cursor,
                        source_locals: site.source_locals.clone(),
                        depends_on_effects: site.depends_on_effects,
                    },
                );
                defs.insert(cursor, site.clone());
            }
            if let Some(written_local) = site.written_local {
                invalidate_cached_locals(&mut available, written_local);
            }
            cursor += site.consumed;
            continue;
        }

        let inst = &block.insts[cursor];
        if let Some(written_local) = written_local_addr(inst) {
            invalidate_cached_locals(&mut available, written_local);
        }
        if writes_effect_state(inst.op) {
            invalidate_effect_dependent_exprs(&mut available);
        }
        cursor += 1;
    }

    if uses.is_empty() {
        return 0;
    }

    let mut temp_for_def = HashMap::<usize, u32>::new();
    for def_cursor in uses.values().copied() {
        let site = defs
            .get(&def_cursor)
            .expect("cache use must reference a known definition");
        temp_for_def
            .entry(def_cursor)
            .or_insert_with(|| locals.allocate_temp_slot(site.result_ty));
    }

    let mut rewritten = Vec::with_capacity(block.insts.len() + temp_for_def.len());
    let mut cursor = 0usize;
    while cursor < block.insts.len() {
        if let Some(site) = defs.get(&cursor) {
            if let Some(&temp_addr) = temp_for_def.get(&cursor) {
                append_original_expr(&mut rewritten, &block.insts, cursor, site.expr_len);
                let expr_last = &block.insts[cursor + site.expr_len - 1];
                rewritten.push(make_temp_local_tee(temp_addr, site.result_ty, expr_last));
                append_consumer_if_present(
                    &mut rewritten,
                    &block.insts,
                    cursor,
                    site.expr_len,
                    site.consumed,
                );
                cursor += site.consumed;
                continue;
            }
        }
        if let Some(def_cursor) = uses.get(&cursor).copied() {
            let site = site_by_cursor
                .get(&cursor)
                .expect("cache hit must reference a known value expression");
            let temp_addr = temp_for_def[&def_cursor];
            let first = &block.insts[cursor];
            let expr_last = &block.insts[cursor + site.expr_len - 1];
            rewritten.push(make_temp_local_get(
                temp_addr,
                site.result_ty,
                first,
                expr_last,
            ));
            append_consumer_if_present(
                &mut rewritten,
                &block.insts,
                cursor,
                site.expr_len,
                site.consumed,
            );
            cursor += site.consumed;
            continue;
        }
        rewritten.push(block.insts[cursor].clone());
        cursor += 1;
    }

    block.insts = rewritten;
    uses.len()
}

fn append_original_expr(
    rewritten: &mut Vec<super::ir::CanonInst>,
    insts: &[super::ir::CanonInst],
    cursor: usize,
    expr_len: usize,
) {
    rewritten.extend(insts[cursor..cursor + expr_len].iter().cloned());
}

fn append_consumer_if_present(
    rewritten: &mut Vec<super::ir::CanonInst>,
    insts: &[super::ir::CanonInst],
    cursor: usize,
    expr_len: usize,
    consumed: usize,
) {
    if consumed > expr_len {
        rewritten.push(insts[cursor + expr_len].clone());
    }
}

fn invalidate_cached_locals(
    available: &mut HashMap<ValueExprKey, AvailableExpr>,
    written_local: u32,
) {
    available.retain(|_, cached| !cached.source_locals.contains(&written_local));
}

fn invalidate_effect_dependent_exprs(available: &mut HashMap<ValueExprKey, AvailableExpr>) {
    available.retain(|_, cached| !cached.depends_on_effects);
}

fn make_temp_local_get(
    local_addr: u32,
    ty: ValType,
    first: &super::ir::CanonInst,
    expr_last: &super::ir::CanonInst,
) -> super::ir::CanonInst {
    super::ir::CanonInst {
        id: first.id,
        op: local_get_op_for_ty(ty),
        operands: vec![raw_local_operand(local_addr)],
        stack_before: first.stack_before.clone(),
        stack_after: expr_last.stack_after.clone(),
        preserved_prefix_len: first.preserved_prefix_len,
        fresh_result_count: expr_last.fresh_result_count,
        effect: expr_last.effect,
    }
}

fn make_temp_local_tee(
    local_addr: u32,
    ty: ValType,
    expr_last: &super::ir::CanonInst,
) -> super::ir::CanonInst {
    super::ir::CanonInst {
        id: expr_last.id,
        op: local_tee_op_for_ty(ty),
        operands: vec![raw_local_operand(local_addr)],
        stack_before: expr_last.stack_after.clone(),
        stack_after: expr_last.stack_after.clone(),
        preserved_prefix_len: expr_last.stack_after.len().saturating_sub(1),
        fresh_result_count: 1,
        effect: expr_last.effect,
    }
}

fn make_temp_local_set(
    local_addr: u32,
    ty: ValType,
    producer: &super::ir::CanonInst,
) -> super::ir::CanonInst {
    super::ir::CanonInst {
        id: producer.id,
        op: local_set_op_for_ty(ty),
        operands: vec![raw_local_operand(local_addr)],
        stack_before: producer.stack_after.clone(),
        stack_after: producer.stack_before.clone(),
        preserved_prefix_len: producer.stack_before.len(),
        fresh_result_count: 0,
        effect: producer.effect,
    }
}

fn written_local_addr(inst: &super::ir::CanonInst) -> Option<u32> {
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

fn buffer_same_block_memory_derived_address_roots(
    block: &mut super::ir::CanonBlock,
    locals: &mut LocalsData,
) -> usize {
    let mut rewritten = Vec::with_capacity(block.insts.len());
    let mut buffered = 0usize;
    for (cursor, inst) in block.insts.iter().enumerate() {
        rewritten.push(inst.clone());
        if !is_i32_memory_load_root_op(inst.op) {
            continue;
        }
        if !matches_same_block_memory_derived_address_use(&block.insts, cursor) {
            continue;
        }
        let temp_addr = locals.allocate_temp_slot(ValType::I32);
        rewritten.push(make_temp_local_set(temp_addr, ValType::I32, inst));
        rewritten.push(make_temp_local_get(temp_addr, ValType::I32, inst, inst));
        buffered += 1;
    }
    if buffered != 0 {
        block.insts = rewritten;
    }
    buffered
}

fn matches_same_block_memory_derived_address_use(
    insts: &[super::ir::CanonInst],
    producer_cursor: usize,
) -> bool {
    let address_start = producer_cursor + 1;
    for consumed in address_suffix_consumed_after_root(insts, address_start) {
        let consumer_cursor = address_start + consumed;
        if insts
            .get(consumer_cursor)
            .is_some_and(|inst| scalar_memory_load_type(inst.op).is_some())
        {
            return true;
        }
        if let Some((value_consumed, scalar)) =
            match_scalar_store_value_expr(insts, consumer_cursor)
        {
            if insts
                .get(consumer_cursor + value_consumed)
                .is_some_and(|inst| scalar_memory_store_type(inst.op) == Some(scalar))
            {
                return true;
            }
        }
    }
    false
}

fn address_suffix_consumed_after_root(insts: &[super::ir::CanonInst], cursor: usize) -> Vec<usize> {
    let mut consumed = vec![0usize];
    if match_i32_const_add_suffix(insts, cursor).is_some() {
        consumed.push(2);
    }
    if raw_local_get(insts.get(cursor), 4).is_some() {
        let mut local_consumed = 1usize;
        if insts
            .get(cursor + local_consumed)
            .is_some_and(|inst| inst.op_eq(vm::op_i32_const as Op))
            && insts
                .get(cursor + local_consumed + 1)
                .is_some_and(|inst| inst.op_eq(vm::op_i32_shl as Op))
        {
            let Some(scale) = raw_i32(
                insts
                    .get(cursor + local_consumed)
                    .and_then(|inst| inst.operands.first()),
            ) else {
                return consumed;
            };
            if !(0..=3).contains(&scale) {
                return consumed;
            }
            local_consumed += 2;
        }
        if insts
            .get(cursor + local_consumed)
            .is_some_and(|inst| inst.op_eq(vm::op_i32_add as Op))
        {
            local_consumed += 1;
            consumed.push(local_consumed);
            if match_i32_const_add_suffix(insts, cursor + local_consumed).is_some() {
                consumed.push(local_consumed + 2);
            }
        }
    }
    consumed
}

fn match_scalar_store_value_expr(
    insts: &[super::ir::CanonInst],
    cursor: usize,
) -> Option<(usize, ScalarType)> {
    for scalar in [
        ScalarType::I32,
        ScalarType::I64,
        ScalarType::F32,
        ScalarType::F64,
    ] {
        if let Some(consumed) = match_scalar_value_expr(insts, cursor, scalar) {
            return Some((consumed, scalar));
        }
    }
    None
}

fn match_scalar_value_expr(
    insts: &[super::ir::CanonInst],
    cursor: usize,
    scalar: ScalarType,
) -> Option<usize> {
    let lhs = match_scalar_atomic_value_expr(insts, cursor, scalar)?;
    let rhs_cursor = cursor + lhs;
    let Some(rhs) = match_scalar_atomic_value_expr(insts, rhs_cursor, scalar) else {
        return Some(lhs);
    };
    insts
        .get(rhs_cursor + rhs)
        .filter(|inst| inst.op_eq(scalar_add_op(scalar)))
        .map(|_| lhs + rhs + 1)
        .or(Some(lhs))
}

fn match_scalar_atomic_value_expr(
    insts: &[super::ir::CanonInst],
    cursor: usize,
    scalar: ScalarType,
) -> Option<usize> {
    let inst = insts.get(cursor)?;
    if raw_local_get(Some(inst), scalar_width(scalar)).is_some()
        || scalar_const_operand(inst, scalar).is_some()
        || scalar_memory_load_type(inst.op) == Some(scalar)
    {
        return Some(1);
    }
    None
}

fn scalar_add_op(scalar: ScalarType) -> Op {
    match scalar {
        ScalarType::I32 => vm::op_i32_add as Op,
        ScalarType::I64 => vm::op_i64_add as Op,
        ScalarType::F32 => vm::op_f32_add as Op,
        ScalarType::F64 => vm::op_f64_add as Op,
    }
}

fn scalar_width(scalar: ScalarType) -> u32 {
    match scalar {
        ScalarType::I32 | ScalarType::F32 => 4,
        ScalarType::I64 | ScalarType::F64 => 8,
    }
}

fn scalar_const_operand(inst: &super::ir::CanonInst, scalar: ScalarType) -> Option<LoweredOperand> {
    match scalar {
        ScalarType::I32 if inst.op_eq(vm::op_i32_const as Op) => inst.operands.first().cloned(),
        ScalarType::I64 if inst.op_eq(vm::op_i64_const as Op) => inst.operands.first().cloned(),
        ScalarType::F32 if inst.op_eq(vm::op_f32_const as Op) => inst.operands.first().cloned(),
        ScalarType::F64 if inst.op_eq(vm::op_f64_const as Op) => inst.operands.first().cloned(),
        _ => None,
    }
}

fn match_i32_const_add_suffix(insts: &[super::ir::CanonInst], cursor: usize) -> Option<i32> {
    let konst = insts.get(cursor)?;
    let add = insts.get(cursor + 1)?;
    if !konst.op_eq(vm::op_i32_const as Op) || !add.op_eq(vm::op_i32_add as Op) {
        return None;
    }
    raw_i32(konst.operands.first())
}

fn raw_local_get(inst: Option<&super::ir::CanonInst>, width: u32) -> Option<LoweredOperand> {
    let inst = inst?;
    if width == 4 && inst.op_eq(vm::op_local_get4 as Op) {
        return inst.operands.first().cloned();
    }
    if width == 8 && inst.op_eq(vm::op_local_get8 as Op) {
        return inst.operands.first().cloned();
    }
    if width == 16 && inst.op_eq(vm::op_local_get16 as Op) {
        return inst.operands.first().cloned();
    }
    None
}

fn writes_effect_state(op: Op) -> bool {
    is_memory_store(op) || is_global_write(op) || is_table_write(op) || is_call(op)
}

fn is_memory_store(op: Op) -> bool {
    let labels = [
        vm::op_i32_store as Op,
        vm::op_i32_store8 as Op,
        vm::op_i32_store16 as Op,
        vm::op_i64_store as Op,
        vm::op_i64_store8 as Op,
        vm::op_i64_store16 as Op,
        vm::op_i64_store32 as Op,
        vm::op_f32_store as Op,
        vm::op_f64_store as Op,
    ];
    labels
        .iter()
        .any(|candidate| std::ptr::fn_addr_eq(*candidate, op))
}

fn is_global_write(op: Op) -> bool {
    let labels = [
        vm::op_global_set4 as Op,
        vm::op_global_set8 as Op,
        vm::op_global_set16 as Op,
    ];
    labels
        .iter()
        .any(|candidate| std::ptr::fn_addr_eq(*candidate, op))
}

fn is_table_write(op: Op) -> bool {
    let labels = [
        vm::op_table_set as Op,
        vm::op_table_init as Op,
        vm::op_table_copy as Op,
        vm::op_table_fill as Op,
    ];
    labels
        .iter()
        .any(|candidate| std::ptr::fn_addr_eq(*candidate, op))
}

fn is_call(op: Op) -> bool {
    let labels = [
        vm::op_call as Op,
        vm::op_call_import as Op,
        vm::op_return_call as Op,
        vm::op_return_call_import as Op,
        vm::op_call_indirect as Op,
        vm::op_return_call_indirect as Op,
    ];
    labels
        .iter()
        .any(|candidate| std::ptr::fn_addr_eq(*candidate, op))
}

fn raw_local_addr(operand: Option<&LoweredOperand>) -> Option<u32> {
    let LoweredOperand::Raw(encoded) = operand? else {
        return None;
    };
    Some(unsafe { Operand { encoded: *encoded }.local_addr })
}

fn local_get_op_for_ty(ty: ValType) -> Op {
    match ty.stack_size().u32() {
        4 => vm::op_local_get4 as Op,
        8 => vm::op_local_get8 as Op,
        16 => vm::op_local_get16 as Op,
        other => panic!("unsupported temp local get width: {other}"),
    }
}

fn local_tee_op_for_ty(ty: ValType) -> Op {
    match ty.stack_size().u32() {
        4 => vm::op_local_tee4 as Op,
        8 => vm::op_local_tee8 as Op,
        16 => vm::op_local_tee16 as Op,
        other => panic!("unsupported temp local tee width: {other}"),
    }
}

fn local_set_op_for_ty(ty: ValType) -> Op {
    match ty.stack_size().u32() {
        4 => vm::op_local_set4 as Op,
        8 => vm::op_local_set8 as Op,
        16 => vm::op_local_set16 as Op,
        other => panic!("unsupported temp local set width: {other}"),
    }
}

fn raw_local_operand(local_addr: u32) -> LoweredOperand {
    LoweredOperand::Raw(unsafe { Operand { local_addr }.encoded })
}

fn fold_i32_const_window(
    insts: &[super::ir::CanonInst],
    cursor: usize,
) -> Option<(super::ir::CanonInst, usize)> {
    let unary = insts.get(cursor..cursor + 2)?;
    if unary[0].op_eq(vm::op_i32_const as Op) && unary[1].op_eq(vm::op_i32_eqz as Op) {
        let value = raw_i32(unary[0].operands.first())?;
        return Some((
            folded_const_inst(
                &unary[0],
                &unary[1],
                vm::op_i32_const as Op,
                raw_i32_operand((value == 0) as i32),
            ),
            2,
        ));
    }

    let binary = insts.get(cursor..cursor + 3)?;
    if !binary[0].op_eq(vm::op_i32_const as Op) || !binary[1].op_eq(vm::op_i32_const as Op) {
        return None;
    }
    let lhs = raw_i32(binary[0].operands.first())?;
    let rhs = raw_i32(binary[1].operands.first())?;
    let value = eval_i32_const_op(binary[2].op, lhs, rhs)?;
    Some((
        folded_const_inst(
            &binary[0],
            &binary[2],
            vm::op_i32_const as Op,
            raw_i32_operand(value),
        ),
        3,
    ))
}

fn fold_i64_const_window(
    insts: &[super::ir::CanonInst],
    cursor: usize,
) -> Option<(super::ir::CanonInst, usize)> {
    let unary = insts.get(cursor..cursor + 2)?;
    if unary[0].op_eq(vm::op_i64_const as Op) && unary[1].op_eq(vm::op_i64_eqz as Op) {
        let value = raw_i64(unary[0].operands.first())?;
        return Some((
            folded_const_inst(
                &unary[0],
                &unary[1],
                vm::op_i32_const as Op,
                raw_i32_operand((value == 0) as i32),
            ),
            2,
        ));
    }

    let binary = insts.get(cursor..cursor + 3)?;
    if !binary[0].op_eq(vm::op_i64_const as Op) || !binary[1].op_eq(vm::op_i64_const as Op) {
        return None;
    }
    let lhs = raw_i64(binary[0].operands.first())?;
    let rhs = raw_i64(binary[1].operands.first())?;
    let folded = if let Some(value) = eval_i64_binop(binary[2].op, lhs, rhs) {
        (vm::op_i64_const as Op, raw_i64_operand(value))
    } else if let Some(value) = eval_i64_compare(binary[2].op, lhs, rhs) {
        (vm::op_i32_const as Op, raw_i32_operand(value as i32))
    } else {
        return None;
    };
    Some((
        folded_const_inst(&binary[0], &binary[2], folded.0, folded.1),
        3,
    ))
}

fn fuse_i32_select_bit_steps(block: &mut super::ir::CanonBlock) -> usize {
    let mut rewritten = Vec::with_capacity(block.insts.len());
    let mut cursor = 0usize;
    let mut fused = 0usize;
    while cursor < block.insts.len() {
        if raw_local_get(block.insts.get(cursor), 4).is_some() {
            if let Some((step, consumed)) =
                match_i32_select_bit_step_window(&block.insts, cursor + 1)
            {
                rewritten.push(block.insts[cursor].clone());
                rewritten.push(step);
                cursor += consumed + 1;
                fused += 1;
                continue;
            }
        }
        if let Some((step, consumed)) = match_i32_select_bit_step_window(&block.insts, cursor) {
            rewritten.push(step);
            cursor += consumed;
            fused += 1;
            continue;
        }
        rewritten.push(block.insts[cursor].clone());
        cursor += 1;
    }
    if fused != 0 {
        block.insts = rewritten;
    }
    fused
}

fn match_i32_select_bit_step_window(
    insts: &[super::ir::CanonInst],
    cursor: usize,
) -> Option<(super::ir::CanonInst, usize)> {
    let shift_one = insts.get(cursor)?;
    let shr = insts.get(cursor + 1)?;
    if raw_i32(shift_one.operands.first())? != 1 || !shr.op_eq(vm::op_i32_shr_u as Op) {
        return None;
    }

    let mut flags = 0;
    let mut at = cursor + 2;
    if insts
        .get(at)
        .and_then(|inst| raw_i32(inst.operands.first()))
        .is_some_and(|value| value == 0x7fff)
        && insts
            .get(at + 1)
            .is_some_and(|inst| inst.op_eq(vm::op_i32_and as Op))
    {
        flags |= I32_SELECT_BIT_STEP_MASK_SHIFTED;
        at += 2;
    }

    let tmp_local = raw_local_tee(insts.get(at), 4)?;
    at += 1;

    match_i32_select_bit_step_xor_condition(insts, cursor, at, tmp_local.clone(), flags)
        .or_else(|| match_i32_select_bit_step_eq_condition(insts, cursor, at, tmp_local, flags))
}

fn match_i32_select_bit_step_xor_condition(
    insts: &[super::ir::CanonInst],
    start: usize,
    cursor: usize,
    tmp_local: LoweredOperand,
    flags: u32,
) -> Option<(super::ir::CanonInst, usize)> {
    let poly = insts.get(cursor)?;
    let xor = insts.get(cursor + 1)?;
    let tmp_get = raw_local_get(insts.get(cursor + 2), 4)?;
    if !xor.op_eq(vm::op_i32_xor as Op) || !same_raw_operand(&tmp_get, &tmp_local) {
        return None;
    }
    let (source_local, source_shift, prev_local, condition_consumed) =
        match_i32_xor_lsb_condition(insts, cursor + 3)?;
    let select_cursor = cursor + 3 + condition_consumed;
    let select = insts.get(select_cursor)?;
    if !is_i32_select_inst(select) {
        return None;
    }
    let (dst_local, flags, last_cursor) =
        match_i32_select_bit_step_consumer(insts, select_cursor + 1, flags);
    let operands = vec![
        tmp_local,
        poly.operands.first()?.clone(),
        source_local,
        raw_u32_operand(source_shift),
        prev_local,
        raw_u32_operand(flags),
        dst_local,
    ];
    Some((
        make_i32_select_bit_step_inst(&insts[start], &insts[last_cursor], operands),
        last_cursor + 1 - start,
    ))
}

fn match_i32_select_bit_step_eq_condition(
    insts: &[super::ir::CanonInst],
    start: usize,
    cursor: usize,
    tmp_local: LoweredOperand,
    flags: u32,
) -> Option<(super::ir::CanonInst, usize)> {
    let tmp_get = raw_local_get(insts.get(cursor), 4)?;
    let poly = insts.get(cursor + 1)?;
    let xor = insts.get(cursor + 2)?;
    let prev = raw_local_get(insts.get(cursor + 3), 4)?;
    let one = insts.get(cursor + 4)?;
    let and = insts.get(cursor + 5)?;
    let source = raw_local_get(insts.get(cursor + 6), 4)?;
    let shift = insts.get(cursor + 7)?;
    let shr = insts.get(cursor + 8)?;
    let eq = insts.get(cursor + 9)?;
    let select = insts.get(cursor + 10)?;
    if !same_raw_operand(&tmp_get, &tmp_local)
        || !xor.op_eq(vm::op_i32_xor as Op)
        || raw_i32(one.operands.first())? != 1
        || !and.op_eq(vm::op_i32_and as Op)
        || !shr.op_eq(vm::op_i32_shr_u as Op)
        || !eq.op_eq(vm::op_i32_eq as Op)
        || !is_i32_select_inst(select)
    {
        return None;
    }
    let (dst_local, flags, last_cursor) = match_i32_select_bit_step_consumer(
        insts,
        cursor + 11,
        flags | I32_SELECT_BIT_STEP_EQ_CONDITION,
    );
    let operands = vec![
        tmp_local,
        poly.operands.first()?.clone(),
        source,
        raw_u32_operand(raw_i32(shift.operands.first())? as u32),
        prev,
        raw_u32_operand(flags),
        dst_local,
    ];
    Some((
        make_i32_select_bit_step_inst(&insts[start], &insts[last_cursor], operands),
        last_cursor + 1 - start,
    ))
}

fn match_i32_xor_lsb_condition(
    insts: &[super::ir::CanonInst],
    cursor: usize,
) -> Option<(LoweredOperand, u32, LoweredOperand, usize)> {
    let source = raw_local_get(insts.get(cursor), 4)?;
    let mut source_shift = 0;
    let mut at = cursor + 1;
    if let (Some(shift), Some(shr)) = (insts.get(at), insts.get(at + 1)) {
        if shift.op_eq(vm::op_i32_const as Op) && shr.op_eq(vm::op_i32_shr_u as Op) {
            source_shift = raw_i32(shift.operands.first())? as u32;
            at += 2;
        }
    }
    let prev = raw_local_get(insts.get(at), 4)?;
    let xor = insts.get(at + 1)?;
    let one = insts.get(at + 2)?;
    let and = insts.get(at + 3)?;
    if !xor.op_eq(vm::op_i32_xor as Op)
        || raw_i32(one.operands.first())? != 1
        || !and.op_eq(vm::op_i32_and as Op)
    {
        return None;
    }
    Some((source, source_shift, prev, at + 4 - cursor))
}

fn match_i32_select_bit_step_consumer(
    insts: &[super::ir::CanonInst],
    cursor: usize,
    flags: u32,
) -> (LoweredOperand, u32, usize) {
    if let Some(dst) = raw_local_tee(insts.get(cursor), 4) {
        return (dst, flags | I32_SELECT_BIT_STEP_TEE_DST, cursor);
    }
    (raw_u32_operand(u32::MAX), flags, cursor - 1)
}

fn make_i32_select_bit_step_inst(
    first: &super::ir::CanonInst,
    last: &super::ir::CanonInst,
    operands: Vec<LoweredOperand>,
) -> super::ir::CanonInst {
    super::ir::CanonInst {
        id: first.id,
        op: vm::op_i32_select_bit_step4 as Op,
        operands,
        stack_before: first.stack_before.clone(),
        stack_after: last.stack_after.clone(),
        preserved_prefix_len: first.preserved_prefix_len,
        fresh_result_count: last.fresh_result_count,
        effect: last.effect,
    }
}

fn raw_local_tee(inst: Option<&super::ir::CanonInst>, width: u32) -> Option<LoweredOperand> {
    let inst = inst?;
    if width == 4 && inst.op_eq(vm::op_local_tee4 as Op) {
        return inst.operands.first().cloned();
    }
    if width == 8 && inst.op_eq(vm::op_local_tee8 as Op) {
        return inst.operands.first().cloned();
    }
    if width == 16 && inst.op_eq(vm::op_local_tee16 as Op) {
        return inst.operands.first().cloned();
    }
    None
}

fn is_i32_select_inst(inst: &super::ir::CanonInst) -> bool {
    inst.op_eq(vm::op_select as Op) || inst.op_eq(vm::op_select4 as Op)
}

fn same_raw_operand(lhs: &LoweredOperand, rhs: &LoweredOperand) -> bool {
    match (lhs, rhs) {
        (LoweredOperand::Raw(lhs), LoweredOperand::Raw(rhs)) => lhs == rhs,
        _ => false,
    }
}

fn raw_u32_operand(value: u32) -> LoweredOperand {
    LoweredOperand::Raw(unsafe { Operand { u32: value }.encoded })
}

fn coalesce_local_set_get(block: &mut super::ir::CanonBlock, pair_cursors: &[usize]) -> usize {
    if pair_cursors.is_empty() {
        return 0;
    }
    let pair_starts = pair_cursors.iter().copied().collect::<HashSet<_>>();
    let mut coalesced = 0usize;
    let mut rewritten = Vec::with_capacity(block.insts.len());
    let mut cursor = 0usize;
    while cursor < block.insts.len() {
        if pair_starts.contains(&cursor) {
            let set = &block.insts[cursor];
            let get = &block.insts[cursor + 1];
            rewritten.push(make_coalesced_tee(set, get));
            coalesced += 1;
            cursor += 2;
            continue;
        }
        rewritten.push(block.insts[cursor].clone());
        cursor += 1;
    }
    block.insts = rewritten;
    coalesced
}

fn make_coalesced_tee(
    set: &super::ir::CanonInst,
    get: &super::ir::CanonInst,
) -> super::ir::CanonInst {
    let tee_op = if set.op_eq(vm::op_local_set4 as Op) && get.op_eq(vm::op_local_get4 as Op) {
        vm::op_local_tee4 as Op
    } else if set.op_eq(vm::op_local_set8 as Op) && get.op_eq(vm::op_local_get8 as Op) {
        vm::op_local_tee8 as Op
    } else if set.op_eq(vm::op_local_set16 as Op) && get.op_eq(vm::op_local_get16 as Op) {
        vm::op_local_tee16 as Op
    } else {
        panic!("analysis emitted an invalid coalescing pair");
    };
    super::ir::CanonInst {
        id: set.id,
        op: tee_op,
        operands: set.operands.clone(),
        stack_before: set.stack_before.clone(),
        stack_after: get.stack_after.clone(),
        preserved_prefix_len: set.preserved_prefix_len,
        fresh_result_count: get.fresh_result_count,
        effect: get.effect,
    }
}

fn hoist_pre_candidates(func: &mut CanonFunc, locals: &mut LocalsData) -> usize {
    let mut hoisted = 0usize;
    loop {
        let analysis = analysis::analyze(func);
        let Some((header, candidate)) =
            analysis
                .pre_candidates
                .iter()
                .enumerate()
                .find_map(|(header, candidate)| {
                    candidate.as_ref().map(|candidate| (header, candidate))
                })
        else {
            break;
        };
        let Some(preheader) = analysis.loop_preheaders[header] else {
            break;
        };
        let temp_addr = locals.allocate_temp_slot(candidate.site.result_ty);
        if !insert_hoisted_expr(func, preheader, candidate, temp_addr) {
            break;
        }
        replace_header_expr_with_temp(func, candidate, temp_addr);
        hoisted += 1;
    }
    hoisted
}

fn insert_hoisted_expr(
    func: &mut CanonFunc,
    preheader: usize,
    candidate: &PreCandidate,
    temp_addr: u32,
) -> bool {
    let expr_insts = {
        let block = &func.blocks[candidate.block_id];
        block.insts[candidate.cursor..candidate.cursor + candidate.site.expr_len].to_vec()
    };
    let block = &mut func.blocks[preheader];
    let insert_at = hoist_insertion_index(block);
    let base_stack = if let Some(inst) = block.insts.get(insert_at) {
        inst.stack_before.clone()
    } else {
        block
            .insts
            .last()
            .map(|inst| inst.stack_after.clone())
            .unwrap_or_default()
    };
    let Some(first) = expr_insts.first() else {
        return false;
    };
    let Some(last) = expr_insts.last() else {
        return false;
    };
    if first.stack_before != base_stack {
        return false;
    }
    let set_inst = super::ir::CanonInst {
        id: super::ir::InstId(usize::MAX),
        op: local_set_op_for_ty(candidate.site.result_ty),
        operands: vec![raw_local_operand(temp_addr)],
        stack_before: last.stack_after.clone(),
        stack_after: base_stack,
        preserved_prefix_len: last.stack_after.len().saturating_sub(1),
        fresh_result_count: 0,
        effect: super::ir::EffectId(usize::MAX),
    };
    let mut hoisted = expr_insts;
    hoisted.push(set_inst);
    block.insts.splice(insert_at..insert_at, hoisted);
    true
}

fn replace_header_expr_with_temp(func: &mut CanonFunc, candidate: &PreCandidate, temp_addr: u32) {
    let block = &mut func.blocks[candidate.block_id];
    let cursor = candidate.cursor;
    let occurrence = &candidate.site;
    let first = block.insts[cursor].clone();
    let expr_last = block.insts[cursor + occurrence.expr_len - 1].clone();
    let mut rewritten = Vec::with_capacity(block.insts.len() - occurrence.expr_len + 1);
    rewritten.extend(block.insts[..cursor].iter().cloned());
    rewritten.push(make_temp_local_get(
        temp_addr,
        occurrence.result_ty,
        &first,
        &expr_last,
    ));
    append_consumer_if_present(
        &mut rewritten,
        &block.insts,
        cursor,
        occurrence.expr_len,
        occurrence.consumed,
    );
    rewritten.extend(block.insts[cursor + occurrence.consumed..].iter().cloned());
    block.insts = rewritten;
}

fn hoist_insertion_index(block: &super::ir::CanonBlock) -> usize {
    let Some(last) = block.insts.last() else {
        return 0;
    };
    if last.op_eq(vm::op_br as Op)
        || last.op_eq(vm::op_br_if as Op)
        || last.op_eq(vm::op_br_table as Op)
        || last.op_eq(vm::op_if as Op)
        || last.op_eq(vm::op_else as Op)
        || last.op_eq(vm::op_return as Op)
    {
        block.insts.len().saturating_sub(1)
    } else {
        block.insts.len()
    }
}

fn folded_const_inst(
    first: &super::ir::CanonInst,
    last: &super::ir::CanonInst,
    op: Op,
    operand: LoweredOperand,
) -> super::ir::CanonInst {
    super::ir::CanonInst {
        id: first.id,
        op,
        operands: vec![operand],
        stack_before: first.stack_before.clone(),
        stack_after: last.stack_after.clone(),
        preserved_prefix_len: first.preserved_prefix_len,
        fresh_result_count: last.fresh_result_count,
        effect: last.effect,
    }
}

fn eval_i32_const_op(op: Op, lhs: i32, rhs: i32) -> Option<i32> {
    Some(if std::ptr::fn_addr_eq(op, vm::op_i32_add as Op) {
        lhs.wrapping_add(rhs)
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_sub as Op) {
        lhs.wrapping_sub(rhs)
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_mul as Op) {
        lhs.wrapping_mul(rhs)
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_and as Op) {
        lhs & rhs
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_or as Op) {
        lhs | rhs
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_xor as Op) {
        lhs ^ rhs
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_shl as Op) {
        lhs.wrapping_shl(rhs as u32)
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_shr_s as Op) {
        lhs.wrapping_shr(rhs as u32)
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_shr_u as Op) {
        ((lhs as u32).wrapping_shr(rhs as u32)) as i32
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_rotl as Op) {
        lhs.rotate_left(rhs as u32)
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_rotr as Op) {
        lhs.rotate_right(rhs as u32)
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_eq as Op) {
        (lhs == rhs) as i32
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_ne as Op) {
        (lhs != rhs) as i32
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_lt_s as Op) {
        (lhs < rhs) as i32
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_lt_u as Op) {
        ((lhs as u32) < (rhs as u32)) as i32
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_gt_s as Op) {
        (lhs > rhs) as i32
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_gt_u as Op) {
        ((lhs as u32) > (rhs as u32)) as i32
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_le_s as Op) {
        (lhs <= rhs) as i32
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_le_u as Op) {
        ((lhs as u32) <= (rhs as u32)) as i32
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_ge_s as Op) {
        (lhs >= rhs) as i32
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_ge_u as Op) {
        ((lhs as u32) >= (rhs as u32)) as i32
    } else {
        return None;
    })
}

fn eval_i64_binop(op: Op, lhs: i64, rhs: i64) -> Option<i64> {
    Some(if std::ptr::fn_addr_eq(op, vm::op_i64_add as Op) {
        lhs.wrapping_add(rhs)
    } else if std::ptr::fn_addr_eq(op, vm::op_i64_sub as Op) {
        lhs.wrapping_sub(rhs)
    } else if std::ptr::fn_addr_eq(op, vm::op_i64_mul as Op) {
        lhs.wrapping_mul(rhs)
    } else if std::ptr::fn_addr_eq(op, vm::op_i64_and as Op) {
        lhs & rhs
    } else if std::ptr::fn_addr_eq(op, vm::op_i64_or as Op) {
        lhs | rhs
    } else if std::ptr::fn_addr_eq(op, vm::op_i64_xor as Op) {
        lhs ^ rhs
    } else if std::ptr::fn_addr_eq(op, vm::op_i64_shl as Op) {
        lhs.wrapping_shl(rhs as u32)
    } else if std::ptr::fn_addr_eq(op, vm::op_i64_shr_s as Op) {
        lhs.wrapping_shr(rhs as u32)
    } else if std::ptr::fn_addr_eq(op, vm::op_i64_shr_u as Op) {
        ((lhs as u64).wrapping_shr(rhs as u32)) as i64
    } else if std::ptr::fn_addr_eq(op, vm::op_i64_rotl as Op) {
        lhs.rotate_left(rhs as u32)
    } else if std::ptr::fn_addr_eq(op, vm::op_i64_rotr as Op) {
        lhs.rotate_right(rhs as u32)
    } else {
        return None;
    })
}

fn eval_i64_compare(op: Op, lhs: i64, rhs: i64) -> Option<bool> {
    Some(if std::ptr::fn_addr_eq(op, vm::op_i64_eq as Op) {
        lhs == rhs
    } else if std::ptr::fn_addr_eq(op, vm::op_i64_ne as Op) {
        lhs != rhs
    } else if std::ptr::fn_addr_eq(op, vm::op_i64_lt_s as Op) {
        lhs < rhs
    } else if std::ptr::fn_addr_eq(op, vm::op_i64_lt_u as Op) {
        (lhs as u64) < (rhs as u64)
    } else if std::ptr::fn_addr_eq(op, vm::op_i64_gt_s as Op) {
        lhs > rhs
    } else if std::ptr::fn_addr_eq(op, vm::op_i64_gt_u as Op) {
        (lhs as u64) > (rhs as u64)
    } else if std::ptr::fn_addr_eq(op, vm::op_i64_le_s as Op) {
        lhs <= rhs
    } else if std::ptr::fn_addr_eq(op, vm::op_i64_le_u as Op) {
        (lhs as u64) <= (rhs as u64)
    } else if std::ptr::fn_addr_eq(op, vm::op_i64_ge_s as Op) {
        lhs >= rhs
    } else if std::ptr::fn_addr_eq(op, vm::op_i64_ge_u as Op) {
        (lhs as u64) >= (rhs as u64)
    } else {
        return None;
    })
}

fn retain_reachable_blocks(func: &mut CanonFunc, reachable: &[usize]) {
    if reachable.len() == func.blocks.len() {
        return;
    }
    let mut new_id_for_old = vec![None; func.blocks.len()];
    for (new_id, old_id) in reachable.iter().copied().enumerate() {
        new_id_for_old[old_id] = Some(new_id);
    }

    func.blocks = reachable
        .iter()
        .copied()
        .enumerate()
        .map(|(new_id, old_id)| {
            let mut block = func.blocks[old_id].clone();
            block.id = new_id;
            for inst in &mut block.insts {
                for operand in &mut inst.operands {
                    if let LoweredOperand::JumpTarget(target) = operand {
                        if let Some(mapped) = new_id_for_old[*target] {
                            *target = mapped;
                        }
                    }
                }
            }
            block
        })
        .collect();
    func.entry_block = 0;
}

fn normalize_after_cfg_rewrites(func: &mut CanonFunc) {
    recompute_cfg(func);
    let reachable = compute_reachable_blocks(func);
    retain_reachable_blocks(func, &reachable);
    recompute_cfg(func);
    renumber_inst_metadata(func);
}

fn compute_reachable_blocks(func: &CanonFunc) -> Vec<usize> {
    fn visit(block_id: usize, func: &CanonFunc, seen: &mut [bool]) {
        if seen[block_id] {
            return;
        }
        seen[block_id] = true;
        for succ in &func.blocks[block_id].successors {
            visit(*succ, func, seen);
        }
    }

    let mut seen = vec![false; func.blocks.len()];
    visit(func.entry_block, func, &mut seen);
    seen.into_iter()
        .enumerate()
        .filter_map(|(block_id, reachable)| reachable.then_some(block_id))
        .collect()
}

fn renumber_inst_metadata(func: &mut CanonFunc) {
    let mut next_inst = 0usize;
    let mut next_effect = 0usize;
    for block in &mut func.blocks {
        for inst in &mut block.insts {
            inst.id = super::ir::InstId(next_inst);
            inst.effect = super::ir::EffectId(next_effect);
            next_inst += 1;
            next_effect += 1;
        }
    }
}

fn verify_block_stacks(func: &CanonFunc) -> bool {
    func.blocks.iter().all(|block| {
        let mut expected_stack = block
            .params
            .iter()
            .map(|param| param.ty)
            .collect::<Vec<_>>();
        for inst in &block.insts {
            if inst.stack_before != expected_stack {
                return false;
            }
            expected_stack = inst.stack_after.clone();
        }
        true
    })
}

fn recompute_cfg(func: &mut CanonFunc) {
    let block_count = func.blocks.len();
    let mut predecessors = vec![Vec::new(); block_count];
    for block_id in 0..block_count {
        let successors = block_successors(&func.blocks, block_id);
        func.blocks[block_id].successors = successors.clone();
        for succ in successors {
            predecessors[succ].push(block_id);
        }
    }
    for (block, preds) in func.blocks.iter_mut().zip(predecessors) {
        block.predecessors = preds;
    }
}

fn block_successors(blocks: &[super::ir::CanonBlock], block_id: usize) -> Vec<usize> {
    let Some(last) = blocks[block_id].insts.last() else {
        return (block_id + 1 < blocks.len())
            .then_some(block_id + 1)
            .into_iter()
            .collect();
    };
    let fallthrough = (block_id + 1 < blocks.len()).then_some(block_id + 1);
    if last.op_eq(vm::op_br as Op)
        || last.op_eq(vm::op_else as Op)
        || last.op_eq(vm::op_return as Op)
    {
        return jump_target(last.operands.first()).into_iter().collect();
    }
    if last.op_eq(vm::op_if as Op) || last.op_eq(vm::op_br_if as Op) {
        let mut out = jump_target(last.operands.first())
            .into_iter()
            .collect::<Vec<_>>();
        if let Some(next) = fallthrough {
            out.push(next);
        }
        out.sort_unstable();
        out.dedup();
        return out;
    }
    if last.op_eq(vm::op_br_table as Op) {
        let mut out = last
            .operands
            .iter()
            .skip(1)
            .filter_map(|operand| jump_target(Some(operand)))
            .collect::<Vec<_>>();
        out.sort_unstable();
        out.dedup();
        return out;
    }
    fallthrough.into_iter().collect()
}

fn make_br(target: usize) -> super::ir::CanonInst {
    super::ir::CanonInst {
        id: super::ir::InstId(usize::MAX),
        op: vm::op_br as Op,
        operands: vec![LoweredOperand::JumpTarget(target)],
        stack_before: Vec::new(),
        stack_after: Vec::new(),
        preserved_prefix_len: 0,
        fresh_result_count: 0,
        effect: super::ir::EffectId(usize::MAX),
    }
}

fn raw_i32(operand: Option<&LoweredOperand>) -> Option<i32> {
    let LoweredOperand::Raw(encoded) = operand? else {
        return None;
    };
    Some(unsafe { Operand { encoded: *encoded }.i32 })
}

fn raw_i64(operand: Option<&LoweredOperand>) -> Option<i64> {
    let LoweredOperand::Raw(encoded) = operand? else {
        return None;
    };
    Some(unsafe { Operand { encoded: *encoded }.i64 })
}

fn raw_i32_operand(value: i32) -> LoweredOperand {
    LoweredOperand::Raw(unsafe { Operand { i32: value }.encoded })
}

fn raw_i64_operand(value: i64) -> LoweredOperand {
    LoweredOperand::Raw(unsafe { Operand { i64: value }.encoded })
}

fn jump_target(operand: Option<&LoweredOperand>) -> Option<usize> {
    match operand? {
        LoweredOperand::JumpTarget(target) => Some(*target),
        LoweredOperand::Raw(_)
        | LoweredOperand::ConstPoolRef(_)
        | LoweredOperand::CallRecipeRef(_) => None,
    }
}

trait CanonInstExt {
    fn op_eq(&self, candidate: Op) -> bool;
}

impl CanonInstExt for super::ir::CanonInst {
    fn op_eq(&self, candidate: Op) -> bool {
        std::ptr::fn_addr_eq(self.op, candidate)
    }
}
