use std::{
    collections::{BTreeSet, HashMap, VecDeque},
    hash::{Hash, Hasher},
};

use crate::{
    common::{FuncIdx, FuncType, Instr, LocalsData, Op, Operand, ValType},
    runtime::vm,
};

use super::{
    cfg::{build_program, BasicBlock, BasicBlockProgram, DecodedInstr, InstructionMeta},
    expr::{
        AliasAddress, AliasKey, AliasSpace, ConstValue, EffectBarrier, EffectEpoch, ExprId,
        ExprOrigin, ExprOriginKind, ExprState, HeapVersion, LocalSlot, PureOpKind, ValueDef,
        ValueGraph, ValueKey, ValueRef,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum JoinAliasAddress {
    Const(u32),
    EntryLocal(usize),
    BlockArgument(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct JoinAliasKey {
    space: AliasSpace,
    index: u32,
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

#[derive(Clone, Default)]
struct BlockBody {
    values: Vec<ValueRef>,
    ops: Vec<BlockOp>,
    terminator: Option<BlockTerminator>,
}

#[derive(Clone)]
struct BlockOp {
    source_start: Option<usize>,
    op: Op,
    kind: BlockOpKind,
    operands: Vec<BlockOperand>,
    value: Option<ValueRef>,
}

#[derive(Clone)]
struct BlockTerminator {
    source_start: Option<usize>,
    op: Op,
    kind: BlockTerminatorKind,
    operands: Vec<BlockOperand>,
}

#[derive(Clone, Copy)]
enum BlockOperand {
    I32(i32),
    I64(i64),
    F32(f32),
    F64(f64),
    U32(u32),
    LocalAddr(u32),
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
            BlockOperand::JumpTarget(value) => Operand {
                jump_addr: *value as u32,
            },
            BlockOperand::Raw(operand) => *operand,
        })
        .collect()
}

fn pure_unary_kind_from_op(op: Op) -> Option<PureOpKind> {
    if std::ptr::fn_addr_eq(op, vm::op_i32_eqz as Op) {
        return Some(PureOpKind::I32Eqz);
    }
    if std::ptr::fn_addr_eq(op, vm::op_i64_eqz as Op) {
        return Some(PureOpKind::I64Eqz);
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
        || std::ptr::fn_addr_eq(op, vm::op_i32_load_shared as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i32_load_indexed_local as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i32_load_indexed_shared as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i64_load as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i64_load_shared as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i64_load_indexed_local as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i64_load_indexed_shared as Op)
        || std::ptr::fn_addr_eq(op, vm::op_f32_load as Op)
        || std::ptr::fn_addr_eq(op, vm::op_f32_load_shared as Op)
        || std::ptr::fn_addr_eq(op, vm::op_f32_load_indexed_local as Op)
        || std::ptr::fn_addr_eq(op, vm::op_f32_load_indexed_shared as Op)
        || std::ptr::fn_addr_eq(op, vm::op_f64_load as Op)
        || std::ptr::fn_addr_eq(op, vm::op_f64_load_shared as Op)
        || std::ptr::fn_addr_eq(op, vm::op_f64_load_indexed_local as Op)
        || std::ptr::fn_addr_eq(op, vm::op_f64_load_indexed_shared as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i32_load_local as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i64_load_local as Op)
        || std::ptr::fn_addr_eq(op, vm::op_f32_load_local as Op)
        || std::ptr::fn_addr_eq(op, vm::op_f64_load_local as Op)
}

fn is_memory_store_op(op: Op) -> bool {
    std::ptr::fn_addr_eq(op, vm::op_i32_store as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i32_store_shared as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i32_store_indexed_local as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i32_store_indexed_shared as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i64_store as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i64_store_shared as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i64_store_indexed_local as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i64_store_indexed_shared as Op)
        || std::ptr::fn_addr_eq(op, vm::op_f32_store as Op)
        || std::ptr::fn_addr_eq(op, vm::op_f32_store_shared as Op)
        || std::ptr::fn_addr_eq(op, vm::op_f32_store_indexed_local as Op)
        || std::ptr::fn_addr_eq(op, vm::op_f32_store_indexed_shared as Op)
        || std::ptr::fn_addr_eq(op, vm::op_f64_store as Op)
        || std::ptr::fn_addr_eq(op, vm::op_f64_store_shared as Op)
        || std::ptr::fn_addr_eq(op, vm::op_f64_store_indexed_local as Op)
        || std::ptr::fn_addr_eq(op, vm::op_f64_store_indexed_shared as Op)
        || std::ptr::fn_addr_eq(op, vm::op_i32_store_local as Op)
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
    _funcidx: FuncIdx,
    _functype: &FuncType,
    locals: &mut LocalsData,
    instrs: Vec<Instr>,
    meta: Vec<InstructionMeta>,
) -> Vec<Instr> {
    let Some(program) = build_program(&instrs, meta) else {
        return instrs;
    };
    let mut rewrite = rewrite_program(&program);
    let licm_modified = apply_licm(&program, &mut rewrite, locals);
    select_superinstructions(&program, &mut rewrite, &licm_modified);
    let reachable = reachable_blocks(&program, &rewrite.relower.block_bodies);
    let mut records = Vec::new();
    for block in &program.blocks {
        if reachable[block.id] {
            records.extend(relower_block_body(&rewrite.relower.block_bodies[block.id]));
        }
    }
    if patch_jump_targets(&mut records).is_err() {
        return instrs;
    }
    flatten_records(&records)
}

fn rewrite_program(program: &BasicBlockProgram) -> FunctionRewrite {
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
            rewrite.entries[block_id] = entry.clone();
        }
        let result = pass.run_block(program, program.block(block_id), &entry);
        let exit_changed = !same_state(&pass.exprs, &rewrite.exits[block_id], &result.exit);
        if exit_changed {
            rewrite.exits[block_id] = result.exit;
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
            graph, block_id, ordinal, *ty, &values,
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
            ),
        );
    }

    merge_aliases(graph, block_id, incoming, &mut state);

    state
}

fn merge_aliases(
    graph: &mut ValueGraph,
    block_id: usize,
    incoming: &[BlockEntryState],
    state: &mut BlockEntryState,
) {
    let mut exact_keys = if let Some(first_entry) = incoming.first() {
        first_entry.aliases.keys().copied().collect::<BTreeSet<_>>()
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
            key,
            incoming
                .iter()
                .map(|entry| entry.aliases.get(&key))
                .collect::<Vec<_>>(),
            state,
        );
    }

    let mut join_keys = BTreeSet::new();
    for entry in incoming {
        for key in entry.aliases.keys().copied() {
            if let Some(join_key) = join_alias_key(key) {
                join_keys.insert(join_key);
            }
        }
    }
    for join_key in join_keys {
        if !space_version_stable(join_key.space, incoming, state.heap) {
            continue;
        }
        let merged_key = alias_key_from_join(block_id, join_key);
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
                    (join_alias_key(*key) == Some(join_key)).then_some(value)
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
        merge_alias_value(graph, block_id, merged_key, values, state);
    }
}

fn merge_alias_value(
    graph: &mut ValueGraph,
    block_id: usize,
    key: AliasKey,
    values: Vec<Option<&ValueRef>>,
    state: &mut BlockEntryState,
) {
    let Some(first_value) = values.first().and_then(|value| *value).copied() else {
        return;
    };
    let merged = merge_value_candidates(
        graph,
        block_id,
        alias_ordinal(key),
        graph[first_value.0].ty,
        &values,
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
    incoming.iter().all(|state| match space {
        AliasSpace::Memory => state.heap.memory == merged.memory,
        AliasSpace::Global => state.heap.global == merged.global,
        AliasSpace::Table => state.heap.table == merged.table,
    })
}

fn merge_value_candidates(
    graph: &mut ValueGraph,
    block_id: usize,
    ordinal: usize,
    ty: ValType,
    values: &[Option<&ValueRef>],
) -> ValueRef {
    let Some(first) = values.first().and_then(|value| *value).copied() else {
        return graph.ensure_block_argument(block_id, ordinal, ty, None, None);
    };
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

fn join_alias_key(key: AliasKey) -> Option<JoinAliasKey> {
    let address = match key.address {
        AliasAddress::Const(value) => JoinAliasAddress::Const(value),
        AliasAddress::Origin(origin) if origin.kind == ExprOriginKind::EntryLocal => {
            JoinAliasAddress::EntryLocal(origin.ordinal)
        }
        AliasAddress::Origin(origin) if origin.kind == ExprOriginKind::BlockArgument => {
            JoinAliasAddress::BlockArgument(origin.ordinal)
        }
        _ => return None,
    };
    Some(JoinAliasKey {
        space: key.space,
        index: key.index,
        width: key.width,
        address,
    })
}

fn alias_key_from_join(block_id: usize, key: JoinAliasKey) -> AliasKey {
    let address = match key.address {
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
    };
    AliasKey {
        space: key.space,
        index: key.index,
        width: key.width,
        address,
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

fn same_value_vec(graph: &ValueGraph, lhs: &[ValueRef], rhs: &[ValueRef]) -> bool {
    lhs.len() == rhs.len()
        && lhs
            .iter()
            .zip(rhs.iter())
            .all(|(lhs, rhs)| same_value(graph, *lhs, *rhs))
}

fn same_value_map<K: Eq + std::hash::Hash + Copy>(
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
        materialized_op: None,
        use_count: 0,
        ref_count: 0,
        removable: false,
    });
    value
}

fn block_body_is_empty(body: &BlockBody) -> bool {
    body.values.is_empty() && body.ops.is_empty() && body.terminator.is_none()
}

fn relower_block_body(body: &BlockBody) -> Vec<RecordEmit> {
    let mut records = Vec::with_capacity(body.ops.len() + usize::from(body.terminator.is_some()));
    records.extend(body.ops.iter().map(relower_block_op));
    if let Some(terminator) = &body.terminator {
        records.push(relower_block_terminator(terminator));
    }
    records
}

fn relower_block_op(op: &BlockOp) -> RecordEmit {
    RecordEmit {
        source_start: op.source_start,
        op: op.op,
        operands: block_operands_to_raw(&op.operands),
    }
}

fn relower_block_terminator(terminator: &BlockTerminator) -> RecordEmit {
    RecordEmit {
        source_start: terminator.source_start,
        op: terminator.op,
        operands: block_operands_to_raw(&terminator.operands),
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
struct StoreWrite {
    op_idx: usize,
}

#[derive(Clone, Copy)]
struct CseEntry {
    expr: ValueRef,
    epoch: EffectEpoch,
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
        aliases.sort_by_key(|(key, _)| (key.space as u8, key.index, key.width));
        for (key, value) in aliases {
            self.register_existing_value(*value);
            self.aliases.insert(*key, *value);
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

        let op_idx = self.push_original(record);
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
            ValueDef::Instr,
            Some(op_idx),
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
        self.push_original(record);
        self.reset_stack_from_snapshot(ordinal, &record.stack_after);
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
        self.push_original(record);
        self.reset_stack_from_snapshot(ordinal, &record.stack_after);
    }

    fn visit_global_get(&mut self, record: &DecodedInstr, slot: LocalSlot, ordinal: usize) {
        self.last_local_write = None;
        self.bump_effect_epoch();
        clear_store_space_on_load(&mut self.last_store, AliasSpace::Global);
        let key = global_alias_key(slot);
        if let Some(source) = self.aliases.get(&key).copied() {
            if let Some(materialized) = self.try_materialize_value(record.old_start, source) {
                self.push_stack(materialized);
                return;
            }
        }
        let op_idx = self.push_original(record);
        let expr = self.new_expr_with_origin(
            type_from_slot(slot.size),
            ExprOrigin {
                block_id: self.block_id,
                ordinal,
                kind: ExprOriginKind::GlobalValue,
            },
            None,
            Some(ValueKey::GlobalGet { slot }),
            ValueDef::Instr,
            Some(op_idx),
            true,
        );
        self.aliases.insert(key, expr);
        self.push_stack(expr);
    }

    fn visit_global_set(&mut self, record: &DecodedInstr, slot: LocalSlot) {
        let Some(value) = self.pop_stack() else {
            self.emit_barrier(record, 0);
            return;
        };
        self.last_local_write = None;
        self.bump_effect_epoch();
        let key = global_alias_key(slot);
        if self
            .aliases
            .get(&key)
            .is_some_and(|current| same_expr(&self.exprs[current.0], &self.exprs[value.0]))
        {
            let _ = self.try_remove_expr(value);
            return;
        }
        clear_alias_space_rewrite(&mut self.aliases, &mut self.last_store, AliasSpace::Global);
        let op_idx = self.push_original(record);
        if let Some(previous) = self.last_store.insert(key, StoreWrite { op_idx }) {
            self.builder.remove(previous.op_idx);
        }
        self.aliases.insert(key, value);
        self.heap.global = self.heap.global.saturating_add(1);
    }

    fn visit_table_get(&mut self, record: &DecodedInstr, tableidx: u32, ordinal: usize) {
        let Some(index) = self.pop_stack() else {
            self.emit_barrier(record, ordinal);
            return;
        };
        self.last_local_write = None;
        self.bump_effect_epoch();
        clear_store_space_on_load(&mut self.last_store, AliasSpace::Table);
        let Some(address) = self.canonical_alias_address(index) else {
            let op_idx = self.push_original(record);
            let expr = self.new_expr_with_origin(
                ValType::FuncRef,
                ExprOrigin {
                    block_id: self.block_id,
                    ordinal,
                    kind: ExprOriginKind::TableValue,
                },
                None,
                None,
                ValueDef::Instr,
                Some(op_idx),
                true,
            );
            self.push_stack(expr);
            return;
        };
        let key = AliasKey {
            space: AliasSpace::Table,
            index: tableidx,
            width: 4,
            address,
        };
        if let Some(source) = self.aliases.get(&key).copied() {
            if let Some(materialized) = self.try_materialize_value(record.old_start, source) {
                self.push_stack(materialized);
                return;
            }
        }
        let op_idx = self.push_original(record);
        let expr = self.new_expr_with_origin(
            ValType::FuncRef,
            ExprOrigin {
                block_id: self.block_id,
                ordinal,
                kind: ExprOriginKind::TableValue,
            },
            None,
            Some(ValueKey::TableGet {
                tableidx,
                index: self.exprs[index.0].origin,
            }),
            ValueDef::Instr,
            Some(op_idx),
            false,
        );
        self.aliases.insert(key, expr);
        self.push_stack(expr);
    }

    fn visit_table_set(&mut self, record: &DecodedInstr, tableidx: u32) {
        let Some(value) = self.pop_stack() else {
            self.emit_barrier(record, 0);
            return;
        };
        let Some(index) = self.pop_stack() else {
            self.incref(value);
            self.push_stack(value);
            self.emit_barrier(record, 0);
            return;
        };
        self.last_local_write = None;
        self.bump_effect_epoch();
        clear_alias_space_rewrite(&mut self.aliases, &mut self.last_store, AliasSpace::Table);
        let op_idx = self.push_original(record);
        if let Some(address) = self.canonical_alias_address(index) {
            let key = AliasKey {
                space: AliasSpace::Table,
                index: tableidx,
                width: 4,
                address,
            };
            if let Some(previous) = self.last_store.insert(key, StoreWrite { op_idx }) {
                self.builder.remove(previous.op_idx);
            }
            self.aliases.insert(key, value);
        }
        self.heap.table = self.heap.table.saturating_add(1);
    }

    fn visit_memory_load(&mut self, record: &DecodedInstr, access: MemoryAccess, ordinal: usize) {
        let Some(address) = self.pop_stack() else {
            self.emit_barrier(record, ordinal);
            return;
        };
        self.last_local_write = None;
        self.bump_effect_epoch();
        let Some(key) = self.memory_alias_key(access, address) else {
            let op_idx = self.push_original(record);
            let expr = self.new_expr_with_origin(
                access.ty,
                ExprOrigin {
                    block_id: self.block_id,
                    ordinal,
                    kind: ExprOriginKind::MemoryValue,
                },
                None,
                None,
                ValueDef::Instr,
                Some(op_idx),
                true,
            );
            self.push_stack(expr);
            clear_store_space_on_load(&mut self.last_store, AliasSpace::Memory);
            return;
        };
        clear_store_space_on_load(&mut self.last_store, AliasSpace::Memory);
        if let Some(source) = self.aliases.get(&key).copied() {
            if let Some(materialized) = self.try_materialize_value(record.old_start, source) {
                self.push_stack(materialized);
                return;
            }
        }
        let op_idx = self.push_original(record);
        let expr = self.new_expr_with_origin(
            access.ty,
            ExprOrigin {
                block_id: self.block_id,
                ordinal,
                kind: ExprOriginKind::MemoryValue,
            },
            None,
            Some(ValueKey::MemoryLoad(key)),
            ValueDef::Instr,
            Some(op_idx),
            false,
        );
        self.aliases.insert(key, expr);
        self.push_stack(expr);
    }

    fn visit_memory_store(&mut self, record: &DecodedInstr, access: MemoryAccess, _ordinal: usize) {
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
        let op_idx = self.push_original(record);
        if let Some(key) = self.memory_alias_key(access, address) {
            if self
                .aliases
                .get(&key)
                .is_some_and(|current| same_expr(&self.exprs[current.0], &self.exprs[value.0]))
            {
                self.builder.remove(op_idx);
                let _ = self.try_remove_expr(value);
                return;
            }
            if let Some(previous) = self.last_store.insert(key, StoreWrite { op_idx }) {
                self.builder.remove(previous.op_idx);
            }
            self.aliases.insert(key, value);
        }
        self.heap.memory = self.heap.memory.saturating_add(1);
    }

    fn emit_barrier(&mut self, record: &DecodedInstr, ordinal: usize) {
        self.last_local_write = None;
        let barrier = effect_barrier(record);
        self.push_original(record);
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
        self.reset_stack_from_snapshot(ordinal, &record.stack_after);
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
        aliases.sort_by_key(|(key, _)| (key.space as u8, key.index, key.width));
        for (key, expr) in aliases {
            state.aliases.insert(*key, *expr);
        }

        state
    }

    fn build_block_body(&self) -> BlockBody {
        let mut expr_by_op = HashMap::new();
        for (expr_idx, expr) in self.exprs.nodes.iter().enumerate() {
            if let Some(op_idx) = expr.materialized_op {
                expr_by_op.entry(op_idx).or_insert(ExprId(expr_idx));
            }
        }
        let mut body = BlockBody::default();
        for (op_idx, entry) in self.builder.live_entries() {
            let value = expr_by_op.get(&op_idx).copied();
            if let Some(value) = value {
                body.values.push(value);
            }
            match entry.kind {
                PendingBlockEntryKind::Op(kind) => body.ops.push(BlockOp {
                    source_start: entry.source_start,
                    op: entry.op,
                    kind,
                    operands: entry.operands.clone(),
                    value,
                }),
                PendingBlockEntryKind::Terminator(kind) => {
                    body.terminator = Some(BlockTerminator {
                        source_start: entry.source_start,
                        op: entry.op,
                        kind,
                        operands: entry.operands.clone(),
                    });
                }
            }
        }
        body
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
            ValueKey::MemoryLoad(_) | ValueKey::GlobalGet { .. } | ValueKey::TableGet { .. } => {
                false
            }
        }
    }

    fn reset_stack_from_snapshot(
        &mut self,
        ordinal: usize,
        snapshot: &crate::parser::core::type_checker::StackSnapshot,
    ) {
        let drained = self.stack.drain(..).collect::<Vec<_>>();
        for expr in drained {
            self.decref(expr);
        }
        for (result_idx, ty) in snapshot.types.iter().enumerate() {
            let expr = self.new_expr_with_origin(
                *ty,
                ExprOrigin {
                    block_id: self.block_id,
                    ordinal: instr_result_origin_ordinal(ordinal, result_idx),
                    kind: ExprOriginKind::InstrResult,
                },
                None,
                None,
                ValueDef::Instr,
                None,
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
            materialized_op: producer_op,
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
            ValueKey::MemoryLoad(_) | ValueKey::GlobalGet { .. } | ValueKey::TableGet { .. } => {
                None
            }
        }
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
            Some(ValueKey::MemoryLoad(_))
            | Some(ValueKey::GlobalGet { .. })
            | Some(ValueKey::TableGet { .. })
            | None => false,
        }
    }

    fn canonical_alias_address(&self, expr: ValueRef) -> Option<AliasAddress> {
        let value = &self.exprs[expr.0];
        value
            .const_value
            .and_then(|value| match value {
                ConstValue::I32(value) => Some(AliasAddress::Const(value as u32)),
                _ => None,
            })
            .or_else(|| {
                self.origin_locals
                    .get(&value.origin)
                    .map(|slot| AliasAddress::Origin(local_alias_origin(self.block_id, *slot)))
            })
            .or_else(|| {
                value
                    .block_argument()
                    .and_then(|id| self.exprs.block_argument(id))
                    .map(|arg| {
                        AliasAddress::Origin(ExprOrigin {
                            block_id: arg.block_id,
                            ordinal: arg.ordinal,
                            kind: ExprOriginKind::BlockArgument,
                        })
                    })
            })
            .or(Some(AliasAddress::Origin(value.origin)))
    }

    fn memory_alias_key(&self, access: MemoryAccess, address: ValueRef) -> Option<AliasKey> {
        Some(AliasKey {
            space: AliasSpace::Memory,
            index: access.memidx,
            width: access.width,
            address: self.canonical_alias_address(address)?,
        })
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

fn clear_store_space_on_load(stores: &mut HashMap<AliasKey, StoreWrite>, space: AliasSpace) {
    stores.retain(|key, _| key.space != space);
}

fn same_expr(lhs: &ExprState, rhs: &ExprState) -> bool {
    lhs.ty == rhs.ty
        && lhs.origin == rhs.origin
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
            let default_invariants = LoopInvariantSet::default();
            let loop_invariants = rewrite
                .relower
                .loop_invariants
                .get(candidate_block)
                .unwrap_or(&default_invariants);
            let candidates =
                collect_licm_candidates(&rewrite.graph, &header_body, &effects, loop_invariants);
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
                        value: None,
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
    effects: &LoopEffects,
    loop_invariants: &LoopInvariantSet,
) -> Vec<LicmCandidate> {
    let mut candidates = Vec::new();
    let mut cursor = 0usize;
    while cursor < body.ops.len() {
        if let Some(candidate) = match_licm_candidate(graph, body, loop_invariants, cursor, effects)
        {
            cursor = candidate.end;
            candidates.push(candidate);
            continue;
        }
        cursor += 1;
    }
    candidates
}

fn match_licm_candidate(
    graph: &ValueGraph,
    body: &BlockBody,
    loop_invariants: &LoopInvariantSet,
    cursor: usize,
    effects: &LoopEffects,
) -> Option<LicmCandidate> {
    if let Some(slot) = block_op_local_get_slot(body.ops.get(cursor)?) {
        if block_op_i32_const(body.ops.get(cursor + 1)?).is_some() {
            let op = body.ops.get(cursor + 2)?;
            if matches!(
                op.kind,
                BlockOpKind::PureBinary(PureOpKind::I32Add | PureOpKind::I32Sub)
            ) {
                if effects.local_writes.contains(&slot)
                    || !block_op_single_use(graph, body.ops.get(cursor)?)
                    || !block_op_single_use(graph, body.ops.get(cursor + 1)?)
                    || !block_op_single_use(graph, body.ops.get(cursor + 2)?)
                    || !block_op_has_invariant_origin(
                        graph,
                        loop_invariants,
                        body.ops.get(cursor + 2)?,
                    )
                {
                    return None;
                }
                return Some(LicmCandidate {
                    start: cursor,
                    end: cursor + 3,
                    result_size: 4,
                    source_start: body.ops[cursor].source_start,
                });
            }
        }
        if let Some(rhs) = body.ops.get(cursor + 1).and_then(block_op_local_get_slot) {
            if body
                .ops
                .get(cursor + 2)
                .is_some_and(|op| matches!(op.kind, BlockOpKind::PureBinary(PureOpKind::I32Add)))
            {
                if effects.local_writes.contains(&slot)
                    || effects.local_writes.contains(&rhs)
                    || !block_op_single_use(graph, body.ops.get(cursor)?)
                    || !block_op_single_use(graph, body.ops.get(cursor + 1)?)
                    || !block_op_single_use(graph, body.ops.get(cursor + 2)?)
                    || !block_op_has_invariant_origin(
                        graph,
                        loop_invariants,
                        body.ops.get(cursor + 2)?,
                    )
                {
                    return None;
                }
                return Some(LicmCandidate {
                    start: cursor,
                    end: cursor + 3,
                    result_size: 4,
                    source_start: body.ops[cursor].source_start,
                });
            }
        }
    }

    if let Some(slot) = block_op_global_get_slot(body.ops.get(cursor)?) {
        if effects.has_call_barrier || effects.global_writes.contains(&slot) {
            return None;
        }
        return Some(LicmCandidate {
            start: cursor,
            end: cursor + 1,
            result_size: slot.size,
            source_start: body.ops[cursor].source_start,
        });
    }

    if effects.has_call_barrier || effects.has_memory_mutation {
        return None;
    }
    let address = body.ops.get(cursor)?;
    let load = body
        .ops
        .get(cursor + 1)
        .and_then(block_op_memory_load_access)?;
    let _address = if let Some(slot) = block_op_local_get_slot(address) {
        if effects.local_writes.contains(&slot) {
            return None;
        }
        AliasAddress::Origin(ExprOrigin {
            block_id: 0,
            ordinal: slot.addr as usize,
            kind: ExprOriginKind::EntryLocal,
        })
    } else if let Some(value) = block_op_i32_const(address) {
        AliasAddress::Const(value as u32)
    } else {
        return None;
    };
    Some(LicmCandidate {
        start: cursor,
        end: cursor + 2,
        result_size: load.ty.stack_size().u32(),
        source_start: body.ops[cursor].source_start,
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
            op.value = None;
            op
        })
        .collect::<Vec<_>>();
    out.push(BlockOp {
        source_start: None,
        op: local_set_op(temp.size),
        kind: BlockOpKind::LocalSet,
        operands: vec![BlockOperand::LocalAddr(temp.addr)],
        value: None,
    });
    out
}

fn insert_before_terminator(body: &mut BlockBody, mut insert: Vec<BlockOp>) {
    body.ops.append(&mut insert);
}

fn block_op_single_use(graph: &ValueGraph, op: &BlockOp) -> bool {
    op.value
        .is_some_and(|value| value_is_single_use(graph, value))
}

fn value_is_single_use(graph: &ValueGraph, value: ValueRef) -> bool {
    let node = &graph[value.0];
    node.use_count <= 1 && !node.is_block_argument()
}

fn block_op_has_invariant_origin(
    graph: &ValueGraph,
    loop_invariants: &LoopInvariantSet,
    op: &BlockOp,
) -> bool {
    op.value
        .map(|value| graph[value.0].origin)
        .is_some_and(|origin| loop_invariants.pure_origins.contains(&origin))
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
    let BlockOperand::U32(index) = *op.operands.first()? else {
        return None;
    };
    if std::ptr::fn_addr_eq(op.op, vm::op_global_get4 as Op) {
        return Some(LocalSlot::new(index, 4));
    }
    if std::ptr::fn_addr_eq(op.op, vm::op_global_get8 as Op) {
        return Some(LocalSlot::new(index, 8));
    }
    if std::ptr::fn_addr_eq(op.op, vm::op_global_get16 as Op) {
        return Some(LocalSlot::new(index, 16));
    }
    None
}

fn block_op_i32_const(op: &BlockOp) -> Option<i32> {
    matches!(op.kind, BlockOpKind::Const).then(|| match op.operands.first()? {
        BlockOperand::I32(value) => Some(*value),
        _ => None,
    })?
}

fn block_op_memory_load_access(op: &BlockOp) -> Option<MemoryAccess> {
    if op.kind != BlockOpKind::MemoryLoad {
        return None;
    }
    if std::ptr::fn_addr_eq(op.op, vm::op_i32_load_local as Op)
        || std::ptr::fn_addr_eq(op.op, vm::op_i32_load as Op)
    {
        return Some(MemoryAccess {
            memidx: 0,
            width: 4,
            ty: ValType::I32,
        });
    }
    if std::ptr::fn_addr_eq(op.op, vm::op_i64_load_local as Op)
        || std::ptr::fn_addr_eq(op.op, vm::op_i64_load as Op)
    {
        return Some(MemoryAccess {
            memidx: 0,
            width: 8,
            ty: ValType::I64,
        });
    }
    if std::ptr::fn_addr_eq(op.op, vm::op_f32_load_local as Op)
        || std::ptr::fn_addr_eq(op.op, vm::op_f32_load as Op)
    {
        return Some(MemoryAccess {
            memidx: 0,
            width: 4,
            ty: ValType::F32,
        });
    }
    if std::ptr::fn_addr_eq(op.op, vm::op_f64_load_local as Op)
        || std::ptr::fn_addr_eq(op.op, vm::op_f64_load as Op)
    {
        return Some(MemoryAccess {
            memidx: 0,
            width: 8,
            ty: ValType::F64,
        });
    }
    None
}

#[derive(Clone, Copy)]
struct MemoryAccess {
    memidx: u32,
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
        if let Some((fused, consumed)) = match_selector_pattern(graph, &body.ops, cursor) {
            out.push(fused);
            cursor += consumed;
            continue;
        }
        out.push(body.ops[cursor].clone());
        cursor += 1;
    }
    BlockBody {
        values: body.values.clone(),
        ops: out,
        terminator: body.terminator.clone(),
    }
}

fn match_selector_pattern(
    graph: &ValueGraph,
    ops: &[BlockOp],
    cursor: usize,
) -> Option<(BlockOp, usize)> {
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
        && !next_op_is_call_like(ops, cursor + 4)
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
            ),
            4,
        ));
    }
    if ops.len() >= cursor + 3
        && block_op_local_get_slot(&ops[cursor]).is_some_and(|slot| slot.size == 4)
        && block_op_i32_const(&ops[cursor + 1]).is_some()
        && matches!(
            ops[cursor + 2].kind,
            BlockOpKind::PureBinary(PureOpKind::I32Add)
        )
        && block_op_single_use(graph, &ops[cursor])
        && block_op_single_use(graph, &ops[cursor + 1])
        && block_op_single_use(graph, &ops[cursor + 2])
        && !next_op_is_call_like(ops, cursor + 3)
    {
        return Some((
            fused_op(
                SelectorPattern::LocalGet4I32ConstAdd,
                &ops[cursor],
                vm::op_local_get4_i32_const_add as Op,
                vec![ops[cursor].operands[0], ops[cursor + 1].operands[0]],
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
        && !next_op_is_call_like(ops, cursor + 4)
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
        && !next_op_is_call_like(ops, cursor + 4)
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
        && !next_op_is_call_like(ops, cursor + 3)
    {
        return Some((
            fused_op(
                SelectorPattern::LocalGet4LocalGet4I32Add,
                &ops[cursor],
                vm::op_local_get4_local_get4_i32_add as Op,
                vec![ops[cursor].operands[0], ops[cursor + 1].operands[0]],
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
        && !next_op_is_call_like(ops, cursor + 4)
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
            ),
            4,
        ));
    }
    None
}

fn next_op_is_call_like(ops: &[BlockOp], idx: usize) -> bool {
    ops.get(idx)
        .is_some_and(|op| matches!(op.kind, BlockOpKind::CallLike))
}

fn fused_op(
    pattern: SelectorPattern,
    first: &BlockOp,
    op: Op,
    operands: Vec<BlockOperand>,
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
        value: None,
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
        || record.op_eq(vm::op_i32_load_shared as Op)
        || record.op_eq(vm::op_i32_load_indexed_local as Op)
        || record.op_eq(vm::op_i32_load_indexed_shared as Op)
    {
        return Some(MemoryAccess {
            memidx: memory_index(record),
            width: 4,
            ty: ValType::I32,
        });
    }
    if record.op_eq(vm::op_i64_load as Op)
        || record.op_eq(vm::op_i64_load_shared as Op)
        || record.op_eq(vm::op_i64_load_indexed_local as Op)
        || record.op_eq(vm::op_i64_load_indexed_shared as Op)
    {
        return Some(MemoryAccess {
            memidx: memory_index(record),
            width: 8,
            ty: ValType::I64,
        });
    }
    if record.op_eq(vm::op_f32_load as Op)
        || record.op_eq(vm::op_f32_load_shared as Op)
        || record.op_eq(vm::op_f32_load_indexed_local as Op)
        || record.op_eq(vm::op_f32_load_indexed_shared as Op)
    {
        return Some(MemoryAccess {
            memidx: memory_index(record),
            width: 4,
            ty: ValType::F32,
        });
    }
    if record.op_eq(vm::op_f64_load as Op)
        || record.op_eq(vm::op_f64_load_shared as Op)
        || record.op_eq(vm::op_f64_load_indexed_local as Op)
        || record.op_eq(vm::op_f64_load_indexed_shared as Op)
    {
        return Some(MemoryAccess {
            memidx: memory_index(record),
            width: 8,
            ty: ValType::F64,
        });
    }
    None
}

fn decode_memory_store(record: &DecodedInstr) -> Option<MemoryAccess> {
    if record.op_eq(vm::op_i32_store as Op)
        || record.op_eq(vm::op_i32_store_shared as Op)
        || record.op_eq(vm::op_i32_store_indexed_local as Op)
        || record.op_eq(vm::op_i32_store_indexed_shared as Op)
    {
        return Some(MemoryAccess {
            memidx: memory_index(record),
            width: 4,
            ty: ValType::I32,
        });
    }
    if record.op_eq(vm::op_i64_store as Op)
        || record.op_eq(vm::op_i64_store_shared as Op)
        || record.op_eq(vm::op_i64_store_indexed_local as Op)
        || record.op_eq(vm::op_i64_store_indexed_shared as Op)
    {
        return Some(MemoryAccess {
            memidx: memory_index(record),
            width: 8,
            ty: ValType::I64,
        });
    }
    if record.op_eq(vm::op_f32_store as Op)
        || record.op_eq(vm::op_f32_store_shared as Op)
        || record.op_eq(vm::op_f32_store_indexed_local as Op)
        || record.op_eq(vm::op_f32_store_indexed_shared as Op)
    {
        return Some(MemoryAccess {
            memidx: memory_index(record),
            width: 4,
            ty: ValType::F32,
        });
    }
    if record.op_eq(vm::op_f64_store as Op)
        || record.op_eq(vm::op_f64_store_shared as Op)
        || record.op_eq(vm::op_f64_store_indexed_local as Op)
        || record.op_eq(vm::op_f64_store_indexed_shared as Op)
    {
        return Some(MemoryAccess {
            memidx: memory_index(record),
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
    if record.op_eq(vm::op_i32_eqz) {
        return Some(PureOpKind::I32Eqz);
    }
    if record.op_eq(vm::op_i64_eqz) {
        return Some(PureOpKind::I64Eqz);
    }
    None
}

fn decode_pure_binary(record: &DecodedInstr) -> Option<PureOpKind> {
    if record.op_eq(vm::op_i32_add) {
        return Some(PureOpKind::I32Add);
    }
    if record.op_eq(vm::op_i32_sub) {
        return Some(PureOpKind::I32Sub);
    }
    if record.op_eq(vm::op_i32_mul) {
        return Some(PureOpKind::I32Mul);
    }
    if record.op_eq(vm::op_i32_and) {
        return Some(PureOpKind::I32And);
    }
    if record.op_eq(vm::op_i32_or) {
        return Some(PureOpKind::I32Or);
    }
    if record.op_eq(vm::op_i32_xor) {
        return Some(PureOpKind::I32Xor);
    }
    if record.op_eq(vm::op_i32_eq) {
        return Some(PureOpKind::I32Eq);
    }
    if record.op_eq(vm::op_i32_ne) {
        return Some(PureOpKind::I32Ne);
    }
    if record.op_eq(vm::op_i32_lt_s) {
        return Some(PureOpKind::I32LtS);
    }
    if record.op_eq(vm::op_i32_lt_u) {
        return Some(PureOpKind::I32LtU);
    }
    if record.op_eq(vm::op_i32_gt_s) {
        return Some(PureOpKind::I32GtS);
    }
    if record.op_eq(vm::op_i32_gt_u) {
        return Some(PureOpKind::I32GtU);
    }
    if record.op_eq(vm::op_i32_le_s) {
        return Some(PureOpKind::I32LeS);
    }
    if record.op_eq(vm::op_i32_le_u) {
        return Some(PureOpKind::I32LeU);
    }
    if record.op_eq(vm::op_i32_ge_s) {
        return Some(PureOpKind::I32GeS);
    }
    if record.op_eq(vm::op_i32_ge_u) {
        return Some(PureOpKind::I32GeU);
    }
    if record.op_eq(vm::op_i64_add) {
        return Some(PureOpKind::I64Add);
    }
    if record.op_eq(vm::op_i64_sub) {
        return Some(PureOpKind::I64Sub);
    }
    if record.op_eq(vm::op_f32_add) {
        return Some(PureOpKind::F32Add);
    }
    if record.op_eq(vm::op_f32_sub) {
        return Some(PureOpKind::F32Sub);
    }
    if record.op_eq(vm::op_f32_mul) {
        return Some(PureOpKind::F32Mul);
    }
    if record.op_eq(vm::op_f32_div) {
        return Some(PureOpKind::F32Div);
    }
    if record.op_eq(vm::op_f32_eq) {
        return Some(PureOpKind::F32Eq);
    }
    if record.op_eq(vm::op_f32_ne) {
        return Some(PureOpKind::F32Ne);
    }
    if record.op_eq(vm::op_f32_lt) {
        return Some(PureOpKind::F32Lt);
    }
    if record.op_eq(vm::op_f32_gt) {
        return Some(PureOpKind::F32Gt);
    }
    if record.op_eq(vm::op_f32_le) {
        return Some(PureOpKind::F32Le);
    }
    if record.op_eq(vm::op_f32_ge) {
        return Some(PureOpKind::F32Ge);
    }
    if record.op_eq(vm::op_f64_add) {
        return Some(PureOpKind::F64Add);
    }
    if record.op_eq(vm::op_f64_sub) {
        return Some(PureOpKind::F64Sub);
    }
    if record.op_eq(vm::op_f64_mul) {
        return Some(PureOpKind::F64Mul);
    }
    if record.op_eq(vm::op_f64_div) {
        return Some(PureOpKind::F64Div);
    }
    if record.op_eq(vm::op_f64_eq) {
        return Some(PureOpKind::F64Eq);
    }
    if record.op_eq(vm::op_f64_ne) {
        return Some(PureOpKind::F64Ne);
    }
    if record.op_eq(vm::op_f64_lt) {
        return Some(PureOpKind::F64Lt);
    }
    if record.op_eq(vm::op_f64_gt) {
        return Some(PureOpKind::F64Gt);
    }
    if record.op_eq(vm::op_f64_le) {
        return Some(PureOpKind::F64Le);
    }
    if record.op_eq(vm::op_f64_ge) {
        return Some(PureOpKind::F64Ge);
    }
    None
}

fn unary_op(op: PureOpKind) -> Option<Op> {
    match op {
        PureOpKind::I32Eqz => Some(vm::op_i32_eqz as Op),
        PureOpKind::I64Eqz => Some(vm::op_i64_eqz as Op),
        _ => None,
    }
}

fn binary_op(op: PureOpKind) -> Option<Op> {
    match op {
        PureOpKind::I32Eqz | PureOpKind::I64Eqz => None,
        PureOpKind::I32Add => Some(vm::op_i32_add as Op),
        PureOpKind::I32Sub => Some(vm::op_i32_sub as Op),
        PureOpKind::I32Mul => Some(vm::op_i32_mul as Op),
        PureOpKind::I32And => Some(vm::op_i32_and as Op),
        PureOpKind::I32Or => Some(vm::op_i32_or as Op),
        PureOpKind::I32Xor => Some(vm::op_i32_xor as Op),
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
        || record.op_eq(vm::op_mem_grow_local as Op)
        || record.op_eq(vm::op_mem_grow_shared as Op)
        || record.op_eq(vm::op_mem_grow_indexed_local as Op)
        || record.op_eq(vm::op_mem_grow_indexed_shared as Op)
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

fn global_alias_key(slot: LocalSlot) -> AliasKey {
    AliasKey {
        space: AliasSpace::Global,
        index: slot.addr,
        width: slot.size as u8,
        address: AliasAddress::Const(0),
    }
}

fn local_alias_origin(block_id: usize, slot: LocalSlot) -> ExprOrigin {
    ExprOrigin {
        block_id,
        ordinal: slot.addr as usize,
        kind: ExprOriginKind::EntryLocal,
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

fn unary_output_type(op: PureOpKind) -> ValType {
    match op {
        PureOpKind::I32Eqz | PureOpKind::I64Eqz => ValType::I32,
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
        | PureOpKind::I32Xor => ValType::I32,
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
        PureOpKind::I64Add | PureOpKind::I64Sub => ValType::I64,
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
        (PureOpKind::I64Eqz, ConstValue::I64(value)) => Some(ConstValue::I32((value == 0) as i32)),
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
        | (PureOpKind::I64Sub, _, Some(ConstValue::I64(0))) => Some((lhs, rhs)),
        (PureOpKind::I64Add, Some(ConstValue::I64(0)), _) => Some((rhs, lhs)),
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
            materialized_op: None,
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
            materialized_op: None,
            use_count: 0,
            ref_count: 0,
            removable: false,
        });
        rhs.aliases.insert(key_rhs, rhs_value);

        let merged = merge_states(&mut graph, 7, &first, &[lhs, rhs]);
        let merged_key = AliasKey {
            space: AliasSpace::Memory,
            index: 0,
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
    fn merge_states_creates_first_class_block_argument_values() {
        let first = DecodedInstr {
            old_start: 0,
            op: vm::op_end as Op,
            operands: Vec::new(),
            stack_before: snapshot(&[ValType::I32]),
            stack_after: empty_snapshot(),
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
            materialized_op: None,
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
            materialized_op: None,
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
}
