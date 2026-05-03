use super::{analysis::AnalysisResults, ir::CanonBlock, ir::CanonFunc, ir::CanonInst};
use crate::{
    common::{
        encode_local_binop32_kind, encode_local_binop64_kind, encode_local_cmp32_kind,
        encode_local_cmp64_kind, encode_local_unary32_kind, encode_local_unary64_kind,
        LocalBinop32Op, LocalBinop64Op, LocalCmp32Op, LocalCmp64Op, LocalFastConstKind,
        LocalFastRhsShape, LocalUnary32Op, LocalUnary64Op, LoweredOperand, MemArg, Op, Operand,
        ValType,
    },
    runtime::vm,
};

const I32_SELECT_BIT_STEP_MASK_SHIFTED: u32 = 1 << 0;
const I32_SELECT_BIT_STEP_EQ_CONDITION: u32 = 1 << 1;
const I32_SELECT_BIT_STEP_TEE_DST: u32 = 1 << 2;

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
        &LocalGetConstAndBrIfSpec,
        &LocalGetConstAndTeeConstEqBrIfSpec,
        &LocalGetConstAndConstCompareBrIfSpec,
        &LocalGetConstAddConstAndConstCompareBrIfSpec,
        &LocalGetConstCompareBrIfSpec,
        &LocalGetLocalCompareBrIfSpec,
        &LocalGet4Set4LocalGetConstCompareBrIfSpec,
        &LocalGetConstAddTeeBrIfSpec,
        &LocalGet4I32IncLocalBaseI32Load8ULocalBaseSet4Spec,
        &LocalGet4I32IncLocalBaseSpec,
        &I32IncLocalBaseI32Load8UUpdateBrIfSpec,
        &I32Load8ULocalBaseSet4LocalGet4Set4LocalGet4BrIfSpec,
        &LocalGet4I32Load8ULocalBaseSet4Spec,
        &I32Load8ULocalBaseSet4LocalGet4Spec,
        &LocalGet4RunSpec::EIGHT,
        &LocalGet4RunSpec::SEVEN,
        &LocalGet4RunSpec::SIX,
        &LocalGet4RunSpec::FIVE,
        &LocalGet4RunSpec::FOUR,
        &LocalGet4RunSpec::THREE,
        &LocalGet4RunSpec::TWO,
        &I32LoadStoreLocalBaseReverseLoopSpec,
        &LocalGet4CopySpec,
        &I32ConstCopySpec,
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
        &I32SelectBitStep4RunSpec,
        &I32SelectBitStep4FromLocalSpec,
        &I32SelectBitStep4Spec,
        &StackI32ConstBinopSpec::ROOT,
        &StackI32ConstBinopSpec::SET,
        &StackI32ConstBinopSpec::TEE,
        &StackI32ConstBinopSpec::BR_IF,
        &StackI32ConstCmpSpec::ROOT,
        &StackI32ConstCmpSpec::SET,
        &StackI32ConstCmpSpec::TEE,
        &StackI32ConstCmpSpec::BR_IF,
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
        &Select4ConsumerSpec::SET,
        &Select4ConsumerSpec::TEE,
        &I32SumClipLocalBaseLoopSpec,
        &I32Load16UUpdateStore16LocalBaseLoopSpec,
        &I32Load16SDot4LocalBaseLoopSpec,
        &I32LoadStoreLocalBaseRelinkLoopSpec,
        &ScalarCopyLocalBaseRunSpec,
        &I32IncLocalBaseSpec,
        &ScalarStoreLocalScaledIndexSpec,
        &ScalarLoadLocalScaledIndexSpec,
        &I32LoadLocalBaseSet4I32Load16ULocalBaseLocalEqSearchLoopSpec,
        &I32LoadLocalBaseSet4I32Load8ULocalBaseLocalMaskedSearchLoopSpec,
        &LocalGet4LocalGet4XorTee4U8Shl1I32Load16USpec,
        &I32LoadLocalBaseTee4BrIfSpec,
        &I32LoadTee4BrIfSpec,
        &I32LoadLocalBaseLocalGet4ScalarLoadTee4CmpBrIfSpec,
        &I32LoadStoreLocalBaseLocalGet4Spec,
        &LocalGet4I32LoadLocalBaseAddSet4Spec,
        &I32Load16UBitmixAccLocalBaseDeltaLoopSpec,
        &I32Load16SMulAddLocalBaseDeltaLoopSpec,
        &I32Load16SMulAddLocalBaseLoopSpec,
        &LocalGet4ScalarLoadLocalBaseSpec,
        &I32LoadLocalBaseSet4I32Load8ULocalBaseLocalMaskedCompareBrIfSpec,
        &I32LoadLocalBaseSet4ScalarLoadLocalBaseLocalEqBrIfSpec,
        &I32LoadLocalBaseSet4ScalarLoadLocalBaseLocalGet4Spec,
        &I32LoadLocalBaseSet4ScalarLoadLocalBaseSpec,
        &ScalarLoadLocalBaseLocalGet4ScalarLoadSpec,
        &ScalarLoadLocalBaseLocalGet4Spec,
        &ScalarLoadLocalBaseSet4Spec,
        &ScalarStoreLocalBaseSpec,
        &ScalarLoadLocalBaseSpec,
        &ScalarLoadLocalGet4Spec,
        &I32LoadConstBaseLocalGet4AddSet4Spec,
        &ScalarStoreConstBaseSpec,
        &ScalarLoadConstBaseSpec,
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
struct LocalGetConstAndBrIfSpec;
struct LocalGetConstAndTeeConstEqBrIfSpec;
struct LocalGetConstAndConstCompareBrIfSpec;
struct LocalGetConstAddConstAndConstCompareBrIfSpec;
struct LocalGetConstCompareBrIfSpec;
struct LocalGetLocalCompareBrIfSpec;
struct LocalGet4Set4LocalGetConstCompareBrIfSpec;
struct LocalGetConstAddTeeBrIfSpec;
struct LocalGet4I32IncLocalBaseSpec;
struct LocalGet4I32IncLocalBaseI32Load8ULocalBaseSet4Spec;
struct I32IncLocalBaseI32Load8UUpdateBrIfSpec;
struct LocalGet4I32Load8ULocalBaseSet4Spec;
struct I32Load8ULocalBaseSet4LocalGet4Spec;
struct I32Load8ULocalBaseSet4LocalGet4Set4LocalGet4BrIfSpec;
struct LocalGet4RunSpec {
    width: usize,
    op: Op,
    label: &'static str,
}
struct LocalGet4CopySpec;
struct I32ConstCopySpec;

impl LocalGet4RunSpec {
    const TWO: Self = Self {
        width: 2,
        op: vm::op_local_get4_local_get4 as Op,
        label: "op_local_get4_local_get4",
    };
    const THREE: Self = Self {
        width: 3,
        op: vm::op_local_get4_local_get4_local_get4 as Op,
        label: "op_local_get4_local_get4_local_get4",
    };
    const FOUR: Self = Self {
        width: 4,
        op: vm::op_local_get4_run as Op,
        label: "op_local_get4_run",
    };
    const FIVE: Self = Self {
        width: 5,
        op: vm::op_local_get4_run as Op,
        label: "op_local_get4_run",
    };
    const SIX: Self = Self {
        width: 6,
        op: vm::op_local_get4_run as Op,
        label: "op_local_get4_run",
    };
    const SEVEN: Self = Self {
        width: 7,
        op: vm::op_local_get4_run as Op,
        label: "op_local_get4_run",
    };
    const EIGHT: Self = Self {
        width: 8,
        op: vm::op_local_get4_run as Op,
        label: "op_local_get4_run",
    };
}

impl FamilySpec for LocalGet4I32IncLocalBaseSpec {
    fn group(&self) -> FamilyGroup {
        FamilyGroup::Memory
    }

    fn name(&self) -> &'static str {
        "op_local_get4_i32_inc_local_base"
    }

    fn matches(&self, ctx: &SelectionContext<'_>, cursor: usize) -> bool {
        emit_local_get4_i32_inc_local_base(ctx, cursor).is_some()
    }

    fn cost(&self, ctx: &SelectionContext<'_>, _cursor: usize) -> i32 {
        112 + loop_bonus(ctx)
    }

    fn emit(&self, ctx: &SelectionContext<'_>, cursor: usize) -> Option<MatchResult> {
        emit_local_get4_i32_inc_local_base(ctx, cursor)
    }
}

impl FamilySpec for LocalGet4I32IncLocalBaseI32Load8ULocalBaseSet4Spec {
    fn group(&self) -> FamilyGroup {
        FamilyGroup::Memory
    }

    fn name(&self) -> &'static str {
        "op_local_get4_i32_inc_local_base_i32_load8_u_local_base_set4"
    }

    fn matches(&self, ctx: &SelectionContext<'_>, cursor: usize) -> bool {
        emit_local_get4_i32_inc_local_base_i32_load8_u_local_base_set4(ctx, cursor).is_some()
    }

    fn cost(&self, ctx: &SelectionContext<'_>, _cursor: usize) -> i32 {
        176 + loop_bonus(ctx)
    }

    fn emit(&self, ctx: &SelectionContext<'_>, cursor: usize) -> Option<MatchResult> {
        emit_local_get4_i32_inc_local_base_i32_load8_u_local_base_set4(ctx, cursor)
    }
}

impl FamilySpec for I32IncLocalBaseI32Load8UUpdateBrIfSpec {
    fn group(&self) -> FamilyGroup {
        FamilyGroup::Memory
    }

    fn name(&self) -> &'static str {
        "op_i32_inc_local_base_i32_load8_u_local_base_set4_local_get4_set4_local_get4_br_if"
    }

    fn matches(&self, ctx: &SelectionContext<'_>, cursor: usize) -> bool {
        emit_i32_inc_local_base_i32_load8_u_local_base_set4_local_get4_set4_local_get4_br_if(
            ctx, cursor,
        )
        .is_some()
    }

    fn cost(&self, ctx: &SelectionContext<'_>, _cursor: usize) -> i32 {
        264 + loop_bonus(ctx)
    }

    fn emit(&self, ctx: &SelectionContext<'_>, cursor: usize) -> Option<MatchResult> {
        emit_i32_inc_local_base_i32_load8_u_local_base_set4_local_get4_set4_local_get4_br_if(
            ctx, cursor,
        )
    }
}

impl FamilySpec for LocalGet4I32Load8ULocalBaseSet4Spec {
    fn group(&self) -> FamilyGroup {
        FamilyGroup::Memory
    }

    fn name(&self) -> &'static str {
        "op_local_get4_i32_load8_u_local_base_set4"
    }

    fn matches(&self, ctx: &SelectionContext<'_>, cursor: usize) -> bool {
        emit_local_get4_i32_load8_u_local_base_set4(ctx, cursor).is_some()
    }

    fn cost(&self, ctx: &SelectionContext<'_>, _cursor: usize) -> i32 {
        96 + loop_bonus(ctx)
    }

    fn emit(&self, ctx: &SelectionContext<'_>, cursor: usize) -> Option<MatchResult> {
        emit_local_get4_i32_load8_u_local_base_set4(ctx, cursor)
    }
}

impl FamilySpec for I32Load8ULocalBaseSet4LocalGet4Spec {
    fn group(&self) -> FamilyGroup {
        FamilyGroup::Memory
    }

    fn name(&self) -> &'static str {
        "op_i32_load8_u_local_base_set4_local_get4"
    }

    fn matches(&self, ctx: &SelectionContext<'_>, cursor: usize) -> bool {
        emit_i32_load8_u_local_base_set4_local_get4(ctx, cursor).is_some()
    }

    fn cost(&self, ctx: &SelectionContext<'_>, _cursor: usize) -> i32 {
        96 + loop_bonus(ctx)
    }

    fn emit(&self, ctx: &SelectionContext<'_>, cursor: usize) -> Option<MatchResult> {
        emit_i32_load8_u_local_base_set4_local_get4(ctx, cursor)
    }
}

impl FamilySpec for I32Load8ULocalBaseSet4LocalGet4Set4LocalGet4BrIfSpec {
    fn group(&self) -> FamilyGroup {
        FamilyGroup::Memory
    }

    fn name(&self) -> &'static str {
        "op_i32_load8_u_local_base_set4_local_get4_set4_local_get4_br_if"
    }

    fn matches(&self, ctx: &SelectionContext<'_>, cursor: usize) -> bool {
        emit_i32_load8_u_local_base_set4_local_get4_set4_local_get4_br_if(ctx, cursor).is_some()
    }

    fn cost(&self, ctx: &SelectionContext<'_>, _cursor: usize) -> i32 {
        184 + loop_bonus(ctx)
    }

    fn emit(&self, ctx: &SelectionContext<'_>, cursor: usize) -> Option<MatchResult> {
        emit_i32_load8_u_local_base_set4_local_get4_set4_local_get4_br_if(ctx, cursor)
    }
}

impl FamilySpec for LocalGet4RunSpec {
    fn group(&self) -> FamilyGroup {
        FamilyGroup::LocalControl
    }

    fn name(&self) -> &'static str {
        self.label
    }

    fn matches(&self, ctx: &SelectionContext<'_>, cursor: usize) -> bool {
        let Some(insts) = ctx.block.insts.get(cursor..cursor + self.width) else {
            return false;
        };
        insts.iter().all(|inst| inst.op_eq(vm::op_local_get4 as Op))
    }

    fn cost(&self, ctx: &SelectionContext<'_>, _cursor: usize) -> i32 {
        i32::try_from(self.width).expect("local.get run width exceeds i32::MAX") * 10
            + loop_bonus(ctx)
    }

    fn emit(&self, ctx: &SelectionContext<'_>, cursor: usize) -> Option<MatchResult> {
        let insts = ctx.block.insts.get(cursor..cursor + self.width)?;
        let mut operands = insts
            .iter()
            .map(|inst| inst.operands.first().cloned())
            .collect::<Option<Vec<_>>>()?;
        if std::ptr::fn_addr_eq(self.op, vm::op_local_get4_run as Op) {
            operands.insert(
                0,
                raw_u32_operand(
                    u32::try_from(self.width).expect("local.get run width exceeds u32::MAX"),
                ),
            );
        }
        Some(MatchResult {
            group: self.group(),
            cost: self.cost(ctx, cursor),
            consumed: self.width,
            ops: vec![KernelOp {
                label: None,
                op: self.op,
                operands,
                family: self.name(),
            }],
        })
    }
}

impl FamilySpec for LocalGet4CopySpec {
    fn group(&self) -> FamilyGroup {
        FamilyGroup::LocalControl
    }

    fn name(&self) -> &'static str {
        "op_local_get4_set4"
    }

    fn matches(&self, ctx: &SelectionContext<'_>, cursor: usize) -> bool {
        let Some([get, consumer]) = ctx.block.insts.get(cursor..cursor + 2) else {
            return false;
        };
        get.op_eq(vm::op_local_get4 as Op)
            && (consumer.op_eq(vm::op_local_set4 as Op) || consumer.op_eq(vm::op_local_tee4 as Op))
    }

    fn cost(&self, ctx: &SelectionContext<'_>, _cursor: usize) -> i32 {
        18 + loop_bonus(ctx)
    }

    fn emit(&self, ctx: &SelectionContext<'_>, cursor: usize) -> Option<MatchResult> {
        let [get, consumer] = ctx.block.insts.get(cursor..cursor + 2)? else {
            return None;
        };
        let (op, dst) = if let Some(dst) = raw_local_set(consumer, 4) {
            (vm::op_local_get4_set4 as Op, dst)
        } else {
            (vm::op_local_get4_tee4 as Op, raw_local_tee(consumer, 4)?)
        };
        Some(MatchResult {
            group: self.group(),
            cost: self.cost(ctx, cursor),
            consumed: 2,
            ops: vec![KernelOp {
                label: None,
                op,
                operands: vec![get.operands.first()?.clone(), dst],
                family: self.name(),
            }],
        })
    }
}

impl FamilySpec for I32ConstCopySpec {
    fn group(&self) -> FamilyGroup {
        FamilyGroup::LocalControl
    }

    fn name(&self) -> &'static str {
        "op_i32_const_set4"
    }

    fn matches(&self, ctx: &SelectionContext<'_>, cursor: usize) -> bool {
        let Some([konst, consumer]) = ctx.block.insts.get(cursor..cursor + 2) else {
            return false;
        };
        const_operand_for_kind(konst, LocalFastConstKind::I32).is_some()
            && (raw_local_set(consumer, 4).is_some() || raw_local_tee(consumer, 4).is_some())
    }

    fn cost(&self, ctx: &SelectionContext<'_>, _cursor: usize) -> i32 {
        18 + loop_bonus(ctx)
    }

    fn emit(&self, ctx: &SelectionContext<'_>, cursor: usize) -> Option<MatchResult> {
        let [konst, consumer] = ctx.block.insts.get(cursor..cursor + 2)? else {
            return None;
        };
        let value = const_operand_for_kind(konst, LocalFastConstKind::I32)?;
        let (op, dst) = if let Some(dst) = raw_local_set(consumer, 4) {
            (vm::op_i32_const_set4 as Op, dst)
        } else {
            (vm::op_i32_const_tee4 as Op, raw_local_tee(consumer, 4)?)
        };
        Some(MatchResult {
            group: self.group(),
            cost: self.cost(ctx, cursor),
            consumed: 2,
            ops: vec![KernelOp {
                label: None,
                op,
                operands: vec![value, dst],
                family: self.name(),
            }],
        })
    }
}

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

impl FamilySpec for LocalGetConstAndBrIfSpec {
    fn group(&self) -> FamilyGroup {
        FamilyGroup::LocalControl
    }

    fn name(&self) -> &'static str {
        "op_local_get4_i32_const_and_br_if"
    }

    fn matches(&self, ctx: &SelectionContext<'_>, cursor: usize) -> bool {
        self.emit(ctx, cursor).is_some()
    }

    fn cost(&self, ctx: &SelectionContext<'_>, _cursor: usize) -> i32 {
        48 + loop_bonus(ctx)
    }

    fn emit(&self, ctx: &SelectionContext<'_>, cursor: usize) -> Option<MatchResult> {
        let [first, second, and, maybe_eqz] = ctx.block.insts.get(cursor..cursor + 4)? else {
            return None;
        };
        if !and.op_eq(vm::op_i32_and as Op) {
            return None;
        }
        let (local, mask) = if let (Some(local), Some(mask)) = (
            raw_local_get(first, 4),
            const_operand_for_kind(second, LocalFastConstKind::I32),
        ) {
            (local, mask)
        } else if let (Some(mask), Some(local)) = (
            const_operand_for_kind(first, LocalFastConstKind::I32),
            raw_local_get(second, 4),
        ) {
            (local, mask)
        } else {
            return None;
        };

        let (op, branch, consumed) = if maybe_eqz.op_eq(vm::op_br_if as Op) {
            (vm::op_local_get4_i32_const_and_br_if as Op, maybe_eqz, 4)
        } else if maybe_eqz.op_eq(vm::op_i32_eqz as Op) {
            let branch = ctx.block.insts.get(cursor + 4)?;
            if !branch.op_eq(vm::op_br_if as Op) {
                return None;
            }
            (vm::op_local_get4_i32_const_and_eqz_br_if as Op, branch, 5)
        } else {
            return None;
        };
        Some(MatchResult {
            group: self.group(),
            cost: self.cost(ctx, cursor),
            consumed,
            ops: vec![KernelOp {
                label: None,
                op,
                operands: vec![
                    local,
                    mask,
                    LoweredOperand::JumpTarget(branch_target(branch)?),
                ],
                family: self.name(),
            }],
        })
    }
}

impl FamilySpec for LocalGetConstAndTeeConstEqBrIfSpec {
    fn group(&self) -> FamilyGroup {
        FamilyGroup::LocalControl
    }

    fn name(&self) -> &'static str {
        "op_local_get4_i32_const_and_tee4_i32_const_eq_br_if"
    }

    fn matches(&self, ctx: &SelectionContext<'_>, cursor: usize) -> bool {
        self.emit(ctx, cursor).is_some()
    }

    fn cost(&self, ctx: &SelectionContext<'_>, _cursor: usize) -> i32 {
        72 + loop_bonus(ctx)
    }

    fn emit(&self, ctx: &SelectionContext<'_>, cursor: usize) -> Option<MatchResult> {
        let [local, mask, and, tee, rhs, eq, branch] = ctx.block.insts.get(cursor..cursor + 7)?
        else {
            return None;
        };
        if !and.op_eq(vm::op_i32_and as Op)
            || !eq.op_eq(vm::op_i32_eq as Op)
            || !branch.op_eq(vm::op_br_if as Op)
        {
            return None;
        }
        Some(MatchResult {
            group: self.group(),
            cost: self.cost(ctx, cursor),
            consumed: 7,
            ops: vec![KernelOp {
                label: None,
                op: vm::op_local_get4_i32_const_and_tee4_i32_const_eq_br_if as Op,
                operands: vec![
                    raw_local_get(local, 4)?,
                    const_operand_for_kind(mask, LocalFastConstKind::I32)?,
                    raw_local_tee(tee, 4)?,
                    const_operand_for_kind(rhs, LocalFastConstKind::I32)?,
                    LoweredOperand::JumpTarget(branch_target(branch)?),
                ],
                family: self.name(),
            }],
        })
    }
}

impl FamilySpec for LocalGetConstAndConstCompareBrIfSpec {
    fn group(&self) -> FamilyGroup {
        FamilyGroup::LocalControl
    }

    fn name(&self) -> &'static str {
        "op_local_get4_i32_const_and_i32_const_compare_br_if"
    }

    fn matches(&self, ctx: &SelectionContext<'_>, cursor: usize) -> bool {
        self.emit(ctx, cursor).is_some()
    }

    fn cost(&self, ctx: &SelectionContext<'_>, _cursor: usize) -> i32 {
        80 + loop_bonus(ctx)
    }

    fn emit(&self, ctx: &SelectionContext<'_>, cursor: usize) -> Option<MatchResult> {
        let [local, mask, and, rhs, compare, branch] = ctx.block.insts.get(cursor..cursor + 6)?
        else {
            return None;
        };
        if !and.op_eq(vm::op_i32_and as Op) || !branch.op_eq(vm::op_br_if as Op) {
            return None;
        }
        Some(MatchResult {
            group: self.group(),
            cost: self.cost(ctx, cursor),
            consumed: 6,
            ops: vec![KernelOp {
                label: None,
                op: vm::op_local_get4_i32_const_and_i32_const_compare_br_if as Op,
                operands: vec![
                    raw_local_get(local, 4)?,
                    const_operand_for_kind(mask, LocalFastConstKind::I32)?,
                    raw_u32_operand(i32_compare_kind(compare.op)?),
                    const_operand_for_kind(rhs, LocalFastConstKind::I32)?,
                    LoweredOperand::JumpTarget(branch_target(branch)?),
                ],
                family: self.name(),
            }],
        })
    }
}

impl FamilySpec for LocalGetConstAddConstAndConstCompareBrIfSpec {
    fn group(&self) -> FamilyGroup {
        FamilyGroup::LocalControl
    }

    fn name(&self) -> &'static str {
        "op_local_get4_i32_const_add_i32_const_and_i32_const_compare_br_if"
    }

    fn matches(&self, ctx: &SelectionContext<'_>, cursor: usize) -> bool {
        self.emit(ctx, cursor).is_some()
    }

    fn cost(&self, ctx: &SelectionContext<'_>, _cursor: usize) -> i32 {
        106 + loop_bonus(ctx)
    }

    fn emit(&self, ctx: &SelectionContext<'_>, cursor: usize) -> Option<MatchResult> {
        let [local, imm, add, mask, and, rhs, compare, branch] =
            ctx.block.insts.get(cursor..cursor + 8)?
        else {
            return None;
        };
        if !add.op_eq(vm::op_i32_add as Op)
            || !and.op_eq(vm::op_i32_and as Op)
            || !branch.op_eq(vm::op_br_if as Op)
        {
            return None;
        }
        Some(MatchResult {
            group: self.group(),
            cost: self.cost(ctx, cursor),
            consumed: 8,
            ops: vec![KernelOp {
                label: None,
                op: vm::op_local_get4_i32_const_add_i32_const_and_i32_const_compare_br_if as Op,
                operands: vec![
                    raw_local_get(local, 4)?,
                    const_operand_for_kind(imm, LocalFastConstKind::I32)?,
                    const_operand_for_kind(mask, LocalFastConstKind::I32)?,
                    raw_u32_operand(i32_compare_kind(compare.op)?),
                    const_operand_for_kind(rhs, LocalFastConstKind::I32)?,
                    LoweredOperand::JumpTarget(branch_target(branch)?),
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

impl FamilySpec for LocalGet4Set4LocalGetConstCompareBrIfSpec {
    fn group(&self) -> FamilyGroup {
        FamilyGroup::LocalControl
    }

    fn name(&self) -> &'static str {
        "op_local_get4_set4_local_get4_i32_const_compare_br_if"
    }

    fn matches(&self, ctx: &SelectionContext<'_>, cursor: usize) -> bool {
        self.emit(ctx, cursor).is_some()
    }

    fn cost(&self, ctx: &SelectionContext<'_>, _cursor: usize) -> i32 {
        74 + loop_bonus(ctx)
    }

    fn emit(&self, ctx: &SelectionContext<'_>, cursor: usize) -> Option<MatchResult> {
        let [copy_src, copy_dst, lhs, rhs, compare, branch] =
            ctx.block.insts.get(cursor..cursor + 6)?
        else {
            return None;
        };
        if !branch.op_eq(vm::op_br_if as Op) {
            return None;
        }
        Some(MatchResult {
            group: self.group(),
            cost: self.cost(ctx, cursor),
            consumed: 6,
            ops: vec![KernelOp {
                label: None,
                op: vm::op_local_get4_set4_local_get4_i32_const_compare_br_if as Op,
                operands: vec![
                    raw_local_get(copy_src, 4)?,
                    raw_local_set(copy_dst, 4)?,
                    raw_local_get(lhs, 4)?,
                    raw_u32_operand(i32_compare_kind(compare.op)?),
                    const_operand_for_kind(rhs, LocalFastConstKind::I32)?,
                    LoweredOperand::JumpTarget(branch_target(branch)?),
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

struct I32SelectBitStep4Spec;

struct I32SelectBitStep4RunSpec;

struct I32SelectBitStep4FromLocalSpec;

impl FamilySpec for I32SelectBitStep4RunSpec {
    fn group(&self) -> FamilyGroup {
        FamilyGroup::LocalControl
    }

    fn name(&self) -> &'static str {
        "op_i32_select_bit_step4_run"
    }

    fn matches(&self, ctx: &SelectionContext<'_>, cursor: usize) -> bool {
        self.emit(ctx, cursor).is_some()
    }

    fn cost(&self, ctx: &SelectionContext<'_>, _cursor: usize) -> i32 {
        128 + loop_bonus(ctx)
    }

    fn emit(&self, ctx: &SelectionContext<'_>, cursor: usize) -> Option<MatchResult> {
        let (operands, consumed) = match_i32_select_bit_step4_run(ctx.block, cursor)?;
        Some(MatchResult {
            group: self.group(),
            cost: self.cost(ctx, cursor),
            consumed,
            ops: vec![KernelOp {
                label: None,
                op: vm::op_i32_select_bit_step4_run as Op,
                operands,
                family: self.name(),
            }],
        })
    }
}

impl FamilySpec for I32SelectBitStep4FromLocalSpec {
    fn group(&self) -> FamilyGroup {
        FamilyGroup::LocalControl
    }

    fn name(&self) -> &'static str {
        "op_local_get4_i32_select_bit_step4"
    }

    fn matches(&self, ctx: &SelectionContext<'_>, cursor: usize) -> bool {
        self.emit(ctx, cursor).is_some()
    }

    fn cost(&self, ctx: &SelectionContext<'_>, _cursor: usize) -> i32 {
        112 + loop_bonus(ctx)
    }

    fn emit(&self, ctx: &SelectionContext<'_>, cursor: usize) -> Option<MatchResult> {
        let local = ctx.block.insts.get(cursor)?;
        let local = raw_local_get(local, 4)?;
        let (operands, consumed) = match_i32_select_bit_step4(ctx.block, cursor + 1)?;
        Some(MatchResult {
            group: self.group(),
            cost: self.cost(ctx, cursor),
            consumed: consumed + 1,
            ops: vec![
                KernelOp {
                    label: None,
                    op: vm::op_local_get4 as Op,
                    operands: vec![local],
                    family: "op_local_get4",
                },
                KernelOp {
                    label: None,
                    op: vm::op_i32_select_bit_step4 as Op,
                    operands,
                    family: "op_i32_select_bit_step4",
                },
            ],
        })
    }
}

impl FamilySpec for I32SelectBitStep4Spec {
    fn group(&self) -> FamilyGroup {
        FamilyGroup::LocalControl
    }

    fn name(&self) -> &'static str {
        "op_i32_select_bit_step4"
    }

    fn matches(&self, ctx: &SelectionContext<'_>, cursor: usize) -> bool {
        self.emit(ctx, cursor).is_some()
    }

    fn cost(&self, ctx: &SelectionContext<'_>, _cursor: usize) -> i32 {
        96 + loop_bonus(ctx)
    }

    fn emit(&self, ctx: &SelectionContext<'_>, cursor: usize) -> Option<MatchResult> {
        let (operands, consumed) = match_i32_select_bit_step4(ctx.block, cursor)?;
        Some(MatchResult {
            group: self.group(),
            cost: self.cost(ctx, cursor),
            consumed,
            ops: vec![KernelOp {
                label: None,
                op: vm::op_i32_select_bit_step4 as Op,
                operands,
                family: self.name(),
            }],
        })
    }
}

#[derive(Clone, Copy)]
enum StackI32ConstBinopConsumer {
    Root,
    Set,
    Tee,
    BrIf,
}

#[derive(Clone, Copy)]
struct StackI32ConstBinopSpec {
    consumer: StackI32ConstBinopConsumer,
    op: Op,
    label: &'static str,
}

impl StackI32ConstBinopSpec {
    const ROOT: Self = Self {
        consumer: StackI32ConstBinopConsumer::Root,
        op: vm::op_i32_const_binop as Op,
        label: "op_i32_const_binop",
    };
    const SET: Self = Self {
        consumer: StackI32ConstBinopConsumer::Set,
        op: vm::op_i32_const_binop_set4 as Op,
        label: "op_i32_const_binop_set4",
    };
    const TEE: Self = Self {
        consumer: StackI32ConstBinopConsumer::Tee,
        op: vm::op_i32_const_binop_tee4 as Op,
        label: "op_i32_const_binop_tee4",
    };
    const BR_IF: Self = Self {
        consumer: StackI32ConstBinopConsumer::BrIf,
        op: vm::op_i32_const_binop_br_if as Op,
        label: "op_i32_const_binop_br_if",
    };
}

impl FamilySpec for StackI32ConstBinopSpec {
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
            StackI32ConstBinopConsumer::Root => 17,
            StackI32ConstBinopConsumer::Set | StackI32ConstBinopConsumer::Tee => 24,
            StackI32ConstBinopConsumer::BrIf => 30,
        };
        base + loop_bonus(ctx)
    }

    fn emit(&self, ctx: &SelectionContext<'_>, cursor: usize) -> Option<MatchResult> {
        let (operands, consumed) = match_stack_i32_const_binop(ctx.block, cursor, self.consumer)?;
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

#[derive(Clone, Copy)]
enum StackI32ConstCmpConsumer {
    Root,
    Set,
    Tee,
    BrIf,
}

#[derive(Clone, Copy)]
struct StackI32ConstCmpSpec {
    consumer: StackI32ConstCmpConsumer,
    op: Op,
    label: &'static str,
}

impl StackI32ConstCmpSpec {
    const ROOT: Self = Self {
        consumer: StackI32ConstCmpConsumer::Root,
        op: vm::op_i32_const_cmp as Op,
        label: "op_i32_const_cmp",
    };
    const SET: Self = Self {
        consumer: StackI32ConstCmpConsumer::Set,
        op: vm::op_i32_const_cmp_set4 as Op,
        label: "op_i32_const_cmp_set4",
    };
    const TEE: Self = Self {
        consumer: StackI32ConstCmpConsumer::Tee,
        op: vm::op_i32_const_cmp_tee4 as Op,
        label: "op_i32_const_cmp_tee4",
    };
    const BR_IF: Self = Self {
        consumer: StackI32ConstCmpConsumer::BrIf,
        op: vm::op_i32_const_cmp_br_if as Op,
        label: "op_i32_const_cmp_br_if",
    };
}

impl FamilySpec for StackI32ConstCmpSpec {
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
            StackI32ConstCmpConsumer::Root => 17,
            StackI32ConstCmpConsumer::Set | StackI32ConstCmpConsumer::Tee => 24,
            StackI32ConstCmpConsumer::BrIf => 30,
        };
        base + loop_bonus(ctx)
    }

    fn emit(&self, ctx: &SelectionContext<'_>, cursor: usize) -> Option<MatchResult> {
        let (operands, consumed) = match_stack_i32_const_cmp(ctx.block, cursor, self.consumer)?;
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
enum Select4Consumer {
    Set,
    Tee,
}

#[derive(Clone, Copy)]
struct Select4ConsumerSpec {
    consumer: Select4Consumer,
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

impl Select4ConsumerSpec {
    const SET: Self = Self {
        consumer: Select4Consumer::Set,
        op: vm::op_select4_set4 as Op,
        label: "op_select4_set4",
    };
    const TEE: Self = Self {
        consumer: Select4Consumer::Tee,
        op: vm::op_select4_tee4 as Op,
        label: "op_select4_tee4",
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

impl FamilySpec for Select4ConsumerSpec {
    fn group(&self) -> FamilyGroup {
        FamilyGroup::CallSelect
    }

    fn name(&self) -> &'static str {
        self.label
    }

    fn matches(&self, ctx: &SelectionContext<'_>, cursor: usize) -> bool {
        self.emit(ctx, cursor).is_some()
    }

    fn cost(&self, ctx: &SelectionContext<'_>, _cursor: usize) -> i32 {
        24 + loop_bonus(ctx)
    }

    fn emit(&self, ctx: &SelectionContext<'_>, cursor: usize) -> Option<MatchResult> {
        let select = ctx.block.insts.get(cursor)?;
        if !select.op_eq(vm::op_select as Op) || raw_select(select.operands.first()) != Some(4) {
            return None;
        }
        let consumer = ctx.block.insts.get(cursor + 1)?;
        let local = match self.consumer {
            Select4Consumer::Set => raw_local_set(consumer, 4)?,
            Select4Consumer::Tee => raw_local_tee(consumer, 4)?,
        };
        Some(MatchResult {
            group: self.group(),
            cost: self.cost(ctx, cursor),
            consumed: 2,
            ops: vec![KernelOp {
                label: None,
                op: self.op,
                operands: vec![local],
                family: self.name(),
            }],
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ScalarType {
    I32,
    I64,
    F32,
    F64,
}

impl ScalarType {
    pub(crate) fn width(self) -> u32 {
        match self {
            Self::I32 | Self::F32 => 4,
            Self::I64 | Self::F64 => 8,
        }
    }

    fn const_kind(self) -> LocalFastConstKind {
        match self {
            Self::I32 => LocalFastConstKind::I32,
            Self::I64 => LocalFastConstKind::I64,
            Self::F32 => LocalFastConstKind::F32,
            Self::F64 => LocalFastConstKind::F64,
        }
    }

    fn add_op(self) -> Op {
        match self {
            Self::I32 => vm::op_i32_add as Op,
            Self::I64 => vm::op_i64_add as Op,
            Self::F32 => vm::op_f32_add as Op,
            Self::F64 => vm::op_f64_add as Op,
        }
    }
}

#[derive(Clone, Copy)]
struct MemoryLoadDesc {
    generic: Op,
    scalar: ScalarType,
    local_base: Op,
    local_scaled_index: Op,
    const_base: Option<Op>,
}

#[derive(Clone, Copy)]
struct MemoryStoreDesc {
    generic: Op,
    scalar: ScalarType,
    local_base: Op,
    local_scaled_index: Op,
    const_base: Option<Op>,
}

macro_rules! optional_op {
    () => {
        None
    };
    ($op:expr) => {
        Some($op)
    };
}

macro_rules! memory_load_desc {
    ($generic:ident, $scalar:ident, $local_base:ident, $local_scaled_index:ident $(, $const_base:ident)?) => {
        MemoryLoadDesc {
            generic: vm::$generic as Op,
            scalar: ScalarType::$scalar,
            local_base: vm::$local_base as Op,
            local_scaled_index: vm::$local_scaled_index as Op,
            const_base: optional_op!($(vm::$const_base as Op)?),
        }
    };
}

macro_rules! memory_store_desc {
    ($generic:ident, $scalar:ident, $local_base:ident, $local_scaled_index:ident $(, $const_base:ident)?) => {
        MemoryStoreDesc {
            generic: vm::$generic as Op,
            scalar: ScalarType::$scalar,
            local_base: vm::$local_base as Op,
            local_scaled_index: vm::$local_scaled_index as Op,
            const_base: optional_op!($(vm::$const_base as Op)?),
        }
    };
}

const MEMORY_LOAD_DESCS: &[MemoryLoadDesc] = &[
    memory_load_desc!(
        op_i32_load,
        I32,
        op_i32_load_local_base,
        op_i32_load_local_scaled_index,
        op_i32_load_const_base
    ),
    memory_load_desc!(
        op_i64_load,
        I64,
        op_i64_load_local_base,
        op_i64_load_local_scaled_index,
        op_i64_load_const_base
    ),
    memory_load_desc!(
        op_f32_load,
        F32,
        op_f32_load_local_base,
        op_f32_load_local_scaled_index,
        op_f32_load_const_base
    ),
    memory_load_desc!(
        op_f64_load,
        F64,
        op_f64_load_local_base,
        op_f64_load_local_scaled_index,
        op_f64_load_const_base
    ),
    memory_load_desc!(
        op_i32_load_shared,
        I32,
        op_i32_load_shared_local_base,
        op_i32_load_shared_local_scaled_index
    ),
    memory_load_desc!(
        op_i64_load_shared,
        I64,
        op_i64_load_shared_local_base,
        op_i64_load_shared_local_scaled_index
    ),
    memory_load_desc!(
        op_f32_load_shared,
        F32,
        op_f32_load_shared_local_base,
        op_f32_load_shared_local_scaled_index
    ),
    memory_load_desc!(
        op_f64_load_shared,
        F64,
        op_f64_load_shared_local_base,
        op_f64_load_shared_local_scaled_index
    ),
    memory_load_desc!(
        op_i32_load_indexed_local,
        I32,
        op_i32_load_indexed_local_base,
        op_i32_load_indexed_local_scaled_index
    ),
    memory_load_desc!(
        op_i64_load_indexed_local,
        I64,
        op_i64_load_indexed_local_base,
        op_i64_load_indexed_local_scaled_index
    ),
    memory_load_desc!(
        op_f32_load_indexed_local,
        F32,
        op_f32_load_indexed_local_base,
        op_f32_load_indexed_local_scaled_index
    ),
    memory_load_desc!(
        op_f64_load_indexed_local,
        F64,
        op_f64_load_indexed_local_base,
        op_f64_load_indexed_local_scaled_index
    ),
    memory_load_desc!(
        op_i32_load_indexed_shared,
        I32,
        op_i32_load_indexed_shared_local_base,
        op_i32_load_indexed_shared_local_scaled_index
    ),
    memory_load_desc!(
        op_i64_load_indexed_shared,
        I64,
        op_i64_load_indexed_shared_local_base,
        op_i64_load_indexed_shared_local_scaled_index
    ),
    memory_load_desc!(
        op_f32_load_indexed_shared,
        F32,
        op_f32_load_indexed_shared_local_base,
        op_f32_load_indexed_shared_local_scaled_index
    ),
    memory_load_desc!(
        op_f64_load_indexed_shared,
        F64,
        op_f64_load_indexed_shared_local_base,
        op_f64_load_indexed_shared_local_scaled_index
    ),
    memory_load_desc!(
        op_i32_load8_u,
        I32,
        op_i32_load8_u_local_base,
        op_i32_load8_u_local_scaled_index
    ),
    memory_load_desc!(
        op_i32_load8_s,
        I32,
        op_i32_load8_s_local_base,
        op_i32_load8_s_local_scaled_index
    ),
    memory_load_desc!(
        op_i32_load16_s,
        I32,
        op_i32_load16_s_local_base,
        op_i32_load16_s_local_scaled_index
    ),
    memory_load_desc!(
        op_i32_load16_u,
        I32,
        op_i32_load16_u_local_base,
        op_i32_load16_u_local_scaled_index
    ),
    memory_load_desc!(
        op_i64_load8_s,
        I64,
        op_i64_load8_s_local_base,
        op_i64_load8_s_local_scaled_index
    ),
    memory_load_desc!(
        op_i64_load8_u,
        I64,
        op_i64_load8_u_local_base,
        op_i64_load8_u_local_scaled_index
    ),
    memory_load_desc!(
        op_i64_load16_s,
        I64,
        op_i64_load16_s_local_base,
        op_i64_load16_s_local_scaled_index
    ),
    memory_load_desc!(
        op_i64_load16_u,
        I64,
        op_i64_load16_u_local_base,
        op_i64_load16_u_local_scaled_index
    ),
    memory_load_desc!(
        op_i64_load32_s,
        I64,
        op_i64_load32_s_local_base,
        op_i64_load32_s_local_scaled_index
    ),
    memory_load_desc!(
        op_i64_load32_u,
        I64,
        op_i64_load32_u_local_base,
        op_i64_load32_u_local_scaled_index
    ),
    memory_load_desc!(
        op_i32_load8_u_shared,
        I32,
        op_i32_load8_u_shared_local_base,
        op_i32_load8_u_shared_local_scaled_index
    ),
    memory_load_desc!(
        op_i32_load8_s_shared,
        I32,
        op_i32_load8_s_shared_local_base,
        op_i32_load8_s_shared_local_scaled_index
    ),
    memory_load_desc!(
        op_i32_load16_s_shared,
        I32,
        op_i32_load16_s_shared_local_base,
        op_i32_load16_s_shared_local_scaled_index
    ),
    memory_load_desc!(
        op_i32_load16_u_shared,
        I32,
        op_i32_load16_u_shared_local_base,
        op_i32_load16_u_shared_local_scaled_index
    ),
    memory_load_desc!(
        op_i64_load8_s_shared,
        I64,
        op_i64_load8_s_shared_local_base,
        op_i64_load8_s_shared_local_scaled_index
    ),
    memory_load_desc!(
        op_i64_load8_u_shared,
        I64,
        op_i64_load8_u_shared_local_base,
        op_i64_load8_u_shared_local_scaled_index
    ),
    memory_load_desc!(
        op_i64_load16_s_shared,
        I64,
        op_i64_load16_s_shared_local_base,
        op_i64_load16_s_shared_local_scaled_index
    ),
    memory_load_desc!(
        op_i64_load16_u_shared,
        I64,
        op_i64_load16_u_shared_local_base,
        op_i64_load16_u_shared_local_scaled_index
    ),
    memory_load_desc!(
        op_i64_load32_s_shared,
        I64,
        op_i64_load32_s_shared_local_base,
        op_i64_load32_s_shared_local_scaled_index
    ),
    memory_load_desc!(
        op_i64_load32_u_shared,
        I64,
        op_i64_load32_u_shared_local_base,
        op_i64_load32_u_shared_local_scaled_index
    ),
    memory_load_desc!(
        op_i32_load8_u_indexed_local,
        I32,
        op_i32_load8_u_indexed_local_base,
        op_i32_load8_u_indexed_local_scaled_index
    ),
    memory_load_desc!(
        op_i32_load8_s_indexed_local,
        I32,
        op_i32_load8_s_indexed_local_base,
        op_i32_load8_s_indexed_local_scaled_index
    ),
    memory_load_desc!(
        op_i32_load16_s_indexed_local,
        I32,
        op_i32_load16_s_indexed_local_base,
        op_i32_load16_s_indexed_local_scaled_index
    ),
    memory_load_desc!(
        op_i32_load16_u_indexed_local,
        I32,
        op_i32_load16_u_indexed_local_base,
        op_i32_load16_u_indexed_local_scaled_index
    ),
    memory_load_desc!(
        op_i64_load8_s_indexed_local,
        I64,
        op_i64_load8_s_indexed_local_base,
        op_i64_load8_s_indexed_local_scaled_index
    ),
    memory_load_desc!(
        op_i64_load8_u_indexed_local,
        I64,
        op_i64_load8_u_indexed_local_base,
        op_i64_load8_u_indexed_local_scaled_index
    ),
    memory_load_desc!(
        op_i64_load16_s_indexed_local,
        I64,
        op_i64_load16_s_indexed_local_base,
        op_i64_load16_s_indexed_local_scaled_index
    ),
    memory_load_desc!(
        op_i64_load16_u_indexed_local,
        I64,
        op_i64_load16_u_indexed_local_base,
        op_i64_load16_u_indexed_local_scaled_index
    ),
    memory_load_desc!(
        op_i64_load32_s_indexed_local,
        I64,
        op_i64_load32_s_indexed_local_base,
        op_i64_load32_s_indexed_local_scaled_index
    ),
    memory_load_desc!(
        op_i64_load32_u_indexed_local,
        I64,
        op_i64_load32_u_indexed_local_base,
        op_i64_load32_u_indexed_local_scaled_index
    ),
    memory_load_desc!(
        op_i32_load8_u_indexed_shared,
        I32,
        op_i32_load8_u_indexed_shared_local_base,
        op_i32_load8_u_indexed_shared_local_scaled_index
    ),
    memory_load_desc!(
        op_i32_load8_s_indexed_shared,
        I32,
        op_i32_load8_s_indexed_shared_local_base,
        op_i32_load8_s_indexed_shared_local_scaled_index
    ),
    memory_load_desc!(
        op_i32_load16_s_indexed_shared,
        I32,
        op_i32_load16_s_indexed_shared_local_base,
        op_i32_load16_s_indexed_shared_local_scaled_index
    ),
    memory_load_desc!(
        op_i32_load16_u_indexed_shared,
        I32,
        op_i32_load16_u_indexed_shared_local_base,
        op_i32_load16_u_indexed_shared_local_scaled_index
    ),
    memory_load_desc!(
        op_i64_load8_s_indexed_shared,
        I64,
        op_i64_load8_s_indexed_shared_local_base,
        op_i64_load8_s_indexed_shared_local_scaled_index
    ),
    memory_load_desc!(
        op_i64_load8_u_indexed_shared,
        I64,
        op_i64_load8_u_indexed_shared_local_base,
        op_i64_load8_u_indexed_shared_local_scaled_index
    ),
    memory_load_desc!(
        op_i64_load16_s_indexed_shared,
        I64,
        op_i64_load16_s_indexed_shared_local_base,
        op_i64_load16_s_indexed_shared_local_scaled_index
    ),
    memory_load_desc!(
        op_i64_load16_u_indexed_shared,
        I64,
        op_i64_load16_u_indexed_shared_local_base,
        op_i64_load16_u_indexed_shared_local_scaled_index
    ),
    memory_load_desc!(
        op_i64_load32_s_indexed_shared,
        I64,
        op_i64_load32_s_indexed_shared_local_base,
        op_i64_load32_s_indexed_shared_local_scaled_index
    ),
    memory_load_desc!(
        op_i64_load32_u_indexed_shared,
        I64,
        op_i64_load32_u_indexed_shared_local_base,
        op_i64_load32_u_indexed_shared_local_scaled_index
    ),
];

const MEMORY_STORE_DESCS: &[MemoryStoreDesc] = &[
    memory_store_desc!(
        op_i32_store,
        I32,
        op_i32_store_local_base,
        op_i32_store_local_scaled_index,
        op_i32_store_const_base_local4
    ),
    memory_store_desc!(
        op_i64_store,
        I64,
        op_i64_store_local_base,
        op_i64_store_local_scaled_index,
        op_i64_store_const_base_local8
    ),
    memory_store_desc!(
        op_f32_store,
        F32,
        op_f32_store_local_base,
        op_f32_store_local_scaled_index,
        op_f32_store_const_base_local4
    ),
    memory_store_desc!(
        op_f64_store,
        F64,
        op_f64_store_local_base,
        op_f64_store_local_scaled_index,
        op_f64_store_const_base_local8
    ),
    memory_store_desc!(
        op_i32_store_shared,
        I32,
        op_i32_store_shared_local_base,
        op_i32_store_shared_local_scaled_index
    ),
    memory_store_desc!(
        op_i64_store_shared,
        I64,
        op_i64_store_shared_local_base,
        op_i64_store_shared_local_scaled_index
    ),
    memory_store_desc!(
        op_f32_store_shared,
        F32,
        op_f32_store_shared_local_base,
        op_f32_store_shared_local_scaled_index
    ),
    memory_store_desc!(
        op_f64_store_shared,
        F64,
        op_f64_store_shared_local_base,
        op_f64_store_shared_local_scaled_index
    ),
    memory_store_desc!(
        op_i32_store_indexed_local,
        I32,
        op_i32_store_indexed_local_base,
        op_i32_store_indexed_local_scaled_index
    ),
    memory_store_desc!(
        op_i64_store_indexed_local,
        I64,
        op_i64_store_indexed_local_base,
        op_i64_store_indexed_local_scaled_index
    ),
    memory_store_desc!(
        op_f32_store_indexed_local,
        F32,
        op_f32_store_indexed_local_base,
        op_f32_store_indexed_local_scaled_index
    ),
    memory_store_desc!(
        op_f64_store_indexed_local,
        F64,
        op_f64_store_indexed_local_base,
        op_f64_store_indexed_local_scaled_index
    ),
    memory_store_desc!(
        op_i32_store_indexed_shared,
        I32,
        op_i32_store_indexed_shared_local_base,
        op_i32_store_indexed_shared_local_scaled_index
    ),
    memory_store_desc!(
        op_i64_store_indexed_shared,
        I64,
        op_i64_store_indexed_shared_local_base,
        op_i64_store_indexed_shared_local_scaled_index
    ),
    memory_store_desc!(
        op_f32_store_indexed_shared,
        F32,
        op_f32_store_indexed_shared_local_base,
        op_f32_store_indexed_shared_local_scaled_index
    ),
    memory_store_desc!(
        op_f64_store_indexed_shared,
        F64,
        op_f64_store_indexed_shared_local_base,
        op_f64_store_indexed_shared_local_scaled_index
    ),
    memory_store_desc!(
        op_i32_store8,
        I32,
        op_i32_store8_local_base,
        op_i32_store8_local_scaled_index
    ),
    memory_store_desc!(
        op_i32_store16,
        I32,
        op_i32_store16_local_base,
        op_i32_store16_local_scaled_index
    ),
    memory_store_desc!(
        op_i64_store8,
        I64,
        op_i64_store8_local_base,
        op_i64_store8_local_scaled_index
    ),
    memory_store_desc!(
        op_i64_store16,
        I64,
        op_i64_store16_local_base,
        op_i64_store16_local_scaled_index
    ),
    memory_store_desc!(
        op_i64_store32,
        I64,
        op_i64_store32_local_base,
        op_i64_store32_local_scaled_index
    ),
    memory_store_desc!(
        op_i32_store8_shared,
        I32,
        op_i32_store8_shared_local_base,
        op_i32_store8_shared_local_scaled_index
    ),
    memory_store_desc!(
        op_i32_store16_shared,
        I32,
        op_i32_store16_shared_local_base,
        op_i32_store16_shared_local_scaled_index
    ),
    memory_store_desc!(
        op_i64_store8_shared,
        I64,
        op_i64_store8_shared_local_base,
        op_i64_store8_shared_local_scaled_index
    ),
    memory_store_desc!(
        op_i64_store16_shared,
        I64,
        op_i64_store16_shared_local_base,
        op_i64_store16_shared_local_scaled_index
    ),
    memory_store_desc!(
        op_i64_store32_shared,
        I64,
        op_i64_store32_shared_local_base,
        op_i64_store32_shared_local_scaled_index
    ),
    memory_store_desc!(
        op_i32_store8_indexed_local,
        I32,
        op_i32_store8_indexed_local_base,
        op_i32_store8_indexed_local_scaled_index
    ),
    memory_store_desc!(
        op_i32_store16_indexed_local,
        I32,
        op_i32_store16_indexed_local_base,
        op_i32_store16_indexed_local_scaled_index
    ),
    memory_store_desc!(
        op_i64_store8_indexed_local,
        I64,
        op_i64_store8_indexed_local_base,
        op_i64_store8_indexed_local_scaled_index
    ),
    memory_store_desc!(
        op_i64_store16_indexed_local,
        I64,
        op_i64_store16_indexed_local_base,
        op_i64_store16_indexed_local_scaled_index
    ),
    memory_store_desc!(
        op_i64_store32_indexed_local,
        I64,
        op_i64_store32_indexed_local_base,
        op_i64_store32_indexed_local_scaled_index
    ),
    memory_store_desc!(
        op_i32_store8_indexed_shared,
        I32,
        op_i32_store8_indexed_shared_local_base,
        op_i32_store8_indexed_shared_local_scaled_index
    ),
    memory_store_desc!(
        op_i32_store16_indexed_shared,
        I32,
        op_i32_store16_indexed_shared_local_base,
        op_i32_store16_indexed_shared_local_scaled_index
    ),
    memory_store_desc!(
        op_i64_store8_indexed_shared,
        I64,
        op_i64_store8_indexed_shared_local_base,
        op_i64_store8_indexed_shared_local_scaled_index
    ),
    memory_store_desc!(
        op_i64_store16_indexed_shared,
        I64,
        op_i64_store16_indexed_shared_local_base,
        op_i64_store16_indexed_shared_local_scaled_index
    ),
    memory_store_desc!(
        op_i64_store32_indexed_shared,
        I64,
        op_i64_store32_indexed_shared_local_base,
        op_i64_store32_indexed_shared_local_scaled_index
    ),
];

pub(crate) fn scalar_memory_load_type(op: Op) -> Option<ScalarType> {
    MEMORY_LOAD_DESCS
        .iter()
        .find(|desc| {
            std::ptr::fn_addr_eq(desc.generic, op)
                || std::ptr::fn_addr_eq(desc.local_base, op)
                || std::ptr::fn_addr_eq(desc.local_scaled_index, op)
                || desc
                    .const_base
                    .is_some_and(|const_base| std::ptr::fn_addr_eq(const_base, op))
        })
        .map(|desc| desc.scalar)
}

pub(crate) fn scalar_memory_store_type(op: Op) -> Option<ScalarType> {
    MEMORY_STORE_DESCS
        .iter()
        .find(|desc| {
            std::ptr::fn_addr_eq(desc.generic, op)
                || std::ptr::fn_addr_eq(desc.local_base, op)
                || std::ptr::fn_addr_eq(desc.local_scaled_index, op)
                || desc
                    .const_base
                    .is_some_and(|const_base| std::ptr::fn_addr_eq(const_base, op))
        })
        .map(|desc| desc.scalar)
}

fn scalar_type_from_val_type(ty: ValType) -> Option<ScalarType> {
    match ty {
        ValType::I32 => Some(ScalarType::I32),
        ValType::I64 => Some(ScalarType::I64),
        ValType::F32 => Some(ScalarType::F32),
        ValType::F64 => Some(ScalarType::F64),
        ValType::V128 | ValType::FuncRef | ValType::ExternRef => None,
    }
}

fn scalar_memory_load_type_for_inst(inst: &CanonInst) -> Option<ScalarType> {
    let _ = scalar_memory_load_type(inst.op)?;
    inst.stack_after
        .last()
        .copied()
        .and_then(scalar_type_from_val_type)
}

fn scalar_memory_store_type_for_inst(inst: &CanonInst) -> Option<ScalarType> {
    let _ = scalar_memory_store_type(inst.op)?;
    inst.stack_before
        .last()
        .copied()
        .and_then(scalar_type_from_val_type)
}

fn scalar_load_desc_for_generic(op: Op, scalar: ScalarType) -> Option<&'static MemoryLoadDesc> {
    MEMORY_LOAD_DESCS
        .iter()
        .find(|desc| desc.scalar == scalar && std::ptr::fn_addr_eq(desc.generic, op))
}

fn scalar_store_desc_for_generic(op: Op, scalar: ScalarType) -> Option<&'static MemoryStoreDesc> {
    MEMORY_STORE_DESCS
        .iter()
        .find(|desc| desc.scalar == scalar && std::ptr::fn_addr_eq(desc.generic, op))
}

fn scalar_local_base_load_family_for_type(op: Op, scalar: ScalarType) -> Option<Op> {
    scalar_load_desc_for_generic(op, scalar).map(|desc| desc.local_base)
}

fn scalar_local_scaled_index_load_family_for_type(op: Op, scalar: ScalarType) -> Option<Op> {
    scalar_load_desc_for_generic(op, scalar).map(|desc| desc.local_scaled_index)
}

fn scalar_const_base_load_family_for_type(op: Op, scalar: ScalarType) -> Option<Op> {
    scalar_load_desc_for_generic(op, scalar).and_then(|desc| desc.const_base)
}

fn local_get4_local_base_load_family(op: Op) -> Option<Op> {
    if std::ptr::fn_addr_eq(op, vm::op_i32_load as Op) {
        return Some(vm::op_local_get4_i32_load_local_base as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i32_load8_s as Op) {
        return Some(vm::op_local_get4_i32_load8_s_local_base as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i32_load8_u as Op) {
        return Some(vm::op_local_get4_i32_load8_u_local_base as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i32_load16_s as Op) {
        return Some(vm::op_local_get4_i32_load16_s_local_base as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i32_load16_u as Op) {
        return Some(vm::op_local_get4_i32_load16_u_local_base as Op);
    }
    None
}

fn local_base_load_set4_family(op: Op, tee: bool) -> Option<Op> {
    if std::ptr::fn_addr_eq(op, vm::op_i32_load_local_base as Op) {
        return Some(if tee {
            vm::op_i32_load_local_base_tee4 as Op
        } else {
            vm::op_i32_load_local_base_set4 as Op
        });
    }
    if std::ptr::fn_addr_eq(op, vm::op_i32_load8_s_local_base as Op) {
        return Some(if tee {
            vm::op_i32_load8_s_local_base_tee4 as Op
        } else {
            vm::op_i32_load8_s_local_base_set4 as Op
        });
    }
    if std::ptr::fn_addr_eq(op, vm::op_i32_load8_u_local_base as Op) {
        return Some(if tee {
            vm::op_i32_load8_u_local_base_tee4 as Op
        } else {
            vm::op_i32_load8_u_local_base_set4 as Op
        });
    }
    if std::ptr::fn_addr_eq(op, vm::op_i32_load16_s_local_base as Op) {
        return Some(if tee {
            vm::op_i32_load16_s_local_base_tee4 as Op
        } else {
            vm::op_i32_load16_s_local_base_set4 as Op
        });
    }
    if std::ptr::fn_addr_eq(op, vm::op_i32_load16_u_local_base as Op) {
        return Some(if tee {
            vm::op_i32_load16_u_local_base_tee4 as Op
        } else {
            vm::op_i32_load16_u_local_base_set4 as Op
        });
    }
    None
}

fn local_base_load_local_get4_family(op: Op, tee: bool) -> Option<Op> {
    if std::ptr::fn_addr_eq(op, vm::op_i32_load_local_base as Op) {
        return Some(if tee {
            vm::op_i32_load_local_base_tee4_local_get4 as Op
        } else {
            vm::op_i32_load_local_base_local_get4 as Op
        });
    }
    if std::ptr::fn_addr_eq(op, vm::op_i32_load8_s_local_base as Op) {
        return Some(if tee {
            vm::op_i32_load8_s_local_base_tee4_local_get4 as Op
        } else {
            vm::op_i32_load8_s_local_base_local_get4 as Op
        });
    }
    if std::ptr::fn_addr_eq(op, vm::op_i32_load8_u_local_base as Op) {
        return Some(if tee {
            vm::op_i32_load8_u_local_base_tee4_local_get4 as Op
        } else {
            vm::op_i32_load8_u_local_base_local_get4 as Op
        });
    }
    if std::ptr::fn_addr_eq(op, vm::op_i32_load16_s_local_base as Op) {
        return Some(if tee {
            vm::op_i32_load16_s_local_base_tee4_local_get4 as Op
        } else {
            vm::op_i32_load16_s_local_base_local_get4 as Op
        });
    }
    if std::ptr::fn_addr_eq(op, vm::op_i32_load16_u_local_base as Op) {
        return Some(if tee {
            vm::op_i32_load16_u_local_base_tee4_local_get4 as Op
        } else {
            vm::op_i32_load16_u_local_base_local_get4 as Op
        });
    }
    None
}

fn local_base_load_local_get4_scalar_load_family(first: Op, second: Op) -> Option<Op> {
    if std::ptr::fn_addr_eq(first, vm::op_i32_load16_u as Op)
        && std::ptr::fn_addr_eq(second, vm::op_i32_load16_u as Op)
    {
        return Some(vm::op_i32_load16_u_local_base_local_get4_i32_load16_u as Op);
    }
    if std::ptr::fn_addr_eq(first, vm::op_i32_load16_s as Op)
        && std::ptr::fn_addr_eq(second, vm::op_i32_load16_s as Op)
    {
        return Some(vm::op_i32_load16_s_local_base_local_get4_i32_load16_s as Op);
    }
    None
}

fn local_base_set4_load_local_get4_family(op: Op) -> Option<Op> {
    if std::ptr::fn_addr_eq(op, vm::op_i32_load as Op) {
        return Some(vm::op_i32_load_local_base_set4_i32_load_local_base_local_get4 as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i32_load8_s as Op) {
        return Some(vm::op_i32_load_local_base_set4_i32_load8_s_local_base_local_get4 as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i32_load8_u as Op) {
        return Some(vm::op_i32_load_local_base_set4_i32_load8_u_local_base_local_get4 as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i32_load16_s as Op) {
        return Some(vm::op_i32_load_local_base_set4_i32_load16_s_local_base_local_get4 as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i32_load16_u as Op) {
        return Some(vm::op_i32_load_local_base_set4_i32_load16_u_local_base_local_get4 as Op);
    }
    None
}

fn local_base_set4_load_local_eq_br_if_family(op: Op) -> Option<Op> {
    if std::ptr::fn_addr_eq(op, vm::op_i32_load as Op) {
        return Some(vm::op_i32_load_local_base_set4_i32_load_local_base_local_eq_br_if as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i32_load8_s as Op) {
        return Some(vm::op_i32_load_local_base_set4_i32_load8_s_local_base_local_eq_br_if as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i32_load8_u as Op) {
        return Some(vm::op_i32_load_local_base_set4_i32_load8_u_local_base_local_eq_br_if as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i32_load16_s as Op) {
        return Some(vm::op_i32_load_local_base_set4_i32_load16_s_local_base_local_eq_br_if as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i32_load16_u as Op) {
        return Some(vm::op_i32_load_local_base_set4_i32_load16_u_local_base_local_eq_br_if as Op);
    }
    None
}

fn local_base_set4_load_family(op: Op) -> Option<Op> {
    if std::ptr::fn_addr_eq(op, vm::op_i32_load as Op) {
        return Some(vm::op_i32_load_local_base_set4_i32_load_local_base as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i32_load8_s as Op) {
        return Some(vm::op_i32_load_local_base_set4_i32_load8_s_local_base as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i32_load8_u as Op) {
        return Some(vm::op_i32_load_local_base_set4_i32_load8_u_local_base as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i32_load16_s as Op) {
        return Some(vm::op_i32_load_local_base_set4_i32_load16_s_local_base as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i32_load16_u as Op) {
        return Some(vm::op_i32_load_local_base_set4_i32_load16_u_local_base as Op);
    }
    None
}

fn local_base_load_tee4_branch_family(op: Op, eqz: bool) -> Option<Op> {
    if std::ptr::fn_addr_eq(op, vm::op_i32_load_local_base as Op) {
        return Some(if eqz {
            vm::op_i32_load_local_base_tee4_i32_eqz_br_if as Op
        } else {
            vm::op_i32_load_local_base_tee4_br_if as Op
        });
    }
    if std::ptr::fn_addr_eq(op, vm::op_i32_load8_s_local_base as Op) {
        return Some(if eqz {
            vm::op_i32_load8_s_local_base_tee4_i32_eqz_br_if as Op
        } else {
            vm::op_i32_load8_s_local_base_tee4_br_if as Op
        });
    }
    if std::ptr::fn_addr_eq(op, vm::op_i32_load8_u_local_base as Op) {
        return Some(if eqz {
            vm::op_i32_load8_u_local_base_tee4_i32_eqz_br_if as Op
        } else {
            vm::op_i32_load8_u_local_base_tee4_br_if as Op
        });
    }
    if std::ptr::fn_addr_eq(op, vm::op_i32_load16_s_local_base as Op) {
        return Some(if eqz {
            vm::op_i32_load16_s_local_base_tee4_i32_eqz_br_if as Op
        } else {
            vm::op_i32_load16_s_local_base_tee4_br_if as Op
        });
    }
    if std::ptr::fn_addr_eq(op, vm::op_i32_load16_u_local_base as Op) {
        return Some(if eqz {
            vm::op_i32_load16_u_local_base_tee4_i32_eqz_br_if as Op
        } else {
            vm::op_i32_load16_u_local_base_tee4_br_if as Op
        });
    }
    None
}

fn i32_load_local_get4_family(op: Op) -> Option<Op> {
    if std::ptr::fn_addr_eq(op, vm::op_i32_load as Op) {
        return Some(vm::op_i32_load_local_get4 as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i32_load8_s as Op) {
        return Some(vm::op_i32_load8_s_local_get4 as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i32_load8_u as Op) {
        return Some(vm::op_i32_load8_u_local_get4 as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i32_load16_s as Op) {
        return Some(vm::op_i32_load16_s_local_get4 as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i32_load16_u as Op) {
        return Some(vm::op_i32_load16_u_local_get4 as Op);
    }
    None
}

fn i32_load_tee4_branch_family(op: Op, eqz: bool) -> Option<Op> {
    if std::ptr::fn_addr_eq(op, vm::op_i32_load as Op) {
        return Some(if eqz {
            vm::op_i32_load_tee4_i32_eqz_br_if as Op
        } else {
            vm::op_i32_load_tee4_br_if as Op
        });
    }
    if std::ptr::fn_addr_eq(op, vm::op_i32_load8_s as Op) {
        return Some(if eqz {
            vm::op_i32_load8_s_tee4_i32_eqz_br_if as Op
        } else {
            vm::op_i32_load8_s_tee4_br_if as Op
        });
    }
    if std::ptr::fn_addr_eq(op, vm::op_i32_load8_u as Op) {
        return Some(if eqz {
            vm::op_i32_load8_u_tee4_i32_eqz_br_if as Op
        } else {
            vm::op_i32_load8_u_tee4_br_if as Op
        });
    }
    if std::ptr::fn_addr_eq(op, vm::op_i32_load16_s as Op) {
        return Some(if eqz {
            vm::op_i32_load16_s_tee4_i32_eqz_br_if as Op
        } else {
            vm::op_i32_load16_s_tee4_br_if as Op
        });
    }
    if std::ptr::fn_addr_eq(op, vm::op_i32_load16_u as Op) {
        return Some(if eqz {
            vm::op_i32_load16_u_tee4_i32_eqz_br_if as Op
        } else {
            vm::op_i32_load16_u_tee4_br_if as Op
        });
    }
    None
}

fn local_base_store_local_get4_family(op: Op) -> Option<Op> {
    if std::ptr::fn_addr_eq(op, vm::op_i32_store_local_base as Op) {
        return Some(vm::op_i32_store_local_base_local_get4 as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i32_store8_local_base as Op) {
        return Some(vm::op_i32_store8_local_base_local_get4 as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i32_store16_local_base as Op) {
        return Some(vm::op_i32_store16_local_base_local_get4 as Op);
    }
    None
}

fn scalar_local_base_store_family_for_type(op: Op, scalar: ScalarType) -> Option<Op> {
    scalar_store_desc_for_generic(op, scalar).map(|desc| desc.local_base)
}

fn scalar_local_scaled_index_store_family_for_type(op: Op, scalar: ScalarType) -> Option<Op> {
    scalar_store_desc_for_generic(op, scalar).map(|desc| desc.local_scaled_index)
}

fn scalar_const_base_store_family_for_type(op: Op, scalar: ScalarType) -> Option<Op> {
    scalar_store_desc_for_generic(op, scalar).and_then(|desc| desc.const_base)
}

pub(crate) fn scalar_local_base_load_family(op: Op) -> Option<Op> {
    MEMORY_LOAD_DESCS
        .iter()
        .find(|desc| std::ptr::fn_addr_eq(desc.generic, op))
        .map(|desc| desc.local_base)
}

pub(crate) fn scalar_local_scaled_index_load_family(op: Op) -> Option<Op> {
    MEMORY_LOAD_DESCS
        .iter()
        .find(|desc| std::ptr::fn_addr_eq(desc.generic, op))
        .map(|desc| desc.local_scaled_index)
}

pub(crate) fn scalar_const_base_load_family(op: Op) -> Option<Op> {
    MEMORY_LOAD_DESCS
        .iter()
        .find(|desc| std::ptr::fn_addr_eq(desc.generic, op))
        .and_then(|desc| desc.const_base)
}

pub(crate) fn scalar_local_base_store_family(op: Op) -> Option<Op> {
    MEMORY_STORE_DESCS
        .iter()
        .find(|desc| std::ptr::fn_addr_eq(desc.generic, op))
        .map(|desc| desc.local_base)
}

pub(crate) fn scalar_local_scaled_index_store_family(op: Op) -> Option<Op> {
    MEMORY_STORE_DESCS
        .iter()
        .find(|desc| std::ptr::fn_addr_eq(desc.generic, op))
        .map(|desc| desc.local_scaled_index)
}

pub(crate) fn scalar_const_base_store_family(op: Op) -> Option<Op> {
    MEMORY_STORE_DESCS
        .iter()
        .find(|desc| std::ptr::fn_addr_eq(desc.generic, op))
        .and_then(|desc| desc.const_base)
}

#[allow(dead_code)]
pub(crate) fn is_scalar_memory_load_op(op: Op) -> bool {
    scalar_memory_load_type(op).is_some()
}

#[allow(dead_code)]
pub(crate) fn is_scalar_memory_store_op(op: Op) -> bool {
    scalar_memory_store_type(op).is_some()
}

pub(crate) fn is_i32_memory_load_root_op(op: Op) -> bool {
    scalar_memory_load_type(op) == Some(ScalarType::I32)
}

struct ScalarLoadConstBaseSpec;
struct ScalarStoreConstBaseSpec;
struct I32LoadStoreLocalBaseReverseLoopSpec;
struct I32LoadStoreLocalBaseRelinkLoopSpec;
struct I32SumClipLocalBaseLoopSpec;
struct I32Load16UUpdateStore16LocalBaseLoopSpec;
struct I32Load16SDot4LocalBaseLoopSpec;
struct I32Load16UBitmixAccLocalBaseDeltaLoopSpec;
struct I32Load16SMulAddLocalBaseDeltaLoopSpec;
struct I32Load16SMulAddLocalBaseLoopSpec;

pub(crate) const ENABLE_UNVERIFIED_LOOP_FUSIONS: bool = false;
struct ScalarCopyLocalBaseRunSpec;
struct I32IncLocalBaseSpec;
struct I32LoadConstBaseLocalGet4AddSet4Spec;
struct LocalGet4LocalGet4XorTee4U8Shl1I32Load16USpec;
struct I32LoadStoreLocalBaseLocalGet4Spec;
struct I32LoadLocalBaseLocalGet4ScalarLoadTee4CmpBrIfSpec;
struct I32LoadLocalBaseTee4BrIfSpec;
struct I32LoadTee4BrIfSpec;
struct LocalGet4I32LoadLocalBaseAddSet4Spec;
struct LocalGet4ScalarLoadLocalBaseSpec;
struct I32LoadLocalBaseSet4I32Load16ULocalBaseLocalEqSearchLoopSpec;
struct I32LoadLocalBaseSet4I32Load8ULocalBaseLocalMaskedSearchLoopSpec;
struct I32LoadLocalBaseSet4I32Load8ULocalBaseLocalMaskedCompareBrIfSpec;
struct I32LoadLocalBaseSet4ScalarLoadLocalBaseLocalEqBrIfSpec;
struct I32LoadLocalBaseSet4ScalarLoadLocalBaseLocalGet4Spec;
struct I32LoadLocalBaseSet4ScalarLoadLocalBaseSpec;
struct ScalarLoadLocalBaseLocalGet4ScalarLoadSpec;
struct ScalarLoadLocalBaseLocalGet4Spec;
struct ScalarLoadLocalBaseSet4Spec;
struct ScalarLoadLocalBaseSpec;
struct ScalarLoadLocalGet4Spec;
struct ScalarStoreLocalBaseSpec;
struct ScalarLoadLocalScaledIndexSpec;
struct ScalarStoreLocalScaledIndexSpec;

impl FamilySpec for I32LoadStoreLocalBaseReverseLoopSpec {
    fn group(&self) -> FamilyGroup {
        FamilyGroup::Memory
    }

    fn name(&self) -> &'static str {
        "op_i32_load_store_local_base_reverse_loop"
    }

    fn matches(&self, ctx: &SelectionContext<'_>, cursor: usize) -> bool {
        emit_i32_load_store_local_base_reverse_loop(ctx, cursor).is_some()
    }

    fn cost(&self, ctx: &SelectionContext<'_>, _cursor: usize) -> i32 {
        300 + loop_bonus(ctx)
    }

    fn emit(&self, ctx: &SelectionContext<'_>, cursor: usize) -> Option<MatchResult> {
        emit_i32_load_store_local_base_reverse_loop(ctx, cursor)
    }
}

impl FamilySpec for I32LoadStoreLocalBaseRelinkLoopSpec {
    fn group(&self) -> FamilyGroup {
        FamilyGroup::Memory
    }

    fn name(&self) -> &'static str {
        "op_i32_load_store_local_base_relink_loop"
    }

    fn matches(&self, ctx: &SelectionContext<'_>, cursor: usize) -> bool {
        emit_i32_load_store_local_base_relink_loop(ctx, cursor).is_some()
    }

    fn cost(&self, ctx: &SelectionContext<'_>, _cursor: usize) -> i32 {
        280 + loop_bonus(ctx)
    }

    fn emit(&self, ctx: &SelectionContext<'_>, cursor: usize) -> Option<MatchResult> {
        emit_i32_load_store_local_base_relink_loop(ctx, cursor)
    }
}

impl FamilySpec for I32SumClipLocalBaseLoopSpec {
    fn group(&self) -> FamilyGroup {
        FamilyGroup::Memory
    }

    fn name(&self) -> &'static str {
        "op_i32_sum_clip_local_base_loop"
    }

    fn matches(&self, ctx: &SelectionContext<'_>, cursor: usize) -> bool {
        ENABLE_UNVERIFIED_LOOP_FUSIONS && emit_i32_sum_clip_local_base_loop(ctx, cursor).is_some()
    }

    fn cost(&self, ctx: &SelectionContext<'_>, _cursor: usize) -> i32 {
        240 + loop_bonus(ctx)
    }

    fn emit(&self, ctx: &SelectionContext<'_>, cursor: usize) -> Option<MatchResult> {
        ENABLE_UNVERIFIED_LOOP_FUSIONS
            .then(|| emit_i32_sum_clip_local_base_loop(ctx, cursor))
            .flatten()
    }
}

impl FamilySpec for I32Load16UUpdateStore16LocalBaseLoopSpec {
    fn group(&self) -> FamilyGroup {
        FamilyGroup::Memory
    }

    fn name(&self) -> &'static str {
        "op_i32_load16_u_update_store16_local_base_loop"
    }

    fn matches(&self, ctx: &SelectionContext<'_>, cursor: usize) -> bool {
        ENABLE_UNVERIFIED_LOOP_FUSIONS
            && emit_i32_load16_u_update_store16_local_base_loop(ctx, cursor).is_some()
    }

    fn cost(&self, ctx: &SelectionContext<'_>, _cursor: usize) -> i32 {
        260 + loop_bonus(ctx)
    }

    fn emit(&self, ctx: &SelectionContext<'_>, cursor: usize) -> Option<MatchResult> {
        ENABLE_UNVERIFIED_LOOP_FUSIONS
            .then(|| emit_i32_load16_u_update_store16_local_base_loop(ctx, cursor))
            .flatten()
    }
}

impl FamilySpec for ScalarCopyLocalBaseRunSpec {
    fn group(&self) -> FamilyGroup {
        FamilyGroup::Memory
    }

    fn name(&self) -> &'static str {
        "op_scalar_copy_local_base_run"
    }

    fn matches(&self, ctx: &SelectionContext<'_>, cursor: usize) -> bool {
        emit_scalar_copy_local_base_run(ctx, cursor).is_some()
    }

    fn cost(&self, ctx: &SelectionContext<'_>, cursor: usize) -> i32 {
        emit_scalar_copy_local_base_run(ctx, cursor)
            .map(|matched| matched.cost)
            .unwrap_or(0)
    }

    fn emit(&self, ctx: &SelectionContext<'_>, cursor: usize) -> Option<MatchResult> {
        emit_scalar_copy_local_base_run(ctx, cursor)
    }
}

impl FamilySpec for I32Load16SDot4LocalBaseLoopSpec {
    fn group(&self) -> FamilyGroup {
        FamilyGroup::Memory
    }

    fn name(&self) -> &'static str {
        "op_i32_load16_s_dot4_local_base_loop"
    }

    fn matches(&self, ctx: &SelectionContext<'_>, cursor: usize) -> bool {
        ENABLE_UNVERIFIED_LOOP_FUSIONS
            && emit_i32_load16_s_dot4_local_base_loop(ctx, cursor).is_some()
    }

    fn cost(&self, ctx: &SelectionContext<'_>, _cursor: usize) -> i32 {
        460 + loop_bonus(ctx)
    }

    fn emit(&self, ctx: &SelectionContext<'_>, cursor: usize) -> Option<MatchResult> {
        ENABLE_UNVERIFIED_LOOP_FUSIONS
            .then(|| emit_i32_load16_s_dot4_local_base_loop(ctx, cursor))
            .flatten()
    }
}

impl FamilySpec for I32Load16SMulAddLocalBaseLoopSpec {
    fn group(&self) -> FamilyGroup {
        FamilyGroup::Memory
    }

    fn name(&self) -> &'static str {
        "op_i32_load16_s_mul_add_local_base_loop"
    }

    fn matches(&self, ctx: &SelectionContext<'_>, cursor: usize) -> bool {
        ENABLE_UNVERIFIED_LOOP_FUSIONS
            && emit_i32_load16_s_mul_add_local_base_loop(ctx, cursor).is_some()
    }

    fn cost(&self, ctx: &SelectionContext<'_>, _cursor: usize) -> i32 {
        170 + loop_bonus(ctx)
    }

    fn emit(&self, ctx: &SelectionContext<'_>, cursor: usize) -> Option<MatchResult> {
        ENABLE_UNVERIFIED_LOOP_FUSIONS
            .then(|| emit_i32_load16_s_mul_add_local_base_loop(ctx, cursor))
            .flatten()
    }
}

impl FamilySpec for I32Load16UBitmixAccLocalBaseDeltaLoopSpec {
    fn group(&self) -> FamilyGroup {
        FamilyGroup::Memory
    }

    fn name(&self) -> &'static str {
        "op_i32_load16_u_bitmix_acc_local_base_delta_loop"
    }

    fn matches(&self, ctx: &SelectionContext<'_>, cursor: usize) -> bool {
        ENABLE_UNVERIFIED_LOOP_FUSIONS
            && emit_i32_load16_u_bitmix_acc_local_base_delta_loop(ctx, cursor).is_some()
    }

    fn cost(&self, ctx: &SelectionContext<'_>, _cursor: usize) -> i32 {
        260 + loop_bonus(ctx)
    }

    fn emit(&self, ctx: &SelectionContext<'_>, cursor: usize) -> Option<MatchResult> {
        ENABLE_UNVERIFIED_LOOP_FUSIONS
            .then(|| emit_i32_load16_u_bitmix_acc_local_base_delta_loop(ctx, cursor))
            .flatten()
    }
}

impl FamilySpec for I32Load16SMulAddLocalBaseDeltaLoopSpec {
    fn group(&self) -> FamilyGroup {
        FamilyGroup::Memory
    }

    fn name(&self) -> &'static str {
        "op_i32_load16_s_mul_add_local_base_delta_loop"
    }

    fn matches(&self, ctx: &SelectionContext<'_>, cursor: usize) -> bool {
        ENABLE_UNVERIFIED_LOOP_FUSIONS
            && emit_i32_load16_s_mul_add_local_base_delta_loop(ctx, cursor).is_some()
    }

    fn cost(&self, ctx: &SelectionContext<'_>, _cursor: usize) -> i32 {
        176 + loop_bonus(ctx)
    }

    fn emit(&self, ctx: &SelectionContext<'_>, cursor: usize) -> Option<MatchResult> {
        ENABLE_UNVERIFIED_LOOP_FUSIONS
            .then(|| emit_i32_load16_s_mul_add_local_base_delta_loop(ctx, cursor))
            .flatten()
    }
}

impl FamilySpec for I32IncLocalBaseSpec {
    fn group(&self) -> FamilyGroup {
        FamilyGroup::Memory
    }

    fn name(&self) -> &'static str {
        "op_i32_inc_local_base"
    }

    fn matches(&self, ctx: &SelectionContext<'_>, cursor: usize) -> bool {
        emit_i32_inc_local_base(ctx, cursor).is_some()
    }

    fn cost(&self, ctx: &SelectionContext<'_>, _cursor: usize) -> i32 {
        82 + loop_bonus(ctx)
    }

    fn emit(&self, ctx: &SelectionContext<'_>, cursor: usize) -> Option<MatchResult> {
        emit_i32_inc_local_base(ctx, cursor)
    }
}

impl FamilySpec for ScalarLoadConstBaseSpec {
    fn group(&self) -> FamilyGroup {
        FamilyGroup::Memory
    }

    fn name(&self) -> &'static str {
        "memory.const_base_load"
    }

    fn matches(&self, ctx: &SelectionContext<'_>, cursor: usize) -> bool {
        emit_scalar_load_const_base(ctx, cursor).is_some()
    }

    fn cost(&self, ctx: &SelectionContext<'_>, _cursor: usize) -> i32 {
        18 + loop_bonus(ctx)
    }

    fn emit(&self, ctx: &SelectionContext<'_>, cursor: usize) -> Option<MatchResult> {
        emit_scalar_load_const_base(ctx, cursor).map(|(result, _)| result)
    }
}

impl FamilySpec for ScalarStoreConstBaseSpec {
    fn group(&self) -> FamilyGroup {
        FamilyGroup::Memory
    }

    fn name(&self) -> &'static str {
        "memory.const_base_store_local"
    }

    fn matches(&self, ctx: &SelectionContext<'_>, cursor: usize) -> bool {
        emit_scalar_store_const_base(ctx, cursor).is_some()
    }

    fn cost(&self, ctx: &SelectionContext<'_>, _cursor: usize) -> i32 {
        24 + loop_bonus(ctx)
    }

    fn emit(&self, ctx: &SelectionContext<'_>, cursor: usize) -> Option<MatchResult> {
        emit_scalar_store_const_base(ctx, cursor).map(|(result, _)| result)
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

impl FamilySpec for LocalGet4LocalGet4XorTee4U8Shl1I32Load16USpec {
    fn group(&self) -> FamilyGroup {
        FamilyGroup::Memory
    }

    fn name(&self) -> &'static str {
        "op_local_get4_local_get4_i32_xor_tee4_u8_shl1_i32_load16_u"
    }

    fn matches(&self, ctx: &SelectionContext<'_>, cursor: usize) -> bool {
        emit_local_get4_local_get4_xor_tee4_u8_shl1_i32_load16_u(ctx, cursor).is_some()
    }

    fn cost(&self, ctx: &SelectionContext<'_>, _cursor: usize) -> i32 {
        104 + loop_bonus(ctx)
    }

    fn emit(&self, ctx: &SelectionContext<'_>, cursor: usize) -> Option<MatchResult> {
        emit_local_get4_local_get4_xor_tee4_u8_shl1_i32_load16_u(ctx, cursor)
    }
}

impl FamilySpec for I32LoadLocalBaseTee4BrIfSpec {
    fn group(&self) -> FamilyGroup {
        FamilyGroup::Memory
    }

    fn name(&self) -> &'static str {
        "op_i32_load_local_base_tee4_br_if"
    }

    fn matches(&self, ctx: &SelectionContext<'_>, cursor: usize) -> bool {
        emit_i32_load_local_base_tee4_br_if(ctx, cursor).is_some()
    }

    fn cost(&self, ctx: &SelectionContext<'_>, _cursor: usize) -> i32 {
        62 + loop_bonus(ctx)
    }

    fn emit(&self, ctx: &SelectionContext<'_>, cursor: usize) -> Option<MatchResult> {
        emit_i32_load_local_base_tee4_br_if(ctx, cursor)
    }
}

impl FamilySpec for I32LoadTee4BrIfSpec {
    fn group(&self) -> FamilyGroup {
        FamilyGroup::Memory
    }

    fn name(&self) -> &'static str {
        "op_i32_load_tee4_br_if"
    }

    fn matches(&self, ctx: &SelectionContext<'_>, cursor: usize) -> bool {
        emit_i32_load_tee4_br_if(ctx, cursor).is_some()
    }

    fn cost(&self, ctx: &SelectionContext<'_>, _cursor: usize) -> i32 {
        44 + loop_bonus(ctx)
    }

    fn emit(&self, ctx: &SelectionContext<'_>, cursor: usize) -> Option<MatchResult> {
        emit_i32_load_tee4_br_if(ctx, cursor)
    }
}

impl FamilySpec for LocalGet4ScalarLoadLocalBaseSpec {
    fn group(&self) -> FamilyGroup {
        FamilyGroup::Memory
    }

    fn name(&self) -> &'static str {
        "memory.local_get4_local_base_load"
    }

    fn matches(&self, ctx: &SelectionContext<'_>, cursor: usize) -> bool {
        emit_local_get4_scalar_load_local_base(ctx, cursor).is_some()
    }

    fn cost(&self, ctx: &SelectionContext<'_>, _cursor: usize) -> i32 {
        38 + loop_bonus(ctx)
    }

    fn emit(&self, ctx: &SelectionContext<'_>, cursor: usize) -> Option<MatchResult> {
        emit_local_get4_scalar_load_local_base(ctx, cursor)
    }
}

impl FamilySpec for I32LoadStoreLocalBaseLocalGet4Spec {
    fn group(&self) -> FamilyGroup {
        FamilyGroup::Memory
    }

    fn name(&self) -> &'static str {
        "op_i32_load_store_local_base_local_get4"
    }

    fn matches(&self, ctx: &SelectionContext<'_>, cursor: usize) -> bool {
        emit_i32_load_store_local_base_local_get4(ctx, cursor).is_some()
    }

    fn cost(&self, ctx: &SelectionContext<'_>, _cursor: usize) -> i32 {
        64 + loop_bonus(ctx)
    }

    fn emit(&self, ctx: &SelectionContext<'_>, cursor: usize) -> Option<MatchResult> {
        emit_i32_load_store_local_base_local_get4(ctx, cursor)
    }
}

impl FamilySpec for I32LoadLocalBaseLocalGet4ScalarLoadTee4CmpBrIfSpec {
    fn group(&self) -> FamilyGroup {
        FamilyGroup::Memory
    }

    fn name(&self) -> &'static str {
        "op_i32_load_local_base_local_get4_i32_load_tee4_cmp_br_if"
    }

    fn matches(&self, ctx: &SelectionContext<'_>, cursor: usize) -> bool {
        emit_i32_load_local_base_local_get4_scalar_load_tee4_cmp_br_if(ctx, cursor).is_some()
    }

    fn cost(&self, ctx: &SelectionContext<'_>, _cursor: usize) -> i32 {
        134 + loop_bonus(ctx)
    }

    fn emit(&self, ctx: &SelectionContext<'_>, cursor: usize) -> Option<MatchResult> {
        emit_i32_load_local_base_local_get4_scalar_load_tee4_cmp_br_if(ctx, cursor)
    }
}

impl FamilySpec for I32LoadLocalBaseSet4I32Load16ULocalBaseLocalEqSearchLoopSpec {
    fn group(&self) -> FamilyGroup {
        FamilyGroup::Memory
    }

    fn name(&self) -> &'static str {
        "op_i32_load_local_base_set4_i32_load16_u_local_base_local_eq_search_loop"
    }

    fn matches(&self, ctx: &SelectionContext<'_>, cursor: usize) -> bool {
        emit_i32_load_local_base_set4_i32_load16_u_local_base_local_eq_search_loop(ctx, cursor)
            .is_some()
    }

    fn cost(&self, ctx: &SelectionContext<'_>, _cursor: usize) -> i32 {
        260 + loop_bonus(ctx)
    }

    fn emit(&self, ctx: &SelectionContext<'_>, cursor: usize) -> Option<MatchResult> {
        emit_i32_load_local_base_set4_i32_load16_u_local_base_local_eq_search_loop(ctx, cursor)
    }
}

impl FamilySpec for I32LoadLocalBaseSet4I32Load8ULocalBaseLocalMaskedSearchLoopSpec {
    fn group(&self) -> FamilyGroup {
        FamilyGroup::Memory
    }

    fn name(&self) -> &'static str {
        "op_i32_load_local_base_set4_i32_load8_u_local_base_local_masked_search_loop"
    }

    fn matches(&self, ctx: &SelectionContext<'_>, cursor: usize) -> bool {
        emit_i32_load_local_base_set4_i32_load8_u_local_base_local_masked_search_loop(ctx, cursor)
            .is_some()
    }

    fn cost(&self, ctx: &SelectionContext<'_>, _cursor: usize) -> i32 {
        280 + loop_bonus(ctx)
    }

    fn emit(&self, ctx: &SelectionContext<'_>, cursor: usize) -> Option<MatchResult> {
        emit_i32_load_local_base_set4_i32_load8_u_local_base_local_masked_search_loop(ctx, cursor)
    }
}

impl FamilySpec for I32LoadLocalBaseSet4I32Load8ULocalBaseLocalMaskedCompareBrIfSpec {
    fn group(&self) -> FamilyGroup {
        FamilyGroup::Memory
    }

    fn name(&self) -> &'static str {
        "op_i32_load_local_base_set4_i32_load8_u_local_base_local_masked_compare_br_if"
    }

    fn matches(&self, ctx: &SelectionContext<'_>, cursor: usize) -> bool {
        emit_i32_load_local_base_set4_i32_load8_u_local_base_local_masked_compare_br_if(ctx, cursor)
            .is_some()
    }

    fn cost(&self, ctx: &SelectionContext<'_>, _cursor: usize) -> i32 {
        136 + loop_bonus(ctx)
    }

    fn emit(&self, ctx: &SelectionContext<'_>, cursor: usize) -> Option<MatchResult> {
        emit_i32_load_local_base_set4_i32_load8_u_local_base_local_masked_compare_br_if(ctx, cursor)
    }
}

impl FamilySpec for I32LoadLocalBaseSet4ScalarLoadLocalBaseLocalGet4Spec {
    fn group(&self) -> FamilyGroup {
        FamilyGroup::Memory
    }

    fn name(&self) -> &'static str {
        "op_i32_load_local_base_set4_i32_load_local_base_local_get4"
    }

    fn matches(&self, ctx: &SelectionContext<'_>, cursor: usize) -> bool {
        emit_i32_load_local_base_set4_scalar_load_local_base_local_get4(ctx, cursor).is_some()
    }

    fn cost(&self, ctx: &SelectionContext<'_>, _cursor: usize) -> i32 {
        86 + loop_bonus(ctx)
    }

    fn emit(&self, ctx: &SelectionContext<'_>, cursor: usize) -> Option<MatchResult> {
        emit_i32_load_local_base_set4_scalar_load_local_base_local_get4(ctx, cursor)
    }
}

impl FamilySpec for I32LoadLocalBaseSet4ScalarLoadLocalBaseLocalEqBrIfSpec {
    fn group(&self) -> FamilyGroup {
        FamilyGroup::Memory
    }

    fn name(&self) -> &'static str {
        "op_i32_load_local_base_set4_i32_load_local_base_local_eq_br_if"
    }

    fn matches(&self, ctx: &SelectionContext<'_>, cursor: usize) -> bool {
        emit_i32_load_local_base_set4_scalar_load_local_base_local_eq_br_if(ctx, cursor).is_some()
    }

    fn cost(&self, ctx: &SelectionContext<'_>, _cursor: usize) -> i32 {
        118 + loop_bonus(ctx)
    }

    fn emit(&self, ctx: &SelectionContext<'_>, cursor: usize) -> Option<MatchResult> {
        emit_i32_load_local_base_set4_scalar_load_local_base_local_eq_br_if(ctx, cursor)
    }
}

impl FamilySpec for I32LoadLocalBaseSet4ScalarLoadLocalBaseSpec {
    fn group(&self) -> FamilyGroup {
        FamilyGroup::Memory
    }

    fn name(&self) -> &'static str {
        "op_i32_load_local_base_set4_i32_load_local_base"
    }

    fn matches(&self, ctx: &SelectionContext<'_>, cursor: usize) -> bool {
        emit_i32_load_local_base_set4_scalar_load_local_base(ctx, cursor).is_some()
    }

    fn cost(&self, ctx: &SelectionContext<'_>, _cursor: usize) -> i32 {
        72 + loop_bonus(ctx)
    }

    fn emit(&self, ctx: &SelectionContext<'_>, cursor: usize) -> Option<MatchResult> {
        emit_i32_load_local_base_set4_scalar_load_local_base(ctx, cursor)
    }
}

impl FamilySpec for ScalarLoadLocalBaseLocalGet4ScalarLoadSpec {
    fn group(&self) -> FamilyGroup {
        FamilyGroup::Memory
    }

    fn name(&self) -> &'static str {
        "op_i32_load16_local_base_local_get4_i32_load16"
    }

    fn matches(&self, ctx: &SelectionContext<'_>, cursor: usize) -> bool {
        emit_scalar_load_local_base_local_get4_scalar_load(ctx, cursor).is_some()
    }

    fn cost(&self, ctx: &SelectionContext<'_>, _cursor: usize) -> i32 {
        76 + loop_bonus(ctx)
    }

    fn emit(&self, ctx: &SelectionContext<'_>, cursor: usize) -> Option<MatchResult> {
        emit_scalar_load_local_base_local_get4_scalar_load(ctx, cursor)
    }
}

impl FamilySpec for LocalGet4I32LoadLocalBaseAddSet4Spec {
    fn group(&self) -> FamilyGroup {
        FamilyGroup::Memory
    }

    fn name(&self) -> &'static str {
        "op_local_get4_i32_load_local_base_i32_add_set4"
    }

    fn matches(&self, ctx: &SelectionContext<'_>, cursor: usize) -> bool {
        emit_local_get4_i32_load_local_base_add_set4(ctx, cursor).is_some()
    }

    fn cost(&self, ctx: &SelectionContext<'_>, _cursor: usize) -> i32 {
        56 + loop_bonus(ctx)
    }

    fn emit(&self, ctx: &SelectionContext<'_>, cursor: usize) -> Option<MatchResult> {
        emit_local_get4_i32_load_local_base_add_set4(ctx, cursor)
    }
}

impl FamilySpec for ScalarLoadLocalBaseSet4Spec {
    fn group(&self) -> FamilyGroup {
        FamilyGroup::Memory
    }

    fn name(&self) -> &'static str {
        "memory.local_base_load_set4"
    }

    fn matches(&self, ctx: &SelectionContext<'_>, cursor: usize) -> bool {
        emit_scalar_load_local_base_set4(ctx, cursor).is_some()
    }

    fn cost(&self, ctx: &SelectionContext<'_>, _cursor: usize) -> i32 {
        40 + loop_bonus(ctx)
    }

    fn emit(&self, ctx: &SelectionContext<'_>, cursor: usize) -> Option<MatchResult> {
        emit_scalar_load_local_base_set4(ctx, cursor)
    }
}

impl FamilySpec for ScalarLoadLocalBaseLocalGet4Spec {
    fn group(&self) -> FamilyGroup {
        FamilyGroup::Memory
    }

    fn name(&self) -> &'static str {
        "memory.local_base_load_local_get4"
    }

    fn matches(&self, ctx: &SelectionContext<'_>, cursor: usize) -> bool {
        emit_scalar_load_local_base_local_get4(ctx, cursor).is_some()
    }

    fn cost(&self, ctx: &SelectionContext<'_>, _cursor: usize) -> i32 {
        44 + loop_bonus(ctx)
    }

    fn emit(&self, ctx: &SelectionContext<'_>, cursor: usize) -> Option<MatchResult> {
        emit_scalar_load_local_base_local_get4(ctx, cursor)
    }
}

impl FamilySpec for ScalarLoadLocalGet4Spec {
    fn group(&self) -> FamilyGroup {
        FamilyGroup::Memory
    }

    fn name(&self) -> &'static str {
        "memory.load_local_get4"
    }

    fn matches(&self, ctx: &SelectionContext<'_>, cursor: usize) -> bool {
        emit_scalar_load_local_get4(ctx, cursor).is_some()
    }

    fn cost(&self, ctx: &SelectionContext<'_>, _cursor: usize) -> i32 {
        24 + loop_bonus(ctx)
    }

    fn emit(&self, ctx: &SelectionContext<'_>, cursor: usize) -> Option<MatchResult> {
        emit_scalar_load_local_get4(ctx, cursor)
    }
}

impl FamilySpec for ScalarLoadLocalBaseSpec {
    fn group(&self) -> FamilyGroup {
        FamilyGroup::Memory
    }

    fn name(&self) -> &'static str {
        "memory.local_base"
    }

    fn matches(&self, ctx: &SelectionContext<'_>, cursor: usize) -> bool {
        emit_scalar_load_local_base(ctx, cursor).is_some()
    }

    fn cost(&self, ctx: &SelectionContext<'_>, _cursor: usize) -> i32 {
        26 + loop_bonus(ctx)
    }

    fn emit(&self, ctx: &SelectionContext<'_>, cursor: usize) -> Option<MatchResult> {
        emit_scalar_load_local_base(ctx, cursor).map(|(result, _)| result)
    }
}

impl FamilySpec for ScalarStoreLocalBaseSpec {
    fn group(&self) -> FamilyGroup {
        FamilyGroup::Memory
    }

    fn name(&self) -> &'static str {
        "memory.local_base"
    }

    fn matches(&self, ctx: &SelectionContext<'_>, cursor: usize) -> bool {
        emit_scalar_store_local_base(ctx, cursor).is_some()
    }

    fn cost(&self, ctx: &SelectionContext<'_>, _cursor: usize) -> i32 {
        30 + loop_bonus(ctx)
    }

    fn emit(&self, ctx: &SelectionContext<'_>, cursor: usize) -> Option<MatchResult> {
        emit_scalar_store_local_base(ctx, cursor)
    }
}

impl FamilySpec for ScalarLoadLocalScaledIndexSpec {
    fn group(&self) -> FamilyGroup {
        FamilyGroup::Memory
    }

    fn name(&self) -> &'static str {
        "memory.local_scaled_index"
    }

    fn matches(&self, ctx: &SelectionContext<'_>, cursor: usize) -> bool {
        emit_scalar_load_local_scaled_index(ctx, cursor).is_some()
    }

    fn cost(&self, ctx: &SelectionContext<'_>, _cursor: usize) -> i32 {
        32 + loop_bonus(ctx)
    }

    fn emit(&self, ctx: &SelectionContext<'_>, cursor: usize) -> Option<MatchResult> {
        emit_scalar_load_local_scaled_index(ctx, cursor).map(|(result, _)| result)
    }
}

impl FamilySpec for ScalarStoreLocalScaledIndexSpec {
    fn group(&self) -> FamilyGroup {
        FamilyGroup::Memory
    }

    fn name(&self) -> &'static str {
        "memory.local_scaled_index"
    }

    fn matches(&self, ctx: &SelectionContext<'_>, cursor: usize) -> bool {
        emit_scalar_store_local_scaled_index(ctx, cursor).is_some()
    }

    fn cost(&self, ctx: &SelectionContext<'_>, _cursor: usize) -> i32 {
        36 + loop_bonus(ctx)
    }

    fn emit(&self, ctx: &SelectionContext<'_>, cursor: usize) -> Option<MatchResult> {
        emit_scalar_store_local_scaled_index(ctx, cursor)
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

fn match_i32_update_store_load_address(
    block: &CanonBlock,
    cursor: usize,
) -> Option<UpdateStoreLoadAddressMatch> {
    let store_addr = match_i32_local_base_address(block, cursor)?;
    let load_cursor = cursor + store_addr.consumed;
    if let Some(cached_next_ptr) = raw_local_tee(block.insts.get(load_cursor)?, 4) {
        if let Some(load_addr) = match_i32_local_base_address(block, load_cursor + 1) {
            if same_raw_operand(&store_addr.base_local, &load_addr.base_local) {
                return Some(UpdateStoreLoadAddressMatch {
                    base_local: store_addr.base_local,
                    store_delta: store_addr.delta,
                    load_delta: load_addr.delta,
                    cached_next_ptr: Some(cached_next_ptr),
                    consumed: store_addr.consumed + 1 + load_addr.consumed,
                });
            }
        }
        let cached_get = raw_local_get(block.insts.get(load_cursor + 1)?, 4)?;
        if store_addr.consumed > 1 && same_raw_operand(&cached_next_ptr, &cached_get) {
            return Some(UpdateStoreLoadAddressMatch {
                base_local: store_addr.base_local,
                store_delta: store_addr.delta,
                load_delta: store_addr.delta,
                cached_next_ptr: Some(cached_next_ptr),
                consumed: store_addr.consumed + 2,
            });
        }
    }
    if let Some(load_addr) = match_i32_local_base_address(block, load_cursor) {
        if same_raw_operand(&store_addr.base_local, &load_addr.base_local) {
            return Some(UpdateStoreLoadAddressMatch {
                base_local: store_addr.base_local,
                store_delta: store_addr.delta,
                load_delta: load_addr.delta,
                cached_next_ptr: None,
                consumed: store_addr.consumed + load_addr.consumed,
            });
        }
    }

    None
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

#[derive(Debug, Clone)]
struct LocalBaseAddressMatch {
    base_local: LoweredOperand,
    delta: i32,
    consumed: usize,
}

#[derive(Debug, Clone)]
struct UpdateStoreLoadAddressMatch {
    base_local: LoweredOperand,
    store_delta: i32,
    load_delta: i32,
    cached_next_ptr: Option<LoweredOperand>,
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

fn emit_i32_load_store_local_base_reverse_loop(
    ctx: &SelectionContext<'_>,
    cursor: usize,
) -> Option<MatchResult> {
    let [prev_get, saved_set, cursor_get, prev_tee, load, cursor_set, addr_get, value_get, store, cond_get, branch] =
        ctx.block.insts.get(cursor..cursor + 11)?
    else {
        return None;
    };
    if !load.op_eq(vm::op_i32_load as Op)
        || !store.op_eq(vm::op_i32_store as Op)
        || !branch.op_eq(vm::op_br_if as Op)
        || branch_target(branch)? != ctx.block.id
    {
        return None;
    }
    let prev = raw_local_get(prev_get, 4)?;
    let saved = raw_local_set(saved_set, 4)?;
    let cursor_local = raw_local_get(cursor_get, 4)?;
    let tee_prev = raw_local_tee(prev_tee, 4)?;
    let set_cursor = raw_local_set(cursor_set, 4)?;
    let store_addr = raw_local_get(addr_get, 4)?;
    let store_value = raw_local_get(value_get, 4)?;
    let cond = raw_local_get(cond_get, 4)?;
    if !same_raw_operand(&prev, &tee_prev)
        || !same_raw_operand(&prev, &store_addr)
        || !same_raw_operand(&saved, &store_value)
        || !same_raw_operand(&cursor_local, &set_cursor)
        || !same_raw_operand(&cursor_local, &cond)
    {
        return None;
    }
    Some(MatchResult {
        group: FamilyGroup::Memory,
        cost: 300 + loop_bonus(ctx),
        consumed: 11,
        ops: vec![KernelOp {
            label: None,
            op: vm::op_i32_load_store_local_base_reverse_loop as Op,
            operands: vec![
                prev,
                saved,
                cursor_local,
                load.operands.first()?.clone(),
                store.operands.first()?.clone(),
            ],
            family: "op_i32_load_store_local_base_reverse_loop",
        }],
    })
}

fn emit_i32_load_store_local_base_relink_loop(
    ctx: &SelectionContext<'_>,
    cursor: usize,
) -> Option<MatchResult> {
    let [cursor_get, current_tee, load, cursor_set, store_addr_get, prev_get, store, current_get, prev_set, cond_get, branch] =
        ctx.block.insts.get(cursor..cursor + 11)?
    else {
        return None;
    };
    if !load.op_eq(vm::op_i32_load as Op)
        || !store.op_eq(vm::op_i32_store as Op)
        || !branch.op_eq(vm::op_br_if as Op)
        || branch_target(branch)? != ctx.block.id
    {
        return None;
    }
    let cursor_local = raw_local_get(cursor_get, 4)?;
    let current = raw_local_tee(current_tee, 4)?;
    let cursor_set = raw_local_set(cursor_set, 4)?;
    let store_addr = raw_local_get(store_addr_get, 4)?;
    let prev = raw_local_get(prev_get, 4)?;
    let current_get = raw_local_get(current_get, 4)?;
    let prev_set = raw_local_set(prev_set, 4)?;
    let cond = raw_local_get(cond_get, 4)?;
    if !same_raw_operand(&cursor_local, &cursor_set)
        || !same_raw_operand(&cursor_local, &cond)
        || !same_raw_operand(&current, &store_addr)
        || !same_raw_operand(&current, &current_get)
        || !same_raw_operand(&prev, &prev_set)
    {
        return None;
    }
    Some(MatchResult {
        group: FamilyGroup::Memory,
        cost: 280 + loop_bonus(ctx),
        consumed: 11,
        ops: vec![KernelOp {
            label: None,
            op: vm::op_i32_load_store_local_base_relink_loop as Op,
            operands: vec![
                cursor_local,
                current,
                prev,
                load.operands.first()?.clone(),
                store.operands.first()?.clone(),
            ],
            family: "op_i32_load_store_local_base_relink_loop",
        }],
    })
}

fn emit_i32_load16_u_update_store16_local_base_loop(
    ctx: &SelectionContext<'_>,
    cursor: usize,
) -> Option<MatchResult> {
    const SUBTRACT: u32 = 1;

    let addr = match_i32_update_store_load_address(ctx.block, cursor)?;
    let mut at = cursor + addr.consumed;
    let load = ctx.block.insts.get(at)?;
    if !load.op_eq(vm::op_i32_load16_u as Op) {
        return None;
    }
    at += 1;
    let scalar = raw_local_get(ctx.block.insts.get(at)?, 4)?;
    at += 1;
    let op = ctx.block.insts.get(at)?;
    let kind = if op.op_eq(vm::op_i32_add as Op) {
        0
    } else if op.op_eq(vm::op_i32_sub as Op) {
        SUBTRACT
    } else {
        return None;
    };
    at += 1;
    let store = ctx.block.insts.get(at)?;
    if !store.op_eq(vm::op_i32_store16 as Op) {
        return None;
    }
    at += 1;
    let ptr_update_consumed = if let Some(cached_next_ptr) = &addr.cached_next_ptr {
        let cached_get = raw_local_get(ctx.block.insts.get(at)?, 4)?;
        let ptr_set = raw_local_set(ctx.block.insts.get(at + 1)?, 4)?;
        if same_raw_operand(cached_next_ptr, &cached_get)
            && same_raw_operand(&addr.base_local, &ptr_set)
            && addr.store_delta == 2
        {
            2
        } else {
            let ptr_update = match_i32_local_add_update(ctx, at)?;
            if !same_raw_operand(&addr.base_local, &ptr_update.target)
                || !matches!(ptr_update.delta, LoopDelta::Const(2))
            {
                return None;
            }
            4
        }
    } else {
        let ptr_update = match_i32_local_add_update(ctx, at)?;
        if !same_raw_operand(&addr.base_local, &ptr_update.target)
            || !matches!(ptr_update.delta, LoopDelta::Const(2))
        {
            return None;
        }
        4
    };
    at += ptr_update_consumed;
    let counter_update = match_i32_local_add_tee_update(ctx.block, at)?;
    at += 4;
    let branch = ctx.block.insts.get(at)?;
    if !branch.op_eq(vm::op_br_if as Op)
        || branch_target(branch)? != ctx.block.id
        || !matches!(counter_update.delta, LoopDelta::Const(-1))
        || same_raw_operand(&addr.base_local, &scalar)
        || same_raw_operand(&addr.base_local, &counter_update.target)
        || same_raw_operand(&scalar, &counter_update.target)
    {
        return None;
    }

    Some(MatchResult {
        group: FamilyGroup::Memory,
        cost: 260 + loop_bonus(ctx),
        consumed: at + 1 - cursor,
        ops: vec![KernelOp {
            label: None,
            op: vm::op_i32_load16_u_update_store16_local_base_loop as Op,
            operands: vec![
                raw_u32_operand(kind),
                addr.base_local,
                scalar,
                counter_update.target,
                raw_i32_operand(addr.load_delta),
                raw_i32_operand(addr.store_delta),
                load.operands.first()?.clone(),
                store.operands.first()?.clone(),
            ],
            family: "op_i32_load16_u_update_store16_local_base_loop",
        }],
    })
}

#[derive(Debug, Clone)]
struct CopyLocalBaseLane {
    dst_base: LoweredOperand,
    src_base: LoweredOperand,
    dst_delta: i32,
    src_delta: i32,
    dst_effective: i64,
    src_effective: i64,
    width: u32,
    load_memarg: LoweredOperand,
    store_memarg: LoweredOperand,
    consumed: usize,
}

fn emit_scalar_copy_local_base_run(
    ctx: &SelectionContext<'_>,
    cursor: usize,
) -> Option<MatchResult> {
    const MAX_COPY_RUN_LANES: usize = 16;

    let first = match_scalar_copy_local_base_lane(ctx.block, cursor)?;
    let mut lanes = vec![first];
    let mut consumed = lanes[0].consumed;
    while lanes.len() < MAX_COPY_RUN_LANES {
        let Some(next) = match_scalar_copy_local_base_lane(ctx.block, cursor + consumed) else {
            break;
        };
        let index = i64::try_from(lanes.len()).ok()?;
        let width = i64::from(lanes[0].width);
        if next.width != lanes[0].width
            || !same_raw_operand(&next.dst_base, &lanes[0].dst_base)
            || !same_raw_operand(&next.src_base, &lanes[0].src_base)
            || next.dst_effective != lanes[0].dst_effective + width * index
            || next.src_effective != lanes[0].src_effective + width * index
        {
            break;
        }
        consumed += next.consumed;
        lanes.push(next);
    }
    if lanes.len() < 2 {
        return None;
    }

    let mut operands = vec![
        raw_u32_operand(lanes[0].width | (u32::try_from(lanes.len()).ok()? << 8)),
        lanes[0].dst_base.clone(),
        lanes[0].src_base.clone(),
    ];
    for lane in &lanes {
        operands.push(raw_i32_operand(lane.dst_delta));
        operands.push(raw_i32_operand(lane.src_delta));
        operands.push(lane.load_memarg.clone());
        operands.push(lane.store_memarg.clone());
    }

    Some(MatchResult {
        group: FamilyGroup::Memory,
        cost: 42 + i32::try_from(lanes.len()).ok()? * 18 + loop_bonus(ctx),
        consumed,
        ops: vec![KernelOp {
            label: None,
            op: vm::op_scalar_copy_local_base_run as Op,
            operands,
            family: "op_scalar_copy_local_base_run",
        }],
    })
}

fn match_scalar_copy_local_base_lane(
    block: &CanonBlock,
    cursor: usize,
) -> Option<CopyLocalBaseLane> {
    let dst = match_i32_local_base_address(block, cursor)?;
    let src_cursor = cursor + dst.consumed;
    let src = match_i32_local_base_address(block, src_cursor)?;
    let load_cursor = src_cursor + src.consumed;
    let load = block.insts.get(load_cursor)?;
    let store = block.insts.get(load_cursor + 1)?;
    let width = scalar_copy_width(load.op, store.op)?;
    let load_memarg = load.operands.first()?.clone();
    let store_memarg = store.operands.first()?.clone();
    Some(CopyLocalBaseLane {
        dst_base: dst.base_local,
        src_base: src.base_local,
        dst_delta: dst.delta,
        src_delta: src.delta,
        dst_effective: effective_copy_offset(dst.delta, &store_memarg)?,
        src_effective: effective_copy_offset(src.delta, &load_memarg)?,
        width,
        load_memarg,
        store_memarg,
        consumed: dst.consumed + src.consumed + 2,
    })
}

fn effective_copy_offset(delta: i32, memarg: &LoweredOperand) -> Option<i64> {
    let memarg = raw_memarg(Some(memarg))?;
    Some(i64::from(delta) + i64::from(memarg.offset))
}

fn scalar_copy_width(load: Op, store: Op) -> Option<u32> {
    let load_width = scalar_copy_load_width(load)?;
    let store_width = scalar_copy_store_width(store)?;
    (load_width == store_width).then_some(load_width)
}

fn scalar_copy_load_width(op: Op) -> Option<u32> {
    if std::ptr::fn_addr_eq(op, vm::op_i32_load8_s as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i32_load8_u as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i64_load8_s as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i64_load8_u as Op)
    {
        Some(1)
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_load16_s as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i32_load16_u as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i64_load16_s as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i64_load16_u as Op)
    {
        Some(2)
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_load as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i64_load32_s as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i64_load32_u as Op)
        || std::ptr::fn_addr_eq(op, vm::op_f32_load as Op)
    {
        Some(4)
    } else if std::ptr::fn_addr_eq(op, vm::op_i64_load as Op)
        || std::ptr::fn_addr_eq(op, vm::op_f64_load as Op)
    {
        Some(8)
    } else {
        None
    }
}

fn scalar_copy_store_width(op: Op) -> Option<u32> {
    if std::ptr::fn_addr_eq(op, vm::op_i32_store8 as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i64_store8 as Op)
    {
        Some(1)
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_store16 as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i64_store16 as Op)
    {
        Some(2)
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_store as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i64_store32 as Op)
        || std::ptr::fn_addr_eq(op, vm::op_f32_store as Op)
    {
        Some(4)
    } else if std::ptr::fn_addr_eq(op, vm::op_i64_store as Op)
        || std::ptr::fn_addr_eq(op, vm::op_f64_store as Op)
    {
        Some(8)
    } else {
        None
    }
}

fn emit_i32_sum_clip_local_base_loop(
    ctx: &SelectionContext<'_>,
    cursor: usize,
) -> Option<MatchResult> {
    let zero = ctx.block.insts.get(cursor)?;
    if raw_i32(Some(&const_operand_for_kind(
        zero,
        LocalFastConstKind::I32,
    )?))?
        != 0
    {
        return None;
    }

    let load_addr = match_i32_local_base_address(ctx.block, cursor + 1)?;
    let mut at = cursor + 1 + load_addr.consumed;
    let load = ctx.block.insts.get(at)?;
    if !load.op_eq(vm::op_i32_load as Op) {
        return None;
    }
    at += 1;
    let value = raw_local_tee(ctx.block.insts.get(at)?, 4)?;
    at += 1;
    let acc_get = raw_local_get(ctx.block.insts.get(at)?, 4)?;
    at += 1;
    if !ctx.block.insts.get(at)?.op_eq(vm::op_i32_add as Op) {
        return None;
    }
    at += 1;
    let acc_tee = raw_local_tee(ctx.block.insts.get(at)?, 4)?;
    at += 1;
    let acc_cmp_get = raw_local_get(ctx.block.insts.get(at)?, 4)?;
    at += 1;
    let clip = raw_local_get(ctx.block.insts.get(at)?, 4)?;
    at += 1;
    if !ctx.block.insts.get(at)?.op_eq(vm::op_i32_gt_s as Op) {
        return None;
    }
    at += 1;
    let overflow = raw_local_tee(ctx.block.insts.get(at)?, 4)?;
    at += 1;
    if !is_select4_inst(ctx.block.insts.get(at)?) {
        return None;
    }
    at += 1;
    let acc_set = raw_local_set(ctx.block.insts.get(at)?, 4)?;
    at += 1;
    if raw_i32(Some(&const_operand_for_kind(
        ctx.block.insts.get(at)?,
        LocalFastConstKind::I32,
    )?))?
        != 10
    {
        return None;
    }
    at += 1;
    let value_get = raw_local_get(ctx.block.insts.get(at)?, 4)?;
    at += 1;
    let prev = raw_local_get(ctx.block.insts.get(at)?, 4)?;
    at += 1;
    if !ctx.block.insts.get(at)?.op_eq(vm::op_i32_gt_s as Op) {
        return None;
    }
    at += 1;
    let overflow_get = raw_local_get(ctx.block.insts.get(at)?, 4)?;
    at += 1;
    if !is_select4_inst(ctx.block.insts.get(at)?) {
        return None;
    }
    at += 1;
    let tally_get = raw_local_get(ctx.block.insts.get(at)?, 4)?;
    at += 1;
    if !ctx.block.insts.get(at)?.op_eq(vm::op_i32_add as Op) {
        return None;
    }
    at += 1;
    let tally_set = raw_local_set(ctx.block.insts.get(at)?, 4)?;
    at += 1;
    let ptr_update = match_i32_local_add_update(ctx, at)?;
    at += 4;
    let value_get_for_prev = raw_local_get(ctx.block.insts.get(at)?, 4)?;
    at += 1;
    let prev_set = raw_local_set(ctx.block.insts.get(at)?, 4)?;
    at += 1;
    let counter_update = match_i32_local_add_tee_update(ctx.block, at)?;
    at += 4;
    let branch = ctx.block.insts.get(at)?;
    if !branch.op_eq(vm::op_br_if as Op)
        || branch_target(branch)? != ctx.block.id
        || !same_raw_operand(&load_addr.base_local, &ptr_update.target)
        || !matches!(ptr_update.delta, LoopDelta::Const(4))
        || !matches!(counter_update.delta, LoopDelta::Const(-1))
        || !same_raw_operand(&acc_get, &acc_tee)
        || !same_raw_operand(&acc_get, &acc_cmp_get)
        || !same_raw_operand(&acc_get, &acc_set)
        || !same_raw_operand(&value, &value_get)
        || !same_raw_operand(&value, &value_get_for_prev)
        || !same_raw_operand(&prev, &prev_set)
        || !same_raw_operand(&overflow, &overflow_get)
        || !same_raw_operand(&tally_get, &tally_set)
    {
        return None;
    }

    Some(MatchResult {
        group: FamilyGroup::Memory,
        cost: 240 + loop_bonus(ctx),
        consumed: at + 1 - cursor,
        ops: vec![KernelOp {
            label: None,
            op: vm::op_i32_sum_clip_local_base_loop as Op,
            operands: vec![
                load_addr.base_local,
                raw_i32_operand(load_addr.delta),
                value,
                acc_get,
                overflow,
                clip,
                tally_get,
                prev,
                counter_update.target,
                load.operands.first()?.clone(),
            ],
            family: "op_i32_sum_clip_local_base_loop",
        }],
    })
}

fn match_i32_local_add_tee_update(block: &CanonBlock, cursor: usize) -> Option<LoopLocalUpdate> {
    let lhs = block.insts.get(cursor)?;
    let rhs = block.insts.get(cursor + 1)?;
    if !block.insts.get(cursor + 2)?.op_eq(vm::op_i32_add as Op) {
        return None;
    }
    let target = raw_local_tee(block.insts.get(cursor + 3)?, 4)?;

    if let Some(lhs_local) = raw_local_get(lhs, 4) {
        if same_raw_operand(&lhs_local, &target) {
            return Some(LoopLocalUpdate {
                target,
                delta: match_loop_delta_operand(rhs)?,
            });
        }
    }
    if let Some(rhs_local) = raw_local_get(rhs, 4) {
        if same_raw_operand(&rhs_local, &target) {
            return Some(LoopLocalUpdate {
                target,
                delta: match_loop_delta_operand(lhs)?,
            });
        }
    }
    None
}

fn is_select4_inst(inst: &CanonInst) -> bool {
    inst.op_eq(vm::op_select4 as Op)
        || (inst.op_eq(vm::op_select as Op) && raw_select(inst.operands.first()) == Some(4))
}

fn emit_i32_load16_s_dot4_local_base_loop(
    ctx: &SelectionContext<'_>,
    cursor: usize,
) -> Option<MatchResult> {
    let inst = |offset: usize| ctx.block.insts.get(cursor + offset);
    let a_base = raw_local_get(inst(0)?, 4)?;
    let index = raw_local_get(inst(1)?, 4)?;
    if !inst(2)?.op_eq(vm::op_i32_add as Op) {
        return None;
    }
    let a_addr = raw_local_tee(inst(3)?, 4)?;
    if raw_i32(Some(&const_operand_for_kind(
        inst(4)?,
        LocalFastConstKind::I32,
    )?))?
        != 6
        || !inst(5)?.op_eq(vm::op_i32_add as Op)
    {
        return None;
    }
    let a6_load = inst(6)?;
    let b_base = raw_local_get(inst(7)?, 4)?;
    let index_again = raw_local_get(inst(8)?, 4)?;
    if !same_raw_operand(&index, &index_again) || !inst(9)?.op_eq(vm::op_i32_add as Op) {
        return None;
    }
    let b_addr = raw_local_tee(inst(10)?, 4)?;
    if raw_i32(Some(&const_operand_for_kind(
        inst(11)?,
        LocalFastConstKind::I32,
    )?))?
        != 6
        || !inst(12)?.op_eq(vm::op_i32_add as Op)
    {
        return None;
    }
    let b6_load = inst(13)?;
    if !a6_load.op_eq(vm::op_i32_load16_s as Op)
        || !b6_load.op_eq(vm::op_i32_load16_s as Op)
        || !inst(14)?.op_eq(vm::op_i32_mul as Op)
    {
        return None;
    }

    let a4_addr = raw_local_get(inst(15)?, 4)?;
    if !same_raw_operand(&a_addr, &a4_addr)
        || raw_i32(Some(&const_operand_for_kind(
            inst(16)?,
            LocalFastConstKind::I32,
        )?))?
            != 4
        || !inst(17)?.op_eq(vm::op_i32_add as Op)
    {
        return None;
    }
    let a4_load = inst(18)?;
    let b4_addr = raw_local_get(inst(19)?, 4)?;
    if !same_raw_operand(&b_addr, &b4_addr)
        || raw_i32(Some(&const_operand_for_kind(
            inst(20)?,
            LocalFastConstKind::I32,
        )?))?
            != 4
        || !inst(21)?.op_eq(vm::op_i32_add as Op)
    {
        return None;
    }
    let b4_load = inst(22)?;
    if !a4_load.op_eq(vm::op_i32_load16_s as Op)
        || !b4_load.op_eq(vm::op_i32_load16_s as Op)
        || !inst(23)?.op_eq(vm::op_i32_mul as Op)
    {
        return None;
    }

    let a2_addr = raw_local_get(inst(24)?, 4)?;
    if !same_raw_operand(&a_addr, &a2_addr)
        || raw_i32(Some(&const_operand_for_kind(
            inst(25)?,
            LocalFastConstKind::I32,
        )?))?
            != 2
        || !inst(26)?.op_eq(vm::op_i32_add as Op)
    {
        return None;
    }
    let a2_load = inst(27)?;
    let b2_addr = raw_local_get(inst(28)?, 4)?;
    if !same_raw_operand(&b_addr, &b2_addr)
        || raw_i32(Some(&const_operand_for_kind(
            inst(29)?,
            LocalFastConstKind::I32,
        )?))?
            != 2
        || !inst(30)?.op_eq(vm::op_i32_add as Op)
    {
        return None;
    }
    let b2_load = inst(31)?;
    if !a2_load.op_eq(vm::op_i32_load16_s as Op)
        || !b2_load.op_eq(vm::op_i32_load16_s as Op)
        || !inst(32)?.op_eq(vm::op_i32_mul as Op)
    {
        return None;
    }

    let a0_addr = raw_local_get(inst(33)?, 4)?;
    let a0_load = inst(34)?;
    let b0_addr = raw_local_get(inst(35)?, 4)?;
    let b0_load = inst(36)?;
    if !same_raw_operand(&a_addr, &a0_addr)
        || !same_raw_operand(&b_addr, &b0_addr)
        || !a0_load.op_eq(vm::op_i32_load16_s as Op)
        || !b0_load.op_eq(vm::op_i32_load16_s as Op)
        || !inst(37)?.op_eq(vm::op_i32_mul as Op)
    {
        return None;
    }
    let acc = raw_local_get(inst(38)?, 4)?;
    for offset in 39..=42 {
        if !inst(offset)?.op_eq(vm::op_i32_add as Op) {
            return None;
        }
    }
    let acc_set = raw_local_set(inst(43)?, 4)?;
    if !same_raw_operand(&acc, &acc_set) {
        return None;
    }
    let index_update = raw_local_get(inst(44)?, 4)?;
    if !same_raw_operand(&index, &index_update)
        || raw_i32(Some(&const_operand_for_kind(
            inst(45)?,
            LocalFastConstKind::I32,
        )?))?
            != 8
        || !inst(46)?.op_eq(vm::op_i32_add as Op)
    {
        return None;
    }
    let index_set = raw_local_set(inst(47)?, 4)?;
    if !same_raw_operand(&index, &index_set) {
        return None;
    }
    let limit = raw_local_get(inst(48)?, 4)?;
    let counter = raw_local_get(inst(49)?, 4)?;
    if raw_i32(Some(&const_operand_for_kind(
        inst(50)?,
        LocalFastConstKind::I32,
    )?))?
        != 4
        || !inst(51)?.op_eq(vm::op_i32_add as Op)
    {
        return None;
    }
    let counter_tee = raw_local_tee(inst(52)?, 4)?;
    if !same_raw_operand(&counter, &counter_tee) || !inst(53)?.op_eq(vm::op_i32_ne as Op) {
        return None;
    }
    let branch = inst(54)?;
    if !branch.op_eq(vm::op_br_if as Op) {
        return None;
    }

    Some(MatchResult {
        group: FamilyGroup::Memory,
        cost: 460 + loop_bonus(ctx),
        consumed: 55,
        ops: vec![KernelOp {
            label: None,
            op: vm::op_i32_load16_s_dot4_local_base_loop as Op,
            operands: vec![
                a_base,
                index,
                a_addr,
                b_base,
                b_addr,
                acc,
                limit,
                counter,
                a6_load.operands.first()?.clone(),
                b6_load.operands.first()?.clone(),
                a4_load.operands.first()?.clone(),
                b4_load.operands.first()?.clone(),
                a2_load.operands.first()?.clone(),
                b2_load.operands.first()?.clone(),
                a0_load.operands.first()?.clone(),
                b0_load.operands.first()?.clone(),
                LoweredOperand::JumpTarget(branch_target(branch)?),
            ],
            family: "op_i32_load16_s_dot4_local_base_loop",
        }],
    })
}

fn emit_i32_load16_s_mul_add_local_base_loop(
    ctx: &SelectionContext<'_>,
    cursor: usize,
) -> Option<MatchResult> {
    let inst = |offset: usize| ctx.block.insts.get(cursor + offset);
    let a = raw_local_get(inst(0)?, 4)?;
    let a_load = inst(1)?;
    let b = raw_local_get(inst(2)?, 4)?;
    let b_load = inst(3)?;
    if !a_load.op_eq(vm::op_i32_load16_s as Op)
        || !b_load.op_eq(vm::op_i32_load16_s as Op)
        || !inst(4)?.op_eq(vm::op_i32_mul as Op)
    {
        return None;
    }

    let acc = raw_local_get(inst(5)?, 4)?;
    let acc_set = raw_local_set(inst(7)?, 4)?;
    if !inst(6)?.op_eq(vm::op_i32_add as Op) || !same_raw_operand(&acc, &acc_set) {
        return None;
    }

    let a_update = raw_local_get(inst(8)?, 4)?;
    let a_delta = raw_i32(Some(&const_operand_for_kind(
        inst(9)?,
        LocalFastConstKind::I32,
    )?))?;
    let a_set = raw_local_set(inst(11)?, 4)?;
    if !same_raw_operand(&a, &a_update)
        || !inst(10)?.op_eq(vm::op_i32_add as Op)
        || !same_raw_operand(&a, &a_set)
    {
        return None;
    }

    let b_update = raw_local_get(inst(12)?, 4)?;
    let b_delta = raw_i32(Some(&const_operand_for_kind(
        inst(13)?,
        LocalFastConstKind::I32,
    )?))?;
    let b_set = raw_local_set(inst(15)?, 4)?;
    if !same_raw_operand(&b, &b_update)
        || !inst(14)?.op_eq(vm::op_i32_add as Op)
        || !same_raw_operand(&b, &b_set)
    {
        return None;
    }

    let counter = raw_local_get(inst(16)?, 4)?;
    if raw_i32(Some(&const_operand_for_kind(
        inst(17)?,
        LocalFastConstKind::I32,
    )?))?
        != -1
        || !inst(18)?.op_eq(vm::op_i32_add as Op)
    {
        return None;
    }
    let counter_tee = raw_local_tee(inst(19)?, 4)?;
    let branch = inst(20)?;
    if !same_raw_operand(&counter, &counter_tee) || !branch.op_eq(vm::op_br_if as Op) {
        return None;
    }

    Some(MatchResult {
        group: FamilyGroup::Memory,
        cost: 170 + loop_bonus(ctx),
        consumed: 21,
        ops: vec![KernelOp {
            label: None,
            op: vm::op_i32_load16_s_mul_add_local_base_loop as Op,
            operands: vec![
                a,
                b,
                acc,
                counter,
                raw_i32_operand(a_delta),
                raw_i32_operand(b_delta),
                a_load.operands.first()?.clone(),
                b_load.operands.first()?.clone(),
                LoweredOperand::JumpTarget(branch_target(branch)?),
            ],
            family: "op_i32_load16_s_mul_add_local_base_loop",
        }],
    })
}

fn emit_i32_load16_u_bitmix_acc_local_base_delta_loop(
    ctx: &SelectionContext<'_>,
    cursor: usize,
) -> Option<MatchResult> {
    const FIRST_UPDATE_IS_B: u32 = 1;
    const A_DELTA_IS_LOCAL: u32 = 1 << 1;
    const B_DELTA_IS_LOCAL: u32 = 1 << 2;

    let inst = |offset: usize| ctx.block.insts.get(cursor + offset);
    let acc = raw_local_get(inst(0)?, 4)?;
    let a = raw_local_get(inst(1)?, 4)?;
    let a_load = inst(2)?;
    let b = raw_local_get(inst(3)?, 4)?;
    let b_load = inst(4)?;
    let product_tee = raw_local_tee(inst(6)?, 4)?;
    if same_raw_operand(&a, &b)
        || same_raw_operand(&acc, &a)
        || same_raw_operand(&acc, &b)
        || !a_load.op_eq(vm::op_i32_load16_u as Op)
        || !b_load.op_eq(vm::op_i32_load16_u as Op)
        || !inst(5)?.op_eq(vm::op_i32_mul as Op)
        || !same_raw_operand(&acc, &product_tee)
    {
        return None;
    }

    if i32_const_value(inst(7)?)? != 2
        || !inst(8)?.op_eq(vm::op_i32_shr_u as Op)
        || i32_const_value(inst(9)?)? != 15
        || !inst(10)?.op_eq(vm::op_i32_and as Op)
    {
        return None;
    }
    let product_get = raw_local_get(inst(11)?, 4)?;
    if !same_raw_operand(&acc, &product_get)
        || i32_const_value(inst(12)?)? != 5
        || !inst(13)?.op_eq(vm::op_i32_shr_u as Op)
        || i32_const_value(inst(14)?)? != 127
        || !inst(15)?.op_eq(vm::op_i32_and as Op)
        || !inst(16)?.op_eq(vm::op_i32_mul as Op)
        || !inst(17)?.op_eq(vm::op_i32_add as Op)
    {
        return None;
    }
    let acc_set = raw_local_set(inst(18)?, 4)?;
    if !same_raw_operand(&acc, &acc_set) {
        return None;
    }

    let first_update = match_i32_local_add_update(ctx, cursor + 19)?;
    let second_update = match_i32_local_add_update(ctx, cursor + 23)?;
    let first_update_is_b = if same_raw_operand(&first_update.target, &a)
        && same_raw_operand(&second_update.target, &b)
    {
        false
    } else if same_raw_operand(&first_update.target, &b)
        && same_raw_operand(&second_update.target, &a)
    {
        true
    } else {
        return None;
    };

    let (a_delta, b_delta) = if first_update_is_b {
        (&second_update.delta, &first_update.delta)
    } else {
        (&first_update.delta, &second_update.delta)
    };
    let counter = raw_local_get(inst(27)?, 4)?;
    if same_raw_operand(&acc, &counter)
        || i32_const_value(inst(28)?)? != -1
        || !inst(29)?.op_eq(vm::op_i32_add as Op)
    {
        return None;
    }
    let counter_tee = raw_local_tee(inst(30)?, 4)?;
    let branch = inst(31)?;
    if !same_raw_operand(&counter, &counter_tee)
        || !branch.op_eq(vm::op_br_if as Op)
        || branch_target(branch)? != ctx.block.id
    {
        return None;
    }

    let mut kind = if first_update_is_b {
        FIRST_UPDATE_IS_B
    } else {
        0
    };
    if a_delta.is_local() {
        kind |= A_DELTA_IS_LOCAL;
    }
    if b_delta.is_local() {
        kind |= B_DELTA_IS_LOCAL;
    }

    Some(MatchResult {
        group: FamilyGroup::Memory,
        cost: 260 + loop_bonus(ctx),
        consumed: 32,
        ops: vec![KernelOp {
            label: None,
            op: vm::op_i32_load16_u_bitmix_acc_local_base_delta_loop as Op,
            operands: vec![
                raw_u32_operand(kind),
                a,
                b,
                acc,
                counter,
                a_delta.operand(),
                b_delta.operand(),
                a_load.operands.first()?.clone(),
                b_load.operands.first()?.clone(),
                LoweredOperand::JumpTarget(branch_target(branch)?),
            ],
            family: "op_i32_load16_u_bitmix_acc_local_base_delta_loop",
        }],
    })
}

enum LoopDelta {
    Const(i32),
    Local(LoweredOperand),
}

impl LoopDelta {
    fn is_local(&self) -> bool {
        matches!(self, Self::Local(_))
    }

    fn operand(&self) -> LoweredOperand {
        match self {
            Self::Const(value) => raw_i32_operand(*value),
            Self::Local(local) => local.clone(),
        }
    }
}

struct LoopLocalUpdate {
    target: LoweredOperand,
    delta: LoopDelta,
}

fn emit_i32_load16_s_mul_add_local_base_delta_loop(
    ctx: &SelectionContext<'_>,
    cursor: usize,
) -> Option<MatchResult> {
    const FIRST_UPDATE_IS_B: u32 = 1;
    const A_DELTA_IS_LOCAL: u32 = 1 << 1;
    const B_DELTA_IS_LOCAL: u32 = 1 << 2;

    let inst = |offset: usize| ctx.block.insts.get(cursor + offset);
    let a = raw_local_get(inst(0)?, 4)?;
    let a_load = inst(1)?;
    let b = raw_local_get(inst(2)?, 4)?;
    let b_load = inst(3)?;
    if same_raw_operand(&a, &b)
        || !a_load.op_eq(vm::op_i32_load16_s as Op)
        || !b_load.op_eq(vm::op_i32_load16_s as Op)
        || !inst(4)?.op_eq(vm::op_i32_mul as Op)
    {
        return None;
    }

    let acc = raw_local_get(inst(5)?, 4)?;
    let acc_set = raw_local_set(inst(7)?, 4)?;
    if !inst(6)?.op_eq(vm::op_i32_add as Op) || !same_raw_operand(&acc, &acc_set) {
        return None;
    }

    let first_update = match_i32_local_add_update(ctx, cursor + 8)?;
    let second_update = match_i32_local_add_update(ctx, cursor + 12)?;
    let first_update_is_b = if same_raw_operand(&first_update.target, &a)
        && same_raw_operand(&second_update.target, &b)
    {
        false
    } else if same_raw_operand(&first_update.target, &b)
        && same_raw_operand(&second_update.target, &a)
    {
        true
    } else {
        return None;
    };

    let (a_delta, b_delta) = if first_update_is_b {
        (&second_update.delta, &first_update.delta)
    } else {
        (&first_update.delta, &second_update.delta)
    };
    if !a_delta.is_local() && !b_delta.is_local() {
        return None;
    }

    let counter = raw_local_get(inst(16)?, 4)?;
    if raw_i32(Some(&const_operand_for_kind(
        inst(17)?,
        LocalFastConstKind::I32,
    )?))?
        != -1
        || !inst(18)?.op_eq(vm::op_i32_add as Op)
    {
        return None;
    }
    let counter_tee = raw_local_tee(inst(19)?, 4)?;
    let branch = inst(20)?;
    if !same_raw_operand(&counter, &counter_tee) || !branch.op_eq(vm::op_br_if as Op) {
        return None;
    }

    let mut kind = if first_update_is_b {
        FIRST_UPDATE_IS_B
    } else {
        0
    };
    if a_delta.is_local() {
        kind |= A_DELTA_IS_LOCAL;
    }
    if b_delta.is_local() {
        kind |= B_DELTA_IS_LOCAL;
    }

    Some(MatchResult {
        group: FamilyGroup::Memory,
        cost: 176 + loop_bonus(ctx),
        consumed: 21,
        ops: vec![KernelOp {
            label: None,
            op: vm::op_i32_load16_s_mul_add_local_base_delta_loop as Op,
            operands: vec![
                raw_u32_operand(kind),
                a,
                b,
                acc,
                counter,
                a_delta.operand(),
                b_delta.operand(),
                a_load.operands.first()?.clone(),
                b_load.operands.first()?.clone(),
                LoweredOperand::JumpTarget(branch_target(branch)?),
            ],
            family: "op_i32_load16_s_mul_add_local_base_delta_loop",
        }],
    })
}

fn match_i32_local_add_update(
    ctx: &SelectionContext<'_>,
    cursor: usize,
) -> Option<LoopLocalUpdate> {
    let lhs = ctx.block.insts.get(cursor)?;
    let rhs = ctx.block.insts.get(cursor + 1)?;
    if !ctx.block.insts.get(cursor + 2)?.op_eq(vm::op_i32_add as Op) {
        return None;
    }
    let target = raw_local_set(ctx.block.insts.get(cursor + 3)?, 4)?;

    if let Some(lhs_local) = raw_local_get(lhs, 4) {
        if same_raw_operand(&lhs_local, &target) {
            return Some(LoopLocalUpdate {
                target,
                delta: match_loop_delta_operand(rhs)?,
            });
        }
    }
    if let Some(rhs_local) = raw_local_get(rhs, 4) {
        if same_raw_operand(&rhs_local, &target) {
            return Some(LoopLocalUpdate {
                target,
                delta: match_loop_delta_operand(lhs)?,
            });
        }
    }
    None
}

fn match_loop_delta_operand(inst: &CanonInst) -> Option<LoopDelta> {
    if let Some(value) = const_operand_for_kind(inst, LocalFastConstKind::I32) {
        return Some(LoopDelta::Const(raw_i32(Some(&value))?));
    }
    raw_local_get(inst, 4).map(LoopDelta::Local)
}

#[derive(Debug, Clone)]
struct I32IncLocalBaseMatch {
    base_local: LoweredOperand,
    store_delta: i32,
    load_delta: i32,
    load_memarg: LoweredOperand,
    store_memarg: LoweredOperand,
    consumed: usize,
}

#[derive(Debug, Clone)]
struct I32Load8ULocalBaseSet4Match {
    base_local: LoweredOperand,
    delta: i32,
    memarg: LoweredOperand,
    dst: LoweredOperand,
    consumed: usize,
}

#[derive(Debug, Clone)]
struct I32Load8UUpdateBrIfMatch {
    base_local: LoweredOperand,
    delta: i32,
    memarg: LoweredOperand,
    byte_dst: LoweredOperand,
    next_src: LoweredOperand,
    ptr_dst: LoweredOperand,
    branch_local: LoweredOperand,
    branch_target: usize,
    consumed: usize,
}

fn emit_i32_inc_local_base(ctx: &SelectionContext<'_>, cursor: usize) -> Option<MatchResult> {
    let matched = match_i32_inc_local_base(ctx.block, cursor)?;
    Some(MatchResult {
        group: FamilyGroup::Memory,
        cost: 82 + loop_bonus(ctx),
        consumed: matched.consumed,
        ops: vec![KernelOp {
            label: None,
            op: vm::op_i32_inc_local_base as Op,
            operands: vec![
                matched.base_local,
                raw_i32_operand(matched.store_delta),
                raw_i32_operand(matched.load_delta),
                matched.load_memarg,
                matched.store_memarg,
            ],
            family: "op_i32_inc_local_base",
        }],
    })
}

fn emit_local_get4_i32_inc_local_base(
    ctx: &SelectionContext<'_>,
    cursor: usize,
) -> Option<MatchResult> {
    let preserved = raw_local_get(ctx.block.insts.get(cursor)?, 4)?;
    let inc = match_i32_inc_local_base(ctx.block, cursor + 1)?;
    Some(MatchResult {
        group: FamilyGroup::Memory,
        cost: 112 + loop_bonus(ctx),
        consumed: 1 + inc.consumed,
        ops: vec![KernelOp {
            label: None,
            op: vm::op_local_get4_i32_inc_local_base as Op,
            operands: vec![
                preserved,
                inc.base_local,
                raw_i32_operand(inc.store_delta),
                raw_i32_operand(inc.load_delta),
                inc.load_memarg,
                inc.store_memarg,
            ],
            family: "op_local_get4_i32_inc_local_base",
        }],
    })
}

fn emit_local_get4_i32_inc_local_base_i32_load8_u_local_base_set4(
    ctx: &SelectionContext<'_>,
    cursor: usize,
) -> Option<MatchResult> {
    let preserved = raw_local_get(ctx.block.insts.get(cursor)?, 4)?;
    let inc = match_i32_inc_local_base(ctx.block, cursor + 1)?;
    let load_set_cursor = cursor + 1 + inc.consumed;
    let load_set = match_i32_load8_u_local_base_set4(ctx.block, load_set_cursor)?;
    Some(MatchResult {
        group: FamilyGroup::Memory,
        cost: 176 + loop_bonus(ctx),
        consumed: 1 + inc.consumed + load_set.consumed,
        ops: vec![KernelOp {
            label: None,
            op: vm::op_local_get4_i32_inc_local_base_i32_load8_u_local_base_set4 as Op,
            operands: vec![
                preserved,
                inc.base_local,
                raw_i32_operand(inc.store_delta),
                raw_i32_operand(inc.load_delta),
                inc.load_memarg,
                inc.store_memarg,
                load_set.base_local,
                raw_i32_operand(load_set.delta),
                load_set.memarg,
                load_set.dst,
            ],
            family: "op_local_get4_i32_inc_local_base_i32_load8_u_local_base_set4",
        }],
    })
}

fn emit_local_get4_i32_load8_u_local_base_set4(
    ctx: &SelectionContext<'_>,
    cursor: usize,
) -> Option<MatchResult> {
    let preserved = raw_local_get(ctx.block.insts.get(cursor)?, 4)?;
    let load_set = match_i32_load8_u_local_base_set4(ctx.block, cursor + 1)?;
    Some(MatchResult {
        group: FamilyGroup::Memory,
        cost: 96 + loop_bonus(ctx),
        consumed: 1 + load_set.consumed,
        ops: vec![KernelOp {
            label: None,
            op: vm::op_local_get4_i32_load8_u_local_base_set4 as Op,
            operands: vec![
                preserved,
                load_set.base_local,
                raw_i32_operand(load_set.delta),
                load_set.memarg,
                load_set.dst,
            ],
            family: "op_local_get4_i32_load8_u_local_base_set4",
        }],
    })
}

fn emit_i32_load8_u_local_base_set4_local_get4(
    ctx: &SelectionContext<'_>,
    cursor: usize,
) -> Option<MatchResult> {
    let load_set = match_i32_load8_u_local_base_set4(ctx.block, cursor)?;
    let get = raw_local_get(ctx.block.insts.get(cursor + load_set.consumed)?, 4)?;
    Some(MatchResult {
        group: FamilyGroup::Memory,
        cost: 96 + loop_bonus(ctx),
        consumed: load_set.consumed + 1,
        ops: vec![KernelOp {
            label: None,
            op: vm::op_i32_load8_u_local_base_set4_local_get4 as Op,
            operands: vec![
                load_set.base_local,
                raw_i32_operand(load_set.delta),
                load_set.memarg,
                load_set.dst,
                get,
            ],
            family: "op_i32_load8_u_local_base_set4_local_get4",
        }],
    })
}

fn emit_i32_inc_local_base_i32_load8_u_local_base_set4_local_get4_set4_local_get4_br_if(
    ctx: &SelectionContext<'_>,
    cursor: usize,
) -> Option<MatchResult> {
    let inc = match_i32_inc_local_base(ctx.block, cursor)?;
    let branch = match_i32_load8_u_local_base_set4_local_get4_set4_local_get4_br_if(
        ctx.block,
        cursor + inc.consumed,
    )?;
    Some(MatchResult {
        group: FamilyGroup::Memory,
        cost: 264 + loop_bonus(ctx),
        consumed: inc.consumed + branch.consumed,
        ops: vec![KernelOp {
            label: None,
            op: vm::op_i32_inc_local_base_i32_load8_u_local_base_set4_local_get4_set4_local_get4_br_if
                as Op,
            operands: vec![
                inc.base_local,
                raw_i32_operand(inc.store_delta),
                raw_i32_operand(inc.load_delta),
                inc.load_memarg,
                inc.store_memarg,
                branch.base_local,
                raw_i32_operand(branch.delta),
                branch.memarg,
                branch.byte_dst,
                branch.next_src,
                branch.ptr_dst,
                branch.branch_local,
                LoweredOperand::JumpTarget(branch.branch_target),
                raw_u32_operand(14),
            ],
            family: "op_i32_inc_local_base_i32_load8_u_local_base_set4_local_get4_set4_local_get4_br_if",
        }],
    })
}

fn match_i32_inc_local_base(block: &CanonBlock, cursor: usize) -> Option<I32IncLocalBaseMatch> {
    if let Some(inst) = block.insts.get(cursor) {
        if inst.op_eq(vm::op_i32_inc_local_base as Op) {
            return Some(I32IncLocalBaseMatch {
                base_local: inst.operands.first()?.clone(),
                store_delta: raw_i32(inst.operands.get(1))?,
                load_delta: raw_i32(inst.operands.get(2))?,
                load_memarg: inst.operands.get(3)?.clone(),
                store_memarg: inst.operands.get(4)?.clone(),
                consumed: 1,
            });
        }
    }

    let store_addr = match_i32_local_base_address(block, cursor)?;
    let load_addr_cursor = cursor + store_addr.consumed;
    let load_addr = match_i32_local_base_address(block, load_addr_cursor)?;
    if !same_raw_operand(&store_addr.base_local, &load_addr.base_local) {
        return None;
    }
    let load_cursor = load_addr_cursor + load_addr.consumed;
    let load = block.insts.get(load_cursor)?;
    let konst = block.insts.get(load_cursor + 1)?;
    let add = block.insts.get(load_cursor + 2)?;
    let store = block.insts.get(load_cursor + 3)?;
    if !load.op_eq(vm::op_i32_load as Op)
        || raw_i32(Some(&const_operand_for_kind(
            konst,
            LocalFastConstKind::I32,
        )?))?
            != 1
        || !add.op_eq(vm::op_i32_add as Op)
        || !store.op_eq(vm::op_i32_store as Op)
    {
        return None;
    }
    Some(I32IncLocalBaseMatch {
        base_local: store_addr.base_local,
        store_delta: store_addr.delta,
        load_delta: load_addr.delta,
        load_memarg: load.operands.first()?.clone(),
        store_memarg: store.operands.first()?.clone(),
        consumed: load_cursor + 4 - cursor,
    })
}

fn emit_i32_load8_u_local_base_set4_local_get4_set4_local_get4_br_if(
    ctx: &SelectionContext<'_>,
    cursor: usize,
) -> Option<MatchResult> {
    let matched =
        match_i32_load8_u_local_base_set4_local_get4_set4_local_get4_br_if(ctx.block, cursor)?;

    Some(MatchResult {
        group: FamilyGroup::Memory,
        cost: 184 + loop_bonus(ctx),
        consumed: matched.consumed,
        ops: vec![KernelOp {
            label: None,
            op: vm::op_i32_load8_u_local_base_set4_local_get4_set4_local_get4_br_if as Op,
            operands: vec![
                matched.base_local,
                raw_i32_operand(matched.delta),
                matched.memarg,
                matched.byte_dst,
                matched.next_src,
                matched.ptr_dst,
                matched.branch_local,
                LoweredOperand::JumpTarget(matched.branch_target),
            ],
            family: "op_i32_load8_u_local_base_set4_local_get4_set4_local_get4_br_if",
        }],
    })
}

fn match_i32_load8_u_local_base_set4_local_get4_set4_local_get4_br_if(
    block: &CanonBlock,
    cursor: usize,
) -> Option<I32Load8UUpdateBrIfMatch> {
    let load_addr = match_i32_local_base_address(block, cursor)?;
    let load = block.insts.get(cursor + load_addr.consumed)?;
    if !load.op_eq(vm::op_i32_load8_u as Op) {
        return None;
    }
    let byte_dst = raw_local_set(block.insts.get(cursor + load_addr.consumed + 1)?, 4)?;
    let next_src = raw_local_get(block.insts.get(cursor + load_addr.consumed + 2)?, 4)?;
    let ptr_dst = raw_local_set(block.insts.get(cursor + load_addr.consumed + 3)?, 4)?;
    let branch_local = raw_local_get(block.insts.get(cursor + load_addr.consumed + 4)?, 4)?;
    let branch = block.insts.get(cursor + load_addr.consumed + 5)?;
    if !branch.op_eq(vm::op_br_if as Op) {
        return None;
    }
    Some(I32Load8UUpdateBrIfMatch {
        base_local: load_addr.base_local,
        delta: load_addr.delta,
        memarg: load.operands.first()?.clone(),
        byte_dst,
        next_src,
        ptr_dst,
        branch_local,
        branch_target: branch_target(branch)?,
        consumed: load_addr.consumed + 6,
    })
}

fn match_i32_load8_u_local_base_set4(
    block: &CanonBlock,
    cursor: usize,
) -> Option<I32Load8ULocalBaseSet4Match> {
    if let Some(inst) = block.insts.get(cursor) {
        if inst.op_eq(vm::op_i32_load8_u_local_base_set4 as Op) {
            return Some(I32Load8ULocalBaseSet4Match {
                base_local: inst.operands.first()?.clone(),
                delta: raw_i32(inst.operands.get(1))?,
                memarg: inst.operands.get(2)?.clone(),
                dst: inst.operands.get(3)?.clone(),
                consumed: 1,
            });
        }
    }

    let matched = match_i32_local_base_address(block, cursor)?;
    let load_cursor = cursor + matched.consumed;
    let load = block.insts.get(load_cursor)?;
    let consumer = block.insts.get(load_cursor + 1)?;
    if !load.op_eq(vm::op_i32_load8_u as Op) {
        return None;
    }
    Some(I32Load8ULocalBaseSet4Match {
        base_local: matched.base_local,
        delta: matched.delta,
        memarg: load.operands.first()?.clone(),
        dst: raw_local_set(consumer, 4)?,
        consumed: matched.consumed + 2,
    })
}

fn emit_scalar_load_const_base(
    ctx: &SelectionContext<'_>,
    cursor: usize,
) -> Option<(MatchResult, ScalarType)> {
    let [konst, load] = ctx.block.insts.get(cursor..cursor + 2)? else {
        return None;
    };
    if !konst.op_eq(vm::op_i32_const as Op) {
        return None;
    }
    let scalar = scalar_memory_load_type_for_inst(load)?;
    let op = scalar_const_base_load_family_for_type(load.op, scalar)?;
    let folded = fold_const_base_memarg(konst.operands.first(), load.operands.first())?;
    Some((
        MatchResult {
            group: FamilyGroup::Memory,
            cost: 18 + loop_bonus(ctx),
            consumed: 2,
            ops: vec![KernelOp {
                label: None,
                op,
                operands: vec![LoweredOperand::Raw(unsafe { folded.encoded })],
                family: "memory.const_base_load",
            }],
        },
        scalar,
    ))
}

fn emit_scalar_store_const_base(
    ctx: &SelectionContext<'_>,
    cursor: usize,
) -> Option<(MatchResult, ScalarType)> {
    let [konst, local, store] = ctx.block.insts.get(cursor..cursor + 3)? else {
        return None;
    };
    if !konst.op_eq(vm::op_i32_const as Op) {
        return None;
    }
    let scalar = scalar_memory_store_type_for_inst(store)?;
    let op = scalar_const_base_store_family_for_type(store.op, scalar)?;
    let local_operand = raw_local_get(local, scalar.width())?;
    let folded = fold_const_base_memarg(konst.operands.first(), store.operands.first())?;
    Some((
        MatchResult {
            group: FamilyGroup::Memory,
            cost: 24 + loop_bonus(ctx),
            consumed: 3,
            ops: vec![KernelOp {
                label: None,
                op,
                operands: vec![
                    LoweredOperand::Raw(unsafe { folded.encoded }),
                    local_operand,
                ],
                family: "memory.const_base_store_local",
            }],
        },
        scalar,
    ))
}

fn emit_local_get4_local_get4_xor_tee4_u8_shl1_i32_load16_u(
    ctx: &SelectionContext<'_>,
    cursor: usize,
) -> Option<MatchResult> {
    let [lhs, rhs, xor, tee, mask, and, shift, shl, load] =
        ctx.block.insts.get(cursor..cursor + 9)?
    else {
        return None;
    };
    if !xor.op_eq(vm::op_i32_xor as Op)
        || !tee.op_eq(vm::op_local_tee4 as Op)
        || !and.op_eq(vm::op_i32_and as Op)
        || !shl.op_eq(vm::op_i32_shl as Op)
        || !load.op_eq(vm::op_i32_load16_u as Op)
    {
        return None;
    }
    if raw_i32(Some(&const_operand_for_kind(
        mask,
        LocalFastConstKind::I32,
    )?))?
        != 255
        || raw_i32(Some(&const_operand_for_kind(
            shift,
            LocalFastConstKind::I32,
        )?))?
            != 1
    {
        return None;
    }
    Some(MatchResult {
        group: FamilyGroup::Memory,
        cost: 104 + loop_bonus(ctx),
        consumed: 9,
        ops: vec![KernelOp {
            label: None,
            op: vm::op_local_get4_local_get4_i32_xor_tee4_u8_shl1_i32_load16_u as Op,
            operands: vec![
                raw_local_get(lhs, 4)?,
                raw_local_get(rhs, 4)?,
                raw_local_tee(tee, 4)?,
                load.operands.first()?.clone(),
            ],
            family: "op_local_get4_local_get4_i32_xor_tee4_u8_shl1_i32_load16_u",
        }],
    })
}

fn emit_i32_load_local_base_tee4_br_if(
    ctx: &SelectionContext<'_>,
    cursor: usize,
) -> Option<MatchResult> {
    let matched = match_i32_local_base_address(ctx.block, cursor)?;
    let load_cursor = cursor + matched.consumed;
    let load = ctx.block.insts.get(load_cursor)?;
    if scalar_memory_load_type_for_inst(load) != Some(ScalarType::I32) {
        return None;
    }
    let local_base_op = scalar_local_base_load_family_for_type(load.op, ScalarType::I32)?;
    let tee = ctx.block.insts.get(load_cursor + 1)?;
    let dst = raw_local_tee(tee, 4)?;
    let next = ctx.block.insts.get(load_cursor + 2)?;
    let (op, branch, consumed) = if next.op_eq(vm::op_br_if as Op) {
        (
            local_base_load_tee4_branch_family(local_base_op, false)?,
            next,
            matched.consumed + 3,
        )
    } else if next.op_eq(vm::op_i32_eqz as Op) {
        let branch = ctx.block.insts.get(load_cursor + 3)?;
        if !branch.op_eq(vm::op_br_if as Op) {
            return None;
        }
        (
            local_base_load_tee4_branch_family(local_base_op, true)?,
            branch,
            matched.consumed + 4,
        )
    } else {
        return None;
    };
    let taken = branch_target(branch)?;
    let mut operands = vec![matched.base_local, raw_i32_operand(matched.delta)];
    operands.extend(load.operands.clone());
    operands.push(dst);
    operands.push(LoweredOperand::JumpTarget(taken));
    Some(MatchResult {
        group: FamilyGroup::Memory,
        cost: 62 + loop_bonus(ctx),
        consumed,
        ops: vec![KernelOp {
            label: None,
            op,
            operands,
            family: "op_i32_load_local_base_tee4_br_if",
        }],
    })
}

fn emit_i32_load_tee4_br_if(ctx: &SelectionContext<'_>, cursor: usize) -> Option<MatchResult> {
    let load = ctx.block.insts.get(cursor)?;
    if scalar_memory_load_type_for_inst(load) != Some(ScalarType::I32) {
        return None;
    }
    let tee = ctx.block.insts.get(cursor + 1)?;
    let dst = raw_local_tee(tee, 4)?;
    let next = ctx.block.insts.get(cursor + 2)?;
    let (op, branch, consumed) = if next.op_eq(vm::op_br_if as Op) {
        (i32_load_tee4_branch_family(load.op, false)?, next, 3)
    } else if next.op_eq(vm::op_i32_eqz as Op) {
        let branch = ctx.block.insts.get(cursor + 3)?;
        if !branch.op_eq(vm::op_br_if as Op) {
            return None;
        }
        (i32_load_tee4_branch_family(load.op, true)?, branch, 4)
    } else {
        return None;
    };
    let taken = branch_target(branch)?;
    let mut operands = load.operands.clone();
    operands.push(dst);
    operands.push(LoweredOperand::JumpTarget(taken));
    Some(MatchResult {
        group: FamilyGroup::Memory,
        cost: 44 + loop_bonus(ctx),
        consumed,
        ops: vec![KernelOp {
            label: None,
            op,
            operands,
            family: "op_i32_load_tee4_br_if",
        }],
    })
}

fn emit_i32_load_store_local_base_local_get4(
    ctx: &SelectionContext<'_>,
    cursor: usize,
) -> Option<MatchResult> {
    let load = ctx.block.insts.get(cursor)?;
    let load_kind = i32_scalar_load_kind(load.op)?;
    let addr = match_i32_local_base_address(ctx.block, cursor + 1)?;
    let value_cursor = cursor + 1 + addr.consumed;
    let value = raw_local_get(ctx.block.insts.get(value_cursor)?, 4)?;
    let store = ctx.block.insts.get(value_cursor + 1)?;
    let store_kind = i32_scalar_store_kind(store.op)?;
    Some(MatchResult {
        group: FamilyGroup::Memory,
        cost: 64 + loop_bonus(ctx),
        consumed: 1 + addr.consumed + 2,
        ops: vec![KernelOp {
            label: None,
            op: vm::op_i32_load_store_local_base_local_get4 as Op,
            operands: {
                let mut operands = vec![
                    raw_u32_operand(load_kind | (store_kind << 8)),
                    load.operands.first()?.clone(),
                    addr.base_local,
                    raw_i32_operand(addr.delta),
                    value,
                ];
                operands.extend(store.operands.clone());
                operands.push(raw_u32_operand(7));
                operands
            },
            family: "op_i32_load_store_local_base_local_get4",
        }],
    })
}

fn emit_i32_load_local_base_local_get4_scalar_load_tee4_cmp_br_if(
    ctx: &SelectionContext<'_>,
    cursor: usize,
) -> Option<MatchResult> {
    let matched = match_i32_local_base_address(ctx.block, cursor)?;
    let first_load_cursor = cursor + matched.consumed;
    let first_load = ctx.block.insts.get(first_load_cursor)?;
    let first_kind = i32_scalar_load_kind(first_load.op)?;
    let first_dst = raw_local_tee(ctx.block.insts.get(first_load_cursor + 1)?, 4)?;
    let second_addr = raw_local_get(ctx.block.insts.get(first_load_cursor + 2)?, 4)?;
    let second_load = ctx.block.insts.get(first_load_cursor + 3)?;
    let second_kind = i32_scalar_load_kind(second_load.op)?;
    let second_dst = raw_local_tee(ctx.block.insts.get(first_load_cursor + 4)?, 4)?;
    let compare = ctx.block.insts.get(first_load_cursor + 5)?;
    let compare_kind = i32_compare_kind(compare.op)?;
    let branch = ctx.block.insts.get(first_load_cursor + 6)?;
    if !branch.op_eq(vm::op_br_if as Op) {
        return None;
    }

    let mut operands = vec![
        raw_u32_operand(first_kind | (second_kind << 8) | (compare_kind << 16)),
        matched.base_local,
        raw_i32_operand(matched.delta),
    ];
    operands.extend(first_load.operands.clone());
    operands.push(first_dst);
    operands.push(second_addr);
    operands.extend(second_load.operands.clone());
    operands.push(second_dst);
    operands.push(LoweredOperand::JumpTarget(branch_target(branch)?));
    Some(MatchResult {
        group: FamilyGroup::Memory,
        cost: 134 + loop_bonus(ctx),
        consumed: matched.consumed + 7,
        ops: vec![KernelOp {
            label: None,
            op: vm::op_i32_load_local_base_local_get4_i32_load_tee4_cmp_br_if as Op,
            operands,
            family: "op_i32_load_local_base_local_get4_i32_load_tee4_cmp_br_if",
        }],
    })
}

fn emit_local_get4_scalar_load_local_base(
    ctx: &SelectionContext<'_>,
    cursor: usize,
) -> Option<MatchResult> {
    let preserved_local = raw_local_get(ctx.block.insts.get(cursor)?, 4)?;
    let matched = match_i32_local_base_address(ctx.block, cursor + 1)?;
    let load = ctx.block.insts.get(cursor + 1 + matched.consumed)?;
    let op = local_get4_local_base_load_family(load.op)?;
    let mut operands = vec![
        preserved_local,
        matched.base_local,
        raw_i32_operand(matched.delta),
    ];
    operands.extend(load.operands.clone());
    Some(MatchResult {
        group: FamilyGroup::Memory,
        cost: 38 + loop_bonus(ctx),
        consumed: 1 + matched.consumed + 1,
        ops: vec![KernelOp {
            label: None,
            op,
            operands,
            family: "memory.local_get4_local_base_load",
        }],
    })
}

fn emit_i32_load_local_base_set4_i32_load16_u_local_base_local_eq_search_loop(
    ctx: &SelectionContext<'_>,
    cursor: usize,
) -> Option<MatchResult> {
    let first_addr = match_i32_local_base_address(ctx.block, cursor)?;
    let first_load_cursor = cursor + first_addr.consumed;
    let first_load = ctx.block.insts.get(first_load_cursor)?;
    if !first_load.op_eq(vm::op_i32_load as Op) {
        return None;
    }
    let set = ctx.block.insts.get(first_load_cursor + 1)?;
    let dst = raw_local_set(set, 4)?;

    let second_addr_cursor = first_load_cursor + 2;
    let second_addr = match_i32_local_base_address(ctx.block, second_addr_cursor)?;
    if !same_raw_operand(&second_addr.base_local, &dst) {
        return None;
    }
    let second_load_cursor = second_addr_cursor + second_addr.consumed;
    let second_load = ctx.block.insts.get(second_load_cursor)?;
    if !second_load.op_eq(vm::op_i32_load16_u as Op) {
        return None;
    }
    let rhs = raw_local_get(ctx.block.insts.get(second_load_cursor + 1)?, 4)?;
    let eq = ctx.block.insts.get(second_load_cursor + 2)?;
    let found_branch = ctx.block.insts.get(second_load_cursor + 3)?;
    if !eq.op_eq(vm::op_i32_eq as Op) || !found_branch.op_eq(vm::op_br_if as Op) {
        return None;
    }

    let next_addr_cursor = second_load_cursor + 4;
    let next_addr = match_i32_local_base_address(ctx.block, next_addr_cursor)?;
    if !same_raw_operand(&next_addr.base_local, &first_addr.base_local) {
        return None;
    }
    let next_load_cursor = next_addr_cursor + next_addr.consumed;
    let next_load = ctx.block.insts.get(next_load_cursor)?;
    if !next_load.op_eq(vm::op_i32_load as Op) {
        return None;
    }
    let tee = ctx.block.insts.get(next_load_cursor + 1)?;
    let tee_dst = raw_local_tee(tee, 4)?;
    if !same_raw_operand(&tee_dst, &first_addr.base_local) {
        return None;
    }
    let loop_branch = ctx.block.insts.get(next_load_cursor + 2)?;
    let miss_branch = ctx.block.insts.get(next_load_cursor + 3)?;
    if !loop_branch.op_eq(vm::op_br_if as Op) || !miss_branch.op_eq(vm::op_br as Op) {
        return None;
    }
    if branch_target(loop_branch)? != ctx.block.id {
        return None;
    }

    let mut operands = vec![first_addr.base_local, raw_i32_operand(first_addr.delta)];
    operands.extend(first_load.operands.clone());
    operands.push(dst);
    operands.push(raw_i32_operand(second_addr.delta));
    operands.extend(second_load.operands.clone());
    operands.push(rhs);
    operands.push(raw_i32_operand(next_addr.delta));
    operands.extend(next_load.operands.clone());
    operands.push(LoweredOperand::JumpTarget(branch_target(found_branch)?));
    operands.push(LoweredOperand::JumpTarget(branch_target(miss_branch)?));
    Some(MatchResult {
        group: FamilyGroup::Memory,
        cost: 260 + loop_bonus(ctx),
        consumed: next_load_cursor + 4 - cursor,
        ops: vec![KernelOp {
            label: None,
            op: vm::op_i32_load_local_base_set4_i32_load16_u_local_base_local_eq_search_loop as Op,
            operands,
            family: "op_i32_load_local_base_set4_i32_load16_u_local_base_local_eq_search_loop",
        }],
    })
}

fn emit_i32_load_local_base_set4_i32_load8_u_local_base_local_masked_search_loop(
    ctx: &SelectionContext<'_>,
    cursor: usize,
) -> Option<MatchResult> {
    let first_addr = match_i32_local_base_address(ctx.block, cursor)?;
    let first_load_cursor = cursor + first_addr.consumed;
    let first_load = ctx.block.insts.get(first_load_cursor)?;
    if !first_load.op_eq(vm::op_i32_load as Op) {
        return None;
    }
    let set = ctx.block.insts.get(first_load_cursor + 1)?;
    let dst = raw_local_set(set, 4)?;

    let second_addr_cursor = first_load_cursor + 2;
    let second_addr = match_i32_local_base_address(ctx.block, second_addr_cursor)?;
    if !same_raw_operand(&second_addr.base_local, &dst) {
        return None;
    }
    let second_load_cursor = second_addr_cursor + second_addr.consumed;
    let second_load = ctx.block.insts.get(second_load_cursor)?;
    if !second_load.op_eq(vm::op_i32_load8_u as Op) {
        return None;
    }
    let rhs = raw_local_get(ctx.block.insts.get(second_load_cursor + 1)?, 4)?;
    let mask = const_operand_for_kind(
        ctx.block.insts.get(second_load_cursor + 2)?,
        LocalFastConstKind::I32,
    )?;
    if raw_i32(Some(&mask))? != 255 {
        return None;
    }
    let and = ctx.block.insts.get(second_load_cursor + 3)?;
    if !and.op_eq(vm::op_i32_and as Op) {
        return None;
    }
    let compare = ctx.block.insts.get(second_load_cursor + 4)?;
    let (compare_kind, found_branch, after_compare_cursor) = if compare.op_eq(vm::op_i32_eq as Op) {
        (
            0,
            ctx.block.insts.get(second_load_cursor + 5)?,
            second_load_cursor + 6,
        )
    } else if compare.op_eq(vm::op_i32_ne as Op) {
        (
            1,
            ctx.block.insts.get(second_load_cursor + 5)?,
            second_load_cursor + 6,
        )
    } else if compare.op_eq(vm::op_i32_xor as Op) {
        let eqz = ctx.block.insts.get(second_load_cursor + 5)?;
        if !eqz.op_eq(vm::op_i32_eqz as Op) {
            return None;
        }
        (
            0,
            ctx.block.insts.get(second_load_cursor + 6)?,
            second_load_cursor + 7,
        )
    } else {
        return None;
    };
    if !found_branch.op_eq(vm::op_br_if as Op) {
        return None;
    }

    let next_addr = match_i32_local_base_address(ctx.block, after_compare_cursor)?;
    if !same_raw_operand(&next_addr.base_local, &first_addr.base_local) {
        return None;
    }
    let next_load_cursor = after_compare_cursor + next_addr.consumed;
    let next_load = ctx.block.insts.get(next_load_cursor)?;
    if !next_load.op_eq(vm::op_i32_load as Op) {
        return None;
    }
    let tee = ctx.block.insts.get(next_load_cursor + 1)?;
    let tee_dst = raw_local_tee(tee, 4)?;
    if !same_raw_operand(&tee_dst, &first_addr.base_local) {
        return None;
    }
    let loop_branch = ctx.block.insts.get(next_load_cursor + 2)?;
    let miss_branch = ctx.block.insts.get(next_load_cursor + 3)?;
    if !loop_branch.op_eq(vm::op_br_if as Op) || !miss_branch.op_eq(vm::op_br as Op) {
        return None;
    }
    if branch_target(loop_branch)? != ctx.block.id {
        return None;
    }

    let mut operands = vec![first_addr.base_local, raw_i32_operand(first_addr.delta)];
    operands.extend(first_load.operands.clone());
    operands.push(dst);
    operands.push(raw_i32_operand(second_addr.delta));
    operands.extend(second_load.operands.clone());
    operands.push(rhs);
    operands.push(raw_u32_operand(compare_kind));
    operands.push(raw_i32_operand(next_addr.delta));
    operands.extend(next_load.operands.clone());
    operands.push(LoweredOperand::JumpTarget(branch_target(found_branch)?));
    operands.push(LoweredOperand::JumpTarget(branch_target(miss_branch)?));
    Some(MatchResult {
        group: FamilyGroup::Memory,
        cost: 280 + loop_bonus(ctx),
        consumed: next_load_cursor + 4 - cursor,
        ops: vec![KernelOp {
            label: None,
            op: vm::op_i32_load_local_base_set4_i32_load8_u_local_base_local_masked_search_loop
                as Op,
            operands,
            family: "op_i32_load_local_base_set4_i32_load8_u_local_base_local_masked_search_loop",
        }],
    })
}

fn emit_i32_load_local_base_set4_i32_load8_u_local_base_local_masked_compare_br_if(
    ctx: &SelectionContext<'_>,
    cursor: usize,
) -> Option<MatchResult> {
    let first_addr = match_i32_local_base_address(ctx.block, cursor)?;
    let first_load_cursor = cursor + first_addr.consumed;
    let first_load = ctx.block.insts.get(first_load_cursor)?;
    if !first_load.op_eq(vm::op_i32_load as Op) {
        return None;
    }
    let set = ctx.block.insts.get(first_load_cursor + 1)?;
    let dst = raw_local_set(set, 4)?;
    let second_addr_cursor = first_load_cursor + 2;
    let second_addr = match_i32_local_base_address(ctx.block, second_addr_cursor)?;
    if !same_raw_operand(&second_addr.base_local, &dst) {
        return None;
    }
    let second_load_cursor = second_addr_cursor + second_addr.consumed;
    let second_load = ctx.block.insts.get(second_load_cursor)?;
    if !second_load.op_eq(vm::op_i32_load8_u as Op) {
        return None;
    }
    let rhs = raw_local_get(ctx.block.insts.get(second_load_cursor + 1)?, 4)?;
    let mask = const_operand_for_kind(
        ctx.block.insts.get(second_load_cursor + 2)?,
        LocalFastConstKind::I32,
    )?;
    if raw_i32(Some(&mask))? != 255 {
        return None;
    }
    let and = ctx.block.insts.get(second_load_cursor + 3)?;
    if !and.op_eq(vm::op_i32_and as Op) {
        return None;
    }
    let compare = ctx.block.insts.get(second_load_cursor + 4)?;
    let (compare_kind, branch, tail_consumed) = if compare.op_eq(vm::op_i32_eq as Op) {
        (0, ctx.block.insts.get(second_load_cursor + 5)?, 6)
    } else if compare.op_eq(vm::op_i32_ne as Op) {
        (1, ctx.block.insts.get(second_load_cursor + 5)?, 6)
    } else if compare.op_eq(vm::op_i32_xor as Op) {
        let eqz = ctx.block.insts.get(second_load_cursor + 5)?;
        if !eqz.op_eq(vm::op_i32_eqz as Op) {
            return None;
        }
        (0, ctx.block.insts.get(second_load_cursor + 6)?, 7)
    } else {
        return None;
    };
    if !branch.op_eq(vm::op_br_if as Op) {
        return None;
    }
    let mut operands = vec![first_addr.base_local, raw_i32_operand(first_addr.delta)];
    operands.extend(first_load.operands.clone());
    operands.push(dst);
    operands.push(raw_i32_operand(second_addr.delta));
    operands.extend(second_load.operands.clone());
    operands.push(rhs);
    operands.push(raw_u32_operand(compare_kind));
    operands.push(LoweredOperand::JumpTarget(branch_target(branch)?));
    Some(MatchResult {
        group: FamilyGroup::Memory,
        cost: 136 + loop_bonus(ctx),
        consumed: first_addr.consumed + 2 + second_addr.consumed + tail_consumed,
        ops: vec![KernelOp {
            label: None,
            op: vm::op_i32_load_local_base_set4_i32_load8_u_local_base_local_masked_compare_br_if
                as Op,
            operands,
            family: "op_i32_load_local_base_set4_i32_load8_u_local_base_local_masked_compare_br_if",
        }],
    })
}

fn emit_i32_load_local_base_set4_scalar_load_local_base_local_get4(
    ctx: &SelectionContext<'_>,
    cursor: usize,
) -> Option<MatchResult> {
    let first_addr = match_i32_local_base_address(ctx.block, cursor)?;
    let first_load_cursor = cursor + first_addr.consumed;
    let first_load = ctx.block.insts.get(first_load_cursor)?;
    if !first_load.op_eq(vm::op_i32_load as Op) {
        return None;
    }
    let set = ctx.block.insts.get(first_load_cursor + 1)?;
    let dst = raw_local_set(set, 4)?;
    let second_addr_cursor = first_load_cursor + 2;
    let second_addr = match_i32_local_base_address(ctx.block, second_addr_cursor)?;
    if !same_raw_operand(&second_addr.base_local, &dst) {
        return None;
    }
    let second_load_cursor = second_addr_cursor + second_addr.consumed;
    let second_load = ctx.block.insts.get(second_load_cursor)?;
    if scalar_memory_load_type_for_inst(second_load) != Some(ScalarType::I32) {
        return None;
    }
    let preserved = raw_local_get(ctx.block.insts.get(second_load_cursor + 1)?, 4)?;
    let op = local_base_set4_load_local_get4_family(second_load.op)?;
    let mut operands = vec![first_addr.base_local, raw_i32_operand(first_addr.delta)];
    operands.extend(first_load.operands.clone());
    operands.push(dst);
    operands.push(raw_i32_operand(second_addr.delta));
    operands.extend(second_load.operands.clone());
    operands.push(preserved);
    Some(MatchResult {
        group: FamilyGroup::Memory,
        cost: 86 + loop_bonus(ctx),
        consumed: first_addr.consumed + 2 + second_addr.consumed + 2,
        ops: vec![KernelOp {
            label: None,
            op,
            operands,
            family: "op_i32_load_local_base_set4_i32_load_local_base_local_get4",
        }],
    })
}

fn emit_i32_load_local_base_set4_scalar_load_local_base_local_eq_br_if(
    ctx: &SelectionContext<'_>,
    cursor: usize,
) -> Option<MatchResult> {
    let first_addr = match_i32_local_base_address(ctx.block, cursor)?;
    let first_load_cursor = cursor + first_addr.consumed;
    let first_load = ctx.block.insts.get(first_load_cursor)?;
    if !first_load.op_eq(vm::op_i32_load as Op) {
        return None;
    }
    let set = ctx.block.insts.get(first_load_cursor + 1)?;
    let dst = raw_local_set(set, 4)?;
    let second_addr_cursor = first_load_cursor + 2;
    let second_addr = match_i32_local_base_address(ctx.block, second_addr_cursor)?;
    if !same_raw_operand(&second_addr.base_local, &dst) {
        return None;
    }
    let second_load_cursor = second_addr_cursor + second_addr.consumed;
    let second_load = ctx.block.insts.get(second_load_cursor)?;
    if scalar_memory_load_type_for_inst(second_load) != Some(ScalarType::I32) {
        return None;
    }
    let rhs = raw_local_get(ctx.block.insts.get(second_load_cursor + 1)?, 4)?;
    let eq = ctx.block.insts.get(second_load_cursor + 2)?;
    let branch = ctx.block.insts.get(second_load_cursor + 3)?;
    if !eq.op_eq(vm::op_i32_eq as Op) || !branch.op_eq(vm::op_br_if as Op) {
        return None;
    }
    let op = local_base_set4_load_local_eq_br_if_family(second_load.op)?;
    let mut operands = vec![first_addr.base_local, raw_i32_operand(first_addr.delta)];
    operands.extend(first_load.operands.clone());
    operands.push(dst);
    operands.push(raw_i32_operand(second_addr.delta));
    operands.extend(second_load.operands.clone());
    operands.push(rhs);
    operands.push(LoweredOperand::JumpTarget(branch_target(branch)?));
    Some(MatchResult {
        group: FamilyGroup::Memory,
        cost: 118 + loop_bonus(ctx),
        consumed: first_addr.consumed + 2 + second_addr.consumed + 4,
        ops: vec![KernelOp {
            label: None,
            op,
            operands,
            family: "op_i32_load_local_base_set4_i32_load_local_base_local_eq_br_if",
        }],
    })
}

fn emit_i32_load_local_base_set4_scalar_load_local_base(
    ctx: &SelectionContext<'_>,
    cursor: usize,
) -> Option<MatchResult> {
    let first_addr = match_i32_local_base_address(ctx.block, cursor)?;
    let first_load_cursor = cursor + first_addr.consumed;
    let first_load = ctx.block.insts.get(first_load_cursor)?;
    if !first_load.op_eq(vm::op_i32_load as Op) {
        return None;
    }
    let set = ctx.block.insts.get(first_load_cursor + 1)?;
    let dst = raw_local_set(set, 4)?;
    let second_addr_cursor = first_load_cursor + 2;
    let second_addr = match_i32_local_base_address(ctx.block, second_addr_cursor)?;
    if !same_raw_operand(&second_addr.base_local, &dst) {
        return None;
    }
    let second_load_cursor = second_addr_cursor + second_addr.consumed;
    let second_load = ctx.block.insts.get(second_load_cursor)?;
    if scalar_memory_load_type_for_inst(second_load) != Some(ScalarType::I32) {
        return None;
    }
    if ctx
        .block
        .insts
        .get(second_load_cursor + 1)
        .is_some_and(|inst| raw_local_get(inst, 4).is_some())
    {
        return None;
    }
    let op = local_base_set4_load_family(second_load.op)?;
    let mut operands = vec![first_addr.base_local, raw_i32_operand(first_addr.delta)];
    operands.extend(first_load.operands.clone());
    operands.push(dst);
    operands.push(raw_i32_operand(second_addr.delta));
    operands.extend(second_load.operands.clone());
    Some(MatchResult {
        group: FamilyGroup::Memory,
        cost: 72 + loop_bonus(ctx),
        consumed: first_addr.consumed + 2 + second_addr.consumed + 1,
        ops: vec![KernelOp {
            label: None,
            op,
            operands,
            family: "op_i32_load_local_base_set4_i32_load_local_base",
        }],
    })
}

fn emit_scalar_load_local_base_local_get4_scalar_load(
    ctx: &SelectionContext<'_>,
    cursor: usize,
) -> Option<MatchResult> {
    let matched = match_i32_local_base_address(ctx.block, cursor)?;
    let first_load_cursor = cursor + matched.consumed;
    let first_load = ctx.block.insts.get(first_load_cursor)?;
    if scalar_memory_load_type_for_inst(first_load) != Some(ScalarType::I32) {
        return None;
    }
    let second_addr = raw_local_get(ctx.block.insts.get(first_load_cursor + 1)?, 4)?;
    let second_load = ctx.block.insts.get(first_load_cursor + 2)?;
    if scalar_memory_load_type_for_inst(second_load) != Some(ScalarType::I32) {
        return None;
    }
    let op = local_base_load_local_get4_scalar_load_family(first_load.op, second_load.op)?;
    let mut operands = vec![matched.base_local, raw_i32_operand(matched.delta)];
    operands.extend(first_load.operands.clone());
    operands.push(second_addr);
    operands.extend(second_load.operands.clone());
    Some(MatchResult {
        group: FamilyGroup::Memory,
        cost: 76 + loop_bonus(ctx),
        consumed: matched.consumed + 3,
        ops: vec![KernelOp {
            label: None,
            op,
            operands,
            family: "op_i32_load16_local_base_local_get4_i32_load16",
        }],
    })
}

fn emit_local_get4_i32_load_local_base_add_set4(
    ctx: &SelectionContext<'_>,
    cursor: usize,
) -> Option<MatchResult> {
    let rhs = raw_local_get(ctx.block.insts.get(cursor)?, 4)?;
    let matched = match_i32_local_base_address(ctx.block, cursor + 1)?;
    let load_cursor = cursor + 1 + matched.consumed;
    let load = ctx.block.insts.get(load_cursor)?;
    if scalar_memory_load_type_for_inst(load) != Some(ScalarType::I32)
        || !scalar_local_base_load_family_for_type(load.op, ScalarType::I32)
            .is_some_and(|op| std::ptr::fn_addr_eq(op, vm::op_i32_load_local_base as Op))
    {
        return None;
    }
    let add = ctx.block.insts.get(load_cursor + 1)?;
    let consumer = ctx.block.insts.get(load_cursor + 2)?;
    if !add.op_eq(vm::op_i32_add as Op) {
        return None;
    }
    let (op, dst) = if let Some(dst) = raw_local_set(consumer, 4) {
        (
            vm::op_local_get4_i32_load_local_base_i32_add_set4 as Op,
            dst,
        )
    } else {
        (
            vm::op_local_get4_i32_load_local_base_i32_add_tee4 as Op,
            raw_local_tee(consumer, 4)?,
        )
    };
    let mut operands = vec![rhs, matched.base_local, raw_i32_operand(matched.delta)];
    operands.extend(load.operands.clone());
    operands.push(dst);
    Some(MatchResult {
        group: FamilyGroup::Memory,
        cost: 56 + loop_bonus(ctx),
        consumed: 1 + matched.consumed + 3,
        ops: vec![KernelOp {
            label: None,
            op,
            operands,
            family: "op_local_get4_i32_load_local_base_i32_add_set4",
        }],
    })
}

fn emit_scalar_load_local_base_local_get4(
    ctx: &SelectionContext<'_>,
    cursor: usize,
) -> Option<MatchResult> {
    let matched = match_i32_local_base_address(ctx.block, cursor)?;
    let load_cursor = cursor + matched.consumed;
    let load = ctx.block.insts.get(load_cursor)?;
    if scalar_memory_load_type_for_inst(load) != Some(ScalarType::I32) {
        return None;
    }
    let local_base_op = scalar_local_base_load_family_for_type(load.op, ScalarType::I32)?;
    let after_load = load_cursor + 1;
    let first_consumer = ctx.block.insts.get(after_load)?;
    let (op, tee_dst, preserved, consumed) = if let Some(dst) = raw_local_tee(first_consumer, 4) {
        let preserved = raw_local_get(ctx.block.insts.get(after_load + 1)?, 4)?;
        (
            local_base_load_local_get4_family(local_base_op, true)?,
            Some(dst),
            preserved,
            matched.consumed + 3,
        )
    } else {
        (
            local_base_load_local_get4_family(local_base_op, false)?,
            None,
            raw_local_get(first_consumer, 4)?,
            matched.consumed + 2,
        )
    };
    let mut operands = vec![matched.base_local, raw_i32_operand(matched.delta)];
    operands.extend(load.operands.clone());
    if let Some(dst) = tee_dst {
        operands.push(dst);
    }
    operands.push(preserved);
    Some(MatchResult {
        group: FamilyGroup::Memory,
        cost: 44 + loop_bonus(ctx),
        consumed,
        ops: vec![KernelOp {
            label: None,
            op,
            operands,
            family: "memory.local_base_load_local_get4",
        }],
    })
}

fn emit_scalar_load_local_get4(ctx: &SelectionContext<'_>, cursor: usize) -> Option<MatchResult> {
    let load = ctx.block.insts.get(cursor)?;
    if scalar_memory_load_type_for_inst(load) != Some(ScalarType::I32) {
        return None;
    }
    let op = i32_load_local_get4_family(load.op)?;
    let preserved = raw_local_get(ctx.block.insts.get(cursor + 1)?, 4)?;
    let mut operands = load.operands.clone();
    operands.push(preserved);
    Some(MatchResult {
        group: FamilyGroup::Memory,
        cost: 24 + loop_bonus(ctx),
        consumed: 2,
        ops: vec![KernelOp {
            label: None,
            op,
            operands,
            family: "memory.load_local_get4",
        }],
    })
}

fn emit_scalar_load_local_base_set4(
    ctx: &SelectionContext<'_>,
    cursor: usize,
) -> Option<MatchResult> {
    let matched = match_i32_local_base_address(ctx.block, cursor)?;
    let load = ctx.block.insts.get(cursor + matched.consumed)?;
    if scalar_memory_load_type_for_inst(load) != Some(ScalarType::I32) {
        return None;
    }
    let local_base_op = scalar_local_base_load_family_for_type(load.op, ScalarType::I32)?;
    let consumer = ctx.block.insts.get(cursor + matched.consumed + 1)?;
    let (dst, tee) = if let Some(dst) = raw_local_set(consumer, 4) {
        (dst, false)
    } else {
        (raw_local_tee(consumer, 4)?, true)
    };
    let op = local_base_load_set4_family(local_base_op, tee)?;
    let mut operands = vec![matched.base_local, raw_i32_operand(matched.delta)];
    operands.extend(load.operands.clone());
    operands.push(dst);
    Some(MatchResult {
        group: FamilyGroup::Memory,
        cost: 40 + loop_bonus(ctx),
        consumed: matched.consumed + 2,
        ops: vec![KernelOp {
            label: None,
            op,
            operands,
            family: "memory.local_base_load_set4",
        }],
    })
}

fn emit_scalar_load_local_base(
    ctx: &SelectionContext<'_>,
    cursor: usize,
) -> Option<(MatchResult, ScalarType)> {
    let matched = match_i32_local_base_address(ctx.block, cursor)?;
    let load = ctx.block.insts.get(cursor + matched.consumed)?;
    let scalar = scalar_memory_load_type_for_inst(load)?;
    let op = scalar_local_base_load_family_for_type(load.op, scalar)?;
    let mut operands = vec![matched.base_local, raw_i32_operand(matched.delta)];
    operands.extend(load.operands.clone());
    Some((
        MatchResult {
            group: FamilyGroup::Memory,
            cost: 26 + loop_bonus(ctx),
            consumed: matched.consumed + 1,
            ops: vec![KernelOp {
                label: None,
                op,
                operands,
                family: "memory.local_base",
            }],
        },
        scalar,
    ))
}

fn emit_scalar_store_local_base(ctx: &SelectionContext<'_>, cursor: usize) -> Option<MatchResult> {
    let addr = match_i32_local_base_address(ctx.block, cursor)?;
    if let Some(result) = emit_i32_store_local_base_local_get4(ctx, cursor, &addr) {
        return Some(result);
    }
    for scalar in [
        ScalarType::I32,
        ScalarType::I64,
        ScalarType::F32,
        ScalarType::F64,
    ] {
        let Some((mut value_ops, value_consumed)) =
            match_scalar_value_expr(ctx, cursor + addr.consumed, scalar)
        else {
            continue;
        };
        let Some(store) = ctx.block.insts.get(cursor + addr.consumed + value_consumed) else {
            continue;
        };
        if scalar_memory_store_type_for_inst(store) != Some(scalar) {
            continue;
        }
        let op = scalar_local_base_store_family_for_type(store.op, scalar)?;
        value_ops.push(KernelOp {
            label: None,
            op,
            operands: {
                let mut operands = vec![addr.base_local, raw_i32_operand(addr.delta)];
                operands.extend(store.operands.clone());
                operands
            },
            family: "memory.local_base",
        });
        return Some(MatchResult {
            group: FamilyGroup::Memory,
            cost: 30 + loop_bonus(ctx),
            consumed: addr.consumed + value_consumed + 1,
            ops: value_ops,
        });
    }
    None
}

fn emit_i32_store_local_base_local_get4(
    ctx: &SelectionContext<'_>,
    cursor: usize,
    addr: &LocalBaseAddressMatch,
) -> Option<MatchResult> {
    let value = raw_local_get(ctx.block.insts.get(cursor + addr.consumed)?, 4)?;
    let store = ctx.block.insts.get(cursor + addr.consumed + 1)?;
    if scalar_memory_store_type_for_inst(store) != Some(ScalarType::I32) {
        return None;
    }
    let op = local_base_store_local_get4_family(scalar_local_base_store_family_for_type(
        store.op,
        ScalarType::I32,
    )?)?;
    let mut operands = vec![addr.base_local.clone(), raw_i32_operand(addr.delta), value];
    operands.extend(store.operands.clone());
    Some(MatchResult {
        group: FamilyGroup::Memory,
        cost: 42 + loop_bonus(ctx),
        consumed: addr.consumed + 2,
        ops: vec![KernelOp {
            label: None,
            op,
            operands,
            family: "memory.local_base.store_local_get4",
        }],
    })
}

fn emit_scalar_load_local_scaled_index(
    ctx: &SelectionContext<'_>,
    cursor: usize,
) -> Option<(MatchResult, ScalarType)> {
    let matched = match_i32_local_scaled_index_address(ctx.block, cursor)?;
    let load = ctx.block.insts.get(cursor + matched.consumed)?;
    let scalar = scalar_memory_load_type_for_inst(load)?;
    let op = scalar_local_scaled_index_load_family_for_type(load.op, scalar)?;
    Some((
        MatchResult {
            group: FamilyGroup::Memory,
            cost: 32 + loop_bonus(ctx),
            consumed: matched.consumed + 1,
            ops: vec![KernelOp {
                label: None,
                op,
                operands: vec![
                    matched.base_local,
                    matched.index_local,
                    raw_u32_operand(matched.scale_log2),
                    raw_i32_operand(matched.delta),
                    load.operands.first()?.clone(),
                ],
                family: "memory.local_scaled_index",
            }],
        },
        scalar,
    ))
}

fn emit_scalar_store_local_scaled_index(
    ctx: &SelectionContext<'_>,
    cursor: usize,
) -> Option<MatchResult> {
    let addr = match_i32_local_scaled_index_address(ctx.block, cursor)?;
    for scalar in [
        ScalarType::I32,
        ScalarType::I64,
        ScalarType::F32,
        ScalarType::F64,
    ] {
        let Some((mut value_ops, value_consumed)) =
            match_scalar_value_expr(ctx, cursor + addr.consumed, scalar)
        else {
            continue;
        };
        let Some(store) = ctx.block.insts.get(cursor + addr.consumed + value_consumed) else {
            continue;
        };
        if scalar_memory_store_type_for_inst(store) != Some(scalar) {
            continue;
        }
        let op = scalar_local_scaled_index_store_family_for_type(store.op, scalar)?;
        value_ops.push(KernelOp {
            label: None,
            op,
            operands: {
                let mut operands = vec![
                    addr.base_local,
                    addr.index_local,
                    raw_u32_operand(addr.scale_log2),
                    raw_i32_operand(addr.delta),
                ];
                operands.extend(store.operands.clone());
                operands
            },
            family: "memory.local_scaled_index",
        });
        return Some(MatchResult {
            group: FamilyGroup::Memory,
            cost: 36 + loop_bonus(ctx),
            consumed: addr.consumed + value_consumed + 1,
            ops: value_ops,
        });
    }
    None
}

fn match_scalar_value_expr(
    ctx: &SelectionContext<'_>,
    cursor: usize,
    scalar: ScalarType,
) -> Option<(Vec<KernelOp>, usize)> {
    let (mut lhs_ops, lhs_consumed) = match_scalar_atomic_value_expr(ctx, cursor, scalar)?;
    let rhs_cursor = cursor + lhs_consumed;
    let Some((rhs_ops, rhs_consumed)) = match_scalar_atomic_value_expr(ctx, rhs_cursor, scalar)
    else {
        return Some((lhs_ops, lhs_consumed));
    };
    let add = ctx.block.insts.get(rhs_cursor + rhs_consumed)?;
    if !add.op_eq(scalar.add_op()) {
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

fn match_scalar_atomic_value_expr(
    ctx: &SelectionContext<'_>,
    cursor: usize,
    scalar: ScalarType,
) -> Option<(Vec<KernelOp>, usize)> {
    if let Some((result, result_scalar)) = emit_scalar_load_local_scaled_index(ctx, cursor) {
        if result_scalar == scalar {
            return Some((result.ops, result.consumed));
        }
    }
    if let Some((result, result_scalar)) = emit_scalar_load_local_base(ctx, cursor) {
        if result_scalar == scalar {
            return Some((result.ops, result.consumed));
        }
    }
    if let Some((result, result_scalar)) = emit_scalar_load_const_base(ctx, cursor) {
        if result_scalar == scalar {
            return Some((result.ops, result.consumed));
        }
    }
    let inst = ctx.block.insts.get(cursor)?;
    if inst.op_eq(vm::op_select as Op)
        || inst.op_eq(vm::op_select4 as Op)
        || inst.op_eq(vm::op_select8 as Op)
        || inst.op_eq(vm::op_select16 as Op)
    {
        return None;
    }
    if raw_local_get(inst, scalar.width()).is_some()
        || const_operand_for_kind(inst, scalar.const_kind()).is_some()
    {
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

fn match_i32_select_bit_step4(
    block: &CanonBlock,
    cursor: usize,
) -> Option<(Vec<LoweredOperand>, usize)> {
    let shift_one = block.insts.get(cursor)?;
    let shr = block.insts.get(cursor + 1)?;
    if i32_const_value(shift_one)? != 1 || !shr.op_eq(vm::op_i32_shr_u as Op) {
        return None;
    }

    let mut flags = 0;
    let mut at = cursor + 2;
    if block
        .insts
        .get(at)
        .and_then(i32_const_value)
        .is_some_and(|value| value == 0x7fff)
        && block
            .insts
            .get(at + 1)
            .is_some_and(|inst| inst.op_eq(vm::op_i32_and as Op))
    {
        flags |= I32_SELECT_BIT_STEP_MASK_SHIFTED;
        at += 2;
    }

    let tmp_tee = block.insts.get(at)?;
    let tmp_local = raw_local_tee(tmp_tee, 4)?;
    at += 1;

    match_i32_select_bit_step4_xor_condition(block, at, &tmp_local, flags)
        .or_else(|| match_i32_select_bit_step4_eq_condition(block, at, &tmp_local, flags))
        .map(|(operands, consumed)| (operands, consumed + (at - cursor)))
}

fn match_i32_select_bit_step4_run(
    block: &CanonBlock,
    cursor: usize,
) -> Option<(Vec<LoweredOperand>, usize)> {
    const MAX_RUN: usize = 16;

    let mut count = 0usize;
    let mut operands = vec![raw_u32_operand(0)];
    while count < MAX_RUN {
        let Some(inst) = block.insts.get(cursor + count) else {
            break;
        };
        if !inst.op_eq(vm::op_i32_select_bit_step4 as Op) || inst.operands.len() != 7 {
            break;
        }
        operands.extend(inst.operands.iter().cloned());
        count += 1;
    }
    if count < 2 {
        return None;
    }
    operands[0] = raw_u32_operand(u32::try_from(count).ok()?);
    Some((operands, count))
}

fn match_i32_select_bit_step4_xor_condition(
    block: &CanonBlock,
    cursor: usize,
    tmp_local: &LoweredOperand,
    flags: u32,
) -> Option<(Vec<LoweredOperand>, usize)> {
    let poly = block.insts.get(cursor)?;
    let xor = block.insts.get(cursor + 1)?;
    let tmp_get = block.insts.get(cursor + 2)?;
    let tmp_get_local = raw_local_get(tmp_get, 4);
    if !xor.op_eq(vm::op_i32_xor as Op)
        || !tmp_get_local
            .as_ref()
            .is_some_and(|local| same_raw_operand(local, tmp_local))
    {
        return None;
    }
    let poly = const_operand_for_kind(poly, LocalFastConstKind::I32)?;
    let (source_local, source_shift, prev_local, condition_consumed) =
        match_i32_xor_lsb_condition(block, cursor + 3)?;
    let select = block.insts.get(cursor + 3 + condition_consumed)?;
    if !is_select4_raw(select) {
        return None;
    }

    let (dst_local, flags, consumer_consumed) =
        match_i32_select_bit_step4_consumer(block, cursor + 4 + condition_consumed, flags);
    Some((
        vec![
            tmp_local.clone(),
            poly,
            source_local,
            raw_u32_operand(source_shift),
            prev_local,
            raw_u32_operand(flags),
            dst_local,
        ],
        4 + condition_consumed + consumer_consumed,
    ))
}

fn match_i32_select_bit_step4_eq_condition(
    block: &CanonBlock,
    cursor: usize,
    tmp_local: &LoweredOperand,
    flags: u32,
) -> Option<(Vec<LoweredOperand>, usize)> {
    let tmp_get = block.insts.get(cursor)?;
    let poly = block.insts.get(cursor + 1)?;
    let xor = block.insts.get(cursor + 2)?;
    let prev = block.insts.get(cursor + 3)?;
    let one = block.insts.get(cursor + 4)?;
    let and = block.insts.get(cursor + 5)?;
    let source = block.insts.get(cursor + 6)?;
    let shift = block.insts.get(cursor + 7)?;
    let shr = block.insts.get(cursor + 8)?;
    let eq = block.insts.get(cursor + 9)?;
    let select = block.insts.get(cursor + 10)?;
    if !same_raw_operand(&raw_local_get(tmp_get, 4)?, tmp_local)
        || !xor.op_eq(vm::op_i32_xor as Op)
        || i32_const_value(one)? != 1
        || !and.op_eq(vm::op_i32_and as Op)
        || !shr.op_eq(vm::op_i32_shr_u as Op)
        || !eq.op_eq(vm::op_i32_eq as Op)
        || !is_select4_raw(select)
    {
        return None;
    }

    let source_shift = i32_const_value(shift)? as u32;
    let (dst_local, flags, consumer_consumed) = match_i32_select_bit_step4_consumer(
        block,
        cursor + 11,
        flags | I32_SELECT_BIT_STEP_EQ_CONDITION,
    );
    Some((
        vec![
            tmp_local.clone(),
            const_operand_for_kind(poly, LocalFastConstKind::I32)?,
            raw_local_get(source, 4)?,
            raw_u32_operand(source_shift),
            raw_local_get(prev, 4)?,
            raw_u32_operand(flags),
            dst_local,
        ],
        11 + consumer_consumed,
    ))
}

fn match_i32_xor_lsb_condition(
    block: &CanonBlock,
    cursor: usize,
) -> Option<(LoweredOperand, u32, LoweredOperand, usize)> {
    let source = raw_local_get(block.insts.get(cursor)?, 4)?;
    let mut source_shift = 0;
    let mut at = cursor + 1;
    if let (Some(shift), Some(shr)) = (block.insts.get(at), block.insts.get(at + 1)) {
        if shift.op_eq(vm::op_i32_const as Op) && shr.op_eq(vm::op_i32_shr_u as Op) {
            source_shift = i32_const_value(shift)? as u32;
            at += 2;
        }
    }

    let prev = raw_local_get(block.insts.get(at)?, 4)?;
    let xor = block.insts.get(at + 1)?;
    let one = block.insts.get(at + 2)?;
    let and = block.insts.get(at + 3)?;
    if !xor.op_eq(vm::op_i32_xor as Op)
        || i32_const_value(one)? != 1
        || !and.op_eq(vm::op_i32_and as Op)
    {
        return None;
    }
    Some((source, source_shift, prev, at + 4 - cursor))
}

fn match_i32_select_bit_step4_consumer(
    block: &CanonBlock,
    cursor: usize,
    flags: u32,
) -> (LoweredOperand, u32, usize) {
    if let Some(dst) = block
        .insts
        .get(cursor)
        .and_then(|inst| raw_local_tee(inst, 4))
    {
        return (dst, flags | I32_SELECT_BIT_STEP_TEE_DST, 1);
    }
    (raw_u32_operand(u32::MAX), flags, 0)
}

fn match_stack_i32_const_binop(
    block: &CanonBlock,
    cursor: usize,
    consumer: StackI32ConstBinopConsumer,
) -> Option<(Vec<LoweredOperand>, usize)> {
    let imm = block.insts.get(cursor)?;
    let binop = block.insts.get(cursor + 1)?;
    if !imm.op_eq(vm::op_i32_const as Op) {
        return None;
    }
    let kind = stack_i32_const_binop_kind(binop.op)?;
    let mut operands = vec![
        raw_u32_operand(encode_local_binop32_kind(kind, LocalFastRhsShape::Const)),
        imm.operands.first()?.clone(),
    ];

    match consumer {
        StackI32ConstBinopConsumer::Root => Some((operands, 2)),
        StackI32ConstBinopConsumer::Set => {
            let tail = block.insts.get(cursor + 2)?;
            operands.push(raw_local_set(tail, 4)?);
            Some((operands, 3))
        }
        StackI32ConstBinopConsumer::Tee => {
            let tail = block.insts.get(cursor + 2)?;
            operands.push(raw_local_tee(tail, 4)?);
            Some((operands, 3))
        }
        StackI32ConstBinopConsumer::BrIf => {
            let branch = block.insts.get(cursor + 2)?;
            if !branch.op_eq(vm::op_br_if as Op) {
                return None;
            }
            operands.push(LoweredOperand::JumpTarget(branch_target(branch)?));
            Some((operands, 3))
        }
    }
}

fn match_stack_i32_const_cmp(
    block: &CanonBlock,
    cursor: usize,
    consumer: StackI32ConstCmpConsumer,
) -> Option<(Vec<LoweredOperand>, usize)> {
    let imm = block.insts.get(cursor)?;
    let cmp = block.insts.get(cursor + 1)?;
    if !imm.op_eq(vm::op_i32_const as Op) {
        return None;
    }
    let kind = stack_i32_const_cmp_kind(cmp.op)?;
    let mut operands = vec![
        raw_u32_operand(encode_local_cmp32_kind(kind, LocalFastRhsShape::Const)),
        imm.operands.first()?.clone(),
    ];

    match consumer {
        StackI32ConstCmpConsumer::Root => Some((operands, 2)),
        StackI32ConstCmpConsumer::Set => {
            let tail = block.insts.get(cursor + 2)?;
            operands.push(raw_local_set(tail, 4)?);
            Some((operands, 3))
        }
        StackI32ConstCmpConsumer::Tee => {
            let tail = block.insts.get(cursor + 2)?;
            operands.push(raw_local_tee(tail, 4)?);
            Some((operands, 3))
        }
        StackI32ConstCmpConsumer::BrIf => {
            let branch = block.insts.get(cursor + 2)?;
            if !branch.op_eq(vm::op_br_if as Op) {
                return None;
            }
            operands.push(LoweredOperand::JumpTarget(branch_target(branch)?));
            Some((operands, 3))
        }
    }
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

fn stack_i32_const_binop_kind(op: Op) -> Option<LocalBinop32Op> {
    let NumericKind::Binop32(kind) = numeric_kind(op)? else {
        return None;
    };
    if matches!(
        kind,
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
            | LocalBinop32Op::I32Rotr
    ) {
        Some(kind)
    } else {
        None
    }
}

fn stack_i32_const_cmp_kind(op: Op) -> Option<LocalCmp32Op> {
    let NumericKind::Cmp32(kind) = numeric_kind(op)? else {
        return None;
    };
    if matches!(
        kind,
        LocalCmp32Op::I32Eq
            | LocalCmp32Op::I32Ne
            | LocalCmp32Op::I32LtS
            | LocalCmp32Op::I32LtU
            | LocalCmp32Op::I32GtS
            | LocalCmp32Op::I32GtU
            | LocalCmp32Op::I32LeS
            | LocalCmp32Op::I32LeU
            | LocalCmp32Op::I32GeS
            | LocalCmp32Op::I32GeU
    ) {
        Some(kind)
    } else {
        None
    }
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
    if inst.op_eq(vm::op_select as Op)
        || inst.op_eq(vm::op_select4 as Op)
        || inst.op_eq(vm::op_select8 as Op)
        || inst.op_eq(vm::op_select16 as Op)
    {
        return None;
    }
    if width == 4 && inst.op_eq(vm::op_local_get4 as Op) {
        return Some(inst.operands.first()?.clone());
    }
    if width == 8 && inst.op_eq(vm::op_local_get8 as Op) {
        return Some(inst.operands.first()?.clone());
    }
    None
}

fn same_raw_operand(lhs: &LoweredOperand, rhs: &LoweredOperand) -> bool {
    match (lhs, rhs) {
        (LoweredOperand::Raw(lhs), LoweredOperand::Raw(rhs)) => lhs == rhs,
        _ => false,
    }
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

fn i32_const_value(inst: &CanonInst) -> Option<i32> {
    raw_i32(Some(&const_operand_for_kind(
        inst,
        LocalFastConstKind::I32,
    )?))
}

fn raw_select(operand: Option<&LoweredOperand>) -> Option<u32> {
    let LoweredOperand::Raw(encoded) = operand? else {
        return None;
    };
    Some(unsafe { Operand { encoded: *encoded }.select })
}

fn is_select4_raw(inst: &CanonInst) -> bool {
    inst.op_eq(vm::op_select as Op) || inst.op_eq(vm::op_select4 as Op)
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

fn i32_scalar_load_kind(op: Op) -> Option<u32> {
    if std::ptr::fn_addr_eq(op, vm::op_i32_load as Op) {
        Some(0)
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_load8_s as Op) {
        Some(1)
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_load8_u as Op) {
        Some(2)
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_load16_s as Op) {
        Some(3)
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_load16_u as Op) {
        Some(4)
    } else {
        None
    }
}

fn i32_scalar_store_kind(op: Op) -> Option<u32> {
    if std::ptr::fn_addr_eq(op, vm::op_i32_store as Op) {
        Some(0)
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_store8 as Op) {
        Some(1)
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_store16 as Op) {
        Some(2)
    } else {
        None
    }
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
