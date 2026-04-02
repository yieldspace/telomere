use super::{analysis::AnalysisResults, ir::CanonBlock, ir::CanonFunc, ir::CanonInst};
use crate::{
    common::{
        encode_local_binop32_kind, encode_local_binop64_kind, encode_local_cmp32_kind,
        encode_local_cmp64_kind, encode_local_unary32_kind, encode_local_unary64_kind,
        LocalBinop32Op, LocalBinop64Op, LocalCmp32Op, LocalCmp64Op, LocalFastConstKind,
        LocalFastRhsShape, LocalUnary32Op, LocalUnary64Op, LoweredOperand, MemArg, Op, Operand,
    },
    runtime::vm,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum FamilyGroup {
    LocalControl,
    CallSelect,
    Memory,
    Generic,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum VersionFact {
    ConstLocal {
        local_addr: u32,
        value: u32,
    },
    LocalZero {
        local_addr: u32,
    },
    LocalNonZero {
        local_addr: u32,
    },
    AddressConstBase {
        offset: u32,
    },
    AddressLocalBase {
        local_addr: u32,
        delta: i32,
    },
    AddressLocalScaledIndex {
        base_local_addr: u32,
        index_local_addr: u32,
        scale_log2: u32,
        delta: i32,
    },
    DirectCallTargetClass {
        imported: bool,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub(crate) struct VersionKey {
    pub(crate) facts: Vec<VersionFact>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BlockVersionKind {
    Generic,
    Specialized,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BlockVersion {
    pub(crate) kind: BlockVersionKind,
    pub(crate) key: VersionKey,
}

impl Default for BlockVersion {
    fn default() -> Self {
        Self {
            kind: BlockVersionKind::Generic,
            key: VersionKey::default(),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct KernelFunction {
    pub(crate) blocks: Vec<KernelBlock>,
}

#[derive(Debug, Clone)]
pub(crate) struct KernelBlock {
    pub(crate) block_id: usize,
    pub(crate) original_block_id: usize,
    pub(crate) label: usize,
    pub(crate) version: BlockVersion,
    pub(crate) ops: Vec<KernelOp>,
}

#[derive(Debug, Clone)]
pub(crate) struct KernelOp {
    pub(crate) label: Option<usize>,
    pub(crate) op: Op,
    pub(crate) operands: Vec<LoweredOperand>,
    pub(crate) family: &'static str,
}

#[derive(Debug, Clone)]
struct MatchResult {
    group: FamilyGroup,
    cost: i32,
    consumed: usize,
    ops: Vec<KernelOp>,
}

struct SelectionContext<'a> {
    analysis: &'a AnalysisResults,
    block: &'a CanonBlock,
}

trait FamilySpec {
    fn group(&self) -> FamilyGroup;
    fn name(&self) -> &'static str;

    fn matches(&self, ctx: &SelectionContext<'_>, cursor: usize) -> bool;

    fn legality(&self, _ctx: &SelectionContext<'_>, _cursor: usize) -> bool {
        true
    }

    fn cost(&self, ctx: &SelectionContext<'_>, _cursor: usize) -> i32 {
        let depth = ctx.analysis.loop_depth[ctx.block.id];
        i32::try_from(depth).expect("loop depth exceeds i32::MAX") * 3
    }

    fn emit(&self, ctx: &SelectionContext<'_>, cursor: usize) -> Option<MatchResult>;
}

fn loop_bonus(ctx: &SelectionContext<'_>) -> i32 {
    let depth = ctx.analysis.loop_depth[ctx.block.id];
    i32::try_from(depth).expect("loop depth exceeds i32::MAX") * 3
}

pub(crate) fn select(func: &CanonFunc, analysis: &AnalysisResults) -> KernelFunction {
    let all_specs: &[&dyn FamilySpec] = &[
        &LocalGetBrIfSpec,
        &LocalGetEqzBrIfSpec,
        &LocalGetConstAddBrIfSpec,
        &LocalGetLocalAddBrIfSpec,
        &LocalGetConstAddRootSpec,
        &LocalGetConstAddSetSpec,
        &LocalGetConstAddTeeSpec,
        &LocalGetLocalAddRootSpec,
        &LocalGetLocalAddSetSpec,
        &LocalGetLocalAddTeeSpec,
        &LocalGetConstCompareBrIfSpec,
        &LocalGetLocalCompareBrIfSpec,
        &LocalGetConstAddTeeBrIfSpec,
        &LocalUnarySpec::ROOT32,
        &LocalUnarySpec::SET32,
        &LocalUnarySpec::TEE32,
        &LocalUnarySpec::ROOT64,
        &LocalUnarySpec::SET64,
        &LocalUnarySpec::TEE64,
        &LocalNumericSpec::BINOP32_ROOT,
        &LocalNumericSpec::BINOP32_SET,
        &LocalNumericSpec::BINOP32_TEE,
        &LocalNumericSpec::BINOP32_BR_IF,
        &LocalNumericSpec::BINOP64_ROOT,
        &LocalNumericSpec::BINOP64_SET,
        &LocalNumericSpec::BINOP64_TEE,
        &LocalNumericSpec::CMP32_ROOT,
        &LocalNumericSpec::CMP32_SET,
        &LocalNumericSpec::CMP32_TEE,
        &LocalNumericSpec::CMP32_BR_IF,
        &LocalNumericSpec::CMP64_ROOT,
        &LocalNumericSpec::CMP64_SET,
        &LocalNumericSpec::CMP64_TEE,
        &LocalNumericSpec::CMP64_BR_IF,
        &CallPassthroughSpec::CALL,
        &CallPassthroughSpec::CALL_IMPORT,
        &CallPassthroughSpec::RETURN_CALL,
        &CallPassthroughSpec::RETURN_CALL_IMPORT,
        &CallPassthroughSpec::CALL_INDIRECT,
        &CallPassthroughSpec::RETURN_CALL_INDIRECT,
        &CallSpec::DIRECT,
        &CallSpec::DIRECT_IMPORT,
        &CallSpec::RETURN,
        &CallSpec::RETURN_IMPORT,
        &CallSpec::INDIRECT,
        &CallSpec::RETURN_INDIRECT,
        &SelectWidthSpec::FOUR,
        &SelectWidthSpec::EIGHT,
        &SelectWidthSpec::SIXTEEN,
        &I32StoreLocalScaledIndexLocalGet4Spec,
        &I32StoreLocalScaledIndexSpec,
        &I32LoadLocalScaledIndexSpec,
        &I32StoreLocalBaseLocalGet4Spec,
        &I32StoreLocalBaseSpec,
        &I32LoadLocalBaseSpec,
        &I32LoadConstBaseLocalGet4AddSet4Spec,
        &I32StoreConstBaseLocal4Spec,
        &I32LoadConstBaseSpec,
    ];
    let specs = all_specs;

    let blocks = func
        .blocks
        .iter()
        .map(|block| {
            let ctx = SelectionContext { analysis, block };
            let mut ops = Vec::new();
            let mut cursor = 0usize;
            while cursor < block.insts.len() {
                let mut selected = GenericSpec
                    .emit(&ctx, cursor)
                    .expect("generic family must always match");
                for spec in specs {
                    if !spec.matches(&ctx, cursor) || !spec.legality(&ctx, cursor) {
                        continue;
                    }
                    let Some(candidate) = spec.emit(&ctx, cursor) else {
                        continue;
                    };
                    let better = candidate.cost > selected.cost
                        || (candidate.cost == selected.cost && candidate.group < selected.group);
                    if better {
                        selected = candidate;
                    }
                }
                for (index, mut op) in selected.ops.into_iter().enumerate() {
                    if cursor == 0 && index == 0 {
                        op.label = Some(block.id);
                    }
                    ops.push(op);
                }
                cursor += selected.consumed;
            }
            KernelBlock {
                block_id: block.id,
                original_block_id: block.id,
                label: block.id,
                version: BlockVersion::default(),
                ops,
            }
        })
        .collect();
    KernelFunction { blocks }
}

pub(crate) fn verify(kernel: &KernelFunction) -> bool {
    !kernel.blocks.is_empty()
        && kernel.blocks.iter().enumerate().all(|(expected, block)| {
            block.block_id == expected
                && !block.ops.is_empty()
                && block.ops.iter().all(|op| !op.family.is_empty())
        })
}

struct GenericSpec;

impl FamilySpec for GenericSpec {
    fn group(&self) -> FamilyGroup {
        FamilyGroup::Generic
    }

    fn name(&self) -> &'static str {
        "generic"
    }

    fn matches(&self, _ctx: &SelectionContext<'_>, _cursor: usize) -> bool {
        true
    }

    fn cost(&self, _ctx: &SelectionContext<'_>, _cursor: usize) -> i32 {
        0
    }

    fn emit(&self, ctx: &SelectionContext<'_>, cursor: usize) -> Option<MatchResult> {
        let inst = ctx.block.insts.get(cursor)?;
        Some(MatchResult {
            group: self.group(),
            cost: 0,
            consumed: 1,
            ops: vec![KernelOp {
                label: None,
                op: inst.op,
                operands: inst.operands.clone(),
                family: self.name(),
            }],
        })
    }
}

struct LocalGetBrIfSpec;
struct LocalGetEqzBrIfSpec;
struct LocalGetConstAddBrIfSpec;
struct LocalGetLocalAddBrIfSpec;
struct LocalGetConstAddRootSpec;
struct LocalGetConstAddSetSpec;
struct LocalGetConstAddTeeSpec;
struct LocalGetLocalAddRootSpec;
struct LocalGetLocalAddSetSpec;
struct LocalGetLocalAddTeeSpec;
struct LocalGetConstCompareBrIfSpec;
struct LocalGetLocalCompareBrIfSpec;
struct LocalGetConstAddTeeBrIfSpec;

impl FamilySpec for LocalGetBrIfSpec {
    fn group(&self) -> FamilyGroup {
        FamilyGroup::LocalControl
    }

    fn name(&self) -> &'static str {
        "op_local_get4_br_if"
    }

    fn matches(&self, ctx: &SelectionContext<'_>, cursor: usize) -> bool {
        let Some([load, branch]) = ctx.block.insts.get(cursor..cursor + 2) else {
            return false;
        };
        load.op_eq(vm::op_local_get4 as Op) && branch.op_eq(vm::op_br_if as Op)
    }

    fn cost(&self, ctx: &SelectionContext<'_>, _cursor: usize) -> i32 {
        20 + loop_bonus(ctx)
    }

    fn emit(&self, ctx: &SelectionContext<'_>, cursor: usize) -> Option<MatchResult> {
        let [load, branch] = ctx.block.insts.get(cursor..cursor + 2)? else {
            return None;
        };
        let taken = branch_target(branch)?;
        Some(MatchResult {
            group: self.group(),
            cost: self.cost(ctx, cursor),
            consumed: 2,
            ops: vec![KernelOp {
                label: None,
                op: vm::op_local_get4_br_if as Op,
                operands: vec![
                    load.operands.first()?.clone(),
                    LoweredOperand::JumpTarget(taken),
                ],
                family: self.name(),
            }],
        })
    }
}

impl FamilySpec for LocalGetEqzBrIfSpec {
    fn group(&self) -> FamilyGroup {
        FamilyGroup::LocalControl
    }

    fn name(&self) -> &'static str {
        "op_local_get4_i32_eqz_br_if"
    }

    fn matches(&self, ctx: &SelectionContext<'_>, cursor: usize) -> bool {
        let Some([load, eqz, branch]) = ctx.block.insts.get(cursor..cursor + 3) else {
            return false;
        };
        load.op_eq(vm::op_local_get4 as Op)
            && eqz.op_eq(vm::op_i32_eqz as Op)
            && branch.op_eq(vm::op_br_if as Op)
    }

    fn cost(&self, ctx: &SelectionContext<'_>, _cursor: usize) -> i32 {
        30 + loop_bonus(ctx)
    }

    fn emit(&self, ctx: &SelectionContext<'_>, cursor: usize) -> Option<MatchResult> {
        let [load, _, branch] = ctx.block.insts.get(cursor..cursor + 3)? else {
            return None;
        };
        let taken = branch_target(branch)?;
        Some(MatchResult {
            group: self.group(),
            cost: self.cost(ctx, cursor),
            consumed: 3,
            ops: vec![KernelOp {
                label: None,
                op: vm::op_local_get4_i32_eqz_br_if as Op,
                operands: vec![
                    load.operands.first()?.clone(),
                    LoweredOperand::JumpTarget(taken),
                ],
                family: self.name(),
            }],
        })
    }
}

impl FamilySpec for LocalGetConstAddBrIfSpec {
    fn group(&self) -> FamilyGroup {
        FamilyGroup::LocalControl
    }

    fn name(&self) -> &'static str {
        "op_local_get4_i32_const_add_br_if"
    }

    fn matches(&self, ctx: &SelectionContext<'_>, cursor: usize) -> bool {
        let Some([load, konst, add, branch]) = ctx.block.insts.get(cursor..cursor + 4) else {
            return false;
        };
        load.op_eq(vm::op_local_get4 as Op)
            && konst.op_eq(vm::op_i32_const as Op)
            && add.op_eq(vm::op_i32_add as Op)
            && branch.op_eq(vm::op_br_if as Op)
    }

    fn cost(&self, ctx: &SelectionContext<'_>, _cursor: usize) -> i32 {
        40 + loop_bonus(ctx)
    }

    fn emit(&self, ctx: &SelectionContext<'_>, cursor: usize) -> Option<MatchResult> {
        let [load, konst, _, branch] = ctx.block.insts.get(cursor..cursor + 4)? else {
            return None;
        };
        let taken = branch_target(branch)?;
        Some(MatchResult {
            group: self.group(),
            cost: self.cost(ctx, cursor),
            consumed: 4,
            ops: vec![KernelOp {
                label: None,
                op: vm::op_local_get4_i32_const_add_br_if as Op,
                operands: vec![
                    load.operands.first()?.clone(),
                    konst.operands.first()?.clone(),
                    LoweredOperand::JumpTarget(taken),
                ],
                family: self.name(),
            }],
        })
    }
}

impl FamilySpec for LocalGetLocalAddBrIfSpec {
    fn group(&self) -> FamilyGroup {
        FamilyGroup::LocalControl
    }

    fn name(&self) -> &'static str {
        "op_local_get4_local_get4_i32_add_br_if"
    }

    fn matches(&self, ctx: &SelectionContext<'_>, cursor: usize) -> bool {
        let Some([lhs, rhs, add, branch]) = ctx.block.insts.get(cursor..cursor + 4) else {
            return false;
        };
        lhs.op_eq(vm::op_local_get4 as Op)
            && rhs.op_eq(vm::op_local_get4 as Op)
            && add.op_eq(vm::op_i32_add as Op)
            && branch.op_eq(vm::op_br_if as Op)
    }

    fn cost(&self, ctx: &SelectionContext<'_>, _cursor: usize) -> i32 {
        45 + loop_bonus(ctx)
    }

    fn emit(&self, ctx: &SelectionContext<'_>, cursor: usize) -> Option<MatchResult> {
        let [lhs, rhs, _, branch] = ctx.block.insts.get(cursor..cursor + 4)? else {
            return None;
        };
        let taken = branch_target(branch)?;
        Some(MatchResult {
            group: self.group(),
            cost: self.cost(ctx, cursor),
            consumed: 4,
            ops: vec![KernelOp {
                label: None,
                op: vm::op_local_get4_local_get4_i32_add_br_if as Op,
                operands: vec![
                    lhs.operands.first()?.clone(),
                    rhs.operands.first()?.clone(),
                    LoweredOperand::JumpTarget(taken),
                ],
                family: self.name(),
            }],
        })
    }
}

impl FamilySpec for LocalGetConstAddRootSpec {
    fn group(&self) -> FamilyGroup {
        FamilyGroup::LocalControl
    }

    fn name(&self) -> &'static str {
        "op_local_get4_i32_const_add"
    }

    fn matches(&self, ctx: &SelectionContext<'_>, cursor: usize) -> bool {
        let Some([load, konst, add]) = ctx.block.insts.get(cursor..cursor + 3) else {
            return false;
        };
        load.op_eq(vm::op_local_get4 as Op)
            && konst.op_eq(vm::op_i32_const as Op)
            && add.op_eq(vm::op_i32_add as Op)
    }

    fn cost(&self, ctx: &SelectionContext<'_>, _cursor: usize) -> i32 {
        26 + loop_bonus(ctx)
    }

    fn emit(&self, ctx: &SelectionContext<'_>, cursor: usize) -> Option<MatchResult> {
        let [load, konst, _] = ctx.block.insts.get(cursor..cursor + 3)? else {
            return None;
        };
        Some(MatchResult {
            group: self.group(),
            cost: self.cost(ctx, cursor),
            consumed: 3,
            ops: vec![KernelOp {
                label: None,
                op: vm::op_local_get4_i32_const_add as Op,
                operands: vec![
                    load.operands.first()?.clone(),
                    konst.operands.first()?.clone(),
                ],
                family: self.name(),
            }],
        })
    }
}

impl FamilySpec for LocalGetConstAddSetSpec {
    fn group(&self) -> FamilyGroup {
        FamilyGroup::LocalControl
    }

    fn name(&self) -> &'static str {
        "op_local_get4_i32_const_add_set4"
    }

    fn matches(&self, ctx: &SelectionContext<'_>, cursor: usize) -> bool {
        let Some([load, konst, add, set]) = ctx.block.insts.get(cursor..cursor + 4) else {
            return false;
        };
        load.op_eq(vm::op_local_get4 as Op)
            && konst.op_eq(vm::op_i32_const as Op)
            && add.op_eq(vm::op_i32_add as Op)
            && set.op_eq(vm::op_local_set4 as Op)
    }

    fn cost(&self, ctx: &SelectionContext<'_>, _cursor: usize) -> i32 {
        30 + loop_bonus(ctx)
    }

    fn emit(&self, ctx: &SelectionContext<'_>, cursor: usize) -> Option<MatchResult> {
        let [load, konst, _, set] = ctx.block.insts.get(cursor..cursor + 4)? else {
            return None;
        };
        Some(MatchResult {
            group: self.group(),
            cost: self.cost(ctx, cursor),
            consumed: 4,
            ops: vec![KernelOp {
                label: None,
                op: vm::op_local_get4_i32_const_add_set4 as Op,
                operands: vec![
                    load.operands.first()?.clone(),
                    konst.operands.first()?.clone(),
                    set.operands.first()?.clone(),
                ],
                family: self.name(),
            }],
        })
    }
}

impl FamilySpec for LocalGetConstAddTeeSpec {
    fn group(&self) -> FamilyGroup {
        FamilyGroup::LocalControl
    }

    fn name(&self) -> &'static str {
        "op_local_get4_i32_const_add_tee4"
    }

    fn matches(&self, ctx: &SelectionContext<'_>, cursor: usize) -> bool {
        let Some([load, konst, add, tee]) = ctx.block.insts.get(cursor..cursor + 4) else {
            return false;
        };
        load.op_eq(vm::op_local_get4 as Op)
            && konst.op_eq(vm::op_i32_const as Op)
            && add.op_eq(vm::op_i32_add as Op)
            && tee.op_eq(vm::op_local_tee4 as Op)
    }

    fn cost(&self, ctx: &SelectionContext<'_>, _cursor: usize) -> i32 {
        31 + loop_bonus(ctx)
    }

    fn emit(&self, ctx: &SelectionContext<'_>, cursor: usize) -> Option<MatchResult> {
        let [load, konst, _, tee] = ctx.block.insts.get(cursor..cursor + 4)? else {
            return None;
        };
        Some(MatchResult {
            group: self.group(),
            cost: self.cost(ctx, cursor),
            consumed: 4,
            ops: vec![KernelOp {
                label: None,
                op: vm::op_local_get4_i32_const_add_tee4 as Op,
                operands: vec![
                    load.operands.first()?.clone(),
                    konst.operands.first()?.clone(),
                    tee.operands.first()?.clone(),
                ],
                family: self.name(),
            }],
        })
    }
}

impl FamilySpec for LocalGetLocalAddRootSpec {
    fn group(&self) -> FamilyGroup {
        FamilyGroup::LocalControl
    }

    fn name(&self) -> &'static str {
        "op_local_get4_local_get4_i32_add"
    }

    fn matches(&self, ctx: &SelectionContext<'_>, cursor: usize) -> bool {
        let Some([lhs, rhs, add]) = ctx.block.insts.get(cursor..cursor + 3) else {
            return false;
        };
        lhs.op_eq(vm::op_local_get4 as Op)
            && rhs.op_eq(vm::op_local_get4 as Op)
            && add.op_eq(vm::op_i32_add as Op)
    }

    fn cost(&self, ctx: &SelectionContext<'_>, _cursor: usize) -> i32 {
        28 + loop_bonus(ctx)
    }

    fn emit(&self, ctx: &SelectionContext<'_>, cursor: usize) -> Option<MatchResult> {
        let [lhs, rhs, _] = ctx.block.insts.get(cursor..cursor + 3)? else {
            return None;
        };
        Some(MatchResult {
            group: self.group(),
            cost: self.cost(ctx, cursor),
            consumed: 3,
            ops: vec![KernelOp {
                label: None,
                op: vm::op_local_get4_local_get4_i32_add as Op,
                operands: vec![lhs.operands.first()?.clone(), rhs.operands.first()?.clone()],
                family: self.name(),
            }],
        })
    }
}

impl FamilySpec for LocalGetLocalAddSetSpec {
    fn group(&self) -> FamilyGroup {
        FamilyGroup::LocalControl
    }

    fn name(&self) -> &'static str {
        "op_local_get4_local_get4_i32_add_set4"
    }

    fn matches(&self, ctx: &SelectionContext<'_>, cursor: usize) -> bool {
        let Some([lhs, rhs, add, set]) = ctx.block.insts.get(cursor..cursor + 4) else {
            return false;
        };
        lhs.op_eq(vm::op_local_get4 as Op)
            && rhs.op_eq(vm::op_local_get4 as Op)
            && add.op_eq(vm::op_i32_add as Op)
            && set.op_eq(vm::op_local_set4 as Op)
    }

    fn cost(&self, ctx: &SelectionContext<'_>, _cursor: usize) -> i32 {
        32 + loop_bonus(ctx)
    }

    fn emit(&self, ctx: &SelectionContext<'_>, cursor: usize) -> Option<MatchResult> {
        let [lhs, rhs, _, set] = ctx.block.insts.get(cursor..cursor + 4)? else {
            return None;
        };
        Some(MatchResult {
            group: self.group(),
            cost: self.cost(ctx, cursor),
            consumed: 4,
            ops: vec![KernelOp {
                label: None,
                op: vm::op_local_get4_local_get4_i32_add_set4 as Op,
                operands: vec![
                    lhs.operands.first()?.clone(),
                    rhs.operands.first()?.clone(),
                    set.operands.first()?.clone(),
                ],
                family: self.name(),
            }],
        })
    }
}

impl FamilySpec for LocalGetLocalAddTeeSpec {
    fn group(&self) -> FamilyGroup {
        FamilyGroup::LocalControl
    }

    fn name(&self) -> &'static str {
        "op_local_get4_local_get4_i32_add_tee4"
    }

    fn matches(&self, ctx: &SelectionContext<'_>, cursor: usize) -> bool {
        let Some([lhs, rhs, add, tee]) = ctx.block.insts.get(cursor..cursor + 4) else {
            return false;
        };
        lhs.op_eq(vm::op_local_get4 as Op)
            && rhs.op_eq(vm::op_local_get4 as Op)
            && add.op_eq(vm::op_i32_add as Op)
            && tee.op_eq(vm::op_local_tee4 as Op)
    }

    fn cost(&self, ctx: &SelectionContext<'_>, _cursor: usize) -> i32 {
        33 + loop_bonus(ctx)
    }

    fn emit(&self, ctx: &SelectionContext<'_>, cursor: usize) -> Option<MatchResult> {
        let [lhs, rhs, _, tee] = ctx.block.insts.get(cursor..cursor + 4)? else {
            return None;
        };
        Some(MatchResult {
            group: self.group(),
            cost: self.cost(ctx, cursor),
            consumed: 4,
            ops: vec![KernelOp {
                label: None,
                op: vm::op_local_get4_local_get4_i32_add_tee4 as Op,
                operands: vec![
                    lhs.operands.first()?.clone(),
                    rhs.operands.first()?.clone(),
                    tee.operands.first()?.clone(),
                ],
                family: self.name(),
            }],
        })
    }
}

impl FamilySpec for LocalGetConstCompareBrIfSpec {
    fn group(&self) -> FamilyGroup {
        FamilyGroup::LocalControl
    }

    fn name(&self) -> &'static str {
        "op_local_get4_i32_const_compare_br_if"
    }

    fn matches(&self, ctx: &SelectionContext<'_>, cursor: usize) -> bool {
        self.emit(ctx, cursor).is_some()
    }

    fn cost(&self, ctx: &SelectionContext<'_>, _cursor: usize) -> i32 {
        42 + loop_bonus(ctx)
    }

    fn emit(&self, ctx: &SelectionContext<'_>, cursor: usize) -> Option<MatchResult> {
        let [first, second, compare, branch] = ctx.block.insts.get(cursor..cursor + 4)? else {
            return None;
        };
        if !branch.op_eq(vm::op_br_if as Op) {
            return None;
        }
        let taken = branch_target(branch)?;
        let compare_kind = i32_compare_kind(compare.op)?;

        let (lhs, rhs, encoded_kind) =
            if first.op_eq(vm::op_local_get4 as Op) && second.op_eq(vm::op_i32_const as Op) {
                (
                    first.operands.first()?.clone(),
                    second.operands.first()?.clone(),
                    compare_kind,
                )
            } else if first.op_eq(vm::op_i32_const as Op) && second.op_eq(vm::op_local_get4 as Op) {
                (
                    second.operands.first()?.clone(),
                    first.operands.first()?.clone(),
                    flip_i32_compare_kind(compare_kind)?,
                )
            } else {
                return None;
            };

        Some(MatchResult {
            group: self.group(),
            cost: self.cost(ctx, cursor),
            consumed: 4,
            ops: vec![KernelOp {
                label: None,
                op: vm::op_local_get4_i32_const_compare_br_if as Op,
                operands: vec![
                    lhs,
                    raw_u32_operand(encoded_kind),
                    rhs,
                    LoweredOperand::JumpTarget(taken),
                ],
                family: self.name(),
            }],
        })
    }
}

impl FamilySpec for LocalGetLocalCompareBrIfSpec {
    fn group(&self) -> FamilyGroup {
        FamilyGroup::LocalControl
    }

    fn name(&self) -> &'static str {
        "op_local_get4_local_get4_compare_br_if"
    }

    fn matches(&self, ctx: &SelectionContext<'_>, cursor: usize) -> bool {
        let Some([lhs, rhs, compare, branch]) = ctx.block.insts.get(cursor..cursor + 4) else {
            return false;
        };
        lhs.op_eq(vm::op_local_get4 as Op)
            && rhs.op_eq(vm::op_local_get4 as Op)
            && i32_compare_kind(compare.op).is_some()
            && branch.op_eq(vm::op_br_if as Op)
    }

    fn cost(&self, ctx: &SelectionContext<'_>, _cursor: usize) -> i32 {
        44 + loop_bonus(ctx)
    }

    fn emit(&self, ctx: &SelectionContext<'_>, cursor: usize) -> Option<MatchResult> {
        let [lhs, rhs, compare, branch] = ctx.block.insts.get(cursor..cursor + 4)? else {
            return None;
        };
        let taken = branch_target(branch)?;
        let compare_kind = i32_compare_kind(compare.op)?;
        Some(MatchResult {
            group: self.group(),
            cost: self.cost(ctx, cursor),
            consumed: 4,
            ops: vec![KernelOp {
                label: None,
                op: vm::op_local_get4_local_get4_compare_br_if as Op,
                operands: vec![
                    lhs.operands.first()?.clone(),
                    rhs.operands.first()?.clone(),
                    raw_u32_operand(compare_kind),
                    LoweredOperand::JumpTarget(taken),
                ],
                family: self.name(),
            }],
        })
    }
}

impl FamilySpec for LocalGetConstAddTeeBrIfSpec {
    fn group(&self) -> FamilyGroup {
        FamilyGroup::LocalControl
    }

    fn name(&self) -> &'static str {
        "op_local_get4_i32_const_add_tee4_br_if"
    }

    fn matches(&self, ctx: &SelectionContext<'_>, cursor: usize) -> bool {
        let Some([load, konst, add, tee, branch]) = ctx.block.insts.get(cursor..cursor + 5) else {
            return false;
        };
        load.op_eq(vm::op_local_get4 as Op)
            && konst.op_eq(vm::op_i32_const as Op)
            && add.op_eq(vm::op_i32_add as Op)
            && tee.op_eq(vm::op_local_tee4 as Op)
            && branch.op_eq(vm::op_br_if as Op)
    }

    fn cost(&self, ctx: &SelectionContext<'_>, _cursor: usize) -> i32 {
        50 + loop_bonus(ctx)
    }

    fn emit(&self, ctx: &SelectionContext<'_>, cursor: usize) -> Option<MatchResult> {
        let [load, konst, _, tee, branch] = ctx.block.insts.get(cursor..cursor + 5)? else {
            return None;
        };
        let taken = branch_target(branch)?;
        Some(MatchResult {
            group: self.group(),
            cost: self.cost(ctx, cursor),
            consumed: 5,
            ops: vec![KernelOp {
                label: None,
                op: vm::op_local_get4_i32_const_add_tee4_br_if as Op,
                operands: vec![
                    load.operands.first()?.clone(),
                    konst.operands.first()?.clone(),
                    tee.operands.first()?.clone(),
                    LoweredOperand::JumpTarget(taken),
                ],
                family: self.name(),
            }],
        })
    }
}

#[derive(Clone, Copy)]
enum UnaryMode {
    Root,
    Set,
    Tee,
}

#[derive(Clone, Copy)]
struct LocalUnarySpec {
    width: u32,
    mode: UnaryMode,
    op: Op,
    label: &'static str,
}

impl LocalUnarySpec {
    const ROOT32: Self = Self {
        width: 4,
        mode: UnaryMode::Root,
        op: vm::op_local_unary32 as Op,
        label: "op_local_unary32",
    };
    const SET32: Self = Self {
        width: 4,
        mode: UnaryMode::Set,
        op: vm::op_local_unary32_set4 as Op,
        label: "op_local_unary32_set4",
    };
    const TEE32: Self = Self {
        width: 4,
        mode: UnaryMode::Tee,
        op: vm::op_local_unary32_tee4 as Op,
        label: "op_local_unary32_tee4",
    };
    const ROOT64: Self = Self {
        width: 8,
        mode: UnaryMode::Root,
        op: vm::op_local_unary64 as Op,
        label: "op_local_unary64",
    };
    const SET64: Self = Self {
        width: 8,
        mode: UnaryMode::Set,
        op: vm::op_local_unary64_set8 as Op,
        label: "op_local_unary64_set8",
    };
    const TEE64: Self = Self {
        width: 8,
        mode: UnaryMode::Tee,
        op: vm::op_local_unary64_tee8 as Op,
        label: "op_local_unary64_tee8",
    };
}

impl FamilySpec for LocalUnarySpec {
    fn group(&self) -> FamilyGroup {
        FamilyGroup::LocalControl
    }

    fn name(&self) -> &'static str {
        self.label
    }

    fn matches(&self, ctx: &SelectionContext<'_>, cursor: usize) -> bool {
        self.emit(ctx, cursor).is_some()
    }

    fn cost(&self, ctx: &SelectionContext<'_>, _cursor: usize) -> i32 {
        let base = match self.mode {
            UnaryMode::Root => 14,
            UnaryMode::Set | UnaryMode::Tee => 18,
        };
        base + loop_bonus(ctx)
    }

    fn emit(&self, ctx: &SelectionContext<'_>, cursor: usize) -> Option<MatchResult> {
        let (operands, consumed) = match_local_unary(ctx.block, cursor, self.width, self.mode)?;
        Some(MatchResult {
            group: self.group(),
            cost: self.cost(ctx, cursor),
            consumed,
            ops: vec![KernelOp {
                label: None,
                op: self.op,
                operands,
                family: self.name(),
            }],
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum NumericClass {
    Binop32,
    Binop64,
    Cmp32,
    Cmp64,
}

#[derive(Clone, Copy)]
enum NumericConsumer {
    Root,
    Set,
    Tee,
    BrIf,
}

#[derive(Clone, Copy)]
struct LocalNumericSpec {
    class: NumericClass,
    consumer: NumericConsumer,
    op: Op,
    label: &'static str,
}

impl LocalNumericSpec {
    const BINOP32_ROOT: Self = Self {
        class: NumericClass::Binop32,
        consumer: NumericConsumer::Root,
        op: vm::op_local_binop32 as Op,
        label: "op_local_binop32",
    };
    const BINOP32_SET: Self = Self {
        class: NumericClass::Binop32,
        consumer: NumericConsumer::Set,
        op: vm::op_local_binop32_set4 as Op,
        label: "op_local_binop32_set4",
    };
    const BINOP32_TEE: Self = Self {
        class: NumericClass::Binop32,
        consumer: NumericConsumer::Tee,
        op: vm::op_local_binop32_tee4 as Op,
        label: "op_local_binop32_tee4",
    };
    const BINOP32_BR_IF: Self = Self {
        class: NumericClass::Binop32,
        consumer: NumericConsumer::BrIf,
        op: vm::op_local_binop32_br_if as Op,
        label: "op_local_binop32_br_if",
    };
    const BINOP64_ROOT: Self = Self {
        class: NumericClass::Binop64,
        consumer: NumericConsumer::Root,
        op: vm::op_local_binop64 as Op,
        label: "op_local_binop64",
    };
    const BINOP64_SET: Self = Self {
        class: NumericClass::Binop64,
        consumer: NumericConsumer::Set,
        op: vm::op_local_binop64_set8 as Op,
        label: "op_local_binop64_set8",
    };
    const BINOP64_TEE: Self = Self {
        class: NumericClass::Binop64,
        consumer: NumericConsumer::Tee,
        op: vm::op_local_binop64_tee8 as Op,
        label: "op_local_binop64_tee8",
    };
    const CMP32_ROOT: Self = Self {
        class: NumericClass::Cmp32,
        consumer: NumericConsumer::Root,
        op: vm::op_local_cmp32 as Op,
        label: "op_local_cmp32",
    };
    const CMP32_SET: Self = Self {
        class: NumericClass::Cmp32,
        consumer: NumericConsumer::Set,
        op: vm::op_local_cmp32_set4 as Op,
        label: "op_local_cmp32_set4",
    };
    const CMP32_TEE: Self = Self {
        class: NumericClass::Cmp32,
        consumer: NumericConsumer::Tee,
        op: vm::op_local_cmp32_tee4 as Op,
        label: "op_local_cmp32_tee4",
    };
    const CMP32_BR_IF: Self = Self {
        class: NumericClass::Cmp32,
        consumer: NumericConsumer::BrIf,
        op: vm::op_local_cmp32_br_if as Op,
        label: "op_local_cmp32_br_if",
    };
    const CMP64_ROOT: Self = Self {
        class: NumericClass::Cmp64,
        consumer: NumericConsumer::Root,
        op: vm::op_local_cmp64 as Op,
        label: "op_local_cmp64",
    };
    const CMP64_SET: Self = Self {
        class: NumericClass::Cmp64,
        consumer: NumericConsumer::Set,
        op: vm::op_local_cmp64_set4 as Op,
        label: "op_local_cmp64_set4",
    };
    const CMP64_TEE: Self = Self {
        class: NumericClass::Cmp64,
        consumer: NumericConsumer::Tee,
        op: vm::op_local_cmp64_tee4 as Op,
        label: "op_local_cmp64_tee4",
    };
    const CMP64_BR_IF: Self = Self {
        class: NumericClass::Cmp64,
        consumer: NumericConsumer::BrIf,
        op: vm::op_local_cmp64_br_if as Op,
        label: "op_local_cmp64_br_if",
    };
}

impl FamilySpec for LocalNumericSpec {
    fn group(&self) -> FamilyGroup {
        FamilyGroup::LocalControl
    }

    fn name(&self) -> &'static str {
        self.label
    }

    fn matches(&self, ctx: &SelectionContext<'_>, cursor: usize) -> bool {
        self.emit(ctx, cursor).is_some()
    }

    fn cost(&self, ctx: &SelectionContext<'_>, _cursor: usize) -> i32 {
        let base = match self.consumer {
            NumericConsumer::Root => 16,
            NumericConsumer::Set | NumericConsumer::Tee => 22,
            NumericConsumer::BrIf => 28,
        };
        base + loop_bonus(ctx)
    }

    fn emit(&self, ctx: &SelectionContext<'_>, cursor: usize) -> Option<MatchResult> {
        let (operands, consumed) =
            match_local_numeric(ctx.block, cursor, self.class, self.consumer)?;
        Some(MatchResult {
            group: self.group(),
            cost: self.cost(ctx, cursor),
            consumed,
            ops: vec![KernelOp {
                label: None,
                op: self.op,
                operands,
                family: self.name(),
            }],
        })
    }
}

struct SelectWidthSpec {
    width: u32,
    op: Op,
    label: &'static str,
}

#[derive(Clone, Copy)]
struct CallPassthroughSpec {
    op: Op,
    label: &'static str,
}

impl CallPassthroughSpec {
    const CALL: Self = Self {
        op: vm::op_call as Op,
        label: "op_call",
    };
    const CALL_IMPORT: Self = Self {
        op: vm::op_call_import as Op,
        label: "op_call_import",
    };
    const RETURN_CALL: Self = Self {
        op: vm::op_return_call as Op,
        label: "op_return_call",
    };
    const RETURN_CALL_IMPORT: Self = Self {
        op: vm::op_return_call_import as Op,
        label: "op_return_call_import",
    };
    const CALL_INDIRECT: Self = Self {
        op: vm::op_call_indirect as Op,
        label: "op_call_indirect",
    };
    const RETURN_CALL_INDIRECT: Self = Self {
        op: vm::op_return_call_indirect as Op,
        label: "op_return_call_indirect",
    };
}

impl FamilySpec for CallPassthroughSpec {
    fn group(&self) -> FamilyGroup {
        FamilyGroup::CallSelect
    }

    fn name(&self) -> &'static str {
        self.label
    }

    fn matches(&self, ctx: &SelectionContext<'_>, cursor: usize) -> bool {
        let Some(inst) = ctx.block.insts.get(cursor) else {
            return false;
        };
        inst.op_eq(self.op)
    }

    fn cost(&self, ctx: &SelectionContext<'_>, _cursor: usize) -> i32 {
        12 + loop_bonus(ctx)
    }

    fn emit(&self, ctx: &SelectionContext<'_>, cursor: usize) -> Option<MatchResult> {
        let inst = ctx.block.insts.get(cursor)?;
        Some(MatchResult {
            group: self.group(),
            cost: self.cost(ctx, cursor),
            consumed: 1,
            ops: vec![KernelOp {
                label: None,
                op: inst.op,
                operands: inst.operands.clone(),
                family: self.name(),
            }],
        })
    }
}

#[derive(Clone, Copy)]
struct CallSpec {
    op: Op,
    label: &'static str,
}

impl CallSpec {
    const DIRECT: Self = Self {
        op: vm::op_call as Op,
        label: "op_call",
    };
    const DIRECT_IMPORT: Self = Self {
        op: vm::op_call_import as Op,
        label: "op_call_import",
    };
    const RETURN: Self = Self {
        op: vm::op_return_call as Op,
        label: "op_return_call",
    };
    const RETURN_IMPORT: Self = Self {
        op: vm::op_return_call_import as Op,
        label: "op_return_call_import",
    };
    const INDIRECT: Self = Self {
        op: vm::op_call_indirect as Op,
        label: "op_call_indirect",
    };
    const RETURN_INDIRECT: Self = Self {
        op: vm::op_return_call_indirect as Op,
        label: "op_return_call_indirect",
    };
}

impl FamilySpec for CallSpec {
    fn group(&self) -> FamilyGroup {
        FamilyGroup::CallSelect
    }

    fn name(&self) -> &'static str {
        self.label
    }

    fn matches(&self, ctx: &SelectionContext<'_>, cursor: usize) -> bool {
        ctx.block
            .insts
            .get(cursor)
            .map(|inst| inst.op_eq(self.op))
            .unwrap_or(false)
    }

    fn cost(&self, ctx: &SelectionContext<'_>, _cursor: usize) -> i32 {
        12 + loop_bonus(ctx)
    }

    fn emit(&self, ctx: &SelectionContext<'_>, cursor: usize) -> Option<MatchResult> {
        let inst = ctx.block.insts.get(cursor)?;
        Some(MatchResult {
            group: self.group(),
            cost: self.cost(ctx, cursor),
            consumed: 1,
            ops: vec![KernelOp {
                label: None,
                op: inst.op,
                operands: inst.operands.clone(),
                family: self.name(),
            }],
        })
    }
}

impl SelectWidthSpec {
    const FOUR: Self = Self {
        width: 4,
        op: vm::op_select4 as Op,
        label: "op_select4",
    };
    const EIGHT: Self = Self {
        width: 8,
        op: vm::op_select8 as Op,
        label: "op_select8",
    };
    const SIXTEEN: Self = Self {
        width: 16,
        op: vm::op_select16 as Op,
        label: "op_select16",
    };
}

impl FamilySpec for SelectWidthSpec {
    fn group(&self) -> FamilyGroup {
        FamilyGroup::CallSelect
    }

    fn name(&self) -> &'static str {
        self.label
    }

    fn matches(&self, ctx: &SelectionContext<'_>, cursor: usize) -> bool {
        let Some(inst) = ctx.block.insts.get(cursor) else {
            return false;
        };
        inst.op_eq(vm::op_select as Op) && raw_select(inst.operands.first()) == Some(self.width)
    }

    fn cost(&self, ctx: &SelectionContext<'_>, _cursor: usize) -> i32 {
        15 + loop_bonus(ctx)
    }

    fn emit(&self, ctx: &SelectionContext<'_>, cursor: usize) -> Option<MatchResult> {
        ctx.block.insts.get(cursor)?;
        Some(MatchResult {
            group: self.group(),
            cost: self.cost(ctx, cursor),
            consumed: 1,
            ops: vec![KernelOp {
                label: None,
                op: self.op,
                operands: Vec::new(),
                family: self.name(),
            }],
        })
    }
}

struct I32LoadConstBaseSpec;
struct I32StoreConstBaseLocal4Spec;
struct I32LoadConstBaseLocalGet4AddSet4Spec;
struct I32LoadLocalBaseSpec;
struct I32StoreLocalBaseLocalGet4Spec;
#[allow(dead_code)]
struct I32StoreLocalBaseSpec;
struct I32LoadLocalScaledIndexSpec;
struct I32StoreLocalScaledIndexLocalGet4Spec;
struct I32StoreLocalScaledIndexSpec;

impl FamilySpec for I32LoadConstBaseSpec {
    fn group(&self) -> FamilyGroup {
        FamilyGroup::Memory
    }

    fn name(&self) -> &'static str {
        "op_i32_load_const_base"
    }

    fn matches(&self, ctx: &SelectionContext<'_>, cursor: usize) -> bool {
        let Some([konst, load]) = ctx.block.insts.get(cursor..cursor + 2) else {
            return false;
        };
        konst.op_eq(vm::op_i32_const as Op) && load.op_eq(vm::op_i32_load as Op)
    }

    fn cost(&self, ctx: &SelectionContext<'_>, _cursor: usize) -> i32 {
        18 + loop_bonus(ctx)
    }

    fn emit(&self, ctx: &SelectionContext<'_>, cursor: usize) -> Option<MatchResult> {
        let [konst, load] = ctx.block.insts.get(cursor..cursor + 2)? else {
            return None;
        };
        let folded = fold_const_base_memarg(konst.operands.first(), load.operands.first())?;
        Some(MatchResult {
            group: self.group(),
            cost: self.cost(ctx, cursor),
            consumed: 2,
            ops: vec![KernelOp {
                label: None,
                op: vm::op_i32_load_const_base as Op,
                operands: vec![LoweredOperand::Raw(unsafe { folded.encoded })],
                family: self.name(),
            }],
        })
    }
}

impl FamilySpec for I32StoreConstBaseLocal4Spec {
    fn group(&self) -> FamilyGroup {
        FamilyGroup::Memory
    }

    fn name(&self) -> &'static str {
        "op_i32_store_const_base_local4"
    }

    fn matches(&self, ctx: &SelectionContext<'_>, cursor: usize) -> bool {
        let Some([konst, local, store]) = ctx.block.insts.get(cursor..cursor + 3) else {
            return false;
        };
        konst.op_eq(vm::op_i32_const as Op)
            && local.op_eq(vm::op_local_get4 as Op)
            && store.op_eq(vm::op_i32_store as Op)
    }

    fn cost(&self, ctx: &SelectionContext<'_>, _cursor: usize) -> i32 {
        24 + loop_bonus(ctx)
    }

    fn emit(&self, ctx: &SelectionContext<'_>, cursor: usize) -> Option<MatchResult> {
        let [konst, local, store] = ctx.block.insts.get(cursor..cursor + 3)? else {
            return None;
        };
        let folded = fold_const_base_memarg(konst.operands.first(), store.operands.first())?;
        Some(MatchResult {
            group: self.group(),
            cost: self.cost(ctx, cursor),
            consumed: 3,
            ops: vec![KernelOp {
                label: None,
                op: vm::op_i32_store_const_base_local4 as Op,
                operands: vec![
                    LoweredOperand::Raw(unsafe { folded.encoded }),
                    local.operands.first()?.clone(),
                ],
                family: self.name(),
            }],
        })
    }
}

impl FamilySpec for I32LoadConstBaseLocalGet4AddSet4Spec {
    fn group(&self) -> FamilyGroup {
        FamilyGroup::Memory
    }

    fn name(&self) -> &'static str {
        "op_i32_load_const_base_local_get4_i32_add_set4"
    }

    fn matches(&self, ctx: &SelectionContext<'_>, cursor: usize) -> bool {
        let Some([konst, load, rhs, add, set]) = ctx.block.insts.get(cursor..cursor + 5) else {
            return false;
        };
        konst.op_eq(vm::op_i32_const as Op)
            && load.op_eq(vm::op_i32_load as Op)
            && rhs.op_eq(vm::op_local_get4 as Op)
            && add.op_eq(vm::op_i32_add as Op)
            && set.op_eq(vm::op_local_set4 as Op)
    }

    fn cost(&self, ctx: &SelectionContext<'_>, _cursor: usize) -> i32 {
        32 + loop_bonus(ctx)
    }

    fn emit(&self, ctx: &SelectionContext<'_>, cursor: usize) -> Option<MatchResult> {
        let [konst, load, rhs, _, set] = ctx.block.insts.get(cursor..cursor + 5)? else {
            return None;
        };
        let folded = fold_const_base_memarg(konst.operands.first(), load.operands.first())?;
        Some(MatchResult {
            group: self.group(),
            cost: self.cost(ctx, cursor),
            consumed: 5,
            ops: vec![KernelOp {
                label: None,
                op: vm::op_i32_load_const_base_local_get4_i32_add_set4 as Op,
                operands: vec![
                    LoweredOperand::Raw(unsafe { folded.encoded }),
                    rhs.operands.first()?.clone(),
                    set.operands.first()?.clone(),
                ],
                family: self.name(),
            }],
        })
    }
}

impl FamilySpec for I32LoadLocalBaseSpec {
    fn group(&self) -> FamilyGroup {
        FamilyGroup::Memory
    }

    fn name(&self) -> &'static str {
        "op_i32_load_local_base"
    }

    fn matches(&self, ctx: &SelectionContext<'_>, cursor: usize) -> bool {
        match_i32_local_base_address(ctx.block, cursor)
            .and_then(|matched| ctx.block.insts.get(cursor + matched.consumed))
            .is_some_and(|load| i32_local_base_load_family(load.op).is_some())
    }

    fn cost(&self, ctx: &SelectionContext<'_>, _cursor: usize) -> i32 {
        26 + loop_bonus(ctx)
    }

    fn emit(&self, ctx: &SelectionContext<'_>, cursor: usize) -> Option<MatchResult> {
        let matched = match_i32_local_base_address(ctx.block, cursor)?;
        let load = ctx.block.insts.get(cursor + matched.consumed)?;
        let op = i32_local_base_load_family(load.op)?;
        let mut operands = vec![matched.base_local, raw_i32_operand(matched.delta)];
        operands.extend(load.operands.clone());
        Some(MatchResult {
            group: self.group(),
            cost: self.cost(ctx, cursor),
            consumed: matched.consumed + 1,
            ops: vec![KernelOp {
                label: None,
                op,
                operands,
                family: self.name(),
            }],
        })
    }
}

impl FamilySpec for I32StoreLocalBaseSpec {
    fn group(&self) -> FamilyGroup {
        FamilyGroup::Memory
    }

    fn name(&self) -> &'static str {
        "op_i32_store_local_base"
    }

    fn matches(&self, ctx: &SelectionContext<'_>, cursor: usize) -> bool {
        let Some(addr) = match_i32_local_base_address(ctx.block, cursor) else {
            return false;
        };
        let Some((_, value_consumed)) = match_i32_value_expr(ctx, cursor + addr.consumed) else {
            return false;
        };
        ctx.block
            .insts
            .get(cursor + addr.consumed + value_consumed)
            .is_some_and(|store| store.op_eq(vm::op_i32_store as Op))
    }

    fn cost(&self, ctx: &SelectionContext<'_>, _cursor: usize) -> i32 {
        30 + loop_bonus(ctx)
    }

    fn emit(&self, ctx: &SelectionContext<'_>, cursor: usize) -> Option<MatchResult> {
        let addr = match_i32_local_base_address(ctx.block, cursor)?;
        let (mut value_ops, value_consumed) = match_i32_value_expr(ctx, cursor + addr.consumed)?;
        let store = ctx
            .block
            .insts
            .get(cursor + addr.consumed + value_consumed)?;
        if !store.op_eq(vm::op_i32_store as Op) {
            return None;
        }
        value_ops.push(KernelOp {
            label: None,
            op: vm::op_i32_store_local_base as Op,
            operands: vec![
                addr.base_local,
                raw_i32_operand(addr.delta),
                store.operands.first()?.clone(),
            ],
            family: self.name(),
        });
        Some(MatchResult {
            group: self.group(),
            cost: self.cost(ctx, cursor),
            consumed: addr.consumed + value_consumed + 1,
            ops: value_ops,
        })
    }
}

impl FamilySpec for I32StoreLocalBaseLocalGet4Spec {
    fn group(&self) -> FamilyGroup {
        FamilyGroup::Memory
    }

    fn name(&self) -> &'static str {
        "op_i32_store_local_base"
    }

    fn matches(&self, ctx: &SelectionContext<'_>, cursor: usize) -> bool {
        let Some(addr) = match_i32_local_base_address(ctx.block, cursor) else {
            return false;
        };
        ctx.block
            .insts
            .get(cursor + addr.consumed)
            .is_some_and(|inst| inst.op_eq(vm::op_local_get4 as Op))
            && ctx
                .block
                .insts
                .get(cursor + addr.consumed + 1)
                .is_some_and(|store| store.op_eq(vm::op_i32_store as Op))
    }

    fn cost(&self, ctx: &SelectionContext<'_>, _cursor: usize) -> i32 {
        34 + loop_bonus(ctx)
    }

    fn emit(&self, ctx: &SelectionContext<'_>, cursor: usize) -> Option<MatchResult> {
        let addr = match_i32_local_base_address(ctx.block, cursor)?;
        let value = ctx.block.insts.get(cursor + addr.consumed)?;
        let store = ctx.block.insts.get(cursor + addr.consumed + 1)?;
        if !value.op_eq(vm::op_local_get4 as Op) || !store.op_eq(vm::op_i32_store as Op) {
            return None;
        }
        Some(MatchResult {
            group: self.group(),
            cost: self.cost(ctx, cursor),
            consumed: addr.consumed + 2,
            ops: vec![
                KernelOp {
                    label: None,
                    op: value.op,
                    operands: value.operands.clone(),
                    family: "generic",
                },
                KernelOp {
                    label: None,
                    op: vm::op_i32_store_local_base as Op,
                    operands: vec![
                        addr.base_local,
                        raw_i32_operand(addr.delta),
                        store.operands.first()?.clone(),
                    ],
                    family: self.name(),
                },
            ],
        })
    }
}

impl FamilySpec for I32LoadLocalScaledIndexSpec {
    fn group(&self) -> FamilyGroup {
        FamilyGroup::Memory
    }

    fn name(&self) -> &'static str {
        "op_i32_load_local_scaled_index"
    }

    fn matches(&self, ctx: &SelectionContext<'_>, cursor: usize) -> bool {
        match_i32_local_scaled_index_address(ctx.block, cursor)
            .and_then(|matched| ctx.block.insts.get(cursor + matched.consumed))
            .is_some_and(|load| load.op_eq(vm::op_i32_load as Op))
    }

    fn cost(&self, ctx: &SelectionContext<'_>, _cursor: usize) -> i32 {
        32 + loop_bonus(ctx)
    }

    fn emit(&self, ctx: &SelectionContext<'_>, cursor: usize) -> Option<MatchResult> {
        let matched = match_i32_local_scaled_index_address(ctx.block, cursor)?;
        let load = ctx.block.insts.get(cursor + matched.consumed)?;
        if !load.op_eq(vm::op_i32_load as Op) {
            return None;
        }
        Some(MatchResult {
            group: self.group(),
            cost: self.cost(ctx, cursor),
            consumed: matched.consumed + 1,
            ops: vec![KernelOp {
                label: None,
                op: vm::op_i32_load_local_scaled_index as Op,
                operands: vec![
                    matched.base_local,
                    matched.index_local,
                    raw_u32_operand(matched.scale_log2),
                    raw_i32_operand(matched.delta),
                    load.operands.first()?.clone(),
                ],
                family: self.name(),
            }],
        })
    }
}

impl FamilySpec for I32StoreLocalScaledIndexSpec {
    fn group(&self) -> FamilyGroup {
        FamilyGroup::Memory
    }

    fn name(&self) -> &'static str {
        "op_i32_store_local_scaled_index"
    }

    fn matches(&self, ctx: &SelectionContext<'_>, cursor: usize) -> bool {
        let Some(addr) = match_i32_local_scaled_index_address(ctx.block, cursor) else {
            return false;
        };
        let Some((_, value_consumed)) = match_i32_value_expr(ctx, cursor + addr.consumed) else {
            return false;
        };
        ctx.block
            .insts
            .get(cursor + addr.consumed + value_consumed)
            .is_some_and(|store| store.op_eq(vm::op_i32_store as Op))
    }

    fn cost(&self, ctx: &SelectionContext<'_>, _cursor: usize) -> i32 {
        36 + loop_bonus(ctx)
    }

    fn emit(&self, ctx: &SelectionContext<'_>, cursor: usize) -> Option<MatchResult> {
        let addr = match_i32_local_scaled_index_address(ctx.block, cursor)?;
        let (mut value_ops, value_consumed) = match_i32_value_expr(ctx, cursor + addr.consumed)?;
        let store = ctx
            .block
            .insts
            .get(cursor + addr.consumed + value_consumed)?;
        if !store.op_eq(vm::op_i32_store as Op) {
            return None;
        }
        value_ops.push(KernelOp {
            label: None,
            op: vm::op_i32_store_local_scaled_index as Op,
            operands: vec![
                addr.base_local,
                addr.index_local,
                raw_u32_operand(addr.scale_log2),
                raw_i32_operand(addr.delta),
                store.operands.first()?.clone(),
            ],
            family: self.name(),
        });
        Some(MatchResult {
            group: self.group(),
            cost: self.cost(ctx, cursor),
            consumed: addr.consumed + value_consumed + 1,
            ops: value_ops,
        })
    }
}

impl FamilySpec for I32StoreLocalScaledIndexLocalGet4Spec {
    fn group(&self) -> FamilyGroup {
        FamilyGroup::Memory
    }

    fn name(&self) -> &'static str {
        "op_i32_store_local_scaled_index"
    }

    fn matches(&self, ctx: &SelectionContext<'_>, cursor: usize) -> bool {
        let Some(addr) = match_i32_local_scaled_index_address(ctx.block, cursor) else {
            return false;
        };
        ctx.block
            .insts
            .get(cursor + addr.consumed)
            .is_some_and(|inst| inst.op_eq(vm::op_local_get4 as Op))
            && ctx
                .block
                .insts
                .get(cursor + addr.consumed + 1)
                .is_some_and(|store| store.op_eq(vm::op_i32_store as Op))
    }

    fn cost(&self, ctx: &SelectionContext<'_>, _cursor: usize) -> i32 {
        40 + loop_bonus(ctx)
    }

    fn emit(&self, ctx: &SelectionContext<'_>, cursor: usize) -> Option<MatchResult> {
        let addr = match_i32_local_scaled_index_address(ctx.block, cursor)?;
        let value = ctx.block.insts.get(cursor + addr.consumed)?;
        let store = ctx.block.insts.get(cursor + addr.consumed + 1)?;
        if !value.op_eq(vm::op_local_get4 as Op) || !store.op_eq(vm::op_i32_store as Op) {
            return None;
        }
        Some(MatchResult {
            group: self.group(),
            cost: self.cost(ctx, cursor),
            consumed: addr.consumed + 2,
            ops: vec![
                KernelOp {
                    label: None,
                    op: value.op,
                    operands: value.operands.clone(),
                    family: "generic",
                },
                KernelOp {
                    label: None,
                    op: vm::op_i32_store_local_scaled_index as Op,
                    operands: vec![
                        addr.base_local,
                        addr.index_local,
                        raw_u32_operand(addr.scale_log2),
                        raw_i32_operand(addr.delta),
                        store.operands.first()?.clone(),
                    ],
                    family: self.name(),
                },
            ],
        })
    }
}

fn match_local_unary(
    block: &CanonBlock,
    cursor: usize,
    width: u32,
    mode: UnaryMode,
) -> Option<(Vec<LoweredOperand>, usize)> {
    let (src, unary, consumer, consumed) = match mode {
        UnaryMode::Root => (
            block.insts.get(cursor)?,
            block.insts.get(cursor + 1)?,
            None,
            2,
        ),
        UnaryMode::Set | UnaryMode::Tee => (
            block.insts.get(cursor)?,
            block.insts.get(cursor + 1)?,
            Some(block.insts.get(cursor + 2)?),
            3,
        ),
    };
    let src_operand = raw_local_get(src, width)?;
    let kind = unary_kind(unary.op)?;
    if kind.width() != width {
        return None;
    }
    let mut operands = vec![raw_u32_operand(kind.encode()), src_operand];
    match mode {
        UnaryMode::Root => {}
        UnaryMode::Set => operands.push(raw_local_set(consumer?, kind.width())?),
        UnaryMode::Tee => operands.push(raw_local_tee(consumer?, kind.width())?),
    }
    Some((operands, consumed))
}

#[derive(Debug, Clone)]
struct LocalBaseAddressMatch {
    base_local: LoweredOperand,
    delta: i32,
    consumed: usize,
}

#[derive(Debug, Clone)]
struct LocalScaledIndexAddressMatch {
    base_local: LoweredOperand,
    index_local: LoweredOperand,
    scale_log2: u32,
    delta: i32,
    consumed: usize,
}

fn match_i32_local_base_address(
    block: &CanonBlock,
    cursor: usize,
) -> Option<LocalBaseAddressMatch> {
    let direct = raw_local_get(block.insts.get(cursor)?, 4)?;
    if let Some(delta) = match_i32_const_add(block, cursor + 1) {
        return Some(LocalBaseAddressMatch {
            base_local: direct,
            delta,
            consumed: 3,
        });
    }
    Some(LocalBaseAddressMatch {
        base_local: direct,
        delta: 0,
        consumed: 1,
    })
}

fn match_i32_local_scaled_index_address(
    block: &CanonBlock,
    cursor: usize,
) -> Option<LocalScaledIndexAddressMatch> {
    let base_local = raw_local_get(block.insts.get(cursor)?, 4)?;
    let index_local = raw_local_get(block.insts.get(cursor + 1)?, 4)?;
    let mut consumed = 2usize;
    let mut scale_log2 = 0u32;
    if let Some(scale_inst) = block.insts.get(cursor + consumed) {
        if scale_inst.op_eq(vm::op_i32_const as Op)
            && block
                .insts
                .get(cursor + consumed + 1)
                .is_some_and(|inst| inst.op_eq(vm::op_i32_shl as Op))
        {
            let scale = raw_i32(scale_inst.operands.first())?;
            if !(0..=3).contains(&scale) {
                return None;
            }
            scale_log2 = u32::try_from(scale).ok()?;
            consumed += 2;
        }
    }
    if !block
        .insts
        .get(cursor + consumed)
        .is_some_and(|inst| inst.op_eq(vm::op_i32_add as Op))
    {
        return None;
    }
    consumed += 1;
    let mut delta = 0i32;
    if let Some(extra_delta) = match_i32_const_add(block, cursor + consumed) {
        delta = extra_delta;
        consumed += 2;
    }
    Some(LocalScaledIndexAddressMatch {
        base_local,
        index_local,
        scale_log2,
        delta,
        consumed,
    })
}

fn match_i32_const_add(block: &CanonBlock, cursor: usize) -> Option<i32> {
    let konst = block.insts.get(cursor)?;
    let add = block.insts.get(cursor + 1)?;
    if !konst.op_eq(vm::op_i32_const as Op) || !add.op_eq(vm::op_i32_add as Op) {
        return None;
    }
    raw_i32(konst.operands.first())
}

fn i32_local_base_load_family(op: Op) -> Option<Op> {
    if std::ptr::fn_addr_eq(op, vm::op_i32_load_local as Op) {
        Some(vm::op_i32_load_local_base as Op)
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_load_shared as Op) {
        Some(vm::op_i32_load_shared_local_base as Op)
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_load_indexed_local as Op) {
        Some(vm::op_i32_load_indexed_local_base as Op)
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_load_indexed_shared as Op) {
        Some(vm::op_i32_load_indexed_shared_local_base as Op)
    } else {
        None
    }
}

fn match_i32_value_expr(
    ctx: &SelectionContext<'_>,
    cursor: usize,
) -> Option<(Vec<KernelOp>, usize)> {
    let (mut lhs_ops, lhs_consumed) = match_i32_atomic_value_expr(ctx, cursor)?;
    let rhs_cursor = cursor + lhs_consumed;
    let Some((rhs_ops, rhs_consumed)) = match_i32_atomic_value_expr(ctx, rhs_cursor) else {
        return Some((lhs_ops, lhs_consumed));
    };
    let add = ctx.block.insts.get(rhs_cursor + rhs_consumed)?;
    if !add.op_eq(vm::op_i32_add as Op) {
        return Some((lhs_ops, lhs_consumed));
    }
    lhs_ops.extend(rhs_ops);
    lhs_ops.push(KernelOp {
        label: None,
        op: add.op,
        operands: add.operands.clone(),
        family: "generic",
    });
    Some((lhs_ops, lhs_consumed + rhs_consumed + 1))
}

fn match_i32_atomic_value_expr(
    ctx: &SelectionContext<'_>,
    cursor: usize,
) -> Option<(Vec<KernelOp>, usize)> {
    if let Some(result) = I32LoadLocalScaledIndexSpec.emit(ctx, cursor) {
        return Some((result.ops, result.consumed));
    }
    if let Some(result) = I32LoadLocalBaseSpec.emit(ctx, cursor) {
        return Some((result.ops, result.consumed));
    }
    if let Some(result) = I32LoadConstBaseSpec.emit(ctx, cursor) {
        return Some((result.ops, result.consumed));
    }
    let inst = ctx.block.insts.get(cursor)?;
    if inst.op_eq(vm::op_local_get4 as Op) || inst.op_eq(vm::op_i32_const as Op) {
        let result = GenericSpec.emit(ctx, cursor)?;
        return Some((result.ops, result.consumed));
    }
    None
}

fn match_local_numeric(
    block: &CanonBlock,
    cursor: usize,
    class: NumericClass,
    consumer: NumericConsumer,
) -> Option<(Vec<LoweredOperand>, usize)> {
    let (lhs, rhs, numeric, tail, consumed) = match consumer {
        NumericConsumer::Root => (
            block.insts.get(cursor)?,
            block.insts.get(cursor + 1)?,
            block.insts.get(cursor + 2)?,
            None,
            3,
        ),
        NumericConsumer::Set | NumericConsumer::Tee | NumericConsumer::BrIf => (
            block.insts.get(cursor)?,
            block.insts.get(cursor + 1)?,
            block.insts.get(cursor + 2)?,
            Some(block.insts.get(cursor + 3)?),
            4,
        ),
    };

    let kind = numeric_kind(numeric.op)?;
    if !kind.matches_class(class) {
        return None;
    }
    if matches!(consumer, NumericConsumer::BrIf) && kind.br_if_op().is_none() {
        return None;
    }

    let matched = match_numeric_inputs(lhs, rhs, kind)?;
    let mut operands = vec![
        raw_u32_operand(matched.kind.encode(matched.rhs_shape)),
        matched.lhs,
        matched.rhs,
    ];

    match consumer {
        NumericConsumer::Root => {}
        NumericConsumer::Set => operands.push(raw_local_set(tail?, matched.kind.result_width())?),
        NumericConsumer::Tee => operands.push(raw_local_tee(tail?, matched.kind.result_width())?),
        NumericConsumer::BrIf => {
            let branch = tail?;
            if !branch.op_eq(vm::op_br_if as Op) {
                return None;
            }
            operands.push(LoweredOperand::JumpTarget(branch_target(branch)?));
        }
    }

    Some((operands, consumed))
}

#[derive(Clone, Copy)]
enum UnaryKind {
    Bits32(LocalUnary32Op),
    Bits64(LocalUnary64Op),
}

impl UnaryKind {
    fn encode(self) -> u32 {
        match self {
            Self::Bits32(op) => encode_local_unary32_kind(op),
            Self::Bits64(op) => encode_local_unary64_kind(op),
        }
    }

    const fn width(self) -> u32 {
        match self {
            Self::Bits32(_) => 4,
            Self::Bits64(_) => 8,
        }
    }
}

fn unary_kind(op: Op) -> Option<UnaryKind> {
    Some(if std::ptr::fn_addr_eq(op, vm::op_i32_clz as Op) {
        UnaryKind::Bits32(LocalUnary32Op::I32Clz)
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_ctz as Op) {
        UnaryKind::Bits32(LocalUnary32Op::I32Ctz)
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_popcnt as Op) {
        UnaryKind::Bits32(LocalUnary32Op::I32Popcnt)
    } else if std::ptr::fn_addr_eq(op, vm::op_i64_clz as Op) {
        UnaryKind::Bits64(LocalUnary64Op::I64Clz)
    } else if std::ptr::fn_addr_eq(op, vm::op_i64_ctz as Op) {
        UnaryKind::Bits64(LocalUnary64Op::I64Ctz)
    } else if std::ptr::fn_addr_eq(op, vm::op_i64_popcnt as Op) {
        UnaryKind::Bits64(LocalUnary64Op::I64Popcnt)
    } else if std::ptr::fn_addr_eq(op, vm::op_f32_abs as Op) {
        UnaryKind::Bits32(LocalUnary32Op::F32Abs)
    } else if std::ptr::fn_addr_eq(op, vm::op_f32_neg as Op) {
        UnaryKind::Bits32(LocalUnary32Op::F32Neg)
    } else if std::ptr::fn_addr_eq(op, vm::op_f32_sqrt as Op) {
        UnaryKind::Bits32(LocalUnary32Op::F32Sqrt)
    } else if std::ptr::fn_addr_eq(op, vm::op_f32_ceil as Op) {
        UnaryKind::Bits32(LocalUnary32Op::F32Ceil)
    } else if std::ptr::fn_addr_eq(op, vm::op_f32_floor as Op) {
        UnaryKind::Bits32(LocalUnary32Op::F32Floor)
    } else if std::ptr::fn_addr_eq(op, vm::op_f32_trunc as Op) {
        UnaryKind::Bits32(LocalUnary32Op::F32Trunc)
    } else if std::ptr::fn_addr_eq(op, vm::op_f32_nearest as Op) {
        UnaryKind::Bits32(LocalUnary32Op::F32Nearest)
    } else if std::ptr::fn_addr_eq(op, vm::op_f64_abs as Op) {
        UnaryKind::Bits64(LocalUnary64Op::F64Abs)
    } else if std::ptr::fn_addr_eq(op, vm::op_f64_neg as Op) {
        UnaryKind::Bits64(LocalUnary64Op::F64Neg)
    } else if std::ptr::fn_addr_eq(op, vm::op_f64_sqrt as Op) {
        UnaryKind::Bits64(LocalUnary64Op::F64Sqrt)
    } else if std::ptr::fn_addr_eq(op, vm::op_f64_ceil as Op) {
        UnaryKind::Bits64(LocalUnary64Op::F64Ceil)
    } else if std::ptr::fn_addr_eq(op, vm::op_f64_floor as Op) {
        UnaryKind::Bits64(LocalUnary64Op::F64Floor)
    } else if std::ptr::fn_addr_eq(op, vm::op_f64_trunc as Op) {
        UnaryKind::Bits64(LocalUnary64Op::F64Trunc)
    } else if std::ptr::fn_addr_eq(op, vm::op_f64_nearest as Op) {
        UnaryKind::Bits64(LocalUnary64Op::F64Nearest)
    } else {
        return None;
    })
}

#[derive(Clone, Copy)]
enum NumericKind {
    Binop32(LocalBinop32Op),
    Binop64(LocalBinop64Op),
    Cmp32(LocalCmp32Op),
    Cmp64(LocalCmp64Op),
}

impl NumericKind {
    fn matches_class(self, class: NumericClass) -> bool {
        matches!(
            (self, class),
            (Self::Binop32(_), NumericClass::Binop32)
                | (Self::Binop64(_), NumericClass::Binop64)
                | (Self::Cmp32(_), NumericClass::Cmp32)
                | (Self::Cmp64(_), NumericClass::Cmp64)
        )
    }

    const fn input_width(self) -> u32 {
        match self {
            Self::Binop32(_) | Self::Cmp32(_) => 4,
            Self::Binop64(_) | Self::Cmp64(_) => 8,
        }
    }

    const fn result_width(self) -> u32 {
        match self {
            Self::Binop32(_) => 4,
            Self::Binop64(_) => 8,
            Self::Cmp32(_) | Self::Cmp64(_) => 4,
        }
    }

    const fn const_kind(self) -> LocalFastConstKind {
        match self {
            Self::Binop32(op) => op.const_kind(),
            Self::Binop64(op) => op.const_kind(),
            Self::Cmp32(op) => op.const_kind(),
            Self::Cmp64(op) => op.const_kind(),
        }
    }

    const fn encode(self, rhs_shape: LocalFastRhsShape) -> u32 {
        match self {
            Self::Binop32(op) => encode_local_binop32_kind(op, rhs_shape),
            Self::Binop64(op) => encode_local_binop64_kind(op, rhs_shape),
            Self::Cmp32(op) => encode_local_cmp32_kind(op, rhs_shape),
            Self::Cmp64(op) => encode_local_cmp64_kind(op, rhs_shape),
        }
    }

    const fn br_if_op(self) -> Option<Op> {
        match self {
            Self::Binop32(
                LocalBinop32Op::I32Add
                | LocalBinop32Op::I32Sub
                | LocalBinop32Op::I32Mul
                | LocalBinop32Op::I32And
                | LocalBinop32Op::I32Or
                | LocalBinop32Op::I32Xor
                | LocalBinop32Op::I32Shl
                | LocalBinop32Op::I32ShrS
                | LocalBinop32Op::I32ShrU
                | LocalBinop32Op::I32Rotl
                | LocalBinop32Op::I32Rotr,
            ) => Some(vm::op_local_binop32_br_if as Op),
            Self::Cmp32(_) => Some(vm::op_local_cmp32_br_if as Op),
            Self::Cmp64(_) => Some(vm::op_local_cmp64_br_if as Op),
            Self::Binop32(_) | Self::Binop64(_) => None,
        }
    }

    const fn is_commutative(self) -> bool {
        match self {
            Self::Binop32(op) => matches!(
                op,
                LocalBinop32Op::I32Add
                    | LocalBinop32Op::I32Mul
                    | LocalBinop32Op::I32And
                    | LocalBinop32Op::I32Or
                    | LocalBinop32Op::I32Xor
                    | LocalBinop32Op::F32Add
                    | LocalBinop32Op::F32Mul
            ),
            Self::Binop64(op) => matches!(
                op,
                LocalBinop64Op::I64Add
                    | LocalBinop64Op::I64Mul
                    | LocalBinop64Op::I64And
                    | LocalBinop64Op::I64Or
                    | LocalBinop64Op::I64Xor
                    | LocalBinop64Op::F64Add
                    | LocalBinop64Op::F64Mul
            ),
            Self::Cmp32(op) => matches!(
                op,
                LocalCmp32Op::I32Eq
                    | LocalCmp32Op::I32Ne
                    | LocalCmp32Op::F32Eq
                    | LocalCmp32Op::F32Ne
            ),
            Self::Cmp64(op) => matches!(
                op,
                LocalCmp64Op::I64Eq
                    | LocalCmp64Op::I64Ne
                    | LocalCmp64Op::F64Eq
                    | LocalCmp64Op::F64Ne
            ),
        }
    }

    fn flipped(self) -> Option<Self> {
        Some(match self {
            Self::Cmp32(LocalCmp32Op::I32Eq) => Self::Cmp32(LocalCmp32Op::I32Eq),
            Self::Cmp32(LocalCmp32Op::I32Ne) => Self::Cmp32(LocalCmp32Op::I32Ne),
            Self::Cmp32(LocalCmp32Op::I32LtS) => Self::Cmp32(LocalCmp32Op::I32GtS),
            Self::Cmp32(LocalCmp32Op::I32LtU) => Self::Cmp32(LocalCmp32Op::I32GtU),
            Self::Cmp32(LocalCmp32Op::I32GtS) => Self::Cmp32(LocalCmp32Op::I32LtS),
            Self::Cmp32(LocalCmp32Op::I32GtU) => Self::Cmp32(LocalCmp32Op::I32LtU),
            Self::Cmp32(LocalCmp32Op::I32LeS) => Self::Cmp32(LocalCmp32Op::I32GeS),
            Self::Cmp32(LocalCmp32Op::I32LeU) => Self::Cmp32(LocalCmp32Op::I32GeU),
            Self::Cmp32(LocalCmp32Op::I32GeS) => Self::Cmp32(LocalCmp32Op::I32LeS),
            Self::Cmp32(LocalCmp32Op::I32GeU) => Self::Cmp32(LocalCmp32Op::I32LeU),
            Self::Cmp32(LocalCmp32Op::F32Eq) => Self::Cmp32(LocalCmp32Op::F32Eq),
            Self::Cmp32(LocalCmp32Op::F32Ne) => Self::Cmp32(LocalCmp32Op::F32Ne),
            Self::Cmp32(LocalCmp32Op::F32Lt) => Self::Cmp32(LocalCmp32Op::F32Gt),
            Self::Cmp32(LocalCmp32Op::F32Gt) => Self::Cmp32(LocalCmp32Op::F32Lt),
            Self::Cmp32(LocalCmp32Op::F32Le) => Self::Cmp32(LocalCmp32Op::F32Ge),
            Self::Cmp32(LocalCmp32Op::F32Ge) => Self::Cmp32(LocalCmp32Op::F32Le),
            Self::Cmp64(LocalCmp64Op::I64Eq) => Self::Cmp64(LocalCmp64Op::I64Eq),
            Self::Cmp64(LocalCmp64Op::I64Ne) => Self::Cmp64(LocalCmp64Op::I64Ne),
            Self::Cmp64(LocalCmp64Op::I64LtS) => Self::Cmp64(LocalCmp64Op::I64GtS),
            Self::Cmp64(LocalCmp64Op::I64LtU) => Self::Cmp64(LocalCmp64Op::I64GtU),
            Self::Cmp64(LocalCmp64Op::I64GtS) => Self::Cmp64(LocalCmp64Op::I64LtS),
            Self::Cmp64(LocalCmp64Op::I64GtU) => Self::Cmp64(LocalCmp64Op::I64LtU),
            Self::Cmp64(LocalCmp64Op::I64LeS) => Self::Cmp64(LocalCmp64Op::I64GeS),
            Self::Cmp64(LocalCmp64Op::I64LeU) => Self::Cmp64(LocalCmp64Op::I64GeU),
            Self::Cmp64(LocalCmp64Op::I64GeS) => Self::Cmp64(LocalCmp64Op::I64LeS),
            Self::Cmp64(LocalCmp64Op::I64GeU) => Self::Cmp64(LocalCmp64Op::I64LeU),
            Self::Cmp64(LocalCmp64Op::F64Eq) => Self::Cmp64(LocalCmp64Op::F64Eq),
            Self::Cmp64(LocalCmp64Op::F64Ne) => Self::Cmp64(LocalCmp64Op::F64Ne),
            Self::Cmp64(LocalCmp64Op::F64Lt) => Self::Cmp64(LocalCmp64Op::F64Gt),
            Self::Cmp64(LocalCmp64Op::F64Gt) => Self::Cmp64(LocalCmp64Op::F64Lt),
            Self::Cmp64(LocalCmp64Op::F64Le) => Self::Cmp64(LocalCmp64Op::F64Ge),
            Self::Cmp64(LocalCmp64Op::F64Ge) => Self::Cmp64(LocalCmp64Op::F64Le),
            Self::Binop32(_) | Self::Binop64(_) => return None,
        })
    }
}

fn numeric_kind(op: Op) -> Option<NumericKind> {
    Some(if std::ptr::fn_addr_eq(op, vm::op_i32_add as Op) {
        NumericKind::Binop32(LocalBinop32Op::I32Add)
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_sub as Op) {
        NumericKind::Binop32(LocalBinop32Op::I32Sub)
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_mul as Op) {
        NumericKind::Binop32(LocalBinop32Op::I32Mul)
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_and as Op) {
        NumericKind::Binop32(LocalBinop32Op::I32And)
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_or as Op) {
        NumericKind::Binop32(LocalBinop32Op::I32Or)
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_xor as Op) {
        NumericKind::Binop32(LocalBinop32Op::I32Xor)
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_shl as Op) {
        NumericKind::Binop32(LocalBinop32Op::I32Shl)
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_shr_s as Op) {
        NumericKind::Binop32(LocalBinop32Op::I32ShrS)
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_shr_u as Op) {
        NumericKind::Binop32(LocalBinop32Op::I32ShrU)
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_rotl as Op) {
        NumericKind::Binop32(LocalBinop32Op::I32Rotl)
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_rotr as Op) {
        NumericKind::Binop32(LocalBinop32Op::I32Rotr)
    } else if std::ptr::fn_addr_eq(op, vm::op_i64_add as Op) {
        NumericKind::Binop64(LocalBinop64Op::I64Add)
    } else if std::ptr::fn_addr_eq(op, vm::op_i64_sub as Op) {
        NumericKind::Binop64(LocalBinop64Op::I64Sub)
    } else if std::ptr::fn_addr_eq(op, vm::op_i64_mul as Op) {
        NumericKind::Binop64(LocalBinop64Op::I64Mul)
    } else if std::ptr::fn_addr_eq(op, vm::op_i64_and as Op) {
        NumericKind::Binop64(LocalBinop64Op::I64And)
    } else if std::ptr::fn_addr_eq(op, vm::op_i64_or as Op) {
        NumericKind::Binop64(LocalBinop64Op::I64Or)
    } else if std::ptr::fn_addr_eq(op, vm::op_i64_xor as Op) {
        NumericKind::Binop64(LocalBinop64Op::I64Xor)
    } else if std::ptr::fn_addr_eq(op, vm::op_i64_shl as Op) {
        NumericKind::Binop64(LocalBinop64Op::I64Shl)
    } else if std::ptr::fn_addr_eq(op, vm::op_i64_shr_s as Op) {
        NumericKind::Binop64(LocalBinop64Op::I64ShrS)
    } else if std::ptr::fn_addr_eq(op, vm::op_i64_shr_u as Op) {
        NumericKind::Binop64(LocalBinop64Op::I64ShrU)
    } else if std::ptr::fn_addr_eq(op, vm::op_i64_rotl as Op) {
        NumericKind::Binop64(LocalBinop64Op::I64Rotl)
    } else if std::ptr::fn_addr_eq(op, vm::op_i64_rotr as Op) {
        NumericKind::Binop64(LocalBinop64Op::I64Rotr)
    } else if std::ptr::fn_addr_eq(op, vm::op_f32_add as Op) {
        NumericKind::Binop32(LocalBinop32Op::F32Add)
    } else if std::ptr::fn_addr_eq(op, vm::op_f32_sub as Op) {
        NumericKind::Binop32(LocalBinop32Op::F32Sub)
    } else if std::ptr::fn_addr_eq(op, vm::op_f32_mul as Op) {
        NumericKind::Binop32(LocalBinop32Op::F32Mul)
    } else if std::ptr::fn_addr_eq(op, vm::op_f32_div as Op) {
        NumericKind::Binop32(LocalBinop32Op::F32Div)
    } else if std::ptr::fn_addr_eq(op, vm::op_f64_add as Op) {
        NumericKind::Binop64(LocalBinop64Op::F64Add)
    } else if std::ptr::fn_addr_eq(op, vm::op_f64_sub as Op) {
        NumericKind::Binop64(LocalBinop64Op::F64Sub)
    } else if std::ptr::fn_addr_eq(op, vm::op_f64_mul as Op) {
        NumericKind::Binop64(LocalBinop64Op::F64Mul)
    } else if std::ptr::fn_addr_eq(op, vm::op_f64_div as Op) {
        NumericKind::Binop64(LocalBinop64Op::F64Div)
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_eq as Op) {
        NumericKind::Cmp32(LocalCmp32Op::I32Eq)
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_ne as Op) {
        NumericKind::Cmp32(LocalCmp32Op::I32Ne)
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_lt_s as Op) {
        NumericKind::Cmp32(LocalCmp32Op::I32LtS)
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_lt_u as Op) {
        NumericKind::Cmp32(LocalCmp32Op::I32LtU)
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_gt_s as Op) {
        NumericKind::Cmp32(LocalCmp32Op::I32GtS)
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_gt_u as Op) {
        NumericKind::Cmp32(LocalCmp32Op::I32GtU)
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_le_s as Op) {
        NumericKind::Cmp32(LocalCmp32Op::I32LeS)
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_le_u as Op) {
        NumericKind::Cmp32(LocalCmp32Op::I32LeU)
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_ge_s as Op) {
        NumericKind::Cmp32(LocalCmp32Op::I32GeS)
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_ge_u as Op) {
        NumericKind::Cmp32(LocalCmp32Op::I32GeU)
    } else if std::ptr::fn_addr_eq(op, vm::op_i64_eq as Op) {
        NumericKind::Cmp64(LocalCmp64Op::I64Eq)
    } else if std::ptr::fn_addr_eq(op, vm::op_i64_ne as Op) {
        NumericKind::Cmp64(LocalCmp64Op::I64Ne)
    } else if std::ptr::fn_addr_eq(op, vm::op_i64_lt_s as Op) {
        NumericKind::Cmp64(LocalCmp64Op::I64LtS)
    } else if std::ptr::fn_addr_eq(op, vm::op_i64_lt_u as Op) {
        NumericKind::Cmp64(LocalCmp64Op::I64LtU)
    } else if std::ptr::fn_addr_eq(op, vm::op_i64_gt_s as Op) {
        NumericKind::Cmp64(LocalCmp64Op::I64GtS)
    } else if std::ptr::fn_addr_eq(op, vm::op_i64_gt_u as Op) {
        NumericKind::Cmp64(LocalCmp64Op::I64GtU)
    } else if std::ptr::fn_addr_eq(op, vm::op_i64_le_s as Op) {
        NumericKind::Cmp64(LocalCmp64Op::I64LeS)
    } else if std::ptr::fn_addr_eq(op, vm::op_i64_le_u as Op) {
        NumericKind::Cmp64(LocalCmp64Op::I64LeU)
    } else if std::ptr::fn_addr_eq(op, vm::op_i64_ge_s as Op) {
        NumericKind::Cmp64(LocalCmp64Op::I64GeS)
    } else if std::ptr::fn_addr_eq(op, vm::op_i64_ge_u as Op) {
        NumericKind::Cmp64(LocalCmp64Op::I64GeU)
    } else if std::ptr::fn_addr_eq(op, vm::op_f32_eq as Op) {
        NumericKind::Cmp32(LocalCmp32Op::F32Eq)
    } else if std::ptr::fn_addr_eq(op, vm::op_f32_ne as Op) {
        NumericKind::Cmp32(LocalCmp32Op::F32Ne)
    } else if std::ptr::fn_addr_eq(op, vm::op_f32_lt as Op) {
        NumericKind::Cmp32(LocalCmp32Op::F32Lt)
    } else if std::ptr::fn_addr_eq(op, vm::op_f32_gt as Op) {
        NumericKind::Cmp32(LocalCmp32Op::F32Gt)
    } else if std::ptr::fn_addr_eq(op, vm::op_f32_le as Op) {
        NumericKind::Cmp32(LocalCmp32Op::F32Le)
    } else if std::ptr::fn_addr_eq(op, vm::op_f32_ge as Op) {
        NumericKind::Cmp32(LocalCmp32Op::F32Ge)
    } else if std::ptr::fn_addr_eq(op, vm::op_f64_eq as Op) {
        NumericKind::Cmp64(LocalCmp64Op::F64Eq)
    } else if std::ptr::fn_addr_eq(op, vm::op_f64_ne as Op) {
        NumericKind::Cmp64(LocalCmp64Op::F64Ne)
    } else if std::ptr::fn_addr_eq(op, vm::op_f64_lt as Op) {
        NumericKind::Cmp64(LocalCmp64Op::F64Lt)
    } else if std::ptr::fn_addr_eq(op, vm::op_f64_gt as Op) {
        NumericKind::Cmp64(LocalCmp64Op::F64Gt)
    } else if std::ptr::fn_addr_eq(op, vm::op_f64_le as Op) {
        NumericKind::Cmp64(LocalCmp64Op::F64Le)
    } else if std::ptr::fn_addr_eq(op, vm::op_f64_ge as Op) {
        NumericKind::Cmp64(LocalCmp64Op::F64Ge)
    } else {
        return None;
    })
}

struct NumericMatch {
    kind: NumericKind,
    lhs: LoweredOperand,
    rhs: LoweredOperand,
    rhs_shape: LocalFastRhsShape,
}

fn match_numeric_inputs(
    lhs: &CanonInst,
    rhs: &CanonInst,
    kind: NumericKind,
) -> Option<NumericMatch> {
    let lhs_local = raw_local_get(lhs, kind.input_width());
    let rhs_local = raw_local_get(rhs, kind.input_width());
    let lhs_const = const_operand_for_kind(lhs, kind.const_kind());
    let rhs_const = const_operand_for_kind(rhs, kind.const_kind());

    if let (Some(lhs), Some(rhs)) = (lhs_local.clone(), rhs_local.clone()) {
        return Some(NumericMatch {
            kind,
            lhs,
            rhs,
            rhs_shape: LocalFastRhsShape::Local,
        });
    }
    if let (Some(lhs), Some(rhs)) = (lhs_local.clone(), rhs_const.clone()) {
        return Some(NumericMatch {
            kind,
            lhs,
            rhs,
            rhs_shape: LocalFastRhsShape::Const,
        });
    }
    if let (Some(lhs), Some(rhs)) = (lhs_const, rhs_local) {
        if kind.is_commutative() {
            return Some(NumericMatch {
                kind,
                lhs: rhs,
                rhs: lhs,
                rhs_shape: LocalFastRhsShape::Const,
            });
        }
        if let Some(flipped) = kind.flipped() {
            return Some(NumericMatch {
                kind: flipped,
                lhs: rhs,
                rhs: lhs,
                rhs_shape: LocalFastRhsShape::Const,
            });
        }
    }
    None
}

fn raw_local_get(inst: &CanonInst, width: u32) -> Option<LoweredOperand> {
    if width == 4 && inst.op_eq(vm::op_local_get4 as Op) {
        return Some(inst.operands.first()?.clone());
    }
    if width == 8 && inst.op_eq(vm::op_local_get8 as Op) {
        return Some(inst.operands.first()?.clone());
    }
    None
}

fn raw_local_set(inst: &CanonInst, width: u32) -> Option<LoweredOperand> {
    if width == 4 && inst.op_eq(vm::op_local_set4 as Op) {
        return Some(inst.operands.first()?.clone());
    }
    if width == 8 && inst.op_eq(vm::op_local_set8 as Op) {
        return Some(inst.operands.first()?.clone());
    }
    None
}

fn raw_local_tee(inst: &CanonInst, width: u32) -> Option<LoweredOperand> {
    if width == 4 && inst.op_eq(vm::op_local_tee4 as Op) {
        return Some(inst.operands.first()?.clone());
    }
    if width == 8 && inst.op_eq(vm::op_local_tee8 as Op) {
        return Some(inst.operands.first()?.clone());
    }
    None
}

fn const_operand_for_kind(
    inst: &CanonInst,
    const_kind: LocalFastConstKind,
) -> Option<LoweredOperand> {
    match const_kind {
        LocalFastConstKind::I32 if inst.op_eq(vm::op_i32_const as Op) => {
            Some(inst.operands.first()?.clone())
        }
        LocalFastConstKind::I64 if inst.op_eq(vm::op_i64_const as Op) => {
            Some(inst.operands.first()?.clone())
        }
        LocalFastConstKind::F32 if inst.op_eq(vm::op_f32_const as Op) => {
            Some(inst.operands.first()?.clone())
        }
        LocalFastConstKind::F64 if inst.op_eq(vm::op_f64_const as Op) => {
            Some(inst.operands.first()?.clone())
        }
        _ => None,
    }
}

fn fold_const_base_memarg(
    base: Option<&LoweredOperand>,
    memarg: Option<&LoweredOperand>,
) -> Option<Operand> {
    let base = raw_i32(base)? as u32;
    let mut memarg = raw_memarg(memarg)?;
    memarg.offset = memarg.offset.wrapping_add(base);
    Some(Operand { memarg })
}

fn branch_target(branch: &CanonInst) -> Option<usize> {
    match branch.operands.first()? {
        LoweredOperand::JumpTarget(target) => Some(*target),
        LoweredOperand::Raw(_)
        | LoweredOperand::ConstPoolRef(_)
        | LoweredOperand::CallRecipeRef(_) => None,
    }
}

fn raw_i32(operand: Option<&LoweredOperand>) -> Option<i32> {
    let LoweredOperand::Raw(encoded) = operand? else {
        return None;
    };
    Some(unsafe { Operand { encoded: *encoded }.i32 })
}

fn raw_select(operand: Option<&LoweredOperand>) -> Option<u32> {
    let LoweredOperand::Raw(encoded) = operand? else {
        return None;
    };
    Some(unsafe { Operand { encoded: *encoded }.select })
}

fn raw_memarg(operand: Option<&LoweredOperand>) -> Option<MemArg> {
    let LoweredOperand::Raw(encoded) = operand? else {
        return None;
    };
    Some(unsafe { Operand { encoded: *encoded }.memarg })
}

fn raw_u32_operand(value: u32) -> LoweredOperand {
    LoweredOperand::Raw(unsafe { Operand { u32: value }.encoded })
}

fn raw_i32_operand(value: i32) -> LoweredOperand {
    LoweredOperand::Raw(unsafe { Operand { i32: value }.encoded })
}

fn i32_compare_kind(op: Op) -> Option<u32> {
    Some(if std::ptr::fn_addr_eq(op, vm::op_i32_eq as Op) {
        0
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_ne as Op) {
        1
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_lt_s as Op) {
        2
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_lt_u as Op) {
        3
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_gt_s as Op) {
        4
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_gt_u as Op) {
        5
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_le_s as Op) {
        6
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_le_u as Op) {
        7
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_ge_s as Op) {
        8
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_ge_u as Op) {
        9
    } else {
        return None;
    })
}

fn flip_i32_compare_kind(kind: u32) -> Option<u32> {
    Some(match kind {
        0 => 0,
        1 => 1,
        2 => 4,
        3 => 5,
        4 => 2,
        5 => 3,
        6 => 8,
        7 => 9,
        8 => 6,
        9 => 7,
        _ => return None,
    })
}

trait CanonInstExt {
    fn op_eq(&self, candidate: Op) -> bool;
}

impl CanonInstExt for CanonInst {
    fn op_eq(&self, candidate: Op) -> bool {
        std::ptr::fn_addr_eq(self.op, candidate)
    }
}
