pub(crate) mod analysis;
pub(crate) mod encode;
pub(crate) mod ir;
pub(crate) mod lower;
pub(crate) mod select;
pub(crate) mod transform;
pub(crate) mod versioning;

use super::{cfg::build_program, InstructionMeta};
use crate::common::{FuncIdx, FuncType, Instr, LocalsData, LoweredFunction, ValType};
#[cfg(feature = "measure-switches")]
use crate::measure_switches;

const CODE_SIZE_GROWTH_BUDGET_PCT: usize = 10;
const CODE_SIZE_GROWTH_BUDGET_ABS: usize = 8;

pub(super) fn optimize_function(
    funcidx: FuncIdx,
    functype: &FuncType,
    locals: &mut LocalsData,
    instrs: Vec<Instr>,
    meta: Vec<InstructionMeta>,
) -> LoweredFunction {
    let fallback_op_lens = meta
        .iter()
        .map(|entry| u16::try_from(entry.len).expect("instruction length exceeds u16::MAX"))
        .collect::<Vec<_>>();

    // #186: the validated maximum operand-stack depth (bytes) of this
    // function. The runtime reserves this at frame entry so in-function
    // pushes need no per-push bounds check. Snapshots from unreachable code
    // are skipped: it never executes, and the validator only records a
    // polymorphic lower bound for it. Folding both `stack_before` and
    // `stack_after` covers block-entry heights and branch points, which only
    // ever appear as some reachable record's before/after snapshot.
    let meta_max_operand_bytes = max_operand_bytes_from_meta(&meta);

    #[cfg(feature = "measure-switches")]
    if !measure_switches::optimizer_pipeline_enabled() {
        // The OFF cell deliberately takes the pipeline's normal verifier fallback. This
        // measurement-only switch is parse-time only and never reaches the dispatch path.
        return with_max_operand_bytes(
            LoweredFunction::from_materialized(instrs, fallback_op_lens),
            meta_max_operand_bytes,
        );
    }

    let Some(program) = build_program(&instrs, meta) else {
        return with_max_operand_bytes(
            LoweredFunction::from_materialized(instrs, fallback_op_lens),
            meta_max_operand_bytes,
        );
    };

    let canon = ir::CanonFunc::from_program(
        funcidx,
        functype.clone(),
        u32::try_from(locals.byte_size()).expect("locals size exceeds u32::MAX"),
        &program,
    );
    if !canon.verify() {
        return with_max_operand_bytes(
            LoweredFunction::from_materialized(instrs, fallback_op_lens),
            meta_max_operand_bytes,
        );
    }

    let analysis = analysis::analyze(&canon);
    if !analysis.verify(&canon) {
        return with_max_operand_bytes(
            LoweredFunction::from_materialized(instrs, fallback_op_lens),
            meta_max_operand_bytes,
        );
    }

    let transformed = transform::run(canon, locals, &analysis);
    // The transformed stream may contain instructions the transform
    // synthesized (their snapshots exist only here), while a fused
    // superinstruction's transient stack peak appears in the *original*
    // records folded above. Taking the max of both folds bounds the final
    // stream: select/versioning/lower/encode do not change stack effects.
    let canon_max_operand_bytes = max_operand_bytes_from_canon(&transformed.func);
    if !transformed.verify() {
        return with_max_operand_bytes(
            LoweredFunction::from_materialized(instrs, fallback_op_lens),
            meta_max_operand_bytes,
        );
    }

    let transformed_analysis = analysis::analyze(&transformed.func);
    if !transformed_analysis.verify(&transformed.func) {
        return with_max_operand_bytes(
            LoweredFunction::from_materialized(instrs, fallback_op_lens),
            meta_max_operand_bytes,
        );
    }

    let kernel = select::select(&transformed.func, &transformed_analysis);
    if !select::verify(&kernel) {
        return with_max_operand_bytes(
            LoweredFunction::from_materialized(instrs, fallback_op_lens),
            meta_max_operand_bytes,
        );
    }

    let versioned = versioning::apply(kernel, &transformed.func, &transformed_analysis);
    if !versioning::verify(&versioned) {
        return with_max_operand_bytes(
            LoweredFunction::from_materialized(instrs, fallback_op_lens),
            meta_max_operand_bytes,
        );
    }

    let lowered_kernel = lower::lower(versioned);
    if !lower::verify(&lowered_kernel) {
        return with_max_operand_bytes(
            LoweredFunction::from_materialized(instrs, fallback_op_lens),
            meta_max_operand_bytes,
        );
    }

    let lowered = encode::encode(lowered_kernel);
    if !encode::verify(&lowered)
        || exceeds_code_size_budget(fallback_op_lens.len(), lowered.code.len())
    {
        return with_max_operand_bytes(
            LoweredFunction::from_materialized(instrs, fallback_op_lens),
            meta_max_operand_bytes,
        );
    }
    with_max_operand_bytes(lowered, meta_max_operand_bytes.max(canon_max_operand_bytes))
}

fn stack_snapshot_bytes(types: &[ValType]) -> u32 {
    types.iter().map(|ty| ty.stack_size().u32()).sum()
}

fn with_max_operand_bytes(mut lowered: LoweredFunction, max_operand_bytes: u32) -> LoweredFunction {
    lowered.max_operand_bytes = max_operand_bytes;
    lowered
}

fn max_operand_bytes_from_meta(meta: &[InstructionMeta]) -> u32 {
    let mut max = 0;
    for entry in meta {
        if entry.stack_before.reachable {
            max = max.max(stack_snapshot_bytes(&entry.stack_before.types));
            max = max.max(stack_snapshot_bytes(&entry.stack_after.types));
        }
    }
    max
}

fn max_operand_bytes_from_canon(func: &ir::CanonFunc) -> u32 {
    // Only blocks reachable from the entry block can execute; a dead block's
    // snapshots may describe heights that never materialize.
    let mut index_by_id = std::collections::HashMap::with_capacity(func.blocks.len());
    for (index, block) in func.blocks.iter().enumerate() {
        index_by_id.insert(block.id, index);
    }
    let mut reachable = vec![false; func.blocks.len()];
    let mut worklist = vec![func.entry_block];
    while let Some(block_id) = worklist.pop() {
        let Some(&index) = index_by_id.get(&block_id) else {
            continue;
        };
        if reachable[index] {
            continue;
        }
        reachable[index] = true;
        worklist.extend(func.blocks[index].successors.iter().copied());
    }
    let mut max = 0;
    for (index, block) in func.blocks.iter().enumerate() {
        if !reachable[index] {
            continue;
        }
        for inst in &block.insts {
            max = max.max(stack_snapshot_bytes(&inst.stack_before));
            max = max.max(stack_snapshot_bytes(&inst.stack_after));
        }
    }
    max
}
fn exceeds_code_size_budget(original_ops: usize, lowered_ops: usize) -> bool {
    let relative_slack = (original_ops
        .saturating_mul(CODE_SIZE_GROWTH_BUDGET_PCT)
        .saturating_add(99))
        / 100;
    let allowed = original_ops.saturating_add(relative_slack.max(CODE_SIZE_GROWTH_BUDGET_ABS));
    lowered_ops > allowed
}

#[cfg(test)]
mod tests {
    use super::exceeds_code_size_budget;

    #[test]
    fn code_size_budget_uses_documented_absolute_slack_for_small_functions() {
        assert!(!exceeds_code_size_budget(20, 28));
        assert!(exceeds_code_size_budget(20, 29));
    }

    #[test]
    fn code_size_budget_uses_documented_relative_slack_for_large_functions() {
        assert!(!exceeds_code_size_budget(1000, 1100));
        assert!(exceeds_code_size_budget(1000, 1101));
    }
}
