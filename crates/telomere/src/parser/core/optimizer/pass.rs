use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque},
    hash::{Hash, Hasher},
    sync::OnceLock,
    time::{Duration, Instant},
};

use crate::{
    common::{CallRecipeRef, FuncIdx, FuncType, Instr, LocalsData, Op, Operand, ValType},
    runtime::vm,
};

#[cfg(test)]
use super::sink::RecordEmit;
use super::{
    cfg::{build_program, BasicBlock, BasicBlockProgram, DecodedInstr, InstructionMeta},
    expr::{
        AddressBaseKind, AddressShape, AliasAddress, AliasKey, AliasSpace, ConstValue,
        EffectBarrier, EffectEpoch, EffectOpId, ExprId, ExprOrigin, ExprOriginKind, ExprState,
        HeapVersion, LocalSlot, LoopValueShape, MaterializationCost, ProviderClass, PureOpKind,
        SlotClass, SlotRef, SlotShape, ValueDef, ValueGraph, ValueKey, ValueRef,
    },
    sink::{
        build_packed_stream, flatten_packed_stream, pack_op, verify_packed_stream, PackedOp,
        PackedOperand,
    },
    OptimizedFunction,
};

trait LocalPass {
    fn run_block(
        &mut self,
        program: &BasicBlockProgram,
        block: BasicBlock,
        entry: &BlockEntryState,
    ) -> BlockRunResult;
}

#[derive(Clone, Debug, Default)]
pub(crate) struct BlockEntryState {
    reachable: bool,
    locals: HashMap<LocalSlot, ValueRef>,
    stack: Vec<ValueRef>,
    heap: HeapVersion,
    aliases: HashMap<AliasKey, ValueRef>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct LoopInvariantSet {
    pure_origins: BTreeSet<ExprOrigin>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum JoinAliasAddress {
    Const(u32),
    EntryLocal(usize),
    BlockArgument(usize),
    Unary {
        op: PureOpKind,
        input: Box<JoinAliasAddress>,
    },
    Binary {
        op: PureOpKind,
        lhs: Box<JoinAliasAddress>,
        rhs: Box<JoinAliasAddress>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct JoinAliasKey {
    space: AliasSpace,
    index: u32,
    offset: u32,
    width: u8,
    address: JoinAliasAddress,
}

#[derive(Clone, Default)]
struct BlockRunResult {
    exit: BlockEntryState,
    body: BlockBody,
    loop_invariants: LoopInvariantSet,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SlotMergeDecision {
    Preserve,
    InsertCopy,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct BlockCopyPlan {
    stack: BTreeMap<usize, SlotMergeDecision>,
    locals: BTreeMap<LocalSlot, SlotMergeDecision>,
    aliases: BTreeMap<AliasKey, SlotMergeDecision>,
}

#[derive(Default)]
struct RelowerPlan {
    block_bodies: Vec<BlockBody>,
    loop_invariants: Vec<LoopInvariantSet>,
    block_copy_plans: Vec<BlockCopyPlan>,
    entry_prefix_ops: Vec<BlockOp>,
}

#[derive(Default)]
struct FunctionRewrite {
    entries: Vec<BlockEntryState>,
    exits: Vec<BlockEntryState>,
    graph: ValueGraph,
    relower: RelowerPlan,
}

#[derive(Clone, Copy)]
struct OptimizerProfileConfig {
    slow_block_threshold: Duration,
    verbose: bool,
    focus_func: Option<u32>,
    focus_block: Option<usize>,
    max_focus_logs: usize,
}

const OPT_FAMILY_TOP_K: usize = 16;
const PACKED_STREAM_GROWTH_BUDGET_PCT: usize = 10;
const PACKED_STREAM_GROWTH_BUDGET_ABS: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum OptimizerFamilyGroup {
    LocalControl,
    Memory,
    CallSelect,
}

impl OptimizerFamilyGroup {
    const ORDER: [Self; 3] = [Self::LocalControl, Self::Memory, Self::CallSelect];

    const fn index(self) -> usize {
        match self {
            Self::LocalControl => 0,
            Self::Memory => 1,
            Self::CallSelect => 2,
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::LocalControl => "local/control",
            Self::Memory => "memory",
            Self::CallSelect => "call/select",
        }
    }
}

#[derive(Clone, Copy, Default)]
struct OptimizerFamilyCandidateStat {
    count: u64,
    expected_provider_eliminations: u64,
}

impl OptimizerProfileConfig {
    fn from_env() -> Option<Self> {
        std::env::var_os("TELOMERE_OPT_PROFILE")?;
        let slow_block_ms = std::env::var("TELOMERE_OPT_PROFILE_SLOW_BLOCK_MS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .filter(|value| *value != 0)
            .unwrap_or(100);
        let verbose = std::env::var_os("TELOMERE_OPT_PROFILE_VERBOSE").is_some();
        let focus_func = std::env::var("TELOMERE_OPT_PROFILE_FOCUS_FUNC")
            .ok()
            .and_then(|value| value.parse::<u32>().ok());
        let focus_block = std::env::var("TELOMERE_OPT_PROFILE_FOCUS_BLOCK")
            .ok()
            .and_then(|value| value.parse::<usize>().ok());
        let max_focus_logs = std::env::var("TELOMERE_OPT_PROFILE_MAX_FOCUS_LOGS")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .filter(|value| *value != 0)
            .unwrap_or(20);
        Some(Self {
            slow_block_threshold: Duration::from_millis(slow_block_ms),
            verbose,
            focus_func,
            focus_block,
            max_focus_logs,
        })
    }
}

struct OptimizerProfiler {
    config: OptimizerProfileConfig,
    funcidx: FuncIdx,
    started_at: Instant,
    total_iterations: u64,
    block_visits: Vec<u64>,
    focus_logs_remaining: usize,
}

fn packed_stream_growth_pct(original_instrs: usize, packed_instrs: usize) -> f64 {
    if original_instrs == 0 {
        return 0.0;
    }
    (packed_instrs as f64 / original_instrs as f64 - 1.0) * 100.0
}

fn packed_stream_within_budget(original_instrs: usize, packed_instrs: usize) -> bool {
    let relative_slack = (original_instrs
        .saturating_mul(PACKED_STREAM_GROWTH_BUDGET_PCT)
        .saturating_add(99))
        / 100;
    let allowed =
        original_instrs.saturating_add(relative_slack.max(PACKED_STREAM_GROWTH_BUDGET_ABS));
    packed_instrs <= allowed
}

fn collect_optimizer_family_candidates(
    ops: &[PackedOp],
) -> [Vec<(&'static str, OptimizerFamilyCandidateStat)>; 3] {
    let mut grouped: [HashMap<&'static str, OptimizerFamilyCandidateStat>; 3] =
        std::array::from_fn(|_| HashMap::new());
    for op in ops {
        let Some((group, label, expected_provider_elims)) = optimizer_family_candidate(op.op)
        else {
            continue;
        };
        let stat = grouped[group.index()].entry(label).or_default();
        stat.count = stat.count.saturating_add(1);
        stat.expected_provider_eliminations = stat
            .expected_provider_eliminations
            .saturating_add(u64::from(expected_provider_elims));
    }
    std::array::from_fn(|group_idx| {
        let mut ranked = grouped[group_idx].drain().collect::<Vec<_>>();
        ranked.sort_by(|(lhs_label, lhs), (rhs_label, rhs)| {
            rhs.count
                .cmp(&lhs.count)
                .then_with(|| {
                    rhs.expected_provider_eliminations
                        .cmp(&lhs.expected_provider_eliminations)
                })
                .then_with(|| lhs_label.cmp(rhs_label))
        });
        ranked.truncate(OPT_FAMILY_TOP_K);
        ranked
    })
}

fn optimizer_family_candidate(op: Op) -> Option<(OptimizerFamilyGroup, &'static str, u8)> {
    if std::ptr::fn_addr_eq(op, vm::op_local_get4_br_if as Op) {
        return Some((OptimizerFamilyGroup::LocalControl, "op_local_get4_br_if", 1));
    }
    if std::ptr::fn_addr_eq(op, vm::op_local_get4_i32_eqz_br_if as Op) {
        return Some((
            OptimizerFamilyGroup::LocalControl,
            "op_local_get4_i32_eqz_br_if",
            2,
        ));
    }
    if std::ptr::fn_addr_eq(op, vm::op_local_get4_i32_const_compare_br_if as Op) {
        return Some((
            OptimizerFamilyGroup::LocalControl,
            "op_local_get4_i32_const_compare_br_if",
            3,
        ));
    }
    if std::ptr::fn_addr_eq(op, vm::op_local_get4_local_get4_compare_br_if as Op) {
        return Some((
            OptimizerFamilyGroup::LocalControl,
            "op_local_get4_local_get4_compare_br_if",
            3,
        ));
    }
    if std::ptr::fn_addr_eq(op, vm::op_local_get4_i32_const_add_br_if as Op) {
        return Some((
            OptimizerFamilyGroup::LocalControl,
            "op_local_get4_i32_const_add_br_if",
            3,
        ));
    }
    if std::ptr::fn_addr_eq(op, vm::op_local_get4_local_get4_i32_add_br_if as Op) {
        return Some((
            OptimizerFamilyGroup::LocalControl,
            "op_local_get4_local_get4_i32_add_br_if",
            3,
        ));
    }
    if std::ptr::fn_addr_eq(op, vm::op_local_get4_i32_const_add_set4 as Op) {
        return Some((
            OptimizerFamilyGroup::LocalControl,
            "op_local_get4_i32_const_add_set4",
            3,
        ));
    }
    if std::ptr::fn_addr_eq(op, vm::op_local_get4_i32_const_add_tee4 as Op) {
        return Some((
            OptimizerFamilyGroup::LocalControl,
            "op_local_get4_i32_const_add_tee4",
            3,
        ));
    }
    if std::ptr::fn_addr_eq(op, vm::op_local_get4_local_get4_i32_add_set4 as Op) {
        return Some((
            OptimizerFamilyGroup::LocalControl,
            "op_local_get4_local_get4_i32_add_set4",
            3,
        ));
    }
    if std::ptr::fn_addr_eq(op, vm::op_local_get4_local_get4_i32_add_tee4 as Op) {
        return Some((
            OptimizerFamilyGroup::LocalControl,
            "op_local_get4_local_get4_i32_add_tee4",
            3,
        ));
    }
    if std::ptr::fn_addr_eq(op, vm::op_local_get4_i32_const_add as Op) {
        return Some((
            OptimizerFamilyGroup::LocalControl,
            "op_local_get4_i32_const_add",
            2,
        ));
    }
    if std::ptr::fn_addr_eq(op, vm::op_local_get4_local_get4_i32_add as Op) {
        return Some((
            OptimizerFamilyGroup::LocalControl,
            "op_local_get4_local_get4_i32_add",
            2,
        ));
    }
    if is_indexed_local_base_memory_family(op) {
        return Some((OptimizerFamilyGroup::Memory, "memory.indexed_local_base", 2));
    }
    if is_local_base_memory_family(op) {
        return Some((OptimizerFamilyGroup::Memory, "memory.local_base", 1));
    }
    if std::ptr::fn_addr_eq(op, vm::op_select4 as Op) {
        return Some((OptimizerFamilyGroup::CallSelect, "select.4", 1));
    }
    if std::ptr::fn_addr_eq(op, vm::op_select8 as Op) {
        return Some((OptimizerFamilyGroup::CallSelect, "select.8", 1));
    }
    if std::ptr::fn_addr_eq(op, vm::op_select16 as Op) {
        return Some((OptimizerFamilyGroup::CallSelect, "select.16", 1));
    }
    if std::ptr::fn_addr_eq(op, vm::op_select as Op) {
        return Some((OptimizerFamilyGroup::CallSelect, "select.generic", 0));
    }
    if std::ptr::fn_addr_eq(op, vm::op_call as Op)
        || std::ptr::fn_addr_eq(op, vm::op_call_import as Op)
    {
        return Some((OptimizerFamilyGroup::CallSelect, "call.direct", 0));
    }
    if std::ptr::fn_addr_eq(op, vm::op_return_call as Op)
        || std::ptr::fn_addr_eq(op, vm::op_return_call_import as Op)
    {
        return Some((OptimizerFamilyGroup::CallSelect, "call.return_direct", 0));
    }
    if std::ptr::fn_addr_eq(op, vm::op_call_indirect as Op) {
        return Some((OptimizerFamilyGroup::CallSelect, "call.indirect", 0));
    }
    if std::ptr::fn_addr_eq(op, vm::op_return_call_indirect as Op) {
        return Some((OptimizerFamilyGroup::CallSelect, "call.return_indirect", 0));
    }
    None
}

impl OptimizerProfiler {
    fn new(config: OptimizerProfileConfig, funcidx: FuncIdx, block_count: usize) -> Self {
        Self {
            config,
            funcidx,
            started_at: Instant::now(),
            total_iterations: 0,
            block_visits: vec![0; block_count],
            focus_logs_remaining: config.max_focus_logs,
        }
    }

    fn log_function_start(&self, program: &BasicBlockProgram) {
        eprintln!(
            "[telomere-opt-profile] func={} start blocks={} records={}",
            self.funcidx.0,
            program.blocks.len(),
            program.records.len(),
        );
    }

    fn before_block(&mut self, block: BasicBlock) -> Instant {
        self.total_iterations = self.total_iterations.saturating_add(1);
        self.block_visits[block.id] = self.block_visits[block.id].saturating_add(1);
        if self.config.verbose {
            eprintln!(
                "[telomere-opt-profile] func={} iter={} block={} visit={} start={} len={}",
                self.funcidx.0,
                self.total_iterations,
                block.id,
                self.block_visits[block.id],
                block.start,
                block.end.saturating_sub(block.start),
            );
        }
        Instant::now()
    }

    fn after_block(
        &self,
        block: BasicBlock,
        elapsed: Duration,
        entry_changed: bool,
        exit_changed: bool,
        expr_count: usize,
    ) {
        if self.config.verbose || elapsed >= self.config.slow_block_threshold {
            eprintln!(
                "[telomere-opt-profile] func={} block={} visit={} elapsed_ms={:.3} entry_changed={} exit_changed={} exprs={}",
                self.funcidx.0,
                block.id,
                self.block_visits[block.id],
                elapsed.as_secs_f64() * 1000.0,
                entry_changed,
                exit_changed,
                expr_count,
            );
        }
    }

    fn log_function_end(
        &self,
        rewrite: &FunctionRewrite,
        original_instrs: usize,
        packed: &super::sink::PackedOpStream,
    ) {
        let elapsed = self.started_at.elapsed();
        let mut ranked = self
            .block_visits
            .iter()
            .enumerate()
            .filter(|(_, visits)| **visits != 0)
            .collect::<Vec<_>>();
        ranked.sort_by_key(|(_, visits)| std::cmp::Reverse(**visits));
        let hottest = ranked
            .into_iter()
            .take(5)
            .map(|(block_id, visits)| format!("{block_id}:{visits}"))
            .collect::<Vec<_>>()
            .join(",");
        eprintln!(
            "[telomere-opt-profile] func={} done elapsed_ms={:.3} iterations={} exprs={} hot_blocks=[{}]",
            self.funcidx.0,
            elapsed.as_secs_f64() * 1000.0,
            self.total_iterations,
            rewrite.graph.nodes.len(),
            hottest,
        );
        let packed_instrs = packed.instr_len();
        eprintln!(
            "[telomere-opt-profile] func={} packed_instrs={} original_instrs={} growth_pct={:.2} budget_pct={} budget_abs={} within_budget={}",
            self.funcidx.0,
            packed_instrs,
            original_instrs,
            packed_stream_growth_pct(original_instrs, packed_instrs),
            PACKED_STREAM_GROWTH_BUDGET_PCT,
            PACKED_STREAM_GROWTH_BUDGET_ABS,
            packed_stream_within_budget(original_instrs, packed_instrs),
        );
        let grouped = collect_optimizer_family_candidates(&packed.ops);
        for (priority_rank, group) in OptimizerFamilyGroup::ORDER.iter().copied().enumerate() {
            let rendered = grouped[group.index()]
                .iter()
                .map(|(label, stat)| {
                    format!(
                        "{label}:count={},expected_provider_elims={}",
                        stat.count, stat.expected_provider_eliminations
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            eprintln!(
                "[telomere-opt-profile] func={} family_group={} priority_rank={} top_k={} candidates=[{}]",
                self.funcidx.0,
                group.label(),
                priority_rank,
                OPT_FAMILY_TOP_K,
                rendered,
            );
        }
    }

    fn should_log_focus(&self, block_id: usize) -> bool {
        self.focus_logs_remaining != 0
            && self.config.focus_func == Some(self.funcidx.0)
            && self.config.focus_block == Some(block_id)
    }

    fn log_state_diff(
        &mut self,
        label: &str,
        block_id: usize,
        graph: &ValueGraph,
        lhs: &BlockEntryState,
        rhs: &BlockEntryState,
    ) {
        if !self.should_log_focus(block_id) {
            return;
        }
        self.focus_logs_remaining -= 1;
        eprintln!(
            "[telomere-opt-profile] func={} block={} {label}-diff {}",
            self.funcidx.0,
            block_id,
            state_diff_summary(graph, lhs, rhs),
        );
    }
}

fn optimizer_profile_config() -> Option<OptimizerProfileConfig> {
    static CONFIG: OnceLock<Option<OptimizerProfileConfig>> = OnceLock::new();
    *CONFIG.get_or_init(OptimizerProfileConfig::from_env)
}

#[derive(Clone, Default)]
struct BlockBody {
    ops: Vec<BlockOp>,
    terminator: Option<BlockTerminator>,
}

#[derive(Clone)]
struct BlockOp {
    source_start: Option<usize>,
    op: Op,
    kind: BlockOpKind,
    operands: Vec<BlockOperand>,
    inputs: Vec<ValueRef>,
    values: Vec<ValueRef>,
}

#[derive(Clone)]
struct BlockTerminator {
    source_start: Option<usize>,
    op: Op,
    kind: BlockTerminatorKind,
    operands: Vec<BlockOperand>,
    inputs: Vec<ValueRef>,
    #[allow(dead_code)]
    values: Vec<ValueRef>,
}

#[derive(Clone, Copy)]
enum BlockOperand {
    I32(i32),
    I64(i64),
    F32(f32),
    F64(f64),
    U32(u32),
    LocalAddr(u32),
    SpillValue(ValueRef),
    JumpTarget(usize),
    Raw(Operand),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FusedOpKind {
    LocalGet4I32ConstAdd,
    LocalGet4I32ConstAddSet4,
    LocalGet4I32ConstAddTee4,
    LocalGet4LocalGet4I32Add,
    LocalGet4LocalGet4I32AddSet4,
    LocalGet4LocalGet4I32AddTee4,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BlockOpKind {
    Const,
    LocalGet,
    LocalSet,
    LocalTee,
    Drop,
    Select,
    PureUnary(PureOpKind),
    PureBinary(PureOpKind),
    GlobalGet,
    GlobalSet,
    TableGet,
    TableSet,
    MemoryLoad,
    MemoryStore,
    CallLike,
    Fused(FusedOpKind),
    Raw,
}

#[cfg(feature = "threads")]
#[derive(Clone, Copy)]
struct AtomicBarrierShape {
    input_count: usize,
}

#[derive(Clone, Copy)]
struct TrapSensitiveBarrierShape {
    input_count: usize,
    result_ty: ValType,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BlockTerminatorKind {
    If,
    Else,
    Br,
    BrIf,
    BrTable,
    Return,
    Loop,
    End,
    SpecialFunctionReturn,
    SpecialBlockReturn,
    Unreachable,
}

#[derive(Clone)]
enum PendingBlockEntryKind {
    Op(BlockOpKind),
    Terminator(BlockTerminatorKind),
}

#[derive(Clone)]
struct PendingBlockEntry {
    source_start: Option<usize>,
    op: Op,
    kind: PendingBlockEntryKind,
    operands: Vec<BlockOperand>,
    inputs: Vec<ValueRef>,
    alive: bool,
}

#[derive(Default, Clone)]
struct BlockBodyBuilder {
    entries: Vec<PendingBlockEntry>,
}

const UNKNOWN_HEAP_VERSION: u32 = u32::MAX;
const INSTR_RESULT_ORIGIN_STRIDE: usize = 256;
impl BlockBodyBuilder {
    fn push_raw(&mut self, source_start: Option<usize>, op: Op, operands: Vec<Operand>) -> usize {
        let idx = self.entries.len();
        self.entries.push(PendingBlockEntry {
            source_start,
            op,
            kind: classify_pending_entry_kind(op),
            operands: typed_operands_from_raw(op, &operands),
            inputs: Vec::new(),
            alive: true,
        });
        idx
    }

    fn push_spill_local_get(
        &mut self,
        source_start: Option<usize>,
        source: ValueRef,
        size: u32,
    ) -> usize {
        let idx = self.entries.len();
        let op = local_get_op(size);
        self.entries.push(PendingBlockEntry {
            source_start,
            op,
            kind: PendingBlockEntryKind::Op(BlockOpKind::LocalGet),
            operands: vec![BlockOperand::SpillValue(source)],
            inputs: Vec::new(),
            alive: true,
        });
        idx
    }

    fn remove(&mut self, idx: usize) {
        if let Some(entry) = self.entries.get_mut(idx) {
            entry.alive = false;
        }
    }

    fn last_alive_index(&self) -> Option<usize> {
        self.entries.iter().rposition(|entry| entry.alive)
    }

    fn entry_mut(&mut self, idx: usize) -> Option<&mut PendingBlockEntry> {
        self.entries.get_mut(idx)
    }

    fn live_entries(&self) -> impl Iterator<Item = (usize, &PendingBlockEntry)> {
        self.entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.alive)
    }
}

fn classify_pending_entry_kind(op: Op) -> PendingBlockEntryKind {
    if let Some(kind) = classify_terminator_kind(op) {
        return PendingBlockEntryKind::Terminator(kind);
    }
    PendingBlockEntryKind::Op(classify_block_op_kind(op))
}

fn classify_block_op_kind(op: Op) -> BlockOpKind {
    if std::ptr::fn_addr_eq(op, vm::op_i32_const as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i64_const as Op)
        || std::ptr::fn_addr_eq(op, vm::op_f32_const as Op)
        || std::ptr::fn_addr_eq(op, vm::op_f64_const as Op)
    {
        return BlockOpKind::Const;
    }
    if local_get_size_from_op(op).is_some() {
        return BlockOpKind::LocalGet;
    }
    if std::ptr::fn_addr_eq(op, vm::op_local_set4 as Op)
        || std::ptr::fn_addr_eq(op, vm::op_local_set8 as Op)
        || std::ptr::fn_addr_eq(op, vm::op_local_set16 as Op)
    {
        return BlockOpKind::LocalSet;
    }
    if std::ptr::fn_addr_eq(op, vm::op_local_tee4 as Op)
        || std::ptr::fn_addr_eq(op, vm::op_local_tee8 as Op)
        || std::ptr::fn_addr_eq(op, vm::op_local_tee16 as Op)
    {
        return BlockOpKind::LocalTee;
    }
    if std::ptr::fn_addr_eq(op, vm::op_drop as Op) {
        return BlockOpKind::Drop;
    }
    if std::ptr::fn_addr_eq(op, vm::op_select as Op) {
        return BlockOpKind::Select;
    }
    if let Some(kind) = pure_unary_kind_from_op(op) {
        return BlockOpKind::PureUnary(kind);
    }
    if let Some(kind) = pure_binary_kind_from_op(op) {
        return BlockOpKind::PureBinary(kind);
    }
    if std::ptr::fn_addr_eq(op, vm::op_global_get4 as Op)
        || std::ptr::fn_addr_eq(op, vm::op_global_get8 as Op)
        || std::ptr::fn_addr_eq(op, vm::op_global_get16 as Op)
    {
        return BlockOpKind::GlobalGet;
    }
    if std::ptr::fn_addr_eq(op, vm::op_global_set4 as Op)
        || std::ptr::fn_addr_eq(op, vm::op_global_set8 as Op)
        || std::ptr::fn_addr_eq(op, vm::op_global_set16 as Op)
    {
        return BlockOpKind::GlobalSet;
    }
    if std::ptr::fn_addr_eq(op, vm::op_table_get as Op) {
        return BlockOpKind::TableGet;
    }
    if std::ptr::fn_addr_eq(op, vm::op_table_set as Op) {
        return BlockOpKind::TableSet;
    }
    if is_memory_load_op(op) {
        return BlockOpKind::MemoryLoad;
    }
    if is_memory_store_op(op) {
        return BlockOpKind::MemoryStore;
    }
    if is_call_like_op(op) {
        return BlockOpKind::CallLike;
    }
    if std::ptr::fn_addr_eq(op, vm::op_local_get4_i32_const_add as Op) {
        return BlockOpKind::Fused(FusedOpKind::LocalGet4I32ConstAdd);
    }
    if std::ptr::fn_addr_eq(op, vm::op_local_get4_i32_const_add_set4 as Op) {
        return BlockOpKind::Fused(FusedOpKind::LocalGet4I32ConstAddSet4);
    }
    if std::ptr::fn_addr_eq(op, vm::op_local_get4_i32_const_add_tee4 as Op) {
        return BlockOpKind::Fused(FusedOpKind::LocalGet4I32ConstAddTee4);
    }
    if std::ptr::fn_addr_eq(op, vm::op_local_get4_local_get4_i32_add as Op) {
        return BlockOpKind::Fused(FusedOpKind::LocalGet4LocalGet4I32Add);
    }
    if std::ptr::fn_addr_eq(op, vm::op_local_get4_local_get4_i32_add_set4 as Op) {
        return BlockOpKind::Fused(FusedOpKind::LocalGet4LocalGet4I32AddSet4);
    }
    if std::ptr::fn_addr_eq(op, vm::op_local_get4_local_get4_i32_add_tee4 as Op) {
        return BlockOpKind::Fused(FusedOpKind::LocalGet4LocalGet4I32AddTee4);
    }
    BlockOpKind::Raw
}

fn classify_terminator_kind(op: Op) -> Option<BlockTerminatorKind> {
    if std::ptr::fn_addr_eq(op, vm::op_if as Op) {
        return Some(BlockTerminatorKind::If);
    }
    if std::ptr::fn_addr_eq(op, vm::op_else as Op) {
        return Some(BlockTerminatorKind::Else);
    }
    if std::ptr::fn_addr_eq(op, vm::op_br as Op) {
        return Some(BlockTerminatorKind::Br);
    }
    if std::ptr::fn_addr_eq(op, vm::op_br_if as Op) {
        return Some(BlockTerminatorKind::BrIf);
    }
    if std::ptr::fn_addr_eq(op, vm::op_local_get4_br_if as Op)
        || std::ptr::fn_addr_eq(op, vm::op_local_get4_i32_const_add_br_if as Op)
        || std::ptr::fn_addr_eq(op, vm::op_local_get4_local_get4_i32_add_br_if as Op)
        || std::ptr::fn_addr_eq(op, vm::op_local_get4_i32_eqz_br_if as Op)
        || std::ptr::fn_addr_eq(op, vm::op_local_get4_i32_const_compare_br_if as Op)
        || std::ptr::fn_addr_eq(op, vm::op_local_get4_local_get4_compare_br_if as Op)
        || std::ptr::fn_addr_eq(op, vm::op_local_get4_i32_const_add_tee4_br_if as Op)
    {
        return Some(BlockTerminatorKind::BrIf);
    }
    if std::ptr::fn_addr_eq(op, vm::op_br_table as Op) {
        return Some(BlockTerminatorKind::BrTable);
    }
    if std::ptr::fn_addr_eq(op, vm::op_return as Op) {
        return Some(BlockTerminatorKind::Return);
    }
    if std::ptr::fn_addr_eq(op, vm::op_loop as Op) {
        return Some(BlockTerminatorKind::Loop);
    }
    if std::ptr::fn_addr_eq(op, vm::op_end as Op) {
        return Some(BlockTerminatorKind::End);
    }
    if std::ptr::fn_addr_eq(op, vm::special_function_return as Op) {
        return Some(BlockTerminatorKind::SpecialFunctionReturn);
    }
    if std::ptr::fn_addr_eq(op, vm::special_block_return as Op) {
        return Some(BlockTerminatorKind::SpecialBlockReturn);
    }
    if std::ptr::fn_addr_eq(op, vm::op_unreachable as Op) {
        return Some(BlockTerminatorKind::Unreachable);
    }
    None
}

fn typed_operands_from_raw(op: Op, operands: &[Operand]) -> Vec<BlockOperand> {
    if std::ptr::fn_addr_eq(op, vm::op_i32_const as Op) {
        return vec![BlockOperand::I32(unsafe { operands[0].i32 })];
    }
    if std::ptr::fn_addr_eq(op, vm::op_i64_const as Op) {
        return vec![BlockOperand::I64(unsafe { operands[0].i64 })];
    }
    if std::ptr::fn_addr_eq(op, vm::op_f32_const as Op) {
        return vec![BlockOperand::F32(unsafe { operands[0].f32 })];
    }
    if std::ptr::fn_addr_eq(op, vm::op_f64_const as Op) {
        return vec![BlockOperand::F64(unsafe { operands[0].f64 })];
    }
    if local_get_size_from_op(op).is_some()
        || std::ptr::fn_addr_eq(op, vm::op_local_set4 as Op)
        || std::ptr::fn_addr_eq(op, vm::op_local_set8 as Op)
        || std::ptr::fn_addr_eq(op, vm::op_local_set16 as Op)
        || std::ptr::fn_addr_eq(op, vm::op_local_tee4 as Op)
        || std::ptr::fn_addr_eq(op, vm::op_local_tee8 as Op)
        || std::ptr::fn_addr_eq(op, vm::op_local_tee16 as Op)
        || std::ptr::fn_addr_eq(op, vm::op_local_get4_i32_const_add as Op)
    {
        return operands
            .iter()
            .map(|operand| BlockOperand::LocalAddr(unsafe { operand.local_addr }))
            .collect();
    }
    if std::ptr::fn_addr_eq(op, vm::op_if as Op)
        || std::ptr::fn_addr_eq(op, vm::op_else as Op)
        || std::ptr::fn_addr_eq(op, vm::op_br as Op)
        || std::ptr::fn_addr_eq(op, vm::op_br_if as Op)
        || std::ptr::fn_addr_eq(op, vm::op_return as Op)
    {
        return vec![BlockOperand::JumpTarget(unsafe {
            operands[0].jump_addr as usize
        })];
    }
    if std::ptr::fn_addr_eq(op, vm::op_br_table as Op) {
        let mut out = Vec::with_capacity(operands.len());
        out.push(BlockOperand::U32(unsafe { operands[0].u32 }));
        out.extend(
            operands[1..]
                .iter()
                .map(|operand| BlockOperand::JumpTarget(unsafe { operand.jump_addr as usize })),
        );
        return out;
    }
    if std::ptr::fn_addr_eq(op, vm::op_local_get4_br_if as Op) {
        return vec![
            BlockOperand::JumpTarget(unsafe { operands[1].jump_addr as usize }),
            BlockOperand::LocalAddr(unsafe { operands[0].local_addr }),
        ];
    }
    if std::ptr::fn_addr_eq(op, vm::op_local_get4_i32_const_add_br_if as Op) {
        return vec![
            BlockOperand::JumpTarget(unsafe { operands[2].jump_addr as usize }),
            BlockOperand::LocalAddr(unsafe { operands[0].local_addr }),
            BlockOperand::I32(unsafe { operands[1].i32 }),
        ];
    }
    if std::ptr::fn_addr_eq(op, vm::op_local_get4_local_get4_i32_add_br_if as Op) {
        return vec![
            BlockOperand::JumpTarget(unsafe { operands[2].jump_addr as usize }),
            BlockOperand::LocalAddr(unsafe { operands[0].local_addr }),
            BlockOperand::LocalAddr(unsafe { operands[1].local_addr }),
        ];
    }
    if std::ptr::fn_addr_eq(op, vm::op_local_get4_i32_eqz_br_if as Op) {
        return vec![
            BlockOperand::JumpTarget(unsafe { operands[1].jump_addr as usize }),
            BlockOperand::LocalAddr(unsafe { operands[0].local_addr }),
        ];
    }
    if std::ptr::fn_addr_eq(op, vm::op_local_get4_i32_const_compare_br_if as Op) {
        return vec![
            BlockOperand::JumpTarget(unsafe { operands[3].jump_addr as usize }),
            BlockOperand::LocalAddr(unsafe { operands[0].local_addr }),
            BlockOperand::U32(unsafe { operands[1].u32 }),
            BlockOperand::I32(unsafe { operands[2].i32 }),
        ];
    }
    if std::ptr::fn_addr_eq(op, vm::op_local_get4_local_get4_compare_br_if as Op) {
        return vec![
            BlockOperand::JumpTarget(unsafe { operands[3].jump_addr as usize }),
            BlockOperand::LocalAddr(unsafe { operands[0].local_addr }),
            BlockOperand::LocalAddr(unsafe { operands[1].local_addr }),
            BlockOperand::U32(unsafe { operands[2].u32 }),
        ];
    }
    if std::ptr::fn_addr_eq(op, vm::op_local_get4_i32_const_add_tee4_br_if as Op) {
        return vec![
            BlockOperand::JumpTarget(unsafe { operands[3].jump_addr as usize }),
            BlockOperand::LocalAddr(unsafe { operands[0].local_addr }),
            BlockOperand::I32(unsafe { operands[1].i32 }),
            BlockOperand::LocalAddr(unsafe { operands[2].local_addr }),
        ];
    }
    if std::ptr::fn_addr_eq(op, vm::op_global_get4 as Op)
        || std::ptr::fn_addr_eq(op, vm::op_global_get8 as Op)
        || std::ptr::fn_addr_eq(op, vm::op_global_get16 as Op)
        || std::ptr::fn_addr_eq(op, vm::op_global_set4 as Op)
        || std::ptr::fn_addr_eq(op, vm::op_global_set8 as Op)
        || std::ptr::fn_addr_eq(op, vm::op_global_set16 as Op)
        || std::ptr::fn_addr_eq(op, vm::op_table_get as Op)
        || std::ptr::fn_addr_eq(op, vm::op_table_set as Op)
        || is_call_like_op(op)
    {
        return operands
            .iter()
            .map(|operand| BlockOperand::U32(unsafe { operand.u32 }))
            .collect();
    }
    operands.iter().copied().map(BlockOperand::Raw).collect()
}

fn block_operand_to_raw_for_op(op: Op, operand: &BlockOperand) -> Operand {
    match operand {
        BlockOperand::I32(value) => Operand { i32: *value },
        BlockOperand::I64(value) => Operand { i64: *value },
        BlockOperand::F32(value) => Operand { f32: *value },
        BlockOperand::F64(value) => Operand { f64: *value },
        BlockOperand::U32(value) if is_direct_call_op(op) => Operand {
            call_recipe_ref: CallRecipeRef::from_funcidx(*value),
        },
        BlockOperand::U32(value) => Operand { u32: *value },
        BlockOperand::LocalAddr(value) => Operand { local_addr: *value },
        BlockOperand::SpillValue(_) => {
            unreachable!("spill placeholders must be resolved before raw lowering")
        }
        BlockOperand::JumpTarget(value) => Operand {
            jump_addr: *value as u32,
        },
        BlockOperand::Raw(operand) => *operand,
    }
}

fn block_operands_to_raw_with_spills_for_op(
    op: Op,
    operands: &[BlockOperand],
    spill_plan: &EffectResultSpillPlan,
) -> Vec<Operand> {
    operands
        .iter()
        .map(|operand| match operand {
            BlockOperand::SpillValue(value) => {
                let slot = spill_plan
                    .slot(*value)
                    .expect("spill placeholder must have an assigned temp local");
                Operand {
                    local_addr: slot.addr,
                }
            }
            _ => block_operand_to_raw_for_op(op, operand),
        })
        .collect()
}

fn pure_unary_kind_from_op(op: Op) -> Option<PureOpKind> {
    if std::ptr::fn_addr_eq(op, vm::op_i32_eqz as Op) {
        return Some(PureOpKind::I32Eqz);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i32_clz as Op) {
        return Some(PureOpKind::I32Clz);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i32_ctz as Op) {
        return Some(PureOpKind::I32Ctz);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i32_popcnt as Op) {
        return Some(PureOpKind::I32Popcnt);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i64_eqz as Op) {
        return Some(PureOpKind::I64Eqz);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i64_clz as Op) {
        return Some(PureOpKind::I64Clz);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i64_ctz as Op) {
        return Some(PureOpKind::I64Ctz);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i64_popcnt as Op) {
        return Some(PureOpKind::I64Popcnt);
    }
    if std::ptr::fn_addr_eq(op, vm::op_f32_abs as Op) {
        return Some(PureOpKind::F32Abs);
    }
    if std::ptr::fn_addr_eq(op, vm::op_f32_neg as Op) {
        return Some(PureOpKind::F32Neg);
    }
    if std::ptr::fn_addr_eq(op, vm::op_f32_sqrt as Op) {
        return Some(PureOpKind::F32Sqrt);
    }
    if std::ptr::fn_addr_eq(op, vm::op_f32_ceil as Op) {
        return Some(PureOpKind::F32Ceil);
    }
    if std::ptr::fn_addr_eq(op, vm::op_f32_floor as Op) {
        return Some(PureOpKind::F32Floor);
    }
    if std::ptr::fn_addr_eq(op, vm::op_f32_trunc as Op) {
        return Some(PureOpKind::F32Trunc);
    }
    if std::ptr::fn_addr_eq(op, vm::op_f32_nearest as Op) {
        return Some(PureOpKind::F32Nearest);
    }
    if std::ptr::fn_addr_eq(op, vm::op_f64_abs as Op) {
        return Some(PureOpKind::F64Abs);
    }
    if std::ptr::fn_addr_eq(op, vm::op_f64_neg as Op) {
        return Some(PureOpKind::F64Neg);
    }
    if std::ptr::fn_addr_eq(op, vm::op_f64_sqrt as Op) {
        return Some(PureOpKind::F64Sqrt);
    }
    if std::ptr::fn_addr_eq(op, vm::op_f64_ceil as Op) {
        return Some(PureOpKind::F64Ceil);
    }
    if std::ptr::fn_addr_eq(op, vm::op_f64_floor as Op) {
        return Some(PureOpKind::F64Floor);
    }
    if std::ptr::fn_addr_eq(op, vm::op_f64_trunc as Op) {
        return Some(PureOpKind::F64Trunc);
    }
    if std::ptr::fn_addr_eq(op, vm::op_f64_nearest as Op) {
        return Some(PureOpKind::F64Nearest);
    }
    None
}

fn pure_binary_kind_from_op(op: Op) -> Option<PureOpKind> {
    if std::ptr::fn_addr_eq(op, vm::op_i32_add as Op) {
        return Some(PureOpKind::I32Add);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i32_sub as Op) {
        return Some(PureOpKind::I32Sub);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i32_mul as Op) {
        return Some(PureOpKind::I32Mul);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i32_and as Op) {
        return Some(PureOpKind::I32And);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i32_or as Op) {
        return Some(PureOpKind::I32Or);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i32_xor as Op) {
        return Some(PureOpKind::I32Xor);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i32_shl as Op) {
        return Some(PureOpKind::I32Shl);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i32_shr_s as Op) {
        return Some(PureOpKind::I32ShrS);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i32_shr_u as Op) {
        return Some(PureOpKind::I32ShrU);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i32_rotl as Op) {
        return Some(PureOpKind::I32Rotl);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i32_rotr as Op) {
        return Some(PureOpKind::I32Rotr);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i32_eq as Op) {
        return Some(PureOpKind::I32Eq);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i32_ne as Op) {
        return Some(PureOpKind::I32Ne);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i32_lt_s as Op) {
        return Some(PureOpKind::I32LtS);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i32_lt_u as Op) {
        return Some(PureOpKind::I32LtU);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i32_gt_s as Op) {
        return Some(PureOpKind::I32GtS);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i32_gt_u as Op) {
        return Some(PureOpKind::I32GtU);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i32_le_s as Op) {
        return Some(PureOpKind::I32LeS);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i32_le_u as Op) {
        return Some(PureOpKind::I32LeU);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i32_ge_s as Op) {
        return Some(PureOpKind::I32GeS);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i32_ge_u as Op) {
        return Some(PureOpKind::I32GeU);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i64_add as Op) {
        return Some(PureOpKind::I64Add);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i64_sub as Op) {
        return Some(PureOpKind::I64Sub);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i64_mul as Op) {
        return Some(PureOpKind::I64Mul);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i64_and as Op) {
        return Some(PureOpKind::I64And);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i64_or as Op) {
        return Some(PureOpKind::I64Or);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i64_xor as Op) {
        return Some(PureOpKind::I64Xor);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i64_shl as Op) {
        return Some(PureOpKind::I64Shl);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i64_shr_s as Op) {
        return Some(PureOpKind::I64ShrS);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i64_shr_u as Op) {
        return Some(PureOpKind::I64ShrU);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i64_rotl as Op) {
        return Some(PureOpKind::I64Rotl);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i64_rotr as Op) {
        return Some(PureOpKind::I64Rotr);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i64_eq as Op) {
        return Some(PureOpKind::I64Eq);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i64_ne as Op) {
        return Some(PureOpKind::I64Ne);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i64_lt_s as Op) {
        return Some(PureOpKind::I64LtS);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i64_lt_u as Op) {
        return Some(PureOpKind::I64LtU);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i64_gt_s as Op) {
        return Some(PureOpKind::I64GtS);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i64_gt_u as Op) {
        return Some(PureOpKind::I64GtU);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i64_le_s as Op) {
        return Some(PureOpKind::I64LeS);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i64_le_u as Op) {
        return Some(PureOpKind::I64LeU);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i64_ge_s as Op) {
        return Some(PureOpKind::I64GeS);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i64_ge_u as Op) {
        return Some(PureOpKind::I64GeU);
    }
    if std::ptr::fn_addr_eq(op, vm::op_f32_add as Op) {
        return Some(PureOpKind::F32Add);
    }
    if std::ptr::fn_addr_eq(op, vm::op_f32_sub as Op) {
        return Some(PureOpKind::F32Sub);
    }
    if std::ptr::fn_addr_eq(op, vm::op_f32_mul as Op) {
        return Some(PureOpKind::F32Mul);
    }
    if std::ptr::fn_addr_eq(op, vm::op_f32_div as Op) {
        return Some(PureOpKind::F32Div);
    }
    if std::ptr::fn_addr_eq(op, vm::op_f32_eq as Op) {
        return Some(PureOpKind::F32Eq);
    }
    if std::ptr::fn_addr_eq(op, vm::op_f32_ne as Op) {
        return Some(PureOpKind::F32Ne);
    }
    if std::ptr::fn_addr_eq(op, vm::op_f32_lt as Op) {
        return Some(PureOpKind::F32Lt);
    }
    if std::ptr::fn_addr_eq(op, vm::op_f32_gt as Op) {
        return Some(PureOpKind::F32Gt);
    }
    if std::ptr::fn_addr_eq(op, vm::op_f32_le as Op) {
        return Some(PureOpKind::F32Le);
    }
    if std::ptr::fn_addr_eq(op, vm::op_f32_ge as Op) {
        return Some(PureOpKind::F32Ge);
    }
    if std::ptr::fn_addr_eq(op, vm::op_f64_add as Op) {
        return Some(PureOpKind::F64Add);
    }
    if std::ptr::fn_addr_eq(op, vm::op_f64_sub as Op) {
        return Some(PureOpKind::F64Sub);
    }
    if std::ptr::fn_addr_eq(op, vm::op_f64_mul as Op) {
        return Some(PureOpKind::F64Mul);
    }
    if std::ptr::fn_addr_eq(op, vm::op_f64_div as Op) {
        return Some(PureOpKind::F64Div);
    }
    if std::ptr::fn_addr_eq(op, vm::op_f64_eq as Op) {
        return Some(PureOpKind::F64Eq);
    }
    if std::ptr::fn_addr_eq(op, vm::op_f64_ne as Op) {
        return Some(PureOpKind::F64Ne);
    }
    if std::ptr::fn_addr_eq(op, vm::op_f64_lt as Op) {
        return Some(PureOpKind::F64Lt);
    }
    if std::ptr::fn_addr_eq(op, vm::op_f64_gt as Op) {
        return Some(PureOpKind::F64Gt);
    }
    if std::ptr::fn_addr_eq(op, vm::op_f64_le as Op) {
        return Some(PureOpKind::F64Le);
    }
    if std::ptr::fn_addr_eq(op, vm::op_f64_ge as Op) {
        return Some(PureOpKind::F64Ge);
    }
    None
}

fn is_memory_load_op(op: Op) -> bool {
    std::ptr::fn_addr_eq(op, vm::op_i32_load as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i32_load8_s as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i32_load8_u as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i32_load16_s as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i32_load16_u as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i32_load_shared as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i32_load8_s_shared as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i32_load8_u_shared as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i32_load16_s_shared as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i32_load16_u_shared as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i32_load_indexed_local as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i32_load8_s_indexed_local as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i32_load8_u_indexed_local as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i32_load16_s_indexed_local as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i32_load16_u_indexed_local as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i32_load_indexed_shared as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i32_load8_s_indexed_shared as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i32_load8_u_indexed_shared as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i32_load16_s_indexed_shared as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i32_load16_u_indexed_shared as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i32_load_local as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i32_load8_s_local as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i32_load8_u_local as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i32_load16_s_local as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i32_load16_u_local as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i64_load as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i64_load8_s as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i64_load8_u as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i64_load16_s as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i64_load16_u as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i64_load32_s as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i64_load32_u as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i64_load_shared as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i64_load8_s_shared as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i64_load8_u_shared as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i64_load16_s_shared as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i64_load16_u_shared as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i64_load32_s_shared as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i64_load32_u_shared as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i64_load_indexed_local as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i64_load8_s_indexed_local as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i64_load8_u_indexed_local as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i64_load16_s_indexed_local as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i64_load16_u_indexed_local as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i64_load32_s_indexed_local as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i64_load32_u_indexed_local as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i64_load_indexed_shared as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i64_load8_s_indexed_shared as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i64_load8_u_indexed_shared as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i64_load16_s_indexed_shared as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i64_load16_u_indexed_shared as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i64_load32_s_indexed_shared as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i64_load32_u_indexed_shared as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i64_load_local as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i64_load8_s_local as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i64_load8_u_local as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i64_load16_s_local as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i64_load16_u_local as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i64_load32_s_local as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i64_load32_u_local as Op)
        || std::ptr::fn_addr_eq(op, vm::op_f32_load as Op)
        || std::ptr::fn_addr_eq(op, vm::op_f32_load_shared as Op)
        || std::ptr::fn_addr_eq(op, vm::op_f32_load_indexed_local as Op)
        || std::ptr::fn_addr_eq(op, vm::op_f32_load_indexed_shared as Op)
        || std::ptr::fn_addr_eq(op, vm::op_f32_load_local as Op)
        || std::ptr::fn_addr_eq(op, vm::op_f64_load as Op)
        || std::ptr::fn_addr_eq(op, vm::op_f64_load_shared as Op)
        || std::ptr::fn_addr_eq(op, vm::op_f64_load_indexed_local as Op)
        || std::ptr::fn_addr_eq(op, vm::op_f64_load_indexed_shared as Op)
        || std::ptr::fn_addr_eq(op, vm::op_f64_load_local as Op)
}

fn is_memory_store_op(op: Op) -> bool {
    std::ptr::fn_addr_eq(op, vm::op_i32_store as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i32_store8 as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i32_store16 as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i32_store_shared as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i32_store8_shared as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i32_store16_shared as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i32_store_indexed_local as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i32_store8_indexed_local as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i32_store16_indexed_local as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i32_store_indexed_shared as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i32_store8_indexed_shared as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i32_store16_indexed_shared as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i32_store_local as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i32_store8_local as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i32_store16_local as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i64_store as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i64_store8 as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i64_store16 as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i64_store32 as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i64_store_shared as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i64_store8_shared as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i64_store16_shared as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i64_store32_shared as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i64_store_indexed_local as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i64_store8_indexed_local as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i64_store16_indexed_local as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i64_store32_indexed_local as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i64_store_indexed_shared as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i64_store8_indexed_shared as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i64_store16_indexed_shared as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i64_store32_indexed_shared as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i64_store_local as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i64_store8_local as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i64_store16_local as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i64_store32_local as Op)
        || std::ptr::fn_addr_eq(op, vm::op_f32_store as Op)
        || std::ptr::fn_addr_eq(op, vm::op_f32_store_shared as Op)
        || std::ptr::fn_addr_eq(op, vm::op_f32_store_indexed_local as Op)
        || std::ptr::fn_addr_eq(op, vm::op_f32_store_indexed_shared as Op)
        || std::ptr::fn_addr_eq(op, vm::op_f32_store_local as Op)
        || std::ptr::fn_addr_eq(op, vm::op_f64_store as Op)
        || std::ptr::fn_addr_eq(op, vm::op_f64_store_shared as Op)
        || std::ptr::fn_addr_eq(op, vm::op_f64_store_indexed_local as Op)
        || std::ptr::fn_addr_eq(op, vm::op_f64_store_indexed_shared as Op)
        || std::ptr::fn_addr_eq(op, vm::op_f64_store_local as Op)
}

fn is_call_like_op(op: Op) -> bool {
    is_direct_call_op(op)
        || std::ptr::fn_addr_eq(op, vm::op_call_indirect as Op)
        || std::ptr::fn_addr_eq(op, vm::op_return_call_indirect as Op)
}

fn is_direct_call_op(op: Op) -> bool {
    std::ptr::fn_addr_eq(op, vm::op_call as Op)
        || std::ptr::fn_addr_eq(op, vm::op_call_import as Op)
        || std::ptr::fn_addr_eq(op, vm::op_return_call as Op)
        || std::ptr::fn_addr_eq(op, vm::op_return_call_import as Op)
}

pub(crate) fn optimize_function(
    funcidx: FuncIdx,
    functype: &FuncType,
    locals: &mut LocalsData,
    instrs: Vec<Instr>,
    meta: Vec<InstructionMeta>,
) -> OptimizedFunction {
    let fallback_op_lens = meta
        .iter()
        .map(|entry| u16::try_from(entry.len).expect("instruction length exceeds u16::MAX"))
        .collect();
    let param_bytes = functype
        .0
        .iter()
        .map(|ty| ty.stack_size().u32())
        .sum::<u32>();
    let Some(program) = build_program(&instrs, meta) else {
        return OptimizedFunction {
            instrs,
            op_lens: fallback_op_lens,
        };
    };
    let mut profiler = optimizer_profile_config()
        .map(|config| OptimizerProfiler::new(config, funcidx, program.blocks.len()));
    if let Some(profiler) = profiler.as_ref() {
        profiler.log_function_start(&program);
    }
    let mut rewrite = rewrite_program(&program, profiler.as_mut());
    debug_assert!(verify_explicit_effect_ir(
        &program,
        &rewrite.relower.block_bodies
    ));
    apply_licm(&program, &mut rewrite, locals, param_bytes);
    debug_assert!(verify_explicit_effect_ir(
        &program,
        &rewrite.relower.block_bodies
    ));
    let reachable = reachable_blocks(&program, &rewrite.relower.block_bodies);
    let spill_plan = build_effect_result_spill_plan(
        &rewrite.graph,
        &rewrite.relower.block_bodies,
        &reachable,
        locals,
        param_bytes,
    );
    debug_assert!(verify_effect_result_spill_ir(
        &rewrite.graph,
        &rewrite.relower.block_bodies,
        &spill_plan,
    ));
    let mut packed_ops = Vec::new();
    if !rewrite.relower.entry_prefix_ops.is_empty() {
        packed_ops.extend(relower_block_body(
            &BlockBody {
                ops: rewrite.relower.entry_prefix_ops.clone(),
                terminator: None,
            },
            &rewrite.graph,
            &spill_plan,
        ));
    }
    for block in &program.blocks {
        if reachable[block.id] {
            if let Some(loop_header) = program
                .records
                .get(block.start)
                .filter(|record| record.op_eq(vm::op_loop))
            {
                packed_ops.push(pack_op(
                    Some(loop_header.old_start),
                    loop_header.op,
                    &loop_header.operands,
                ));
            }
            packed_ops.extend(relower_block_body(
                &rewrite.relower.block_bodies[block.id],
                &rewrite.graph,
                &spill_plan,
            ));
        }
    }
    debug_assert!(verify_relower_preserves_call_ops(
        &program,
        &rewrite.relower.block_bodies,
        &packed_ops,
    ));
    debug_assert!(verify_relower_preserves_effect_result_spills(
        &rewrite.graph,
        &rewrite.relower.block_bodies,
        &spill_plan,
        &packed_ops,
    ));
    if patch_packed_jump_targets(&mut packed_ops).is_err() {
        return OptimizedFunction {
            instrs,
            op_lens: fallback_op_lens,
        };
    }
    let packed = build_packed_stream(packed_ops);
    debug_assert!(verify_packed_stream(&packed));
    if let Some(profiler) = profiler.as_ref() {
        profiler.log_function_end(&rewrite, instrs.len(), &packed);
    }
    if !packed_stream_within_budget(instrs.len(), packed.instr_len()) {
        return OptimizedFunction {
            instrs,
            op_lens: fallback_op_lens,
        };
    }
    let op_lens = packed
        .ops
        .iter()
        .map(|op| u16::try_from(op.len()).expect("packed instruction length exceeds u16::MAX"))
        .collect();
    OptimizedFunction {
        instrs: flatten_packed_stream(&packed),
        op_lens,
    }
}

fn rewrite_program(
    program: &BasicBlockProgram,
    mut profiler: Option<&mut OptimizerProfiler>,
) -> FunctionRewrite {
    let mut pass = BlockOptimizer::default();
    let mut rewrite = FunctionRewrite {
        entries: vec![BlockEntryState::default(); program.blocks.len()],
        exits: vec![BlockEntryState::default(); program.blocks.len()],
        relower: RelowerPlan {
            block_bodies: vec![BlockBody::default(); program.blocks.len()],
            loop_invariants: vec![LoopInvariantSet::default(); program.blocks.len()],
            block_copy_plans: vec![BlockCopyPlan::default(); program.blocks.len()],
            entry_prefix_ops: Vec::new(),
        },
        graph: ValueGraph::default(),
    };
    let mut worklist = VecDeque::new();
    let mut queued = vec![false; program.blocks.len()];
    worklist.push_back(0usize);
    queued[0] = true;

    while let Some(block_id) = worklist.pop_front() {
        queued[block_id] = false;
        let Some((entry, copy_plan)) =
            compute_entry_state(program, &mut pass.exprs, &rewrite, block_id)
        else {
            if clear_block_rewrite(&pass.exprs, &mut rewrite, block_id) {
                enqueue_successors(program, block_id, &mut worklist, &mut queued);
            }
            continue;
        };
        let entry_changed = !same_state(&pass.exprs, &rewrite.entries[block_id], &entry);
        if entry_changed {
            if let Some(profiler) = profiler.as_deref_mut() {
                profiler.log_state_diff(
                    "entry",
                    block_id,
                    &pass.exprs,
                    &rewrite.entries[block_id],
                    &entry,
                );
            }
        }
        if entry_changed {
            rewrite.entries[block_id] = entry.clone();
        }
        rewrite.relower.block_copy_plans[block_id] = copy_plan.clone();
        pass.current_copy_plan = copy_plan;
        let block = program.block(block_id);
        let block_started_at = profiler
            .as_deref_mut()
            .map(|profiler| profiler.before_block(block));
        let result = pass.run_block(program, block, &entry);
        let exit_changed = !same_state(&pass.exprs, &rewrite.exits[block_id], &result.exit);
        if exit_changed {
            if let Some(profiler) = profiler.as_deref_mut() {
                profiler.log_state_diff(
                    "exit",
                    block_id,
                    &pass.exprs,
                    &rewrite.exits[block_id],
                    &result.exit,
                );
            }
        }
        if exit_changed {
            rewrite.exits[block_id] = result.exit;
        }
        if let (Some(profiler), Some(block_started_at)) = (profiler.as_deref(), block_started_at) {
            profiler.after_block(
                block,
                block_started_at.elapsed(),
                entry_changed,
                exit_changed,
                pass.exprs.nodes.len(),
            );
        }
        rewrite.relower.block_bodies[block_id] = result.body;
        rewrite.relower.loop_invariants[block_id] = result.loop_invariants;
        if entry_changed || exit_changed {
            enqueue_successors(program, block_id, &mut worklist, &mut queued);
        }
    }

    rewrite.graph = pass.exprs;
    debug_assert!(verify_slot_plan(program, &rewrite));
    rewrite
}

fn compute_entry_state(
    program: &BasicBlockProgram,
    graph: &mut ValueGraph,
    rewrite: &FunctionRewrite,
    block_id: usize,
) -> Option<(BlockEntryState, BlockCopyPlan)> {
    let block = program.block(block_id);
    let first = program.records.get(block.start)?;
    let mut incoming = Vec::new();
    if block_id == 0 {
        incoming.push(default_entry_state(graph, block_id, first));
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
    Some(merge_states_with_copy_plan(
        graph, block_id, first, &incoming,
    ))
}

fn clear_block_rewrite(graph: &ValueGraph, rewrite: &mut FunctionRewrite, block_id: usize) -> bool {
    let entry_changed = !same_state(
        graph,
        &rewrite.entries[block_id],
        &BlockEntryState::default(),
    );
    let exit_changed = !same_state(graph, &rewrite.exits[block_id], &BlockEntryState::default());
    let body_changed = !block_body_is_empty(&rewrite.relower.block_bodies[block_id]);
    let invariants_changed =
        rewrite.relower.loop_invariants[block_id] != LoopInvariantSet::default();
    let copy_plan_changed = rewrite.relower.block_copy_plans[block_id] != BlockCopyPlan::default();
    if entry_changed {
        rewrite.entries[block_id] = BlockEntryState::default();
    }
    if exit_changed {
        rewrite.exits[block_id] = BlockEntryState::default();
    }
    if body_changed {
        rewrite.relower.block_bodies[block_id] = BlockBody::default();
    }
    if invariants_changed {
        rewrite.relower.loop_invariants[block_id] = LoopInvariantSet::default();
    }
    if copy_plan_changed {
        rewrite.relower.block_copy_plans[block_id] = BlockCopyPlan::default();
    }
    entry_changed || exit_changed || body_changed || invariants_changed || copy_plan_changed
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

fn default_entry_state(
    graph: &mut ValueGraph,
    block_id: usize,
    first: &DecodedInstr,
) -> BlockEntryState {
    BlockEntryState {
        reachable: true,
        stack: first
            .stack_before
            .types
            .iter()
            .enumerate()
            .map(|(ordinal, ty)| {
                ensure_seed_value(
                    graph,
                    *ty,
                    ExprOrigin {
                        block_id,
                        ordinal,
                        kind: ExprOriginKind::EntryStack,
                    },
                )
            })
            .collect(),
        ..BlockEntryState::default()
    }
}

#[cfg_attr(not(test), allow(dead_code))]
fn merge_states(
    graph: &mut ValueGraph,
    block_id: usize,
    first: &DecodedInstr,
    incoming: &[BlockEntryState],
) -> BlockEntryState {
    merge_states_with_copy_plan(graph, block_id, first, incoming).0
}

fn merge_states_with_copy_plan(
    graph: &mut ValueGraph,
    block_id: usize,
    first: &DecodedInstr,
    incoming: &[BlockEntryState],
) -> (BlockEntryState, BlockCopyPlan) {
    let preserve_existing_block_arguments = incoming.len() > 1;
    let mut state = BlockEntryState {
        reachable: true,
        stack: Vec::with_capacity(first.stack_before.types.len()),
        heap: merge_heap_versions(incoming),
        ..BlockEntryState::default()
    };
    let mut copy_plan = BlockCopyPlan::default();

    for (ordinal, ty) in first.stack_before.types.iter().enumerate() {
        let values = incoming
            .iter()
            .map(|entry| entry.stack.get(ordinal))
            .collect::<Vec<_>>();
        let merged = merge_value_candidates(
            graph,
            block_id,
            ordinal,
            *ty,
            &values,
            preserve_existing_block_arguments,
        );
        if let Some(decision) = slot_merge_decision(graph, &values, merged) {
            if decision == SlotMergeDecision::InsertCopy {
                set_value_slot_shape(graph, merged, None, None, None);
            }
            copy_plan.stack.insert(ordinal, decision);
        }
        state.stack.push(merged);
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
        let merged_ty = values
            .iter()
            .flatten()
            .next()
            .map(|value| graph[value.0].ty)
            .unwrap_or_else(|| type_from_slot(slot.size));
        let merged = merge_value_candidates(
            graph,
            block_id,
            1024 + slot.addr as usize,
            merged_ty,
            &values,
            preserve_existing_block_arguments,
        );
        if graph[merged.0].is_block_argument() {
            set_direct_slot_shape(graph, merged, slot_ref_for_local_slot(slot));
            copy_plan.locals.insert(slot, SlotMergeDecision::Preserve);
        }
        state.locals.insert(slot, merged);
    }

    merge_aliases(
        graph,
        block_id,
        incoming,
        &mut state,
        &mut copy_plan,
        preserve_existing_block_arguments,
    );

    (state, copy_plan)
}

fn merge_aliases(
    graph: &mut ValueGraph,
    block_id: usize,
    incoming: &[BlockEntryState],
    state: &mut BlockEntryState,
    copy_plan: &mut BlockCopyPlan,
    preserve_existing_block_arguments: bool,
) {
    let mut exact_keys = if let Some(first_entry) = incoming.first() {
        first_entry.aliases.keys().cloned().collect::<BTreeSet<_>>()
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
            graph,
            block_id,
            key.clone(),
            incoming
                .iter()
                .map(|entry| entry.aliases.get(&key))
                .collect::<Vec<_>>(),
            state,
            copy_plan,
            preserve_existing_block_arguments,
        );
    }

    let mut join_keys = BTreeSet::new();
    for entry in incoming {
        for key in entry.aliases.keys() {
            if let Some(join_key) = join_alias_key(key) {
                join_keys.insert(join_key);
            }
        }
    }
    for join_key in join_keys {
        if !space_version_stable(join_key.space, incoming, state.heap) {
            continue;
        }
        let merged_key = alias_key_from_join(block_id, join_key.clone());
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
                    (join_alias_key(key) == Some(join_key.clone())).then_some(value)
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
        merge_alias_value(
            graph,
            block_id,
            merged_key,
            values,
            state,
            copy_plan,
            preserve_existing_block_arguments,
        );
    }
}

fn merge_alias_value(
    graph: &mut ValueGraph,
    block_id: usize,
    key: AliasKey,
    values: Vec<Option<&ValueRef>>,
    state: &mut BlockEntryState,
    copy_plan: &mut BlockCopyPlan,
    preserve_existing_block_arguments: bool,
) {
    let Some(first_value) = values.first().and_then(|value| *value).copied() else {
        return;
    };
    let merged = merge_value_candidates(
        graph,
        block_id,
        alias_ordinal(key.clone()),
        graph[first_value.0].ty,
        &values,
        preserve_existing_block_arguments,
    );
    if let Some(decision) = slot_merge_decision(graph, &values, merged) {
        if decision == SlotMergeDecision::InsertCopy {
            set_value_slot_shape(graph, merged, None, None, None);
        }
        copy_plan.aliases.insert(key.clone(), decision);
    }
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
    let merged_version = match space {
        AliasSpace::Memory => merged.memory,
        AliasSpace::Global => merged.global,
        AliasSpace::Table => merged.table,
    };
    merged_version != UNKNOWN_HEAP_VERSION
        && incoming.iter().all(|state| match space {
            AliasSpace::Memory => state.heap.memory == merged_version,
            AliasSpace::Global => state.heap.global == merged_version,
            AliasSpace::Table => state.heap.table == merged_version,
        })
}

fn merge_value_candidates(
    graph: &mut ValueGraph,
    block_id: usize,
    ordinal: usize,
    ty: ValType,
    values: &[Option<&ValueRef>],
    preserve_existing_block_arguments: bool,
) -> ValueRef {
    let Some(first) = values.first().and_then(|value| *value).copied() else {
        return graph.ensure_block_argument(block_id, ordinal, ty, None, None, None, None, None);
    };
    let const_value = merge_const_value_candidates(graph, values);
    let key = merge_key_candidates(graph, values);
    let address_shape = merge_address_shape_candidates(graph, values);
    let loop_value_shape = merge_loop_value_shape_candidates(graph, values);
    let slot_shape = merge_slot_shape_candidates(graph, values);
    let has_block_argument_input = values
        .iter()
        .flatten()
        .any(|value| graph[value.0].is_block_argument());
    let has_same_block_argument_input = values.iter().flatten().any(|value| {
        let origin = graph[value.0].origin;
        origin.kind == ExprOriginKind::BlockArgument && origin.block_id == block_id
    });
    if preserve_existing_block_arguments
        && graph
            .existing_block_argument_value(block_id, ordinal)
            .is_some()
        && (values.len() > 1 || has_block_argument_input)
        && !has_same_block_argument_input
    {
        let preserved_key = (!has_block_argument_input)
            .then_some(key)
            .flatten()
            .filter(|key| !value_key_references_block_argument(*key));
        return graph.ensure_block_argument(
            block_id,
            ordinal,
            ty,
            (!has_block_argument_input).then_some(const_value).flatten(),
            preserved_key,
            address_shape,
            loop_value_shape,
            slot_shape,
        );
    }
    if values
        .iter()
        .all(|value| value.is_some_and(|candidate| same_value(graph, *candidate, first)))
    {
        return first;
    }
    graph.ensure_block_argument(
        block_id,
        ordinal,
        ty,
        const_value,
        key,
        address_shape,
        loop_value_shape,
        slot_shape,
    )
}

fn merge_const_value_candidates(
    graph: &ValueGraph,
    values: &[Option<&ValueRef>],
) -> Option<ConstValue> {
    let mut iter = values
        .iter()
        .map(|value| value.and_then(|value| graph[value.0].const_value));
    let first = iter.next()?;
    iter.all(|candidate| candidate == first)
        .then_some(first)
        .flatten()
}

fn merge_key_candidates(graph: &ValueGraph, values: &[Option<&ValueRef>]) -> Option<ValueKey> {
    let mut iter = values
        .iter()
        .map(|value| value.and_then(|value| graph[value.0].key));
    let first = iter.next()?;
    iter.all(|candidate| candidate == first)
        .then_some(first)
        .flatten()
}

fn value_key_references_block_argument(key: ValueKey) -> bool {
    match key {
        ValueKey::Unary { input, .. } => input.kind == ExprOriginKind::BlockArgument,
        ValueKey::Binary { lhs, rhs, .. } => {
            lhs.kind == ExprOriginKind::BlockArgument || rhs.kind == ExprOriginKind::BlockArgument
        }
    }
}

fn merge_address_shape_candidates(
    graph: &ValueGraph,
    values: &[Option<&ValueRef>],
) -> Option<AddressShape> {
    let mut iter = values
        .iter()
        .map(|value| value.and_then(|value| graph[value.0].address_shape));
    let first = iter.next()?;
    iter.all(|shape| shape == first).then_some(first).flatten()
}

fn merge_loop_value_shape_candidates(
    graph: &ValueGraph,
    values: &[Option<&ValueRef>],
) -> Option<LoopValueShape> {
    let mut iter = values
        .iter()
        .map(|value| value.and_then(|value| graph[value.0].loop_value_shape.clone()));
    let first = iter.next()?;
    if iter.all(|shape| shape == first) {
        first
    } else {
        None
    }
}

fn merge_slot_shape_candidates(
    graph: &ValueGraph,
    values: &[Option<&ValueRef>],
) -> Option<SlotShape> {
    let mut iter = values
        .iter()
        .map(|value| value.and_then(|value| effective_slot_shape(graph, *value)));
    let first = iter.next()?;
    if iter.all(|shape| shape == first) {
        first
    } else {
        None
    }
}

fn slot_merge_decision(
    graph: &ValueGraph,
    values: &[Option<&ValueRef>],
    merged: ValueRef,
) -> Option<SlotMergeDecision> {
    if !graph[merged.0].is_block_argument() {
        return None;
    }
    let shapes = values
        .iter()
        .map(|value| value.and_then(|value| effective_slot_shape(graph, *value)))
        .collect::<Vec<_>>();
    let first = shapes.first().cloned()?;
    if shapes.iter().all(|shape| *shape == first) {
        return first.map(|_| SlotMergeDecision::Preserve);
    }
    shapes
        .iter()
        .any(|shape| shape.is_some())
        .then_some(SlotMergeDecision::InsertCopy)
}

fn alias_ordinal(key: AliasKey) -> usize {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    key.hash(&mut hasher);
    hasher.finish() as usize
}

fn join_alias_key(key: &AliasKey) -> Option<JoinAliasKey> {
    let address = join_alias_address(&key.address)?;
    Some(JoinAliasKey {
        space: key.space,
        index: key.index,
        offset: key.offset,
        width: key.width,
        address,
    })
}

fn join_alias_address(address: &AliasAddress) -> Option<JoinAliasAddress> {
    match address {
        AliasAddress::Const(value) => Some(JoinAliasAddress::Const(*value)),
        AliasAddress::Origin(origin) if origin.kind == ExprOriginKind::EntryLocal => {
            Some(JoinAliasAddress::EntryLocal(origin.ordinal))
        }
        AliasAddress::Origin(origin) if origin.kind == ExprOriginKind::BlockArgument => {
            Some(JoinAliasAddress::BlockArgument(origin.ordinal))
        }
        AliasAddress::Unary { op, input } if alias_address_supports_pure_chain(*op) => {
            Some(JoinAliasAddress::Unary {
                op: *op,
                input: Box::new(join_alias_address(input)?),
            })
        }
        AliasAddress::Binary { op, lhs, rhs } if alias_address_supports_pure_chain(*op) => {
            Some(JoinAliasAddress::Binary {
                op: *op,
                lhs: Box::new(join_alias_address(lhs)?),
                rhs: Box::new(join_alias_address(rhs)?),
            })
        }
        _ => None,
    }
}

fn alias_key_from_join(block_id: usize, key: JoinAliasKey) -> AliasKey {
    let address = alias_address_from_join(block_id, key.address);
    AliasKey {
        space: key.space,
        index: key.index,
        offset: key.offset,
        width: key.width,
        address,
    }
}

fn alias_address_from_join(block_id: usize, address: JoinAliasAddress) -> AliasAddress {
    match address {
        JoinAliasAddress::Const(value) => AliasAddress::Const(value),
        JoinAliasAddress::EntryLocal(ordinal) => AliasAddress::Origin(ExprOrigin {
            block_id,
            ordinal,
            kind: ExprOriginKind::EntryLocal,
        }),
        JoinAliasAddress::BlockArgument(ordinal) => AliasAddress::Origin(ExprOrigin {
            block_id,
            ordinal,
            kind: ExprOriginKind::BlockArgument,
        }),
        JoinAliasAddress::Unary { op, input } => AliasAddress::Unary {
            op,
            input: Box::new(alias_address_from_join(block_id, *input)),
        },
        JoinAliasAddress::Binary { op, lhs, rhs } => AliasAddress::Binary {
            op,
            lhs: Box::new(alias_address_from_join(block_id, *lhs)),
            rhs: Box::new(alias_address_from_join(block_id, *rhs)),
        },
    }
}
fn instr_result_origin_ordinal(ordinal: usize, result_index: usize) -> usize {
    ordinal
        .saturating_mul(INSTR_RESULT_ORIGIN_STRIDE)
        .saturating_add(result_index)
}

fn same_state(graph: &ValueGraph, lhs: &BlockEntryState, rhs: &BlockEntryState) -> bool {
    lhs.reachable == rhs.reachable
        && lhs.heap == rhs.heap
        && same_value_vec(graph, &lhs.stack, &rhs.stack)
        && same_value_map(graph, &lhs.locals, &rhs.locals)
        && same_value_map(graph, &lhs.aliases, &rhs.aliases)
}

fn state_diff_summary(graph: &ValueGraph, lhs: &BlockEntryState, rhs: &BlockEntryState) -> String {
    if lhs.reachable != rhs.reachable {
        return format!("reachable lhs={} rhs={}", lhs.reachable, rhs.reachable);
    }
    if lhs.heap != rhs.heap {
        return format!("heap lhs={:?} rhs={:?}", lhs.heap, rhs.heap);
    }
    if lhs.stack.len() != rhs.stack.len() {
        return format!("stack-len lhs={} rhs={}", lhs.stack.len(), rhs.stack.len());
    }
    for (index, (lhs_value, rhs_value)) in lhs.stack.iter().zip(rhs.stack.iter()).enumerate() {
        if !same_value(graph, *lhs_value, *rhs_value) {
            return format!(
                "stack[{index}] lhs={} rhs={}",
                describe_value(graph, *lhs_value),
                describe_value(graph, *rhs_value),
            );
        }
    }
    if lhs.locals.len() != rhs.locals.len() {
        return format!(
            "locals-len lhs={} rhs={}",
            lhs.locals.len(),
            rhs.locals.len()
        );
    }
    let mut local_keys = lhs.locals.keys().copied().collect::<Vec<_>>();
    local_keys.sort();
    for key in local_keys {
        let Some(rhs_value) = rhs.locals.get(&key).copied() else {
            return format!("local {:?} missing on rhs", key);
        };
        let lhs_value = lhs.locals[&key];
        if !same_value(graph, lhs_value, rhs_value) {
            return format!(
                "local {:?} lhs={} rhs={}",
                key,
                describe_value(graph, lhs_value),
                describe_value(graph, rhs_value),
            );
        }
    }
    if lhs.aliases.len() != rhs.aliases.len() {
        return format!(
            "aliases-len lhs={} rhs={}",
            lhs.aliases.len(),
            rhs.aliases.len()
        );
    }
    let mut alias_keys = lhs.aliases.keys().cloned().collect::<Vec<_>>();
    alias_keys.sort();
    for key in alias_keys {
        let Some(rhs_value) = rhs.aliases.get(&key).copied() else {
            return format!("alias {:?} missing on rhs", key);
        };
        let lhs_value = lhs.aliases[&key];
        if !same_value(graph, lhs_value, rhs_value) {
            return format!(
                "alias {:?} lhs={} rhs={}",
                key,
                describe_value(graph, lhs_value),
                describe_value(graph, rhs_value),
            );
        }
    }
    "unknown".to_owned()
}

fn describe_value(graph: &ValueGraph, value: ValueRef) -> String {
    let value = &graph[value.0];
    format!(
        "origin={:?} def={:?} const={:?} key={:?} slot={:?}",
        value.origin, value.def, value.const_value, value.key, value.slot_shape
    )
}

fn same_value_vec(graph: &ValueGraph, lhs: &[ValueRef], rhs: &[ValueRef]) -> bool {
    lhs.len() == rhs.len()
        && lhs
            .iter()
            .zip(rhs.iter())
            .all(|(lhs, rhs)| same_value(graph, *lhs, *rhs))
}

fn same_value_map<K: Eq + std::hash::Hash>(
    graph: &ValueGraph,
    lhs: &HashMap<K, ValueRef>,
    rhs: &HashMap<K, ValueRef>,
) -> bool {
    lhs.len() == rhs.len()
        && lhs.iter().all(|(key, lhs_value)| {
            rhs.get(key)
                .is_some_and(|rhs_value| same_value(graph, *lhs_value, *rhs_value))
        })
}

fn same_value(graph: &ValueGraph, lhs: ValueRef, rhs: ValueRef) -> bool {
    let lhs = &graph[lhs.0];
    let rhs = &graph[rhs.0];
    lhs.ty == rhs.ty
        && lhs.origin == rhs.origin
        && lhs.def == rhs.def
        && lhs.block_argument() == rhs.block_argument()
        && lhs.const_value == rhs.const_value
        && lhs.key == rhs.key
        && lhs.address_shape == rhs.address_shape
        && lhs.loop_value_shape == rhs.loop_value_shape
        && lhs.slot_shape == rhs.slot_shape
}

fn ensure_seed_value(graph: &mut ValueGraph, ty: ValType, origin: ExprOrigin) -> ValueRef {
    let value = ExprId(graph.nodes.len());
    graph.nodes.push(ExprState {
        ty,
        origin,
        def: ValueDef::Synthetic,
        const_value: None,
        key: None,
        address_shape: None,
        loop_value_shape: None,
        slot_shape: None,
        provider_class: ProviderClass::None,
        materialization_cost: MaterializationCost::Unknown,
        producer_op: None,
        materialized_block: None,
        materialized_op: None,
        needs_spill: false,
        use_count: 0,
        ref_count: 0,
        removable: false,
    });
    value
}

fn block_body_is_empty(body: &BlockBody) -> bool {
    body.ops.is_empty() && body.terminator.is_none()
}

#[derive(Default)]
struct EffectResultSpillPlan {
    slots: HashMap<ValueRef, LocalSlot>,
}

impl EffectResultSpillPlan {
    fn slot(&self, value: ValueRef) -> Option<LocalSlot> {
        self.slots.get(&value).copied()
    }
}

#[derive(Clone)]
struct SpecializedLocalControlLowering {
    source_start: Option<usize>,
    op: Op,
    operands: Vec<BlockOperand>,
    absorbed_ops: BTreeSet<usize>,
    consumer_after_idx: usize,
}

#[derive(Clone)]
struct SpecializedMemoryLowering {
    op: Op,
    operands: Vec<BlockOperand>,
}

#[derive(Clone, Debug)]
struct MatchedAddressLowering {
    base: LocalSlot,
    offset_delta: i32,
    absorbed_ops: BTreeSet<usize>,
}

#[derive(Clone, Debug)]
struct TrailingValueSlice {
    start_idx: usize,
    #[cfg(debug_assertions)]
    op_indices: BTreeSet<usize>,
}

fn symbolic_spill_slot(source: ValueRef, size: u32) -> LocalSlot {
    LocalSlot::new(u32::MAX.saturating_sub(source.0 as u32), size)
}

fn entry_local_address_shape(slot: LocalSlot, ty: ValType) -> Option<AddressShape> {
    (ty == ValType::I32).then_some(AddressShape {
        base: AddressBaseKind::EntryLocal(slot),
        offset_delta: 0,
    })
}

fn spill_local_address_shape(slot: LocalSlot, ty: ValType) -> Option<AddressShape> {
    (ty == ValType::I32).then_some(AddressShape {
        base: AddressBaseKind::SpillLocal(slot),
        offset_delta: 0,
    })
}

fn entry_local_loop_value_shape(slot: LocalSlot, ty: ValType) -> Option<LoopValueShape> {
    (ty == ValType::I32 && slot.size == 4).then_some(LoopValueShape::Local4(slot))
}

fn spill_local_loop_value_shape(slot: LocalSlot, ty: ValType) -> Option<LoopValueShape> {
    (ty == ValType::I32 && slot.size == 4).then_some(LoopValueShape::Local4(slot))
}

fn slot_ref_for_local_slot(slot: LocalSlot) -> SlotRef {
    SlotRef::entry_local(slot)
}

fn slot_ref_from_address_shape(shape: AddressShape) -> Option<SlotRef> {
    if shape.offset_delta != 0 {
        return None;
    }
    Some(match shape.base {
        AddressBaseKind::EntryLocal(slot) => SlotRef::entry_local(slot),
        AddressBaseKind::SpillLocal(slot) => SlotRef::spill_local(slot),
    })
}

fn slot_ref_from_loop_value_shape(shape: &LoopValueShape) -> Option<SlotRef> {
    match shape {
        LoopValueShape::Local4(slot) => Some(SlotRef::entry_local(*slot)),
        _ => None,
    }
}

fn build_slot_shape(
    slot: Option<SlotRef>,
    address: Option<AddressShape>,
    loop_value: Option<LoopValueShape>,
) -> Option<SlotShape> {
    if slot.is_none() && address.is_none() && loop_value.is_none() {
        None
    } else {
        Some(SlotShape {
            slot,
            address,
            loop_value,
        })
    }
}

fn direct_slot_from_shape_parts(
    address: Option<AddressShape>,
    loop_value: Option<&LoopValueShape>,
) -> Option<SlotRef> {
    address
        .and_then(slot_ref_from_address_shape)
        .or_else(|| loop_value.and_then(slot_ref_from_loop_value_shape))
}

fn effective_slot_shape(graph: &ValueGraph, value: ValueRef) -> Option<SlotShape> {
    let node = &graph[value.0];
    node.slot_shape.clone().or_else(|| {
        build_slot_shape(
            direct_slot_from_shape_parts(node.address_shape, node.loop_value_shape.as_ref()),
            node.address_shape,
            node.loop_value_shape.clone(),
        )
    })
}

fn set_direct_slot_shape(graph: &mut ValueGraph, value: ValueRef, slot: SlotRef) {
    let node = &mut graph[value.0];
    node.slot_shape = build_slot_shape(
        Some(slot),
        node.address_shape,
        node.loop_value_shape.clone(),
    );
    node.refresh_optimizer_metadata();
}

fn set_value_slot_shape(
    graph: &mut ValueGraph,
    value: ValueRef,
    slot: Option<SlotRef>,
    address_shape: Option<AddressShape>,
    loop_value_shape: Option<LoopValueShape>,
) {
    let node = &mut graph[value.0];
    let direct_slot =
        slot.or_else(|| direct_slot_from_shape_parts(address_shape, loop_value_shape.as_ref()));
    node.address_shape = address_shape;
    node.loop_value_shape = loop_value_shape.clone();
    node.slot_shape = build_slot_shape(direct_slot, address_shape, loop_value_shape);
    node.refresh_optimizer_metadata();
}

fn materializable_slot(slot: SlotRef) -> Option<LocalSlot> {
    match slot.class {
        SlotClass::EntryLocal | SlotClass::TempLocal | SlotClass::SpillLocal => Some(slot.slot),
        SlotClass::VirtualStack | SlotClass::ConstPoolRef => None,
    }
}

fn slot_shape_local_operand(shape: &SlotShape, expected_size: u32) -> Option<BlockOperand> {
    let slot = shape.slot.and_then(materializable_slot)?;
    (slot.size == expected_size).then_some(BlockOperand::LocalAddr(slot.addr))
}

fn selector_value_slot_ref_operand(
    graph: &ValueGraph,
    value: ValueRef,
    expected_size: u32,
) -> Option<BlockOperand> {
    if graph[value.0].is_effect_result() {
        return None;
    }
    effective_slot_shape(graph, value)
        .and_then(|shape| slot_shape_local_operand(&shape, expected_size))
}

fn shared_select_slot_shape(
    graph: &ValueGraph,
    lhs: ValueRef,
    rhs: ValueRef,
    expected_size: u32,
) -> Option<SlotShape> {
    let lhs_shape = effective_slot_shape(graph, lhs)?;
    let rhs_shape = effective_slot_shape(graph, rhs)?;
    (lhs_shape == rhs_shape && slot_shape_local_operand(&lhs_shape, expected_size).is_some())
        .then_some(lhs_shape)
}

fn i32_const_expr(exprs: &ValueGraph, value: ValueRef) -> Option<i32> {
    match exprs[value.0].const_value {
        Some(ConstValue::I32(value)) => Some(value),
        _ => None,
    }
}

fn i32_compare_op(op: PureOpKind) -> bool {
    matches!(
        op,
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
    )
}

fn flip_i32_compare_op(op: PureOpKind) -> Option<PureOpKind> {
    match op {
        PureOpKind::I32Eq | PureOpKind::I32Ne => Some(op),
        PureOpKind::I32LtS => Some(PureOpKind::I32GtS),
        PureOpKind::I32LtU => Some(PureOpKind::I32GtU),
        PureOpKind::I32GtS => Some(PureOpKind::I32LtS),
        PureOpKind::I32GtU => Some(PureOpKind::I32LtU),
        PureOpKind::I32LeS => Some(PureOpKind::I32GeS),
        PureOpKind::I32LeU => Some(PureOpKind::I32GeU),
        PureOpKind::I32GeS => Some(PureOpKind::I32LeS),
        PureOpKind::I32GeU => Some(PureOpKind::I32LeU),
        _ => None,
    }
}

fn derive_unary_loop_value_shape(
    exprs: &ValueGraph,
    op: PureOpKind,
    input: ValueRef,
) -> Option<LoopValueShape> {
    match op {
        PureOpKind::I32Eqz => {
            exprs[input.0]
                .loop_value_shape
                .clone()
                .map(|input| LoopValueShape::CompareEqz {
                    input: Box::new(input),
                })
        }
        _ => None,
    }
}

fn derive_binary_address_shape(
    exprs: &ValueGraph,
    op: PureOpKind,
    lhs: ValueRef,
    rhs: ValueRef,
) -> Option<AddressShape> {
    match op {
        PureOpKind::I32Add => {
            combine_address_shape_and_const(exprs[lhs.0].address_shape, i32_const_expr(exprs, rhs))
                .or_else(|| {
                    combine_address_shape_and_const(
                        exprs[rhs.0].address_shape,
                        i32_const_expr(exprs, lhs),
                    )
                })
        }
        PureOpKind::I32Sub => combine_address_shape_and_const(
            exprs[lhs.0].address_shape,
            i32_const_expr(exprs, rhs).map(|value| value.wrapping_neg()),
        ),
        _ => None,
    }
}

fn combine_address_shape_and_const(
    base: Option<AddressShape>,
    delta: Option<i32>,
) -> Option<AddressShape> {
    let base = base?;
    Some(AddressShape {
        base: base.base,
        offset_delta: base.offset_delta.wrapping_add(delta?),
    })
}

fn derive_binary_loop_value_shape(
    exprs: &ValueGraph,
    op: PureOpKind,
    lhs: ValueRef,
    rhs: ValueRef,
) -> Option<LoopValueShape> {
    match op {
        PureOpKind::I32Add => combine_loop_shape_and_const(
            exprs[lhs.0].loop_value_shape.as_ref(),
            i32_const_expr(exprs, rhs),
        )
        .or_else(|| {
            combine_loop_shape_and_const(
                exprs[rhs.0].loop_value_shape.as_ref(),
                i32_const_expr(exprs, lhs),
            )
        })
        .or_else(|| {
            combine_local4_add(
                exprs[lhs.0].loop_value_shape.as_ref(),
                exprs[rhs.0].loop_value_shape.as_ref(),
            )
        }),
        PureOpKind::I32Sub => combine_loop_shape_and_const(
            exprs[lhs.0].loop_value_shape.as_ref(),
            i32_const_expr(exprs, rhs).map(|value| value.wrapping_neg()),
        ),
        _ if i32_compare_op(op) => derive_i32_compare_loop_value_shape(exprs, op, lhs, rhs),
        _ => None,
    }
}

fn combine_loop_shape_and_const(
    base: Option<&LoopValueShape>,
    delta: Option<i32>,
) -> Option<LoopValueShape> {
    let delta = delta?;
    match base? {
        LoopValueShape::Local4(base) => Some(LoopValueShape::Local4ConstAdd {
            base: *base,
            imm: delta,
        }),
        LoopValueShape::Local4ConstAdd { base, imm } => Some(LoopValueShape::Local4ConstAdd {
            base: *base,
            imm: imm.wrapping_add(delta),
        }),
        _ => None,
    }
}

fn combine_local4_add(
    lhs: Option<&LoopValueShape>,
    rhs: Option<&LoopValueShape>,
) -> Option<LoopValueShape> {
    match (lhs?, rhs?) {
        (LoopValueShape::Local4(lhs), LoopValueShape::Local4(rhs)) => {
            Some(LoopValueShape::Local4Local4Add {
                lhs: *lhs,
                rhs: *rhs,
            })
        }
        _ => None,
    }
}

fn derive_i32_compare_loop_value_shape(
    exprs: &ValueGraph,
    op: PureOpKind,
    lhs: ValueRef,
    rhs: ValueRef,
) -> Option<LoopValueShape> {
    if let (Some(lhs_shape), Some(rhs_const)) = (
        exprs[lhs.0].loop_value_shape.as_ref(),
        i32_const_expr(exprs, rhs),
    ) {
        return Some(LoopValueShape::CompareConstI32 {
            lhs: Box::new(lhs_shape.clone()),
            op,
            imm: rhs_const,
        });
    }
    if let (Some(lhs_const), Some(rhs_shape), Some(flipped)) = (
        i32_const_expr(exprs, lhs),
        exprs[rhs.0].loop_value_shape.as_ref(),
        flip_i32_compare_op(op),
    ) {
        return Some(LoopValueShape::CompareConstI32 {
            lhs: Box::new(rhs_shape.clone()),
            op: flipped,
            imm: lhs_const,
        });
    }
    match (
        exprs[lhs.0].loop_value_shape.as_ref(),
        exprs[rhs.0].loop_value_shape.as_ref(),
    ) {
        (Some(LoopValueShape::Local4(lhs)), Some(LoopValueShape::Local4(rhs))) => {
            Some(LoopValueShape::CompareLocal4 {
                lhs: *lhs,
                op,
                rhs: *rhs,
            })
        }
        _ => None,
    }
}

fn build_specialized_memory_lowering(
    body: &BlockBody,
    graph: &ValueGraph,
    spill_plan: &EffectResultSpillPlan,
    op_idx: usize,
    op: &BlockOp,
) -> Option<(SpecializedMemoryLowering, BTreeSet<usize>)> {
    let opcode = specialized_memory_op(op.op)?;
    let memarg = block_op_memarg(op)?;
    let shape = match op.kind {
        BlockOpKind::MemoryLoad => {
            let address = memory_address_input(op)?;
            match_memory_address_shape(body, graph, spill_plan, op_idx, address)?
        }
        BlockOpKind::MemoryStore => {
            let address = memory_address_input(op)?;
            let value = memory_store_value_input(op)?;
            match_store_address_shape(body, graph, spill_plan, op_idx, address, value)?
        }
        _ => return None,
    };
    let mut operands = vec![
        BlockOperand::LocalAddr(shape.base.addr),
        BlockOperand::I32(shape.offset_delta),
        BlockOperand::Raw(Operand { memarg }),
    ];
    if let Some(memidx) = block_op_index_memidx(op) {
        operands.push(BlockOperand::U32(memidx));
    }
    Some((
        SpecializedMemoryLowering {
            op: opcode,
            operands,
        },
        shape.absorbed_ops,
    ))
}

fn memory_address_input(op: &BlockOp) -> Option<ValueRef> {
    matches!(op.kind, BlockOpKind::MemoryLoad | BlockOpKind::MemoryStore)
        .then(|| op.inputs.first().copied())
        .flatten()
}

fn memory_store_value_input(op: &BlockOp) -> Option<ValueRef> {
    (op.kind == BlockOpKind::MemoryStore)
        .then(|| op.inputs.get(1).copied())
        .flatten()
}

fn block_op_memarg(op: &BlockOp) -> Option<crate::common::MemArg> {
    let BlockOperand::Raw(operand) = *op.operands.first()? else {
        return None;
    };
    Some(unsafe { operand.memarg })
}

fn block_op_index_memidx(op: &BlockOp) -> Option<u32> {
    match *op.operands.get(1)? {
        BlockOperand::U32(memidx) => Some(memidx),
        BlockOperand::Raw(operand) => Some(unsafe { operand.u32 }),
        _ => None,
    }
}

fn match_memory_address_shape(
    body: &BlockBody,
    graph: &ValueGraph,
    spill_plan: &EffectResultSpillPlan,
    op_idx: usize,
    value: ValueRef,
) -> Option<MatchedAddressLowering> {
    match_direct_address_shape(body, graph, spill_plan, op_idx, value)
        .or_else(|| match_offset_address_shape(body, graph, spill_plan, op_idx, value))
}

fn match_store_address_shape(
    body: &BlockBody,
    graph: &ValueGraph,
    spill_plan: &EffectResultSpillPlan,
    op_idx: usize,
    address: ValueRef,
    value: ValueRef,
) -> Option<MatchedAddressLowering> {
    match_store_direct_address_shape(body, graph, spill_plan, op_idx, address, value).or_else(
        || match_store_offset_address_shape(body, graph, spill_plan, op_idx, address, value),
    )
}

fn match_direct_address_shape(
    body: &BlockBody,
    graph: &ValueGraph,
    spill_plan: &EffectResultSpillPlan,
    op_idx: usize,
    _value: ValueRef,
) -> Option<MatchedAddressLowering> {
    let base_idx = op_idx.checked_sub(1)?;
    let base_op = body.ops.get(base_idx)?;
    if block_op_any_local_get_slot(base_op, spill_plan).is_none()
        || !block_op_address_base_single_use(graph, body, op_idx + 1, base_op)
    {
        return None;
    }
    let base = block_op_any_local_get_slot(base_op, spill_plan)?;
    Some(MatchedAddressLowering {
        base,
        offset_delta: 0,
        absorbed_ops: BTreeSet::from([base_idx]),
    })
}

fn match_store_direct_address_shape(
    body: &BlockBody,
    graph: &ValueGraph,
    spill_plan: &EffectResultSpillPlan,
    op_idx: usize,
    _address: ValueRef,
    value: ValueRef,
) -> Option<MatchedAddressLowering> {
    let value_slice = find_contiguous_trailing_value_slice(body, op_idx, value)?;
    let base_idx = value_slice.start_idx.checked_sub(1)?;
    let base_op = body.ops.get(base_idx)?;
    if block_op_any_local_get_slot(base_op, spill_plan).is_none()
        || !block_op_address_base_single_use(graph, body, op_idx + 1, base_op)
    {
        return None;
    }
    let base = block_op_any_local_get_slot(base_op, spill_plan)?;
    Some(MatchedAddressLowering {
        base,
        offset_delta: 0,
        absorbed_ops: BTreeSet::from([base_idx]),
    })
}

fn match_offset_address_shape(
    body: &BlockBody,
    graph: &ValueGraph,
    spill_plan: &EffectResultSpillPlan,
    op_idx: usize,
    value: ValueRef,
) -> Option<MatchedAddressLowering> {
    let binary_idx = op_idx.checked_sub(1)?;
    let binary_op = body.ops.get(binary_idx)?;
    if block_op_single_result(binary_op) != Some(value) || !block_op_single_use(graph, binary_op) {
        return None;
    }
    match binary_op.inputs.as_slice() {
        [_, _] => {}
        _ => return None,
    }
    match binary_op.kind {
        BlockOpKind::PureBinary(PureOpKind::I32Add) => {
            match_adjacent_base_and_const(body, graph, spill_plan, op_idx + 1, binary_idx, false)
                .or_else(|| {
                    match_adjacent_base_and_const(
                        body,
                        graph,
                        spill_plan,
                        op_idx + 1,
                        binary_idx,
                        false,
                    )
                })
        }
        BlockOpKind::PureBinary(PureOpKind::I32Sub) => {
            match_adjacent_base_and_const(body, graph, spill_plan, op_idx + 1, binary_idx, true)
        }
        _ => None,
    }
}

fn match_store_offset_address_shape(
    body: &BlockBody,
    graph: &ValueGraph,
    spill_plan: &EffectResultSpillPlan,
    op_idx: usize,
    address: ValueRef,
    value: ValueRef,
) -> Option<MatchedAddressLowering> {
    let value_slice = find_contiguous_trailing_value_slice(body, op_idx, value)?;
    let binary_idx = value_slice.start_idx.checked_sub(1)?;
    let binary_op = body.ops.get(binary_idx)?;
    if block_op_single_result(binary_op) != Some(address) || !block_op_single_use(graph, binary_op)
    {
        return None;
    }
    match binary_op.inputs.as_slice() {
        [_, _] => {}
        _ => return None,
    }
    match binary_op.kind {
        BlockOpKind::PureBinary(PureOpKind::I32Add) => match_store_adjacent_base_and_const(
            body,
            graph,
            spill_plan,
            op_idx + 1,
            binary_idx,
            false,
        )
        .or_else(|| {
            match_store_adjacent_base_and_const(
                body,
                graph,
                spill_plan,
                op_idx + 1,
                binary_idx,
                false,
            )
        }),
        BlockOpKind::PureBinary(PureOpKind::I32Sub) => match_store_adjacent_base_and_const(
            body,
            graph,
            spill_plan,
            op_idx + 1,
            binary_idx,
            true,
        ),
        _ => None,
    }
}

fn match_store_adjacent_base_and_const(
    body: &BlockBody,
    graph: &ValueGraph,
    spill_plan: &EffectResultSpillPlan,
    after_idx: usize,
    binary_idx: usize,
    negate_delta: bool,
) -> Option<MatchedAddressLowering> {
    let lhs_idx = binary_idx.checked_sub(2)?;
    let rhs_idx = binary_idx.checked_sub(1)?;
    let first = body.ops.get(lhs_idx)?;
    let second = body.ops.get(rhs_idx)?;
    let (base_idx, base_op, const_idx, const_op) =
        match_adjacent_base_and_const_ops(spill_plan, first, lhs_idx, second, rhs_idx)?;
    if !block_op_address_base_single_use(graph, body, after_idx, base_op)
        || !block_op_single_use(graph, const_op)
    {
        return None;
    }
    let base = block_op_any_local_get_slot(base_op, spill_plan)?;
    let delta = block_op_i32_const(const_op)?;
    let offset_delta = if negate_delta {
        delta.wrapping_neg()
    } else {
        delta
    };
    Some(MatchedAddressLowering {
        base,
        offset_delta,
        absorbed_ops: BTreeSet::from([base_idx, const_idx, binary_idx]),
    })
}

fn match_adjacent_base_and_const(
    body: &BlockBody,
    graph: &ValueGraph,
    spill_plan: &EffectResultSpillPlan,
    after_idx: usize,
    binary_idx: usize,
    negate_delta: bool,
) -> Option<MatchedAddressLowering> {
    let lhs_idx = binary_idx.checked_sub(2)?;
    let rhs_idx = binary_idx.checked_sub(1)?;
    let first = body.ops.get(lhs_idx)?;
    let second = body.ops.get(rhs_idx)?;
    let (base_idx, base_op, const_idx, const_op) =
        match_adjacent_base_and_const_ops(spill_plan, first, lhs_idx, second, rhs_idx)?;
    if !block_op_address_base_single_use(graph, body, after_idx, base_op)
        || !block_op_single_use(graph, const_op)
    {
        return None;
    }
    let base = block_op_any_local_get_slot(base_op, spill_plan)?;
    let delta = block_op_i32_const(const_op)?;
    let offset_delta = if negate_delta {
        delta.wrapping_neg()
    } else {
        delta
    };
    Some(MatchedAddressLowering {
        base,
        offset_delta,
        absorbed_ops: BTreeSet::from([base_idx, const_idx, binary_idx]),
    })
}

fn find_contiguous_trailing_value_slice(
    body: &BlockBody,
    end_exclusive: usize,
    root: ValueRef,
) -> Option<TrailingValueSlice> {
    let mut op_indices = BTreeSet::new();
    let mut seen_values = HashSet::new();
    collect_trailing_value_ops(body, end_exclusive, root, &mut seen_values, &mut op_indices)?;
    let start_idx = *op_indices.first()?;
    let end_idx = *op_indices.last()?;
    if end_idx + 1 != end_exclusive {
        return None;
    }
    if op_indices.len() != end_exclusive.saturating_sub(start_idx) {
        return None;
    }
    Some(TrailingValueSlice {
        start_idx,
        #[cfg(debug_assertions)]
        op_indices,
    })
}

fn collect_trailing_value_ops(
    body: &BlockBody,
    end_exclusive: usize,
    root: ValueRef,
    seen_values: &mut HashSet<ValueRef>,
    op_indices: &mut BTreeSet<usize>,
) -> Option<()> {
    if !seen_values.insert(root) {
        return Some(());
    }
    let producer_idx = producer_op_index_before(body, root, end_exclusive)?;
    if op_indices.insert(producer_idx) {
        let producer = body.ops.get(producer_idx)?;
        for input in &producer.inputs {
            collect_trailing_value_ops(body, producer_idx, *input, seen_values, op_indices)?;
        }
    }
    Some(())
}

fn producer_op_index_before(
    body: &BlockBody,
    value: ValueRef,
    end_exclusive: usize,
) -> Option<usize> {
    body.ops
        .iter()
        .take(end_exclusive)
        .enumerate()
        .rev()
        .find_map(|(idx, op)| op.values.contains(&value).then_some(idx))
}

fn match_adjacent_base_and_const_ops<'a>(
    spill_plan: &EffectResultSpillPlan,
    first: &'a BlockOp,
    first_idx: usize,
    second: &'a BlockOp,
    second_idx: usize,
) -> Option<(usize, &'a BlockOp, usize, &'a BlockOp)> {
    if block_op_any_local_get_slot(first, spill_plan).is_some()
        && block_op_i32_const(second).is_some()
    {
        return Some((first_idx, first, second_idx, second));
    }
    if block_op_any_local_get_slot(second, spill_plan).is_some()
        && block_op_i32_const(first).is_some()
    {
        return Some((second_idx, second, first_idx, first));
    }
    None
}

fn build_effect_result_spill_plan(
    graph: &ValueGraph,
    bodies: &[BlockBody],
    reachable: &[bool],
    locals: &mut LocalsData,
    param_bytes: u32,
) -> EffectResultSpillPlan {
    let mut values = HashSet::new();
    for (block_id, body) in bodies.iter().enumerate() {
        if !reachable.get(block_id).copied().unwrap_or(false) {
            continue;
        }
        for op in &body.ops {
            for operand in &op.operands {
                if let BlockOperand::SpillValue(value) = *operand {
                    values.insert(value);
                }
            }
        }
        if let Some(terminator) = &body.terminator {
            for operand in &terminator.operands {
                if let BlockOperand::SpillValue(value) = *operand {
                    values.insert(value);
                }
            }
        }
    }
    let mut plan = EffectResultSpillPlan::default();
    for value in values {
        let Some(size) = value_type_size(graph[value.0].ty) else {
            continue;
        };
        let slot = LocalSlot::new(
            allocate_optimizer_temp_slot(locals, param_bytes, graph[value.0].ty),
            size,
        );
        plan.slots.insert(value, slot);
    }
    plan
}

fn allocate_optimizer_temp_slot(locals: &mut LocalsData, param_bytes: u32, ty: ValType) -> u32 {
    param_bytes + locals.allocate_temp_slot(ty)
}

fn relower_block_body(
    body: &BlockBody,
    graph: &ValueGraph,
    spill_plan: &EffectResultSpillPlan,
) -> Vec<PackedOp> {
    let mut skipped_ops = HashSet::new();
    let mut ops_rev = Vec::with_capacity(body.ops.len() + usize::from(body.terminator.is_some()));

    if let Some(terminator) = &body.terminator {
        if let Some(spec) = build_specialized_br_if_lowering(graph, body, terminator) {
            debug_assert!(verify_specialized_local_control_lowering(body, &spec));
            skipped_ops.extend(spec.absorbed_ops.iter().copied());
            ops_rev.push(relower_specialized_local_control_terminator(
                &spec, spill_plan,
            ));
        } else if terminator.kind != BlockTerminatorKind::Loop {
            ops_rev.push(relower_block_terminator(terminator, spill_plan));
        }
    }

    for (op_idx, op) in body.ops.iter().enumerate().rev() {
        if skipped_ops.contains(&op_idx) {
            continue;
        }

        let lowered = if let Some(spec) =
            build_specialized_local_set_tee_lowering(graph, body, op_idx, op)
                .or_else(|| build_specialized_local_root_lowering(graph, body, op_idx, op))
        {
            debug_assert!(verify_specialized_local_control_lowering(body, &spec));
            debug_assert!(spec
                .absorbed_ops
                .iter()
                .all(|absorbed_idx| !skipped_ops.contains(absorbed_idx)));
            skipped_ops.extend(spec.absorbed_ops.iter().copied());
            relower_specialized_local_control_op(&spec, spill_plan)
        } else if let Some((spec, absorbed_ops)) =
            build_specialized_memory_lowering(body, graph, spill_plan, op_idx, op)
        {
            debug_verify_specialized_memory_lowering(body, op_idx, &spec, &absorbed_ops);
            debug_assert!(absorbed_ops
                .iter()
                .all(|absorbed_idx| !skipped_ops.contains(absorbed_idx)));
            skipped_ops.extend(absorbed_ops);
            pack_emitted_op(
                op.source_start,
                spec.op,
                block_operands_to_raw_with_spills_for_op(spec.op, &spec.operands, spill_plan),
            )
        } else {
            relower_block_op(op, spill_plan)
        };

        if let Some(slot) = spill_slot_for_effect_result(graph, op, spill_plan) {
            ops_rev.push(pack_emitted_op(
                None,
                local_tee_op(slot.size),
                vec![Operand {
                    local_addr: slot.addr,
                }],
            ));
        }
        ops_rev.push(lowered);
    }

    ops_rev.reverse();
    ops_rev
}

fn pack_emitted_op(source_start: Option<usize>, op: Op, operands: Vec<Operand>) -> PackedOp {
    pack_op(source_start, op, &operands)
}

fn relower_specialized_local_control_op(
    spec: &SpecializedLocalControlLowering,
    spill_plan: &EffectResultSpillPlan,
) -> PackedOp {
    pack_emitted_op(
        spec.source_start,
        spec.op,
        block_operands_to_raw_with_spills_for_op(spec.op, &spec.operands, spill_plan),
    )
}

fn relower_specialized_local_control_terminator(
    spec: &SpecializedLocalControlLowering,
    spill_plan: &EffectResultSpillPlan,
) -> PackedOp {
    let terminator = BlockTerminator {
        source_start: spec.source_start,
        op: spec.op,
        kind: BlockTerminatorKind::BrIf,
        operands: spec.operands.clone(),
        inputs: Vec::new(),
        values: Vec::new(),
    };
    pack_emitted_op(
        spec.source_start,
        spec.op,
        terminator_operands_to_raw_with_spills(&terminator, spill_plan),
    )
}

fn spill_slot_for_effect_result(
    graph: &ValueGraph,
    op: &BlockOp,
    spill_plan: &EffectResultSpillPlan,
) -> Option<LocalSlot> {
    let mut spilled = op
        .values
        .iter()
        .filter_map(|value| {
            let node = &graph[value.0];
            (node.needs_spill && node.is_effect_result())
                .then(|| spill_plan.slot(*value))
                .flatten()
        })
        .collect::<Vec<_>>();
    spilled.dedup();
    if spilled.len() > 1 {
        return None;
    }
    spilled.into_iter().next()
}

fn preserved_values_cross_control_boundary(op: Op) -> bool {
    std::ptr::fn_addr_eq(op, vm::op_if as Op)
        || std::ptr::fn_addr_eq(op, vm::op_br_if as Op)
        || std::ptr::fn_addr_eq(op, vm::op_else as Op)
        || std::ptr::fn_addr_eq(op, vm::op_loop as Op)
        || std::ptr::fn_addr_eq(op, vm::op_end as Op)
        || std::ptr::fn_addr_eq(op, vm::special_block_return as Op)
}

fn full_stack_live_on_unreachable_control_transfer(op: Op) -> bool {
    std::ptr::fn_addr_eq(op, vm::op_br as Op)
        || std::ptr::fn_addr_eq(op, vm::op_br_table as Op)
        || std::ptr::fn_addr_eq(op, vm::op_return as Op)
        || std::ptr::fn_addr_eq(op, vm::special_function_return as Op)
        || std::ptr::fn_addr_eq(op, vm::op_return_call as Op)
        || std::ptr::fn_addr_eq(op, vm::op_return_call_import as Op)
        || std::ptr::fn_addr_eq(op, vm::op_return_call_indirect as Op)
}

fn debug_verify_specialized_memory_lowering(
    _body: &BlockBody,
    _op_idx: usize,
    _spec: &SpecializedMemoryLowering,
    _absorbed_ops: &BTreeSet<usize>,
) {
    #[cfg(debug_assertions)]
    {
        let original = &_body.ops[_op_idx];
        debug_assert!(matches!(
            original.kind,
            BlockOpKind::MemoryLoad | BlockOpKind::MemoryStore
        ));
        debug_assert!(specialized_memory_op(original.op)
            .is_some_and(|candidate| std::ptr::fn_addr_eq(candidate, _spec.op)));
        debug_assert!(same_memarg(block_op_memarg(original), spec_memarg(_spec)));
        debug_assert_eq!(block_op_index_memidx(original), spec_memidx(_spec));
        if original.kind == BlockOpKind::MemoryStore {
            if let Some(value) = memory_store_value_input(original) {
                if let Some(value_slice) =
                    find_contiguous_trailing_value_slice(_body, _op_idx, value)
                {
                    debug_assert_eq!(
                        value_slice.op_indices.last().copied(),
                        _op_idx.checked_sub(1),
                        "store specialization must keep the trailing value suffix immediately before the store",
                    );
                    for value_idx in &value_slice.op_indices {
                        debug_assert!(
                            !_absorbed_ops.contains(value_idx),
                            "store specialization must not absorb value producer ops from the trailing value slice: store_op_idx={_op_idx} source_start={:?} value_op_idx={value_idx} value_slice={:?} absorbed={_absorbed_ops:?} body={}",
                            original.source_start,
                            value_slice.op_indices,
                            debug_body_window(_body, value_slice.start_idx.saturating_sub(3), _op_idx + 1),
                        );
                    }
                }
            }
        }
    }
}

#[cfg(debug_assertions)]
fn debug_body_window(body: &BlockBody, start_idx: usize, end_inclusive: usize) -> String {
    let last = end_inclusive.min(body.ops.len().saturating_sub(1));
    (start_idx..=last)
        .filter_map(|idx| body.ops.get(idx).map(|op| debug_block_op_summary(idx, op)))
        .collect::<Vec<_>>()
        .join(" | ")
}

#[cfg(debug_assertions)]
fn debug_block_op_summary(idx: usize, op: &BlockOp) -> String {
    let extra =
        if let Some(slot) = block_op_any_local_get_slot(op, &EffectResultSpillPlan::default()) {
            format!(" slot={}+{}", slot.addr, slot.size)
        } else if let Some(value) = block_op_i32_const(op) {
            format!(" i32={value}")
        } else if let Some(memarg) = block_op_memarg(op) {
            let memidx = block_op_index_memidx(op).unwrap_or_default();
            format!(
                " memarg(offset={},align={},memidx={memidx})",
                memarg.offset, memarg.align
            )
        } else {
            String::new()
        };
    format!(
        "#{idx}:{:?}:src={:?}:inputs={:?}:values={:?}{extra}",
        op.kind, op.source_start, op.inputs, op.values
    )
}

#[cold]
#[inline(never)]
fn verify_specialized_local_control_lowering(
    body: &BlockBody,
    spec: &SpecializedLocalControlLowering,
) -> bool {
    if spec.consumer_after_idx > body.ops.len() {
        return false;
    }
    let mut seen_absorbed = HashSet::new();
    for absorbed_idx in &spec.absorbed_ops {
        if !seen_absorbed.insert(*absorbed_idx) || *absorbed_idx >= spec.consumer_after_idx {
            return false;
        }
        let Some(op) = body.ops.get(*absorbed_idx) else {
            return false;
        };
        if matches!(op.kind, BlockOpKind::LocalSet | BlockOpKind::LocalTee) {
            continue;
        }
        let Some(value) = block_op_single_result(op) else {
            if matches!(
                op.kind,
                BlockOpKind::Const
                    | BlockOpKind::LocalGet
                    | BlockOpKind::PureUnary(_)
                    | BlockOpKind::PureBinary(_)
            ) {
                continue;
            }
            return false;
        };
        if value_used_after(body, spec.consumer_after_idx, value)
            || value_feeds_memory_address(body, spec.consumer_after_idx, value)
        {
            return false;
        }
    }
    true
}

#[cfg(debug_assertions)]
fn same_memarg(lhs: Option<crate::common::MemArg>, rhs: Option<crate::common::MemArg>) -> bool {
    match (lhs, rhs) {
        (Some(lhs), Some(rhs)) => lhs.align == rhs.align && lhs.offset == rhs.offset,
        (None, None) => true,
        _ => false,
    }
}

#[cfg(debug_assertions)]
fn spec_memarg(spec: &SpecializedMemoryLowering) -> Option<crate::common::MemArg> {
    let BlockOperand::Raw(operand) = *spec.operands.get(2)? else {
        return None;
    };
    Some(unsafe { operand.memarg })
}

#[cfg(debug_assertions)]
fn spec_memidx(spec: &SpecializedMemoryLowering) -> Option<u32> {
    match *spec.operands.get(3)? {
        BlockOperand::U32(memidx) => Some(memidx),
        BlockOperand::Raw(operand) => Some(unsafe { operand.u32 }),
        _ => None,
    }
}

fn relower_block_op(op: &BlockOp, spill_plan: &EffectResultSpillPlan) -> PackedOp {
    if op.kind == BlockOpKind::Select {
        if let Some(size) = block_op_select_size(op) {
            let typed_op = match size {
                4 => Some(vm::op_select4 as Op),
                8 => Some(vm::op_select8 as Op),
                16 => Some(vm::op_select16 as Op),
                _ => None,
            };
            if let Some(opcode) = typed_op {
                return pack_emitted_op(op.source_start, opcode, Vec::new());
            }
        }
    }
    pack_emitted_op(
        op.source_start,
        op.op,
        block_operands_to_raw_with_spills_for_op(op.op, &op.operands, spill_plan),
    )
}

fn relower_block_terminator(
    terminator: &BlockTerminator,
    spill_plan: &EffectResultSpillPlan,
) -> PackedOp {
    pack_emitted_op(
        terminator.source_start,
        terminator.op,
        terminator_operands_to_raw_with_spills(terminator, spill_plan),
    )
}

fn terminator_operands_to_raw_with_spills(
    terminator: &BlockTerminator,
    spill_plan: &EffectResultSpillPlan,
) -> Vec<Operand> {
    if std::ptr::fn_addr_eq(terminator.op, vm::op_local_get4_br_if as Op) {
        return vec![
            block_operand_to_raw_with_spills_for_op(
                terminator.op,
                &terminator.operands[1],
                spill_plan,
            ),
            block_operand_to_raw_with_spills_for_op(
                terminator.op,
                &terminator.operands[0],
                spill_plan,
            ),
        ];
    }
    if std::ptr::fn_addr_eq(terminator.op, vm::op_local_get4_i32_const_add_br_if as Op) {
        return vec![
            block_operand_to_raw_with_spills_for_op(
                terminator.op,
                &terminator.operands[1],
                spill_plan,
            ),
            block_operand_to_raw_with_spills_for_op(
                terminator.op,
                &terminator.operands[2],
                spill_plan,
            ),
            block_operand_to_raw_with_spills_for_op(
                terminator.op,
                &terminator.operands[0],
                spill_plan,
            ),
        ];
    }
    if std::ptr::fn_addr_eq(
        terminator.op,
        vm::op_local_get4_local_get4_i32_add_br_if as Op,
    ) {
        return vec![
            block_operand_to_raw_with_spills_for_op(
                terminator.op,
                &terminator.operands[1],
                spill_plan,
            ),
            block_operand_to_raw_with_spills_for_op(
                terminator.op,
                &terminator.operands[2],
                spill_plan,
            ),
            block_operand_to_raw_with_spills_for_op(
                terminator.op,
                &terminator.operands[0],
                spill_plan,
            ),
        ];
    }
    if std::ptr::fn_addr_eq(terminator.op, vm::op_local_get4_i32_eqz_br_if as Op) {
        return vec![
            block_operand_to_raw_with_spills_for_op(
                terminator.op,
                &terminator.operands[1],
                spill_plan,
            ),
            block_operand_to_raw_with_spills_for_op(
                terminator.op,
                &terminator.operands[0],
                spill_plan,
            ),
        ];
    }
    if std::ptr::fn_addr_eq(
        terminator.op,
        vm::op_local_get4_i32_const_compare_br_if as Op,
    ) {
        return vec![
            block_operand_to_raw_with_spills_for_op(
                terminator.op,
                &terminator.operands[1],
                spill_plan,
            ),
            block_operand_to_raw_with_spills_for_op(
                terminator.op,
                &terminator.operands[2],
                spill_plan,
            ),
            block_operand_to_raw_with_spills_for_op(
                terminator.op,
                &terminator.operands[3],
                spill_plan,
            ),
            block_operand_to_raw_with_spills_for_op(
                terminator.op,
                &terminator.operands[0],
                spill_plan,
            ),
        ];
    }
    if std::ptr::fn_addr_eq(
        terminator.op,
        vm::op_local_get4_local_get4_compare_br_if as Op,
    ) {
        return vec![
            block_operand_to_raw_with_spills_for_op(
                terminator.op,
                &terminator.operands[1],
                spill_plan,
            ),
            block_operand_to_raw_with_spills_for_op(
                terminator.op,
                &terminator.operands[2],
                spill_plan,
            ),
            block_operand_to_raw_with_spills_for_op(
                terminator.op,
                &terminator.operands[3],
                spill_plan,
            ),
            block_operand_to_raw_with_spills_for_op(
                terminator.op,
                &terminator.operands[0],
                spill_plan,
            ),
        ];
    }
    if std::ptr::fn_addr_eq(
        terminator.op,
        vm::op_local_get4_i32_const_add_tee4_br_if as Op,
    ) {
        return vec![
            block_operand_to_raw_with_spills_for_op(
                terminator.op,
                &terminator.operands[1],
                spill_plan,
            ),
            block_operand_to_raw_with_spills_for_op(
                terminator.op,
                &terminator.operands[2],
                spill_plan,
            ),
            block_operand_to_raw_with_spills_for_op(
                terminator.op,
                &terminator.operands[3],
                spill_plan,
            ),
            block_operand_to_raw_with_spills_for_op(
                terminator.op,
                &terminator.operands[0],
                spill_plan,
            ),
        ];
    }
    block_operands_to_raw_with_spills_for_op(terminator.op, &terminator.operands, spill_plan)
}

fn block_operand_to_raw_with_spills_for_op(
    op: Op,
    operand: &BlockOperand,
    spill_plan: &EffectResultSpillPlan,
) -> Operand {
    match operand {
        BlockOperand::SpillValue(value) => {
            let slot = spill_plan
                .slot(*value)
                .expect("spill placeholder must have an assigned temp local");
            Operand {
                local_addr: slot.addr,
            }
        }
        _ => block_operand_to_raw_for_op(op, operand),
    }
}

#[derive(Default)]
struct BlockOptimizer {
    block_id: usize,
    current_copy_plan: BlockCopyPlan,
    effect_epoch: EffectEpoch,
    builder: BlockBodyBuilder,
    exprs: ValueGraph,
    latest_by_origin: HashMap<ExprOrigin, ValueRef>,
    touched_values: Vec<ValueRef>,
    stack: Vec<ValueRef>,
    locals: HashMap<LocalSlot, ValueRef>,
    origin_locals: HashMap<ExprOrigin, LocalSlot>,
    cse: HashMap<ValueKey, CseEntry>,
    aliases: HashMap<AliasKey, ValueRef>,
    last_local_write: Option<LocalWrite>,
    last_store: HashMap<AliasKey, StoreWrite>,
    heap: HeapVersion,
    loop_invariants: LoopInvariantSet,
}

#[derive(Clone, Copy)]
struct LocalWrite {
    slot: LocalSlot,
    op_idx: usize,
    value: ValueRef,
}

#[derive(Clone, Copy)]
struct StoreWrite;

#[derive(Clone, Copy)]
struct CseEntry {
    expr: ValueRef,
    epoch: EffectEpoch,
}

enum AliasReuse {
    Rematerialized(ValueRef),
    SpillLocal(ValueRef),
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
            body: self.build_block_body(),
            loop_invariants: self.loop_invariants.clone(),
        }
    }
}

impl BlockOptimizer {
    fn reset(&mut self, block: BasicBlock, entry: &BlockEntryState) {
        self.block_id = block.id;
        self.effect_epoch = 0;
        self.builder = BlockBodyBuilder::default();
        for value in self.touched_values.drain(..) {
            if let Some(node) = self.exprs.nodes.get_mut(value.0) {
                node.use_count = 0;
                node.ref_count = 0;
            }
        }
        self.latest_by_origin.clear();
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
            self.register_existing_value(*value);
            let block_argument_local_bind_allowed = !self.exprs[value.0].is_block_argument()
                || self.current_copy_plan.locals.get(slot) == Some(&SlotMergeDecision::Preserve);
            if block_argument_local_bind_allowed {
                self.bind_local(*slot, *value);
                self.seed_cse(*value);
            }
            self.maybe_mark_loop_invariant(*value);
        }

        for (ordinal, value) in entry.stack.iter().enumerate() {
            self.register_existing_value(*value);
            let block_argument_seed_allowed = !self.exprs[value.0].is_block_argument()
                || self.current_copy_plan.stack.get(&ordinal) == Some(&SlotMergeDecision::Preserve);
            if block_argument_seed_allowed {
                if let Some(slot) = effective_slot_shape(&self.exprs, *value)
                    .and_then(|shape| shape.slot)
                    .and_then(materializable_slot)
                {
                    if self.exprs[value.0].origin.kind != ExprOriginKind::BlockArgument {
                        self.origin_locals
                            .entry(self.exprs[value.0].origin)
                            .or_insert(slot);
                    }
                }
            }
            self.push_stack(*value);
            self.seed_cse(*value);
            self.maybe_mark_loop_invariant(*value);
        }

        let mut aliases = entry.aliases.iter().collect::<Vec<_>>();
        aliases.sort_by_key(|(key, _)| (key.space as u8, key.index, key.offset, key.width));
        for (key, value) in aliases {
            self.register_existing_value(*value);
            self.aliases.insert(key.clone(), *value);
            self.maybe_mark_loop_invariant(*value);
        }
    }

    fn register_existing_value(&mut self, value: ValueRef) {
        self.touch_value(value);
        self.latest_by_origin
            .insert(self.exprs[value.0].origin, value);
    }

    fn seed_cse(&mut self, expr: ValueRef) {
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
        if record.op_eq(vm::op_br_table) {
            self.visit_br_table(record, ordinal);
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
        if is_memory_grow_op(record.op) {
            self.emit_explicit_barrier_results(
                record,
                EffectBarrier::Memory,
                1,
                &[ValType::I32],
                ordinal,
            );
            return;
        }
        #[cfg(feature = "threads")]
        if let Some(shape) = decode_atomic_barrier_shape(record) {
            self.emit_explicit_barrier_results(
                record,
                EffectBarrier::Memory,
                shape.input_count,
                &[ValType::I32],
                ordinal,
            );
            return;
        }
        if let Some(shape) = decode_trap_sensitive_barrier_shape(record) {
            self.emit_explicit_barrier_results(
                record,
                EffectBarrier::TrapSensitive,
                shape.input_count,
                &[shape.result_ty],
                ordinal,
            );
            return;
        }
        self.emit_barrier(record, ordinal);
    }

    fn visit_local_get(&mut self, record: &DecodedInstr, slot: LocalSlot, _ordinal: usize) {
        if let Some(write) = self.last_local_write {
            if write.slot == slot && self.builder.last_alive_index() == Some(write.op_idx) {
                if let Some(last) = self.builder.entry_mut(write.op_idx) {
                    if let Some(tee_op) = set_to_tee(last.op, slot.size) {
                        last.op = tee_op;
                        last.kind = PendingBlockEntryKind::Op(classify_block_op_kind(tee_op));
                        let source = write.value;
                        self.push_stack(source);
                        self.last_local_write = Some(LocalWrite {
                            slot,
                            op_idx: write.op_idx,
                            value: source,
                        });
                        return;
                    }
                }
            }
        }

        let source = self.locals.get(&slot).copied();
        if let Some(source) = source {
            let source_state = &self.exprs[source.0];
            let source_is_block_argument = source_state.origin.kind
                == ExprOriginKind::BlockArgument
                || source_state.is_block_argument();
            let allow_rematerialization =
                !source_is_block_argument || source_state.const_value.is_some();
            if allow_rematerialization {
                if let Some(materialized) = self.try_materialize_value(record.old_start, source) {
                    self.last_local_write = None;
                    self.push_stack(materialized);
                    return;
                }
            }
        }

        let block_argument_source = source.is_some_and(|source| {
            let state = &self.exprs[source.0];
            state.origin.kind == ExprOriginKind::BlockArgument || state.is_block_argument()
        });
        let op_idx = self.push_original(record);
        self.last_local_write = None;
        let expr = if let Some(source) = source {
            let source_state = self.exprs[source.0].clone();
            if block_argument_source {
                let expr = self.new_expr_with_origin(
                    source_state.ty,
                    source_state.origin,
                    source_state.const_value,
                    None,
                    ValueDef::Synthetic,
                    Some(op_idx),
                    true,
                );
                set_value_slot_shape(
                    &mut self.exprs,
                    expr,
                    Some(slot_ref_for_local_slot(slot)),
                    source_state.address_shape,
                    source_state.loop_value_shape.clone(),
                );
                expr
            } else {
                let expr = self.new_expr_with_origin(
                    source_state.ty,
                    source_state.origin,
                    source_state.const_value,
                    source_state.key,
                    ValueDef::Synthetic,
                    Some(op_idx),
                    true,
                );
                self.copy_value_shapes_from(expr, source);
                expr
            }
        } else {
            let ty = record
                .stack_after
                .types
                .last()
                .copied()
                .unwrap_or_else(|| type_from_slot(slot.size));
            let expr = self.new_expr_with_origin(
                ty,
                ExprOrigin {
                    block_id: self.block_id,
                    ordinal: slot.addr as usize,
                    kind: ExprOriginKind::EntryLocal,
                },
                None,
                None,
                ValueDef::Synthetic,
                Some(op_idx),
                true,
            );
            let ty = self.exprs[expr.0].ty;
            set_value_slot_shape(
                &mut self.exprs,
                expr,
                Some(slot_ref_for_local_slot(slot)),
                entry_local_address_shape(slot, ty),
                entry_local_loop_value_shape(slot, ty),
            );
            expr
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
        if self.locals.get(&slot).is_some_and(|current| {
            let current_state = &self.exprs[current.0];
            let value_state = &self.exprs[value.0];
            let block_argument_elision_safe = (!current_state.is_block_argument()
                && !value_state.is_block_argument())
                || (current_state.slot_shape.is_some()
                    && current_state.slot_shape == value_state.slot_shape);
            block_argument_elision_safe && same_expr(current_state, value_state)
        }) {
            self.last_local_write = None;
            if is_tee {
                self.push_stack(value);
            } else {
                let _ = self.try_remove_expr(value);
            }
            return;
        }
        let op_idx = self.push_original(record);
        if let Some(entry) = self.builder.entry_mut(op_idx) {
            entry.inputs = vec![value];
        }
        self.bind_local(slot, value);
        self.last_local_write = Some(LocalWrite {
            slot,
            op_idx,
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
            let removable_other = if chosen == lhs {
                self.can_remove_expr(rhs)
            } else {
                self.can_remove_expr(lhs)
            };
            if self.can_remove_expr(cond) && removable_other {
                let cond_removed = self.try_remove_expr(cond);
                let dropped = if chosen == lhs {
                    self.try_remove_expr(rhs)
                } else {
                    self.try_remove_expr(lhs)
                };
                debug_assert!(cond_removed && dropped);
                self.push_stack(chosen);
                self.incref(chosen);
                return;
            }
        }

        let op_idx = self.push_effect_op(record);
        if let Some(entry) = self.builder.entry_mut(op_idx.0) {
            entry.inputs = vec![lhs, rhs, cond];
        }
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
            ValueDef::EffectResult(op_idx, 0),
            Some(op_idx.0),
            false,
        );
        if let Some(shape) = shared_select_slot_shape(&self.exprs, lhs, rhs, select_size) {
            set_value_slot_shape(
                &mut self.exprs,
                expr,
                shape.slot,
                shape.address,
                shape.loop_value,
            );
        }
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
                if let Some(materialized) = self.try_materialize_value(record.old_start, source) {
                    self.try_remove_expr(value);
                    self.push_stack(materialized);
                    return;
                }
            }
        }
        let op_idx = self.push_original(record);
        if let Some(entry) = self.builder.entry_mut(op_idx) {
            entry.inputs = vec![value];
        }
        let expr = self.new_expr_with_origin(
            unary_output_type(op),
            ExprOrigin {
                block_id: self.block_id,
                ordinal: instr_result_origin_ordinal(ordinal, 0),
                kind: ExprOriginKind::InstrResult,
            },
            None,
            Some(key),
            ValueDef::Instr,
            Some(op_idx),
            true,
        );
        self.set_value_shapes(
            expr,
            None,
            derive_unary_loop_value_shape(&self.exprs, op, value),
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
                if self.can_remove_expr(lhs) && self.can_remove_expr(rhs) {
                    self.try_remove_expr(lhs);
                    self.try_remove_expr(rhs);
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
                if let Some(materialized) = self.try_materialize_value(record.old_start, source) {
                    self.try_remove_expr(lhs);
                    self.try_remove_expr(rhs);
                    self.push_stack(materialized);
                    return;
                }
            }
        }

        let op_idx = self.push_original(record);
        if let Some(entry) = self.builder.entry_mut(op_idx) {
            entry.inputs = vec![lhs, rhs];
        }
        let expr = self.new_expr_with_origin(
            binary_output_type(op),
            ExprOrigin {
                block_id: self.block_id,
                ordinal: instr_result_origin_ordinal(ordinal, 0),
                kind: ExprOriginKind::InstrResult,
            },
            None,
            Some(key),
            ValueDef::Instr,
            Some(op_idx),
            true,
        );
        let address_shape = derive_binary_address_shape(&self.exprs, op, lhs, rhs);
        let loop_value_shape = derive_binary_loop_value_shape(&self.exprs, op, lhs, rhs);
        self.set_value_shapes(expr, address_shape, loop_value_shape);
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
                    let inputs = self.barrier_inputs(record);
                    let op_idx = self.builder.push_raw(
                        Some(record.old_start),
                        vm::op_br,
                        vec![Operand {
                            jump_addr: record.operand_jump_addr(0) as u32,
                        }],
                    );
                    if let Some(entry) = self.builder.entry_mut(op_idx) {
                        entry.inputs = inputs;
                    }
                    self.bind_results_from_snapshot(record, EffectOpId(op_idx), ordinal);
                } else {
                    self.apply_fallthrough_snapshot(record);
                }
                return;
            }
        }
        let op_idx = self.push_effect_op(record);
        if let Some(entry) = self.builder.entry_mut(op_idx.0) {
            entry.inputs = vec![cond];
        }
        self.bind_results_from_snapshot(record, op_idx, ordinal);
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
                    let inputs = self.barrier_inputs(record);
                    let op_idx = self.builder.push_raw(
                        Some(record.old_start),
                        vm::op_br,
                        vec![Operand {
                            jump_addr: record.operand_jump_addr(0) as u32,
                        }],
                    );
                    if let Some(entry) = self.builder.entry_mut(op_idx) {
                        entry.inputs = inputs;
                    }
                    self.bind_results_from_snapshot(record, EffectOpId(op_idx), ordinal);
                } else {
                    self.apply_fallthrough_snapshot(record);
                }
                return;
            }
        }
        let op_idx = self.push_effect_op(record);
        if let Some(entry) = self.builder.entry_mut(op_idx.0) {
            entry.inputs = vec![cond];
        }
        self.bind_results_from_snapshot(record, op_idx, ordinal);
    }

    fn visit_br_table(&mut self, record: &DecodedInstr, ordinal: usize) {
        let Some(index) = self.pop_stack() else {
            self.emit_barrier(record, ordinal);
            return;
        };
        self.last_local_write = None;
        let op_idx = self.push_effect_op(record);
        if let Some(entry) = self.builder.entry_mut(op_idx.0) {
            entry.inputs = vec![index];
        }
        self.apply_barrier(effect_barrier(record));
        self.bind_results_from_snapshot(record, op_idx, ordinal);
    }

    fn visit_global_get(&mut self, record: &DecodedInstr, slot: LocalSlot, ordinal: usize) {
        self.last_local_write = None;
        let key = Self::global_alias_key(slot);
        if let Some(source) = self.aliases.get(&key).copied() {
            if let Some(reuse) = self.try_reuse_alias_value(record.old_start, source) {
                let reused = match reuse {
                    AliasReuse::Rematerialized(materialized) => {
                        debug_assert!(self.can_materialize(source));
                        materialized
                    }
                    AliasReuse::SpillLocal(materialized) => {
                        debug_assert!(self.exprs[source.0].is_effect_result());
                        materialized
                    }
                };
                self.push_stack(reused);
                return;
            }
        }
        self.bump_effect_epoch();
        let op_idx = self.push_effect_op(record);
        let ty = record
            .stack_after
            .types
            .last()
            .copied()
            .unwrap_or_else(|| type_from_slot(slot.size));
        let expr = self.new_expr_with_origin(
            ty,
            ExprOrigin {
                block_id: self.block_id,
                ordinal,
                kind: ExprOriginKind::GlobalValue,
            },
            None,
            None,
            ValueDef::EffectResult(op_idx, 0),
            Some(op_idx.0),
            false,
        );
        self.bind_alias_value(key, expr);
        self.push_stack(expr);
    }

    fn visit_global_set(&mut self, record: &DecodedInstr, _slot: LocalSlot) {
        let Some(value) = self.pop_stack() else {
            self.emit_barrier(record, 0);
            return;
        };
        self.last_local_write = None;
        self.bump_effect_epoch();
        clear_alias_space_rewrite(&mut self.aliases, &mut self.last_store, AliasSpace::Global);
        let op_idx = self.push_original(record);
        if let Some(entry) = self.builder.entry_mut(op_idx) {
            entry.inputs = vec![value];
        }
        self.heap.global = self.heap.global.saturating_add(1);
        self.bind_alias_value(Self::global_alias_key(_slot), value);
    }

    fn visit_table_get(&mut self, record: &DecodedInstr, _tableidx: u32, ordinal: usize) {
        let Some(_index) = self.pop_stack() else {
            self.emit_barrier(record, ordinal);
            return;
        };
        self.last_local_write = None;
        self.bump_effect_epoch();
        let op_idx = self.push_effect_op(record);
        if let Some(entry) = self.builder.entry_mut(op_idx.0) {
            entry.inputs = vec![_index];
        }
        let expr = self.new_expr_with_origin(
            ValType::FuncRef,
            ExprOrigin {
                block_id: self.block_id,
                ordinal,
                kind: ExprOriginKind::TableValue,
            },
            None,
            None,
            ValueDef::EffectResult(op_idx, 0),
            Some(op_idx.0),
            false,
        );
        self.push_stack(expr);
    }

    fn visit_table_set(&mut self, record: &DecodedInstr, _tableidx: u32) {
        let Some(value) = self.pop_stack() else {
            self.emit_barrier(record, 0);
            return;
        };
        let Some(_index) = self.pop_stack() else {
            self.incref(value);
            self.push_stack(value);
            self.emit_barrier(record, 0);
            return;
        };
        self.last_local_write = None;
        self.bump_effect_epoch();
        clear_alias_space_rewrite(&mut self.aliases, &mut self.last_store, AliasSpace::Table);
        let op_idx = self.push_original(record);
        if let Some(entry) = self.builder.entry_mut(op_idx) {
            entry.inputs = vec![_index, value];
        }
        self.heap.table = self.heap.table.saturating_add(1);
    }

    fn visit_memory_load(&mut self, record: &DecodedInstr, access: MemoryAccess, ordinal: usize) {
        let Some(address) = self.pop_stack() else {
            self.emit_barrier(record, ordinal);
            return;
        };
        self.last_local_write = None;
        let alias_key = self.memory_alias_key(access, address);
        if let Some(key) = alias_key.as_ref() {
            if let Some(source) = self.aliases.get(key).copied() {
                if self.can_remove_expr_tree(address) {
                    if let Some(reuse) = self.try_reuse_alias_value(record.old_start, source) {
                        let reused = match reuse {
                            AliasReuse::Rematerialized(materialized) => {
                                debug_assert!(self.can_materialize(source));
                                materialized
                            }
                            AliasReuse::SpillLocal(materialized) => {
                                debug_assert!(self.exprs[source.0].is_effect_result());
                                materialized
                            }
                        };
                        debug_assert_eq!(self.exprs[source.0].ty, access.ty);
                        self.remove_expr_tree(address);
                        self.push_stack(reused);
                        return;
                    }
                }
            }
        }
        self.bump_effect_epoch();
        let op_idx = self.push_effect_op(record);
        if let Some(entry) = self.builder.entry_mut(op_idx.0) {
            entry.inputs = vec![address];
        }
        let expr = self.new_expr_with_origin(
            access.ty,
            ExprOrigin {
                block_id: self.block_id,
                ordinal,
                kind: ExprOriginKind::MemoryValue,
            },
            None,
            None,
            ValueDef::EffectResult(op_idx, 0),
            Some(op_idx.0),
            false,
        );
        if let Some(key) = alias_key {
            self.bind_alias_value(key, expr);
        }
        self.push_stack(expr);
    }

    fn visit_memory_store(
        &mut self,
        record: &DecodedInstr,
        _access: MemoryAccess,
        _ordinal: usize,
    ) {
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
        let alias_key = self.memory_alias_key(_access, address);
        let op_idx = self.push_original(record);
        if let Some(entry) = self.builder.entry_mut(op_idx) {
            entry.inputs = vec![address, value];
        }
        self.heap.memory = self.heap.memory.saturating_add(1);
        if let Some(key) = alias_key {
            self.bind_store_alias_value(key, value, op_idx);
        }
    }

    fn emit_barrier(&mut self, record: &DecodedInstr, ordinal: usize) {
        self.last_local_write = None;
        let barrier = effect_barrier(record);
        let op_idx = self.push_effect_op(record);
        let inputs = self.barrier_inputs(record);
        if let Some(entry) = self.builder.entry_mut(op_idx.0) {
            entry.inputs = inputs;
        }
        self.apply_barrier(barrier);
        self.bind_results_from_snapshot(record, op_idx, ordinal);
    }

    fn emit_explicit_barrier_results(
        &mut self,
        record: &DecodedInstr,
        barrier: EffectBarrier,
        input_count: usize,
        result_types: &[ValType],
        ordinal: usize,
    ) {
        self.last_local_write = None;
        let op_idx = self.push_effect_op(record);
        let inputs = self.explicit_barrier_inputs(input_count);
        if let Some(entry) = self.builder.entry_mut(op_idx.0) {
            entry.inputs = inputs;
        }
        self.apply_barrier(barrier);
        let preserved_prefix_len = record
            .stack_before
            .types
            .len()
            .saturating_sub(input_count)
            .min(self.stack.len());
        debug_assert_eq!(
            preserved_prefix_len + result_types.len(),
            record.stack_after.types.len(),
            "explicit barrier shape must match stack_after metadata",
        );
        while self.stack.len() > preserved_prefix_len {
            let _ = self.pop_stack();
        }
        for (result_idx, ty) in result_types.iter().enumerate() {
            let expr = self.new_expr_with_origin(
                *ty,
                ExprOrigin {
                    block_id: self.block_id,
                    ordinal: instr_result_origin_ordinal(ordinal, result_idx),
                    kind: ExprOriginKind::InstrResult,
                },
                None,
                None,
                ValueDef::EffectResult(op_idx, result_idx),
                Some(op_idx.0),
                false,
            );
            self.push_stack(expr);
        }
    }

    fn apply_barrier(&mut self, barrier: EffectBarrier) {
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
    }

    fn barrier_inputs(&self, record: &DecodedInstr) -> Vec<ValueRef> {
        let preserved_prefix_len = record.preserved_prefix_len.min(self.stack.len());
        self.stack[preserved_prefix_len..].to_vec()
    }

    fn explicit_barrier_inputs(&self, input_count: usize) -> Vec<ValueRef> {
        self.stack
            .len()
            .checked_sub(input_count)
            .map(|start| self.stack[start..].to_vec())
            .unwrap_or_default()
    }

    fn apply_fallthrough_snapshot(&mut self, record: &DecodedInstr) {
        let preserved_prefix_len = record
            .preserved_prefix_len
            .min(self.stack.len())
            .min(record.stack_after.types.len());
        while self.stack.len() > preserved_prefix_len {
            let _ = self.pop_stack();
        }
        debug_assert!(
            record.fresh_result_count == 0,
            "const-folded control should not synthesize fresh results"
        );
    }

    fn emit_const(&mut self, source_start: usize, ty: ValType, value: ConstValue, ordinal: usize) {
        let (op, operand) = match value {
            ConstValue::I32(value) => (vm::op_i32_const as Op, Operand { i32: value }),
            ConstValue::I64(value) => (vm::op_i64_const as Op, Operand { i64: value }),
            ConstValue::F32(value) => (vm::op_f32_const as Op, Operand { f32: value }),
            ConstValue::F64(value) => (vm::op_f64_const as Op, Operand { f64: value }),
        };
        let op_idx = self.builder.push_raw(Some(source_start), op, vec![operand]);
        let expr = self.new_expr_with_origin(
            ty,
            ExprOrigin {
                block_id: self.block_id,
                ordinal,
                kind: ExprOriginKind::SyntheticConst,
            },
            Some(value),
            None,
            ValueDef::Const,
            Some(op_idx),
            true,
        );
        self.push_stack(expr);
    }

    fn push_original(&mut self, record: &DecodedInstr) -> usize {
        self.builder
            .push_raw(Some(record.old_start), record.op, record.operands.clone())
    }

    fn push_effect_op(&mut self, record: &DecodedInstr) -> EffectOpId {
        EffectOpId(self.push_original(record))
    }

    fn bind_local(&mut self, slot: LocalSlot, expr: ValueRef) {
        if let Some(previous) = self.locals.insert(slot, expr) {
            self.decref(previous);
            if self.exprs[previous.0].origin.kind != ExprOriginKind::BlockArgument
                && self.origin_locals.get(&self.exprs[previous.0].origin) == Some(&slot)
            {
                self.origin_locals.remove(&self.exprs[previous.0].origin);
            }
        }
        if self.exprs[expr.0].origin.kind != ExprOriginKind::BlockArgument {
            self.origin_locals.insert(self.exprs[expr.0].origin, slot);
        }
        self.incref(expr);
    }

    fn push_stack(&mut self, expr: ValueRef) {
        self.stack.push(expr);
        self.incref(expr);
    }

    fn pop_stack(&mut self) -> Option<ValueRef> {
        let expr = self.stack.pop()?;
        self.decref(expr);
        self.touch_value(expr);
        self.exprs[expr.0].use_count = self.exprs[expr.0].use_count.saturating_add(1);
        Some(expr)
    }

    fn incref(&mut self, expr: ValueRef) {
        self.touch_value(expr);
        self.exprs[expr.0].ref_count += 1;
    }

    fn decref(&mut self, expr: ValueRef) {
        self.touch_value(expr);
        self.exprs[expr.0].ref_count = self.exprs[expr.0].ref_count.saturating_sub(1);
    }

    fn try_remove_expr(&mut self, expr: ValueRef) -> bool {
        if !self.can_remove_expr(expr) {
            return false;
        }
        let state = &self.exprs[expr.0];
        let Some(op_idx) = state.producer_op else {
            return false;
        };
        self.builder.remove(op_idx);
        true
    }

    fn can_remove_expr_tree(&self, expr: ValueRef) -> bool {
        if !self.can_remove_expr(expr) {
            return false;
        }
        match self.exprs[expr.0].key {
            Some(ValueKey::Unary { input, .. }) => self
                .latest_by_origin
                .get(&input)
                .copied()
                .is_some_and(|input| self.can_remove_expr_tree(input)),
            Some(ValueKey::Binary { lhs, rhs, .. }) => {
                self.latest_by_origin
                    .get(&lhs)
                    .copied()
                    .is_some_and(|lhs| self.can_remove_expr_tree(lhs))
                    && self
                        .latest_by_origin
                        .get(&rhs)
                        .copied()
                        .is_some_and(|rhs| self.can_remove_expr_tree(rhs))
            }
            None => true,
        }
    }

    fn remove_expr_tree(&mut self, expr: ValueRef) {
        if !self.can_remove_expr_tree(expr) {
            return;
        }
        let state = self.exprs[expr.0].clone();
        let Some(op_idx) = state.producer_op else {
            return;
        };
        self.builder.remove(op_idx);
        match state.key {
            Some(ValueKey::Unary { input, .. }) => {
                if let Some(input) = self.latest_by_origin.get(&input).copied() {
                    self.remove_expr_tree(input);
                }
            }
            Some(ValueKey::Binary { lhs, rhs, .. }) => {
                if let Some(lhs) = self.latest_by_origin.get(&lhs).copied() {
                    self.remove_expr_tree(lhs);
                }
                if let Some(rhs) = self.latest_by_origin.get(&rhs).copied() {
                    self.remove_expr_tree(rhs);
                }
            }
            None => {}
        }
    }

    fn can_remove_expr(&self, expr: ValueRef) -> bool {
        let state = &self.exprs[expr.0];
        state.ref_count == 0
            && state.removable
            && state.producer_op.is_some()
            && state.materialized_block == Some(self.block_id)
            && !self.builder_uses_expr(expr, state.producer_op)
    }

    fn builder_uses_expr(&self, expr: ValueRef, producer_op: Option<usize>) -> bool {
        self.builder
            .live_entries()
            .any(|(idx, entry)| Some(idx) != producer_op && entry.inputs.contains(&expr))
    }

    fn can_materialize(&self, expr: ValueRef) -> bool {
        let state = &self.exprs[expr.0];
        state.const_value.is_some()
            || self.origin_locals.contains_key(&state.origin)
            || self.can_materialize_key(state.key)
    }

    fn bump_effect_epoch(&mut self) {
        self.effect_epoch += 1;
        self.cse.clear();
    }

    fn pin_stack_prefix_values(&mut self, prefix_len: usize) {
        let preserved = self
            .stack
            .iter()
            .take(prefix_len)
            .copied()
            .collect::<Vec<_>>();
        for value in preserved {
            self.incref(value);
        }
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
            state.locals.insert(*slot, *expr);
        }

        for expr in &self.stack {
            state.stack.push(*expr);
        }

        let mut aliases = self.aliases.iter().collect::<Vec<_>>();
        aliases.sort_by_key(|(key, _)| (key.space as u8, key.index, key.offset, key.width));
        for (key, expr) in aliases {
            state.aliases.insert(key.clone(), *expr);
        }

        state
    }

    fn build_block_body(&self) -> BlockBody {
        let mut values_by_op = HashMap::new();
        for (expr_idx, expr) in self.exprs.nodes.iter().enumerate() {
            if expr.materialized_block != Some(self.block_id) {
                continue;
            }
            if let Some(op_idx) = expr.materialized_op {
                values_by_op
                    .entry(op_idx)
                    .or_insert_with(Vec::new)
                    .push(ExprId(expr_idx));
            }
        }
        for values in values_by_op.values_mut() {
            values.sort_by_key(|value| {
                self.exprs[value.0]
                    .effect_result()
                    .map(|(_, result_index)| result_index)
                    .unwrap_or(0)
            });
            values.dedup();
        }
        let mut body = BlockBody::default();
        for (op_idx, entry) in self.builder.live_entries() {
            let values = values_by_op.remove(&op_idx).unwrap_or_default();
            match entry.kind {
                PendingBlockEntryKind::Op(kind) => body.ops.push(BlockOp {
                    source_start: entry.source_start,
                    op: entry.op,
                    kind,
                    operands: entry.operands.clone(),
                    inputs: entry.inputs.clone(),
                    values,
                }),
                PendingBlockEntryKind::Terminator(kind) => {
                    body.terminator = Some(BlockTerminator {
                        source_start: entry.source_start,
                        op: entry.op,
                        kind,
                        operands: entry.operands.clone(),
                        inputs: entry.inputs.clone(),
                        values,
                    });
                }
            }
        }
        body
    }

    fn global_alias_key(slot: LocalSlot) -> AliasKey {
        AliasKey {
            space: AliasSpace::Global,
            index: slot.addr,
            offset: 0,
            width: slot.size as u8,
            address: AliasAddress::Const(0),
        }
    }

    fn memory_alias_key(&self, access: MemoryAccess, address: ValueRef) -> Option<AliasKey> {
        Some(AliasKey {
            space: AliasSpace::Memory,
            index: access.memidx,
            offset: access.offset,
            width: access.width,
            address: self.alias_address_for_value(address)?,
        })
    }

    fn alias_address_for_value(&self, value: ValueRef) -> Option<AliasAddress> {
        let state = &self.exprs[value.0];
        if state.ty != ValType::I32 {
            return None;
        }
        if let Some(ConstValue::I32(value)) = state.const_value {
            return Some(AliasAddress::Const(value as u32));
        }
        match state.origin.kind {
            ExprOriginKind::EntryLocal | ExprOriginKind::BlockArgument => {
                return Some(AliasAddress::Origin(state.origin));
            }
            _ => {}
        }
        match state.key {
            Some(ValueKey::Unary { op, input })
                if alias_address_supports_pure_chain(op)
                    && unary_output_type(op) == ValType::I32 =>
            {
                let input = self.latest_by_origin.get(&input).copied()?;
                Some(AliasAddress::Unary {
                    op,
                    input: Box::new(self.alias_address_for_value(input)?),
                })
            }
            Some(ValueKey::Binary { op, lhs, rhs })
                if alias_address_supports_pure_chain(op)
                    && binary_output_type(op) == ValType::I32 =>
            {
                let lhs = self.latest_by_origin.get(&lhs).copied()?;
                let rhs = self.latest_by_origin.get(&rhs).copied()?;
                Some(AliasAddress::Binary {
                    op,
                    lhs: Box::new(self.alias_address_for_value(lhs)?),
                    rhs: Box::new(self.alias_address_for_value(rhs)?),
                })
            }
            _ => Some(AliasAddress::Origin(state.origin)),
        }
    }

    fn try_reuse_alias_value(
        &mut self,
        source_start: usize,
        source: ValueRef,
    ) -> Option<AliasReuse> {
        if let Some(materialized) = self.try_materialize_value(source_start, source) {
            return Some(AliasReuse::Rematerialized(materialized));
        }
        self.try_materialize_effect_result_from_spill(source_start, source)
            .map(AliasReuse::SpillLocal)
    }

    fn bind_alias_value(&mut self, key: AliasKey, value: ValueRef) {
        self.aliases.insert(key, value);
    }

    fn bind_store_alias_value(&mut self, key: AliasKey, value: ValueRef, producer_op: usize) {
        self.aliases.insert(key.clone(), value);
        let _ = producer_op;
        self.last_store.insert(key, StoreWrite);
    }

    fn can_materialize_key(&self, key: Option<ValueKey>) -> bool {
        let Some(key) = key else {
            return false;
        };
        match key {
            ValueKey::Unary { input, .. } => self
                .latest_by_origin
                .get(&input)
                .copied()
                .is_some_and(|expr| self.can_materialize(expr)),
            ValueKey::Binary { lhs, rhs, .. } => {
                self.latest_by_origin
                    .get(&lhs)
                    .copied()
                    .is_some_and(|expr| self.can_materialize(expr))
                    && self
                        .latest_by_origin
                        .get(&rhs)
                        .copied()
                        .is_some_and(|expr| self.can_materialize(expr))
            }
        }
    }

    fn bind_results_from_snapshot(
        &mut self,
        record: &DecodedInstr,
        op_id: EffectOpId,
        ordinal: usize,
    ) {
        let keep_full_stack = !record.stack_after.reachable
            && full_stack_live_on_unreachable_control_transfer(record.op);
        let pin_prefix_len = if keep_full_stack {
            self.stack.len()
        } else if !record.stack_after.reachable {
            if preserved_values_cross_control_boundary(record.op) {
                record
                    .preserved_prefix_len
                    .min(self.stack.len())
                    .min(record.stack_after.types.len())
            } else {
                0
            }
        } else if preserved_values_cross_control_boundary(record.op) {
            record
                .preserved_prefix_len
                .min(self.stack.len())
                .min(record.stack_after.types.len())
        } else {
            0
        };
        if pin_prefix_len > 0 {
            self.pin_stack_prefix_values(pin_prefix_len);
        }
        if keep_full_stack {
            return;
        }
        let preserved_prefix_len = record
            .preserved_prefix_len
            .min(self.stack.len())
            .min(record.stack_after.types.len());
        while self.stack.len() > preserved_prefix_len {
            let _ = self.pop_stack();
        }
        debug_assert!(
            self.stack
                .iter()
                .zip(record.stack_after.types.iter().take(preserved_prefix_len))
                .all(|(value, ty)| self.exprs[value.0].ty == *ty),
            "preserved stack prefix must match stack_after metadata: block={} source_start={:?} preserved_prefix_len={} stack_before={:?} stack_types={:?} expected_prefix={:?} stack_after={:?} op_ptr={:p}",
            self.block_id,
            record.old_start,
            preserved_prefix_len,
            record.stack_before.types,
            self.stack
                .iter()
                .map(|value| self.exprs[value.0].ty)
                .collect::<Vec<_>>(),
            record.stack_after.types.iter().take(preserved_prefix_len).collect::<Vec<_>>(),
            record.stack_after.types,
            record.op,
        );
        if !record.stack_after.reachable {
            return;
        }
        let fresh_types = &record.stack_after.types[preserved_prefix_len..];
        debug_assert_eq!(fresh_types.len(), record.fresh_result_count);
        for (result_idx, ty) in fresh_types.iter().enumerate() {
            let expr = self.new_expr_with_origin(
                *ty,
                ExprOrigin {
                    block_id: self.block_id,
                    ordinal: instr_result_origin_ordinal(ordinal, result_idx),
                    kind: ExprOriginKind::InstrResult,
                },
                None,
                None,
                ValueDef::EffectResult(op_id, result_idx),
                Some(op_id.0),
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
        const_value: Option<ConstValue>,
        key: Option<ValueKey>,
        def: ValueDef,
        producer_op: Option<usize>,
        removable: bool,
    ) -> ValueRef {
        let id = ExprId(self.exprs.nodes.len());
        self.exprs.nodes.push(ExprState {
            ty,
            origin,
            def,
            const_value,
            key,
            address_shape: None,
            loop_value_shape: None,
            slot_shape: None,
            provider_class: ProviderClass::None,
            materialization_cost: MaterializationCost::Unknown,
            producer_op,
            materialized_block: producer_op.map(|_| self.block_id),
            materialized_op: producer_op,
            needs_spill: false,
            use_count: 0,
            ref_count: 0,
            removable,
        });
        self.exprs.nodes[id.0].refresh_optimizer_metadata();
        self.touch_value(id);
        self.latest_by_origin.insert(origin, id);
        id
    }

    fn set_value_shapes(
        &mut self,
        value: ValueRef,
        address_shape: Option<AddressShape>,
        loop_value_shape: Option<LoopValueShape>,
    ) {
        let slot = self.exprs[value.0]
            .slot_shape
            .as_ref()
            .and_then(|shape| shape.slot);
        set_value_slot_shape(
            &mut self.exprs,
            value,
            slot,
            address_shape,
            loop_value_shape,
        );
    }

    fn copy_value_shapes_from(&mut self, target: ValueRef, source: ValueRef) {
        let source_state = self.exprs[source.0].clone();
        self.exprs[target.0].address_shape = source_state.address_shape;
        self.exprs[target.0].loop_value_shape = source_state.loop_value_shape.clone();
        self.exprs[target.0].slot_shape = source_state.slot_shape;
        self.exprs[target.0].refresh_optimizer_metadata();
    }

    fn lookup_cse_source(&self, key: ValueKey) -> Option<ValueRef> {
        let entry = self.cse.get(&key).copied()?;
        (entry.epoch == self.effect_epoch).then_some(entry.expr)
    }

    fn try_materialize_value(
        &mut self,
        _source_start: usize,
        source: ValueRef,
    ) -> Option<ValueRef> {
        let source_state = self.exprs[source.0].clone();
        let source_is_block_argument = source_state.origin.kind == ExprOriginKind::BlockArgument
            || source_state.is_block_argument();
        if !source_is_block_argument {
            return None;
        }
        let local_slot = effective_slot_shape(&self.exprs, source)
            .and_then(|shape| shape.slot)
            .and_then(materializable_slot);
        if source_is_block_argument
            && effective_slot_shape(&self.exprs, source)
                .and_then(|shape| shape.slot)
                .and_then(materializable_slot)
                .is_none()
        {
            return None;
        }
        if let Some(slot) = local_slot {
            let op = local_get_op(slot.size);
            let op_idx = self.builder.push_raw(
                None,
                op,
                vec![Operand {
                    local_addr: slot.addr,
                }],
            );
            let source_state = self.exprs[source.0].clone();
            let expr = self.new_expr_with_origin(
                source_state.ty,
                source_state.origin,
                source_state.const_value,
                source_state.key,
                ValueDef::Synthetic,
                Some(op_idx),
                true,
            );
            self.copy_value_shapes_from(expr, source);
            return Some(expr);
        }
        None
    }

    fn try_materialize_effect_result_from_spill(
        &mut self,
        source_start: usize,
        source: ValueRef,
    ) -> Option<ValueRef> {
        let source_state = self.exprs[source.0].clone();
        if !source_state.is_effect_result() {
            return None;
        }
        let size = value_type_size(source_state.ty)?;
        self.exprs[source.0].needs_spill = true;
        let op_idx = self
            .builder
            .push_spill_local_get(Some(source_start), source, size);
        let expr = self.new_expr_with_origin(
            source_state.ty,
            source_state.origin,
            source_state.const_value,
            source_state.key,
            source_state.def,
            Some(op_idx),
            true,
        );
        let slot = symbolic_spill_slot(source, size);
        set_value_slot_shape(
            &mut self.exprs,
            expr,
            Some(SlotRef::spill_local(slot)),
            spill_local_address_shape(slot, source_state.ty),
            spill_local_loop_value_shape(slot, source_state.ty),
        );
        self.exprs[source.0].refresh_optimizer_metadata();
        Some(expr)
    }

    fn maybe_mark_loop_invariant(&mut self, expr: ValueRef) {
        if self.expr_is_loop_invariant(expr) {
            self.loop_invariants
                .pure_origins
                .insert(self.exprs[expr.0].origin);
        }
    }

    fn expr_is_loop_invariant(&self, expr: ValueRef) -> bool {
        let state = &self.exprs[expr.0];
        if state.is_block_argument() {
            return false;
        }
        if state.const_value.is_some() {
            return true;
        }
        match state.origin.kind {
            ExprOriginKind::EntryLocal | ExprOriginKind::SyntheticConst => return true,
            ExprOriginKind::EntryStack | ExprOriginKind::BlockArgument => return false,
            _ => {}
        }
        match state.key {
            Some(ValueKey::Unary { input, .. }) => self
                .latest_by_origin
                .get(&input)
                .copied()
                .is_some_and(|input| self.expr_is_loop_invariant(input)),
            Some(ValueKey::Binary { lhs, rhs, .. }) => {
                self.latest_by_origin
                    .get(&lhs)
                    .copied()
                    .is_some_and(|lhs| self.expr_is_loop_invariant(lhs))
                    && self
                        .latest_by_origin
                        .get(&rhs)
                        .copied()
                        .is_some_and(|rhs| self.expr_is_loop_invariant(rhs))
            }
            None => false,
        }
    }

    fn touch_value(&mut self, value: ValueRef) {
        self.touched_values.push(value);
    }
}

fn clear_alias_space_rewrite(
    aliases: &mut HashMap<AliasKey, ValueRef>,
    stores: &mut HashMap<AliasKey, StoreWrite>,
    space: AliasSpace,
) {
    aliases.retain(|key, _| key.space != space);
    stores.retain(|key, _| key.space != space);
}

fn same_expr(lhs: &ExprState, rhs: &ExprState) -> bool {
    lhs.ty == rhs.ty
        && lhs.origin == rhs.origin
        && lhs.def == rhs.def
        && lhs.block_argument() == rhs.block_argument()
        && lhs.const_value == rhs.const_value
        && lhs.key == rhs.key
        && lhs.address_shape == rhs.address_shape
        && lhs.loop_value_shape == rhs.loop_value_shape
        && lhs.slot_shape == rhs.slot_shape
}

#[derive(Clone)]
struct NaturalLoop {
    header: usize,
    preheader: LoopPreheader,
    blocks: BTreeSet<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum LoopPreheader {
    Entry,
    Block(usize),
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
    root_value: ValueRef,
    result_size: u32,
    source_start: Option<usize>,
}

#[derive(Clone, Copy)]
struct LicmInvariantLeaf {
    value: ValueRef,
}

fn apply_licm(
    program: &BasicBlockProgram,
    rewrite: &mut FunctionRewrite,
    locals: &mut LocalsData,
    param_bytes: u32,
) -> Vec<bool> {
    let loops = collect_natural_loops(program);
    let mut modified = vec![false; program.blocks.len()];
    for loop_info in loops {
        let effects = summarize_loop_effects(program, &loop_info.blocks);
        let mut candidate_blocks = vec![loop_info.header];
        if rewrite.relower.block_bodies[loop_info.header]
            .ops
            .is_empty()
            && rewrite.relower.block_bodies[loop_info.header]
                .terminator
                .as_ref()
                .is_some_and(|terminator| terminator.kind == BlockTerminatorKind::Loop)
        {
            if let Some(first_loop_block) = program.successors[loop_info.header].first().copied() {
                candidate_blocks.push(first_loop_block);
            }
        }

        for candidate_block in candidate_blocks {
            let header_body = rewrite.relower.block_bodies[candidate_block].clone();
            let candidates = collect_licm_candidates(&rewrite.graph, &header_body, &effects);
            if candidates.is_empty() {
                continue;
            }

            let mut preheader_insert = Vec::new();
            let mut new_header = Vec::with_capacity(header_body.ops.len());
            let mut cursor = 0usize;
            while cursor < header_body.ops.len() {
                if let Some(candidate) = candidates
                    .iter()
                    .find(|candidate| candidate.start == cursor)
                {
                    let temp = LocalSlot::new(
                        allocate_optimizer_temp_slot(
                            locals,
                            param_bytes,
                            type_from_slot(candidate.result_size),
                        ),
                        candidate.result_size,
                    );
                    preheader_insert.extend(emit_licm_candidate(candidate, temp, &header_body.ops));
                    let replacement_source_start = if candidate.start == 0 {
                        Some(program.records[program.blocks[candidate_block].start].old_start)
                    } else {
                        candidate.source_start
                    };
                    new_header.push(BlockOp {
                        source_start: replacement_source_start,
                        op: local_get_op(candidate.result_size),
                        kind: BlockOpKind::LocalGet,
                        operands: vec![BlockOperand::LocalAddr(temp.addr)],
                        inputs: Vec::new(),
                        values: vec![candidate.root_value],
                    });
                    cursor = candidate.end;
                    modified[candidate_block] = true;
                    if let LoopPreheader::Block(preheader) = loop_info.preheader {
                        modified[preheader] = true;
                    }
                    continue;
                }
                new_header.push(header_body.ops[cursor].clone());
                cursor += 1;
            }

            if preheader_insert.is_empty() {
                continue;
            }
            match loop_info.preheader {
                LoopPreheader::Block(preheader) => {
                    insert_before_terminator(
                        &mut rewrite.relower.block_bodies[preheader],
                        preheader_insert,
                    );
                }
                LoopPreheader::Entry => {
                    rewrite.relower.entry_prefix_ops.extend(preheader_insert);
                }
            }
            rewrite.relower.block_bodies[candidate_block].ops = new_header;
            break;
        }
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
            let preheader = if outside_preds.len() == 1 {
                let preheader = outside_preds[0];
                if program.successors[preheader].as_slice() != [*succ] {
                    continue;
                }
                LoopPreheader::Block(preheader)
            } else if outside_preds.is_empty() && *succ == 0 {
                LoopPreheader::Entry
            } else {
                continue;
            };
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
    graph: &ValueGraph,
    body: &BlockBody,
    effects: &LoopEffects,
) -> Vec<LicmCandidate> {
    let producer_indices = licm_producer_indices(body);
    let origin_values = licm_origin_values(graph);
    let mut by_start = BTreeMap::new();
    for cursor in 0..body.ops.len() {
        if let Some(candidate) = match_licm_candidate(
            graph,
            body,
            &producer_indices,
            &origin_values,
            cursor,
            effects,
        ) {
            by_start
                .entry(candidate.start)
                .and_modify(|existing: &mut LicmCandidate| {
                    let existing_len = existing.end.saturating_sub(existing.start);
                    let candidate_len = candidate.end.saturating_sub(candidate.start);
                    if candidate_len > existing_len {
                        *existing = candidate;
                    }
                })
                .or_insert(candidate);
        }
    }
    by_start.into_values().collect()
}

fn match_licm_candidate(
    graph: &ValueGraph,
    body: &BlockBody,
    producer_indices: &HashMap<ValueRef, usize>,
    origin_values: &HashMap<ExprOrigin, ValueRef>,
    cursor: usize,
    effects: &LoopEffects,
) -> Option<LicmCandidate> {
    if let Some(candidate) = match_licm_preparation_candidate(graph, body, cursor, effects) {
        return Some(candidate);
    }
    let root = body.ops.get(cursor)?;
    if root.kind == BlockOpKind::GlobalGet {
        let slot = block_op_global_get_slot(root)?;
        if effects.global_writes.contains(&slot)
            || effects.has_call_barrier
            || !block_op_eligible_for_licm(graph, root)
        {
            return None;
        }
        let root_value = block_op_value_used_after(body, cursor + 1, root)
            .or_else(|| root.values.first().copied())?;
        return Some(LicmCandidate {
            start: cursor,
            end: cursor + 1,
            root_value,
            result_size: slot.size,
            source_start: root.source_start,
        });
    }
    let root_value = block_op_single_result(root)?;
    if graph[root_value.0].is_effect_result() || !block_op_eligible_for_licm(graph, root) {
        return None;
    }
    let mut op_indices = BTreeSet::new();
    collect_licm_value_ops(
        graph,
        body,
        root_value,
        producer_indices,
        origin_values,
        effects,
        &mut op_indices,
    )?;
    let start = *op_indices.first()?;
    let end = op_indices.last()?.saturating_add(1);
    if start != cursor {
        return None;
    }
    if !(start..end).all(|index| op_indices.contains(&index)) {
        return None;
    }
    Some(LicmCandidate {
        start,
        end,
        root_value,
        result_size: value_type_size(graph[root_value.0].ty)?,
        source_start: root.source_start,
    })
}

fn match_licm_preparation_candidate(
    graph: &ValueGraph,
    body: &BlockBody,
    cursor: usize,
    effects: &LoopEffects,
) -> Option<LicmCandidate> {
    let first = body.ops.get(cursor)?;
    if let Some(unary) = body.ops.get(cursor + 1) {
        if matches!(unary.kind, BlockOpKind::PureUnary(PureOpKind::I32Eqz)) {
            let lhs = licm_invariant_leaf(
                graph,
                body,
                cursor + 2,
                first,
                unary.inputs.as_slice(),
                effects,
            )?;
            if unary.inputs.as_slice() == [lhs.value]
                && block_op_value_use_count_after(body, cursor + 2, unary) <= 1
            {
                let root_value = block_op_value_used_after(body, cursor + 2, unary)?;
                return Some(LicmCandidate {
                    start: cursor,
                    end: cursor + 2,
                    root_value,
                    result_size: value_type_size(graph[root_value.0].ty)?,
                    source_start: first.source_start,
                });
            }
        }
    }

    let second = body.ops.get(cursor + 1)?;
    let third = body.ops.get(cursor + 2)?;
    let lhs = licm_invariant_leaf(
        graph,
        body,
        cursor + 3,
        first,
        third.inputs.as_slice(),
        effects,
    )?;
    if matches!(
        third.kind,
        BlockOpKind::PureBinary(PureOpKind::I32Add) | BlockOpKind::PureBinary(PureOpKind::I32Sub)
    ) {
        if let Some(fourth) = body.ops.get(cursor + 3) {
            if matches!(
                fourth.kind,
                BlockOpKind::MemoryLoad | BlockOpKind::MemoryStore
            ) {
                let address_input = memory_address_input(fourth)?;
                let root_inputs = [address_input];
                let root_value = block_op_value_used_by_inputs(third, &root_inputs)?;
                let const_value = block_op_i32_const(second)
                    .and_then(|_| block_op_value_used_by_inputs(second, third.inputs.as_slice()))?;
                let inputs = third.inputs.as_slice();
                let matches_address_prep = inputs == [lhs.value, const_value]
                    || (matches!(third.kind, BlockOpKind::PureBinary(PureOpKind::I32Add))
                        && inputs == [const_value, lhs.value]);
                if matches_address_prep && value_use_count_after(body, cursor + 4, root_value) == 0
                {
                    return Some(LicmCandidate {
                        start: cursor,
                        end: cursor + 3,
                        root_value,
                        result_size: value_type_size(graph[root_value.0].ty)?,
                        source_start: first.source_start,
                    });
                }
            }
        }
    }
    if !matches!(third.kind, BlockOpKind::PureBinary(_)) {
        return None;
    }
    let rhs_const = block_op_i32_const(second).map(|_| second);
    let rhs_leaf = rhs_const
        .is_none()
        .then(|| {
            licm_invariant_leaf(
                graph,
                body,
                cursor + 3,
                second,
                third.inputs.as_slice(),
                effects,
            )
        })
        .flatten();
    let inputs = third.inputs.as_slice();
    let root_value = block_op_value_used_after(body, cursor + 3, third)?;
    let result_size = value_type_size(graph[root_value.0].ty)?;
    if block_op_value_use_count_after(body, cursor + 3, third) > 1 {
        return None;
    }

    let matches_const_rhs = rhs_const.is_some_and(|const_op| {
        inputs
            == [
                lhs.value,
                block_op_value_used_by_inputs(const_op, inputs).unwrap(),
            ]
            || (matches!(third.kind, BlockOpKind::PureBinary(PureOpKind::I32Add))
                && inputs
                    == [
                        block_op_value_used_by_inputs(const_op, inputs).unwrap(),
                        lhs.value,
                    ])
            || (matches!(third.kind, BlockOpKind::PureBinary(op) if i32_compare_op(op))
                && inputs
                    == [
                        block_op_value_used_by_inputs(const_op, inputs).unwrap(),
                        lhs.value,
                    ])
    });
    if matches_const_rhs && licm_supported_const_rhs_binary(third.kind) {
        return Some(LicmCandidate {
            start: cursor,
            end: cursor + 3,
            root_value,
            result_size,
            source_start: first.source_start,
        });
    }

    let rhs = rhs_leaf?;
    if licm_supported_dual_leaf_binary(third.kind)
        && (inputs == [lhs.value, rhs.value] || inputs == [rhs.value, lhs.value])
    {
        return Some(LicmCandidate {
            start: cursor,
            end: cursor + 3,
            root_value,
            result_size,
            source_start: first.source_start,
        });
    }

    None
}

fn licm_supported_const_rhs_binary(kind: BlockOpKind) -> bool {
    matches!(
        kind,
        BlockOpKind::PureBinary(PureOpKind::I32Add) | BlockOpKind::PureBinary(PureOpKind::I32Sub)
    ) || matches!(kind, BlockOpKind::PureBinary(op) if i32_compare_op(op))
}

fn licm_supported_dual_leaf_binary(kind: BlockOpKind) -> bool {
    matches!(kind, BlockOpKind::PureBinary(PureOpKind::I32Add))
        || matches!(kind, BlockOpKind::PureBinary(op) if i32_compare_op(op))
}

fn licm_invariant_leaf(
    _graph: &ValueGraph,
    body: &BlockBody,
    candidate_end: usize,
    op: &BlockOp,
    consumer_inputs: &[ValueRef],
    effects: &LoopEffects,
) -> Option<LicmInvariantLeaf> {
    let value = block_op_value_used_by_inputs(op, consumer_inputs)?;
    if value_used_after(body, candidate_end, value)
        || body
            .terminator
            .as_ref()
            .is_some_and(|terminator| terminator.inputs.contains(&value))
    {
        return None;
    }
    if matches!(op.kind, BlockOpKind::Const) {
        return Some(LicmInvariantLeaf { value });
    }
    if op.kind == BlockOpKind::LocalGet
        && matches!(op.operands.first(), Some(BlockOperand::SpillValue(_)))
    {
        return Some(LicmInvariantLeaf { value });
    }
    if let Some(slot) = block_op_local_get_slot(op) {
        if effects.local_writes.contains(&slot) {
            return None;
        }
        return Some(LicmInvariantLeaf { value });
    }
    if let Some(slot) = block_op_global_get_slot(op) {
        if effects.global_writes.contains(&slot) || effects.has_call_barrier {
            return None;
        }
        return Some(LicmInvariantLeaf { value });
    }
    None
}

fn block_op_value_used_by_inputs(op: &BlockOp, inputs: &[ValueRef]) -> Option<ValueRef> {
    inputs
        .iter()
        .copied()
        .find(|input| op.values.contains(input))
}

fn block_op_value_used_after(body: &BlockBody, start_idx: usize, op: &BlockOp) -> Option<ValueRef> {
    body.ops
        .iter()
        .skip(start_idx)
        .flat_map(|candidate| candidate.inputs.iter().copied())
        .find(|input| op.values.contains(input))
        .or_else(|| {
            body.terminator
                .as_ref()
                .and_then(|terminator| block_op_value_used_by_inputs(op, &terminator.inputs))
        })
}

fn block_op_value_use_count_after(body: &BlockBody, start_idx: usize, op: &BlockOp) -> usize {
    body.ops
        .iter()
        .skip(start_idx)
        .map(|candidate| {
            candidate
                .inputs
                .iter()
                .filter(|input| op.values.contains(input))
                .count()
        })
        .sum::<usize>()
        + usize::from(body.terminator.as_ref().is_some_and(|terminator| {
            terminator
                .inputs
                .iter()
                .any(|input| op.values.contains(input))
        }))
}

fn emit_licm_candidate(
    candidate: &LicmCandidate,
    temp: LocalSlot,
    header_ops: &[BlockOp],
) -> Vec<BlockOp> {
    let mut out = header_ops[candidate.start..candidate.end]
        .iter()
        .cloned()
        .map(|mut op| {
            op.source_start = None;
            op
        })
        .collect::<Vec<_>>();
    out.push(BlockOp {
        source_start: None,
        op: local_set_op(temp.size),
        kind: BlockOpKind::LocalSet,
        operands: vec![BlockOperand::LocalAddr(temp.addr)],
        inputs: vec![candidate.root_value],
        values: Vec::new(),
    });
    out
}

fn insert_before_terminator(body: &mut BlockBody, mut insert: Vec<BlockOp>) {
    body.ops.append(&mut insert);
}

fn block_op_single_use(graph: &ValueGraph, op: &BlockOp) -> bool {
    block_op_single_result(op).is_some_and(|value| selector_value_is_single_use(graph, op, value))
}

fn block_op_address_base_single_use(
    graph: &ValueGraph,
    _body: &BlockBody,
    _next_idx: usize,
    op: &BlockOp,
) -> bool {
    if op.kind != BlockOpKind::LocalGet {
        return false;
    }
    let Some(value) = block_op_primary_result(op) else {
        return false;
    };
    let node = &graph[value.0];
    !node.is_effect_result()
        || matches!(
            op.operands.first(),
            Some(BlockOperand::SpillValue(_) | BlockOperand::LocalAddr(_))
        )
}

fn block_op_select_size(op: &BlockOp) -> Option<u32> {
    if op.kind != BlockOpKind::Select {
        return None;
    }
    let BlockOperand::Raw(operand) = *op.operands.first()? else {
        return None;
    };
    Some(unsafe { operand.select })
}

fn selector_value_is_single_use(graph: &ValueGraph, _op: &BlockOp, value: ValueRef) -> bool {
    let node = &graph[value.0];
    node.use_count <= 1 && !node.is_effect_result() && !node.is_block_argument()
}

fn block_op_single_use_for_licm(graph: &ValueGraph, op: &BlockOp) -> bool {
    block_op_single_result(op).is_some_and(|value| {
        let node = &graph[value.0];
        node.use_count <= 1 && (!node.is_effect_result() || op.kind == BlockOpKind::GlobalGet)
    })
}

fn block_op_single_result(op: &BlockOp) -> Option<ValueRef> {
    if op.values.len() == 1 {
        Some(op.values[0])
    } else {
        None
    }
}

fn block_op_primary_result(op: &BlockOp) -> Option<ValueRef> {
    op.values.last().copied()
}

fn value_feeds_memory_address(body: &BlockBody, start_idx: usize, value: ValueRef) -> bool {
    body.ops
        .iter()
        .skip(start_idx)
        .any(|op| memory_address_input(op) == Some(value))
}

fn block_op_local_get_slot(op: &BlockOp) -> Option<LocalSlot> {
    if op.kind != BlockOpKind::LocalGet {
        return None;
    }
    let BlockOperand::LocalAddr(addr) = *op.operands.first()? else {
        return None;
    };
    Some(LocalSlot::new(addr, local_get_size_from_op(op.op)?))
}

fn block_op_any_local_get_slot(
    op: &BlockOp,
    spill_plan: &EffectResultSpillPlan,
) -> Option<LocalSlot> {
    block_op_spill_local_get_slot(op, spill_plan).or_else(|| block_op_local_get_slot(op))
}

fn block_op_spill_local_get_slot(
    op: &BlockOp,
    spill_plan: &EffectResultSpillPlan,
) -> Option<LocalSlot> {
    if op.kind != BlockOpKind::LocalGet {
        return None;
    }
    let BlockOperand::SpillValue(source) = *op.operands.first()? else {
        return None;
    };
    spill_plan.slot(source)
}

fn block_op_global_get_slot(op: &BlockOp) -> Option<LocalSlot> {
    if op.kind != BlockOpKind::GlobalGet {
        return None;
    }
    let BlockOperand::U32(addr) = *op.operands.first()? else {
        return None;
    };
    if std::ptr::fn_addr_eq(op.op, vm::op_global_get4 as Op) {
        return Some(LocalSlot::new(addr, 4));
    }
    if std::ptr::fn_addr_eq(op.op, vm::op_global_get8 as Op) {
        return Some(LocalSlot::new(addr, 8));
    }
    if std::ptr::fn_addr_eq(op.op, vm::op_global_get16 as Op) {
        return Some(LocalSlot::new(addr, 16));
    }
    None
}

fn specialized_memory_op(op: Op) -> Option<Op> {
    if std::ptr::fn_addr_eq(op, vm::op_i32_load as Op) {
        return Some(vm::op_i32_load_local_base as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i64_load as Op) {
        return Some(vm::op_i64_load_local_base as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_f32_load as Op) {
        return Some(vm::op_f32_load_local_base as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_f64_load as Op) {
        return Some(vm::op_f64_load_local_base as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i32_load8_s as Op) {
        return Some(vm::op_i32_load8_s_local_base as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i32_load8_u as Op) {
        return Some(vm::op_i32_load8_u_local_base as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i32_load16_s as Op) {
        return Some(vm::op_i32_load16_s_local_base as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i32_load16_u as Op) {
        return Some(vm::op_i32_load16_u_local_base as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i64_load8_s as Op) {
        return Some(vm::op_i64_load8_s_local_base as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i64_load8_u as Op) {
        return Some(vm::op_i64_load8_u_local_base as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i64_load16_s as Op) {
        return Some(vm::op_i64_load16_s_local_base as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i64_load16_u as Op) {
        return Some(vm::op_i64_load16_u_local_base as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i64_load32_s as Op) {
        return Some(vm::op_i64_load32_s_local_base as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i64_load32_u as Op) {
        return Some(vm::op_i64_load32_u_local_base as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i32_store as Op) {
        return Some(vm::op_i32_store_local_base as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i64_store as Op) {
        return Some(vm::op_i64_store_local_base as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_f32_store as Op) {
        return Some(vm::op_f32_store_local_base as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_f64_store as Op) {
        return Some(vm::op_f64_store_local_base as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i32_store8 as Op) {
        return Some(vm::op_i32_store8_local_base as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i32_store16 as Op) {
        return Some(vm::op_i32_store16_local_base as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i64_store8 as Op) {
        return Some(vm::op_i64_store8_local_base as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i64_store16 as Op) {
        return Some(vm::op_i64_store16_local_base as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i64_store32 as Op) {
        return Some(vm::op_i64_store32_local_base as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i32_load_local as Op) {
        return Some(vm::op_i32_load_local_base as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i64_load_local as Op) {
        return Some(vm::op_i64_load_local_base as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_f32_load_local as Op) {
        return Some(vm::op_f32_load_local_base as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_f64_load_local as Op) {
        return Some(vm::op_f64_load_local_base as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i32_load8_s_local as Op) {
        return Some(vm::op_i32_load8_s_local_base as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i32_load8_u_local as Op) {
        return Some(vm::op_i32_load8_u_local_base as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i32_load16_s_local as Op) {
        return Some(vm::op_i32_load16_s_local_base as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i32_load16_u_local as Op) {
        return Some(vm::op_i32_load16_u_local_base as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i64_load8_s_local as Op) {
        return Some(vm::op_i64_load8_s_local_base as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i64_load8_u_local as Op) {
        return Some(vm::op_i64_load8_u_local_base as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i64_load16_s_local as Op) {
        return Some(vm::op_i64_load16_s_local_base as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i64_load16_u_local as Op) {
        return Some(vm::op_i64_load16_u_local_base as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i64_load32_s_local as Op) {
        return Some(vm::op_i64_load32_s_local_base as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i64_load32_u_local as Op) {
        return Some(vm::op_i64_load32_u_local_base as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i32_store_local as Op) {
        return Some(vm::op_i32_store_local_base as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i64_store_local as Op) {
        return Some(vm::op_i64_store_local_base as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_f32_store_local as Op) {
        return Some(vm::op_f32_store_local_base as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_f64_store_local as Op) {
        return Some(vm::op_f64_store_local_base as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i32_store8_local as Op) {
        return Some(vm::op_i32_store8_local_base as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i32_store16_local as Op) {
        return Some(vm::op_i32_store16_local_base as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i64_store8_local as Op) {
        return Some(vm::op_i64_store8_local_base as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i64_store16_local as Op) {
        return Some(vm::op_i64_store16_local_base as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i64_store32_local as Op) {
        return Some(vm::op_i64_store32_local_base as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i32_load_indexed_local as Op) {
        return Some(vm::op_i32_load_indexed_local_base as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i64_load_indexed_local as Op) {
        return Some(vm::op_i64_load_indexed_local_base as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_f32_load_indexed_local as Op) {
        return Some(vm::op_f32_load_indexed_local_base as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_f64_load_indexed_local as Op) {
        return Some(vm::op_f64_load_indexed_local_base as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i32_load8_s_indexed_local as Op) {
        return Some(vm::op_i32_load8_s_indexed_local_base as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i32_load8_u_indexed_local as Op) {
        return Some(vm::op_i32_load8_u_indexed_local_base as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i32_load16_s_indexed_local as Op) {
        return Some(vm::op_i32_load16_s_indexed_local_base as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i32_load16_u_indexed_local as Op) {
        return Some(vm::op_i32_load16_u_indexed_local_base as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i64_load8_s_indexed_local as Op) {
        return Some(vm::op_i64_load8_s_indexed_local_base as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i64_load8_u_indexed_local as Op) {
        return Some(vm::op_i64_load8_u_indexed_local_base as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i64_load16_s_indexed_local as Op) {
        return Some(vm::op_i64_load16_s_indexed_local_base as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i64_load16_u_indexed_local as Op) {
        return Some(vm::op_i64_load16_u_indexed_local_base as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i64_load32_s_indexed_local as Op) {
        return Some(vm::op_i64_load32_s_indexed_local_base as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i64_load32_u_indexed_local as Op) {
        return Some(vm::op_i64_load32_u_indexed_local_base as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i32_store_indexed_local as Op) {
        return Some(vm::op_i32_store_indexed_local_base as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i64_store_indexed_local as Op) {
        return Some(vm::op_i64_store_indexed_local_base as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_f32_store_indexed_local as Op) {
        return Some(vm::op_f32_store_indexed_local_base as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_f64_store_indexed_local as Op) {
        return Some(vm::op_f64_store_indexed_local_base as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i32_store8_indexed_local as Op) {
        return Some(vm::op_i32_store8_indexed_local_base as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i32_store16_indexed_local as Op) {
        return Some(vm::op_i32_store16_indexed_local_base as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i64_store8_indexed_local as Op) {
        return Some(vm::op_i64_store8_indexed_local_base as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i64_store16_indexed_local as Op) {
        return Some(vm::op_i64_store16_indexed_local_base as Op);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i64_store32_indexed_local as Op) {
        return Some(vm::op_i64_store32_indexed_local_base as Op);
    }
    None
}

fn is_local_base_memory_family(op: Op) -> bool {
    const SOURCES: &[Op] = &[
        vm::op_i32_load as Op,
        vm::op_i64_load as Op,
        vm::op_f32_load as Op,
        vm::op_f64_load as Op,
        vm::op_i32_load8_s as Op,
        vm::op_i32_load8_u as Op,
        vm::op_i32_load16_s as Op,
        vm::op_i32_load16_u as Op,
        vm::op_i64_load8_s as Op,
        vm::op_i64_load8_u as Op,
        vm::op_i64_load16_s as Op,
        vm::op_i64_load16_u as Op,
        vm::op_i64_load32_s as Op,
        vm::op_i64_load32_u as Op,
        vm::op_i32_store as Op,
        vm::op_i64_store as Op,
        vm::op_f32_store as Op,
        vm::op_f64_store as Op,
        vm::op_i32_store8 as Op,
        vm::op_i32_store16 as Op,
        vm::op_i64_store8 as Op,
        vm::op_i64_store16 as Op,
        vm::op_i64_store32 as Op,
        vm::op_i32_load_local as Op,
        vm::op_i64_load_local as Op,
        vm::op_f32_load_local as Op,
        vm::op_f64_load_local as Op,
        vm::op_i32_load8_s_local as Op,
        vm::op_i32_load8_u_local as Op,
        vm::op_i32_load16_s_local as Op,
        vm::op_i32_load16_u_local as Op,
        vm::op_i64_load8_s_local as Op,
        vm::op_i64_load8_u_local as Op,
        vm::op_i64_load16_s_local as Op,
        vm::op_i64_load16_u_local as Op,
        vm::op_i64_load32_s_local as Op,
        vm::op_i64_load32_u_local as Op,
        vm::op_i32_store_local as Op,
        vm::op_i64_store_local as Op,
        vm::op_f32_store_local as Op,
        vm::op_f64_store_local as Op,
        vm::op_i32_store8_local as Op,
        vm::op_i32_store16_local as Op,
        vm::op_i64_store8_local as Op,
        vm::op_i64_store16_local as Op,
        vm::op_i64_store32_local as Op,
    ];
    SOURCES
        .iter()
        .filter_map(|source| specialized_memory_op(*source))
        .any(|candidate| std::ptr::fn_addr_eq(candidate, op))
}

fn is_indexed_local_base_memory_family(op: Op) -> bool {
    const SOURCES: &[Op] = &[
        vm::op_i32_load_indexed_local as Op,
        vm::op_i64_load_indexed_local as Op,
        vm::op_f32_load_indexed_local as Op,
        vm::op_f64_load_indexed_local as Op,
        vm::op_i32_load8_s_indexed_local as Op,
        vm::op_i32_load8_u_indexed_local as Op,
        vm::op_i32_load16_s_indexed_local as Op,
        vm::op_i32_load16_u_indexed_local as Op,
        vm::op_i64_load8_s_indexed_local as Op,
        vm::op_i64_load8_u_indexed_local as Op,
        vm::op_i64_load16_s_indexed_local as Op,
        vm::op_i64_load16_u_indexed_local as Op,
        vm::op_i64_load32_s_indexed_local as Op,
        vm::op_i64_load32_u_indexed_local as Op,
        vm::op_i32_store_indexed_local as Op,
        vm::op_i64_store_indexed_local as Op,
        vm::op_f32_store_indexed_local as Op,
        vm::op_f64_store_indexed_local as Op,
        vm::op_i32_store8_indexed_local as Op,
        vm::op_i32_store16_indexed_local as Op,
        vm::op_i64_store8_indexed_local as Op,
        vm::op_i64_store16_indexed_local as Op,
        vm::op_i64_store32_indexed_local as Op,
    ];
    SOURCES
        .iter()
        .filter_map(|source| specialized_memory_op(*source))
        .any(|candidate| std::ptr::fn_addr_eq(candidate, op))
}

fn block_op_i32_const(op: &BlockOp) -> Option<i32> {
    matches!(op.kind, BlockOpKind::Const).then(|| match op.operands.first()? {
        BlockOperand::I32(value) => Some(*value),
        _ => None,
    })?
}

fn licm_origin_values(graph: &ValueGraph) -> HashMap<ExprOrigin, ValueRef> {
    let mut origin_values = HashMap::new();
    for (expr_idx, node) in graph.nodes.iter().enumerate() {
        origin_values.insert(node.origin, ExprId(expr_idx));
    }
    origin_values
}

fn licm_producer_indices(body: &BlockBody) -> HashMap<ValueRef, usize> {
    let mut out = HashMap::new();
    for (index, op) in body.ops.iter().enumerate() {
        if let Some(value) = block_op_single_result(op) {
            out.insert(value, index);
        }
    }
    out
}

fn block_op_eligible_for_licm(graph: &ValueGraph, op: &BlockOp) -> bool {
    block_op_single_use_for_licm(graph, op)
}

#[allow(clippy::too_many_arguments)]
fn collect_licm_value_ops(
    graph: &ValueGraph,
    body: &BlockBody,
    value: ValueRef,
    producer_indices: &HashMap<ValueRef, usize>,
    origin_values: &HashMap<ExprOrigin, ValueRef>,
    effects: &LoopEffects,
    op_indices: &mut BTreeSet<usize>,
) -> Option<()> {
    let node = &graph[value.0];
    if node.const_value.is_some() || matches!(node.origin.kind, ExprOriginKind::EntryLocal) {
        let &index = producer_indices.get(&value)?;
        let op = body.ops.get(index)?;
        if let Some(slot) = block_op_local_get_slot(op) {
            if effects.local_writes.contains(&slot) || !block_op_eligible_for_licm(graph, op) {
                return None;
            }
        } else if !matches!(op.kind, BlockOpKind::Const) || !block_op_eligible_for_licm(graph, op) {
            return None;
        }
        op_indices.insert(index);
        return Some(());
    }
    if node.is_block_argument() {
        return None;
    }
    if node.is_effect_result() {
        let &index = producer_indices.get(&value)?;
        let op = body.ops.get(index)?;
        let slot = block_op_global_get_slot(op)?;
        if effects.global_writes.contains(&slot)
            || effects.has_call_barrier
            || !block_op_eligible_for_licm(graph, op)
        {
            return None;
        }
        op_indices.insert(index);
        return Some(());
    }
    let &index = producer_indices.get(&value)?;
    let op = body.ops.get(index)?;
    if !matches!(
        op.kind,
        BlockOpKind::PureUnary(_) | BlockOpKind::PureBinary(_)
    ) || !block_op_eligible_for_licm(graph, op)
    {
        return None;
    }
    match node.key? {
        ValueKey::Unary { input, .. } => {
            let input = origin_values.get(&input).copied()?;
            collect_licm_value_ops(
                graph,
                body,
                input,
                producer_indices,
                origin_values,
                effects,
                op_indices,
            )?;
        }
        ValueKey::Binary { lhs, rhs, .. } => {
            let lhs = origin_values.get(&lhs).copied()?;
            let rhs = origin_values.get(&rhs).copied()?;
            collect_licm_value_ops(
                graph,
                body,
                lhs,
                producer_indices,
                origin_values,
                effects,
                op_indices,
            )?;
            collect_licm_value_ops(
                graph,
                body,
                rhs,
                producer_indices,
                origin_values,
                effects,
                op_indices,
            )?;
        }
    }
    op_indices.insert(index);
    Some(())
}

#[allow(dead_code)]
#[derive(Clone, Copy)]
struct MemoryAccess {
    memidx: u32,
    offset: u32,
    width: u8,
    ty: ValType,
}

#[cold]
#[inline(never)]
fn build_specialized_local_root_lowering(
    graph: &ValueGraph,
    body: &BlockBody,
    op_idx: usize,
    op: &BlockOp,
) -> Option<SpecializedLocalControlLowering> {
    if next_entry_is_barrier(body, op_idx + 1) {
        return None;
    }
    let start_idx = op_idx.checked_sub(2)?;
    if body.ops[start_idx..start_idx + 2]
        .iter()
        .any(|provider| local_get_reads_block_argument(graph, provider))
    {
        return None;
    }
    let (matched, _) = match_selector_root_shape(graph, body, start_idx, op_idx + 1)?;
    let (specialized_op, operands) = match matched {
        SelectorRootMatch::LocalConstAdd { base, imm } => (
            vm::op_local_get4_i32_const_add as Op,
            vec![base, BlockOperand::I32(imm)],
        ),
        SelectorRootMatch::LocalLocalAdd { lhs, rhs } => {
            (vm::op_local_get4_local_get4_i32_add as Op, vec![lhs, rhs])
        }
    };
    Some(SpecializedLocalControlLowering {
        source_start: body
            .ops
            .get(start_idx)
            .and_then(|provider| provider.source_start)
            .or(op.source_start),
        op: specialized_op,
        operands,
        absorbed_ops: absorbed_ops_range(start_idx, op_idx),
        consumer_after_idx: op_idx + 1,
    })
}

#[cold]
#[inline(never)]
fn build_specialized_local_set_tee_lowering(
    graph: &ValueGraph,
    body: &BlockBody,
    op_idx: usize,
    op: &BlockOp,
) -> Option<SpecializedLocalControlLowering> {
    if !matches!(op.kind, BlockOpKind::LocalSet | BlockOpKind::LocalTee) {
        return None;
    }
    if body
        .ops
        .get(op_idx + 1)
        .is_some_and(|_| next_entry_is_barrier(body, op_idx + 1))
    {
        return None;
    }
    let start_idx = op_idx.checked_sub(3)?;
    if body.ops[start_idx..start_idx + 2]
        .iter()
        .any(|provider| local_get_reads_block_argument(graph, provider))
    {
        return None;
    }
    let input = *op.inputs.first()?;
    let root_op = &body.ops[start_idx + 2];
    let matched = match_selector_root_shape(graph, body, start_idx, op_idx + 1)
        .and_then(|(matched, root_value)| {
            (root_value == input && !value_used_after(body, op_idx + 1, root_value))
                .then_some(matched)
        })
        .or_else(|| {
            let root_value = block_op_single_result(root_op)?;
            (root_value == input
                && selector_value_is_single_use(graph, root_op, root_value)
                && !value_used_after(body, op_idx + 1, root_value)
                && !value_feeds_memory_address(body, op_idx + 1, root_value))
            .then(|| {
                match_selector_root_shape_from_body(
                    graph,
                    root_op,
                    &body.ops[start_idx],
                    &body.ops[start_idx + 1],
                )
            })
            .flatten()
        })?;
    let dst = *op.operands.first()?;
    let (specialized_op, operands) = match (op.kind, matched) {
        (BlockOpKind::LocalSet, SelectorRootMatch::LocalConstAdd { base, imm }) => (
            vm::op_local_get4_i32_const_add_set4 as Op,
            vec![base, BlockOperand::I32(imm), dst],
        ),
        (BlockOpKind::LocalSet, SelectorRootMatch::LocalLocalAdd { lhs, rhs }) => (
            vm::op_local_get4_local_get4_i32_add_set4 as Op,
            vec![lhs, rhs, dst],
        ),
        (BlockOpKind::LocalTee, SelectorRootMatch::LocalConstAdd { base, imm }) => (
            vm::op_local_get4_i32_const_add_tee4 as Op,
            vec![base, BlockOperand::I32(imm), dst],
        ),
        (BlockOpKind::LocalTee, SelectorRootMatch::LocalLocalAdd { lhs, rhs }) => (
            vm::op_local_get4_local_get4_i32_add_tee4 as Op,
            vec![lhs, rhs, dst],
        ),
        _ => return None,
    };
    Some(SpecializedLocalControlLowering {
        source_start: body
            .ops
            .get(start_idx)
            .and_then(|provider| provider.source_start)
            .or(op.source_start),
        op: specialized_op,
        operands,
        absorbed_ops: absorbed_ops_range(start_idx, op_idx),
        consumer_after_idx: op_idx + 1,
    })
}

#[cold]
#[inline(never)]
fn build_specialized_br_if_lowering(
    graph: &ValueGraph,
    body: &BlockBody,
    terminator: &BlockTerminator,
) -> Option<SpecializedLocalControlLowering> {
    if terminator.kind != BlockTerminatorKind::BrIf {
        return None;
    }
    let (fused, consumed) = match_br_if_pattern(graph, body, terminator)?;
    let start_idx = body.ops.len().checked_sub(consumed)?;
    Some(SpecializedLocalControlLowering {
        source_start: body
            .ops
            .get(start_idx)
            .and_then(|provider| provider.source_start)
            .or(terminator.source_start),
        op: fused.op,
        operands: fused.operands,
        absorbed_ops: absorbed_ops_range(start_idx, body.ops.len()),
        consumer_after_idx: body.ops.len(),
    })
}

#[cold]
#[inline(never)]
fn absorbed_ops_range(start_idx: usize, end_exclusive: usize) -> BTreeSet<usize> {
    (start_idx..end_exclusive).collect()
}

#[derive(Clone, Copy)]
enum SelectorRootMatch {
    LocalConstAdd {
        base: BlockOperand,
        imm: i32,
    },
    LocalLocalAdd {
        lhs: BlockOperand,
        rhs: BlockOperand,
    },
}

#[derive(Clone, Copy)]
enum SelectorCompareMatch {
    Eqz {
        input: BlockOperand,
    },
    Const {
        lhs: BlockOperand,
        cmp_kind: u32,
        imm: i32,
    },
    Local {
        lhs: BlockOperand,
        rhs: BlockOperand,
        cmp_kind: u32,
    },
}

fn match_selector_root_shape(
    graph: &ValueGraph,
    body: &BlockBody,
    cursor: usize,
    next_idx: usize,
) -> Option<(SelectorRootMatch, ValueRef)> {
    let ops = &body.ops;
    let root_op = ops.get(cursor + 2)?;
    if !block_op_single_use(graph, root_op) {
        return None;
    }
    let root_value = block_op_single_result(root_op)?;
    if value_feeds_memory_address(body, next_idx, root_value) {
        return None;
    }
    match graph[root_value.0].loop_value_shape.as_ref() {
        Some(LoopValueShape::Local4ConstAdd { base, imm }) => {
            let base_operand = selector_local_input_operand(graph, ops.get(cursor)?, *base)?;
            let const_op = ops.get(cursor + 1)?;
            let expected_imm = selector_const_delta(root_op, const_op)?;
            if *imm != expected_imm {
                return None;
            }
            Some((
                SelectorRootMatch::LocalConstAdd {
                    base: base_operand,
                    imm: *imm,
                },
                root_value,
            ))
        }
        Some(LoopValueShape::Local4Local4Add { lhs, rhs }) => {
            if root_op.kind != BlockOpKind::PureBinary(PureOpKind::I32Add) {
                return None;
            }
            let lhs_operand = selector_local_input_operand(graph, ops.get(cursor)?, *lhs)?;
            let rhs_operand = selector_local_input_operand(graph, ops.get(cursor + 1)?, *rhs)?;
            Some((
                SelectorRootMatch::LocalLocalAdd {
                    lhs: lhs_operand,
                    rhs: rhs_operand,
                },
                root_value,
            ))
        }
        _ => match_selector_root_shape_from_body(
            graph,
            root_op,
            ops.get(cursor)?,
            ops.get(cursor + 1)?,
        )
        .map(|matched| (matched, root_value)),
    }
}

fn match_selector_root_shape_from_body(
    graph: &ValueGraph,
    root_op: &BlockOp,
    first: &BlockOp,
    second: &BlockOp,
) -> Option<SelectorRootMatch> {
    if let Some(base) = selector_non_block_argument_local_get4_operand(graph, first) {
        if let Some(imm) = selector_const_delta(root_op, second) {
            return Some(SelectorRootMatch::LocalConstAdd { base, imm });
        }
    }
    if root_op.kind == BlockOpKind::PureBinary(PureOpKind::I32Add) {
        if let Some(base) = selector_non_block_argument_local_get4_operand(graph, second) {
            if let Some(imm) = block_op_i32_const(first) {
                return Some(SelectorRootMatch::LocalConstAdd { base, imm });
            }
        }
        if let (Some(lhs), Some(rhs)) = (
            selector_non_block_argument_local_get4_operand(graph, first),
            selector_non_block_argument_local_get4_operand(graph, second),
        ) {
            return Some(SelectorRootMatch::LocalLocalAdd { lhs, rhs });
        }
    }
    None
}

fn selector_local_input_operand(
    graph: &ValueGraph,
    op: &BlockOp,
    expected_slot: LocalSlot,
) -> Option<BlockOperand> {
    if op.kind != BlockOpKind::LocalGet {
        return None;
    }
    let value = block_op_single_result(op)?;
    let node = &graph[value.0];
    if node.use_count > 1 {
        return None;
    }
    let matches_expected_slot = node
        .loop_value_shape
        .as_ref()
        .and_then(slot_ref_from_loop_value_shape)
        .or_else(|| effective_slot_shape(graph, value).and_then(|shape| shape.slot))
        .and_then(materializable_slot)
        .is_some_and(|slot| slot == expected_slot);
    if !matches_expected_slot {
        return None;
    }
    let size = local_get_size_from_op(op.op)?;
    match *op.operands.first()? {
        BlockOperand::LocalAddr(addr) if expected_slot.addr == addr && expected_slot.size == 4 => {
            if node.is_effect_result() {
                return None;
            }
            Some(BlockOperand::LocalAddr(addr))
        }
        BlockOperand::SpillValue(source)
            if size == 4 && expected_slot == symbolic_spill_slot(source, size) =>
        {
            Some(BlockOperand::SpillValue(source))
        }
        _ => None,
    }
}

fn selector_any_local_get4_operand(op: &BlockOp) -> Option<BlockOperand> {
    if op.kind != BlockOpKind::LocalGet {
        return None;
    }
    if local_get_size_from_op(op.op)? != 4 {
        return None;
    }
    match *op.operands.first()? {
        BlockOperand::LocalAddr(addr) => Some(BlockOperand::LocalAddr(addr)),
        BlockOperand::SpillValue(source) => Some(BlockOperand::SpillValue(source)),
        _ => None,
    }
}

fn selector_non_block_argument_local_get4_operand(
    graph: &ValueGraph,
    op: &BlockOp,
) -> Option<BlockOperand> {
    let value = block_op_single_result(op)?;
    if local_get_result_is_non_lossless_block_argument(graph, value) {
        return None;
    }
    selector_any_local_get4_operand(op)
}

fn local_get_reads_block_argument(graph: &ValueGraph, op: &BlockOp) -> bool {
    op.kind == BlockOpKind::LocalGet
        && block_op_single_result(op)
            .is_some_and(|value| local_get_result_is_non_lossless_block_argument(graph, value))
}

fn local_get_result_is_non_lossless_block_argument(graph: &ValueGraph, value: ValueRef) -> bool {
    let node = &graph[value.0];
    if node.origin.kind != ExprOriginKind::BlockArgument && !node.is_block_argument() {
        return false;
    }
    node.address_shape.is_none()
        && node.loop_value_shape.is_none()
        && node.const_value.is_none()
        && node.key.is_none()
}

fn selector_const_delta(root_op: &BlockOp, const_op: &BlockOp) -> Option<i32> {
    let imm = block_op_i32_const(const_op)?;
    match root_op.kind {
        BlockOpKind::PureBinary(PureOpKind::I32Add) => Some(imm),
        BlockOpKind::PureBinary(PureOpKind::I32Sub) => Some(imm.wrapping_neg()),
        _ => None,
    }
}

fn match_br_if_pattern(
    graph: &ValueGraph,
    body: &BlockBody,
    terminator: &BlockTerminator,
) -> Option<(BlockTerminator, usize)> {
    let condition = *terminator.inputs.first()?;
    if graph[condition.0].needs_spill {
        return None;
    }
    if graph[condition.0].is_effect_result()
        && !last_op_is_spill_local_get_for_value(body, condition)
    {
        return None;
    }

    if let Some((mut matched, consumed)) = match_br_if_tee_pattern(graph, body, condition) {
        matched.insert(0, branch_target_operand(terminator)?);
        return Some((
            fused_br_if_terminator(
                terminator,
                vm::op_local_get4_i32_const_add_tee4_br_if as Op,
                matched,
            ),
            consumed,
        ));
    }

    if body.ops.len() >= 3 {
        if let Some((matched, root_value)) =
            match_selector_root_shape(graph, body, body.ops.len() - 3, body.ops.len())
        {
            if root_value == condition {
                let (op, operands) = match matched {
                    SelectorRootMatch::LocalConstAdd { base, imm } => (
                        vm::op_local_get4_i32_const_add_br_if as Op,
                        vec![
                            branch_target_operand(terminator)?,
                            base,
                            BlockOperand::I32(imm),
                        ],
                    ),
                    SelectorRootMatch::LocalLocalAdd { lhs, rhs } => (
                        vm::op_local_get4_local_get4_i32_add_br_if as Op,
                        vec![branch_target_operand(terminator)?, lhs, rhs],
                    ),
                };
                return Some((fused_br_if_terminator(terminator, op, operands), 3));
            }
        }
    }

    if let Some((compare, consumed)) = match_compare_br_if_pattern(graph, body, condition) {
        let (op, operands) = match compare {
            SelectorCompareMatch::Eqz { input } => (
                vm::op_local_get4_i32_eqz_br_if as Op,
                vec![branch_target_operand(terminator)?, input],
            ),
            SelectorCompareMatch::Const { lhs, cmp_kind, imm } => (
                vm::op_local_get4_i32_const_compare_br_if as Op,
                vec![
                    branch_target_operand(terminator)?,
                    lhs,
                    BlockOperand::U32(cmp_kind),
                    BlockOperand::I32(imm),
                ],
            ),
            SelectorCompareMatch::Local { lhs, rhs, cmp_kind } => (
                vm::op_local_get4_local_get4_compare_br_if as Op,
                vec![
                    branch_target_operand(terminator)?,
                    lhs,
                    rhs,
                    BlockOperand::U32(cmp_kind),
                ],
            ),
        };
        return Some((fused_br_if_terminator(terminator, op, operands), consumed));
    }

    let local_get = body.ops.last()?;
    let operand = selector_any_local_get4_operand(local_get)?;
    let value = block_op_single_result(local_get)?;
    if value != condition || value_feeds_memory_address(body, body.ops.len(), value) {
        return None;
    }
    Some((
        fused_br_if_terminator(
            terminator,
            vm::op_local_get4_br_if as Op,
            vec![branch_target_operand(terminator)?, operand],
        ),
        1,
    ))
}

fn match_br_if_tee_pattern(
    graph: &ValueGraph,
    body: &BlockBody,
    condition: ValueRef,
) -> Option<(Vec<BlockOperand>, usize)> {
    if body.ops.len() < 4 {
        return None;
    }
    let tee = body.ops.last()?;
    if tee.kind != BlockOpKind::LocalTee {
        return None;
    }
    let root_start = body.ops.len() - 4;
    let (matched, root_value) = match_selector_root_shape(graph, body, root_start, body.ops.len())?;
    if root_value != condition || value_used_after(body, body.ops.len(), root_value) {
        return None;
    }
    let dst = *tee.operands.first()?;
    let SelectorRootMatch::LocalConstAdd { base, imm } = matched else {
        return None;
    };
    Some((vec![base, BlockOperand::I32(imm), dst], 4))
}

fn match_compare_br_if_pattern(
    graph: &ValueGraph,
    body: &BlockBody,
    condition: ValueRef,
) -> Option<(SelectorCompareMatch, usize)> {
    match graph[condition.0].loop_value_shape.as_ref() {
        Some(LoopValueShape::CompareEqz { input }) => (|| {
            let LoopValueShape::Local4(slot) = input.as_ref() else {
                return None;
            };
            if body.ops.len() < 2 {
                return None;
            }
            let eqz = body.ops.last()?;
            if eqz.kind != BlockOpKind::PureUnary(PureOpKind::I32Eqz)
                || !block_op_single_use(graph, eqz)
                || block_op_single_result(eqz) != Some(condition)
            {
                return None;
            }
            let input_operand =
                selector_local_input_operand(graph, &body.ops[body.ops.len() - 2], *slot)?;
            Some((
                SelectorCompareMatch::Eqz {
                    input: input_operand,
                },
                2,
            ))
        })(),
        Some(LoopValueShape::CompareConstI32 { lhs, op, imm }) => (|| {
            let LoopValueShape::Local4(slot) = lhs.as_ref() else {
                return None;
            };
            if body.ops.len() < 3 {
                return None;
            }
            let compare = body.ops.last()?;
            if compare.kind != BlockOpKind::PureBinary(*op)
                || !block_op_single_use(graph, compare)
                || block_op_single_result(compare) != Some(condition)
            {
                return None;
            }
            let cmp_kind = encode_i32_compare_kind(*op)?;
            let lhs_operand =
                selector_local_input_operand(graph, &body.ops[body.ops.len() - 3], *slot)?;
            let const_op = &body.ops[body.ops.len() - 2];
            if block_op_i32_const(const_op)? != *imm {
                return None;
            }
            Some((
                SelectorCompareMatch::Const {
                    lhs: lhs_operand,
                    cmp_kind,
                    imm: *imm,
                },
                3,
            ))
        })(),
        Some(LoopValueShape::CompareLocal4 { lhs, op, rhs }) => (|| {
            if body.ops.len() < 3 {
                return None;
            }
            let compare = body.ops.last()?;
            if compare.kind != BlockOpKind::PureBinary(*op)
                || !block_op_single_use(graph, compare)
                || block_op_single_result(compare) != Some(condition)
            {
                return None;
            }
            let cmp_kind = encode_i32_compare_kind(*op)?;
            let lhs_operand =
                selector_local_input_operand(graph, &body.ops[body.ops.len() - 3], *lhs)?;
            let rhs_operand =
                selector_local_input_operand(graph, &body.ops[body.ops.len() - 2], *rhs)?;
            Some((
                SelectorCompareMatch::Local {
                    lhs: lhs_operand,
                    rhs: rhs_operand,
                    cmp_kind,
                },
                3,
            ))
        })(),
        _ => None,
    }
    .or_else(|| match_compare_br_if_pattern_from_slot_refs(graph, body, condition))
    .or_else(|| match_compare_br_if_pattern_from_body(graph, body, condition))
}

fn value_defined_in_body_before(body: &BlockBody, consumer_idx: usize, value: ValueRef) -> bool {
    body.ops
        .iter()
        .take(consumer_idx)
        .any(|op| block_op_single_result(op) == Some(value))
}

fn trailing_const_input_consumed(
    body: &BlockBody,
    compare_idx: usize,
    value: ValueRef,
    imm: i32,
) -> Option<usize> {
    let const_idx = compare_idx.checked_sub(1)?;
    let const_op = body.ops.get(const_idx)?;
    (const_op.kind == BlockOpKind::Const
        && block_op_single_result(const_op) == Some(value)
        && block_op_i32_const(const_op) == Some(imm))
    .then_some(2)
}

fn match_compare_br_if_pattern_from_slot_refs(
    graph: &ValueGraph,
    body: &BlockBody,
    condition: ValueRef,
) -> Option<(SelectorCompareMatch, usize)> {
    let compare_idx = body.ops.len().checked_sub(1)?;
    let compare = body.ops.get(compare_idx)?;
    if block_op_single_result(compare) != Some(condition) || !block_op_single_use(graph, compare) {
        return None;
    }
    match compare.kind {
        BlockOpKind::PureUnary(PureOpKind::I32Eqz) => {
            let input = *compare.inputs.first()?;
            if value_defined_in_body_before(body, compare_idx, input) {
                return None;
            }
            let input = selector_value_slot_ref_operand(graph, input, 4)?;
            Some((SelectorCompareMatch::Eqz { input }, 1))
        }
        BlockOpKind::PureBinary(op) if i32_compare_op(op) => {
            let lhs = *compare.inputs.first()?;
            let rhs = *compare.inputs.get(1)?;
            if let (Some(lhs_operand), Some(imm)) = (
                selector_value_slot_ref_operand(graph, lhs, 4),
                i32_const_expr(graph, rhs),
            ) {
                if value_defined_in_body_before(body, compare_idx, lhs) {
                    return None;
                }
                let consumed =
                    trailing_const_input_consumed(body, compare_idx, rhs, imm).or_else(|| {
                        (!value_defined_in_body_before(body, compare_idx, rhs)).then_some(1)
                    })?;
                return Some((
                    SelectorCompareMatch::Const {
                        lhs: lhs_operand,
                        cmp_kind: encode_i32_compare_kind(op)?,
                        imm,
                    },
                    consumed,
                ));
            }
            if let (Some(rhs_operand), Some(imm), Some(flipped)) = (
                selector_value_slot_ref_operand(graph, rhs, 4),
                i32_const_expr(graph, lhs),
                flip_i32_compare_op(op),
            ) {
                if value_defined_in_body_before(body, compare_idx, rhs) {
                    return None;
                }
                let consumed =
                    trailing_const_input_consumed(body, compare_idx, lhs, imm).or_else(|| {
                        (!value_defined_in_body_before(body, compare_idx, lhs)).then_some(1)
                    })?;
                return Some((
                    SelectorCompareMatch::Const {
                        lhs: rhs_operand,
                        cmp_kind: encode_i32_compare_kind(flipped)?,
                        imm,
                    },
                    consumed,
                ));
            }
            if value_defined_in_body_before(body, compare_idx, lhs)
                || value_defined_in_body_before(body, compare_idx, rhs)
            {
                return None;
            }
            let lhs = selector_value_slot_ref_operand(graph, lhs, 4)?;
            let rhs = selector_value_slot_ref_operand(graph, rhs, 4)?;
            Some((
                SelectorCompareMatch::Local {
                    lhs,
                    rhs,
                    cmp_kind: encode_i32_compare_kind(op)?,
                },
                1,
            ))
        }
        _ => None,
    }
}

fn match_compare_br_if_pattern_from_body(
    _graph: &ValueGraph,
    body: &BlockBody,
    _condition: ValueRef,
) -> Option<(SelectorCompareMatch, usize)> {
    if body.ops.len() >= 2 {
        let local_get = &body.ops[body.ops.len() - 2];
        let eqz = body.ops.last()?;
        if eqz.kind == BlockOpKind::PureUnary(PureOpKind::I32Eqz) {
            let operand = selector_any_local_get4_operand(local_get)?;
            return Some((SelectorCompareMatch::Eqz { input: operand }, 2));
        }
    }

    if body.ops.len() < 3 {
        return None;
    }
    let lhs_op = &body.ops[body.ops.len() - 3];
    let rhs_op = &body.ops[body.ops.len() - 2];
    let compare = body.ops.last()?;
    let BlockOpKind::PureBinary(op) = compare.kind else {
        return None;
    };
    if !i32_compare_op(op) {
        return None;
    }
    if let (Some(lhs), Some(imm)) = (
        selector_any_local_get4_operand(lhs_op),
        block_op_i32_const(rhs_op),
    ) {
        return Some((
            SelectorCompareMatch::Const {
                lhs,
                cmp_kind: encode_i32_compare_kind(op)?,
                imm,
            },
            3,
        ));
    }
    if let (Some(rhs), Some(imm), Some(flipped)) = (
        selector_any_local_get4_operand(rhs_op),
        block_op_i32_const(lhs_op),
        flip_i32_compare_op(op),
    ) {
        return Some((
            SelectorCompareMatch::Const {
                lhs: rhs,
                cmp_kind: encode_i32_compare_kind(flipped)?,
                imm,
            },
            3,
        ));
    }
    if let (Some(lhs), Some(rhs)) = (
        selector_any_local_get4_operand(lhs_op),
        selector_any_local_get4_operand(rhs_op),
    ) {
        return Some((
            SelectorCompareMatch::Local {
                lhs,
                rhs,
                cmp_kind: encode_i32_compare_kind(op)?,
            },
            3,
        ));
    }
    None
}

fn branch_target_operand(terminator: &BlockTerminator) -> Option<BlockOperand> {
    let BlockOperand::JumpTarget(target) = *terminator.operands.first()? else {
        return None;
    };
    Some(BlockOperand::JumpTarget(target))
}

fn last_op_is_spill_local_get_for_value(body: &BlockBody, value: ValueRef) -> bool {
    let Some(op) = body.ops.last() else {
        return false;
    };
    op.kind == BlockOpKind::LocalGet
        && block_op_single_result(op) == Some(value)
        && matches!(op.operands.first(), Some(BlockOperand::SpillValue(_)))
}

fn fused_br_if_terminator(
    terminator: &BlockTerminator,
    op: Op,
    mut operands: Vec<BlockOperand>,
) -> BlockTerminator {
    if !matches!(operands.first(), Some(BlockOperand::JumpTarget(_))) {
        let target = branch_target_operand(terminator).expect("br_if terminator must carry target");
        operands.insert(0, target);
    }
    BlockTerminator {
        source_start: terminator.source_start,
        op,
        kind: BlockTerminatorKind::BrIf,
        operands,
        inputs: terminator.inputs.clone(),
        values: terminator.values.clone(),
    }
}

fn encode_i32_compare_kind(op: PureOpKind) -> Option<u32> {
    match op {
        PureOpKind::I32Eq => Some(0),
        PureOpKind::I32Ne => Some(1),
        PureOpKind::I32LtS => Some(2),
        PureOpKind::I32LtU => Some(3),
        PureOpKind::I32GtS => Some(4),
        PureOpKind::I32GtU => Some(5),
        PureOpKind::I32LeS => Some(6),
        PureOpKind::I32LeU => Some(7),
        PureOpKind::I32GeS => Some(8),
        PureOpKind::I32GeU => Some(9),
        _ => None,
    }
}

fn value_used_after(body: &BlockBody, start_idx: usize, value: ValueRef) -> bool {
    value_use_count_after(body, start_idx, value) > 0
}

fn value_use_count_after(body: &BlockBody, start_idx: usize, value: ValueRef) -> usize {
    body.ops
        .iter()
        .skip(start_idx)
        .map(|op| op.inputs.iter().filter(|input| **input == value).count())
        .sum::<usize>()
        + if start_idx < body.ops.len() {
            body.terminator
                .as_ref()
                .map(|terminator| {
                    terminator
                        .inputs
                        .iter()
                        .filter(|input| **input == value)
                        .count()
                })
                .unwrap_or_default()
        } else {
            0
        }
}

fn next_entry_is_barrier(body: &BlockBody, idx: usize) -> bool {
    if let Some(op) = body.ops.get(idx) {
        return matches!(
            op.kind,
            BlockOpKind::CallLike
                | BlockOpKind::GlobalGet
                | BlockOpKind::GlobalSet
                | BlockOpKind::TableGet
                | BlockOpKind::TableSet
                | BlockOpKind::MemoryLoad
                | BlockOpKind::MemoryStore
                | BlockOpKind::Select
                | BlockOpKind::Raw
        );
    }
    body.terminator.is_some()
}

#[cfg_attr(not(test), allow(dead_code))]
fn br_if_candidate_shape(graph: &ValueGraph, condition: ValueRef) -> Option<LoopValueShape> {
    let candidate = graph[condition.0].loop_value_shape.clone().or_else(|| {
        effective_slot_shape(graph, condition)
            .and_then(|shape| shape.slot)
            .and_then(materializable_slot)
            .filter(|slot| slot.size == 4)
            .map(LoopValueShape::Local4)
    })?;
    match &candidate {
        LoopValueShape::Local4(_)
        | LoopValueShape::Local4ConstAdd { .. }
        | LoopValueShape::Local4Local4Add { .. }
        | LoopValueShape::CompareEqz { .. }
        | LoopValueShape::CompareConstI32 { .. }
        | LoopValueShape::CompareLocal4 { .. } => Some(candidate),
    }
}

fn reachable_blocks(program: &BasicBlockProgram, bodies: &[BlockBody]) -> Vec<bool> {
    let mut reachable = vec![false; program.blocks.len()];
    let mut queue = VecDeque::from([0usize]);
    while let Some(block_id) = queue.pop_front() {
        if reachable[block_id] {
            continue;
        }
        reachable[block_id] = true;
        for succ in rewritten_successors(program, block_id, &bodies[block_id]) {
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
    body: &BlockBody,
) -> Vec<usize> {
    let fallthrough = program.next_block_id(block_id);
    let Some(last) = &body.terminator else {
        return fallthrough.into_iter().collect();
    };
    match last.kind {
        BlockTerminatorKind::Br | BlockTerminatorKind::Else | BlockTerminatorKind::Return => {
            return single_target(program, last).into_iter().collect();
        }
        BlockTerminatorKind::BrIf | BlockTerminatorKind::If => {
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
        BlockTerminatorKind::BrTable => return table_targets(program, last),
        BlockTerminatorKind::SpecialFunctionReturn => return Vec::new(),
        BlockTerminatorKind::SpecialBlockReturn => return fallthrough.into_iter().collect(),
        BlockTerminatorKind::Unreachable => return Vec::new(),
        _ => {}
    }
    fallthrough.into_iter().collect()
}

fn single_target(program: &BasicBlockProgram, terminator: &BlockTerminator) -> Option<usize> {
    let BlockOperand::JumpTarget(target) = *terminator.operands.first()? else {
        return None;
    };
    program.block_for_old_start(target)
}

fn table_targets(program: &BasicBlockProgram, terminator: &BlockTerminator) -> Vec<usize> {
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

#[cfg(test)]
pub(crate) fn patch_jump_targets(records: &mut [RecordEmit]) -> Result<(), ()> {
    let mut old_to_new = HashMap::new();
    let mut cursor = 0usize;
    let mut record_positions = Vec::with_capacity(records.len());
    for record in records.iter() {
        record_positions.push((record.source_start, cursor));
        if let Some(old_start) = record.source_start {
            old_to_new.entry(old_start).or_insert(cursor);
        }
        cursor += record.len();
    }
    for record in records.iter_mut() {
        if let Some(target_index) = record_jump_target_operand_index(record.op) {
            let target = unsafe { record.operands[target_index].jump_addr as usize };
            let Some(patched) = old_to_new
                .get(&target)
                .copied()
                .or_else(|| infer_missing_target_cursor(&record_positions, target))
            else {
                return Err(());
            };
            record.operands[target_index] = Operand {
                jump_addr: patched as u32,
            };
        } else if std::ptr::fn_addr_eq(record.op, vm::op_br_table as Op) {
            let table_len = unsafe { record.operands[0].u32 as usize };
            for idx in 1..=table_len + 1 {
                let target = unsafe { record.operands[idx].jump_addr as usize };
                let Some(patched) = old_to_new.get(&target).copied() else {
                    return Err(());
                };
                record.operands[idx] = Operand {
                    jump_addr: patched as u32,
                };
            }
        }
    }
    Ok(())
}

fn patch_packed_jump_targets(ops: &mut [PackedOp]) -> Result<(), ()> {
    let mut old_to_new = HashMap::new();
    let mut cursor = 0usize;
    let mut record_positions = Vec::with_capacity(ops.len());
    for op in ops.iter() {
        record_positions.push((op.source_start, cursor));
        if let Some(old_start) = op.source_start {
            old_to_new.entry(old_start).or_insert(cursor);
        }
        cursor += op.len();
    }
    for op in ops.iter_mut() {
        if let Some(target_index) = record_jump_target_operand_index(op.op) {
            let Some(PackedOperand::JumpTarget(target)) = op.operands.get(target_index).copied()
            else {
                return Err(());
            };
            let Some(patched) = old_to_new
                .get(&(target as usize))
                .copied()
                .or_else(|| infer_missing_target_cursor(&record_positions, target as usize))
            else {
                return Err(());
            };
            op.operands[target_index] = PackedOperand::JumpTarget(patched as u32);
        } else if std::ptr::fn_addr_eq(op.op, vm::op_br_table as Op) {
            let Some(PackedOperand::U32(table_len)) = op.operands.first().copied() else {
                return Err(());
            };
            for idx in 1..=table_len as usize + 1 {
                let Some(PackedOperand::JumpTarget(target)) = op.operands.get(idx).copied() else {
                    return Err(());
                };
                let Some(patched) = old_to_new.get(&(target as usize)).copied() else {
                    return Err(());
                };
                op.operands[idx] = PackedOperand::JumpTarget(patched as u32);
            }
        }
    }
    Ok(())
}

fn infer_missing_target_cursor(
    record_positions: &[(Option<usize>, usize)],
    target: usize,
) -> Option<usize> {
    let next_known = record_positions
        .iter()
        .position(|(source_start, _)| source_start.is_some_and(|start| start > target))?;
    let first_unknown_before_next = record_positions[..next_known]
        .iter()
        .rposition(|(source_start, _)| source_start.is_some())
        .map_or(0, |idx| idx + 1);
    if let Some(cursor) = record_positions
        .get(first_unknown_before_next..next_known)?
        .iter()
        .find(|(source_start, _)| source_start.is_none())
        .map(|(_, cursor)| *cursor)
    {
        return Some(cursor);
    }
    record_positions.get(next_known).map(|(_, cursor)| *cursor)
}

fn record_jump_target_operand_index(op: Op) -> Option<usize> {
    if std::ptr::fn_addr_eq(op, vm::op_if as Op)
        || std::ptr::fn_addr_eq(op, vm::op_else as Op)
        || std::ptr::fn_addr_eq(op, vm::op_br as Op)
        || std::ptr::fn_addr_eq(op, vm::op_br_if as Op)
        || std::ptr::fn_addr_eq(op, vm::op_return as Op)
    {
        return Some(0);
    }
    if std::ptr::fn_addr_eq(op, vm::op_local_get4_br_if as Op)
        || std::ptr::fn_addr_eq(op, vm::op_local_get4_i32_eqz_br_if as Op)
    {
        return Some(1);
    }
    if std::ptr::fn_addr_eq(op, vm::op_local_get4_i32_const_add_br_if as Op)
        || std::ptr::fn_addr_eq(op, vm::op_local_get4_local_get4_i32_add_br_if as Op)
    {
        return Some(2);
    }
    if std::ptr::fn_addr_eq(op, vm::op_local_get4_i32_const_compare_br_if as Op)
        || std::ptr::fn_addr_eq(op, vm::op_local_get4_local_get4_compare_br_if as Op)
        || std::ptr::fn_addr_eq(op, vm::op_local_get4_i32_const_add_tee4_br_if as Op)
    {
        return Some(3);
    }
    None
}

fn verify_explicit_effect_ir(program: &BasicBlockProgram, bodies: &[BlockBody]) -> bool {
    let reachable = reachable_blocks(program, bodies);
    verify_barrier_op_counts(program, &reachable, |source_start, op| {
        count_explicit_ir_ops(bodies, source_start, op)
    })
}

fn verify_slot_plan(program: &BasicBlockProgram, rewrite: &FunctionRewrite) -> bool {
    for block in &program.blocks {
        let entry = &rewrite.entries[block.id];
        if !entry.reachable {
            continue;
        }
        let mut incoming = Vec::new();
        if block.id == 0 {
            continue;
        }
        for pred in &program.predecessors[block.id] {
            let pred_state = &rewrite.exits[*pred];
            if pred_state.reachable {
                incoming.push(pred_state);
            }
        }
        if incoming.is_empty() {
            continue;
        }
        let copy_plan = &rewrite.relower.block_copy_plans[block.id];
        for (ordinal, merged) in entry.stack.iter().copied().enumerate() {
            let values = incoming
                .iter()
                .map(|state| state.stack.get(ordinal))
                .collect::<Vec<_>>();
            if slot_merge_decision(&rewrite.graph, &values, merged)
                != copy_plan.stack.get(&ordinal).copied()
            {
                return false;
            }
        }
        for (slot, merged) in &entry.locals {
            if !rewrite.graph[merged.0].is_block_argument() {
                continue;
            }
            if rewrite.graph[merged.0]
                .slot_shape
                .as_ref()
                .and_then(|shape| shape.slot)
                != Some(slot_ref_for_local_slot(*slot))
            {
                return false;
            }
            if copy_plan.locals.get(slot) != Some(&SlotMergeDecision::Preserve) {
                return false;
            }
        }
        for (key, merged) in &entry.aliases {
            let values = incoming
                .iter()
                .map(|state| state.aliases.get(key))
                .collect::<Vec<_>>();
            if slot_merge_decision(&rewrite.graph, &values, *merged)
                != copy_plan.aliases.get(key).copied()
            {
                return false;
            }
        }
    }
    true
}

fn verify_relower_preserves_call_ops(
    program: &BasicBlockProgram,
    bodies: &[BlockBody],
    ops: &[PackedOp],
) -> bool {
    let reachable = reachable_blocks(program, bodies);
    verify_barrier_op_counts(program, &reachable, |source_start, op| {
        ops.iter()
            .filter(|packed| {
                packed.source_start == Some(source_start) && std::ptr::fn_addr_eq(packed.op, op)
            })
            .count()
    })
}

fn verify_effect_result_spill_ir(
    graph: &ValueGraph,
    bodies: &[BlockBody],
    spill_plan: &EffectResultSpillPlan,
) -> bool {
    for body in bodies {
        for op in &body.ops {
            for operand in &op.operands {
                let BlockOperand::SpillValue(source) = *operand else {
                    continue;
                };
                let node = &graph[source.0];
                if !node.is_effect_result()
                    || !node.needs_spill
                    || spill_plan.slot(source).is_none()
                {
                    return false;
                }
            }
        }
        if let Some(terminator) = &body.terminator {
            for operand in &terminator.operands {
                let BlockOperand::SpillValue(source) = *operand else {
                    continue;
                };
                let node = &graph[source.0];
                if !node.is_effect_result()
                    || !node.needs_spill
                    || spill_plan.slot(source).is_none()
                {
                    return false;
                }
            }
        }
    }
    true
}

fn verify_relower_preserves_effect_result_spills(
    graph: &ValueGraph,
    bodies: &[BlockBody],
    spill_plan: &EffectResultSpillPlan,
    ops: &[PackedOp],
) -> bool {
    let mut expected_reads = HashMap::new();
    for body in bodies {
        for op in &body.ops {
            for operand in &op.operands {
                let BlockOperand::SpillValue(source) = *operand else {
                    continue;
                };
                let Some(slot) = spill_plan.slot(source) else {
                    return false;
                };
                *expected_reads
                    .entry((slot.addr, slot.size))
                    .or_insert(0usize) += 1;
            }
        }
        if let Some(terminator) = &body.terminator {
            for operand in &terminator.operands {
                let BlockOperand::SpillValue(source) = *operand else {
                    continue;
                };
                let Some(slot) = spill_plan.slot(source) else {
                    return false;
                };
                *expected_reads
                    .entry((slot.addr, slot.size))
                    .or_insert(0usize) += 1;
            }
        }
    }

    let mut actual_reads = HashMap::new();
    for op in ops {
        for &(addr, size) in expected_reads.keys() {
            if std::ptr::fn_addr_eq(op.op, local_tee_op(size)) {
                continue;
            }
            let count = op
                .operands
                .iter()
                .filter(|operand| matches!(operand, PackedOperand::LocalAddr(local_addr) if *local_addr == addr))
                .count();
            if count > 0 {
                *actual_reads.entry((addr, size)).or_insert(0usize) += count;
            }
        }
    }

    for (slot_key, expected) in expected_reads {
        if actual_reads.get(&slot_key).copied().unwrap_or_default() < expected {
            return false;
        }
    }

    for (&value, slot) in &spill_plan.slots {
        if !graph[value.0].needs_spill {
            return false;
        }
        let tee_count = ops
            .iter()
            .filter(|op| {
                op.source_start.is_none()
                    && std::ptr::fn_addr_eq(op.op, local_tee_op(slot.size))
                    && matches!(op.operands.first(), Some(PackedOperand::LocalAddr(addr)) if *addr == slot.addr)
            })
            .count();
        if tee_count == 0 {
            return false;
        }
    }

    true
}

fn verify_barrier_op_counts(
    program: &BasicBlockProgram,
    reachable: &[bool],
    mut count_same_op: impl FnMut(usize, Op) -> usize,
) -> bool {
    for block in &program.blocks {
        if !reachable[block.id] {
            continue;
        }
        for record in &program.records[block.start..block.end] {
            if !matches!(effect_barrier(record), EffectBarrier::Call) {
                continue;
            }
            let count = count_same_op(record.old_start, record.op);
            if count != 1 {
                return false;
            }
        }
    }
    true
}

fn count_explicit_ir_ops(bodies: &[BlockBody], source_start: usize, op: Op) -> usize {
    let mut count = 0usize;
    for body in bodies {
        count += body
            .ops
            .iter()
            .filter(|entry| {
                entry.source_start == Some(source_start) && std::ptr::fn_addr_eq(entry.op, op)
            })
            .count();
        count += body
            .terminator
            .iter()
            .filter(|entry| {
                entry.source_start == Some(source_start) && std::ptr::fn_addr_eq(entry.op, op)
            })
            .count();
    }
    count
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
    let size = local_get_size_from_op(record.op)?;
    Some(LocalSlot::new(record.operand_local_addr(), size))
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
        || record.op_eq(vm::op_i32_load8_s as Op)
        || record.op_eq(vm::op_i32_load8_u as Op)
        || record.op_eq(vm::op_i32_load16_s as Op)
        || record.op_eq(vm::op_i32_load16_u as Op)
        || record.op_eq(vm::op_i32_load_shared as Op)
        || record.op_eq(vm::op_i32_load8_s_shared as Op)
        || record.op_eq(vm::op_i32_load8_u_shared as Op)
        || record.op_eq(vm::op_i32_load16_s_shared as Op)
        || record.op_eq(vm::op_i32_load16_u_shared as Op)
        || record.op_eq(vm::op_i32_load_indexed_local as Op)
        || record.op_eq(vm::op_i32_load8_s_indexed_local as Op)
        || record.op_eq(vm::op_i32_load8_u_indexed_local as Op)
        || record.op_eq(vm::op_i32_load16_s_indexed_local as Op)
        || record.op_eq(vm::op_i32_load16_u_indexed_local as Op)
        || record.op_eq(vm::op_i32_load_indexed_shared as Op)
        || record.op_eq(vm::op_i32_load8_s_indexed_shared as Op)
        || record.op_eq(vm::op_i32_load8_u_indexed_shared as Op)
        || record.op_eq(vm::op_i32_load16_s_indexed_shared as Op)
        || record.op_eq(vm::op_i32_load16_u_indexed_shared as Op)
        || record.op_eq(vm::op_i32_load_local as Op)
        || record.op_eq(vm::op_i32_load8_s_local as Op)
        || record.op_eq(vm::op_i32_load8_u_local as Op)
        || record.op_eq(vm::op_i32_load16_s_local as Op)
        || record.op_eq(vm::op_i32_load16_u_local as Op)
    {
        let offset = record.operand_memarg(0).offset;
        let width = if record.op_eq(vm::op_i32_load8_s as Op)
            || record.op_eq(vm::op_i32_load8_u as Op)
            || record.op_eq(vm::op_i32_load8_s_shared as Op)
            || record.op_eq(vm::op_i32_load8_u_shared as Op)
            || record.op_eq(vm::op_i32_load8_s_indexed_local as Op)
            || record.op_eq(vm::op_i32_load8_u_indexed_local as Op)
            || record.op_eq(vm::op_i32_load8_s_indexed_shared as Op)
            || record.op_eq(vm::op_i32_load8_u_indexed_shared as Op)
            || record.op_eq(vm::op_i32_load8_s_local as Op)
            || record.op_eq(vm::op_i32_load8_u_local as Op)
        {
            1
        } else if record.op_eq(vm::op_i32_load16_s as Op)
            || record.op_eq(vm::op_i32_load16_u as Op)
            || record.op_eq(vm::op_i32_load16_s_shared as Op)
            || record.op_eq(vm::op_i32_load16_u_shared as Op)
            || record.op_eq(vm::op_i32_load16_s_indexed_local as Op)
            || record.op_eq(vm::op_i32_load16_u_indexed_local as Op)
            || record.op_eq(vm::op_i32_load16_s_indexed_shared as Op)
            || record.op_eq(vm::op_i32_load16_u_indexed_shared as Op)
            || record.op_eq(vm::op_i32_load16_s_local as Op)
            || record.op_eq(vm::op_i32_load16_u_local as Op)
        {
            2
        } else {
            4
        };
        return Some(MemoryAccess {
            memidx: memory_index(record),
            offset,
            width,
            ty: ValType::I32,
        });
    }
    if record.op_eq(vm::op_i64_load as Op)
        || record.op_eq(vm::op_i64_load8_s as Op)
        || record.op_eq(vm::op_i64_load8_u as Op)
        || record.op_eq(vm::op_i64_load16_s as Op)
        || record.op_eq(vm::op_i64_load16_u as Op)
        || record.op_eq(vm::op_i64_load32_s as Op)
        || record.op_eq(vm::op_i64_load32_u as Op)
        || record.op_eq(vm::op_i64_load_shared as Op)
        || record.op_eq(vm::op_i64_load8_s_shared as Op)
        || record.op_eq(vm::op_i64_load8_u_shared as Op)
        || record.op_eq(vm::op_i64_load16_s_shared as Op)
        || record.op_eq(vm::op_i64_load16_u_shared as Op)
        || record.op_eq(vm::op_i64_load32_s_shared as Op)
        || record.op_eq(vm::op_i64_load32_u_shared as Op)
        || record.op_eq(vm::op_i64_load_indexed_local as Op)
        || record.op_eq(vm::op_i64_load8_s_indexed_local as Op)
        || record.op_eq(vm::op_i64_load8_u_indexed_local as Op)
        || record.op_eq(vm::op_i64_load16_s_indexed_local as Op)
        || record.op_eq(vm::op_i64_load16_u_indexed_local as Op)
        || record.op_eq(vm::op_i64_load32_s_indexed_local as Op)
        || record.op_eq(vm::op_i64_load32_u_indexed_local as Op)
        || record.op_eq(vm::op_i64_load_indexed_shared as Op)
        || record.op_eq(vm::op_i64_load8_s_indexed_shared as Op)
        || record.op_eq(vm::op_i64_load8_u_indexed_shared as Op)
        || record.op_eq(vm::op_i64_load16_s_indexed_shared as Op)
        || record.op_eq(vm::op_i64_load16_u_indexed_shared as Op)
        || record.op_eq(vm::op_i64_load32_s_indexed_shared as Op)
        || record.op_eq(vm::op_i64_load32_u_indexed_shared as Op)
        || record.op_eq(vm::op_i64_load_local as Op)
        || record.op_eq(vm::op_i64_load8_s_local as Op)
        || record.op_eq(vm::op_i64_load8_u_local as Op)
        || record.op_eq(vm::op_i64_load16_s_local as Op)
        || record.op_eq(vm::op_i64_load16_u_local as Op)
        || record.op_eq(vm::op_i64_load32_s_local as Op)
        || record.op_eq(vm::op_i64_load32_u_local as Op)
    {
        let offset = record.operand_memarg(0).offset;
        let width = if record.op_eq(vm::op_i64_load8_s as Op)
            || record.op_eq(vm::op_i64_load8_u as Op)
            || record.op_eq(vm::op_i64_load8_s_shared as Op)
            || record.op_eq(vm::op_i64_load8_u_shared as Op)
            || record.op_eq(vm::op_i64_load8_s_indexed_local as Op)
            || record.op_eq(vm::op_i64_load8_u_indexed_local as Op)
            || record.op_eq(vm::op_i64_load8_s_indexed_shared as Op)
            || record.op_eq(vm::op_i64_load8_u_indexed_shared as Op)
            || record.op_eq(vm::op_i64_load8_s_local as Op)
            || record.op_eq(vm::op_i64_load8_u_local as Op)
        {
            1
        } else if record.op_eq(vm::op_i64_load16_s as Op)
            || record.op_eq(vm::op_i64_load16_u as Op)
            || record.op_eq(vm::op_i64_load16_s_shared as Op)
            || record.op_eq(vm::op_i64_load16_u_shared as Op)
            || record.op_eq(vm::op_i64_load16_s_indexed_local as Op)
            || record.op_eq(vm::op_i64_load16_u_indexed_local as Op)
            || record.op_eq(vm::op_i64_load16_s_indexed_shared as Op)
            || record.op_eq(vm::op_i64_load16_u_indexed_shared as Op)
            || record.op_eq(vm::op_i64_load16_s_local as Op)
            || record.op_eq(vm::op_i64_load16_u_local as Op)
        {
            2
        } else if record.op_eq(vm::op_i64_load32_s as Op)
            || record.op_eq(vm::op_i64_load32_u as Op)
            || record.op_eq(vm::op_i64_load32_s_shared as Op)
            || record.op_eq(vm::op_i64_load32_u_shared as Op)
            || record.op_eq(vm::op_i64_load32_s_indexed_local as Op)
            || record.op_eq(vm::op_i64_load32_u_indexed_local as Op)
            || record.op_eq(vm::op_i64_load32_s_indexed_shared as Op)
            || record.op_eq(vm::op_i64_load32_u_indexed_shared as Op)
            || record.op_eq(vm::op_i64_load32_s_local as Op)
            || record.op_eq(vm::op_i64_load32_u_local as Op)
        {
            4
        } else {
            8
        };
        return Some(MemoryAccess {
            memidx: memory_index(record),
            offset,
            width,
            ty: ValType::I64,
        });
    }
    if record.op_eq(vm::op_f32_load as Op)
        || record.op_eq(vm::op_f32_load_shared as Op)
        || record.op_eq(vm::op_f32_load_indexed_local as Op)
        || record.op_eq(vm::op_f32_load_indexed_shared as Op)
    {
        let offset = record.operand_memarg(0).offset;
        return Some(MemoryAccess {
            memidx: memory_index(record),
            offset,
            width: 4,
            ty: ValType::F32,
        });
    }
    if record.op_eq(vm::op_f64_load as Op)
        || record.op_eq(vm::op_f64_load_shared as Op)
        || record.op_eq(vm::op_f64_load_indexed_local as Op)
        || record.op_eq(vm::op_f64_load_indexed_shared as Op)
    {
        let offset = record.operand_memarg(0).offset;
        return Some(MemoryAccess {
            memidx: memory_index(record),
            offset,
            width: 8,
            ty: ValType::F64,
        });
    }
    None
}

fn decode_memory_store(record: &DecodedInstr) -> Option<MemoryAccess> {
    if record.op_eq(vm::op_i32_store as Op)
        || record.op_eq(vm::op_i32_store8 as Op)
        || record.op_eq(vm::op_i32_store16 as Op)
        || record.op_eq(vm::op_i32_store_shared as Op)
        || record.op_eq(vm::op_i32_store8_shared as Op)
        || record.op_eq(vm::op_i32_store16_shared as Op)
        || record.op_eq(vm::op_i32_store_indexed_local as Op)
        || record.op_eq(vm::op_i32_store8_indexed_local as Op)
        || record.op_eq(vm::op_i32_store16_indexed_local as Op)
        || record.op_eq(vm::op_i32_store_indexed_shared as Op)
        || record.op_eq(vm::op_i32_store8_indexed_shared as Op)
        || record.op_eq(vm::op_i32_store16_indexed_shared as Op)
        || record.op_eq(vm::op_i32_store_local as Op)
        || record.op_eq(vm::op_i32_store8_local as Op)
        || record.op_eq(vm::op_i32_store16_local as Op)
    {
        let offset = record.operand_memarg(0).offset;
        let width = if record.op_eq(vm::op_i32_store8 as Op)
            || record.op_eq(vm::op_i32_store8_shared as Op)
            || record.op_eq(vm::op_i32_store8_indexed_local as Op)
            || record.op_eq(vm::op_i32_store8_indexed_shared as Op)
            || record.op_eq(vm::op_i32_store8_local as Op)
        {
            1
        } else if record.op_eq(vm::op_i32_store16 as Op)
            || record.op_eq(vm::op_i32_store16_shared as Op)
            || record.op_eq(vm::op_i32_store16_indexed_local as Op)
            || record.op_eq(vm::op_i32_store16_indexed_shared as Op)
            || record.op_eq(vm::op_i32_store16_local as Op)
        {
            2
        } else {
            4
        };
        return Some(MemoryAccess {
            memidx: memory_index(record),
            offset,
            width,
            ty: ValType::I32,
        });
    }
    if record.op_eq(vm::op_i64_store as Op)
        || record.op_eq(vm::op_i64_store8 as Op)
        || record.op_eq(vm::op_i64_store16 as Op)
        || record.op_eq(vm::op_i64_store32 as Op)
        || record.op_eq(vm::op_i64_store_shared as Op)
        || record.op_eq(vm::op_i64_store8_shared as Op)
        || record.op_eq(vm::op_i64_store16_shared as Op)
        || record.op_eq(vm::op_i64_store32_shared as Op)
        || record.op_eq(vm::op_i64_store_indexed_local as Op)
        || record.op_eq(vm::op_i64_store8_indexed_local as Op)
        || record.op_eq(vm::op_i64_store16_indexed_local as Op)
        || record.op_eq(vm::op_i64_store32_indexed_local as Op)
        || record.op_eq(vm::op_i64_store_indexed_shared as Op)
        || record.op_eq(vm::op_i64_store8_indexed_shared as Op)
        || record.op_eq(vm::op_i64_store16_indexed_shared as Op)
        || record.op_eq(vm::op_i64_store32_indexed_shared as Op)
        || record.op_eq(vm::op_i64_store_local as Op)
        || record.op_eq(vm::op_i64_store8_local as Op)
        || record.op_eq(vm::op_i64_store16_local as Op)
        || record.op_eq(vm::op_i64_store32_local as Op)
    {
        let offset = record.operand_memarg(0).offset;
        let width = if record.op_eq(vm::op_i64_store8 as Op)
            || record.op_eq(vm::op_i64_store8_shared as Op)
            || record.op_eq(vm::op_i64_store8_indexed_local as Op)
            || record.op_eq(vm::op_i64_store8_indexed_shared as Op)
            || record.op_eq(vm::op_i64_store8_local as Op)
        {
            1
        } else if record.op_eq(vm::op_i64_store16 as Op)
            || record.op_eq(vm::op_i64_store16_shared as Op)
            || record.op_eq(vm::op_i64_store16_indexed_local as Op)
            || record.op_eq(vm::op_i64_store16_indexed_shared as Op)
            || record.op_eq(vm::op_i64_store16_local as Op)
        {
            2
        } else if record.op_eq(vm::op_i64_store32 as Op)
            || record.op_eq(vm::op_i64_store32_shared as Op)
            || record.op_eq(vm::op_i64_store32_indexed_local as Op)
            || record.op_eq(vm::op_i64_store32_indexed_shared as Op)
            || record.op_eq(vm::op_i64_store32_local as Op)
        {
            4
        } else {
            8
        };
        return Some(MemoryAccess {
            memidx: memory_index(record),
            offset,
            width,
            ty: ValType::I64,
        });
    }
    if record.op_eq(vm::op_f32_store as Op)
        || record.op_eq(vm::op_f32_store_shared as Op)
        || record.op_eq(vm::op_f32_store_indexed_local as Op)
        || record.op_eq(vm::op_f32_store_indexed_shared as Op)
    {
        let offset = record.operand_memarg(0).offset;
        return Some(MemoryAccess {
            memidx: memory_index(record),
            offset,
            width: 4,
            ty: ValType::F32,
        });
    }
    if record.op_eq(vm::op_f64_store as Op)
        || record.op_eq(vm::op_f64_store_shared as Op)
        || record.op_eq(vm::op_f64_store_indexed_local as Op)
        || record.op_eq(vm::op_f64_store_indexed_shared as Op)
    {
        let offset = record.operand_memarg(0).offset;
        return Some(MemoryAccess {
            memidx: memory_index(record),
            offset,
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
    pure_unary_kind_from_op(record.op)
}

fn decode_pure_binary(record: &DecodedInstr) -> Option<PureOpKind> {
    pure_binary_kind_from_op(record.op)
}

fn unary_op(op: PureOpKind) -> Option<Op> {
    match op {
        PureOpKind::I32Eqz => Some(vm::op_i32_eqz as Op),
        PureOpKind::I32Clz => Some(vm::op_i32_clz as Op),
        PureOpKind::I32Ctz => Some(vm::op_i32_ctz as Op),
        PureOpKind::I32Popcnt => Some(vm::op_i32_popcnt as Op),
        PureOpKind::I64Eqz => Some(vm::op_i64_eqz as Op),
        PureOpKind::I64Clz => Some(vm::op_i64_clz as Op),
        PureOpKind::I64Ctz => Some(vm::op_i64_ctz as Op),
        PureOpKind::I64Popcnt => Some(vm::op_i64_popcnt as Op),
        PureOpKind::F32Abs => Some(vm::op_f32_abs as Op),
        PureOpKind::F32Neg => Some(vm::op_f32_neg as Op),
        PureOpKind::F32Sqrt => Some(vm::op_f32_sqrt as Op),
        PureOpKind::F32Ceil => Some(vm::op_f32_ceil as Op),
        PureOpKind::F32Floor => Some(vm::op_f32_floor as Op),
        PureOpKind::F32Trunc => Some(vm::op_f32_trunc as Op),
        PureOpKind::F32Nearest => Some(vm::op_f32_nearest as Op),
        PureOpKind::F64Abs => Some(vm::op_f64_abs as Op),
        PureOpKind::F64Neg => Some(vm::op_f64_neg as Op),
        PureOpKind::F64Sqrt => Some(vm::op_f64_sqrt as Op),
        PureOpKind::F64Ceil => Some(vm::op_f64_ceil as Op),
        PureOpKind::F64Floor => Some(vm::op_f64_floor as Op),
        PureOpKind::F64Trunc => Some(vm::op_f64_trunc as Op),
        PureOpKind::F64Nearest => Some(vm::op_f64_nearest as Op),
        _ => None,
    }
}

fn binary_op(op: PureOpKind) -> Option<Op> {
    match op {
        PureOpKind::I32Eqz
        | PureOpKind::I32Clz
        | PureOpKind::I32Ctz
        | PureOpKind::I32Popcnt
        | PureOpKind::I64Eqz
        | PureOpKind::I64Clz
        | PureOpKind::I64Ctz
        | PureOpKind::I64Popcnt
        | PureOpKind::F32Abs
        | PureOpKind::F32Neg
        | PureOpKind::F32Sqrt
        | PureOpKind::F32Ceil
        | PureOpKind::F32Floor
        | PureOpKind::F32Trunc
        | PureOpKind::F32Nearest
        | PureOpKind::F64Abs
        | PureOpKind::F64Neg
        | PureOpKind::F64Sqrt
        | PureOpKind::F64Ceil
        | PureOpKind::F64Floor
        | PureOpKind::F64Trunc
        | PureOpKind::F64Nearest => None,
        PureOpKind::I32Add => Some(vm::op_i32_add as Op),
        PureOpKind::I32Sub => Some(vm::op_i32_sub as Op),
        PureOpKind::I32Mul => Some(vm::op_i32_mul as Op),
        PureOpKind::I32And => Some(vm::op_i32_and as Op),
        PureOpKind::I32Or => Some(vm::op_i32_or as Op),
        PureOpKind::I32Xor => Some(vm::op_i32_xor as Op),
        PureOpKind::I32Shl => Some(vm::op_i32_shl as Op),
        PureOpKind::I32ShrS => Some(vm::op_i32_shr_s as Op),
        PureOpKind::I32ShrU => Some(vm::op_i32_shr_u as Op),
        PureOpKind::I32Rotl => Some(vm::op_i32_rotl as Op),
        PureOpKind::I32Rotr => Some(vm::op_i32_rotr as Op),
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
        PureOpKind::I64Mul => Some(vm::op_i64_mul as Op),
        PureOpKind::I64And => Some(vm::op_i64_and as Op),
        PureOpKind::I64Or => Some(vm::op_i64_or as Op),
        PureOpKind::I64Xor => Some(vm::op_i64_xor as Op),
        PureOpKind::I64Shl => Some(vm::op_i64_shl as Op),
        PureOpKind::I64ShrS => Some(vm::op_i64_shr_s as Op),
        PureOpKind::I64ShrU => Some(vm::op_i64_shr_u as Op),
        PureOpKind::I64Rotl => Some(vm::op_i64_rotl as Op),
        PureOpKind::I64Rotr => Some(vm::op_i64_rotr as Op),
        PureOpKind::I64Eq => Some(vm::op_i64_eq as Op),
        PureOpKind::I64Ne => Some(vm::op_i64_ne as Op),
        PureOpKind::I64LtS => Some(vm::op_i64_lt_s as Op),
        PureOpKind::I64LtU => Some(vm::op_i64_lt_u as Op),
        PureOpKind::I64GtS => Some(vm::op_i64_gt_s as Op),
        PureOpKind::I64GtU => Some(vm::op_i64_gt_u as Op),
        PureOpKind::I64LeS => Some(vm::op_i64_le_s as Op),
        PureOpKind::I64LeU => Some(vm::op_i64_le_u as Op),
        PureOpKind::I64GeS => Some(vm::op_i64_ge_s as Op),
        PureOpKind::I64GeU => Some(vm::op_i64_ge_u as Op),
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

fn is_memory_grow_op(op: Op) -> bool {
    std::ptr::fn_addr_eq(op, vm::op_mem_grow_local as Op)
        || std::ptr::fn_addr_eq(op, vm::op_mem_grow_shared as Op)
        || std::ptr::fn_addr_eq(op, vm::op_mem_grow_indexed_local as Op)
        || std::ptr::fn_addr_eq(op, vm::op_mem_grow_indexed_shared as Op)
}

#[cfg(feature = "threads")]
fn decode_atomic_barrier_shape(record: &DecodedInstr) -> Option<AtomicBarrierShape> {
    if record.op_eq(vm::op_memory_atomic_notify_unshared as Op)
        || record.op_eq(vm::op_memory_atomic_notify_shared as Op)
        || record.op_eq(vm::op_memory_atomic_notify_indexed_unshared as Op)
        || record.op_eq(vm::op_memory_atomic_notify_indexed_shared as Op)
    {
        return Some(AtomicBarrierShape { input_count: 2 });
    }
    if record.op_eq(vm::op_memory_atomic_wait32_unshared as Op)
        || record.op_eq(vm::op_memory_atomic_wait32_shared as Op)
        || record.op_eq(vm::op_memory_atomic_wait32_indexed_unshared as Op)
        || record.op_eq(vm::op_memory_atomic_wait32_indexed_shared as Op)
        || record.op_eq(vm::op_memory_atomic_wait64_unshared as Op)
        || record.op_eq(vm::op_memory_atomic_wait64_shared as Op)
        || record.op_eq(vm::op_memory_atomic_wait64_indexed_unshared as Op)
        || record.op_eq(vm::op_memory_atomic_wait64_indexed_shared as Op)
    {
        return Some(AtomicBarrierShape { input_count: 3 });
    }
    None
}

fn decode_trap_sensitive_barrier_shape(record: &DecodedInstr) -> Option<TrapSensitiveBarrierShape> {
    if record.op_eq(vm::op_i32_div_s)
        || record.op_eq(vm::op_i32_div_u)
        || record.op_eq(vm::op_i32_rem_s)
        || record.op_eq(vm::op_i32_rem_u)
    {
        return Some(TrapSensitiveBarrierShape {
            input_count: 2,
            result_ty: ValType::I32,
        });
    }
    if record.op_eq(vm::op_i64_div_s)
        || record.op_eq(vm::op_i64_div_u)
        || record.op_eq(vm::op_i64_rem_s)
        || record.op_eq(vm::op_i64_rem_u)
    {
        return Some(TrapSensitiveBarrierShape {
            input_count: 2,
            result_ty: ValType::I64,
        });
    }
    if record.op_eq(vm::op_i32_trunc_f32_s)
        || record.op_eq(vm::op_i32_trunc_f32_u)
        || record.op_eq(vm::op_i32_trunc_f64_s)
        || record.op_eq(vm::op_i32_trunc_f64_u)
    {
        return Some(TrapSensitiveBarrierShape {
            input_count: 1,
            result_ty: ValType::I32,
        });
    }
    if record.op_eq(vm::op_i64_trunc_f32_s)
        || record.op_eq(vm::op_i64_trunc_f32_u)
        || record.op_eq(vm::op_i64_trunc_f64_s)
        || record.op_eq(vm::op_i64_trunc_f64_u)
    {
        return Some(TrapSensitiveBarrierShape {
            input_count: 1,
            result_ty: ValType::I64,
        });
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
        || is_memory_grow_op(record.op)
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

fn value_type_size(ty: ValType) -> Option<u32> {
    match ty {
        ValType::I32 | ValType::F32 => Some(4),
        ValType::I64 | ValType::F64 => Some(8),
        ValType::V128 => Some(16),
        _ => None,
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

fn local_get_size_from_op(op: Op) -> Option<u32> {
    if std::ptr::fn_addr_eq(op, vm::op_local_get4 as Op)
        || std::ptr::fn_addr_eq(op, vm::op_local_get4_profiled as Op)
    {
        return Some(4);
    }
    if std::ptr::fn_addr_eq(op, vm::op_local_get8 as Op)
        || std::ptr::fn_addr_eq(op, vm::op_local_get8_profiled as Op)
    {
        return Some(8);
    }
    if std::ptr::fn_addr_eq(op, vm::op_local_get16 as Op)
        || std::ptr::fn_addr_eq(op, vm::op_local_get16_profiled as Op)
    {
        return Some(16);
    }
    None
}

fn local_set_op(size: u32) -> Op {
    match size {
        4 => vm::op_local_set4 as Op,
        8 => vm::op_local_set8 as Op,
        16 => vm::op_local_set16 as Op,
        _ => vm::op_local_set4 as Op,
    }
}

fn local_tee_op(size: u32) -> Op {
    match size {
        4 => vm::op_local_tee4 as Op,
        8 => vm::op_local_tee8 as Op,
        16 => vm::op_local_tee16 as Op,
        _ => vm::op_local_tee4 as Op,
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

fn alias_address_supports_pure_chain(op: PureOpKind) -> bool {
    unary_op(op).is_some() || binary_op(op).is_some()
}

fn unary_output_type(op: PureOpKind) -> ValType {
    match op {
        PureOpKind::I32Eqz | PureOpKind::I64Eqz => ValType::I32,
        PureOpKind::I32Clz | PureOpKind::I32Ctz | PureOpKind::I32Popcnt => ValType::I32,
        PureOpKind::I64Clz | PureOpKind::I64Ctz | PureOpKind::I64Popcnt => ValType::I64,
        PureOpKind::F32Abs
        | PureOpKind::F32Neg
        | PureOpKind::F32Sqrt
        | PureOpKind::F32Ceil
        | PureOpKind::F32Floor
        | PureOpKind::F32Trunc
        | PureOpKind::F32Nearest => ValType::F32,
        PureOpKind::F64Abs
        | PureOpKind::F64Neg
        | PureOpKind::F64Sqrt
        | PureOpKind::F64Ceil
        | PureOpKind::F64Floor
        | PureOpKind::F64Trunc
        | PureOpKind::F64Nearest => ValType::F64,
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
        | PureOpKind::I32Xor
        | PureOpKind::I32Shl
        | PureOpKind::I32ShrS
        | PureOpKind::I32ShrU
        | PureOpKind::I32Rotl
        | PureOpKind::I32Rotr => ValType::I32,
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
        PureOpKind::I64Add
        | PureOpKind::I64Sub
        | PureOpKind::I64Mul
        | PureOpKind::I64And
        | PureOpKind::I64Or
        | PureOpKind::I64Xor
        | PureOpKind::I64Shl
        | PureOpKind::I64ShrS
        | PureOpKind::I64ShrU
        | PureOpKind::I64Rotl
        | PureOpKind::I64Rotr => ValType::I64,
        PureOpKind::I64Eq
        | PureOpKind::I64Ne
        | PureOpKind::I64LtS
        | PureOpKind::I64LtU
        | PureOpKind::I64GtS
        | PureOpKind::I64GtU
        | PureOpKind::I64LeS
        | PureOpKind::I64LeU
        | PureOpKind::I64GeS
        | PureOpKind::I64GeU => ValType::I32,
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
        (PureOpKind::I32Clz, ConstValue::I32(value)) => {
            Some(ConstValue::I32(value.leading_zeros() as i32))
        }
        (PureOpKind::I32Ctz, ConstValue::I32(value)) => {
            Some(ConstValue::I32(value.trailing_zeros() as i32))
        }
        (PureOpKind::I32Popcnt, ConstValue::I32(value)) => {
            Some(ConstValue::I32(value.count_ones() as i32))
        }
        (PureOpKind::I64Eqz, ConstValue::I64(value)) => Some(ConstValue::I32((value == 0) as i32)),
        (PureOpKind::I64Clz, ConstValue::I64(value)) => {
            Some(ConstValue::I64(value.leading_zeros() as i64))
        }
        (PureOpKind::I64Ctz, ConstValue::I64(value)) => {
            Some(ConstValue::I64(value.trailing_zeros() as i64))
        }
        (PureOpKind::I64Popcnt, ConstValue::I64(value)) => {
            Some(ConstValue::I64(value.count_ones() as i64))
        }
        (PureOpKind::F32Abs, ConstValue::F32(value)) => Some(ConstValue::F32(value.abs())),
        (PureOpKind::F32Neg, ConstValue::F32(value)) => Some(ConstValue::F32(-value)),
        (PureOpKind::F32Sqrt, ConstValue::F32(value)) => Some(ConstValue::F32(value.sqrt())),
        (PureOpKind::F32Ceil, ConstValue::F32(value)) => Some(ConstValue::F32(value.ceil())),
        (PureOpKind::F32Floor, ConstValue::F32(value)) => Some(ConstValue::F32(value.floor())),
        (PureOpKind::F32Trunc, ConstValue::F32(value)) => Some(ConstValue::F32(value.trunc())),
        (PureOpKind::F32Nearest, ConstValue::F32(value)) => {
            Some(ConstValue::F32(value.round_ties_even()))
        }
        (PureOpKind::F64Abs, ConstValue::F64(value)) => Some(ConstValue::F64(value.abs())),
        (PureOpKind::F64Neg, ConstValue::F64(value)) => Some(ConstValue::F64(-value)),
        (PureOpKind::F64Sqrt, ConstValue::F64(value)) => Some(ConstValue::F64(value.sqrt())),
        (PureOpKind::F64Ceil, ConstValue::F64(value)) => Some(ConstValue::F64(value.ceil())),
        (PureOpKind::F64Floor, ConstValue::F64(value)) => Some(ConstValue::F64(value.floor())),
        (PureOpKind::F64Trunc, ConstValue::F64(value)) => Some(ConstValue::F64(value.trunc())),
        (PureOpKind::F64Nearest, ConstValue::F64(value)) => {
            Some(ConstValue::F64(value.round_ties_even()))
        }
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
        (PureOpKind::I32Shl, ConstValue::I32(lhs), ConstValue::I32(rhs)) => {
            Some(ConstValue::I32(lhs.wrapping_shl(rhs as u32)))
        }
        (PureOpKind::I32ShrS, ConstValue::I32(lhs), ConstValue::I32(rhs)) => {
            Some(ConstValue::I32(lhs.wrapping_shr(rhs as u32)))
        }
        (PureOpKind::I32ShrU, ConstValue::I32(lhs), ConstValue::I32(rhs)) => Some(ConstValue::I32(
            ((lhs as u32).wrapping_shr(rhs as u32)) as i32,
        )),
        (PureOpKind::I32Rotl, ConstValue::I32(lhs), ConstValue::I32(rhs)) => {
            Some(ConstValue::I32(lhs.rotate_left(rhs as u32)))
        }
        (PureOpKind::I32Rotr, ConstValue::I32(lhs), ConstValue::I32(rhs)) => {
            Some(ConstValue::I32(lhs.rotate_right(rhs as u32)))
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
        (PureOpKind::I64Mul, ConstValue::I64(lhs), ConstValue::I64(rhs)) => {
            Some(ConstValue::I64(lhs.wrapping_mul(rhs)))
        }
        (PureOpKind::I64And, ConstValue::I64(lhs), ConstValue::I64(rhs)) => {
            Some(ConstValue::I64(lhs & rhs))
        }
        (PureOpKind::I64Or, ConstValue::I64(lhs), ConstValue::I64(rhs)) => {
            Some(ConstValue::I64(lhs | rhs))
        }
        (PureOpKind::I64Xor, ConstValue::I64(lhs), ConstValue::I64(rhs)) => {
            Some(ConstValue::I64(lhs ^ rhs))
        }
        (PureOpKind::I64Shl, ConstValue::I64(lhs), ConstValue::I64(rhs)) => {
            Some(ConstValue::I64(lhs.wrapping_shl(rhs as u32)))
        }
        (PureOpKind::I64ShrS, ConstValue::I64(lhs), ConstValue::I64(rhs)) => {
            Some(ConstValue::I64(lhs.wrapping_shr(rhs as u32)))
        }
        (PureOpKind::I64ShrU, ConstValue::I64(lhs), ConstValue::I64(rhs)) => Some(ConstValue::I64(
            ((lhs as u64).wrapping_shr(rhs as u32)) as i64,
        )),
        (PureOpKind::I64Rotl, ConstValue::I64(lhs), ConstValue::I64(rhs)) => {
            Some(ConstValue::I64(lhs.rotate_left(rhs as u32)))
        }
        (PureOpKind::I64Rotr, ConstValue::I64(lhs), ConstValue::I64(rhs)) => {
            Some(ConstValue::I64(lhs.rotate_right(rhs as u32)))
        }
        (PureOpKind::I64Eq, ConstValue::I64(lhs), ConstValue::I64(rhs)) => {
            Some(ConstValue::I32((lhs == rhs) as i32))
        }
        (PureOpKind::I64Ne, ConstValue::I64(lhs), ConstValue::I64(rhs)) => {
            Some(ConstValue::I32((lhs != rhs) as i32))
        }
        (PureOpKind::I64LtS, ConstValue::I64(lhs), ConstValue::I64(rhs)) => {
            Some(ConstValue::I32((lhs < rhs) as i32))
        }
        (PureOpKind::I64LtU, ConstValue::I64(lhs), ConstValue::I64(rhs)) => {
            Some(ConstValue::I32(((lhs as u64) < (rhs as u64)) as i32))
        }
        (PureOpKind::I64GtS, ConstValue::I64(lhs), ConstValue::I64(rhs)) => {
            Some(ConstValue::I32((lhs > rhs) as i32))
        }
        (PureOpKind::I64GtU, ConstValue::I64(lhs), ConstValue::I64(rhs)) => {
            Some(ConstValue::I32(((lhs as u64) > (rhs as u64)) as i32))
        }
        (PureOpKind::I64LeS, ConstValue::I64(lhs), ConstValue::I64(rhs)) => {
            Some(ConstValue::I32((lhs <= rhs) as i32))
        }
        (PureOpKind::I64LeU, ConstValue::I64(lhs), ConstValue::I64(rhs)) => {
            Some(ConstValue::I32(((lhs as u64) <= (rhs as u64)) as i32))
        }
        (PureOpKind::I64GeS, ConstValue::I64(lhs), ConstValue::I64(rhs)) => {
            Some(ConstValue::I32((lhs >= rhs) as i32))
        }
        (PureOpKind::I64GeU, ConstValue::I64(lhs), ConstValue::I64(rhs)) => {
            Some(ConstValue::I32(((lhs as u64) >= (rhs as u64)) as i32))
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
            | PureOpKind::I64Mul
            | PureOpKind::I64And
            | PureOpKind::I64Or
            | PureOpKind::I64Xor
            | PureOpKind::I64Eq
            | PureOpKind::I64Ne
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
        | (PureOpKind::I32Shl, _, Some(ConstValue::I32(0)))
        | (PureOpKind::I32ShrS, _, Some(ConstValue::I32(0)))
        | (PureOpKind::I32ShrU, _, Some(ConstValue::I32(0)))
        | (PureOpKind::I32Rotl, _, Some(ConstValue::I32(0)))
        | (PureOpKind::I32Rotr, _, Some(ConstValue::I32(0)))
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
        | (PureOpKind::I64Sub, _, Some(ConstValue::I64(0)))
        | (PureOpKind::I64Shl, _, Some(ConstValue::I64(0)))
        | (PureOpKind::I64ShrS, _, Some(ConstValue::I64(0)))
        | (PureOpKind::I64ShrU, _, Some(ConstValue::I64(0)))
        | (PureOpKind::I64Rotl, _, Some(ConstValue::I64(0)))
        | (PureOpKind::I64Rotr, _, Some(ConstValue::I64(0)))
        | (PureOpKind::I64Or, _, Some(ConstValue::I64(0)))
        | (PureOpKind::I64Xor, _, Some(ConstValue::I64(0))) => Some((lhs, rhs)),
        (PureOpKind::I64Add, Some(ConstValue::I64(0)), _)
        | (PureOpKind::I64Or, Some(ConstValue::I64(0)), _)
        | (PureOpKind::I64Xor, Some(ConstValue::I64(0)), _) => Some((rhs, lhs)),
        (PureOpKind::I64Mul, _, Some(ConstValue::I64(1)))
        | (PureOpKind::I64And, _, Some(ConstValue::I64(-1))) => Some((lhs, rhs)),
        (PureOpKind::I64Mul, Some(ConstValue::I64(1)), _)
        | (PureOpKind::I64And, Some(ConstValue::I64(-1)), _) => Some((rhs, lhs)),
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

    fn snapshot(types: &[ValType]) -> StackSnapshot {
        StackSnapshot {
            reachable: true,
            types: types.to_vec(),
        }
    }

    fn push_test_state(graph: &mut ValueGraph, state: ExprState) -> ValueRef {
        let value = ExprId(graph.nodes.len());
        graph.nodes.push(state);
        graph.nodes[value.0].refresh_optimizer_metadata();
        value
    }

    fn expect_specialized_local_control_consumer(
        graph: &ValueGraph,
        body: &BlockBody,
        op_idx: usize,
    ) -> SpecializedLocalControlLowering {
        let op = &body.ops[op_idx];
        build_specialized_local_set_tee_lowering(graph, body, op_idx, op)
            .or_else(|| build_specialized_local_root_lowering(graph, body, op_idx, op))
            .expect("consumer should specialize")
    }

    fn expect_specialized_br_if(
        graph: &ValueGraph,
        body: &BlockBody,
    ) -> SpecializedLocalControlLowering {
        let terminator = body
            .terminator
            .as_ref()
            .expect("body should have terminator");
        build_specialized_br_if_lowering(graph, body, terminator).expect("br_if should specialize")
    }

    #[test]
    fn merge_states_preserves_entry_local_memory_alias_across_join() {
        let first = DecodedInstr {
            old_start: 0,
            op: vm::op_end as Op,
            operands: Vec::new(),
            stack_before: empty_snapshot(),
            stack_after: empty_snapshot(),
            preserved_prefix_len: 0,
            fresh_result_count: 0,
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
            offset: 0,
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
            offset: 0,
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
        let mut graph = ValueGraph::default();
        let lhs_value = ExprId(graph.nodes.len());
        graph.nodes.push(ExprState {
            ty: ValType::I32,
            origin: value_origin_lhs,
            def: ValueDef::Instr,
            const_value: Some(ConstValue::I32(42)),
            key: None,
            address_shape: None,
            loop_value_shape: None,
            slot_shape: None,
            provider_class: ProviderClass::None,
            materialization_cost: MaterializationCost::Unknown,
            producer_op: None,
            materialized_block: None,
            materialized_op: None,
            needs_spill: false,
            use_count: 0,
            ref_count: 0,
            removable: false,
        });
        lhs.aliases.insert(key_lhs, lhs_value);
        let mut rhs = BlockEntryState {
            reachable: true,
            heap: HeapVersion {
                memory: 1,
                global: 0,
                table: 0,
            },
            ..BlockEntryState::default()
        };
        let rhs_value = ExprId(graph.nodes.len());
        graph.nodes.push(ExprState {
            ty: ValType::I32,
            origin: value_origin_rhs,
            def: ValueDef::Instr,
            const_value: Some(ConstValue::I32(42)),
            key: None,
            address_shape: None,
            loop_value_shape: None,
            slot_shape: None,
            provider_class: ProviderClass::None,
            materialization_cost: MaterializationCost::Unknown,
            producer_op: None,
            materialized_block: None,
            materialized_op: None,
            needs_spill: false,
            use_count: 0,
            ref_count: 0,
            removable: false,
        });
        rhs.aliases.insert(key_rhs, rhs_value);

        let merged = merge_states(&mut graph, 7, &first, &[lhs, rhs]);
        let merged_key = AliasKey {
            space: AliasSpace::Memory,
            index: 0,
            offset: 0,
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
        assert_eq!(graph[merged_value.0].const_value, Some(ConstValue::I32(42)));
    }

    #[test]
    fn merge_states_drops_aliases_once_heap_version_widens_to_unknown() {
        let first = DecodedInstr {
            old_start: 0,
            op: vm::op_end as Op,
            operands: Vec::new(),
            stack_before: empty_snapshot(),
            stack_after: empty_snapshot(),
            preserved_prefix_len: 0,
            fresh_result_count: 0,
        };
        let key_lhs = AliasKey {
            space: AliasSpace::Memory,
            index: 0,
            offset: 0,
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
            offset: 0,
            width: 4,
            address: AliasAddress::Origin(ExprOrigin {
                block_id: 11,
                ordinal: 0,
                kind: ExprOriginKind::EntryLocal,
            }),
        };
        let mut graph = ValueGraph::default();
        let lhs_value = ExprId(graph.nodes.len());
        graph.nodes.push(ExprState {
            ty: ValType::I32,
            origin: ExprOrigin {
                block_id: 10,
                ordinal: 1,
                kind: ExprOriginKind::MemoryValue,
            },
            def: ValueDef::Instr,
            const_value: Some(ConstValue::I32(7)),
            key: None,
            address_shape: None,
            loop_value_shape: None,
            slot_shape: None,
            provider_class: ProviderClass::None,
            materialization_cost: MaterializationCost::Unknown,
            producer_op: None,
            materialized_block: None,
            materialized_op: None,
            needs_spill: false,
            use_count: 0,
            ref_count: 0,
            removable: false,
        });
        let rhs_value = ExprId(graph.nodes.len());
        graph.nodes.push(ExprState {
            ty: ValType::I32,
            origin: ExprOrigin {
                block_id: 11,
                ordinal: 1,
                kind: ExprOriginKind::MemoryValue,
            },
            def: ValueDef::Instr,
            const_value: Some(ConstValue::I32(7)),
            key: None,
            address_shape: None,
            loop_value_shape: None,
            slot_shape: None,
            provider_class: ProviderClass::None,
            materialization_cost: MaterializationCost::Unknown,
            producer_op: None,
            materialized_block: None,
            materialized_op: None,
            needs_spill: false,
            use_count: 0,
            ref_count: 0,
            removable: false,
        });
        let mut lhs = BlockEntryState {
            reachable: true,
            heap: HeapVersion {
                memory: 3,
                global: 0,
                table: 0,
            },
            ..BlockEntryState::default()
        };
        lhs.aliases.insert(key_lhs, lhs_value);
        let mut rhs = BlockEntryState {
            reachable: true,
            heap: HeapVersion {
                memory: UNKNOWN_HEAP_VERSION,
                global: 0,
                table: 0,
            },
            ..BlockEntryState::default()
        };
        rhs.aliases.insert(key_rhs, rhs_value);

        let merged = merge_states(&mut graph, 7, &first, &[lhs, rhs]);
        assert_eq!(merged.heap.memory, UNKNOWN_HEAP_VERSION);
        assert!(
            merged.aliases.is_empty(),
            "must-prove aliases must not survive once heap version widens to unknown"
        );
    }

    #[test]
    fn merge_states_creates_first_class_block_argument_values() {
        let first = DecodedInstr {
            old_start: 0,
            op: vm::op_end as Op,
            operands: Vec::new(),
            stack_before: snapshot(&[ValType::I32]),
            stack_after: empty_snapshot(),
            preserved_prefix_len: 0,
            fresh_result_count: 0,
        };
        let mut graph = ValueGraph::default();
        let lhs_stack = ExprId(graph.nodes.len());
        graph.nodes.push(ExprState {
            ty: ValType::I32,
            origin: ExprOrigin {
                block_id: 10,
                ordinal: 0,
                kind: ExprOriginKind::EntryStack,
            },
            def: ValueDef::Synthetic,
            const_value: Some(ConstValue::I32(1)),
            key: None,
            address_shape: None,
            loop_value_shape: None,
            slot_shape: None,
            provider_class: ProviderClass::None,
            materialization_cost: MaterializationCost::Unknown,
            producer_op: None,
            materialized_block: None,
            materialized_op: None,
            needs_spill: false,
            use_count: 0,
            ref_count: 0,
            removable: false,
        });
        let rhs_stack = ExprId(graph.nodes.len());
        graph.nodes.push(ExprState {
            ty: ValType::I32,
            origin: ExprOrigin {
                block_id: 11,
                ordinal: 0,
                kind: ExprOriginKind::EntryStack,
            },
            def: ValueDef::Synthetic,
            const_value: Some(ConstValue::I32(2)),
            key: None,
            address_shape: None,
            loop_value_shape: None,
            slot_shape: None,
            provider_class: ProviderClass::None,
            materialization_cost: MaterializationCost::Unknown,
            producer_op: None,
            materialized_block: None,
            materialized_op: None,
            needs_spill: false,
            use_count: 0,
            ref_count: 0,
            removable: false,
        });

        let lhs = BlockEntryState {
            reachable: true,
            stack: vec![lhs_stack],
            ..BlockEntryState::default()
        };
        let rhs = BlockEntryState {
            reachable: true,
            stack: vec![rhs_stack],
            ..BlockEntryState::default()
        };

        let merged = merge_states(&mut graph, 7, &first, &[lhs, rhs]);
        let merged_stack = merged.stack[0];
        assert!(graph[merged_stack.0].is_block_argument());
        assert_eq!(graph.block_arguments.len(), 1);
        let block_argument = graph
            .block_argument(
                graph[merged_stack.0]
                    .block_argument()
                    .expect("block argument id"),
            )
            .expect("block argument");
        assert_eq!(block_argument.block_id, 7);
        assert_eq!(block_argument.ordinal, 0);
    }

    #[test]
    fn merge_states_keeps_existing_block_argument_when_joining_block_arguments() {
        let first = DecodedInstr {
            old_start: 0,
            op: vm::op_end as Op,
            operands: Vec::new(),
            stack_before: empty_snapshot(),
            stack_after: empty_snapshot(),
            preserved_prefix_len: 0,
            fresh_result_count: 0,
        };
        let slot = LocalSlot::new(8, 4);
        let mut graph = ValueGraph::default();
        let header_value = graph.ensure_block_argument(
            7,
            1024 + slot.addr as usize,
            ValType::I32,
            None,
            None,
            None,
            None,
            None,
        );
        let pred_value = graph.ensure_block_argument(
            3,
            1024 + slot.addr as usize,
            ValType::I32,
            None,
            None,
            None,
            None,
            None,
        );

        let mut lhs = BlockEntryState {
            reachable: true,
            ..BlockEntryState::default()
        };
        lhs.locals.insert(slot, pred_value);

        let mut rhs = BlockEntryState {
            reachable: true,
            ..BlockEntryState::default()
        };
        rhs.locals.insert(slot, pred_value);

        let merged = merge_states(&mut graph, 7, &first, &[lhs, rhs]);
        assert_eq!(merged.locals[&slot], header_value);
        assert!(graph[merged.locals[&slot].0].is_block_argument());
        assert_eq!(graph[merged.locals[&slot].0].origin.block_id, 7);
        assert_eq!(graph[merged.locals[&slot].0].key, None);
    }

    #[test]
    fn merge_states_existing_block_argument_drops_const_and_key_from_block_argument_inputs() {
        let first = DecodedInstr {
            old_start: 0,
            op: vm::op_end as Op,
            operands: Vec::new(),
            stack_before: empty_snapshot(),
            stack_after: empty_snapshot(),
            preserved_prefix_len: 0,
            fresh_result_count: 0,
        };
        let slot = LocalSlot::new(12, 4);
        let mut graph = ValueGraph::default();
        let header_value = graph.ensure_block_argument(
            7,
            1024 + slot.addr as usize,
            ValType::I32,
            None,
            None,
            None,
            None,
            None,
        );
        let pred_origin = ExprOrigin {
            block_id: 3,
            ordinal: 1024 + slot.addr as usize,
            kind: ExprOriginKind::BlockArgument,
        };
        let pred_value = graph.ensure_block_argument(
            3,
            1024 + slot.addr as usize,
            ValType::I32,
            Some(ConstValue::I32(7)),
            Some(ValueKey::Binary {
                op: PureOpKind::I32Add,
                lhs: pred_origin,
                rhs: ExprOrigin {
                    block_id: 3,
                    ordinal: 99,
                    kind: ExprOriginKind::SyntheticConst,
                },
            }),
            None,
            None,
            None,
        );

        let mut lhs = BlockEntryState {
            reachable: true,
            ..BlockEntryState::default()
        };
        lhs.locals.insert(slot, pred_value);

        let mut rhs = BlockEntryState {
            reachable: true,
            ..BlockEntryState::default()
        };
        rhs.locals.insert(slot, pred_value);

        let merged = merge_states(&mut graph, 7, &first, &[lhs, rhs]);
        assert_eq!(merged.locals[&slot], header_value);
        assert_eq!(graph[header_value.0].const_value, None);
        assert_eq!(graph[header_value.0].key, None);
    }

    #[test]
    fn merge_states_keeps_existing_block_argument_across_multi_pred_same_value() {
        let first = DecodedInstr {
            old_start: 0,
            op: vm::op_end as Op,
            operands: Vec::new(),
            stack_before: empty_snapshot(),
            stack_after: empty_snapshot(),
            preserved_prefix_len: 0,
            fresh_result_count: 0,
        };
        let slot = LocalSlot::new(40, 4);
        let mut graph = ValueGraph::default();
        let header_value = graph.ensure_block_argument(
            7,
            1024 + slot.addr as usize,
            ValType::I32,
            None,
            None,
            None,
            None,
            None,
        );
        let shared_value = ExprId(graph.nodes.len());
        graph.nodes.push(ExprState {
            ty: ValType::I32,
            origin: ExprOrigin {
                block_id: 19,
                ordinal: 6144,
                kind: ExprOriginKind::InstrResult,
            },
            def: ValueDef::Instr,
            const_value: None,
            key: Some(ValueKey::Binary {
                op: PureOpKind::I32And,
                lhs: ExprOrigin {
                    block_id: 13,
                    ordinal: 1088,
                    kind: ExprOriginKind::BlockArgument,
                },
                rhs: ExprOrigin {
                    block_id: 19,
                    ordinal: 23,
                    kind: ExprOriginKind::SyntheticConst,
                },
            }),
            address_shape: None,
            loop_value_shape: None,
            slot_shape: None,
            provider_class: ProviderClass::None,
            materialization_cost: MaterializationCost::Unknown,
            producer_op: None,
            materialized_block: None,
            materialized_op: None,
            needs_spill: false,
            use_count: 0,
            ref_count: 0,
            removable: false,
        });

        let mut lhs = BlockEntryState {
            reachable: true,
            ..BlockEntryState::default()
        };
        lhs.locals.insert(slot, shared_value);

        let mut rhs = BlockEntryState {
            reachable: true,
            ..BlockEntryState::default()
        };
        rhs.locals.insert(slot, shared_value);

        let merged = merge_states(&mut graph, 7, &first, &[lhs, rhs]);
        assert_eq!(merged.locals[&slot], header_value);
        assert!(graph[merged.locals[&slot].0].is_block_argument());
        assert_eq!(graph[merged.locals[&slot].0].origin.block_id, 7);
    }

    #[test]
    fn merge_states_preserves_identical_shapes_on_block_argument_join() {
        let first = DecodedInstr {
            old_start: 0,
            op: vm::op_end as Op,
            operands: Vec::new(),
            stack_before: empty_snapshot(),
            stack_after: empty_snapshot(),
            preserved_prefix_len: 0,
            fresh_result_count: 0,
        };
        let slot = LocalSlot::new(8, 4);
        let shape = AddressShape {
            base: AddressBaseKind::EntryLocal(slot),
            offset_delta: 4,
        };
        let loop_shape = LoopValueShape::Local4ConstAdd { base: slot, imm: 4 };
        let mut graph = ValueGraph::default();
        let lhs_value = ExprId(graph.nodes.len());
        graph.nodes.push(ExprState {
            ty: ValType::I32,
            origin: ExprOrigin {
                block_id: 10,
                ordinal: 1,
                kind: ExprOriginKind::InstrResult,
            },
            def: ValueDef::Instr,
            const_value: None,
            key: None,
            address_shape: Some(shape),
            loop_value_shape: Some(loop_shape.clone()),
            slot_shape: None,
            provider_class: ProviderClass::None,
            materialization_cost: MaterializationCost::Unknown,
            producer_op: None,
            materialized_block: None,
            materialized_op: None,
            needs_spill: false,
            use_count: 0,
            ref_count: 0,
            removable: false,
        });
        let rhs_value = ExprId(graph.nodes.len());
        graph.nodes.push(ExprState {
            ty: ValType::I32,
            origin: ExprOrigin {
                block_id: 11,
                ordinal: 1,
                kind: ExprOriginKind::InstrResult,
            },
            def: ValueDef::Instr,
            const_value: None,
            key: None,
            address_shape: Some(shape),
            loop_value_shape: Some(loop_shape.clone()),
            slot_shape: None,
            provider_class: ProviderClass::None,
            materialization_cost: MaterializationCost::Unknown,
            producer_op: None,
            materialized_block: None,
            materialized_op: None,
            needs_spill: false,
            use_count: 0,
            ref_count: 0,
            removable: false,
        });

        let mut lhs = BlockEntryState {
            reachable: true,
            ..BlockEntryState::default()
        };
        lhs.locals.insert(slot, lhs_value);
        let mut rhs = BlockEntryState {
            reachable: true,
            ..BlockEntryState::default()
        };
        rhs.locals.insert(slot, rhs_value);

        let merged = merge_states(&mut graph, 7, &first, &[lhs, rhs]);
        let merged_value = merged.locals[&slot];
        assert!(graph[merged_value.0].is_block_argument());
        assert_eq!(graph[merged_value.0].address_shape, Some(shape));
        assert_eq!(graph[merged_value.0].loop_value_shape, Some(loop_shape));
    }

    #[test]
    fn merge_states_drops_shapes_when_join_inputs_disagree() {
        let first = DecodedInstr {
            old_start: 0,
            op: vm::op_end as Op,
            operands: Vec::new(),
            stack_before: empty_snapshot(),
            stack_after: empty_snapshot(),
            preserved_prefix_len: 0,
            fresh_result_count: 0,
        };
        let slot = LocalSlot::new(8, 4);
        let mut graph = ValueGraph::default();
        let lhs_value = ExprId(graph.nodes.len());
        graph.nodes.push(ExprState {
            ty: ValType::I32,
            origin: ExprOrigin {
                block_id: 10,
                ordinal: 1,
                kind: ExprOriginKind::InstrResult,
            },
            def: ValueDef::Instr,
            const_value: None,
            key: None,
            address_shape: Some(AddressShape {
                base: AddressBaseKind::EntryLocal(slot),
                offset_delta: 4,
            }),
            loop_value_shape: Some(LoopValueShape::Local4ConstAdd { base: slot, imm: 4 }),
            slot_shape: None,
            provider_class: ProviderClass::None,
            materialization_cost: MaterializationCost::Unknown,
            producer_op: None,
            materialized_block: None,
            materialized_op: None,
            needs_spill: false,
            use_count: 0,
            ref_count: 0,
            removable: false,
        });
        let rhs_value = ExprId(graph.nodes.len());
        graph.nodes.push(ExprState {
            ty: ValType::I32,
            origin: ExprOrigin {
                block_id: 11,
                ordinal: 1,
                kind: ExprOriginKind::InstrResult,
            },
            def: ValueDef::Instr,
            const_value: None,
            key: None,
            address_shape: Some(AddressShape {
                base: AddressBaseKind::EntryLocal(slot),
                offset_delta: 8,
            }),
            loop_value_shape: Some(LoopValueShape::Local4ConstAdd { base: slot, imm: 8 }),
            slot_shape: None,
            provider_class: ProviderClass::None,
            materialization_cost: MaterializationCost::Unknown,
            producer_op: None,
            materialized_block: None,
            materialized_op: None,
            needs_spill: false,
            use_count: 0,
            ref_count: 0,
            removable: false,
        });

        let mut lhs = BlockEntryState {
            reachable: true,
            ..BlockEntryState::default()
        };
        lhs.locals.insert(slot, lhs_value);
        let mut rhs = BlockEntryState {
            reachable: true,
            ..BlockEntryState::default()
        };
        rhs.locals.insert(slot, rhs_value);

        let merged = merge_states(&mut graph, 7, &first, &[lhs, rhs]);
        let merged_value = merged.locals[&slot];
        assert!(graph[merged_value.0].is_block_argument());
        assert_eq!(graph[merged_value.0].address_shape, None);
        assert_eq!(graph[merged_value.0].loop_value_shape, None);
    }

    #[test]
    fn merge_states_marks_stack_slot_join_as_preserve_when_slot_shape_matches() {
        let first = DecodedInstr {
            old_start: 0,
            op: vm::op_end as Op,
            operands: Vec::new(),
            stack_before: snapshot(&[ValType::I32]),
            stack_after: empty_snapshot(),
            preserved_prefix_len: 0,
            fresh_result_count: 0,
        };
        let slot = LocalSlot::new(12, 4);
        let slot_shape = build_slot_shape(
            Some(SlotRef::entry_local(slot)),
            Some(AddressShape {
                base: AddressBaseKind::EntryLocal(slot),
                offset_delta: 0,
            }),
            Some(LoopValueShape::Local4(slot)),
        );
        let mut graph = ValueGraph::default();
        let lhs_value = ExprId(graph.nodes.len());
        graph.nodes.push(ExprState {
            ty: ValType::I32,
            origin: ExprOrigin {
                block_id: 1,
                ordinal: 0,
                kind: ExprOriginKind::InstrResult,
            },
            def: ValueDef::Instr,
            const_value: None,
            key: None,
            address_shape: Some(AddressShape {
                base: AddressBaseKind::EntryLocal(slot),
                offset_delta: 0,
            }),
            loop_value_shape: Some(LoopValueShape::Local4(slot)),
            slot_shape: slot_shape.clone(),
            provider_class: ProviderClass::LocalLoad,
            materialization_cost: MaterializationCost::Local,
            producer_op: None,
            materialized_block: None,
            materialized_op: None,
            needs_spill: false,
            use_count: 0,
            ref_count: 0,
            removable: false,
        });
        let rhs_value = ExprId(graph.nodes.len());
        graph.nodes.push(ExprState {
            ty: ValType::I32,
            origin: ExprOrigin {
                block_id: 2,
                ordinal: 0,
                kind: ExprOriginKind::InstrResult,
            },
            def: ValueDef::Instr,
            const_value: None,
            key: None,
            address_shape: Some(AddressShape {
                base: AddressBaseKind::EntryLocal(slot),
                offset_delta: 0,
            }),
            loop_value_shape: Some(LoopValueShape::Local4(slot)),
            slot_shape,
            provider_class: ProviderClass::LocalLoad,
            materialization_cost: MaterializationCost::Local,
            producer_op: None,
            materialized_block: None,
            materialized_op: None,
            needs_spill: false,
            use_count: 0,
            ref_count: 0,
            removable: false,
        });
        let lhs = BlockEntryState {
            reachable: true,
            stack: vec![lhs_value],
            ..BlockEntryState::default()
        };
        let rhs = BlockEntryState {
            reachable: true,
            stack: vec![rhs_value],
            ..BlockEntryState::default()
        };

        let (merged, copy_plan) = merge_states_with_copy_plan(&mut graph, 7, &first, &[lhs, rhs]);
        let merged_value = merged.stack[0];
        assert!(graph[merged_value.0].is_block_argument());
        assert_eq!(copy_plan.stack.get(&0), Some(&SlotMergeDecision::Preserve));
        assert_eq!(
            graph[merged_value.0]
                .slot_shape
                .as_ref()
                .and_then(|shape| shape.slot),
            Some(SlotRef::entry_local(slot))
        );
    }

    #[test]
    fn merge_states_marks_stack_slot_join_for_copy_when_slot_shape_disagrees() {
        let first = DecodedInstr {
            old_start: 0,
            op: vm::op_end as Op,
            operands: Vec::new(),
            stack_before: snapshot(&[ValType::I32]),
            stack_after: empty_snapshot(),
            preserved_prefix_len: 0,
            fresh_result_count: 0,
        };
        let slot0 = LocalSlot::new(12, 4);
        let slot1 = LocalSlot::new(20, 4);
        let mut graph = ValueGraph::default();
        let lhs_value = ExprId(graph.nodes.len());
        graph.nodes.push(ExprState {
            ty: ValType::I32,
            origin: ExprOrigin {
                block_id: 1,
                ordinal: 0,
                kind: ExprOriginKind::InstrResult,
            },
            def: ValueDef::Instr,
            const_value: None,
            key: None,
            address_shape: Some(AddressShape {
                base: AddressBaseKind::EntryLocal(slot0),
                offset_delta: 0,
            }),
            loop_value_shape: Some(LoopValueShape::Local4(slot0)),
            slot_shape: build_slot_shape(
                Some(SlotRef::entry_local(slot0)),
                Some(AddressShape {
                    base: AddressBaseKind::EntryLocal(slot0),
                    offset_delta: 0,
                }),
                Some(LoopValueShape::Local4(slot0)),
            ),
            provider_class: ProviderClass::LocalLoad,
            materialization_cost: MaterializationCost::Local,
            producer_op: None,
            materialized_block: None,
            materialized_op: None,
            needs_spill: false,
            use_count: 0,
            ref_count: 0,
            removable: false,
        });
        let rhs_value = ExprId(graph.nodes.len());
        graph.nodes.push(ExprState {
            ty: ValType::I32,
            origin: ExprOrigin {
                block_id: 2,
                ordinal: 0,
                kind: ExprOriginKind::InstrResult,
            },
            def: ValueDef::Instr,
            const_value: None,
            key: None,
            address_shape: Some(AddressShape {
                base: AddressBaseKind::EntryLocal(slot1),
                offset_delta: 0,
            }),
            loop_value_shape: Some(LoopValueShape::Local4(slot1)),
            slot_shape: build_slot_shape(
                Some(SlotRef::entry_local(slot1)),
                Some(AddressShape {
                    base: AddressBaseKind::EntryLocal(slot1),
                    offset_delta: 0,
                }),
                Some(LoopValueShape::Local4(slot1)),
            ),
            provider_class: ProviderClass::LocalLoad,
            materialization_cost: MaterializationCost::Local,
            producer_op: None,
            materialized_block: None,
            materialized_op: None,
            needs_spill: false,
            use_count: 0,
            ref_count: 0,
            removable: false,
        });
        let lhs = BlockEntryState {
            reachable: true,
            stack: vec![lhs_value],
            ..BlockEntryState::default()
        };
        let rhs = BlockEntryState {
            reachable: true,
            stack: vec![rhs_value],
            ..BlockEntryState::default()
        };

        let (merged, copy_plan) = merge_states_with_copy_plan(&mut graph, 7, &first, &[lhs, rhs]);
        let merged_value = merged.stack[0];
        assert!(graph[merged_value.0].is_block_argument());
        assert_eq!(
            copy_plan.stack.get(&0),
            Some(&SlotMergeDecision::InsertCopy)
        );
        assert_eq!(graph[merged_value.0].slot_shape, None);
    }

    #[test]
    fn try_materialize_value_uses_slot_preserving_block_argument() {
        let slot = LocalSlot::new(16, 4);
        let mut graph = ValueGraph::default();
        let value = graph.ensure_block_argument(
            7,
            0,
            ValType::I32,
            None,
            None,
            Some(AddressShape {
                base: AddressBaseKind::EntryLocal(slot),
                offset_delta: 0,
            }),
            Some(LoopValueShape::Local4(slot)),
            build_slot_shape(
                Some(SlotRef::entry_local(slot)),
                Some(AddressShape {
                    base: AddressBaseKind::EntryLocal(slot),
                    offset_delta: 0,
                }),
                Some(LoopValueShape::Local4(slot)),
            ),
        );
        let entry = BlockEntryState {
            reachable: true,
            stack: vec![value],
            ..BlockEntryState::default()
        };
        let mut optimizer = BlockOptimizer {
            exprs: graph,
            current_copy_plan: BlockCopyPlan::default(),
            ..BlockOptimizer::default()
        };
        optimizer
            .current_copy_plan
            .locals
            .insert(slot, SlotMergeDecision::Preserve);
        optimizer.reset(
            BasicBlock {
                id: 7,
                start: 0,
                end: 0,
            },
            &entry,
        );
        let materialized = optimizer
            .try_materialize_value(13, value)
            .expect("slot-preserving block argument should materialize");
        let body = optimizer.build_block_body();
        assert_eq!(optimizer.exprs[materialized.0].origin.block_id, 7);
        assert_eq!(body.ops.len(), 1);
        assert!(std::ptr::fn_addr_eq(
            body.ops[0].op,
            vm::op_local_get4 as Op
        ));
        assert!(matches!(
            body.ops[0].operands.as_slice(),
            [BlockOperand::LocalAddr(16)]
        ));
    }

    #[test]
    fn try_materialize_value_uses_canonical_slot_for_block_argument_origin() {
        let canonical_slot = LocalSlot::new(16, 4);
        let stale_slot = LocalSlot::new(32, 4);
        let mut graph = ValueGraph::default();
        let value = graph.ensure_block_argument(
            7,
            0,
            ValType::I32,
            None,
            None,
            Some(AddressShape {
                base: AddressBaseKind::EntryLocal(canonical_slot),
                offset_delta: 0,
            }),
            Some(LoopValueShape::Local4(canonical_slot)),
            build_slot_shape(
                Some(SlotRef::entry_local(canonical_slot)),
                Some(AddressShape {
                    base: AddressBaseKind::EntryLocal(canonical_slot),
                    offset_delta: 0,
                }),
                Some(LoopValueShape::Local4(canonical_slot)),
            ),
        );
        let entry = BlockEntryState {
            reachable: true,
            stack: vec![value],
            ..BlockEntryState::default()
        };
        let mut optimizer = BlockOptimizer {
            exprs: graph,
            ..BlockOptimizer::default()
        };
        optimizer.reset(
            BasicBlock {
                id: 7,
                start: 0,
                end: 0,
            },
            &entry,
        );
        optimizer
            .origin_locals
            .insert(optimizer.exprs[value.0].origin, stale_slot);

        let materialized = optimizer
            .try_materialize_value(13, value)
            .expect("slot-preserving block argument should materialize");
        let body = optimizer.build_block_body();
        assert_eq!(optimizer.exprs[materialized.0].origin.block_id, 7);
        assert_eq!(body.ops.len(), 1);
        assert!(std::ptr::fn_addr_eq(
            body.ops[0].op,
            vm::op_local_get4 as Op
        ));
        assert!(matches!(
            body.ops[0].operands.as_slice(),
            [BlockOperand::LocalAddr(16)]
        ));
    }

    #[test]
    fn try_materialize_value_does_not_const_rematerialize_block_argument() {
        let slot = LocalSlot::new(24, 4);
        let mut graph = ValueGraph::default();
        let value = graph.ensure_block_argument(
            7,
            0,
            ValType::I32,
            Some(ConstValue::I32(2000)),
            None,
            Some(AddressShape {
                base: AddressBaseKind::EntryLocal(slot),
                offset_delta: 0,
            }),
            Some(LoopValueShape::Local4(slot)),
            build_slot_shape(
                Some(SlotRef::entry_local(slot)),
                Some(AddressShape {
                    base: AddressBaseKind::EntryLocal(slot),
                    offset_delta: 0,
                }),
                Some(LoopValueShape::Local4(slot)),
            ),
        );
        let entry = BlockEntryState {
            reachable: true,
            stack: vec![value],
            ..BlockEntryState::default()
        };
        let mut optimizer = BlockOptimizer {
            exprs: graph,
            ..BlockOptimizer::default()
        };
        optimizer.reset(
            BasicBlock {
                id: 7,
                start: 0,
                end: 0,
            },
            &entry,
        );

        let materialized = optimizer
            .try_materialize_value(13, value)
            .expect("slot-preserving block argument should materialize via local slot");
        let body = optimizer.build_block_body();
        assert_eq!(
            optimizer.exprs[materialized.0].const_value,
            Some(ConstValue::I32(2000))
        );
        assert_eq!(body.ops.len(), 1);
        assert!(std::ptr::fn_addr_eq(
            body.ops[0].op,
            vm::op_local_get4 as Op
        ));
        assert!(matches!(
            body.ops[0].operands.as_slice(),
            [BlockOperand::LocalAddr(24)]
        ));
    }

    #[test]
    fn try_materialize_value_rejects_non_slot_preserving_block_argument() {
        let mut graph = ValueGraph::default();
        let value = graph.ensure_block_argument(7, 0, ValType::I32, None, None, None, None, None);
        let entry = BlockEntryState {
            reachable: true,
            stack: vec![value],
            ..BlockEntryState::default()
        };
        let mut optimizer = BlockOptimizer {
            exprs: graph,
            ..BlockOptimizer::default()
        };
        optimizer.reset(
            BasicBlock {
                id: 7,
                start: 0,
                end: 0,
            },
            &entry,
        );
        assert!(optimizer.try_materialize_value(13, value).is_none());
    }

    #[test]
    fn try_materialize_value_rejects_non_block_argument_local_load() {
        let slot = LocalSlot::new(16, 4);
        let mut graph = ValueGraph::default();
        let value = ExprId(graph.nodes.len());
        graph.nodes.push(ExprState {
            ty: ValType::I32,
            origin: ExprOrigin {
                block_id: 7,
                ordinal: 0,
                kind: ExprOriginKind::EntryLocal,
            },
            def: ValueDef::Synthetic,
            const_value: None,
            key: None,
            address_shape: Some(AddressShape {
                base: AddressBaseKind::EntryLocal(slot),
                offset_delta: 0,
            }),
            loop_value_shape: Some(LoopValueShape::Local4(slot)),
            slot_shape: build_slot_shape(
                Some(SlotRef::entry_local(slot)),
                Some(AddressShape {
                    base: AddressBaseKind::EntryLocal(slot),
                    offset_delta: 0,
                }),
                Some(LoopValueShape::Local4(slot)),
            ),
            provider_class: ProviderClass::LocalLoad,
            materialization_cost: MaterializationCost::Local,
            producer_op: Some(0),
            materialized_block: Some(7),
            materialized_op: Some(0),
            needs_spill: false,
            use_count: 0,
            ref_count: 0,
            removable: true,
        });
        let entry = BlockEntryState {
            reachable: true,
            stack: vec![value],
            ..BlockEntryState::default()
        };
        let mut optimizer = BlockOptimizer {
            exprs: graph,
            ..BlockOptimizer::default()
        };
        optimizer.reset(
            BasicBlock {
                id: 7,
                start: 0,
                end: 0,
            },
            &entry,
        );
        assert!(optimizer.try_materialize_value(13, value).is_none());
    }

    #[test]
    fn reset_skips_direct_local_binding_for_insert_copy_block_argument() {
        let slot = LocalSlot::new(12, 4);
        let mut graph = ValueGraph::default();
        let value = graph.ensure_block_argument(
            7,
            1024 + slot.addr as usize,
            ValType::I32,
            None,
            None,
            None,
            None,
            None,
        );
        let mut entry = BlockEntryState {
            reachable: true,
            ..BlockEntryState::default()
        };
        entry.locals.insert(slot, value);

        let mut optimizer = BlockOptimizer {
            exprs: graph,
            current_copy_plan: BlockCopyPlan::default(),
            ..BlockOptimizer::default()
        };
        optimizer
            .current_copy_plan
            .locals
            .insert(slot, SlotMergeDecision::InsertCopy);
        optimizer.reset(
            BasicBlock {
                id: 7,
                start: 0,
                end: 0,
            },
            &entry,
        );
        assert!(!optimizer.locals.contains_key(&slot));
    }

    #[test]
    fn local_control_relower_specializes_plain_add_at_consumer() {
        let src = LocalSlot::new(0, 4);
        let mut graph = ValueGraph::default();
        let base = ExprId(graph.nodes.len());
        graph.nodes.push(ExprState {
            ty: ValType::I32,
            origin: ExprOrigin {
                block_id: 1,
                ordinal: 0,
                kind: ExprOriginKind::InstrResult,
            },
            def: ValueDef::Instr,
            const_value: None,
            key: None,
            address_shape: Some(AddressShape {
                base: AddressBaseKind::EntryLocal(src),
                offset_delta: 0,
            }),
            loop_value_shape: Some(LoopValueShape::Local4(src)),
            slot_shape: build_slot_shape(
                Some(SlotRef::entry_local(src)),
                Some(AddressShape {
                    base: AddressBaseKind::EntryLocal(src),
                    offset_delta: 0,
                }),
                Some(LoopValueShape::Local4(src)),
            ),
            provider_class: ProviderClass::LocalLoad,
            materialization_cost: MaterializationCost::Local,
            producer_op: Some(0),
            materialized_block: Some(1),
            materialized_op: Some(0),
            needs_spill: false,
            use_count: 1,
            ref_count: 0,
            removable: true,
        });
        let imm = ExprId(graph.nodes.len());
        graph.nodes.push(ExprState {
            ty: ValType::I32,
            origin: ExprOrigin {
                block_id: 1,
                ordinal: 1,
                kind: ExprOriginKind::InstrResult,
            },
            def: ValueDef::Const,
            const_value: Some(ConstValue::I32(7)),
            key: None,
            address_shape: None,
            loop_value_shape: None,
            slot_shape: None,
            provider_class: ProviderClass::Const,
            materialization_cost: MaterializationCost::Immediate,
            producer_op: Some(1),
            materialized_block: Some(1),
            materialized_op: Some(1),
            needs_spill: false,
            use_count: 1,
            ref_count: 0,
            removable: true,
        });
        let add = ExprId(graph.nodes.len());
        graph.nodes.push(ExprState {
            ty: ValType::I32,
            origin: ExprOrigin {
                block_id: 1,
                ordinal: 2,
                kind: ExprOriginKind::InstrResult,
            },
            def: ValueDef::Instr,
            const_value: None,
            key: Some(ValueKey::Binary {
                op: PureOpKind::I32Add,
                lhs: graph[base.0].origin,
                rhs: graph[imm.0].origin,
            }),
            address_shape: Some(AddressShape {
                base: AddressBaseKind::EntryLocal(src),
                offset_delta: 7,
            }),
            loop_value_shape: Some(LoopValueShape::Local4ConstAdd { base: src, imm: 7 }),
            slot_shape: build_slot_shape(
                Some(SlotRef::entry_local(src)),
                Some(AddressShape {
                    base: AddressBaseKind::EntryLocal(src),
                    offset_delta: 7,
                }),
                Some(LoopValueShape::Local4ConstAdd { base: src, imm: 7 }),
            ),
            provider_class: ProviderClass::PureBinary,
            materialization_cost: MaterializationCost::Pure,
            producer_op: Some(2),
            materialized_block: Some(1),
            materialized_op: Some(2),
            needs_spill: false,
            use_count: 1,
            ref_count: 0,
            removable: true,
        });

        let body = BlockBody {
            ops: vec![
                BlockOp {
                    source_start: Some(10),
                    op: vm::op_local_get4 as Op,
                    kind: BlockOpKind::LocalGet,
                    operands: vec![BlockOperand::LocalAddr(src.addr)],
                    inputs: Vec::new(),
                    values: vec![base],
                },
                BlockOp {
                    source_start: Some(12),
                    op: vm::op_i32_const as Op,
                    kind: BlockOpKind::Const,
                    operands: vec![BlockOperand::I32(7)],
                    inputs: Vec::new(),
                    values: vec![imm],
                },
                BlockOp {
                    source_start: Some(14),
                    op: vm::op_i32_add as Op,
                    kind: BlockOpKind::PureBinary(PureOpKind::I32Add),
                    operands: Vec::new(),
                    inputs: vec![base, imm],
                    values: vec![add],
                },
                BlockOp {
                    source_start: Some(16),
                    op: vm::op_drop as Op,
                    kind: BlockOpKind::Drop,
                    operands: Vec::new(),
                    inputs: vec![add],
                    values: Vec::new(),
                },
            ],
            terminator: None,
        };

        let spec = expect_specialized_local_control_consumer(&graph, &body, 2);
        assert!(std::ptr::fn_addr_eq(
            spec.op,
            vm::op_local_get4_i32_const_add as Op
        ));
        assert_eq!(spec.source_start, Some(10));
        assert_eq!(spec.absorbed_ops, BTreeSet::from([0usize, 1usize]));
        assert!(verify_specialized_local_control_lowering(&body, &spec));
    }

    #[test]
    fn local_control_relower_specializes_local_set_consumer() {
        let src = LocalSlot::new(0, 4);
        let dst = LocalSlot::new(4, 4);
        let mut graph = ValueGraph::default();
        let base = ExprId(graph.nodes.len());
        graph.nodes.push(ExprState {
            ty: ValType::I32,
            origin: ExprOrigin {
                block_id: 1,
                ordinal: 0,
                kind: ExprOriginKind::InstrResult,
            },
            def: ValueDef::Instr,
            const_value: None,
            key: None,
            address_shape: Some(AddressShape {
                base: AddressBaseKind::EntryLocal(src),
                offset_delta: 0,
            }),
            loop_value_shape: Some(LoopValueShape::Local4(src)),
            slot_shape: build_slot_shape(
                Some(SlotRef::entry_local(src)),
                Some(AddressShape {
                    base: AddressBaseKind::EntryLocal(src),
                    offset_delta: 0,
                }),
                Some(LoopValueShape::Local4(src)),
            ),
            provider_class: ProviderClass::LocalLoad,
            materialization_cost: MaterializationCost::Local,
            producer_op: Some(0),
            materialized_block: Some(1),
            materialized_op: Some(0),
            needs_spill: false,
            use_count: 1,
            ref_count: 0,
            removable: true,
        });
        let imm = ExprId(graph.nodes.len());
        graph.nodes.push(ExprState {
            ty: ValType::I32,
            origin: ExprOrigin {
                block_id: 1,
                ordinal: 1,
                kind: ExprOriginKind::InstrResult,
            },
            def: ValueDef::Const,
            const_value: Some(ConstValue::I32(5)),
            key: None,
            address_shape: None,
            loop_value_shape: None,
            slot_shape: None,
            provider_class: ProviderClass::Const,
            materialization_cost: MaterializationCost::Immediate,
            producer_op: Some(1),
            materialized_block: Some(1),
            materialized_op: Some(1),
            needs_spill: false,
            use_count: 1,
            ref_count: 0,
            removable: true,
        });
        let add = ExprId(graph.nodes.len());
        graph.nodes.push(ExprState {
            ty: ValType::I32,
            origin: ExprOrigin {
                block_id: 1,
                ordinal: 2,
                kind: ExprOriginKind::InstrResult,
            },
            def: ValueDef::Instr,
            const_value: None,
            key: Some(ValueKey::Binary {
                op: PureOpKind::I32Add,
                lhs: graph[base.0].origin,
                rhs: graph[imm.0].origin,
            }),
            address_shape: Some(AddressShape {
                base: AddressBaseKind::EntryLocal(src),
                offset_delta: 5,
            }),
            loop_value_shape: Some(LoopValueShape::Local4ConstAdd { base: src, imm: 5 }),
            slot_shape: build_slot_shape(
                Some(SlotRef::entry_local(src)),
                Some(AddressShape {
                    base: AddressBaseKind::EntryLocal(src),
                    offset_delta: 5,
                }),
                Some(LoopValueShape::Local4ConstAdd { base: src, imm: 5 }),
            ),
            provider_class: ProviderClass::PureBinary,
            materialization_cost: MaterializationCost::Pure,
            producer_op: Some(2),
            materialized_block: Some(1),
            materialized_op: Some(2),
            needs_spill: false,
            use_count: 1,
            ref_count: 0,
            removable: true,
        });

        let body = BlockBody {
            ops: vec![
                BlockOp {
                    source_start: Some(20),
                    op: vm::op_local_get4 as Op,
                    kind: BlockOpKind::LocalGet,
                    operands: vec![BlockOperand::LocalAddr(src.addr)],
                    inputs: Vec::new(),
                    values: vec![base],
                },
                BlockOp {
                    source_start: Some(22),
                    op: vm::op_i32_const as Op,
                    kind: BlockOpKind::Const,
                    operands: vec![BlockOperand::I32(5)],
                    inputs: Vec::new(),
                    values: vec![imm],
                },
                BlockOp {
                    source_start: Some(24),
                    op: vm::op_i32_add as Op,
                    kind: BlockOpKind::PureBinary(PureOpKind::I32Add),
                    operands: Vec::new(),
                    inputs: vec![base, imm],
                    values: vec![add],
                },
                BlockOp {
                    source_start: Some(26),
                    op: vm::op_local_set4 as Op,
                    kind: BlockOpKind::LocalSet,
                    operands: vec![BlockOperand::LocalAddr(dst.addr)],
                    inputs: vec![add],
                    values: Vec::new(),
                },
            ],
            terminator: None,
        };

        let spec = expect_specialized_local_control_consumer(&graph, &body, 3);
        assert!(std::ptr::fn_addr_eq(
            spec.op,
            vm::op_local_get4_i32_const_add_set4 as Op
        ));
        assert_eq!(spec.source_start, Some(20));
        assert_eq!(spec.absorbed_ops, BTreeSet::from([0usize, 1usize, 2usize]));
        assert!(verify_specialized_local_control_lowering(&body, &spec));
    }

    #[test]
    fn local_control_relower_accepts_slot_preserving_block_argument_leaf_for_br_if() {
        let src = LocalSlot::new(0, 4);
        let mut graph = ValueGraph::default();
        let base = ExprId(graph.nodes.len());
        graph.nodes.push(ExprState {
            ty: ValType::I32,
            origin: ExprOrigin {
                block_id: 7,
                ordinal: 0,
                kind: ExprOriginKind::BlockArgument,
            },
            def: ValueDef::BlockArgument(crate::parser::core::optimizer::expr::BlockArgumentId(0)),
            const_value: None,
            key: None,
            address_shape: Some(AddressShape {
                base: AddressBaseKind::EntryLocal(src),
                offset_delta: 0,
            }),
            loop_value_shape: None,
            slot_shape: build_slot_shape(
                Some(SlotRef::entry_local(src)),
                Some(AddressShape {
                    base: AddressBaseKind::EntryLocal(src),
                    offset_delta: 0,
                }),
                None,
            ),
            provider_class: ProviderClass::LocalLoad,
            materialization_cost: MaterializationCost::Local,
            producer_op: Some(0),
            materialized_block: Some(7),
            materialized_op: Some(0),
            needs_spill: false,
            use_count: 1,
            ref_count: 0,
            removable: true,
        });
        let imm = ExprId(graph.nodes.len());
        graph.nodes.push(ExprState {
            ty: ValType::I32,
            origin: ExprOrigin {
                block_id: 7,
                ordinal: 1,
                kind: ExprOriginKind::InstrResult,
            },
            def: ValueDef::Const,
            const_value: Some(ConstValue::I32(3)),
            key: None,
            address_shape: None,
            loop_value_shape: None,
            slot_shape: None,
            provider_class: ProviderClass::Const,
            materialization_cost: MaterializationCost::Immediate,
            producer_op: Some(1),
            materialized_block: Some(7),
            materialized_op: Some(1),
            needs_spill: false,
            use_count: 1,
            ref_count: 0,
            removable: true,
        });
        let add = ExprId(graph.nodes.len());
        graph.nodes.push(ExprState {
            ty: ValType::I32,
            origin: ExprOrigin {
                block_id: 7,
                ordinal: 2,
                kind: ExprOriginKind::InstrResult,
            },
            def: ValueDef::Instr,
            const_value: None,
            key: Some(ValueKey::Binary {
                op: PureOpKind::I32Add,
                lhs: graph[base.0].origin,
                rhs: graph[imm.0].origin,
            }),
            address_shape: Some(AddressShape {
                base: AddressBaseKind::EntryLocal(src),
                offset_delta: 3,
            }),
            loop_value_shape: Some(LoopValueShape::Local4ConstAdd { base: src, imm: 3 }),
            slot_shape: build_slot_shape(
                Some(SlotRef::entry_local(src)),
                Some(AddressShape {
                    base: AddressBaseKind::EntryLocal(src),
                    offset_delta: 3,
                }),
                Some(LoopValueShape::Local4ConstAdd { base: src, imm: 3 }),
            ),
            provider_class: ProviderClass::PureBinary,
            materialization_cost: MaterializationCost::Pure,
            producer_op: Some(2),
            materialized_block: Some(7),
            materialized_op: Some(2),
            needs_spill: false,
            use_count: 1,
            ref_count: 0,
            removable: true,
        });

        let body = BlockBody {
            ops: vec![
                BlockOp {
                    source_start: Some(30),
                    op: vm::op_local_get4 as Op,
                    kind: BlockOpKind::LocalGet,
                    operands: vec![BlockOperand::LocalAddr(src.addr)],
                    inputs: Vec::new(),
                    values: vec![base],
                },
                BlockOp {
                    source_start: Some(32),
                    op: vm::op_i32_const as Op,
                    kind: BlockOpKind::Const,
                    operands: vec![BlockOperand::I32(3)],
                    inputs: Vec::new(),
                    values: vec![imm],
                },
                BlockOp {
                    source_start: Some(34),
                    op: vm::op_i32_add as Op,
                    kind: BlockOpKind::PureBinary(PureOpKind::I32Add),
                    operands: Vec::new(),
                    inputs: vec![base, imm],
                    values: vec![add],
                },
            ],
            terminator: Some(BlockTerminator {
                source_start: Some(36),
                op: vm::op_br_if as Op,
                kind: BlockTerminatorKind::BrIf,
                operands: vec![BlockOperand::JumpTarget(11)],
                inputs: vec![add],
                values: Vec::new(),
            }),
        };

        let spec = expect_specialized_br_if(&graph, &body);
        assert!(std::ptr::fn_addr_eq(
            spec.op,
            vm::op_local_get4_i32_const_add_br_if as Op
        ));
        assert_eq!(spec.source_start, Some(30));
        assert_eq!(spec.absorbed_ops, BTreeSet::from([0usize, 1usize, 2usize]));
        assert!(verify_specialized_local_control_lowering(&body, &spec));
    }

    #[test]
    fn local_control_relower_specializes_eqz_br_if_without_provider_source_spans() {
        let src = LocalSlot::new(0, 4);
        let mut graph = ValueGraph::default();
        let base = ExprId(graph.nodes.len());
        graph.nodes.push(ExprState {
            ty: ValType::I32,
            origin: ExprOrigin {
                block_id: 5,
                ordinal: 0,
                kind: ExprOriginKind::InstrResult,
            },
            def: ValueDef::Instr,
            const_value: None,
            key: None,
            address_shape: Some(AddressShape {
                base: AddressBaseKind::EntryLocal(src),
                offset_delta: 0,
            }),
            loop_value_shape: Some(LoopValueShape::Local4(src)),
            slot_shape: build_slot_shape(
                Some(SlotRef::entry_local(src)),
                Some(AddressShape {
                    base: AddressBaseKind::EntryLocal(src),
                    offset_delta: 0,
                }),
                Some(LoopValueShape::Local4(src)),
            ),
            provider_class: ProviderClass::LocalLoad,
            materialization_cost: MaterializationCost::Local,
            producer_op: Some(0),
            materialized_block: Some(5),
            materialized_op: Some(0),
            needs_spill: false,
            use_count: 1,
            ref_count: 0,
            removable: true,
        });
        let compare = ExprId(graph.nodes.len());
        graph.nodes.push(ExprState {
            ty: ValType::I32,
            origin: ExprOrigin {
                block_id: 5,
                ordinal: 1,
                kind: ExprOriginKind::InstrResult,
            },
            def: ValueDef::Instr,
            const_value: None,
            key: Some(ValueKey::Unary {
                op: PureOpKind::I32Eqz,
                input: graph[base.0].origin,
            }),
            address_shape: None,
            loop_value_shape: Some(LoopValueShape::CompareEqz {
                input: Box::new(LoopValueShape::Local4(src)),
            }),
            slot_shape: None,
            provider_class: ProviderClass::PureUnary,
            materialization_cost: MaterializationCost::Pure,
            producer_op: Some(1),
            materialized_block: Some(5),
            materialized_op: Some(1),
            needs_spill: false,
            use_count: 1,
            ref_count: 0,
            removable: true,
        });

        let body = BlockBody {
            ops: vec![
                BlockOp {
                    source_start: None,
                    op: vm::op_local_get4 as Op,
                    kind: BlockOpKind::LocalGet,
                    operands: vec![BlockOperand::LocalAddr(src.addr)],
                    inputs: Vec::new(),
                    values: vec![base],
                },
                BlockOp {
                    source_start: None,
                    op: vm::op_i32_eqz as Op,
                    kind: BlockOpKind::PureUnary(PureOpKind::I32Eqz),
                    operands: Vec::new(),
                    inputs: vec![base],
                    values: vec![compare],
                },
            ],
            terminator: Some(BlockTerminator {
                source_start: Some(24),
                op: vm::op_br_if as Op,
                kind: BlockTerminatorKind::BrIf,
                operands: vec![BlockOperand::JumpTarget(11)],
                inputs: vec![compare],
                values: Vec::new(),
            }),
        };

        let spec = expect_specialized_br_if(&graph, &body);
        assert!(std::ptr::fn_addr_eq(
            spec.op,
            vm::op_local_get4_i32_eqz_br_if as Op
        ));
        assert_eq!(spec.source_start, Some(24));
        assert_eq!(spec.absorbed_ops, BTreeSet::from([0usize, 1usize]));
        assert!(verify_specialized_local_control_lowering(&body, &spec));
    }

    #[test]
    fn local_control_relower_specializes_eqz_br_if_for_block_argument_leaf() {
        let src = LocalSlot::new(0, 4);
        let mut graph = ValueGraph::default();
        let base = ExprId(graph.nodes.len());
        graph.nodes.push(ExprState {
            ty: ValType::I32,
            origin: ExprOrigin {
                block_id: 5,
                ordinal: 0,
                kind: ExprOriginKind::BlockArgument,
            },
            def: ValueDef::BlockArgument(crate::parser::core::optimizer::expr::BlockArgumentId(0)),
            const_value: None,
            key: None,
            address_shape: Some(AddressShape {
                base: AddressBaseKind::EntryLocal(src),
                offset_delta: 0,
            }),
            loop_value_shape: Some(LoopValueShape::Local4(src)),
            slot_shape: build_slot_shape(
                Some(SlotRef::entry_local(src)),
                Some(AddressShape {
                    base: AddressBaseKind::EntryLocal(src),
                    offset_delta: 0,
                }),
                Some(LoopValueShape::Local4(src)),
            ),
            provider_class: ProviderClass::LocalLoad,
            materialization_cost: MaterializationCost::Local,
            producer_op: Some(0),
            materialized_block: Some(5),
            materialized_op: Some(0),
            needs_spill: false,
            use_count: 1,
            ref_count: 0,
            removable: true,
        });
        let compare = ExprId(graph.nodes.len());
        graph.nodes.push(ExprState {
            ty: ValType::I32,
            origin: ExprOrigin {
                block_id: 5,
                ordinal: 1,
                kind: ExprOriginKind::InstrResult,
            },
            def: ValueDef::Instr,
            const_value: None,
            key: Some(ValueKey::Unary {
                op: PureOpKind::I32Eqz,
                input: graph[base.0].origin,
            }),
            address_shape: None,
            loop_value_shape: Some(LoopValueShape::CompareEqz {
                input: Box::new(LoopValueShape::Local4(src)),
            }),
            slot_shape: None,
            provider_class: ProviderClass::PureUnary,
            materialization_cost: MaterializationCost::Pure,
            producer_op: Some(1),
            materialized_block: Some(5),
            materialized_op: Some(1),
            needs_spill: false,
            use_count: 1,
            ref_count: 0,
            removable: true,
        });

        let body = BlockBody {
            ops: vec![
                BlockOp {
                    source_start: Some(20),
                    op: vm::op_local_get4 as Op,
                    kind: BlockOpKind::LocalGet,
                    operands: vec![BlockOperand::LocalAddr(src.addr)],
                    inputs: Vec::new(),
                    values: vec![base],
                },
                BlockOp {
                    source_start: Some(22),
                    op: vm::op_i32_eqz as Op,
                    kind: BlockOpKind::PureUnary(PureOpKind::I32Eqz),
                    operands: Vec::new(),
                    inputs: vec![base],
                    values: vec![compare],
                },
            ],
            terminator: Some(BlockTerminator {
                source_start: Some(24),
                op: vm::op_br_if as Op,
                kind: BlockTerminatorKind::BrIf,
                operands: vec![BlockOperand::JumpTarget(11)],
                inputs: vec![compare],
                values: Vec::new(),
            }),
        };

        let spec = expect_specialized_br_if(&graph, &body);
        assert!(std::ptr::fn_addr_eq(
            spec.op,
            vm::op_local_get4_i32_eqz_br_if as Op
        ));
        assert_eq!(spec.source_start, Some(20));
        assert_eq!(spec.absorbed_ops, BTreeSet::from([0usize, 1usize]));
        assert!(verify_specialized_local_control_lowering(&body, &spec));
    }

    #[test]
    fn local_control_relower_specializes_eqz_br_if_for_block_argument_leaf_without_shapes() {
        let src = LocalSlot::new(0, 4);
        let mut graph = ValueGraph::default();
        let base = ExprId(graph.nodes.len());
        graph.nodes.push(ExprState {
            ty: ValType::I32,
            origin: ExprOrigin {
                block_id: 5,
                ordinal: 0,
                kind: ExprOriginKind::BlockArgument,
            },
            def: ValueDef::BlockArgument(crate::parser::core::optimizer::expr::BlockArgumentId(0)),
            const_value: None,
            key: None,
            address_shape: None,
            loop_value_shape: None,
            slot_shape: build_slot_shape(
                Some(SlotRef::entry_local(src)),
                Some(AddressShape {
                    base: AddressBaseKind::EntryLocal(src),
                    offset_delta: 0,
                }),
                Some(LoopValueShape::Local4(src)),
            ),
            provider_class: ProviderClass::LocalLoad,
            materialization_cost: MaterializationCost::Local,
            producer_op: Some(0),
            materialized_block: Some(5),
            materialized_op: Some(0),
            needs_spill: false,
            use_count: 1,
            ref_count: 0,
            removable: true,
        });
        let compare = ExprId(graph.nodes.len());
        graph.nodes.push(ExprState {
            ty: ValType::I32,
            origin: ExprOrigin {
                block_id: 5,
                ordinal: 1,
                kind: ExprOriginKind::InstrResult,
            },
            def: ValueDef::Instr,
            const_value: None,
            key: Some(ValueKey::Unary {
                op: PureOpKind::I32Eqz,
                input: graph[base.0].origin,
            }),
            address_shape: None,
            loop_value_shape: None,
            slot_shape: None,
            provider_class: ProviderClass::PureUnary,
            materialization_cost: MaterializationCost::Pure,
            producer_op: Some(1),
            materialized_block: Some(5),
            materialized_op: Some(1),
            needs_spill: false,
            use_count: 1,
            ref_count: 0,
            removable: true,
        });

        let body = BlockBody {
            ops: vec![
                BlockOp {
                    source_start: Some(20),
                    op: vm::op_local_get4 as Op,
                    kind: BlockOpKind::LocalGet,
                    operands: vec![BlockOperand::LocalAddr(src.addr)],
                    inputs: Vec::new(),
                    values: vec![base],
                },
                BlockOp {
                    source_start: Some(22),
                    op: vm::op_i32_eqz as Op,
                    kind: BlockOpKind::PureUnary(PureOpKind::I32Eqz),
                    operands: Vec::new(),
                    inputs: vec![base],
                    values: vec![compare],
                },
            ],
            terminator: Some(BlockTerminator {
                source_start: Some(24),
                op: vm::op_br_if as Op,
                kind: BlockTerminatorKind::BrIf,
                operands: vec![BlockOperand::JumpTarget(11)],
                inputs: vec![compare],
                values: Vec::new(),
            }),
        };

        let spec = expect_specialized_br_if(&graph, &body);
        assert!(std::ptr::fn_addr_eq(
            spec.op,
            vm::op_local_get4_i32_eqz_br_if as Op
        ));
        assert_eq!(spec.source_start, Some(20));
        assert_eq!(spec.absorbed_ops, BTreeSet::from([0usize, 1usize]));
        assert!(verify_specialized_local_control_lowering(&body, &spec));
    }

    #[test]
    fn visit_select_preserves_lossless_slot_shape() {
        let slot = LocalSlot::new(0, 4);
        let address_shape = AddressShape {
            base: AddressBaseKind::EntryLocal(slot),
            offset_delta: 0,
        };
        let slot_shape = build_slot_shape(
            Some(SlotRef::entry_local(slot)),
            Some(address_shape),
            Some(LoopValueShape::Local4(slot)),
        )
        .expect("slot shape");
        let mut optimizer = BlockOptimizer {
            block_id: 7,
            ..BlockOptimizer::default()
        };
        let lhs = push_test_state(
            &mut optimizer.exprs,
            ExprState {
                ty: ValType::I32,
                origin: ExprOrigin {
                    block_id: 7,
                    ordinal: 0,
                    kind: ExprOriginKind::BlockArgument,
                },
                def: ValueDef::BlockArgument(
                    crate::parser::core::optimizer::expr::BlockArgumentId(0),
                ),
                const_value: None,
                key: None,
                address_shape: None,
                loop_value_shape: None,
                slot_shape: Some(slot_shape.clone()),
                provider_class: ProviderClass::None,
                materialization_cost: MaterializationCost::Unknown,
                producer_op: None,
                materialized_block: None,
                materialized_op: None,
                needs_spill: false,
                use_count: 0,
                ref_count: 0,
                removable: false,
            },
        );
        let rhs = push_test_state(
            &mut optimizer.exprs,
            ExprState {
                ty: ValType::I32,
                origin: ExprOrigin {
                    block_id: 7,
                    ordinal: 1,
                    kind: ExprOriginKind::BlockArgument,
                },
                def: ValueDef::BlockArgument(
                    crate::parser::core::optimizer::expr::BlockArgumentId(1),
                ),
                const_value: None,
                key: None,
                address_shape: None,
                loop_value_shape: None,
                slot_shape: Some(slot_shape.clone()),
                provider_class: ProviderClass::None,
                materialization_cost: MaterializationCost::Unknown,
                producer_op: None,
                materialized_block: None,
                materialized_op: None,
                needs_spill: false,
                use_count: 0,
                ref_count: 0,
                removable: false,
            },
        );
        let cond = push_test_state(
            &mut optimizer.exprs,
            ExprState {
                ty: ValType::I32,
                origin: ExprOrigin {
                    block_id: 7,
                    ordinal: 2,
                    kind: ExprOriginKind::BlockArgument,
                },
                def: ValueDef::BlockArgument(
                    crate::parser::core::optimizer::expr::BlockArgumentId(2),
                ),
                const_value: None,
                key: None,
                address_shape: None,
                loop_value_shape: None,
                slot_shape: None,
                provider_class: ProviderClass::None,
                materialization_cost: MaterializationCost::Unknown,
                producer_op: None,
                materialized_block: None,
                materialized_op: None,
                needs_spill: false,
                use_count: 0,
                ref_count: 0,
                removable: false,
            },
        );
        optimizer.push_stack(lhs);
        optimizer.push_stack(rhs);
        optimizer.push_stack(cond);

        let record = DecodedInstr {
            old_start: 12,
            op: vm::op_select as Op,
            operands: vec![Operand { select: 4 }],
            stack_before: snapshot(&[ValType::I32, ValType::I32, ValType::I32]),
            stack_after: snapshot(&[ValType::I32]),
            preserved_prefix_len: 0,
            fresh_result_count: 1,
        };

        optimizer.visit_select(&record, 0);

        let result = *optimizer.stack.last().expect("select result");
        assert_eq!(
            effective_slot_shape(&optimizer.exprs, result),
            Some(slot_shape)
        );
        assert_eq!(optimizer.builder.entries.len(), 1);
        assert!(std::ptr::fn_addr_eq(
            optimizer.builder.entries[0].op,
            vm::op_select as Op
        ));
    }

    #[test]
    fn local_control_relower_specializes_eqz_br_if_from_slot_ref_without_provider_ops() {
        let src = LocalSlot::new(0, 4);
        let mut graph = ValueGraph::default();
        let base = push_test_state(
            &mut graph,
            ExprState {
                ty: ValType::I32,
                origin: ExprOrigin {
                    block_id: 5,
                    ordinal: 0,
                    kind: ExprOriginKind::BlockArgument,
                },
                def: ValueDef::BlockArgument(
                    crate::parser::core::optimizer::expr::BlockArgumentId(0),
                ),
                const_value: None,
                key: None,
                address_shape: None,
                loop_value_shape: None,
                slot_shape: build_slot_shape(Some(SlotRef::entry_local(src)), None, None),
                provider_class: ProviderClass::None,
                materialization_cost: MaterializationCost::Unknown,
                producer_op: None,
                materialized_block: None,
                materialized_op: None,
                needs_spill: false,
                use_count: 1,
                ref_count: 0,
                removable: false,
            },
        );
        let base_origin = graph[base.0].origin;
        let compare = push_test_state(
            &mut graph,
            ExprState {
                ty: ValType::I32,
                origin: ExprOrigin {
                    block_id: 5,
                    ordinal: 1,
                    kind: ExprOriginKind::InstrResult,
                },
                def: ValueDef::Instr,
                const_value: None,
                key: Some(ValueKey::Unary {
                    op: PureOpKind::I32Eqz,
                    input: base_origin,
                }),
                address_shape: None,
                loop_value_shape: None,
                slot_shape: None,
                provider_class: ProviderClass::None,
                materialization_cost: MaterializationCost::Unknown,
                producer_op: Some(0),
                materialized_block: Some(5),
                materialized_op: Some(0),
                needs_spill: false,
                use_count: 1,
                ref_count: 0,
                removable: true,
            },
        );

        let body = BlockBody {
            ops: vec![BlockOp {
                source_start: Some(22),
                op: vm::op_i32_eqz as Op,
                kind: BlockOpKind::PureUnary(PureOpKind::I32Eqz),
                operands: Vec::new(),
                inputs: vec![base],
                values: vec![compare],
            }],
            terminator: Some(BlockTerminator {
                source_start: Some(24),
                op: vm::op_br_if as Op,
                kind: BlockTerminatorKind::BrIf,
                operands: vec![BlockOperand::JumpTarget(11)],
                inputs: vec![compare],
                values: Vec::new(),
            }),
        };

        let spec = expect_specialized_br_if(&graph, &body);
        assert!(std::ptr::fn_addr_eq(
            spec.op,
            vm::op_local_get4_i32_eqz_br_if as Op
        ));
        assert_eq!(spec.source_start, Some(22));
        assert_eq!(spec.absorbed_ops, BTreeSet::from([0usize]));
        assert!(verify_specialized_local_control_lowering(&body, &spec));
    }

    #[test]
    fn local_control_relower_specializes_const_compare_br_if_from_slot_ref_without_provider_ops() {
        let src = LocalSlot::new(0, 4);
        let mut graph = ValueGraph::default();
        let lhs = push_test_state(
            &mut graph,
            ExprState {
                ty: ValType::I32,
                origin: ExprOrigin {
                    block_id: 5,
                    ordinal: 0,
                    kind: ExprOriginKind::BlockArgument,
                },
                def: ValueDef::BlockArgument(
                    crate::parser::core::optimizer::expr::BlockArgumentId(0),
                ),
                const_value: None,
                key: None,
                address_shape: None,
                loop_value_shape: None,
                slot_shape: build_slot_shape(Some(SlotRef::entry_local(src)), None, None),
                provider_class: ProviderClass::None,
                materialization_cost: MaterializationCost::Unknown,
                producer_op: None,
                materialized_block: None,
                materialized_op: None,
                needs_spill: false,
                use_count: 1,
                ref_count: 0,
                removable: false,
            },
        );
        let imm = push_test_state(
            &mut graph,
            ExprState {
                ty: ValType::I32,
                origin: ExprOrigin {
                    block_id: 5,
                    ordinal: 1,
                    kind: ExprOriginKind::SyntheticConst,
                },
                def: ValueDef::Const,
                const_value: Some(ConstValue::I32(7)),
                key: None,
                address_shape: None,
                loop_value_shape: None,
                slot_shape: None,
                provider_class: ProviderClass::None,
                materialization_cost: MaterializationCost::Unknown,
                producer_op: None,
                materialized_block: None,
                materialized_op: None,
                needs_spill: false,
                use_count: 1,
                ref_count: 0,
                removable: true,
            },
        );
        let lhs_origin = graph[lhs.0].origin;
        let imm_origin = graph[imm.0].origin;
        let compare = push_test_state(
            &mut graph,
            ExprState {
                ty: ValType::I32,
                origin: ExprOrigin {
                    block_id: 5,
                    ordinal: 2,
                    kind: ExprOriginKind::InstrResult,
                },
                def: ValueDef::Instr,
                const_value: None,
                key: Some(ValueKey::Binary {
                    op: PureOpKind::I32Eq,
                    lhs: lhs_origin,
                    rhs: imm_origin,
                }),
                address_shape: None,
                loop_value_shape: None,
                slot_shape: None,
                provider_class: ProviderClass::None,
                materialization_cost: MaterializationCost::Unknown,
                producer_op: Some(0),
                materialized_block: Some(5),
                materialized_op: Some(0),
                needs_spill: false,
                use_count: 1,
                ref_count: 0,
                removable: true,
            },
        );

        let body = BlockBody {
            ops: vec![BlockOp {
                source_start: Some(22),
                op: vm::op_i32_eq as Op,
                kind: BlockOpKind::PureBinary(PureOpKind::I32Eq),
                operands: Vec::new(),
                inputs: vec![lhs, imm],
                values: vec![compare],
            }],
            terminator: Some(BlockTerminator {
                source_start: Some(24),
                op: vm::op_br_if as Op,
                kind: BlockTerminatorKind::BrIf,
                operands: vec![BlockOperand::JumpTarget(11)],
                inputs: vec![compare],
                values: Vec::new(),
            }),
        };

        let spec = expect_specialized_br_if(&graph, &body);
        assert!(std::ptr::fn_addr_eq(
            spec.op,
            vm::op_local_get4_i32_const_compare_br_if as Op
        ));
        assert_eq!(spec.source_start, Some(22));
        assert_eq!(spec.absorbed_ops, BTreeSet::from([0usize]));
        assert!(verify_specialized_local_control_lowering(&body, &spec));
    }

    #[test]
    fn local_control_relower_specializes_local_compare_br_if_from_slot_ref_without_provider_ops() {
        let lhs_slot = LocalSlot::new(0, 4);
        let rhs_slot = LocalSlot::new(4, 4);
        let mut graph = ValueGraph::default();
        let lhs = push_test_state(
            &mut graph,
            ExprState {
                ty: ValType::I32,
                origin: ExprOrigin {
                    block_id: 5,
                    ordinal: 0,
                    kind: ExprOriginKind::BlockArgument,
                },
                def: ValueDef::BlockArgument(
                    crate::parser::core::optimizer::expr::BlockArgumentId(0),
                ),
                const_value: None,
                key: None,
                address_shape: None,
                loop_value_shape: None,
                slot_shape: build_slot_shape(Some(SlotRef::entry_local(lhs_slot)), None, None),
                provider_class: ProviderClass::None,
                materialization_cost: MaterializationCost::Unknown,
                producer_op: None,
                materialized_block: None,
                materialized_op: None,
                needs_spill: false,
                use_count: 1,
                ref_count: 0,
                removable: false,
            },
        );
        let rhs = push_test_state(
            &mut graph,
            ExprState {
                ty: ValType::I32,
                origin: ExprOrigin {
                    block_id: 5,
                    ordinal: 1,
                    kind: ExprOriginKind::BlockArgument,
                },
                def: ValueDef::BlockArgument(
                    crate::parser::core::optimizer::expr::BlockArgumentId(1),
                ),
                const_value: None,
                key: None,
                address_shape: None,
                loop_value_shape: None,
                slot_shape: build_slot_shape(Some(SlotRef::entry_local(rhs_slot)), None, None),
                provider_class: ProviderClass::None,
                materialization_cost: MaterializationCost::Unknown,
                producer_op: None,
                materialized_block: None,
                materialized_op: None,
                needs_spill: false,
                use_count: 1,
                ref_count: 0,
                removable: false,
            },
        );
        let lhs_origin = graph[lhs.0].origin;
        let rhs_origin = graph[rhs.0].origin;
        let compare = push_test_state(
            &mut graph,
            ExprState {
                ty: ValType::I32,
                origin: ExprOrigin {
                    block_id: 5,
                    ordinal: 2,
                    kind: ExprOriginKind::InstrResult,
                },
                def: ValueDef::Instr,
                const_value: None,
                key: Some(ValueKey::Binary {
                    op: PureOpKind::I32LtS,
                    lhs: lhs_origin,
                    rhs: rhs_origin,
                }),
                address_shape: None,
                loop_value_shape: None,
                slot_shape: None,
                provider_class: ProviderClass::None,
                materialization_cost: MaterializationCost::Unknown,
                producer_op: Some(0),
                materialized_block: Some(5),
                materialized_op: Some(0),
                needs_spill: false,
                use_count: 1,
                ref_count: 0,
                removable: true,
            },
        );

        let body = BlockBody {
            ops: vec![BlockOp {
                source_start: Some(22),
                op: vm::op_i32_lt_s as Op,
                kind: BlockOpKind::PureBinary(PureOpKind::I32LtS),
                operands: Vec::new(),
                inputs: vec![lhs, rhs],
                values: vec![compare],
            }],
            terminator: Some(BlockTerminator {
                source_start: Some(24),
                op: vm::op_br_if as Op,
                kind: BlockTerminatorKind::BrIf,
                operands: vec![BlockOperand::JumpTarget(11)],
                inputs: vec![compare],
                values: Vec::new(),
            }),
        };

        let spec = expect_specialized_br_if(&graph, &body);
        assert!(std::ptr::fn_addr_eq(
            spec.op,
            vm::op_local_get4_local_get4_compare_br_if as Op
        ));
        assert_eq!(spec.source_start, Some(22));
        assert_eq!(spec.absorbed_ops, BTreeSet::from([0usize]));
        assert!(verify_specialized_local_control_lowering(&body, &spec));
    }

    #[test]
    fn local_control_relower_does_not_specialize_special_function_return_as_br_if() {
        let src = LocalSlot::new(0, 4);
        let mut graph = ValueGraph::default();
        let value = ExprId(graph.nodes.len());
        graph.nodes.push(ExprState {
            ty: ValType::I32,
            origin: ExprOrigin {
                block_id: 9,
                ordinal: 0,
                kind: ExprOriginKind::InstrResult,
            },
            def: ValueDef::Instr,
            const_value: None,
            key: None,
            address_shape: Some(AddressShape {
                base: AddressBaseKind::EntryLocal(src),
                offset_delta: 0,
            }),
            loop_value_shape: Some(LoopValueShape::Local4(src)),
            slot_shape: build_slot_shape(
                Some(SlotRef::entry_local(src)),
                Some(AddressShape {
                    base: AddressBaseKind::EntryLocal(src),
                    offset_delta: 0,
                }),
                Some(LoopValueShape::Local4(src)),
            ),
            provider_class: ProviderClass::LocalLoad,
            materialization_cost: MaterializationCost::Local,
            producer_op: Some(0),
            materialized_block: Some(9),
            materialized_op: Some(0),
            needs_spill: false,
            use_count: 1,
            ref_count: 0,
            removable: true,
        });

        let body = BlockBody {
            ops: vec![BlockOp {
                source_start: Some(40),
                op: vm::op_local_get4 as Op,
                kind: BlockOpKind::LocalGet,
                operands: vec![BlockOperand::LocalAddr(src.addr)],
                inputs: Vec::new(),
                values: vec![value],
            }],
            terminator: Some(BlockTerminator {
                source_start: Some(42),
                op: vm::special_function_return as Op,
                kind: BlockTerminatorKind::SpecialFunctionReturn,
                operands: vec![BlockOperand::U32(1)],
                inputs: vec![value],
                values: Vec::new(),
            }),
        };

        assert!(
            build_specialized_br_if_lowering(
                &graph,
                &body,
                body.terminator.as_ref().expect("terminator")
            )
            .is_none(),
            "special function return must not be specialized as br_if"
        );
    }

    #[test]
    fn br_if_candidate_shape_reads_loop_value_shapes_from_residual_graph() {
        let slot0 = LocalSlot::new(0, 4);
        let slot1 = LocalSlot::new(4, 4);
        let mut graph = ValueGraph::default();

        let direct = ExprId(graph.nodes.len());
        graph.nodes.push(ExprState {
            ty: ValType::I32,
            origin: ExprOrigin {
                block_id: 1,
                ordinal: 0,
                kind: ExprOriginKind::InstrResult,
            },
            def: ValueDef::Instr,
            const_value: None,
            key: None,
            address_shape: Some(AddressShape {
                base: AddressBaseKind::EntryLocal(slot0),
                offset_delta: 0,
            }),
            loop_value_shape: Some(LoopValueShape::Local4(slot0)),
            slot_shape: None,
            provider_class: ProviderClass::None,
            materialization_cost: MaterializationCost::Unknown,
            producer_op: None,
            materialized_block: None,
            materialized_op: None,
            needs_spill: false,
            use_count: 0,
            ref_count: 0,
            removable: false,
        });
        let add = ExprId(graph.nodes.len());
        graph.nodes.push(ExprState {
            ty: ValType::I32,
            origin: ExprOrigin {
                block_id: 1,
                ordinal: 1,
                kind: ExprOriginKind::InstrResult,
            },
            def: ValueDef::Instr,
            const_value: None,
            key: None,
            address_shape: Some(AddressShape {
                base: AddressBaseKind::EntryLocal(slot0),
                offset_delta: 3,
            }),
            loop_value_shape: Some(LoopValueShape::Local4ConstAdd {
                base: slot0,
                imm: 3,
            }),
            slot_shape: None,
            provider_class: ProviderClass::None,
            materialization_cost: MaterializationCost::Unknown,
            producer_op: None,
            materialized_block: None,
            materialized_op: None,
            needs_spill: false,
            use_count: 0,
            ref_count: 0,
            removable: false,
        });
        let compare = ExprId(graph.nodes.len());
        graph.nodes.push(ExprState {
            ty: ValType::I32,
            origin: ExprOrigin {
                block_id: 1,
                ordinal: 2,
                kind: ExprOriginKind::InstrResult,
            },
            def: ValueDef::Instr,
            const_value: None,
            key: None,
            address_shape: None,
            loop_value_shape: Some(LoopValueShape::CompareEqz {
                input: Box::new(LoopValueShape::Local4ConstAdd {
                    base: slot0,
                    imm: 3,
                }),
            }),
            slot_shape: None,
            provider_class: ProviderClass::None,
            materialization_cost: MaterializationCost::Unknown,
            producer_op: None,
            materialized_block: None,
            materialized_op: None,
            needs_spill: false,
            use_count: 0,
            ref_count: 0,
            removable: false,
        });
        let local_compare = ExprId(graph.nodes.len());
        graph.nodes.push(ExprState {
            ty: ValType::I32,
            origin: ExprOrigin {
                block_id: 1,
                ordinal: 3,
                kind: ExprOriginKind::InstrResult,
            },
            def: ValueDef::Instr,
            const_value: None,
            key: None,
            address_shape: None,
            loop_value_shape: Some(LoopValueShape::CompareLocal4 {
                lhs: slot0,
                op: PureOpKind::I32Eq,
                rhs: slot1,
            }),
            slot_shape: None,
            provider_class: ProviderClass::None,
            materialization_cost: MaterializationCost::Unknown,
            producer_op: None,
            materialized_block: None,
            materialized_op: None,
            needs_spill: false,
            use_count: 0,
            ref_count: 0,
            removable: false,
        });

        assert_eq!(
            br_if_candidate_shape(&graph, direct),
            Some(LoopValueShape::Local4(slot0))
        );
        assert_eq!(
            br_if_candidate_shape(&graph, add),
            Some(LoopValueShape::Local4ConstAdd {
                base: slot0,
                imm: 3
            })
        );
        assert_eq!(
            br_if_candidate_shape(&graph, compare),
            Some(LoopValueShape::CompareEqz {
                input: Box::new(LoopValueShape::Local4ConstAdd {
                    base: slot0,
                    imm: 3
                }),
            })
        );
        assert_eq!(
            br_if_candidate_shape(&graph, local_compare),
            Some(LoopValueShape::CompareLocal4 {
                lhs: slot0,
                op: PureOpKind::I32Eq,
                rhs: slot1,
            })
        );
    }

    #[test]
    fn br_if_candidate_shape_reads_slot_ref_from_residual_graph() {
        let slot = LocalSlot::new(12, 4);
        let mut graph = ValueGraph::default();
        let value = push_test_state(
            &mut graph,
            ExprState {
                ty: ValType::I32,
                origin: ExprOrigin {
                    block_id: 1,
                    ordinal: 0,
                    kind: ExprOriginKind::BlockArgument,
                },
                def: ValueDef::BlockArgument(
                    crate::parser::core::optimizer::expr::BlockArgumentId(0),
                ),
                const_value: None,
                key: None,
                address_shape: None,
                loop_value_shape: None,
                slot_shape: build_slot_shape(Some(SlotRef::entry_local(slot)), None, None),
                provider_class: ProviderClass::None,
                materialization_cost: MaterializationCost::Unknown,
                producer_op: None,
                materialized_block: None,
                materialized_op: None,
                needs_spill: false,
                use_count: 0,
                ref_count: 0,
                removable: false,
            },
        );

        assert_eq!(
            br_if_candidate_shape(&graph, value),
            Some(LoopValueShape::Local4(slot))
        );
    }

    #[test]
    fn licm_preparation_candidate_matches_spill_local_address_prep_for_memory_load() {
        let mut graph = ValueGraph::default();
        let spill_value = ExprId(graph.nodes.len());
        graph.nodes.push(ExprState {
            ty: ValType::I32,
            origin: ExprOrigin {
                block_id: 1,
                ordinal: 0,
                kind: ExprOriginKind::InstrResult,
            },
            def: ValueDef::EffectResult(EffectOpId(0), 0),
            const_value: None,
            key: None,
            address_shape: Some(AddressShape {
                base: AddressBaseKind::SpillLocal(LocalSlot::new(12, 4)),
                offset_delta: 0,
            }),
            loop_value_shape: Some(LoopValueShape::Local4(LocalSlot::new(12, 4))),
            slot_shape: None,
            provider_class: ProviderClass::None,
            materialization_cost: MaterializationCost::Unknown,
            producer_op: None,
            materialized_block: None,
            materialized_op: None,
            needs_spill: true,
            use_count: 1,
            ref_count: 0,
            removable: false,
        });
        let const_value = ExprId(graph.nodes.len());
        graph.nodes.push(ExprState {
            ty: ValType::I32,
            origin: ExprOrigin {
                block_id: 1,
                ordinal: 1,
                kind: ExprOriginKind::InstrResult,
            },
            def: ValueDef::Const,
            const_value: Some(ConstValue::I32(1)),
            key: None,
            address_shape: None,
            loop_value_shape: None,
            slot_shape: None,
            provider_class: ProviderClass::None,
            materialization_cost: MaterializationCost::Unknown,
            producer_op: None,
            materialized_block: None,
            materialized_op: None,
            needs_spill: false,
            use_count: 1,
            ref_count: 0,
            removable: true,
        });
        let address_value = ExprId(graph.nodes.len());
        graph.nodes.push(ExprState {
            ty: ValType::I32,
            origin: ExprOrigin {
                block_id: 1,
                ordinal: 2,
                kind: ExprOriginKind::InstrResult,
            },
            def: ValueDef::Instr,
            const_value: None,
            key: None,
            address_shape: Some(AddressShape {
                base: AddressBaseKind::SpillLocal(LocalSlot::new(12, 4)),
                offset_delta: 1,
            }),
            loop_value_shape: Some(LoopValueShape::Local4ConstAdd {
                base: LocalSlot::new(12, 4),
                imm: 1,
            }),
            slot_shape: None,
            provider_class: ProviderClass::None,
            materialization_cost: MaterializationCost::Unknown,
            producer_op: None,
            materialized_block: None,
            materialized_op: None,
            needs_spill: false,
            use_count: 1,
            ref_count: 0,
            removable: true,
        });
        let load_value = ExprId(graph.nodes.len());
        graph.nodes.push(ExprState {
            ty: ValType::I32,
            origin: ExprOrigin {
                block_id: 1,
                ordinal: 3,
                kind: ExprOriginKind::InstrResult,
            },
            def: ValueDef::EffectResult(EffectOpId(1), 0),
            const_value: None,
            key: None,
            address_shape: None,
            loop_value_shape: None,
            slot_shape: None,
            provider_class: ProviderClass::None,
            materialization_cost: MaterializationCost::Unknown,
            producer_op: None,
            materialized_block: None,
            materialized_op: None,
            needs_spill: false,
            use_count: 0,
            ref_count: 0,
            removable: false,
        });

        let body = BlockBody {
            ops: vec![
                BlockOp {
                    source_start: Some(10),
                    op: local_get_op(4),
                    kind: BlockOpKind::LocalGet,
                    operands: vec![BlockOperand::SpillValue(spill_value)],
                    inputs: Vec::new(),
                    values: vec![spill_value],
                },
                BlockOp {
                    source_start: Some(12),
                    op: vm::op_i32_const as Op,
                    kind: BlockOpKind::Const,
                    operands: vec![BlockOperand::I32(1)],
                    inputs: Vec::new(),
                    values: vec![const_value],
                },
                BlockOp {
                    source_start: Some(14),
                    op: vm::op_i32_add as Op,
                    kind: BlockOpKind::PureBinary(PureOpKind::I32Add),
                    operands: Vec::new(),
                    inputs: vec![spill_value, const_value],
                    values: vec![address_value],
                },
                BlockOp {
                    source_start: Some(16),
                    op: vm::op_i32_load8_u as Op,
                    kind: BlockOpKind::MemoryLoad,
                    operands: vec![BlockOperand::Raw(Operand {
                        memarg: crate::common::MemArg {
                            align: 0,
                            offset: 0,
                        },
                    })],
                    inputs: vec![address_value],
                    values: vec![load_value],
                },
            ],
            terminator: None,
        };

        let candidate = match_licm_preparation_candidate(&graph, &body, 0, &LoopEffects::default())
            .expect("address preparation must be hoistable");
        assert_eq!(candidate.start, 0);
        assert_eq!(candidate.end, 3);
        assert_eq!(candidate.root_value, address_value);
        assert_eq!(candidate.result_size, 4);
        assert_eq!(candidate.source_start, Some(10));
    }

    #[test]
    fn packed_stream_budget_uses_relative_growth_with_small_function_slack() {
        assert!(packed_stream_within_budget(4, 12));
        assert!(!packed_stream_within_budget(4, 13));
        assert!(packed_stream_within_budget(100, 110));
        assert!(!packed_stream_within_budget(100, 111));
    }
}
