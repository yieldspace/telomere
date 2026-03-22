use std::{collections::HashSet, ops::Range};

use crate::{
    common::{Instr, MemArg, Operand},
    parser::core::instruction_generator::InstructionProgram,
    runtime::vm::{self, compute_memory_offset},
    VMResult,
};

#[derive(Clone)]
struct DecodedInstruction {
    old_range: Range<usize>,
    kind: DecodedKind,
    raw: Box<[Instr]>,
}

#[derive(Clone, Copy, Debug)]
enum DecodedKind {
    Raw,
    I32Const(i32),
    LocalGet4(u32),
    LocalSet4(u32),
    LocalTee4(u32),
    BrIf(u32),
    I32Add,
    I32Sub,
    I32And,
    I32Shl,
    I32ShrU,
    I32Eqz,
    I32GeU,
    I32LoadLocal(MemArg),
    I32Load8ULocal(MemArg),
    I32Load16SLocal(MemArg),
    I32Load16ULocal(MemArg),
    F32LoadLocal(MemArg),
    I32StoreLocal(MemArg),
    I32Store8Local(MemArg),
    I32Store16Local(MemArg),
}

#[derive(Clone, Copy)]
enum LocalImmSetTeeOp {
    Add,
    Sub,
    And,
    Shl,
    ShrU,
}

#[derive(Clone, Copy)]
enum LocalLocalSetTeeOp {
    Add,
}

#[derive(Clone, Copy)]
enum LocalAddrLoadOp {
    I32,
    I32Load8U,
    I32Load16S,
    I32Load16U,
    F32,
}

#[derive(Clone, Copy)]
enum LocalLocalStoreOp {
    I32,
    I32Store8,
    I32Store16,
}

enum OptimizedInstruction {
    Raw(DecodedInstruction),
    LocalImmSetTee {
        old_range: Range<usize>,
        src_local: u32,
        imm: i32,
        dst_local: u32,
        tee: bool,
        op: LocalImmSetTeeOp,
    },
    LocalLocalSetTee {
        old_range: Range<usize>,
        lhs_local_addr: u32,
        rhs_local_addr: u32,
        dst_local: u32,
        tee: bool,
        op: LocalLocalSetTeeOp,
    },
    I32LocalEqzBrIf {
        old_range: Range<usize>,
        local_addr: u32,
        target_old: u32,
    },
    I32LocalLocalGeUBrIf {
        old_range: Range<usize>,
        lhs_local_addr: u32,
        rhs_local_addr: u32,
        target_old: u32,
    },
    I32LoadConstLocal {
        old_range: Range<usize>,
        start: u32,
    },
    I32LocalGet4StoreConstLocal {
        old_range: Range<usize>,
        start: u32,
        local_addr: u32,
    },
    LocalAddrLoad {
        old_range: Range<usize>,
        local_addr: u32,
        memarg: MemArg,
        op: LocalAddrLoadOp,
    },
    LocalLocalStore {
        old_range: Range<usize>,
        addr_local_addr: u32,
        value_local_addr: u32,
        memarg: MemArg,
        op: LocalLocalStoreOp,
    },
}

type MatchOutcome = (usize, OptimizedInstruction);
type Matcher = fn(&[DecodedInstruction], usize, &HashSet<usize>) -> Option<MatchOutcome>;

const LOCAL_IMM_SET_TEE_MATCHERS: &[Matcher] = &[
    match_local_add_sub_imm_set_tee,
    match_local_bitwise_imm_set_tee,
];
const LOCAL_LOCAL_SET_TEE_MATCHERS: &[Matcher] = &[match_local_local_add_set_tee];
const BRANCH_MATCHERS: &[Matcher] = &[match_local_eqz_br_if, match_local_local_ge_u_br_if];
const CONST_ADDR_MATCHERS: &[Matcher] = &[match_const_i32_load, match_const_local_get_i32_store];
const LOCAL_ADDR_LOAD_MATCHERS: &[Matcher] = &[
    match_local_addr_i32_load,
    match_local_addr_i32_load8_u,
    match_local_addr_i32_load16_s,
    match_local_addr_i32_load16_u,
    match_local_addr_f32_load,
];
const LOCAL_LOCAL_STORE_MATCHERS: &[Matcher] = &[
    match_local_local_i32_store,
    match_local_local_i32_store8,
    match_local_local_i32_store16,
];

pub(crate) fn optimize_core_program(program: InstructionProgram) -> Vec<Instr> {
    if program.instruction_starts.is_empty() {
        return program.instr;
    }

    let decoded = decode_instructions(&program.instr, &program.instruction_starts);
    let jump_targets = collect_jump_targets(&decoded);
    let optimized = fuse_superinstructions(decoded, &jump_targets);
    lower_program(optimized, program.instr.len())
}

fn decode_instructions(instrs: &[Instr], starts: &[usize]) -> Vec<DecodedInstruction> {
    let mut decoded = Vec::with_capacity(starts.len());
    for (index, &start) in starts.iter().enumerate() {
        let end = starts.get(index + 1).copied().unwrap_or(instrs.len());
        let raw = instrs[start..end].to_vec().into_boxed_slice();
        let kind = decode_kind(&raw);
        decoded.push(DecodedInstruction {
            old_range: start..end,
            kind,
            raw,
        });
    }
    decoded
}

fn decode_kind(raw: &[Instr]) -> DecodedKind {
    let op = unsafe { raw[0].op };
    if raw.len() == 2 && std::ptr::fn_addr_eq(op, vm::op_i32_const as crate::common::Op) {
        return DecodedKind::I32Const(unsafe { raw[1].operand.i32 });
    }
    if raw.len() == 2 && std::ptr::fn_addr_eq(op, vm::op_local_get4 as crate::common::Op) {
        return DecodedKind::LocalGet4(unsafe { raw[1].operand.local_addr });
    }
    if raw.len() == 2 && std::ptr::fn_addr_eq(op, vm::op_local_set4 as crate::common::Op) {
        return DecodedKind::LocalSet4(unsafe { raw[1].operand.local_addr });
    }
    if raw.len() == 2 && std::ptr::fn_addr_eq(op, vm::op_local_tee4 as crate::common::Op) {
        return DecodedKind::LocalTee4(unsafe { raw[1].operand.local_addr });
    }
    if raw.len() == 2 && std::ptr::fn_addr_eq(op, vm::op_br_if as crate::common::Op) {
        return DecodedKind::BrIf(unsafe { raw[1].operand.jump_addr });
    }
    if raw.len() == 1 && std::ptr::fn_addr_eq(op, vm::op_i32_add as crate::common::Op) {
        return DecodedKind::I32Add;
    }
    if raw.len() == 1 && std::ptr::fn_addr_eq(op, vm::op_i32_sub as crate::common::Op) {
        return DecodedKind::I32Sub;
    }
    if raw.len() == 1 && std::ptr::fn_addr_eq(op, vm::op_i32_and as crate::common::Op) {
        return DecodedKind::I32And;
    }
    if raw.len() == 1 && std::ptr::fn_addr_eq(op, vm::op_i32_shl as crate::common::Op) {
        return DecodedKind::I32Shl;
    }
    if raw.len() == 1 && std::ptr::fn_addr_eq(op, vm::op_i32_shr_u as crate::common::Op) {
        return DecodedKind::I32ShrU;
    }
    if raw.len() == 1 && std::ptr::fn_addr_eq(op, vm::op_i32_eqz as crate::common::Op) {
        return DecodedKind::I32Eqz;
    }
    if raw.len() == 1 && std::ptr::fn_addr_eq(op, vm::op_i32_ge_u as crate::common::Op) {
        return DecodedKind::I32GeU;
    }
    if raw.len() == 2 && std::ptr::fn_addr_eq(op, vm::op_i32_load_local as crate::common::Op) {
        return DecodedKind::I32LoadLocal(unsafe { raw[1].operand.memarg });
    }
    if raw.len() == 2 && std::ptr::fn_addr_eq(op, vm::op_i32_load8_u_local as crate::common::Op) {
        return DecodedKind::I32Load8ULocal(unsafe { raw[1].operand.memarg });
    }
    if raw.len() == 2 && std::ptr::fn_addr_eq(op, vm::op_i32_load16_s_local as crate::common::Op) {
        return DecodedKind::I32Load16SLocal(unsafe { raw[1].operand.memarg });
    }
    if raw.len() == 2 && std::ptr::fn_addr_eq(op, vm::op_i32_load16_u_local as crate::common::Op) {
        return DecodedKind::I32Load16ULocal(unsafe { raw[1].operand.memarg });
    }
    if raw.len() == 2 && std::ptr::fn_addr_eq(op, vm::op_f32_load_local as crate::common::Op) {
        return DecodedKind::F32LoadLocal(unsafe { raw[1].operand.memarg });
    }
    if raw.len() == 2 && std::ptr::fn_addr_eq(op, vm::op_i32_store_local as crate::common::Op) {
        return DecodedKind::I32StoreLocal(unsafe { raw[1].operand.memarg });
    }
    if raw.len() == 2 && std::ptr::fn_addr_eq(op, vm::op_i32_store8_local as crate::common::Op) {
        return DecodedKind::I32Store8Local(unsafe { raw[1].operand.memarg });
    }
    if raw.len() == 2 && std::ptr::fn_addr_eq(op, vm::op_i32_store16_local as crate::common::Op) {
        return DecodedKind::I32Store16Local(unsafe { raw[1].operand.memarg });
    }
    DecodedKind::Raw
}

fn collect_jump_targets(decoded: &[DecodedInstruction]) -> HashSet<usize> {
    let mut targets = HashSet::new();
    for instruction in decoded {
        let raw = instruction.raw.as_ref();
        let op = unsafe { raw[0].op };
        if raw.len() >= 2
            && (std::ptr::fn_addr_eq(op, vm::op_br as crate::common::Op)
                || std::ptr::fn_addr_eq(op, vm::op_br_if as crate::common::Op)
                || std::ptr::fn_addr_eq(op, vm::op_if as crate::common::Op)
                || std::ptr::fn_addr_eq(op, vm::op_else as crate::common::Op)
                || std::ptr::fn_addr_eq(op, vm::op_return as crate::common::Op))
        {
            targets.insert(unsafe { raw[1].operand.jump_addr as usize });
            continue;
        }
        if raw.len() >= 3 && std::ptr::fn_addr_eq(op, vm::op_br_table as crate::common::Op) {
            let table_size = unsafe { raw[1].operand.u32 as usize };
            for target in &raw[2..=table_size + 2] {
                targets.insert(unsafe { target.operand.jump_addr as usize });
            }
        }
    }
    targets
}

fn fuse_superinstructions(
    decoded: Vec<DecodedInstruction>,
    jump_targets: &HashSet<usize>,
) -> Vec<OptimizedInstruction> {
    let mut optimized = Vec::with_capacity(decoded.len());
    let mut index = 0;

    while index < decoded.len() {
        if let Some((consumed, fused)) = try_matchers(
            LOCAL_IMM_SET_TEE_MATCHERS,
            decoded.as_slice(),
            index,
            jump_targets,
        ) {
            optimized.push(fused);
            index += consumed;
            continue;
        }
        if let Some((consumed, fused)) = try_matchers(
            LOCAL_LOCAL_SET_TEE_MATCHERS,
            decoded.as_slice(),
            index,
            jump_targets,
        ) {
            optimized.push(fused);
            index += consumed;
            continue;
        }
        if let Some((consumed, fused)) =
            try_matchers(BRANCH_MATCHERS, decoded.as_slice(), index, jump_targets)
        {
            optimized.push(fused);
            index += consumed;
            continue;
        }
        if let Some((consumed, fused)) =
            try_matchers(CONST_ADDR_MATCHERS, decoded.as_slice(), index, jump_targets)
        {
            optimized.push(fused);
            index += consumed;
            continue;
        }
        if let Some((consumed, fused)) = try_matchers(
            LOCAL_ADDR_LOAD_MATCHERS,
            decoded.as_slice(),
            index,
            jump_targets,
        ) {
            optimized.push(fused);
            index += consumed;
            continue;
        }
        if let Some((consumed, fused)) = try_matchers(
            LOCAL_LOCAL_STORE_MATCHERS,
            decoded.as_slice(),
            index,
            jump_targets,
        ) {
            optimized.push(fused);
            index += consumed;
            continue;
        }

        optimized.push(OptimizedInstruction::Raw(decoded[index].clone()));
        index += 1;
    }

    optimized
}

fn try_matchers(
    matchers: &[Matcher],
    decoded: &[DecodedInstruction],
    index: usize,
    jump_targets: &HashSet<usize>,
) -> Option<MatchOutcome> {
    for matcher in matchers {
        if let Some(outcome) = matcher(decoded, index, jump_targets) {
            return Some(outcome);
        }
    }
    None
}

fn sequence_crosses_jump_targets(
    decoded: &[DecodedInstruction],
    jump_targets: &HashSet<usize>,
    mut range: Range<usize>,
) -> bool {
    range.any(|idx| jump_targets.contains(&decoded[idx].old_range.start))
}

fn match_local_add_sub_imm_set_tee(
    decoded: &[DecodedInstruction],
    index: usize,
    jump_targets: &HashSet<usize>,
) -> Option<MatchOutcome> {
    let [first, second, third, fourth] = decoded.get(index..index + 4)? else {
        return None;
    };
    if sequence_crosses_jump_targets(decoded, jump_targets, index + 1..index + 4) {
        return None;
    }
    let src_local = match first.kind {
        DecodedKind::LocalGet4(local_addr) => local_addr,
        _ => return None,
    };
    let imm = match second.kind {
        DecodedKind::I32Const(value) => value,
        _ => return None,
    };
    let op = match third.kind {
        DecodedKind::I32Add => LocalImmSetTeeOp::Add,
        DecodedKind::I32Sub => LocalImmSetTeeOp::Sub,
        _ => return None,
    };
    let (dst_local, tee) = match fourth.kind {
        DecodedKind::LocalSet4(local_addr) => (local_addr, false),
        DecodedKind::LocalTee4(local_addr) => (local_addr, true),
        _ => return None,
    };

    Some((
        4,
        OptimizedInstruction::LocalImmSetTee {
            old_range: first.old_range.start..fourth.old_range.end,
            src_local,
            imm,
            dst_local,
            tee,
            op,
        },
    ))
}

fn match_local_bitwise_imm_set_tee(
    decoded: &[DecodedInstruction],
    index: usize,
    jump_targets: &HashSet<usize>,
) -> Option<MatchOutcome> {
    let [first, second, third, fourth] = decoded.get(index..index + 4)? else {
        return None;
    };
    if sequence_crosses_jump_targets(decoded, jump_targets, index + 1..index + 4) {
        return None;
    }
    let src_local = match first.kind {
        DecodedKind::LocalGet4(local_addr) => local_addr,
        _ => return None,
    };
    let imm = match second.kind {
        DecodedKind::I32Const(value) => value,
        _ => return None,
    };
    let op = match third.kind {
        DecodedKind::I32And => LocalImmSetTeeOp::And,
        DecodedKind::I32Shl => LocalImmSetTeeOp::Shl,
        DecodedKind::I32ShrU => LocalImmSetTeeOp::ShrU,
        _ => return None,
    };
    let (dst_local, tee) = match fourth.kind {
        DecodedKind::LocalSet4(local_addr) => (local_addr, false),
        DecodedKind::LocalTee4(local_addr) => (local_addr, true),
        _ => return None,
    };

    Some((
        4,
        OptimizedInstruction::LocalImmSetTee {
            old_range: first.old_range.start..fourth.old_range.end,
            src_local,
            imm,
            dst_local,
            tee,
            op,
        },
    ))
}

fn match_local_local_add_set_tee(
    decoded: &[DecodedInstruction],
    index: usize,
    jump_targets: &HashSet<usize>,
) -> Option<MatchOutcome> {
    let [first, second, third, fourth] = decoded.get(index..index + 4)? else {
        return None;
    };
    if sequence_crosses_jump_targets(decoded, jump_targets, index + 1..index + 4) {
        return None;
    }
    let lhs_local_addr = match first.kind {
        DecodedKind::LocalGet4(local_addr) => local_addr,
        _ => return None,
    };
    let rhs_local_addr = match second.kind {
        DecodedKind::LocalGet4(local_addr) => local_addr,
        _ => return None,
    };
    if !matches!(third.kind, DecodedKind::I32Add) {
        return None;
    }
    let (dst_local, tee) = match fourth.kind {
        DecodedKind::LocalSet4(local_addr) => (local_addr, false),
        DecodedKind::LocalTee4(local_addr) => (local_addr, true),
        _ => return None,
    };

    Some((
        4,
        OptimizedInstruction::LocalLocalSetTee {
            old_range: first.old_range.start..fourth.old_range.end,
            lhs_local_addr,
            rhs_local_addr,
            dst_local,
            tee,
            op: LocalLocalSetTeeOp::Add,
        },
    ))
}

fn match_local_eqz_br_if(
    decoded: &[DecodedInstruction],
    index: usize,
    jump_targets: &HashSet<usize>,
) -> Option<MatchOutcome> {
    let [first, second, third] = decoded.get(index..index + 3)? else {
        return None;
    };
    if sequence_crosses_jump_targets(decoded, jump_targets, index + 1..index + 3) {
        return None;
    }
    let local_addr = match first.kind {
        DecodedKind::LocalGet4(local_addr) => local_addr,
        _ => return None,
    };
    if !matches!(second.kind, DecodedKind::I32Eqz) {
        return None;
    }
    let target_old = match third.kind {
        DecodedKind::BrIf(target) => target,
        _ => return None,
    };

    Some((
        3,
        OptimizedInstruction::I32LocalEqzBrIf {
            old_range: first.old_range.start..third.old_range.end,
            local_addr,
            target_old,
        },
    ))
}

fn match_local_local_ge_u_br_if(
    decoded: &[DecodedInstruction],
    index: usize,
    jump_targets: &HashSet<usize>,
) -> Option<MatchOutcome> {
    let [first, second, third, fourth] = decoded.get(index..index + 4)? else {
        return None;
    };
    if sequence_crosses_jump_targets(decoded, jump_targets, index + 1..index + 4) {
        return None;
    }
    let lhs_local_addr = match first.kind {
        DecodedKind::LocalGet4(local_addr) => local_addr,
        _ => return None,
    };
    let rhs_local_addr = match second.kind {
        DecodedKind::LocalGet4(local_addr) => local_addr,
        _ => return None,
    };
    if !matches!(third.kind, DecodedKind::I32GeU) {
        return None;
    }
    let target_old = match fourth.kind {
        DecodedKind::BrIf(target) => target,
        _ => return None,
    };

    Some((
        4,
        OptimizedInstruction::I32LocalLocalGeUBrIf {
            old_range: first.old_range.start..fourth.old_range.end,
            lhs_local_addr,
            rhs_local_addr,
            target_old,
        },
    ))
}

fn match_const_i32_load(
    decoded: &[DecodedInstruction],
    index: usize,
    jump_targets: &HashSet<usize>,
) -> Option<MatchOutcome> {
    let [first, second] = decoded.get(index..index + 2)? else {
        return None;
    };
    if sequence_crosses_jump_targets(decoded, jump_targets, index + 1..index + 2) {
        return None;
    }
    let addr = match first.kind {
        DecodedKind::I32Const(value) => value,
        _ => return None,
    };
    let memarg = match second.kind {
        DecodedKind::I32LoadLocal(memarg) => memarg,
        _ => return None,
    };
    let start = match compute_memory_offset(memarg, addr as u32) {
        VMResult::Success(start) => u32::try_from(start).ok()?,
        _ => return None,
    };

    Some((
        2,
        OptimizedInstruction::I32LoadConstLocal {
            old_range: first.old_range.start..second.old_range.end,
            start,
        },
    ))
}

fn match_const_local_get_i32_store(
    decoded: &[DecodedInstruction],
    index: usize,
    jump_targets: &HashSet<usize>,
) -> Option<MatchOutcome> {
    let [first, second, third] = decoded.get(index..index + 3)? else {
        return None;
    };
    if sequence_crosses_jump_targets(decoded, jump_targets, index + 1..index + 3) {
        return None;
    }
    let addr = match first.kind {
        DecodedKind::I32Const(value) => value,
        _ => return None,
    };
    let local_addr = match second.kind {
        DecodedKind::LocalGet4(local_addr) => local_addr,
        _ => return None,
    };
    let memarg = match third.kind {
        DecodedKind::I32StoreLocal(memarg) => memarg,
        _ => return None,
    };
    let start = match compute_memory_offset(memarg, addr as u32) {
        VMResult::Success(start) => u32::try_from(start).ok()?,
        _ => return None,
    };

    Some((
        3,
        OptimizedInstruction::I32LocalGet4StoreConstLocal {
            old_range: first.old_range.start..third.old_range.end,
            start,
            local_addr,
        },
    ))
}

fn match_local_addr_i32_load(
    decoded: &[DecodedInstruction],
    index: usize,
    jump_targets: &HashSet<usize>,
) -> Option<MatchOutcome> {
    match_local_addr_load(decoded, index, jump_targets, LocalAddrLoadOp::I32)
}

fn match_local_addr_i32_load8_u(
    decoded: &[DecodedInstruction],
    index: usize,
    jump_targets: &HashSet<usize>,
) -> Option<MatchOutcome> {
    match_local_addr_load(decoded, index, jump_targets, LocalAddrLoadOp::I32Load8U)
}

fn match_local_addr_i32_load16_s(
    decoded: &[DecodedInstruction],
    index: usize,
    jump_targets: &HashSet<usize>,
) -> Option<MatchOutcome> {
    match_local_addr_load(decoded, index, jump_targets, LocalAddrLoadOp::I32Load16S)
}

fn match_local_addr_i32_load16_u(
    decoded: &[DecodedInstruction],
    index: usize,
    jump_targets: &HashSet<usize>,
) -> Option<MatchOutcome> {
    match_local_addr_load(decoded, index, jump_targets, LocalAddrLoadOp::I32Load16U)
}

fn match_local_addr_f32_load(
    decoded: &[DecodedInstruction],
    index: usize,
    jump_targets: &HashSet<usize>,
) -> Option<MatchOutcome> {
    match_local_addr_load(decoded, index, jump_targets, LocalAddrLoadOp::F32)
}

fn match_local_addr_load(
    decoded: &[DecodedInstruction],
    index: usize,
    jump_targets: &HashSet<usize>,
    op: LocalAddrLoadOp,
) -> Option<MatchOutcome> {
    let [first, second] = decoded.get(index..index + 2)? else {
        return None;
    };
    if sequence_crosses_jump_targets(decoded, jump_targets, index + 1..index + 2) {
        return None;
    }
    let local_addr = match first.kind {
        DecodedKind::LocalGet4(local_addr) => local_addr,
        _ => return None,
    };
    let memarg = match (op, second.kind) {
        (LocalAddrLoadOp::I32, DecodedKind::I32LoadLocal(memarg))
        | (LocalAddrLoadOp::I32Load8U, DecodedKind::I32Load8ULocal(memarg))
        | (LocalAddrLoadOp::I32Load16S, DecodedKind::I32Load16SLocal(memarg))
        | (LocalAddrLoadOp::I32Load16U, DecodedKind::I32Load16ULocal(memarg))
        | (LocalAddrLoadOp::F32, DecodedKind::F32LoadLocal(memarg)) => memarg,
        _ => return None,
    };

    Some((
        2,
        OptimizedInstruction::LocalAddrLoad {
            old_range: first.old_range.start..second.old_range.end,
            local_addr,
            memarg,
            op,
        },
    ))
}

fn match_local_local_i32_store(
    decoded: &[DecodedInstruction],
    index: usize,
    jump_targets: &HashSet<usize>,
) -> Option<MatchOutcome> {
    match_local_local_store(decoded, index, jump_targets, LocalLocalStoreOp::I32)
}

fn match_local_local_i32_store8(
    decoded: &[DecodedInstruction],
    index: usize,
    jump_targets: &HashSet<usize>,
) -> Option<MatchOutcome> {
    match_local_local_store(decoded, index, jump_targets, LocalLocalStoreOp::I32Store8)
}

fn match_local_local_i32_store16(
    decoded: &[DecodedInstruction],
    index: usize,
    jump_targets: &HashSet<usize>,
) -> Option<MatchOutcome> {
    match_local_local_store(decoded, index, jump_targets, LocalLocalStoreOp::I32Store16)
}

fn match_local_local_store(
    decoded: &[DecodedInstruction],
    index: usize,
    jump_targets: &HashSet<usize>,
    op: LocalLocalStoreOp,
) -> Option<MatchOutcome> {
    let [first, second, third] = decoded.get(index..index + 3)? else {
        return None;
    };
    if sequence_crosses_jump_targets(decoded, jump_targets, index + 1..index + 3) {
        return None;
    }
    let addr_local_addr = match first.kind {
        DecodedKind::LocalGet4(local_addr) => local_addr,
        _ => return None,
    };
    let value_local_addr = match second.kind {
        DecodedKind::LocalGet4(local_addr) => local_addr,
        _ => return None,
    };
    let memarg = match (op, third.kind) {
        (LocalLocalStoreOp::I32, DecodedKind::I32StoreLocal(memarg))
        | (LocalLocalStoreOp::I32Store8, DecodedKind::I32Store8Local(memarg))
        | (LocalLocalStoreOp::I32Store16, DecodedKind::I32Store16Local(memarg)) => memarg,
        _ => return None,
    };

    Some((
        3,
        OptimizedInstruction::LocalLocalStore {
            old_range: first.old_range.start..third.old_range.end,
            addr_local_addr,
            value_local_addr,
            memarg,
            op,
        },
    ))
}

fn lower_program(optimized: Vec<OptimizedInstruction>, old_flat_len: usize) -> Vec<Instr> {
    let mut old_to_new = vec![0u32; old_flat_len];
    let mut new_len = 0usize;
    for instruction in &optimized {
        for old_index in old_range(instruction).clone() {
            old_to_new[old_index] =
                u32::try_from(new_len).expect("optimized program grew too large");
        }
        new_len += output_len(instruction);
    }

    let mut lowered = Vec::with_capacity(new_len);
    for instruction in optimized {
        lower_instruction(instruction, &old_to_new, &mut lowered);
    }
    lowered
}

fn old_range(instruction: &OptimizedInstruction) -> &Range<usize> {
    match instruction {
        OptimizedInstruction::Raw(decoded) => &decoded.old_range,
        OptimizedInstruction::LocalImmSetTee { old_range, .. }
        | OptimizedInstruction::LocalLocalSetTee { old_range, .. }
        | OptimizedInstruction::I32LocalEqzBrIf { old_range, .. }
        | OptimizedInstruction::I32LocalLocalGeUBrIf { old_range, .. }
        | OptimizedInstruction::I32LoadConstLocal { old_range, .. }
        | OptimizedInstruction::I32LocalGet4StoreConstLocal { old_range, .. }
        | OptimizedInstruction::LocalAddrLoad { old_range, .. }
        | OptimizedInstruction::LocalLocalStore { old_range, .. } => old_range,
    }
}

fn output_len(instruction: &OptimizedInstruction) -> usize {
    match instruction {
        OptimizedInstruction::Raw(decoded) => decoded.raw.len(),
        OptimizedInstruction::LocalImmSetTee { .. } => 4,
        OptimizedInstruction::LocalLocalSetTee { .. } => 4,
        OptimizedInstruction::I32LocalEqzBrIf { .. } => 3,
        OptimizedInstruction::I32LocalLocalGeUBrIf { .. } => 4,
        OptimizedInstruction::I32LoadConstLocal { .. } => 2,
        OptimizedInstruction::I32LocalGet4StoreConstLocal { .. } => 3,
        OptimizedInstruction::LocalAddrLoad { .. } => 3,
        OptimizedInstruction::LocalLocalStore { .. } => 4,
    }
}

fn lower_instruction(
    instruction: OptimizedInstruction,
    old_to_new: &[u32],
    lowered: &mut Vec<Instr>,
) {
    match instruction {
        OptimizedInstruction::Raw(decoded) => {
            lowered.extend(rewrite_raw_jumps(decoded.raw.as_ref(), old_to_new))
        }
        OptimizedInstruction::LocalImmSetTee {
            src_local,
            imm,
            dst_local,
            tee,
            op,
            ..
        } => {
            let op = match (op, tee) {
                (LocalImmSetTeeOp::Add, false) => vm::op_i32_local_add_imm_set4,
                (LocalImmSetTeeOp::Add, true) => vm::op_i32_local_add_imm_tee4,
                (LocalImmSetTeeOp::Sub, false) => vm::op_i32_local_sub_imm_set4,
                (LocalImmSetTeeOp::Sub, true) => vm::op_i32_local_sub_imm_tee4,
                (LocalImmSetTeeOp::And, false) => vm::op_i32_local_and_imm_set4,
                (LocalImmSetTeeOp::And, true) => vm::op_i32_local_and_imm_tee4,
                (LocalImmSetTeeOp::Shl, false) => vm::op_i32_local_shl_imm_set4,
                (LocalImmSetTeeOp::Shl, true) => vm::op_i32_local_shl_imm_tee4,
                (LocalImmSetTeeOp::ShrU, false) => vm::op_i32_local_shr_u_imm_set4,
                (LocalImmSetTeeOp::ShrU, true) => vm::op_i32_local_shr_u_imm_tee4,
            };
            lowered.push(Instr { op });
            lowered.push(Instr {
                operand: Operand {
                    local_addr: src_local,
                },
            });
            lowered.push(Instr {
                operand: Operand { i32: imm },
            });
            lowered.push(Instr {
                operand: Operand {
                    local_addr: dst_local,
                },
            });
        }
        OptimizedInstruction::LocalLocalSetTee {
            lhs_local_addr,
            rhs_local_addr,
            dst_local,
            tee,
            op,
            ..
        } => {
            let op = match (op, tee) {
                (LocalLocalSetTeeOp::Add, false) => vm::op_i32_local_local_add_set4,
                (LocalLocalSetTeeOp::Add, true) => vm::op_i32_local_local_add_tee4,
            };
            lowered.push(Instr { op });
            lowered.push(Instr {
                operand: Operand {
                    local_addr: lhs_local_addr,
                },
            });
            lowered.push(Instr {
                operand: Operand {
                    local_addr: rhs_local_addr,
                },
            });
            lowered.push(Instr {
                operand: Operand {
                    local_addr: dst_local,
                },
            });
        }
        OptimizedInstruction::I32LocalEqzBrIf {
            local_addr,
            target_old,
            ..
        } => {
            lowered.push(Instr {
                op: vm::op_i32_local_eqz_br_if,
            });
            lowered.push(Instr {
                operand: Operand { local_addr },
            });
            lowered.push(Instr {
                operand: Operand {
                    jump_addr: remap_jump_target(target_old, old_to_new),
                },
            });
        }
        OptimizedInstruction::I32LocalLocalGeUBrIf {
            lhs_local_addr,
            rhs_local_addr,
            target_old,
            ..
        } => {
            lowered.push(Instr {
                op: vm::op_i32_local_local_ge_u_br_if,
            });
            lowered.push(Instr {
                operand: Operand {
                    local_addr: lhs_local_addr,
                },
            });
            lowered.push(Instr {
                operand: Operand {
                    local_addr: rhs_local_addr,
                },
            });
            lowered.push(Instr {
                operand: Operand {
                    jump_addr: remap_jump_target(target_old, old_to_new),
                },
            });
        }
        OptimizedInstruction::I32LoadConstLocal { start, .. } => {
            lowered.push(Instr {
                op: vm::op_i32_load_const_local,
            });
            lowered.push(Instr {
                operand: Operand { u32: start },
            });
        }
        OptimizedInstruction::I32LocalGet4StoreConstLocal {
            start, local_addr, ..
        } => {
            lowered.push(Instr {
                op: vm::op_i32_local_get4_store_const_local,
            });
            lowered.push(Instr {
                operand: Operand { u32: start },
            });
            lowered.push(Instr {
                operand: Operand { local_addr },
            });
        }
        OptimizedInstruction::LocalAddrLoad {
            local_addr,
            memarg,
            op,
            ..
        } => {
            let op = match op {
                LocalAddrLoadOp::I32 => vm::op_i32_local_addr_load,
                LocalAddrLoadOp::I32Load8U => vm::op_i32_local_addr_load8_u,
                LocalAddrLoadOp::I32Load16S => vm::op_i32_local_addr_load16_s,
                LocalAddrLoadOp::I32Load16U => vm::op_i32_local_addr_load16_u,
                LocalAddrLoadOp::F32 => vm::op_f32_local_addr_load,
            };
            lowered.push(Instr { op });
            lowered.push(Instr {
                operand: Operand { local_addr },
            });
            lowered.push(Instr {
                operand: Operand { memarg },
            });
        }
        OptimizedInstruction::LocalLocalStore {
            addr_local_addr,
            value_local_addr,
            memarg,
            op,
            ..
        } => {
            let op = match op {
                LocalLocalStoreOp::I32 => vm::op_i32_local_local_store,
                LocalLocalStoreOp::I32Store8 => vm::op_i32_local_local_store8,
                LocalLocalStoreOp::I32Store16 => vm::op_i32_local_local_store16,
            };
            lowered.push(Instr { op });
            lowered.push(Instr {
                operand: Operand {
                    local_addr: addr_local_addr,
                },
            });
            lowered.push(Instr {
                operand: Operand {
                    local_addr: value_local_addr,
                },
            });
            lowered.push(Instr {
                operand: Operand { memarg },
            });
        }
    }
}

fn rewrite_raw_jumps(raw: &[Instr], old_to_new: &[u32]) -> Vec<Instr> {
    let mut rewritten = raw.to_vec();
    let op = unsafe { raw[0].op };
    if raw.len() >= 2
        && (std::ptr::fn_addr_eq(op, vm::op_br as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_br_if as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_if as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_else as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_return as crate::common::Op))
    {
        let target = unsafe { raw[1].operand.jump_addr };
        rewritten[1] = Instr {
            operand: Operand {
                jump_addr: remap_jump_target(target, old_to_new),
            },
        };
        return rewritten;
    }
    if raw.len() >= 3 && std::ptr::fn_addr_eq(op, vm::op_br_table as crate::common::Op) {
        let table_size = unsafe { raw[1].operand.u32 as usize };
        for index in 0..=table_size {
            let target_index = index + 2;
            let target = unsafe { raw[target_index].operand.jump_addr };
            rewritten[target_index] = Instr {
                operand: Operand {
                    jump_addr: remap_jump_target(target, old_to_new),
                },
            };
        }
    }
    rewritten
}

fn remap_jump_target(target_old: u32, old_to_new: &[u32]) -> u32 {
    old_to_new[target_old as usize]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn optimizer_does_not_fuse_across_jump_target_boundary() {
        let optimized = optimize_core_program(InstructionProgram {
            instr: vec![
                Instr { op: vm::op_br_if },
                Instr {
                    operand: Operand { jump_addr: 4 },
                },
                Instr {
                    op: vm::op_local_get4,
                },
                Instr {
                    operand: Operand { local_addr: 0 },
                },
                Instr {
                    op: vm::op_i32_load_local,
                },
                Instr {
                    operand: Operand {
                        memarg: MemArg {
                            align: 4,
                            offset: 0,
                        },
                    },
                },
            ],
            instruction_starts: vec![0, 2, 4],
        });

        assert!(std::ptr::fn_addr_eq(
            unsafe { optimized[2].op },
            vm::op_local_get4 as crate::common::Op
        ));
    }
}
