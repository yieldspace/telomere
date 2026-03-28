use std::collections::HashMap;
use std::ptr;

use crate::common::{
    decode_local_binop32_kind, decode_local_binop64_kind, decode_local_cmp32_kind,
    decode_local_cmp64_kind, decode_local_unary32_kind, decode_local_unary64_kind, CallRecipeRef,
    Instr, LocalFastConstKind, LocalFastRhsShape, Op, Operand,
};
use crate::{
    common::{BlockReturn, LoopParam, MemArg},
    runtime::vm,
};

use super::pass::{specialized_memory_family, SpecializedMemoryFamily};

#[cfg(test)]
#[derive(Clone)]
pub(crate) struct RecordEmit {
    pub(crate) source_start: Option<usize>,
    pub(crate) op: Op,
    pub(crate) operands: Vec<Operand>,
}

#[cfg(test)]
impl RecordEmit {
    pub(crate) fn len(&self) -> usize {
        1 + self.operands.len()
    }
}

#[derive(Clone)]
pub(crate) struct PackedOpStream {
    const_pool: Vec<ConstPoolValue>,
    pub(crate) ops: Vec<PackedOp>,
}

impl PackedOpStream {
    pub(crate) fn instr_len(&self) -> usize {
        self.ops.iter().map(PackedOp::len).sum()
    }
}

#[derive(Clone)]
pub(crate) struct PackedOp {
    #[allow(dead_code)]
    pub(crate) source_start: Option<usize>,
    pub(crate) op: Op,
    pub(crate) operands: Vec<PackedOperand>,
}

impl PackedOp {
    pub(crate) fn len(&self) -> usize {
        1 + self.operands.len()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ConstPoolRef(pub(crate) u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum ConstPoolValue {
    I32(i32),
    I64(i64),
}

#[derive(Clone, Copy)]
pub(crate) enum PackedOperand {
    I32(i32),
    I64(i64),
    ConstPoolRef(ConstPoolRef),
    F32(f32),
    F64(f64),
    U32(u32),
    CallRecipeRef(CallRecipeRef),
    LocalAddr(u32),
    SelectWidth(u32),
    JumpTarget(u32),
    MemArg(MemArg),
    BlockReturn(BlockReturn),
    LoopParam(LoopParam),
    Raw(Operand),
}

impl PackedOperand {
    #[cold]
    #[inline(never)]
    fn into_raw(self, const_pool: &[ConstPoolValue]) -> Operand {
        match self {
            Self::I32(value) => Operand { i32: value },
            Self::I64(value) => Operand { i64: value },
            Self::ConstPoolRef(index) => match const_pool[index.0 as usize] {
                ConstPoolValue::I32(value) => Operand { i32: value },
                ConstPoolValue::I64(value) => Operand { i64: value },
            },
            Self::F32(value) => Operand { f32: value },
            Self::F64(value) => Operand { f64: value },
            Self::U32(value) => Operand { u32: value },
            Self::CallRecipeRef(value) => Operand {
                call_recipe_ref: value,
            },
            Self::LocalAddr(value) => Operand { local_addr: value },
            Self::SelectWidth(value) => Operand { select: value },
            Self::JumpTarget(value) => Operand { jump_addr: value },
            Self::MemArg(value) => Operand { memarg: value },
            Self::BlockReturn(value) => Operand {
                block_return: value,
            },
            Self::LoopParam(value) => Operand { loop_param: value },
            Self::Raw(value) => value,
        }
    }
}

#[cfg(test)]
#[cold]
#[inline(never)]
pub(crate) fn pack_records(records: &[RecordEmit]) -> PackedOpStream {
    let (const_pool, const_refs) = build_const_pool(records);
    PackedOpStream {
        const_pool,
        ops: records
            .iter()
            .map(|record| pack_record(record, &const_refs))
            .collect(),
    }
}

#[cold]
#[inline(never)]
pub(crate) fn pack_op(source_start: Option<usize>, op: Op, operands: &[Operand]) -> PackedOp {
    PackedOp {
        source_start,
        op,
        operands: pack_operands(op, operands, &HashMap::new()),
    }
}

#[cold]
#[inline(never)]
pub(crate) fn build_packed_stream(mut ops: Vec<PackedOp>) -> PackedOpStream {
    let (const_pool, const_refs) = build_const_pool_from_packed_ops(&ops);
    for op in &mut ops {
        apply_const_pool_refs(op, &const_refs);
    }
    PackedOpStream { const_pool, ops }
}

#[cold]
#[inline(never)]
#[cfg(test)]
fn build_const_pool(
    records: &[RecordEmit],
) -> (Vec<ConstPoolValue>, HashMap<ConstPoolValue, ConstPoolRef>) {
    let mut counts = HashMap::new();
    let mut first_seen = Vec::new();
    for record in records {
        let Some(value) = const_pool_candidate(record) else {
            continue;
        };
        let entry = counts.entry(value).or_insert_with(|| {
            first_seen.push(value);
            0usize
        });
        *entry += 1;
    }
    let mut const_pool = Vec::new();
    let mut const_refs = HashMap::new();
    for value in first_seen {
        if counts[&value] < 2 {
            continue;
        }
        let index = ConstPoolRef(
            u32::try_from(const_pool.len()).expect("const pool length exceeds u32::MAX"),
        );
        const_refs.insert(value, index);
        const_pool.push(value);
    }
    (const_pool, const_refs)
}

#[cold]
#[inline(never)]
fn build_const_pool_from_packed_ops(
    ops: &[PackedOp],
) -> (Vec<ConstPoolValue>, HashMap<ConstPoolValue, ConstPoolRef>) {
    let mut counts = HashMap::new();
    let mut first_seen = Vec::new();
    for op in ops {
        let Some(value) = const_pool_candidate_from_packed_op(op) else {
            continue;
        };
        let entry = counts.entry(value).or_insert_with(|| {
            first_seen.push(value);
            0usize
        });
        *entry += 1;
    }
    let mut const_pool = Vec::new();
    let mut const_refs = HashMap::new();
    for value in first_seen {
        if counts[&value] < 2 {
            continue;
        }
        let index = ConstPoolRef(
            u32::try_from(const_pool.len()).expect("const pool length exceeds u32::MAX"),
        );
        const_refs.insert(value, index);
        const_pool.push(value);
    }
    (const_pool, const_refs)
}

#[cold]
#[inline(never)]
#[cfg(test)]
fn const_pool_candidate(record: &RecordEmit) -> Option<ConstPoolValue> {
    if ptr::fn_addr_eq(record.op, vm::op_i32_const as Op) {
        return Some(ConstPoolValue::I32(unsafe { record.operands[0].i32 }));
    }
    if ptr::fn_addr_eq(record.op, vm::op_i64_const as Op) {
        return Some(ConstPoolValue::I64(unsafe { record.operands[0].i64 }));
    }
    None
}

#[cold]
#[inline(never)]
fn const_pool_candidate_from_packed_op(op: &PackedOp) -> Option<ConstPoolValue> {
    if ptr::fn_addr_eq(op.op, vm::op_i32_const as Op) {
        let PackedOperand::I32(value) = *op.operands.first()? else {
            return None;
        };
        return Some(ConstPoolValue::I32(value));
    }
    if ptr::fn_addr_eq(op.op, vm::op_i64_const as Op) {
        let PackedOperand::I64(value) = *op.operands.first()? else {
            return None;
        };
        return Some(ConstPoolValue::I64(value));
    }
    None
}

#[cold]
#[inline(never)]
fn apply_const_pool_refs(op: &mut PackedOp, const_refs: &HashMap<ConstPoolValue, ConstPoolRef>) {
    if ptr::fn_addr_eq(op.op, vm::op_i32_const as Op) {
        let Some(PackedOperand::I32(value)) = op.operands.first().copied() else {
            return;
        };
        if let Some(index) = const_refs.get(&ConstPoolValue::I32(value)).copied() {
            op.operands[0] = PackedOperand::ConstPoolRef(index);
        }
        return;
    }
    if ptr::fn_addr_eq(op.op, vm::op_i64_const as Op) {
        let Some(PackedOperand::I64(value)) = op.operands.first().copied() else {
            return;
        };
        if let Some(index) = const_refs.get(&ConstPoolValue::I64(value)).copied() {
            op.operands[0] = PackedOperand::ConstPoolRef(index);
        }
    }
}

#[cold]
#[inline(never)]
#[cfg(test)]
fn pack_record(
    record: &RecordEmit,
    const_refs: &HashMap<ConstPoolValue, ConstPoolRef>,
) -> PackedOp {
    PackedOp {
        source_start: record.source_start,
        op: record.op,
        operands: pack_operands(record.op, &record.operands, const_refs),
    }
}

#[cold]
#[inline(never)]
fn pack_operands(
    op: Op,
    operands: &[Operand],
    const_refs: &HashMap<ConstPoolValue, ConstPoolRef>,
) -> Vec<PackedOperand> {
    if ptr::fn_addr_eq(op, vm::op_i32_const as Op) {
        let value = unsafe { operands[0].i32 };
        return vec![const_refs
            .get(&ConstPoolValue::I32(value))
            .copied()
            .map_or(PackedOperand::I32(value), PackedOperand::ConstPoolRef)];
    }
    if ptr::fn_addr_eq(op, vm::op_i64_const as Op) {
        let value = unsafe { operands[0].i64 };
        return vec![const_refs
            .get(&ConstPoolValue::I64(value))
            .copied()
            .map_or(PackedOperand::I64(value), PackedOperand::ConstPoolRef)];
    }
    if is_direct_call_op(op) {
        return operands
            .iter()
            .map(|operand| PackedOperand::CallRecipeRef(unsafe { operand.call_recipe_ref }))
            .collect();
    }
    if let Some(packed) = pack_fused_local_control_operands(op, operands) {
        return packed;
    }
    if let Some(packed) = pack_specialized_memory_operands(op, operands) {
        return packed;
    }
    if ptr::fn_addr_eq(op, vm::op_f32_const as Op) {
        return vec![PackedOperand::F32(unsafe { operands[0].f32 })];
    }
    if ptr::fn_addr_eq(op, vm::op_f64_const as Op) {
        return vec![PackedOperand::F64(unsafe { operands[0].f64 })];
    }
    if ptr::fn_addr_eq(op, vm::op_select as Op) {
        return vec![PackedOperand::SelectWidth(unsafe { operands[0].select })];
    }
    if ptr::fn_addr_eq(op, vm::op_loop as Op) {
        return vec![PackedOperand::LoopParam(unsafe { operands[0].loop_param })];
    }
    if ptr::fn_addr_eq(op, vm::special_function_return as Op) {
        return vec![PackedOperand::U32(unsafe { operands[0].drop_size })];
    }
    if ptr::fn_addr_eq(op, vm::special_block_return as Op) {
        return vec![PackedOperand::BlockReturn(unsafe {
            operands[0].block_return
        })];
    }
    if ptr::fn_addr_eq(op, vm::op_br_table as Op) {
        let mut out = Vec::with_capacity(operands.len());
        out.push(PackedOperand::U32(unsafe { operands[0].u32 }));
        out.extend(
            operands[1..]
                .iter()
                .map(|operand| PackedOperand::JumpTarget(unsafe { operand.jump_addr })),
        );
        return out;
    }
    if let Some(index) = jump_target_operand_index(op) {
        let mut out = pack_fallback_operands(op, operands);
        out[index] = PackedOperand::JumpTarget(unsafe { operands[index].jump_addr });
        return out;
    }
    if let Some(index) = memarg_operand_index(op) {
        let mut out = pack_fallback_operands(op, operands);
        out[index] = PackedOperand::MemArg(unsafe { operands[index].memarg });
        return out;
    }
    pack_fallback_operands(op, operands)
}

#[cold]
#[inline(never)]
fn pack_fused_local_control_operands(op: Op, operands: &[Operand]) -> Option<Vec<PackedOperand>> {
    if let Some(packed) = pack_local_unary_operands(op, operands) {
        return Some(packed);
    }
    if let Some(packed) = pack_local_fast_operands(op, operands) {
        return Some(packed);
    }
    if ptr::fn_addr_eq(op, vm::op_local_get4_i32_const_add as Op) {
        return Some(vec![
            PackedOperand::LocalAddr(unsafe { operands[0].local_addr }),
            PackedOperand::I32(unsafe { operands[1].i32 }),
        ]);
    }
    if ptr::fn_addr_eq(op, vm::op_local_get4_i32_const_add_set4 as Op)
        || ptr::fn_addr_eq(op, vm::op_local_get4_i32_const_add_tee4 as Op)
    {
        return Some(vec![
            PackedOperand::LocalAddr(unsafe { operands[0].local_addr }),
            PackedOperand::I32(unsafe { operands[1].i32 }),
            PackedOperand::LocalAddr(unsafe { operands[2].local_addr }),
        ]);
    }
    if ptr::fn_addr_eq(op, vm::op_local_get4_local_get4_i32_add as Op) {
        return Some(vec![
            PackedOperand::LocalAddr(unsafe { operands[0].local_addr }),
            PackedOperand::LocalAddr(unsafe { operands[1].local_addr }),
        ]);
    }
    if ptr::fn_addr_eq(op, vm::op_local_get4_local_get4_i32_add_set4 as Op)
        || ptr::fn_addr_eq(op, vm::op_local_get4_local_get4_i32_add_tee4 as Op)
    {
        return Some(vec![
            PackedOperand::LocalAddr(unsafe { operands[0].local_addr }),
            PackedOperand::LocalAddr(unsafe { operands[1].local_addr }),
            PackedOperand::LocalAddr(unsafe { operands[2].local_addr }),
        ]);
    }
    if ptr::fn_addr_eq(op, vm::op_local_get4_br_if as Op)
        || ptr::fn_addr_eq(op, vm::op_local_get4_i32_eqz_br_if as Op)
    {
        return Some(vec![
            PackedOperand::LocalAddr(unsafe { operands[0].local_addr }),
            PackedOperand::JumpTarget(unsafe { operands[1].jump_addr }),
        ]);
    }
    if ptr::fn_addr_eq(op, vm::op_local_get4_i32_const_add_br_if as Op) {
        return Some(vec![
            PackedOperand::LocalAddr(unsafe { operands[0].local_addr }),
            PackedOperand::I32(unsafe { operands[1].i32 }),
            PackedOperand::JumpTarget(unsafe { operands[2].jump_addr }),
        ]);
    }
    if ptr::fn_addr_eq(op, vm::op_local_get4_local_get4_i32_add_br_if as Op) {
        return Some(vec![
            PackedOperand::LocalAddr(unsafe { operands[0].local_addr }),
            PackedOperand::LocalAddr(unsafe { operands[1].local_addr }),
            PackedOperand::JumpTarget(unsafe { operands[2].jump_addr }),
        ]);
    }
    if ptr::fn_addr_eq(op, vm::op_local_get4_i32_const_compare_br_if as Op) {
        return Some(vec![
            PackedOperand::LocalAddr(unsafe { operands[0].local_addr }),
            PackedOperand::U32(unsafe { operands[1].u32 }),
            PackedOperand::I32(unsafe { operands[2].i32 }),
            PackedOperand::JumpTarget(unsafe { operands[3].jump_addr }),
        ]);
    }
    if ptr::fn_addr_eq(op, vm::op_local_get4_local_get4_compare_br_if as Op) {
        return Some(vec![
            PackedOperand::LocalAddr(unsafe { operands[0].local_addr }),
            PackedOperand::LocalAddr(unsafe { operands[1].local_addr }),
            PackedOperand::U32(unsafe { operands[2].u32 }),
            PackedOperand::JumpTarget(unsafe { operands[3].jump_addr }),
        ]);
    }
    if ptr::fn_addr_eq(op, vm::op_local_get4_i32_const_add_tee4_br_if as Op) {
        return Some(vec![
            PackedOperand::LocalAddr(unsafe { operands[0].local_addr }),
            PackedOperand::I32(unsafe { operands[1].i32 }),
            PackedOperand::LocalAddr(unsafe { operands[2].local_addr }),
            PackedOperand::JumpTarget(unsafe { operands[3].jump_addr }),
        ]);
    }
    None
}

#[cold]
#[inline(never)]
fn pack_local_unary_operands(op: Op, operands: &[Operand]) -> Option<Vec<PackedOperand>> {
    let kind = unsafe { operands.first()?.u32 };
    local_unary_kind_width(op, kind)?;
    let mut out = vec![
        PackedOperand::U32(kind),
        PackedOperand::LocalAddr(unsafe { operands.get(1)?.local_addr }),
    ];
    if ptr::fn_addr_eq(op, vm::op_local_unary32_set4 as Op)
        || ptr::fn_addr_eq(op, vm::op_local_unary32_tee4 as Op)
        || ptr::fn_addr_eq(op, vm::op_local_unary64_set8 as Op)
        || ptr::fn_addr_eq(op, vm::op_local_unary64_tee8 as Op)
    {
        out.push(PackedOperand::LocalAddr(unsafe {
            operands.get(2)?.local_addr
        }));
    }
    Some(out)
}

#[cold]
#[inline(never)]
fn pack_local_fast_operands(op: Op, operands: &[Operand]) -> Option<Vec<PackedOperand>> {
    let kind = unsafe { operands.first()?.u32 };
    let (rhs_shape, const_kind) = local_fast_kind_metadata(op, kind)?;
    let mut out = vec![
        PackedOperand::U32(kind),
        PackedOperand::LocalAddr(unsafe { operands.get(1)?.local_addr }),
        pack_local_fast_rhs(rhs_shape, const_kind, operands.get(2)?),
    ];
    if ptr::fn_addr_eq(op, vm::op_local_binop32_set4 as Op)
        || ptr::fn_addr_eq(op, vm::op_local_binop32_tee4 as Op)
        || ptr::fn_addr_eq(op, vm::op_local_binop64_set8 as Op)
        || ptr::fn_addr_eq(op, vm::op_local_binop64_tee8 as Op)
        || ptr::fn_addr_eq(op, vm::op_local_cmp32_set4 as Op)
        || ptr::fn_addr_eq(op, vm::op_local_cmp32_tee4 as Op)
        || ptr::fn_addr_eq(op, vm::op_local_cmp64_set4 as Op)
        || ptr::fn_addr_eq(op, vm::op_local_cmp64_tee4 as Op)
    {
        out.push(PackedOperand::LocalAddr(unsafe {
            operands.get(3)?.local_addr
        }));
    } else if ptr::fn_addr_eq(op, vm::op_local_binop32_br_if as Op)
        || ptr::fn_addr_eq(op, vm::op_local_cmp32_br_if as Op)
        || ptr::fn_addr_eq(op, vm::op_local_cmp64_br_if as Op)
    {
        out.push(PackedOperand::JumpTarget(unsafe {
            operands.get(3)?.jump_addr
        }));
    }
    Some(out)
}

#[inline(always)]
fn pack_local_fast_rhs(
    rhs_shape: LocalFastRhsShape,
    const_kind: LocalFastConstKind,
    operand: &Operand,
) -> PackedOperand {
    match rhs_shape {
        LocalFastRhsShape::Local => PackedOperand::LocalAddr(unsafe { operand.local_addr }),
        LocalFastRhsShape::Const => match const_kind {
            LocalFastConstKind::I32 => PackedOperand::I32(unsafe { operand.i32 }),
            LocalFastConstKind::I64 => PackedOperand::I64(unsafe { operand.i64 }),
            LocalFastConstKind::F32 => PackedOperand::F32(unsafe { operand.f32 }),
            LocalFastConstKind::F64 => PackedOperand::F64(unsafe { operand.f64 }),
        },
    }
}

#[inline(always)]
fn local_unary_kind_width(op: Op, kind: u32) -> Option<u32> {
    if ptr::fn_addr_eq(op, vm::op_local_unary32 as Op)
        || ptr::fn_addr_eq(op, vm::op_local_unary32_set4 as Op)
        || ptr::fn_addr_eq(op, vm::op_local_unary32_tee4 as Op)
    {
        let decoded = decode_local_unary32_kind(kind)?;
        let _ = decoded.const_kind();
        return Some(4);
    }
    if ptr::fn_addr_eq(op, vm::op_local_unary64 as Op)
        || ptr::fn_addr_eq(op, vm::op_local_unary64_set8 as Op)
        || ptr::fn_addr_eq(op, vm::op_local_unary64_tee8 as Op)
    {
        let decoded = decode_local_unary64_kind(kind)?;
        let _ = decoded.const_kind();
        return Some(8);
    }
    None
}

#[inline(always)]
fn local_fast_kind_metadata(op: Op, kind: u32) -> Option<(LocalFastRhsShape, LocalFastConstKind)> {
    if ptr::fn_addr_eq(op, vm::op_local_binop32 as Op)
        || ptr::fn_addr_eq(op, vm::op_local_binop32_set4 as Op)
        || ptr::fn_addr_eq(op, vm::op_local_binop32_tee4 as Op)
        || ptr::fn_addr_eq(op, vm::op_local_binop32_br_if as Op)
    {
        let (decoded, rhs_shape) = decode_local_binop32_kind(kind)?;
        return Some((rhs_shape, decoded.const_kind()));
    }
    if ptr::fn_addr_eq(op, vm::op_local_binop64 as Op)
        || ptr::fn_addr_eq(op, vm::op_local_binop64_set8 as Op)
        || ptr::fn_addr_eq(op, vm::op_local_binop64_tee8 as Op)
    {
        let (decoded, rhs_shape) = decode_local_binop64_kind(kind)?;
        return Some((rhs_shape, decoded.const_kind()));
    }
    if ptr::fn_addr_eq(op, vm::op_local_cmp32 as Op)
        || ptr::fn_addr_eq(op, vm::op_local_cmp32_set4 as Op)
        || ptr::fn_addr_eq(op, vm::op_local_cmp32_tee4 as Op)
        || ptr::fn_addr_eq(op, vm::op_local_cmp32_br_if as Op)
    {
        let (decoded, rhs_shape) = decode_local_cmp32_kind(kind)?;
        return Some((rhs_shape, decoded.const_kind()));
    }
    if ptr::fn_addr_eq(op, vm::op_local_cmp64 as Op)
        || ptr::fn_addr_eq(op, vm::op_local_cmp64_set4 as Op)
        || ptr::fn_addr_eq(op, vm::op_local_cmp64_tee4 as Op)
        || ptr::fn_addr_eq(op, vm::op_local_cmp64_br_if as Op)
    {
        let (decoded, rhs_shape) = decode_local_cmp64_kind(kind)?;
        return Some((rhs_shape, decoded.const_kind()));
    }
    None
}

#[cold]
#[inline(never)]
fn pack_specialized_memory_operands(op: Op, operands: &[Operand]) -> Option<Vec<PackedOperand>> {
    let family = specialized_memory_family(op)?;
    if operands.len() != family.operand_width() {
        return None;
    }
    Some(match family {
        SpecializedMemoryFamily::LocalBase | SpecializedMemoryFamily::SharedLocalBase => vec![
            PackedOperand::LocalAddr(unsafe { operands[0].local_addr }),
            PackedOperand::I32(unsafe { operands[1].i32 }),
            PackedOperand::MemArg(unsafe { operands[2].memarg }),
        ],
        SpecializedMemoryFamily::IndexedLocalBase
        | SpecializedMemoryFamily::IndexedSharedLocalBase => vec![
            PackedOperand::LocalAddr(unsafe { operands[0].local_addr }),
            PackedOperand::I32(unsafe { operands[1].i32 }),
            PackedOperand::MemArg(unsafe { operands[2].memarg }),
            PackedOperand::U32(unsafe { operands[3].u32 }),
        ],
        SpecializedMemoryFamily::LocalScaledIndex
        | SpecializedMemoryFamily::SharedLocalScaledIndex => vec![
            PackedOperand::LocalAddr(unsafe { operands[0].local_addr }),
            PackedOperand::LocalAddr(unsafe { operands[1].local_addr }),
            PackedOperand::U32(unsafe { operands[2].u32 }),
            PackedOperand::I32(unsafe { operands[3].i32 }),
            PackedOperand::MemArg(unsafe { operands[4].memarg }),
        ],
        SpecializedMemoryFamily::IndexedLocalScaledIndex
        | SpecializedMemoryFamily::IndexedSharedLocalScaledIndex => vec![
            PackedOperand::LocalAddr(unsafe { operands[0].local_addr }),
            PackedOperand::LocalAddr(unsafe { operands[1].local_addr }),
            PackedOperand::U32(unsafe { operands[2].u32 }),
            PackedOperand::I32(unsafe { operands[3].i32 }),
            PackedOperand::MemArg(unsafe { operands[4].memarg }),
            PackedOperand::U32(unsafe { operands[5].u32 }),
        ],
    })
}

#[cold]
#[inline(never)]
fn pack_fallback_operands(op: Op, operands: &[Operand]) -> Vec<PackedOperand> {
    if is_call_like_op(op) || is_u32_operand_op(op) {
        return operands
            .iter()
            .map(|operand| PackedOperand::U32(unsafe { operand.u32 }))
            .collect();
    }
    if is_local_addr_operand_op(op) {
        return operands
            .iter()
            .map(|operand| PackedOperand::LocalAddr(unsafe { operand.local_addr }))
            .collect();
    }
    operands.iter().copied().map(PackedOperand::Raw).collect()
}

#[cold]
#[inline(never)]
pub(crate) fn flatten_packed_stream(stream: &PackedOpStream) -> Vec<Instr> {
    let mut instrs = Vec::with_capacity(stream.instr_len());
    for op in &stream.ops {
        instrs.push(Instr { op: op.op });
        for operand in &op.operands {
            instrs.push(Instr {
                operand: operand.into_raw(&stream.const_pool),
            });
        }
    }
    instrs
}

#[allow(dead_code)]
#[cfg(test)]
pub(crate) fn flatten_records(records: &[RecordEmit]) -> Vec<Instr> {
    let mut instrs = Vec::with_capacity(records.iter().map(RecordEmit::len).sum());
    for record in records {
        instrs.push(Instr { op: record.op });
        for operand in &record.operands {
            instrs.push(Instr { operand: *operand });
        }
    }
    instrs
}

#[cold]
#[inline(never)]
pub(crate) fn verify_packed_stream(stream: &PackedOpStream) -> bool {
    let instr_len = stream.instr_len() as u32;
    for op in &stream.ops {
        if ptr::fn_addr_eq(op.op, vm::op_i32_const as Op) {
            if op.operands.len() != 1
                || !verify_const_pool_operand(
                    &op.operands[0],
                    &stream.const_pool,
                    ConstPoolValueKind::I32,
                )
            {
                return false;
            }
        } else if ptr::fn_addr_eq(op.op, vm::op_i64_const as Op) {
            if op.operands.len() != 1
                || !verify_const_pool_operand(
                    &op.operands[0],
                    &stream.const_pool,
                    ConstPoolValueKind::I64,
                )
            {
                return false;
            }
        } else if op
            .operands
            .iter()
            .any(|operand| matches!(operand, PackedOperand::ConstPoolRef(_)))
        {
            return false;
        }
        if let Some(valid) = verify_fused_local_control_operands(op, instr_len) {
            if !valid {
                return false;
            }
            continue;
        }
        let is_generic_select = ptr::fn_addr_eq(op.op, vm::op_select as Op);
        if is_generic_select
            && (op.operands.len() != 1 || !matches!(op.operands[0], PackedOperand::SelectWidth(_)))
        {
            return false;
        }
        let is_typed_select = ptr::fn_addr_eq(op.op, vm::op_select4 as Op)
            || ptr::fn_addr_eq(op.op, vm::op_select8 as Op)
            || ptr::fn_addr_eq(op.op, vm::op_select16 as Op);
        if is_typed_select && !op.operands.is_empty() {
            return false;
        }
        if is_direct_call_op(op.op)
            && (op.operands.len() != 1
                || !op
                    .operands
                    .iter()
                    .all(|operand| matches!(operand, PackedOperand::CallRecipeRef(_))))
        {
            return false;
        }
        if is_indirect_call_op(op.op)
            && (op.operands.len() != 2
                || !op
                    .operands
                    .iter()
                    .all(|operand| matches!(operand, PackedOperand::U32(_))))
        {
            return false;
        }
        if let Some(valid) = verify_specialized_memory_operands(op) {
            if !valid {
                return false;
            }
            continue;
        }
        if let Some(index) = jump_target_operand_index(op.op) {
            let Some(PackedOperand::JumpTarget(target)) = op.operands.get(index) else {
                return false;
            };
            if *target >= instr_len {
                return false;
            }
        } else if ptr::fn_addr_eq(op.op, vm::op_br_table as Op) {
            let Some(PackedOperand::U32(table_len)) = op.operands.first() else {
                return false;
            };
            if op.operands.len() != *table_len as usize + 2 {
                return false;
            }
            if !op.operands[1..].iter().all(|operand| matches!(operand, PackedOperand::JumpTarget(target) if *target < instr_len)) {
                return false;
            }
        }
        if let Some(index) = memarg_operand_index(op.op) {
            if !matches!(op.operands.get(index), Some(PackedOperand::MemArg(_))) {
                return false;
            }
        }
    }
    true
}

#[cold]
#[inline(never)]
fn verify_specialized_memory_operands(op: &PackedOp) -> Option<bool> {
    let family = specialized_memory_family(op.op)?;
    if op.operands.len() != family.operand_width() {
        return Some(false);
    }
    Some(match family {
        SpecializedMemoryFamily::LocalBase | SpecializedMemoryFamily::SharedLocalBase => {
            matches!(
                op.operands.as_slice(),
                [
                    PackedOperand::LocalAddr(_),
                    PackedOperand::I32(_),
                    PackedOperand::MemArg(_)
                ]
            )
        }
        SpecializedMemoryFamily::IndexedLocalBase
        | SpecializedMemoryFamily::IndexedSharedLocalBase => {
            matches!(
                op.operands.as_slice(),
                [
                    PackedOperand::LocalAddr(_),
                    PackedOperand::I32(_),
                    PackedOperand::MemArg(_),
                    PackedOperand::U32(_)
                ]
            )
        }
        SpecializedMemoryFamily::LocalScaledIndex
        | SpecializedMemoryFamily::SharedLocalScaledIndex => {
            matches!(
                op.operands.as_slice(),
                [
                    PackedOperand::LocalAddr(_),
                    PackedOperand::LocalAddr(_),
                    PackedOperand::U32(_),
                    PackedOperand::I32(_),
                    PackedOperand::MemArg(_)
                ]
            )
        }
        SpecializedMemoryFamily::IndexedLocalScaledIndex
        | SpecializedMemoryFamily::IndexedSharedLocalScaledIndex => {
            matches!(
                op.operands.as_slice(),
                [
                    PackedOperand::LocalAddr(_),
                    PackedOperand::LocalAddr(_),
                    PackedOperand::U32(_),
                    PackedOperand::I32(_),
                    PackedOperand::MemArg(_),
                    PackedOperand::U32(_)
                ]
            )
        }
    })
}

#[derive(Clone, Copy)]
enum ConstPoolValueKind {
    I32,
    I64,
}

#[cold]
#[inline(never)]
fn verify_const_pool_operand(
    operand: &PackedOperand,
    const_pool: &[ConstPoolValue],
    expected_kind: ConstPoolValueKind,
) -> bool {
    match (expected_kind, operand) {
        (ConstPoolValueKind::I32, PackedOperand::I32(_)) => true,
        (ConstPoolValueKind::I64, PackedOperand::I64(_)) => true,
        (ConstPoolValueKind::I32, PackedOperand::ConstPoolRef(index)) => {
            matches!(
                const_pool.get(index.0 as usize),
                Some(ConstPoolValue::I32(_))
            )
        }
        (ConstPoolValueKind::I64, PackedOperand::ConstPoolRef(index)) => {
            matches!(
                const_pool.get(index.0 as usize),
                Some(ConstPoolValue::I64(_))
            )
        }
        _ => false,
    }
}

#[cold]
#[inline(never)]
fn verify_fused_local_control_operands(op: &PackedOp, instr_len: u32) -> Option<bool> {
    let operands = op.operands.as_slice();
    if let Some(valid) = verify_local_unary_operands(op) {
        return Some(valid);
    }
    if let Some(valid) = verify_local_fast_operands(op, instr_len) {
        return Some(valid);
    }
    if ptr::fn_addr_eq(op.op, vm::op_local_get4_i32_const_add as Op) {
        return Some(matches!(
            operands,
            [PackedOperand::LocalAddr(_), PackedOperand::I32(_)]
        ));
    }
    if ptr::fn_addr_eq(op.op, vm::op_local_get4_i32_const_add_set4 as Op)
        || ptr::fn_addr_eq(op.op, vm::op_local_get4_i32_const_add_tee4 as Op)
    {
        return Some(matches!(
            operands,
            [
                PackedOperand::LocalAddr(_),
                PackedOperand::I32(_),
                PackedOperand::LocalAddr(_)
            ]
        ));
    }
    if ptr::fn_addr_eq(op.op, vm::op_local_get4_local_get4_i32_add as Op) {
        return Some(matches!(
            operands,
            [PackedOperand::LocalAddr(_), PackedOperand::LocalAddr(_)]
        ));
    }
    if ptr::fn_addr_eq(op.op, vm::op_local_get4_local_get4_i32_add_set4 as Op)
        || ptr::fn_addr_eq(op.op, vm::op_local_get4_local_get4_i32_add_tee4 as Op)
    {
        return Some(matches!(
            operands,
            [
                PackedOperand::LocalAddr(_),
                PackedOperand::LocalAddr(_),
                PackedOperand::LocalAddr(_)
            ]
        ));
    }
    if ptr::fn_addr_eq(op.op, vm::op_local_get4_br_if as Op)
        || ptr::fn_addr_eq(op.op, vm::op_local_get4_i32_eqz_br_if as Op)
    {
        return Some(matches!(
            operands,
            [
                PackedOperand::LocalAddr(_),
                PackedOperand::JumpTarget(target)
            ] if *target < instr_len
        ));
    }
    if ptr::fn_addr_eq(op.op, vm::op_local_get4_i32_const_add_br_if as Op) {
        return Some(matches!(
            operands,
            [
                PackedOperand::LocalAddr(_),
                PackedOperand::I32(_),
                PackedOperand::JumpTarget(target)
            ] if *target < instr_len
        ));
    }
    if ptr::fn_addr_eq(op.op, vm::op_local_get4_local_get4_i32_add_br_if as Op) {
        return Some(matches!(
            operands,
            [
                PackedOperand::LocalAddr(_),
                PackedOperand::LocalAddr(_),
                PackedOperand::JumpTarget(target)
            ] if *target < instr_len
        ));
    }
    if ptr::fn_addr_eq(op.op, vm::op_local_get4_i32_const_compare_br_if as Op) {
        return Some(matches!(
            operands,
            [
                PackedOperand::LocalAddr(_),
                PackedOperand::U32(_),
                PackedOperand::I32(_),
                PackedOperand::JumpTarget(target)
            ] if *target < instr_len
        ));
    }
    if ptr::fn_addr_eq(op.op, vm::op_local_get4_local_get4_compare_br_if as Op) {
        return Some(matches!(
            operands,
            [
                PackedOperand::LocalAddr(_),
                PackedOperand::LocalAddr(_),
                PackedOperand::U32(_),
                PackedOperand::JumpTarget(target)
            ] if *target < instr_len
        ));
    }
    if ptr::fn_addr_eq(op.op, vm::op_local_get4_i32_const_add_tee4_br_if as Op) {
        return Some(matches!(
            operands,
            [
                PackedOperand::LocalAddr(_),
                PackedOperand::I32(_),
                PackedOperand::LocalAddr(_),
                PackedOperand::JumpTarget(target)
            ] if *target < instr_len
        ));
    }
    None
}

#[cold]
#[inline(never)]
fn verify_local_unary_operands(op: &PackedOp) -> Option<bool> {
    let is_unary32 = ptr::fn_addr_eq(op.op, vm::op_local_unary32 as Op)
        || ptr::fn_addr_eq(op.op, vm::op_local_unary32_set4 as Op)
        || ptr::fn_addr_eq(op.op, vm::op_local_unary32_tee4 as Op);
    let is_unary64 = ptr::fn_addr_eq(op.op, vm::op_local_unary64 as Op)
        || ptr::fn_addr_eq(op.op, vm::op_local_unary64_set8 as Op)
        || ptr::fn_addr_eq(op.op, vm::op_local_unary64_tee8 as Op);
    if !is_unary32 && !is_unary64 {
        return None;
    }
    let operands = op.operands.as_slice();
    let Some(PackedOperand::U32(kind)) = operands.first() else {
        return Some(false);
    };
    let Some(width) = local_unary_kind_width(op.op, *kind) else {
        return Some(false);
    };
    let src_matches = matches!(operands.get(1), Some(PackedOperand::LocalAddr(_)));
    let dst_matches = matches!(operands.get(2), Some(PackedOperand::LocalAddr(_)));
    let expected_dst_width = matches!(
        op.op,
        op if ptr::fn_addr_eq(op, vm::op_local_unary32_set4 as Op)
            || ptr::fn_addr_eq(op, vm::op_local_unary32_tee4 as Op)
    )
    .then_some(4)
    .or_else(|| {
        matches!(
            op.op,
            op if ptr::fn_addr_eq(op, vm::op_local_unary64_set8 as Op)
                || ptr::fn_addr_eq(op, vm::op_local_unary64_tee8 as Op)
        )
        .then_some(8)
    });

    if ptr::fn_addr_eq(op.op, vm::op_local_unary32 as Op)
        || ptr::fn_addr_eq(op.op, vm::op_local_unary64 as Op)
    {
        return Some(operands.len() == 2 && src_matches && width > 0);
    }

    if let Some(expected_dst_width) = expected_dst_width {
        return Some(
            operands.len() == 3 && src_matches && dst_matches && width == expected_dst_width,
        );
    }

    Some(false)
}

#[cold]
#[inline(never)]
fn verify_local_fast_operands(op: &PackedOp, instr_len: u32) -> Option<bool> {
    let operands = op.operands.as_slice();
    let Some(PackedOperand::U32(kind)) = operands.first() else {
        if local_fast_kind_metadata(op.op, 0).is_some() {
            return Some(false);
        }
        return None;
    };
    let (rhs_shape, const_kind) = local_fast_kind_metadata(op.op, *kind)?;

    let rhs_matches = matches_local_fast_rhs(operands.get(2)?, rhs_shape, const_kind);
    let lhs_matches = matches!(operands.get(1), Some(PackedOperand::LocalAddr(_)));

    if ptr::fn_addr_eq(op.op, vm::op_local_binop32 as Op)
        || ptr::fn_addr_eq(op.op, vm::op_local_binop64 as Op)
        || ptr::fn_addr_eq(op.op, vm::op_local_cmp32 as Op)
        || ptr::fn_addr_eq(op.op, vm::op_local_cmp64 as Op)
    {
        return Some(operands.len() == 3 && lhs_matches && rhs_matches);
    }

    if ptr::fn_addr_eq(op.op, vm::op_local_binop32_set4 as Op)
        || ptr::fn_addr_eq(op.op, vm::op_local_binop32_tee4 as Op)
        || ptr::fn_addr_eq(op.op, vm::op_local_binop64_set8 as Op)
        || ptr::fn_addr_eq(op.op, vm::op_local_binop64_tee8 as Op)
        || ptr::fn_addr_eq(op.op, vm::op_local_cmp32_set4 as Op)
        || ptr::fn_addr_eq(op.op, vm::op_local_cmp32_tee4 as Op)
        || ptr::fn_addr_eq(op.op, vm::op_local_cmp64_set4 as Op)
        || ptr::fn_addr_eq(op.op, vm::op_local_cmp64_tee4 as Op)
    {
        return Some(
            operands.len() == 4
                && lhs_matches
                && rhs_matches
                && matches!(operands.get(3), Some(PackedOperand::LocalAddr(_))),
        );
    }

    if ptr::fn_addr_eq(op.op, vm::op_local_binop32_br_if as Op) {
        return Some(
            operands.len() == 4
                && lhs_matches
                && rhs_matches
                && const_kind == LocalFastConstKind::I32
                && matches!(operands.get(3), Some(PackedOperand::JumpTarget(target)) if *target < instr_len),
        );
    }

    if ptr::fn_addr_eq(op.op, vm::op_local_cmp32_br_if as Op)
        || ptr::fn_addr_eq(op.op, vm::op_local_cmp64_br_if as Op)
    {
        return Some(
            operands.len() == 4
                && lhs_matches
                && rhs_matches
                && matches!(operands.get(3), Some(PackedOperand::JumpTarget(target)) if *target < instr_len),
        );
    }

    Some(false)
}

#[inline(always)]
fn matches_local_fast_rhs(
    operand: &PackedOperand,
    rhs_shape: LocalFastRhsShape,
    const_kind: LocalFastConstKind,
) -> bool {
    matches!(
        (rhs_shape, const_kind, operand),
        (LocalFastRhsShape::Local, _, PackedOperand::LocalAddr(_))
            | (
                LocalFastRhsShape::Const,
                LocalFastConstKind::I32,
                PackedOperand::I32(_)
            )
            | (
                LocalFastRhsShape::Const,
                LocalFastConstKind::I64,
                PackedOperand::I64(_)
            )
            | (
                LocalFastRhsShape::Const,
                LocalFastConstKind::F32,
                PackedOperand::F32(_)
            )
            | (
                LocalFastRhsShape::Const,
                LocalFastConstKind::F64,
                PackedOperand::F64(_)
            )
    )
}

#[cold]
#[inline(never)]
fn is_local_addr_operand_op(op: Op) -> bool {
    ptr::fn_addr_eq(op, vm::op_local_get4 as Op)
        || ptr::fn_addr_eq(op, vm::op_local_get8 as Op)
        || ptr::fn_addr_eq(op, vm::op_local_get16 as Op)
        || ptr::fn_addr_eq(op, vm::op_local_set4 as Op)
        || ptr::fn_addr_eq(op, vm::op_local_set8 as Op)
        || ptr::fn_addr_eq(op, vm::op_local_set16 as Op)
        || ptr::fn_addr_eq(op, vm::op_local_tee4 as Op)
        || ptr::fn_addr_eq(op, vm::op_local_tee8 as Op)
        || ptr::fn_addr_eq(op, vm::op_local_tee16 as Op)
        || ptr::fn_addr_eq(op, vm::op_local_get4_i32_const_add as Op)
        || ptr::fn_addr_eq(op, vm::op_local_get4_i32_const_add_set4 as Op)
        || ptr::fn_addr_eq(op, vm::op_local_get4_i32_const_add_tee4 as Op)
        || ptr::fn_addr_eq(op, vm::op_local_get4_local_get4_i32_add as Op)
        || ptr::fn_addr_eq(op, vm::op_local_get4_local_get4_i32_add_set4 as Op)
        || ptr::fn_addr_eq(op, vm::op_local_get4_local_get4_i32_add_tee4 as Op)
}

#[cold]
#[inline(never)]
fn is_u32_operand_op(op: Op) -> bool {
    ptr::fn_addr_eq(op, vm::op_global_get4 as Op)
        || ptr::fn_addr_eq(op, vm::op_global_get8 as Op)
        || ptr::fn_addr_eq(op, vm::op_global_get16 as Op)
        || ptr::fn_addr_eq(op, vm::op_global_set4 as Op)
        || ptr::fn_addr_eq(op, vm::op_global_set8 as Op)
        || ptr::fn_addr_eq(op, vm::op_global_set16 as Op)
        || ptr::fn_addr_eq(op, vm::op_table_get as Op)
        || ptr::fn_addr_eq(op, vm::op_table_set as Op)
}

#[cold]
#[inline(never)]
fn is_call_like_op(op: Op) -> bool {
    is_direct_call_op(op) || is_indirect_call_op(op)
}

#[cold]
#[inline(never)]
fn is_direct_call_op(op: Op) -> bool {
    ptr::fn_addr_eq(op, vm::op_call as Op)
        || ptr::fn_addr_eq(op, vm::op_call_import as Op)
        || ptr::fn_addr_eq(op, vm::op_return_call as Op)
        || ptr::fn_addr_eq(op, vm::op_return_call_import as Op)
}

#[cold]
#[inline(never)]
fn is_indirect_call_op(op: Op) -> bool {
    ptr::fn_addr_eq(op, vm::op_call_indirect as Op)
        || ptr::fn_addr_eq(op, vm::op_return_call_indirect as Op)
}

#[cold]
#[inline(never)]
fn jump_target_operand_index(op: Op) -> Option<usize> {
    if ptr::fn_addr_eq(op, vm::op_if as Op)
        || ptr::fn_addr_eq(op, vm::op_else as Op)
        || ptr::fn_addr_eq(op, vm::op_br as Op)
        || ptr::fn_addr_eq(op, vm::op_br_if as Op)
        || ptr::fn_addr_eq(op, vm::op_return as Op)
    {
        return Some(0);
    }
    if ptr::fn_addr_eq(op, vm::op_local_get4_br_if as Op)
        || ptr::fn_addr_eq(op, vm::op_local_get4_i32_eqz_br_if as Op)
    {
        return Some(1);
    }
    if ptr::fn_addr_eq(op, vm::op_local_get4_i32_const_add_br_if as Op)
        || ptr::fn_addr_eq(op, vm::op_local_get4_local_get4_i32_add_br_if as Op)
    {
        return Some(2);
    }
    if ptr::fn_addr_eq(op, vm::op_local_get4_i32_const_compare_br_if as Op)
        || ptr::fn_addr_eq(op, vm::op_local_get4_local_get4_compare_br_if as Op)
        || ptr::fn_addr_eq(op, vm::op_local_get4_i32_const_add_tee4_br_if as Op)
        || ptr::fn_addr_eq(op, vm::op_local_binop32_br_if as Op)
        || ptr::fn_addr_eq(op, vm::op_local_cmp32_br_if as Op)
        || ptr::fn_addr_eq(op, vm::op_local_cmp64_br_if as Op)
    {
        return Some(3);
    }
    None
}

#[cold]
#[inline(never)]
fn memarg_operand_index(op: Op) -> Option<usize> {
    if let Some(family) = specialized_memory_family(op) {
        return Some(family.memarg_index());
    }
    let first = [
        vm::op_i32_load as Op,
        vm::op_i32_load8_s as Op,
        vm::op_i32_load8_u as Op,
        vm::op_i32_load16_s as Op,
        vm::op_i32_load16_u as Op,
        vm::op_i32_load_shared as Op,
        vm::op_i32_load8_s_shared as Op,
        vm::op_i32_load8_u_shared as Op,
        vm::op_i32_load16_s_shared as Op,
        vm::op_i32_load16_u_shared as Op,
        vm::op_i64_load as Op,
        vm::op_i64_load8_s as Op,
        vm::op_i64_load8_u as Op,
        vm::op_i64_load16_s as Op,
        vm::op_i64_load16_u as Op,
        vm::op_i64_load32_s as Op,
        vm::op_i64_load32_u as Op,
        vm::op_i64_load_shared as Op,
        vm::op_i64_load8_s_shared as Op,
        vm::op_i64_load8_u_shared as Op,
        vm::op_i64_load16_s_shared as Op,
        vm::op_i64_load16_u_shared as Op,
        vm::op_i64_load32_s_shared as Op,
        vm::op_i64_load32_u_shared as Op,
        vm::op_f32_load as Op,
        vm::op_f32_load_shared as Op,
        vm::op_f64_load as Op,
        vm::op_f64_load_shared as Op,
        vm::op_i32_store as Op,
        vm::op_i32_store8 as Op,
        vm::op_i32_store16 as Op,
        vm::op_i32_store_shared as Op,
        vm::op_i32_store8_shared as Op,
        vm::op_i32_store16_shared as Op,
        vm::op_i64_store as Op,
        vm::op_i64_store8 as Op,
        vm::op_i64_store16 as Op,
        vm::op_i64_store32 as Op,
        vm::op_i64_store_shared as Op,
        vm::op_i64_store8_shared as Op,
        vm::op_i64_store16_shared as Op,
        vm::op_i64_store32_shared as Op,
        vm::op_f32_store as Op,
        vm::op_f32_store_shared as Op,
        vm::op_f64_store as Op,
        vm::op_f64_store_shared as Op,
    ];
    if first
        .iter()
        .any(|candidate| ptr::fn_addr_eq(*candidate, op))
    {
        return Some(0);
    }
    let second = [
        vm::op_i32_load_local as Op,
        vm::op_i32_load8_s_local as Op,
        vm::op_i32_load8_u_local as Op,
        vm::op_i32_load16_s_local as Op,
        vm::op_i32_load16_u_local as Op,
        vm::op_i64_load_local as Op,
        vm::op_i64_load8_s_local as Op,
        vm::op_i64_load8_u_local as Op,
        vm::op_i64_load16_s_local as Op,
        vm::op_i64_load16_u_local as Op,
        vm::op_i64_load32_s_local as Op,
        vm::op_i64_load32_u_local as Op,
        vm::op_f32_load_local as Op,
        vm::op_f64_load_local as Op,
        vm::op_i32_store_local as Op,
        vm::op_i32_store8_local as Op,
        vm::op_i32_store16_local as Op,
        vm::op_i64_store_local as Op,
        vm::op_i64_store8_local as Op,
        vm::op_i64_store16_local as Op,
        vm::op_i64_store32_local as Op,
        vm::op_f32_store_local as Op,
        vm::op_f64_store_local as Op,
        vm::op_i32_load_indexed_local as Op,
        vm::op_i32_load8_s_indexed_local as Op,
        vm::op_i32_load8_u_indexed_local as Op,
        vm::op_i32_load16_s_indexed_local as Op,
        vm::op_i32_load16_u_indexed_local as Op,
        vm::op_i64_load_indexed_local as Op,
        vm::op_i64_load8_s_indexed_local as Op,
        vm::op_i64_load8_u_indexed_local as Op,
        vm::op_i64_load16_s_indexed_local as Op,
        vm::op_i64_load16_u_indexed_local as Op,
        vm::op_i64_load32_s_indexed_local as Op,
        vm::op_i64_load32_u_indexed_local as Op,
        vm::op_f32_load_indexed_local as Op,
        vm::op_f64_load_indexed_local as Op,
        vm::op_i32_store_indexed_local as Op,
        vm::op_i32_store8_indexed_local as Op,
        vm::op_i32_store16_indexed_local as Op,
        vm::op_i64_store_indexed_local as Op,
        vm::op_i64_store8_indexed_local as Op,
        vm::op_i64_store16_indexed_local as Op,
        vm::op_i64_store32_indexed_local as Op,
        vm::op_f32_store_indexed_local as Op,
        vm::op_f64_store_indexed_local as Op,
    ];
    if second
        .iter()
        .any(|candidate| ptr::fn_addr_eq(*candidate, op))
    {
        return Some(0);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pack_records_classifies_phase1_operands() {
        let memarg = MemArg {
            align: 2,
            offset: 16,
        };
        let records = vec![
            RecordEmit {
                source_start: Some(3),
                op: vm::op_br_if as Op,
                operands: vec![Operand { jump_addr: 12 }],
            },
            RecordEmit {
                source_start: Some(4),
                op: vm::op_select as Op,
                operands: vec![Operand { select: 8 }],
            },
            RecordEmit {
                source_start: Some(5),
                op: vm::op_i32_load as Op,
                operands: vec![Operand { memarg }],
            },
            RecordEmit {
                source_start: Some(6),
                op: vm::op_call as Op,
                operands: vec![Operand {
                    call_recipe_ref: CallRecipeRef::from_funcidx(7),
                }],
            },
        ];
        let packed = pack_records(&records);
        assert!(matches!(
            packed.ops[0].operands[0],
            PackedOperand::JumpTarget(12)
        ));
        assert!(matches!(
            packed.ops[1].operands[0],
            PackedOperand::SelectWidth(8)
        ));
        assert!(matches!(
            packed.ops[2].operands[0],
            PackedOperand::MemArg(arg) if arg.align == 2 && arg.offset == 16
        ));
        assert!(matches!(
            packed.ops[3].operands[0],
            PackedOperand::CallRecipeRef(target) if target == CallRecipeRef::from_funcidx(7)
        ));
    }

    #[test]
    fn pack_records_promotes_reused_integer_consts_to_const_pool() {
        let records = vec![
            RecordEmit {
                source_start: Some(1),
                op: vm::op_i32_const as Op,
                operands: vec![Operand { i32: 7 }],
            },
            RecordEmit {
                source_start: Some(2),
                op: vm::op_i32_const as Op,
                operands: vec![Operand { i32: 9 }],
            },
            RecordEmit {
                source_start: Some(3),
                op: vm::op_i64_const as Op,
                operands: vec![Operand { i64: -1 }],
            },
            RecordEmit {
                source_start: Some(4),
                op: vm::op_i32_const as Op,
                operands: vec![Operand { i32: 7 }],
            },
            RecordEmit {
                source_start: Some(5),
                op: vm::op_i64_const as Op,
                operands: vec![Operand { i64: -1 }],
            },
        ];
        let packed = pack_records(&records);
        assert_eq!(
            packed.const_pool,
            vec![ConstPoolValue::I32(7), ConstPoolValue::I64(-1)]
        );
        assert!(matches!(
            packed.ops[0].operands[0],
            PackedOperand::ConstPoolRef(ConstPoolRef(0))
        ));
        assert!(matches!(packed.ops[1].operands[0], PackedOperand::I32(9)));
        assert!(matches!(
            packed.ops[2].operands[0],
            PackedOperand::ConstPoolRef(ConstPoolRef(1))
        ));
        assert!(matches!(
            packed.ops[3].operands[0],
            PackedOperand::ConstPoolRef(ConstPoolRef(0))
        ));
        assert!(matches!(
            packed.ops[4].operands[0],
            PackedOperand::ConstPoolRef(ConstPoolRef(1))
        ));
    }

    #[test]
    fn pack_records_types_fused_local_control_operands() {
        let records = vec![
            RecordEmit {
                source_start: Some(1),
                op: vm::op_local_get4_i32_const_compare_br_if as Op,
                operands: vec![
                    Operand { local_addr: 4 },
                    Operand { u32: 6 },
                    Operand { i32: -7 },
                    Operand { jump_addr: 12 },
                ],
            },
            RecordEmit {
                source_start: Some(2),
                op: vm::op_local_get4_local_get4_i32_add_tee4 as Op,
                operands: vec![
                    Operand { local_addr: 1 },
                    Operand { local_addr: 2 },
                    Operand { local_addr: 3 },
                ],
            },
            RecordEmit {
                source_start: Some(3),
                op: vm::op_local_get4_i32_const_add_tee4_br_if as Op,
                operands: vec![
                    Operand { local_addr: 8 },
                    Operand { i32: -4 },
                    Operand { local_addr: 12 },
                    Operand { jump_addr: 20 },
                ],
            },
        ];
        let packed = pack_records(&records);
        assert!(matches!(
            packed.ops[0].operands.as_slice(),
            [
                PackedOperand::LocalAddr(4),
                PackedOperand::U32(6),
                PackedOperand::I32(-7),
                PackedOperand::JumpTarget(12)
            ]
        ));
        assert!(matches!(
            packed.ops[1].operands.as_slice(),
            [
                PackedOperand::LocalAddr(1),
                PackedOperand::LocalAddr(2),
                PackedOperand::LocalAddr(3)
            ]
        ));
        assert!(matches!(
            packed.ops[2].operands.as_slice(),
            [
                PackedOperand::LocalAddr(8),
                PackedOperand::I32(-4),
                PackedOperand::LocalAddr(12),
                PackedOperand::JumpTarget(20)
            ]
        ));
    }

    #[test]
    fn flatten_packed_stream_preserves_runtime_stream() {
        let records = vec![
            RecordEmit {
                source_start: Some(1),
                op: vm::op_i32_const as Op,
                operands: vec![Operand { i32: 42 }],
            },
            RecordEmit {
                source_start: Some(2),
                op: vm::op_i32_const as Op,
                operands: vec![Operand { i32: 42 }],
            },
            RecordEmit {
                source_start: Some(3),
                op: vm::op_br_if as Op,
                operands: vec![Operand { jump_addr: 5 }],
            },
        ];
        let packed = pack_records(&records);
        let flattened = flatten_packed_stream(&packed);
        assert_eq!(packed.const_pool, vec![ConstPoolValue::I32(42)]);
        assert!(ptr::fn_addr_eq(
            unsafe { flattened[0].op },
            vm::op_i32_const as Op
        ));
        assert_eq!(unsafe { flattened[1].operand.i32 }, 42);
        assert!(ptr::fn_addr_eq(
            unsafe { flattened[2].op },
            vm::op_i32_const as Op
        ));
        assert_eq!(unsafe { flattened[3].operand.i32 }, 42);
        assert!(ptr::fn_addr_eq(
            unsafe { flattened[4].op },
            vm::op_br_if as Op
        ));
        assert_eq!(unsafe { flattened[5].operand.jump_addr }, 5);
        assert_eq!(flattened.len(), flatten_records(&records).len());
    }

    #[test]
    fn verify_packed_stream_rejects_untyped_jump_target() {
        let stream = PackedOpStream {
            const_pool: Vec::new(),
            ops: vec![PackedOp {
                source_start: None,
                op: vm::op_br_if as Op,
                operands: vec![PackedOperand::U32(1)],
            }],
        };
        assert!(!verify_packed_stream(&stream));
    }

    #[test]
    fn verify_packed_stream_rejects_untyped_fused_compare_operand() {
        let stream = PackedOpStream {
            const_pool: Vec::new(),
            ops: vec![PackedOp {
                source_start: None,
                op: vm::op_local_get4_i32_const_compare_br_if as Op,
                operands: vec![
                    PackedOperand::Raw(Operand { local_addr: 1 }),
                    PackedOperand::U32(0),
                    PackedOperand::I32(7),
                    PackedOperand::JumpTarget(4),
                ],
            }],
        };
        assert!(!verify_packed_stream(&stream));
    }

    #[test]
    fn pack_records_types_local_fast_numeric_operands() {
        let records = vec![
            RecordEmit {
                source_start: Some(1),
                op: vm::op_local_binop64_tee8 as Op,
                operands: vec![
                    Operand {
                        u32: crate::common::encode_local_binop64_kind(
                            crate::common::LocalBinop64Op::I64Xor,
                            crate::common::LocalFastRhsShape::Local,
                        ),
                    },
                    Operand { local_addr: 8 },
                    Operand { local_addr: 16 },
                    Operand { local_addr: 24 },
                ],
            },
            RecordEmit {
                source_start: Some(2),
                op: vm::op_local_cmp64_br_if as Op,
                operands: vec![
                    Operand {
                        u32: crate::common::encode_local_cmp64_kind(
                            crate::common::LocalCmp64Op::F64Lt,
                            crate::common::LocalFastRhsShape::Const,
                        ),
                    },
                    Operand { local_addr: 0 },
                    Operand { f64: 1.5 },
                    Operand { jump_addr: 9 },
                ],
            },
        ];

        let packed = pack_records(&records);
        assert!(matches!(
            packed.ops[0].operands.as_slice(),
            [
                PackedOperand::U32(kind),
                PackedOperand::LocalAddr(8),
                PackedOperand::LocalAddr(16),
                PackedOperand::LocalAddr(24)
            ] if crate::common::decode_local_binop64_kind(*kind)
                == Some((crate::common::LocalBinop64Op::I64Xor, crate::common::LocalFastRhsShape::Local))
        ));
        assert!(matches!(
            packed.ops[1].operands.as_slice(),
            [
                PackedOperand::U32(kind),
                PackedOperand::LocalAddr(0),
                PackedOperand::F64(value),
                PackedOperand::JumpTarget(9)
            ] if *value == 1.5
                && crate::common::decode_local_cmp64_kind(*kind)
                    == Some((crate::common::LocalCmp64Op::F64Lt, crate::common::LocalFastRhsShape::Const))
        ));
    }

    #[test]
    fn verify_packed_stream_rejects_float_binop32_br_if_kind() {
        let stream = PackedOpStream {
            const_pool: Vec::new(),
            ops: vec![PackedOp {
                source_start: None,
                op: vm::op_local_binop32_br_if as Op,
                operands: vec![
                    PackedOperand::U32(crate::common::encode_local_binop32_kind(
                        crate::common::LocalBinop32Op::F32Add,
                        crate::common::LocalFastRhsShape::Local,
                    )),
                    PackedOperand::LocalAddr(0),
                    PackedOperand::LocalAddr(4),
                    PackedOperand::JumpTarget(7),
                ],
            }],
        };
        assert!(!verify_packed_stream(&stream));
    }

    #[test]
    fn verify_packed_stream_rejects_local_fast_payload_mismatch() {
        let stream = PackedOpStream {
            const_pool: Vec::new(),
            ops: vec![PackedOp {
                source_start: None,
                op: vm::op_local_cmp64 as Op,
                operands: vec![
                    PackedOperand::U32(crate::common::encode_local_cmp64_kind(
                        crate::common::LocalCmp64Op::F64Lt,
                        crate::common::LocalFastRhsShape::Const,
                    )),
                    PackedOperand::LocalAddr(0),
                    PackedOperand::I64(7),
                ],
            }],
        };
        assert!(!verify_packed_stream(&stream));
    }

    #[test]
    fn pack_records_types_local_unary_operands() {
        let records = vec![
            RecordEmit {
                source_start: Some(1),
                op: vm::op_local_unary32_tee4 as Op,
                operands: vec![
                    Operand {
                        u32: crate::common::encode_local_unary32_kind(
                            crate::common::LocalUnary32Op::F32Neg,
                        ),
                    },
                    Operand { local_addr: 8 },
                    Operand { local_addr: 12 },
                ],
            },
            RecordEmit {
                source_start: Some(2),
                op: vm::op_local_unary64 as Op,
                operands: vec![
                    Operand {
                        u32: crate::common::encode_local_unary64_kind(
                            crate::common::LocalUnary64Op::I64Popcnt,
                        ),
                    },
                    Operand { local_addr: 16 },
                ],
            },
        ];

        let packed = pack_records(&records);
        assert!(matches!(
            packed.ops[0].operands.as_slice(),
            [
                PackedOperand::U32(kind),
                PackedOperand::LocalAddr(8),
                PackedOperand::LocalAddr(12)
            ] if crate::common::decode_local_unary32_kind(*kind)
                == Some(crate::common::LocalUnary32Op::F32Neg)
        ));
        assert!(matches!(
            packed.ops[1].operands.as_slice(),
            [PackedOperand::U32(kind), PackedOperand::LocalAddr(16)]
                if crate::common::decode_local_unary64_kind(*kind)
                    == Some(crate::common::LocalUnary64Op::I64Popcnt)
        ));
    }

    #[test]
    fn verify_packed_stream_rejects_local_unary_payload_mismatch() {
        let stream = PackedOpStream {
            const_pool: Vec::new(),
            ops: vec![PackedOp {
                source_start: None,
                op: vm::op_local_unary32_set4 as Op,
                operands: vec![
                    PackedOperand::U32(crate::common::encode_local_unary32_kind(
                        crate::common::LocalUnary32Op::I32Clz,
                    )),
                    PackedOperand::LocalAddr(0),
                    PackedOperand::JumpTarget(7),
                ],
            }],
        };
        assert!(!verify_packed_stream(&stream));
    }

    #[test]
    fn verify_packed_stream_rejects_invalid_local_unary_kind() {
        let stream = PackedOpStream {
            const_pool: Vec::new(),
            ops: vec![PackedOp {
                source_start: None,
                op: vm::op_local_unary64 as Op,
                operands: vec![PackedOperand::U32(u32::MAX), PackedOperand::LocalAddr(0)],
            }],
        };
        assert!(!verify_packed_stream(&stream));
    }

    #[test]
    fn verify_packed_stream_rejects_mismatched_const_pool_ref() {
        let stream = PackedOpStream {
            const_pool: vec![ConstPoolValue::I64(11)],
            ops: vec![PackedOp {
                source_start: None,
                op: vm::op_i32_const as Op,
                operands: vec![PackedOperand::ConstPoolRef(ConstPoolRef(0))],
            }],
        };
        assert!(!verify_packed_stream(&stream));
    }

    #[test]
    fn verify_packed_stream_rejects_direct_call_without_recipe_ref() {
        let stream = PackedOpStream {
            const_pool: Vec::new(),
            ops: vec![PackedOp {
                source_start: None,
                op: vm::op_call as Op,
                operands: vec![PackedOperand::U32(7)],
            }],
        };
        assert!(!verify_packed_stream(&stream));
    }

    #[test]
    fn verify_packed_stream_rejects_indirect_call_with_wrong_arity() {
        let stream = PackedOpStream {
            const_pool: Vec::new(),
            ops: vec![PackedOp {
                source_start: None,
                op: vm::op_call_indirect as Op,
                operands: vec![PackedOperand::U32(0)],
            }],
        };
        assert!(!verify_packed_stream(&stream));
    }
}
