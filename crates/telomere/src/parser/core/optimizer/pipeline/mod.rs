pub(crate) mod analysis;
pub(crate) mod encode;
pub(crate) mod ir;
pub(crate) mod lower;
pub(crate) mod select;
pub(crate) mod transform;
pub(crate) mod versioning;

use super::{cfg::build_program, InstructionMeta};
use crate::common::{FuncIdx, FuncType, Instr, LocalsData, LoweredFunction};

const CODE_SIZE_GROWTH_BUDGET_PCT: usize = 20;
const CODE_SIZE_GROWTH_BUDGET_ABS: usize = 16;

pub(super) fn optimize_function(
    funcidx: FuncIdx,
    functype: &FuncType,
    locals: &mut LocalsData,
    instrs: Vec<Instr>,
    meta: Vec<InstructionMeta>,
) -> LoweredFunction {
    let fallback_instrs = instrs.clone();
    let fallback_op_lens = meta
        .iter()
        .map(|entry| u16::try_from(entry.len).expect("instruction length exceeds u16::MAX"))
        .collect::<Vec<_>>();
    let Some(program) = build_program(&instrs, meta) else {
        return LoweredFunction::from_materialized(fallback_instrs, fallback_op_lens);
    };

    let canon = ir::CanonFunc::from_program(
        funcidx,
        functype.clone(),
        u32::try_from(locals.byte_size()).expect("locals size exceeds u32::MAX"),
        &program,
    );
    if !canon.verify() {
        return LoweredFunction::from_materialized(fallback_instrs, fallback_op_lens);
    }

    let analysis = analysis::analyze(&canon);
    if !analysis.verify(&canon) {
        return LoweredFunction::from_materialized(fallback_instrs, fallback_op_lens);
    }

    let transformed = transform::run(canon, locals, &analysis);
    if !transformed.verify() {
        return LoweredFunction::from_materialized(fallback_instrs, fallback_op_lens);
    }

    let transformed_analysis = analysis::analyze(&transformed.func);
    if !transformed_analysis.verify(&transformed.func) {
        return LoweredFunction::from_materialized(fallback_instrs, fallback_op_lens);
    }

    let kernel = select::select(&transformed.func, &transformed_analysis);
    if !select::verify(&kernel) {
        return LoweredFunction::from_materialized(fallback_instrs, fallback_op_lens);
    }

    let versioned = versioning::apply(kernel, &transformed.func, &transformed_analysis);
    if !versioning::verify(&versioned) {
        return LoweredFunction::from_materialized(fallback_instrs, fallback_op_lens);
    }

    let lowered_kernel = lower::lower(versioned);
    if !lower::verify(&lowered_kernel) {
        return LoweredFunction::from_materialized(fallback_instrs, fallback_op_lens);
    }

    let lowered = encode::encode(lowered_kernel);
    if !encode::verify(&lowered)
        || exceeds_code_size_budget(fallback_op_lens.len(), lowered.code.len())
    {
        return LoweredFunction::from_materialized(fallback_instrs, fallback_op_lens);
    }
    lowered
}

fn exceeds_code_size_budget(original_ops: usize, lowered_ops: usize) -> bool {
    let relative_slack = (original_ops
        .saturating_mul(CODE_SIZE_GROWTH_BUDGET_PCT)
        .saturating_add(99))
        / 100;
    let allowed = original_ops.saturating_add(relative_slack.max(CODE_SIZE_GROWTH_BUDGET_ABS));
    lowered_ops > allowed
}
