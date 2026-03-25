use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque},
    hash::{Hash, Hasher},
    sync::OnceLock,
    time::{Duration, Instant},
};

use crate::{
    common::{FuncIdx, FuncType, Instr, LocalsData, Op, Operand, ValType},
    runtime::vm,
};

use super::{
    cfg::{build_program, BasicBlock, BasicBlockProgram, DecodedInstr, InstructionMeta},
    expr::{
        AliasAddress, AliasKey, AliasSpace, ConstValue, EffectBarrier, EffectEpoch, EffectOpId,
        ExprId, ExprOrigin, ExprOriginKind, ExprState, HeapVersion, LocalSlot, PureOpKind,
        ValueDef, ValueGraph, ValueKey, ValueRef,
    },
    sink::{flatten_records, RecordEmit},
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

#[derive(Default)]
struct RelowerPlan {
    block_bodies: Vec<BlockBody>,
    loop_invariants: Vec<LoopInvariantSet>,
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

    fn log_function_end(&self, rewrite: &FunctionRewrite) {
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
const SYNTHETIC_CONST_ORIGIN_BASE: usize = 1 << 20;

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
    if std::ptr::fn_addr_eq(op, vm::op_local_get4 as Op)
        || std::ptr::fn_addr_eq(op, vm::op_local_get8 as Op)
        || std::ptr::fn_addr_eq(op, vm::op_local_get16 as Op)
    {
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
    if std::ptr::fn_addr_eq(op, vm::op_local_get4 as Op)
        || std::ptr::fn_addr_eq(op, vm::op_local_get8 as Op)
        || std::ptr::fn_addr_eq(op, vm::op_local_get16 as Op)
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

fn block_operands_to_raw(operands: &[BlockOperand]) -> Vec<Operand> {
    operands
        .iter()
        .map(|operand| match operand {
            BlockOperand::I32(value) => Operand { i32: *value },
            BlockOperand::I64(value) => Operand { i64: *value },
            BlockOperand::F32(value) => Operand { f32: *value },
            BlockOperand::F64(value) => Operand { f64: *value },
            BlockOperand::U32(value) => Operand { u32: *value },
            BlockOperand::LocalAddr(value) => Operand { local_addr: *value },
            BlockOperand::SpillValue(_) => {
                unreachable!("spill placeholders must be resolved before raw lowering")
            }
            BlockOperand::JumpTarget(value) => Operand {
                jump_addr: *value as u32,
            },
            BlockOperand::Raw(operand) => *operand,
        })
        .collect()
}

fn block_operands_to_raw_with_spills(
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
            _ => block_operands_to_raw(std::slice::from_ref(operand))
                .into_iter()
                .next()
                .expect("single operand lowering must succeed"),
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
    std::ptr::fn_addr_eq(op, vm::op_call as Op)
        || std::ptr::fn_addr_eq(op, vm::op_call_import as Op)
        || std::ptr::fn_addr_eq(op, vm::op_return_call as Op)
        || std::ptr::fn_addr_eq(op, vm::op_return_call_import as Op)
        || std::ptr::fn_addr_eq(op, vm::op_call_indirect as Op)
        || std::ptr::fn_addr_eq(op, vm::op_return_call_indirect as Op)
}

pub(crate) fn optimize_function(
    funcidx: FuncIdx,
    _functype: &FuncType,
    locals: &mut LocalsData,
    instrs: Vec<Instr>,
    meta: Vec<InstructionMeta>,
) -> Vec<Instr> {
    let Some(program) = build_program(&instrs, meta) else {
        return instrs;
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
    let licm_modified = apply_licm(&program, &mut rewrite, locals);
    select_superinstructions(&program, &mut rewrite, &licm_modified);
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
    );
    debug_assert!(verify_effect_result_spill_ir(
        &rewrite.graph,
        &rewrite.relower.block_bodies,
        &spill_plan,
    ));
    let mut records = Vec::new();
    for block in &program.blocks {
        if reachable[block.id] {
            records.extend(relower_block_body(
                &rewrite.relower.block_bodies[block.id],
                &rewrite.graph,
                &spill_plan,
            ));
        }
    }
    debug_assert!(verify_relower_preserves_call_ops(
        &program,
        &rewrite.relower.block_bodies,
        &records,
    ));
    debug_assert!(verify_relower_preserves_effect_result_spills(
        &rewrite.graph,
        &rewrite.relower.block_bodies,
        &spill_plan,
        &records,
    ));
    if patch_jump_targets(&mut records).is_err() {
        return instrs;
    }
    if let Some(profiler) = profiler.as_ref() {
        profiler.log_function_end(&rewrite);
    }
    flatten_records(&records)
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
        },
        graph: ValueGraph::default(),
    };
    let mut worklist = VecDeque::new();
    let mut queued = vec![false; program.blocks.len()];
    worklist.push_back(0usize);
    queued[0] = true;

    while let Some(block_id) = worklist.pop_front() {
        queued[block_id] = false;
        let Some(entry) = compute_entry_state(program, &mut pass.exprs, &rewrite, block_id) else {
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
    rewrite
}

fn compute_entry_state(
    program: &BasicBlockProgram,
    graph: &mut ValueGraph,
    rewrite: &FunctionRewrite,
    block_id: usize,
) -> Option<BlockEntryState> {
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
    Some(merge_states(graph, block_id, first, &incoming))
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
    entry_changed || exit_changed || body_changed || invariants_changed
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

fn merge_states(
    graph: &mut ValueGraph,
    block_id: usize,
    first: &DecodedInstr,
    incoming: &[BlockEntryState],
) -> BlockEntryState {
    let preserve_existing_block_arguments = incoming.len() > 1;
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
            graph,
            block_id,
            ordinal,
            *ty,
            &values,
            preserve_existing_block_arguments,
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
                graph,
                block_id,
                1024 + slot.addr as usize,
                type_from_slot(slot.size),
                &values,
                preserve_existing_block_arguments,
            ),
        );
    }

    merge_aliases(
        graph,
        block_id,
        incoming,
        &mut state,
        preserve_existing_block_arguments,
    );

    state
}

fn merge_aliases(
    graph: &mut ValueGraph,
    block_id: usize,
    incoming: &[BlockEntryState],
    state: &mut BlockEntryState,
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
        return graph.ensure_block_argument(block_id, ordinal, ty, None, None);
    };
    let has_block_argument_input = values
        .iter()
        .flatten()
        .any(|value| graph[value.0].is_block_argument());
    if preserve_existing_block_arguments
        && graph
            .existing_block_argument_value(block_id, ordinal)
            .is_some()
        && (values.len() > 1 || has_block_argument_input)
    {
        let const_value = values
            .iter()
            .map(|value| value.and_then(|value| graph[value.0].const_value))
            .reduce(|lhs, rhs| if lhs == rhs { lhs } else { None })
            .flatten();
        let key = values
            .iter()
            .map(|value| value.and_then(|value| graph[value.0].key))
            .reduce(|lhs, rhs| if lhs == rhs { lhs } else { None })
            .flatten();
        return graph.ensure_block_argument(block_id, ordinal, ty, const_value, key);
    }
    if values
        .iter()
        .all(|value| value.is_some_and(|candidate| same_value(graph, *candidate, first)))
    {
        return first;
    }
    let const_value = values
        .iter()
        .map(|value| value.and_then(|value| graph[value.0].const_value))
        .reduce(|lhs, rhs| if lhs == rhs { lhs } else { None })
        .flatten();
    let key = values
        .iter()
        .map(|value| value.and_then(|value| graph[value.0].key))
        .reduce(|lhs, rhs| if lhs == rhs { lhs } else { None })
        .flatten();
    graph.ensure_block_argument(block_id, ordinal, ty, const_value, key)
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
        "origin={:?} def={:?} const={:?} key={:?}",
        value.origin, value.def, value.const_value, value.key
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
}

fn ensure_seed_value(graph: &mut ValueGraph, ty: ValType, origin: ExprOrigin) -> ValueRef {
    let value = ExprId(graph.nodes.len());
    graph.nodes.push(ExprState {
        ty,
        origin,
        def: ValueDef::Synthetic,
        const_value: None,
        key: None,
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

#[derive(Default)]
struct MemoryRelowerPlan {
    skip_ops: HashSet<usize>,
    folded_offsets: HashMap<usize, u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AddressBase {
    EntryValue(ValueRef),
    MaterializedValue(ValueRef),
}

#[derive(Clone, Debug)]
struct AddressShape {
    #[allow(dead_code)]
    base: AddressBase,
    offset_delta: i64,
    absorbed_ops: BTreeSet<usize>,
}

fn build_memory_relower_plan(body: &BlockBody, graph: &ValueGraph) -> MemoryRelowerPlan {
    let producer_indices = licm_producer_indices(body);
    let body_origin_values = block_origin_values(body, graph);
    let graph_origin_values = licm_origin_values(graph);
    let mut plan = MemoryRelowerPlan::default();
    for (op_idx, op) in body.ops.iter().enumerate() {
        if !matches!(op.kind, BlockOpKind::MemoryLoad | BlockOpKind::MemoryStore) {
            continue;
        }
        let Some(address) = memory_address_input(op) else {
            continue;
        };
        let Some(memarg) = block_op_memarg(op) else {
            continue;
        };
        let Some(shape) = resolve_address_shape(
            graph,
            body,
            &producer_indices,
            &body_origin_values,
            &graph_origin_values,
            address,
        ) else {
            continue;
        };
        if shape.absorbed_ops.is_empty() {
            continue;
        }
        let offset = i64::from(memarg.offset).checked_add(shape.offset_delta);
        let Some(offset) = offset.and_then(|value| u32::try_from(value).ok()) else {
            continue;
        };
        plan.skip_ops.extend(shape.absorbed_ops.iter().copied());
        plan.folded_offsets.insert(op_idx, offset);
    }
    plan
}

fn block_origin_values(body: &BlockBody, graph: &ValueGraph) -> HashMap<ExprOrigin, ValueRef> {
    let mut out = HashMap::new();
    for op in &body.ops {
        for value in &op.values {
            out.insert(graph[value.0].origin, *value);
        }
    }
    if let Some(terminator) = &body.terminator {
        for value in &terminator.values {
            out.insert(graph[value.0].origin, *value);
        }
    }
    out
}

fn memory_address_input(op: &BlockOp) -> Option<ValueRef> {
    matches!(op.kind, BlockOpKind::MemoryLoad | BlockOpKind::MemoryStore)
        .then(|| op.inputs.first().copied())
        .flatten()
}

fn block_op_memarg(op: &BlockOp) -> Option<crate::common::MemArg> {
    let BlockOperand::Raw(operand) = *op.operands.first()? else {
        return None;
    };
    Some(unsafe { operand.memarg })
}

fn value_for_origin<'a>(
    body_origin_values: &'a HashMap<ExprOrigin, ValueRef>,
    graph_origin_values: &'a HashMap<ExprOrigin, ValueRef>,
    origin: ExprOrigin,
) -> Option<ValueRef> {
    body_origin_values
        .get(&origin)
        .copied()
        .or_else(|| graph_origin_values.get(&origin).copied())
}

fn resolve_address_shape(
    graph: &ValueGraph,
    body: &BlockBody,
    producer_indices: &HashMap<ValueRef, usize>,
    body_origin_values: &HashMap<ExprOrigin, ValueRef>,
    graph_origin_values: &HashMap<ExprOrigin, ValueRef>,
    value: ValueRef,
) -> Option<AddressShape> {
    let node = &graph[value.0];
    if node.ty != ValType::I32 {
        return None;
    }
    if let Some(op_idx) = producer_indices.get(&value).copied() {
        let producer = body.ops.get(op_idx)?;
        if producer.kind == BlockOpKind::LocalGet {
            return Some(AddressShape {
                base: AddressBase::MaterializedValue(value),
                offset_delta: 0,
                absorbed_ops: BTreeSet::new(),
            });
        }
    }
    if matches!(
        node.origin.kind,
        ExprOriginKind::EntryLocal | ExprOriginKind::EntryStack | ExprOriginKind::BlockArgument
    ) {
        return Some(AddressShape {
            base: AddressBase::EntryValue(value),
            offset_delta: 0,
            absorbed_ops: BTreeSet::new(),
        });
    }

    let ValueKey::Binary { op, lhs, rhs } = node.key? else {
        return None;
    };
    match op {
        PureOpKind::I32Add => {
            let lhs_value = value_for_origin(body_origin_values, graph_origin_values, lhs)?;
            let rhs_value = value_for_origin(body_origin_values, graph_origin_values, rhs)?;
            if let Some(delta) = absorbable_address_const(graph, body, producer_indices, rhs_value)
            {
                return extend_address_shape(
                    graph,
                    body,
                    producer_indices,
                    body_origin_values,
                    graph_origin_values,
                    value,
                    lhs_value,
                    rhs_value,
                    i64::from(delta),
                );
            }
            if let Some(delta) = absorbable_address_const(graph, body, producer_indices, lhs_value)
            {
                return extend_address_shape(
                    graph,
                    body,
                    producer_indices,
                    body_origin_values,
                    graph_origin_values,
                    value,
                    rhs_value,
                    lhs_value,
                    i64::from(delta),
                );
            }
        }
        PureOpKind::I32Sub => {
            let lhs_value = value_for_origin(body_origin_values, graph_origin_values, lhs)?;
            let rhs_value = value_for_origin(body_origin_values, graph_origin_values, rhs)?;
            let delta = absorbable_address_const(graph, body, producer_indices, rhs_value)?;
            return extend_address_shape(
                graph,
                body,
                producer_indices,
                body_origin_values,
                graph_origin_values,
                value,
                lhs_value,
                rhs_value,
                -i64::from(delta),
            );
        }
        _ => {}
    }
    None
}

#[allow(clippy::too_many_arguments)]
fn extend_address_shape(
    graph: &ValueGraph,
    body: &BlockBody,
    producer_indices: &HashMap<ValueRef, usize>,
    body_origin_values: &HashMap<ExprOrigin, ValueRef>,
    graph_origin_values: &HashMap<ExprOrigin, ValueRef>,
    value: ValueRef,
    base_value: ValueRef,
    const_value: ValueRef,
    delta: i64,
) -> Option<AddressShape> {
    let current_idx = *producer_indices.get(&value)?;
    let current_op = body.ops.get(current_idx)?;
    if !matches!(
        current_op.kind,
        BlockOpKind::PureBinary(PureOpKind::I32Add | PureOpKind::I32Sub)
    ) || !block_op_single_use(graph, current_op)
    {
        return None;
    }
    let const_idx = *producer_indices.get(&const_value)?;
    let const_op = body.ops.get(const_idx)?;
    if const_op.kind != BlockOpKind::Const || !block_op_single_use(graph, const_op) {
        return None;
    }
    let mut shape = resolve_address_shape(
        graph,
        body,
        producer_indices,
        body_origin_values,
        graph_origin_values,
        base_value,
    )?;
    shape.offset_delta = shape.offset_delta.checked_add(delta)?;
    shape.absorbed_ops.insert(current_idx);
    shape.absorbed_ops.insert(const_idx);
    Some(shape)
}

fn absorbable_address_const(
    graph: &ValueGraph,
    body: &BlockBody,
    producer_indices: &HashMap<ValueRef, usize>,
    value: ValueRef,
) -> Option<i32> {
    let ConstValue::I32(delta) = graph[value.0].const_value? else {
        return None;
    };
    let op_idx = *producer_indices.get(&value)?;
    let op = body.ops.get(op_idx)?;
    (op.kind == BlockOpKind::Const && block_op_single_use(graph, op)).then_some(delta)
}

fn build_effect_result_spill_plan(
    graph: &ValueGraph,
    bodies: &[BlockBody],
    reachable: &[bool],
    locals: &mut LocalsData,
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
    }
    let mut plan = EffectResultSpillPlan::default();
    for value in values {
        let Some(size) = value_type_size(graph[value.0].ty) else {
            continue;
        };
        let slot = LocalSlot::new(locals.allocate_temp_slot(graph[value.0].ty), size);
        plan.slots.insert(value, slot);
    }
    plan
}

fn relower_block_body(
    body: &BlockBody,
    graph: &ValueGraph,
    spill_plan: &EffectResultSpillPlan,
) -> Vec<RecordEmit> {
    let memory_plan = build_memory_relower_plan(body, graph);
    let mut records = Vec::with_capacity(body.ops.len() + usize::from(body.terminator.is_some()));
    for (op_idx, op) in body.ops.iter().enumerate() {
        if memory_plan.skip_ops.contains(&op_idx) {
            continue;
        }
        let mut lowered = relower_block_op(op, spill_plan);
        if let Some(offset) = memory_plan.folded_offsets.get(&op_idx).copied() {
            if let Some(memarg) = lowered.operands.first_mut() {
                memarg.memarg.offset = offset;
            }
        }
        records.push(lowered);
        if let Some(slot) = spill_slot_for_effect_result(graph, op, spill_plan) {
            records.push(RecordEmit {
                source_start: None,
                op: local_tee_op(slot.size),
                operands: vec![Operand {
                    local_addr: slot.addr,
                }],
            });
        }
    }
    if let Some(terminator) = &body.terminator {
        records.push(relower_block_terminator(terminator, spill_plan));
    }
    records
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

fn relower_block_op(op: &BlockOp, spill_plan: &EffectResultSpillPlan) -> RecordEmit {
    RecordEmit {
        source_start: op.source_start,
        op: op.op,
        operands: block_operands_to_raw_with_spills(&op.operands, spill_plan),
    }
}

fn relower_block_terminator(
    terminator: &BlockTerminator,
    spill_plan: &EffectResultSpillPlan,
) -> RecordEmit {
    RecordEmit {
        source_start: terminator.source_start,
        op: terminator.op,
        operands: block_operands_to_raw_with_spills(&terminator.operands, spill_plan),
    }
}

#[derive(Default)]
struct BlockOptimizer {
    block_id: usize,
    effect_epoch: EffectEpoch,
    next_synthetic_const_ordinal: usize,
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
        self.next_synthetic_const_ordinal = SYNTHETIC_CONST_ORIGIN_BASE;
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
            self.bind_local(*slot, *value);
            self.seed_cse(*value);
            self.maybe_mark_loop_invariant(*value);
        }

        for value in &entry.stack {
            self.register_existing_value(*value);
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

        if let Some(source) = self.locals.get(&slot).copied() {
            if let Some(materialized) = self.try_materialize_value(record.old_start, source) {
                self.last_local_write = None;
                self.push_stack(materialized);
                return;
            }
        }

        let op_idx = self.push_original(record);
        self.last_local_write = None;
        let expr = if let Some(source) = self.locals.get(&slot).copied() {
            let source_state = self.exprs[source.0].clone();
            self.new_expr_with_origin(
                source_state.ty,
                source_state.origin,
                source_state.const_value,
                source_state.key,
                source_state.def,
                Some(op_idx),
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
                ValueDef::Synthetic,
                Some(op_idx),
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
        let op_idx = self.push_original(record);
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

        let op_idx = self.push_effect_op(record);
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
        let op_idx = self.push_original(record);
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

        let op_idx = self.push_original(record);
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
                    self.builder.push_raw(
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
        let op_idx = self.push_effect_op(record);
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
                    self.builder.push_raw(
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
        let op_idx = self.push_effect_op(record);
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
        let expr = self.new_expr_with_origin(
            type_from_slot(slot.size),
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
        self.push_original(record);
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
        self.push_original(record);
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
            if self.origin_locals.get(&self.exprs[previous.0].origin) == Some(&slot) {
                self.origin_locals.remove(&self.exprs[previous.0].origin);
            }
        }
        self.origin_locals.insert(self.exprs[expr.0].origin, slot);
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
        state.ref_count == 0 && state.removable && state.producer_op.is_some()
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
            "preserved stack prefix must match stack_after metadata",
        );
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
            producer_op,
            materialized_block: producer_op.map(|_| self.block_id),
            materialized_op: producer_op,
            needs_spill: false,
            use_count: 0,
            ref_count: 0,
            removable,
        });
        self.touch_value(id);
        self.latest_by_origin.insert(origin, id);
        id
    }

    fn lookup_cse_source(&self, key: ValueKey) -> Option<ValueRef> {
        let entry = self.cse.get(&key).copied()?;
        (entry.epoch == self.effect_epoch).then_some(entry.expr)
    }

    fn try_materialize_value(&mut self, source_start: usize, source: ValueRef) -> Option<ValueRef> {
        if let Some(value) = self.exprs[source.0].const_value {
            let ordinal = self.allocate_synthetic_const_ordinal();
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
            let op_idx = self.builder.push_raw(
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
                source_state.const_value,
                source_state.key,
                source_state.def,
                Some(op_idx),
                true,
            ));
        }
        self.try_materialize_pure_value(source_start, source)
    }

    fn try_materialize_pure_value(
        &mut self,
        source_start: usize,
        source: ValueRef,
    ) -> Option<ValueRef> {
        let source_state = self.exprs[source.0].clone();
        match source_state.key? {
            ValueKey::Unary { op, input } => {
                let input_expr = self.latest_by_origin.get(&input).copied()?;
                let _ = self.try_materialize_value(source_start, input_expr)?;
                let op_idx = self
                    .builder
                    .push_raw(Some(source_start), unary_op(op)?, Vec::new());
                Some(self.new_expr_with_origin(
                    source_state.ty,
                    source_state.origin,
                    source_state.const_value,
                    source_state.key,
                    source_state.def,
                    Some(op_idx),
                    true,
                ))
            }
            ValueKey::Binary { op, lhs, rhs } => {
                let lhs_expr = self.latest_by_origin.get(&lhs).copied()?;
                let rhs_expr = self.latest_by_origin.get(&rhs).copied()?;
                let _ = self.try_materialize_value(source_start, lhs_expr)?;
                let _ = self.try_materialize_value(source_start, rhs_expr)?;
                let op_idx = self
                    .builder
                    .push_raw(Some(source_start), binary_op(op)?, Vec::new());
                Some(self.new_expr_with_origin(
                    source_state.ty,
                    source_state.origin,
                    source_state.const_value,
                    source_state.key,
                    source_state.def,
                    Some(op_idx),
                    true,
                ))
            }
        }
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
        Some(self.new_expr_with_origin(
            source_state.ty,
            source_state.origin,
            source_state.const_value,
            source_state.key,
            source_state.def,
            Some(op_idx),
            true,
        ))
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

    fn allocate_synthetic_const_ordinal(&mut self) -> usize {
        let ordinal = self.next_synthetic_const_ordinal;
        self.next_synthetic_const_ordinal = self.next_synthetic_const_ordinal.saturating_add(1);
        ordinal
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
    root_value: ValueRef,
    result_size: u32,
    source_start: Option<usize>,
}

fn apply_licm(
    program: &BasicBlockProgram,
    rewrite: &mut FunctionRewrite,
    locals: &mut LocalsData,
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
            let candidates = collect_licm_candidates(
                &rewrite.graph,
                &header_body,
                &rewrite.relower.loop_invariants[candidate_block],
                &effects,
            );
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
                        locals.allocate_temp_slot(type_from_slot(candidate.result_size)),
                        candidate.result_size,
                    );
                    preheader_insert.extend(emit_licm_candidate(candidate, temp, &header_body.ops));
                    new_header.push(BlockOp {
                        source_start: candidate.source_start,
                        op: local_get_op(candidate.result_size),
                        kind: BlockOpKind::LocalGet,
                        operands: vec![BlockOperand::LocalAddr(temp.addr)],
                        inputs: Vec::new(),
                        values: vec![candidate.root_value],
                    });
                    cursor = candidate.end;
                    modified[candidate_block] = true;
                    modified[loop_info.preheader] = true;
                    continue;
                }
                new_header.push(header_body.ops[cursor].clone());
                cursor += 1;
            }

            if preheader_insert.is_empty() {
                continue;
            }
            insert_before_terminator(
                &mut rewrite.relower.block_bodies[loop_info.preheader],
                preheader_insert,
            );
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
    graph: &ValueGraph,
    body: &BlockBody,
    loop_invariants: &LoopInvariantSet,
    effects: &LoopEffects,
) -> Vec<LicmCandidate> {
    let producer_indices = licm_producer_indices(body);
    let origin_values = licm_origin_values(graph);
    let mut by_start = BTreeMap::new();
    for cursor in 0..body.ops.len() {
        if let Some(candidate) = match_licm_candidate(
            graph,
            body,
            loop_invariants,
            &producer_indices,
            &origin_values,
            cursor,
            effects,
        ) {
            by_start.entry(candidate.start).or_insert(candidate);
        }
    }
    by_start.into_values().collect()
}

fn match_licm_candidate(
    graph: &ValueGraph,
    body: &BlockBody,
    loop_invariants: &LoopInvariantSet,
    producer_indices: &HashMap<ValueRef, usize>,
    origin_values: &HashMap<ExprOrigin, ValueRef>,
    cursor: usize,
    effects: &LoopEffects,
) -> Option<LicmCandidate> {
    let root = body.ops.get(cursor)?;
    if root.kind == BlockOpKind::GlobalGet {
        let slot = block_op_global_get_slot(root)?;
        if effects.global_writes.contains(&slot)
            || effects.has_call_barrier
            || !block_op_eligible_for_licm(graph, root)
        {
            return None;
        }
        return Some(LicmCandidate {
            start: cursor,
            end: cursor + 1,
            root_value: block_op_single_result(root)?,
            result_size: slot.size,
            source_start: root.source_start,
        });
    }
    let root_value = block_op_single_result(root)?;
    if graph[root_value.0].is_effect_result()
        || !loop_invariants
            .pure_origins
            .contains(&graph[root_value.0].origin)
        || !block_op_eligible_for_licm(graph, root)
    {
        return None;
    }
    let mut op_indices = BTreeSet::new();
    collect_licm_value_ops(
        graph,
        body,
        root_value,
        producer_indices,
        origin_values,
        loop_invariants,
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

fn selector_value_is_single_use(graph: &ValueGraph, op: &BlockOp, value: ValueRef) -> bool {
    let node = &graph[value.0];
    node.use_count <= 1
        && op.source_start.is_some()
        && !node.is_effect_result()
        && !node.is_block_argument()
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
    if std::ptr::fn_addr_eq(op.op, vm::op_local_get4 as Op) {
        return Some(LocalSlot::new(addr, 4));
    }
    if std::ptr::fn_addr_eq(op.op, vm::op_local_get8 as Op) {
        return Some(LocalSlot::new(addr, 8));
    }
    if std::ptr::fn_addr_eq(op.op, vm::op_local_get16 as Op) {
        return Some(LocalSlot::new(addr, 16));
    }
    None
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
    loop_invariants: &LoopInvariantSet,
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
    if !loop_invariants.pure_origins.contains(&node.origin) {
        return None;
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
                loop_invariants,
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
                loop_invariants,
                effects,
                op_indices,
            )?;
            collect_licm_value_ops(
                graph,
                body,
                rhs,
                producer_indices,
                origin_values,
                loop_invariants,
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
    rewrite: &mut FunctionRewrite,
    _licm_modified: &[bool],
) {
    for block in &program.blocks {
        let body = rewrite.relower.block_bodies[block.id].clone();
        rewrite.relower.block_bodies[block.id] =
            select_block_superinstructions(&rewrite.graph, &body);
    }
}

fn select_block_superinstructions(graph: &ValueGraph, body: &BlockBody) -> BlockBody {
    let mut out = Vec::with_capacity(body.ops.len());
    let mut cursor = 0usize;
    while cursor < body.ops.len() {
        if let Some((fused, consumed)) = match_selector_pattern(graph, body, cursor) {
            out.push(fused);
            cursor += consumed;
            continue;
        }
        out.push(body.ops[cursor].clone());
        cursor += 1;
    }
    BlockBody {
        ops: out,
        terminator: body.terminator.clone(),
    }
}

fn match_selector_pattern(
    graph: &ValueGraph,
    body: &BlockBody,
    cursor: usize,
) -> Option<(BlockOp, usize)> {
    let ops = &body.ops;
    if ops.len() >= cursor + 4
        && block_op_local_get_slot(&ops[cursor]).is_some_and(|slot| slot.size == 4)
        && block_op_i32_const(&ops[cursor + 1]).is_some()
        && matches!(
            ops[cursor + 2].kind,
            BlockOpKind::PureBinary(PureOpKind::I32Add | PureOpKind::I32Sub)
        )
        && ops[cursor + 3].kind == BlockOpKind::LocalSet
        && block_op_single_use(graph, &ops[cursor])
        && block_op_single_use(graph, &ops[cursor + 1])
        && block_op_single_use(graph, &ops[cursor + 2])
        && !next_entry_is_barrier(body, cursor + 4)
    {
        let imm = if matches!(
            ops[cursor + 2].kind,
            BlockOpKind::PureBinary(PureOpKind::I32Sub)
        ) {
            block_op_i32_const(&ops[cursor + 1])?.wrapping_neg()
        } else {
            block_op_i32_const(&ops[cursor + 1])?
        };
        return Some((
            fused_op(
                SelectorPattern::LocalGet4I32ConstAddSet4,
                &ops[cursor],
                vm::op_local_get4_i32_const_add_set4 as Op,
                vec![
                    ops[cursor].operands[0],
                    BlockOperand::I32(imm),
                    ops[cursor + 3].operands[0],
                ],
                Vec::new(),
            ),
            4,
        ));
    }
    if ops.len() >= cursor + 3
        && block_op_local_get_slot(&ops[cursor]).is_some_and(|slot| slot.size == 4)
        && block_op_i32_const(&ops[cursor + 1]).is_some()
        && matches!(
            ops[cursor + 2].kind,
            BlockOpKind::PureBinary(PureOpKind::I32Add | PureOpKind::I32Sub)
        )
        && block_op_single_use(graph, &ops[cursor])
        && block_op_single_use(graph, &ops[cursor + 1])
        && block_op_single_use(graph, &ops[cursor + 2])
        && block_op_single_result(&ops[cursor + 2])
            .is_none_or(|value| !value_feeds_memory_address(body, cursor + 3, value))
        && !next_entry_is_barrier(body, cursor + 3)
    {
        let imm = if matches!(
            ops[cursor + 2].kind,
            BlockOpKind::PureBinary(PureOpKind::I32Sub)
        ) {
            block_op_i32_const(&ops[cursor + 1])?.wrapping_neg()
        } else {
            block_op_i32_const(&ops[cursor + 1])?
        };
        return Some((
            fused_op(
                SelectorPattern::LocalGet4I32ConstAdd,
                &ops[cursor],
                vm::op_local_get4_i32_const_add as Op,
                vec![ops[cursor].operands[0], BlockOperand::I32(imm)],
                ops[cursor + 2].values.clone(),
            ),
            3,
        ));
    }
    if ops.len() >= cursor + 4
        && block_op_local_get_slot(&ops[cursor]).is_some_and(|slot| slot.size == 4)
        && block_op_i32_const(&ops[cursor + 1]).is_some()
        && matches!(
            ops[cursor + 2].kind,
            BlockOpKind::PureBinary(PureOpKind::I32Add | PureOpKind::I32Sub)
        )
        && ops[cursor + 3].kind == BlockOpKind::LocalTee
        && block_op_single_use(graph, &ops[cursor])
        && block_op_single_use(graph, &ops[cursor + 1])
        && block_op_single_use(graph, &ops[cursor + 2])
        && block_op_single_result(&ops[cursor + 2])
            .is_none_or(|value| !value_feeds_memory_address(body, cursor + 4, value))
        && !next_entry_is_barrier(body, cursor + 4)
    {
        let imm = if matches!(
            ops[cursor + 2].kind,
            BlockOpKind::PureBinary(PureOpKind::I32Sub)
        ) {
            block_op_i32_const(&ops[cursor + 1])?.wrapping_neg()
        } else {
            block_op_i32_const(&ops[cursor + 1])?
        };
        return Some((
            fused_op(
                SelectorPattern::LocalGet4I32ConstAddTee4,
                &ops[cursor],
                vm::op_local_get4_i32_const_add_tee4 as Op,
                vec![
                    ops[cursor].operands[0],
                    BlockOperand::I32(imm),
                    ops[cursor + 3].operands[0],
                ],
                ops[cursor + 2].values.clone(),
            ),
            4,
        ));
    }
    if ops.len() >= cursor + 4
        && block_op_local_get_slot(&ops[cursor]).is_some_and(|slot| slot.size == 4)
        && block_op_local_get_slot(&ops[cursor + 1]).is_some_and(|slot| slot.size == 4)
        && matches!(
            ops[cursor + 2].kind,
            BlockOpKind::PureBinary(PureOpKind::I32Add)
        )
        && ops[cursor + 3].kind == BlockOpKind::LocalSet
        && block_op_single_use(graph, &ops[cursor])
        && block_op_single_use(graph, &ops[cursor + 1])
        && block_op_single_use(graph, &ops[cursor + 2])
        && !next_entry_is_barrier(body, cursor + 4)
    {
        return Some((
            fused_op(
                SelectorPattern::LocalGet4LocalGet4I32AddSet4,
                &ops[cursor],
                vm::op_local_get4_local_get4_i32_add_set4 as Op,
                vec![
                    ops[cursor].operands[0],
                    ops[cursor + 1].operands[0],
                    ops[cursor + 3].operands[0],
                ],
                Vec::new(),
            ),
            4,
        ));
    }
    if ops.len() >= cursor + 3
        && block_op_local_get_slot(&ops[cursor]).is_some_and(|slot| slot.size == 4)
        && block_op_local_get_slot(&ops[cursor + 1]).is_some_and(|slot| slot.size == 4)
        && matches!(
            ops[cursor + 2].kind,
            BlockOpKind::PureBinary(PureOpKind::I32Add)
        )
        && block_op_single_use(graph, &ops[cursor])
        && block_op_single_use(graph, &ops[cursor + 1])
        && block_op_single_use(graph, &ops[cursor + 2])
        && block_op_single_result(&ops[cursor + 2])
            .is_none_or(|value| !value_feeds_memory_address(body, cursor + 3, value))
        && !next_entry_is_barrier(body, cursor + 3)
    {
        return Some((
            fused_op(
                SelectorPattern::LocalGet4LocalGet4I32Add,
                &ops[cursor],
                vm::op_local_get4_local_get4_i32_add as Op,
                vec![ops[cursor].operands[0], ops[cursor + 1].operands[0]],
                ops[cursor + 2].values.clone(),
            ),
            3,
        ));
    }
    if ops.len() >= cursor + 4
        && block_op_local_get_slot(&ops[cursor]).is_some_and(|slot| slot.size == 4)
        && block_op_local_get_slot(&ops[cursor + 1]).is_some_and(|slot| slot.size == 4)
        && matches!(
            ops[cursor + 2].kind,
            BlockOpKind::PureBinary(PureOpKind::I32Add)
        )
        && ops[cursor + 3].kind == BlockOpKind::LocalTee
        && block_op_single_use(graph, &ops[cursor])
        && block_op_single_use(graph, &ops[cursor + 1])
        && block_op_single_use(graph, &ops[cursor + 2])
        && block_op_single_result(&ops[cursor + 2])
            .is_none_or(|value| !value_feeds_memory_address(body, cursor + 4, value))
        && !next_entry_is_barrier(body, cursor + 4)
    {
        return Some((
            fused_op(
                SelectorPattern::LocalGet4LocalGet4I32AddTee4,
                &ops[cursor],
                vm::op_local_get4_local_get4_i32_add_tee4 as Op,
                vec![
                    ops[cursor].operands[0],
                    ops[cursor + 1].operands[0],
                    ops[cursor + 3].operands[0],
                ],
                ops[cursor + 2].values.clone(),
            ),
            4,
        ));
    }
    None
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

fn fused_op(
    pattern: SelectorPattern,
    first: &BlockOp,
    op: Op,
    operands: Vec<BlockOperand>,
    values: Vec<ValueRef>,
) -> BlockOp {
    let kind = match pattern {
        SelectorPattern::LocalGet4I32ConstAdd => {
            BlockOpKind::Fused(FusedOpKind::LocalGet4I32ConstAdd)
        }
        SelectorPattern::LocalGet4I32ConstAddSet4 => {
            BlockOpKind::Fused(FusedOpKind::LocalGet4I32ConstAddSet4)
        }
        SelectorPattern::LocalGet4I32ConstAddTee4 => {
            BlockOpKind::Fused(FusedOpKind::LocalGet4I32ConstAddTee4)
        }
        SelectorPattern::LocalGet4LocalGet4I32Add => {
            BlockOpKind::Fused(FusedOpKind::LocalGet4LocalGet4I32Add)
        }
        SelectorPattern::LocalGet4LocalGet4I32AddSet4 => {
            BlockOpKind::Fused(FusedOpKind::LocalGet4LocalGet4I32AddSet4)
        }
        SelectorPattern::LocalGet4LocalGet4I32AddTee4 => {
            BlockOpKind::Fused(FusedOpKind::LocalGet4LocalGet4I32AddTee4)
        }
    };
    BlockOp {
        source_start: first.source_start,
        op,
        kind,
        operands,
        inputs: Vec::new(),
        values,
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

fn verify_explicit_effect_ir(program: &BasicBlockProgram, bodies: &[BlockBody]) -> bool {
    let reachable = reachable_blocks(program, bodies);
    verify_barrier_op_counts(program, &reachable, |source_start, op| {
        count_explicit_ir_ops(bodies, source_start, op)
    })
}

fn verify_relower_preserves_call_ops(
    program: &BasicBlockProgram,
    bodies: &[BlockBody],
    records: &[RecordEmit],
) -> bool {
    let reachable = reachable_blocks(program, bodies);
    verify_barrier_op_counts(program, &reachable, |source_start, op| {
        records
            .iter()
            .filter(|record| {
                record.source_start == Some(source_start) && std::ptr::fn_addr_eq(record.op, op)
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
                if op.kind != BlockOpKind::LocalGet {
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
    records: &[RecordEmit],
) -> bool {
    for body in bodies {
        for op in &body.ops {
            for operand in &op.operands {
                let BlockOperand::SpillValue(source) = *operand else {
                    continue;
                };
                let Some(slot) = spill_plan.slot(source) else {
                    return false;
                };
                let lowered = records
                    .iter()
                    .filter(|record| {
                        record.source_start == op.source_start
                            && std::ptr::fn_addr_eq(record.op, local_get_op(slot.size))
                            && unsafe { record.operands[0].local_addr } == slot.addr
                    })
                    .count();
                if lowered == 0 {
                    return false;
                }
            }
        }
    }

    for (&value, slot) in &spill_plan.slots {
        if !graph[value.0].needs_spill {
            return false;
        }
        let tee_count = records
            .iter()
            .filter(|record| {
                record.source_start.is_none()
                    && std::ptr::fn_addr_eq(record.op, local_tee_op(slot.size))
                    && unsafe { record.operands[0].local_addr } == slot.addr
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
        let header_value =
            graph.ensure_block_argument(7, 1024 + slot.addr as usize, ValType::I32, None, None);
        let pred_value =
            graph.ensure_block_argument(3, 1024 + slot.addr as usize, ValType::I32, None, None);

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
        let header_value =
            graph.ensure_block_argument(7, 1024 + slot.addr as usize, ValType::I32, None, None);
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
}
