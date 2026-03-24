use std::{collections::HashSet, ops::Range, sync::Arc};

use crate::{
    common::{
        ControlFlowMetadataKind, ControlFlowMetadataSite, Instr, MemArg, Op, Operand, ReturnShape,
        StackMapSite, StackMapSourceSite, UnwindSiteMetadata, UnwindSourceSite, ValueSize,
    },
    parser::core::instruction_generator::InstructionProgram,
    runtime::vm::{self, compute_memory_offset},
    VMResult,
};

pub(crate) struct OptimizedCoreProgram {
    pub(crate) instr: Vec<Instr>,
    pub(crate) control_flow_metadata: Arc<[ControlFlowMetadataSite]>,
    pub(crate) stack_map_sites: Arc<[StackMapSite]>,
    pub(crate) unwind_sites: Arc<[UnwindSiteMetadata]>,
    pub(crate) instruction_ordinal_by_raw_start: Arc<[u32]>,
}

#[derive(Clone)]
struct DecodedInstruction {
    old_range: Range<usize>,
    kind: DecodedKind,
    raw: Box<[Instr]>,
}

#[derive(Clone, Copy, Debug)]
enum TypedConst {
    I32(i32),
    I64(i64),
    F32(u32),
    F64(u64),
}

impl TypedConst {
    fn width(self) -> ValueSize {
        match self {
            Self::I32(_) | Self::F32(_) => ValueSize::Byte4,
            Self::I64(_) | Self::F64(_) => ValueSize::Byte8,
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum TypedScalarOp {
    I32(vm::I32ScalarKind),
    I64(vm::I64ScalarKind),
    F32(vm::FloatScalarKind),
    F64(vm::FloatScalarKind),
}

impl TypedScalarOp {
    fn width(self) -> ValueSize {
        match self {
            Self::I32(_) | Self::F32(_) => ValueSize::Byte4,
            Self::I64(_) | Self::F64(_) => ValueSize::Byte8,
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum TypedCompareOp {
    I32(vm::IntCompareKind),
    I64(vm::IntCompareKind),
    F32(vm::FloatCompareKind),
    F64(vm::FloatCompareKind),
}

impl TypedCompareOp {
    fn width(self) -> ValueSize {
        match self {
            Self::I32(_) | Self::F32(_) => ValueSize::Byte4,
            Self::I64(_) | Self::F64(_) => ValueSize::Byte8,
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum TypedLoadOp {
    Bits4(vm::Load4Kind),
    Bits8(vm::Load8Kind),
}

impl TypedLoadOp {
    fn width(self) -> ValueSize {
        match self {
            Self::Bits4(_) => ValueSize::Byte4,
            Self::Bits8(_) => ValueSize::Byte8,
        }
    }

    fn uses_dedicated_const(self) -> bool {
        matches!(self, Self::Bits4(vm::Load4Kind::I32))
    }

    fn uses_dedicated_local_addr(self) -> bool {
        matches!(
            self,
            Self::Bits4(
                vm::Load4Kind::I32
                    | vm::Load4Kind::I32Load8U
                    | vm::Load4Kind::I32Load16S
                    | vm::Load4Kind::I32Load16U
                    | vm::Load4Kind::F32
            )
        )
    }
}

#[derive(Clone, Copy, Debug)]
enum TypedStoreOp {
    Bits4(vm::Store4Kind),
    Bits8(vm::Store8Kind),
}

impl TypedStoreOp {
    fn value_width(self) -> ValueSize {
        match self {
            Self::Bits4(_) => ValueSize::Byte4,
            Self::Bits8(_) => ValueSize::Byte8,
        }
    }

    fn uses_dedicated_const(self) -> bool {
        matches!(self, Self::Bits4(vm::Store4Kind::I32))
    }

    fn uses_dedicated_local_local(self) -> bool {
        matches!(
            self,
            Self::Bits4(
                vm::Store4Kind::I32 | vm::Store4Kind::I32Store8 | vm::Store4Kind::I32Store16
            )
        )
    }
}

#[derive(Clone, Copy, Debug)]
enum DecodedKind {
    Raw,
    Const(TypedConst),
    LocalGet(ValueSize, u32),
    LocalSet(ValueSize, u32),
    LocalTee(ValueSize, u32),
    Select(ValueSize),
    BrIf(u32),
    If(u32),
    Eqz(ValueSize),
    Scalar(TypedScalarOp),
    Compare(TypedCompareOp),
    Load(TypedLoadOp, MemArg),
    Store(TypedStoreOp, MemArg),
}

#[derive(Clone, Copy)]
enum ControlBranchKind {
    BrIf,
    If,
}

#[derive(Clone, Copy)]
enum NarrowCopyKind {
    Load8Store8,
    Load16Store16,
}

#[derive(Clone, Copy)]
enum ProducerSeed {
    Local {
        width: ValueSize,
        local_addr: u32,
    },
    LocalImmScalar {
        width: ValueSize,
        src_local: u32,
        imm: TypedConst,
        op: TypedScalarOp,
    },
    LocalLocalScalar {
        width: ValueSize,
        lhs_local_addr: u32,
        rhs_local_addr: u32,
        op: TypedScalarOp,
    },
    LocalAddrLoad {
        width: ValueSize,
        local_addr: u32,
        memarg: MemArg,
        op: TypedLoadOp,
    },
    LocalImmAddrLoad {
        width: ValueSize,
        local_addr: u32,
        imm: i32,
        memarg: MemArg,
        op: TypedLoadOp,
    },
    ConstAddrLoad {
        width: ValueSize,
        start: u32,
        op: TypedLoadOp,
    },
}

impl ProducerSeed {
    fn width(self) -> ValueSize {
        match self {
            Self::Local { width, .. }
            | Self::LocalImmScalar { width, .. }
            | Self::LocalLocalScalar { width, .. }
            | Self::LocalAddrLoad { width, .. }
            | Self::LocalImmAddrLoad { width, .. }
            | Self::ConstAddrLoad { width, .. } => width,
        }
    }
}

struct ProducerSeedMatch {
    seed: ProducerSeed,
    consumed: usize,
}

enum OptimizedInstruction {
    Raw(DecodedInstruction),
    ConstSetTee {
        old_range: Range<usize>,
        value: TypedConst,
        dst_local: u32,
        tee: bool,
    },
    LocalCopy {
        old_range: Range<usize>,
        src_local: u32,
        dst_local: u32,
        width: ValueSize,
        tee: bool,
    },
    LocalImmPush {
        old_range: Range<usize>,
        src_local: u32,
        imm: TypedConst,
        op: TypedScalarOp,
    },
    LocalLocalPush {
        old_range: Range<usize>,
        lhs_local_addr: u32,
        rhs_local_addr: u32,
        op: TypedScalarOp,
    },
    LocalImmSetTee {
        old_range: Range<usize>,
        src_local: u32,
        imm: TypedConst,
        dst_local: u32,
        tee: bool,
        op: TypedScalarOp,
    },
    LocalLocalSetTee {
        old_range: Range<usize>,
        lhs_local_addr: u32,
        rhs_local_addr: u32,
        dst_local: u32,
        tee: bool,
        op: TypedScalarOp,
    },
    LocalBranch {
        old_range: Range<usize>,
        local_addr: u32,
        target_old: u32,
        width: ValueSize,
        zero_test: bool,
        branch_kind: ControlBranchKind,
    },
    I32LocalAndImmBranch {
        old_range: Range<usize>,
        local_addr: u32,
        imm: i32,
        target_old: u32,
        zero_test: bool,
        branch_kind: ControlBranchKind,
    },
    ProducerImmAndBranch {
        old_range: Range<usize>,
        seed: ProducerSeed,
        rhs_const: TypedConst,
        width: ValueSize,
        target_old: u32,
        zero_test: bool,
        branch_kind: ControlBranchKind,
    },
    I32LocalAddrLoad8UAndImmEqzBranch {
        old_range: Range<usize>,
        local_addr: u32,
        memarg: MemArg,
        imm: i32,
        target_old: u32,
        branch_kind: ControlBranchKind,
    },
    I32LocalLocalGeUBrIf {
        old_range: Range<usize>,
        lhs_local_addr: u32,
        rhs_local_addr: u32,
        target_old: u32,
    },
    CompareSetTeeLocal {
        old_range: Range<usize>,
        lhs_local_addr: u32,
        rhs_local_addr: u32,
        dst_local: u32,
        tee: bool,
        op: TypedCompareOp,
    },
    CompareSetTeeConst {
        old_range: Range<usize>,
        lhs_local_addr: u32,
        rhs_const: TypedConst,
        dst_local: u32,
        tee: bool,
        op: TypedCompareOp,
    },
    CompareBrIfLocal {
        old_range: Range<usize>,
        lhs_local_addr: u32,
        rhs_local_addr: u32,
        target_old: u32,
        op: TypedCompareOp,
    },
    CompareBrIfConst {
        old_range: Range<usize>,
        lhs_local_addr: u32,
        rhs_const: TypedConst,
        target_old: u32,
        op: TypedCompareOp,
    },
    CompareSelectLocal {
        old_range: Range<usize>,
        lhs_local_addr: u32,
        rhs_local_addr: u32,
        select_width: ValueSize,
        op: TypedCompareOp,
    },
    CompareSelectConst {
        old_range: Range<usize>,
        lhs_local_addr: u32,
        rhs_const: TypedConst,
        select_width: ValueSize,
        op: TypedCompareOp,
    },
    LoadConstLocal {
        old_range: Range<usize>,
        start: u32,
        op: TypedLoadOp,
    },
    StoreConstLocal {
        old_range: Range<usize>,
        start: u32,
        value_local_addr: u32,
        op: TypedStoreOp,
    },
    LocalAddrLoad {
        old_range: Range<usize>,
        local_addr: u32,
        memarg: MemArg,
        op: TypedLoadOp,
    },
    LocalImmAddrLoad {
        old_range: Range<usize>,
        local_addr: u32,
        imm: i32,
        memarg: MemArg,
        op: TypedLoadOp,
    },
    I32LocalLocalLoadTeeAddImmStore {
        old_range: Range<usize>,
        store_addr_local_addr: u32,
        load_addr_local_addr: u32,
        tee_local_addr: u32,
        imm: i32,
        load_memarg: MemArg,
        store_memarg: MemArg,
    },
    LocalLocalStore {
        old_range: Range<usize>,
        addr_local_addr: u32,
        value_local_addr: u32,
        memarg: MemArg,
        op: TypedStoreOp,
    },
    LocalImmLocalStore {
        old_range: Range<usize>,
        addr_local_addr: u32,
        imm: i32,
        value_local_addr: u32,
        memarg: MemArg,
        op: TypedStoreOp,
    },
    I32LocalLocalNarrowCopy {
        old_range: Range<usize>,
        dst_local_addr: u32,
        src_local_addr: u32,
        load_memarg: MemArg,
        store_memarg: MemArg,
        kind: NarrowCopyKind,
    },
    ProducerTeeEqzBranch {
        old_range: Range<usize>,
        seed: ProducerSeed,
        tee_local_addr: u32,
        target_old: u32,
        width: ValueSize,
        branch_kind: ControlBranchKind,
    },
    ProducerTeeImmCompareBranch {
        old_range: Range<usize>,
        seed: ProducerSeed,
        tee_local_addr: u32,
        rhs_const: TypedConst,
        target_old: u32,
        op: TypedCompareOp,
        branch_kind: ControlBranchKind,
    },
    ProducerTeeImmScalarSetTee {
        old_range: Range<usize>,
        seed: ProducerSeed,
        tee_local_addr: u32,
        rhs_const: TypedConst,
        dst_local: u32,
        dst_tee: bool,
        op: TypedScalarOp,
    },
    ProducerImmScalarSetTee {
        old_range: Range<usize>,
        seed: ProducerSeed,
        rhs_const: TypedConst,
        dst_local: u32,
        dst_tee: bool,
        op: TypedScalarOp,
    },
    ProducerTeeConstSelfSelect {
        old_range: Range<usize>,
        seed: ProducerSeed,
        tee_local_addr: u32,
        rhs_const: TypedConst,
        width: ValueSize,
    },
    ProducerCompareSelectLocal {
        old_range: Range<usize>,
        seed: ProducerSeed,
        rhs_local_addr: u32,
        select_width: ValueSize,
        op: TypedCompareOp,
    },
    ProducerCompareSelectConst {
        old_range: Range<usize>,
        seed: ProducerSeed,
        rhs_const: TypedConst,
        select_width: ValueSize,
        op: TypedCompareOp,
    },
    CompareTeeSelectLocal {
        old_range: Range<usize>,
        lhs_local_addr: u32,
        rhs_local_addr: u32,
        tee_local_addr: u32,
        select_width: ValueSize,
        op: TypedCompareOp,
    },
    CompareTeeSelectConst {
        old_range: Range<usize>,
        lhs_local_addr: u32,
        rhs_const: TypedConst,
        tee_local_addr: u32,
        select_width: ValueSize,
        op: TypedCompareOp,
    },
}

type MatchOutcome = (usize, OptimizedInstruction);
type Matcher = fn(&[DecodedInstruction], usize, &HashSet<usize>) -> Option<MatchOutcome>;

const LOCAL_COPY_MATCHERS: &[Matcher] = &[match_local_copy];
const CONST_SET_TEE_MATCHERS: &[Matcher] = &[match_const_set_tee];
const LOCAL_IMM_PUSH_MATCHERS: &[Matcher] = &[match_local_imm_scalar_push];
const LOCAL_LOCAL_PUSH_MATCHERS: &[Matcher] = &[match_local_local_scalar_push];
const LOCAL_IMM_SET_TEE_MATCHERS: &[Matcher] = &[match_local_imm_scalar_set_tee];
const LOCAL_LOCAL_SET_TEE_MATCHERS: &[Matcher] = &[match_local_local_scalar_set_tee];
const COMPARE_SET_TEE_MATCHERS: &[Matcher] = &[
    match_local_local_compare_set_tee,
    match_local_const_compare_set_tee,
];
const COMPARE_SELECT_MATCHERS: &[Matcher] = &[
    match_local_local_compare_select,
    match_local_const_compare_select,
];
const LOAD_MASK_BRANCH_MATCHERS: &[Matcher] = &[match_i32_local_addr_load8_u_and_imm_eqz_branch];
const PRODUCER_IMM_AND_BRANCH_MATCHERS: &[Matcher] = &[match_producer_imm_and_branch];
const PRODUCER_COMPARE_SELECT_MATCHERS: &[Matcher] = &[
    match_producer_local_compare_select,
    match_producer_const_compare_select,
];
const PRODUCER_IMM_SCALAR_SET_TEE_MATCHERS: &[Matcher] = &[match_producer_imm_scalar_set_tee];
const PRODUCER_TEE_EQZ_BRANCH_MATCHERS: &[Matcher] = &[match_producer_tee_eqz_branch];
const PRODUCER_TEE_IMM_COMPARE_BRANCH_MATCHERS: &[Matcher] =
    &[match_producer_tee_imm_compare_branch];
const PRODUCER_TEE_IMM_SCALAR_SET_TEE_MATCHERS: &[Matcher] =
    &[match_producer_tee_imm_scalar_set_tee];
const PRODUCER_TEE_SELECT_MATCHERS: &[Matcher] = &[match_producer_tee_const_self_select];
const COMPARE_TEE_SELECT_MATCHERS: &[Matcher] = &[
    match_local_local_compare_tee_select,
    match_local_const_compare_tee_select,
];
const BRANCH_MATCHERS: &[Matcher] = &[
    match_i32_local_and_imm_branch,
    match_local_branch,
    match_local_local_ge_u_br_if,
    match_local_local_compare_br_if,
    match_local_const_compare_br_if,
];
const CONST_ADDR_MATCHERS: &[Matcher] = &[match_const_load, match_const_local_store];
const LOCAL_ADDR_LOAD_MATCHERS: &[Matcher] = &[match_local_imm_addr_load, match_local_addr_load];
const LOAD_MODIFY_STORE_MATCHERS: &[Matcher] = &[match_i32_local_local_load_tee_add_imm_store];
const LOAD_STORE_NARROW_COPY_MATCHERS: &[Matcher] = &[
    match_i32_local_local_load8_u_store8_copy,
    match_i32_local_local_load16_u_store16_copy,
];
const LOCAL_LOCAL_STORE_MATCHERS: &[Matcher] =
    &[match_local_imm_local_store, match_local_local_store];

#[cfg(test)]
pub(crate) fn optimize_core_program(program: InstructionProgram) -> Vec<Instr> {
    optimize_core_program_with_function_index(program, 0)
}

#[allow(dead_code)]
pub(crate) fn optimize_core_program_with_function_index(
    program: InstructionProgram,
    function_index: u32,
) -> Vec<Instr> {
    optimize_core_program_with_metadata(program, function_index, 0, &[], &[]).instr
}

pub(crate) fn optimize_core_program_with_metadata(
    program: InstructionProgram,
    function_index: u32,
    frame_stack_base: u32,
    stack_map_source_sites: &[StackMapSourceSite],
    unwind_source_sites: &[UnwindSourceSite],
) -> OptimizedCoreProgram {
    if program.instruction_starts.is_empty() {
        return OptimizedCoreProgram {
            instr: program.instr,
            control_flow_metadata: Arc::from([]),
            stack_map_sites: Arc::from([]),
            unwind_sites: Arc::from([]),
            instruction_ordinal_by_raw_start: Arc::from([]),
        };
    }

    let decoded = decode_instructions(&program.instr, &program.instruction_starts);
    let jump_targets = collect_jump_targets(&decoded);
    let optimized = fuse_superinstructions(decoded, &jump_targets);
    let lowered = lower_program(optimized, program.instr.len(), function_index);
    let control_flow_metadata = collect_control_flow_metadata(
        &lowered.instr,
        &lowered.instruction_starts,
        frame_stack_base,
    );
    let stack_map_sites = collect_stack_map_metadata(
        stack_map_source_sites,
        &lowered.old_to_new,
        &lowered.instruction_starts,
    );
    let unwind_sites = collect_unwind_metadata(
        unwind_source_sites,
        &lowered.old_to_new,
        &lowered.instruction_starts,
    );
    let instruction_ordinal_by_raw_start =
        build_instruction_ordinal_by_raw_start(lowered.instr.len(), &lowered.instruction_starts);
    OptimizedCoreProgram {
        instr: lowered.instr,
        control_flow_metadata,
        stack_map_sites,
        unwind_sites,
        instruction_ordinal_by_raw_start,
    }
}

fn build_instruction_ordinal_by_raw_start(len: usize, instruction_starts: &[usize]) -> Arc<[u32]> {
    let mut map = vec![u32::MAX; len];
    for (instruction_ordinal, &start) in instruction_starts.iter().enumerate() {
        map[start] = instruction_ordinal as u32;
    }
    Arc::from(map)
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

    macro_rules! decode1 {
        ($vmop:path, $kind:expr) => {
            if raw.len() == 1 && std::ptr::fn_addr_eq(op, $vmop as crate::common::Op) {
                return $kind;
            }
        };
    }
    macro_rules! decode2 {
        ($vmop:path, $kind:expr) => {
            if raw.len() == 2 && std::ptr::fn_addr_eq(op, $vmop as crate::common::Op) {
                return $kind;
            }
        };
    }

    decode2!(
        vm::op_i32_const,
        DecodedKind::Const(TypedConst::I32(unsafe { raw[1].operand.i32 }))
    );
    decode2!(
        vm::op_i64_const,
        DecodedKind::Const(TypedConst::I64(unsafe { raw[1].operand.i64 }))
    );
    decode2!(
        vm::op_f32_const,
        DecodedKind::Const(TypedConst::F32(unsafe { raw[1].operand.f32 }.to_bits()))
    );
    decode2!(
        vm::op_f64_const,
        DecodedKind::Const(TypedConst::F64(unsafe { raw[1].operand.f64 }.to_bits()))
    );

    decode2!(
        vm::op_local_get4,
        DecodedKind::LocalGet(ValueSize::Byte4, unsafe { raw[1].operand.local_addr })
    );
    decode2!(
        vm::op_local_get8,
        DecodedKind::LocalGet(ValueSize::Byte8, unsafe { raw[1].operand.local_addr })
    );
    decode2!(
        vm::op_local_get16,
        DecodedKind::LocalGet(ValueSize::Byte16, unsafe { raw[1].operand.local_addr })
    );
    decode2!(
        vm::op_local_set4,
        DecodedKind::LocalSet(ValueSize::Byte4, unsafe { raw[1].operand.local_addr })
    );
    decode2!(
        vm::op_local_set8,
        DecodedKind::LocalSet(ValueSize::Byte8, unsafe { raw[1].operand.local_addr })
    );
    decode2!(
        vm::op_local_set16,
        DecodedKind::LocalSet(ValueSize::Byte16, unsafe { raw[1].operand.local_addr })
    );
    decode2!(
        vm::op_local_tee4,
        DecodedKind::LocalTee(ValueSize::Byte4, unsafe { raw[1].operand.local_addr })
    );
    decode2!(
        vm::op_local_tee8,
        DecodedKind::LocalTee(ValueSize::Byte8, unsafe { raw[1].operand.local_addr })
    );
    decode2!(
        vm::op_local_tee16,
        DecodedKind::LocalTee(ValueSize::Byte16, unsafe { raw[1].operand.local_addr })
    );

    decode2!(
        vm::op_br_if,
        DecodedKind::BrIf(unsafe { raw[1].operand.jump_addr })
    );
    decode2!(
        vm::op_if,
        DecodedKind::If(unsafe { raw[1].operand.jump_addr })
    );

    decode1!(vm::op_select4, DecodedKind::Select(ValueSize::Byte4));
    decode1!(vm::op_select8, DecodedKind::Select(ValueSize::Byte8));
    if raw.len() == 2 && std::ptr::fn_addr_eq(op, vm::op_select as crate::common::Op) {
        match unsafe { raw[1].operand.select } {
            4 => return DecodedKind::Select(ValueSize::Byte4),
            8 => return DecodedKind::Select(ValueSize::Byte8),
            _ => {}
        }
    }

    decode1!(vm::op_i32_eqz, DecodedKind::Eqz(ValueSize::Byte4));
    decode1!(vm::op_i64_eqz, DecodedKind::Eqz(ValueSize::Byte8));

    decode1!(
        vm::op_i32_add,
        DecodedKind::Scalar(TypedScalarOp::I32(vm::I32ScalarKind::Add))
    );
    decode1!(
        vm::op_i32_sub,
        DecodedKind::Scalar(TypedScalarOp::I32(vm::I32ScalarKind::Sub))
    );
    decode1!(
        vm::op_i32_mul,
        DecodedKind::Scalar(TypedScalarOp::I32(vm::I32ScalarKind::Mul))
    );
    decode1!(
        vm::op_i32_and,
        DecodedKind::Scalar(TypedScalarOp::I32(vm::I32ScalarKind::And))
    );
    decode1!(
        vm::op_i32_or,
        DecodedKind::Scalar(TypedScalarOp::I32(vm::I32ScalarKind::Or))
    );
    decode1!(
        vm::op_i32_xor,
        DecodedKind::Scalar(TypedScalarOp::I32(vm::I32ScalarKind::Xor))
    );
    decode1!(
        vm::op_i32_shl,
        DecodedKind::Scalar(TypedScalarOp::I32(vm::I32ScalarKind::Shl))
    );
    decode1!(
        vm::op_i32_shr_s,
        DecodedKind::Scalar(TypedScalarOp::I32(vm::I32ScalarKind::ShrS))
    );
    decode1!(
        vm::op_i32_shr_u,
        DecodedKind::Scalar(TypedScalarOp::I32(vm::I32ScalarKind::ShrU))
    );
    decode1!(
        vm::op_i32_div_s,
        DecodedKind::Scalar(TypedScalarOp::I32(vm::I32ScalarKind::DivS))
    );
    decode1!(
        vm::op_i32_div_u,
        DecodedKind::Scalar(TypedScalarOp::I32(vm::I32ScalarKind::DivU))
    );
    decode1!(
        vm::op_i32_rem_s,
        DecodedKind::Scalar(TypedScalarOp::I32(vm::I32ScalarKind::RemS))
    );
    decode1!(
        vm::op_i32_rem_u,
        DecodedKind::Scalar(TypedScalarOp::I32(vm::I32ScalarKind::RemU))
    );

    decode1!(
        vm::op_i64_add,
        DecodedKind::Scalar(TypedScalarOp::I64(vm::I64ScalarKind::Add))
    );
    decode1!(
        vm::op_i64_sub,
        DecodedKind::Scalar(TypedScalarOp::I64(vm::I64ScalarKind::Sub))
    );
    decode1!(
        vm::op_i64_mul,
        DecodedKind::Scalar(TypedScalarOp::I64(vm::I64ScalarKind::Mul))
    );
    decode1!(
        vm::op_i64_and,
        DecodedKind::Scalar(TypedScalarOp::I64(vm::I64ScalarKind::And))
    );
    decode1!(
        vm::op_i64_or,
        DecodedKind::Scalar(TypedScalarOp::I64(vm::I64ScalarKind::Or))
    );
    decode1!(
        vm::op_i64_xor,
        DecodedKind::Scalar(TypedScalarOp::I64(vm::I64ScalarKind::Xor))
    );
    decode1!(
        vm::op_i64_shl,
        DecodedKind::Scalar(TypedScalarOp::I64(vm::I64ScalarKind::Shl))
    );
    decode1!(
        vm::op_i64_shr_s,
        DecodedKind::Scalar(TypedScalarOp::I64(vm::I64ScalarKind::ShrS))
    );
    decode1!(
        vm::op_i64_shr_u,
        DecodedKind::Scalar(TypedScalarOp::I64(vm::I64ScalarKind::ShrU))
    );
    decode1!(
        vm::op_i64_div_s,
        DecodedKind::Scalar(TypedScalarOp::I64(vm::I64ScalarKind::DivS))
    );
    decode1!(
        vm::op_i64_div_u,
        DecodedKind::Scalar(TypedScalarOp::I64(vm::I64ScalarKind::DivU))
    );
    decode1!(
        vm::op_i64_rem_s,
        DecodedKind::Scalar(TypedScalarOp::I64(vm::I64ScalarKind::RemS))
    );
    decode1!(
        vm::op_i64_rem_u,
        DecodedKind::Scalar(TypedScalarOp::I64(vm::I64ScalarKind::RemU))
    );

    decode1!(
        vm::op_f32_add,
        DecodedKind::Scalar(TypedScalarOp::F32(vm::FloatScalarKind::Add))
    );
    decode1!(
        vm::op_f32_sub,
        DecodedKind::Scalar(TypedScalarOp::F32(vm::FloatScalarKind::Sub))
    );
    decode1!(
        vm::op_f32_mul,
        DecodedKind::Scalar(TypedScalarOp::F32(vm::FloatScalarKind::Mul))
    );
    decode1!(
        vm::op_f32_div,
        DecodedKind::Scalar(TypedScalarOp::F32(vm::FloatScalarKind::Div))
    );
    decode1!(
        vm::op_f64_add,
        DecodedKind::Scalar(TypedScalarOp::F64(vm::FloatScalarKind::Add))
    );
    decode1!(
        vm::op_f64_sub,
        DecodedKind::Scalar(TypedScalarOp::F64(vm::FloatScalarKind::Sub))
    );
    decode1!(
        vm::op_f64_mul,
        DecodedKind::Scalar(TypedScalarOp::F64(vm::FloatScalarKind::Mul))
    );
    decode1!(
        vm::op_f64_div,
        DecodedKind::Scalar(TypedScalarOp::F64(vm::FloatScalarKind::Div))
    );

    decode1!(
        vm::op_i32_eq,
        DecodedKind::Compare(TypedCompareOp::I32(vm::IntCompareKind::Eq))
    );
    decode1!(
        vm::op_i32_ne,
        DecodedKind::Compare(TypedCompareOp::I32(vm::IntCompareKind::Ne))
    );
    decode1!(
        vm::op_i32_lt_s,
        DecodedKind::Compare(TypedCompareOp::I32(vm::IntCompareKind::LtS))
    );
    decode1!(
        vm::op_i32_lt_u,
        DecodedKind::Compare(TypedCompareOp::I32(vm::IntCompareKind::LtU))
    );
    decode1!(
        vm::op_i32_gt_s,
        DecodedKind::Compare(TypedCompareOp::I32(vm::IntCompareKind::GtS))
    );
    decode1!(
        vm::op_i32_gt_u,
        DecodedKind::Compare(TypedCompareOp::I32(vm::IntCompareKind::GtU))
    );
    decode1!(
        vm::op_i32_le_s,
        DecodedKind::Compare(TypedCompareOp::I32(vm::IntCompareKind::LeS))
    );
    decode1!(
        vm::op_i32_le_u,
        DecodedKind::Compare(TypedCompareOp::I32(vm::IntCompareKind::LeU))
    );
    decode1!(
        vm::op_i32_ge_s,
        DecodedKind::Compare(TypedCompareOp::I32(vm::IntCompareKind::GeS))
    );
    decode1!(
        vm::op_i32_ge_u,
        DecodedKind::Compare(TypedCompareOp::I32(vm::IntCompareKind::GeU))
    );

    decode1!(
        vm::op_i64_eq,
        DecodedKind::Compare(TypedCompareOp::I64(vm::IntCompareKind::Eq))
    );
    decode1!(
        vm::op_i64_ne,
        DecodedKind::Compare(TypedCompareOp::I64(vm::IntCompareKind::Ne))
    );
    decode1!(
        vm::op_i64_lt_s,
        DecodedKind::Compare(TypedCompareOp::I64(vm::IntCompareKind::LtS))
    );
    decode1!(
        vm::op_i64_lt_u,
        DecodedKind::Compare(TypedCompareOp::I64(vm::IntCompareKind::LtU))
    );
    decode1!(
        vm::op_i64_gt_s,
        DecodedKind::Compare(TypedCompareOp::I64(vm::IntCompareKind::GtS))
    );
    decode1!(
        vm::op_i64_gt_u,
        DecodedKind::Compare(TypedCompareOp::I64(vm::IntCompareKind::GtU))
    );
    decode1!(
        vm::op_i64_le_s,
        DecodedKind::Compare(TypedCompareOp::I64(vm::IntCompareKind::LeS))
    );
    decode1!(
        vm::op_i64_le_u,
        DecodedKind::Compare(TypedCompareOp::I64(vm::IntCompareKind::LeU))
    );
    decode1!(
        vm::op_i64_ge_s,
        DecodedKind::Compare(TypedCompareOp::I64(vm::IntCompareKind::GeS))
    );
    decode1!(
        vm::op_i64_ge_u,
        DecodedKind::Compare(TypedCompareOp::I64(vm::IntCompareKind::GeU))
    );

    decode1!(
        vm::op_f32_eq,
        DecodedKind::Compare(TypedCompareOp::F32(vm::FloatCompareKind::Eq))
    );
    decode1!(
        vm::op_f32_ne,
        DecodedKind::Compare(TypedCompareOp::F32(vm::FloatCompareKind::Ne))
    );
    decode1!(
        vm::op_f32_lt,
        DecodedKind::Compare(TypedCompareOp::F32(vm::FloatCompareKind::Lt))
    );
    decode1!(
        vm::op_f32_gt,
        DecodedKind::Compare(TypedCompareOp::F32(vm::FloatCompareKind::Gt))
    );
    decode1!(
        vm::op_f32_le,
        DecodedKind::Compare(TypedCompareOp::F32(vm::FloatCompareKind::Le))
    );
    decode1!(
        vm::op_f32_ge,
        DecodedKind::Compare(TypedCompareOp::F32(vm::FloatCompareKind::Ge))
    );
    decode1!(
        vm::op_f64_eq,
        DecodedKind::Compare(TypedCompareOp::F64(vm::FloatCompareKind::Eq))
    );
    decode1!(
        vm::op_f64_ne,
        DecodedKind::Compare(TypedCompareOp::F64(vm::FloatCompareKind::Ne))
    );
    decode1!(
        vm::op_f64_lt,
        DecodedKind::Compare(TypedCompareOp::F64(vm::FloatCompareKind::Lt))
    );
    decode1!(
        vm::op_f64_gt,
        DecodedKind::Compare(TypedCompareOp::F64(vm::FloatCompareKind::Gt))
    );
    decode1!(
        vm::op_f64_le,
        DecodedKind::Compare(TypedCompareOp::F64(vm::FloatCompareKind::Le))
    );
    decode1!(
        vm::op_f64_ge,
        DecodedKind::Compare(TypedCompareOp::F64(vm::FloatCompareKind::Ge))
    );

    decode2!(
        vm::op_i32_load_local,
        DecodedKind::Load(TypedLoadOp::Bits4(vm::Load4Kind::I32), unsafe {
            raw[1].operand.memarg
        })
    );
    decode2!(
        vm::op_i32_load8_s_local,
        DecodedKind::Load(TypedLoadOp::Bits4(vm::Load4Kind::I32Load8S), unsafe {
            raw[1].operand.memarg
        })
    );
    decode2!(
        vm::op_i32_load8_u_local,
        DecodedKind::Load(TypedLoadOp::Bits4(vm::Load4Kind::I32Load8U), unsafe {
            raw[1].operand.memarg
        })
    );
    decode2!(
        vm::op_i32_load16_s_local,
        DecodedKind::Load(TypedLoadOp::Bits4(vm::Load4Kind::I32Load16S), unsafe {
            raw[1].operand.memarg
        })
    );
    decode2!(
        vm::op_i32_load16_u_local,
        DecodedKind::Load(TypedLoadOp::Bits4(vm::Load4Kind::I32Load16U), unsafe {
            raw[1].operand.memarg
        })
    );
    decode2!(
        vm::op_i64_load_local,
        DecodedKind::Load(TypedLoadOp::Bits8(vm::Load8Kind::I64), unsafe {
            raw[1].operand.memarg
        })
    );
    decode2!(
        vm::op_i64_load8_s_local,
        DecodedKind::Load(TypedLoadOp::Bits8(vm::Load8Kind::I64Load8S), unsafe {
            raw[1].operand.memarg
        })
    );
    decode2!(
        vm::op_i64_load8_u_local,
        DecodedKind::Load(TypedLoadOp::Bits8(vm::Load8Kind::I64Load8U), unsafe {
            raw[1].operand.memarg
        })
    );
    decode2!(
        vm::op_i64_load16_s_local,
        DecodedKind::Load(TypedLoadOp::Bits8(vm::Load8Kind::I64Load16S), unsafe {
            raw[1].operand.memarg
        })
    );
    decode2!(
        vm::op_i64_load16_u_local,
        DecodedKind::Load(TypedLoadOp::Bits8(vm::Load8Kind::I64Load16U), unsafe {
            raw[1].operand.memarg
        })
    );
    decode2!(
        vm::op_i64_load32_s_local,
        DecodedKind::Load(TypedLoadOp::Bits8(vm::Load8Kind::I64Load32S), unsafe {
            raw[1].operand.memarg
        })
    );
    decode2!(
        vm::op_i64_load32_u_local,
        DecodedKind::Load(TypedLoadOp::Bits8(vm::Load8Kind::I64Load32U), unsafe {
            raw[1].operand.memarg
        })
    );
    decode2!(
        vm::op_f32_load_local,
        DecodedKind::Load(TypedLoadOp::Bits4(vm::Load4Kind::F32), unsafe {
            raw[1].operand.memarg
        })
    );
    decode2!(
        vm::op_f64_load_local,
        DecodedKind::Load(TypedLoadOp::Bits8(vm::Load8Kind::F64), unsafe {
            raw[1].operand.memarg
        })
    );

    decode2!(
        vm::op_i32_store_local,
        DecodedKind::Store(TypedStoreOp::Bits4(vm::Store4Kind::I32), unsafe {
            raw[1].operand.memarg
        })
    );
    decode2!(
        vm::op_i32_store8_local,
        DecodedKind::Store(TypedStoreOp::Bits4(vm::Store4Kind::I32Store8), unsafe {
            raw[1].operand.memarg
        })
    );
    decode2!(
        vm::op_i32_store16_local,
        DecodedKind::Store(TypedStoreOp::Bits4(vm::Store4Kind::I32Store16), unsafe {
            raw[1].operand.memarg
        })
    );
    decode2!(
        vm::op_i64_store_local,
        DecodedKind::Store(TypedStoreOp::Bits8(vm::Store8Kind::I64), unsafe {
            raw[1].operand.memarg
        })
    );
    decode2!(
        vm::op_i64_store8_local,
        DecodedKind::Store(TypedStoreOp::Bits8(vm::Store8Kind::I64Store8), unsafe {
            raw[1].operand.memarg
        })
    );
    decode2!(
        vm::op_i64_store16_local,
        DecodedKind::Store(TypedStoreOp::Bits8(vm::Store8Kind::I64Store16), unsafe {
            raw[1].operand.memarg
        })
    );
    decode2!(
        vm::op_i64_store32_local,
        DecodedKind::Store(TypedStoreOp::Bits8(vm::Store8Kind::I64Store32), unsafe {
            raw[1].operand.memarg
        })
    );
    decode2!(
        vm::op_f32_store_local,
        DecodedKind::Store(TypedStoreOp::Bits4(vm::Store4Kind::F32), unsafe {
            raw[1].operand.memarg
        })
    );
    decode2!(
        vm::op_f64_store_local,
        DecodedKind::Store(TypedStoreOp::Bits8(vm::Store8Kind::F64), unsafe {
            raw[1].operand.memarg
        })
    );

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
            PRODUCER_IMM_AND_BRANCH_MATCHERS,
            decoded.as_slice(),
            index,
            jump_targets,
        ) {
            optimized.push(fused);
            index += consumed;
            continue;
        }
        if let Some((consumed, fused)) = try_matchers(
            PRODUCER_COMPARE_SELECT_MATCHERS,
            decoded.as_slice(),
            index,
            jump_targets,
        ) {
            optimized.push(fused);
            index += consumed;
            continue;
        }
        if let Some((consumed, fused)) = try_matchers(
            PRODUCER_IMM_SCALAR_SET_TEE_MATCHERS,
            decoded.as_slice(),
            index,
            jump_targets,
        ) {
            optimized.push(fused);
            index += consumed;
            continue;
        }
        if let Some((consumed, fused)) = try_matchers(
            PRODUCER_TEE_EQZ_BRANCH_MATCHERS,
            decoded.as_slice(),
            index,
            jump_targets,
        ) {
            optimized.push(fused);
            index += consumed;
            continue;
        }
        if let Some((consumed, fused)) = try_matchers(
            PRODUCER_TEE_IMM_COMPARE_BRANCH_MATCHERS,
            decoded.as_slice(),
            index,
            jump_targets,
        ) {
            optimized.push(fused);
            index += consumed;
            continue;
        }
        if let Some((consumed, fused)) = try_matchers(
            PRODUCER_TEE_IMM_SCALAR_SET_TEE_MATCHERS,
            decoded.as_slice(),
            index,
            jump_targets,
        ) {
            optimized.push(fused);
            index += consumed;
            continue;
        }
        if let Some((consumed, fused)) = try_matchers(
            PRODUCER_TEE_SELECT_MATCHERS,
            decoded.as_slice(),
            index,
            jump_targets,
        ) {
            optimized.push(fused);
            index += consumed;
            continue;
        }
        if let Some((consumed, fused)) = try_matchers(
            COMPARE_TEE_SELECT_MATCHERS,
            decoded.as_slice(),
            index,
            jump_targets,
        ) {
            optimized.push(fused);
            index += consumed;
            continue;
        }
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
        if let Some((consumed, fused)) = try_matchers(
            COMPARE_SET_TEE_MATCHERS,
            decoded.as_slice(),
            index,
            jump_targets,
        ) {
            optimized.push(fused);
            index += consumed;
            continue;
        }
        if let Some((consumed, fused)) = try_matchers(
            COMPARE_SELECT_MATCHERS,
            decoded.as_slice(),
            index,
            jump_targets,
        ) {
            optimized.push(fused);
            index += consumed;
            continue;
        }
        if let Some((consumed, fused)) = try_matchers(
            LOAD_MASK_BRANCH_MATCHERS,
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
        if let Some((consumed, fused)) = try_matchers(
            CONST_SET_TEE_MATCHERS,
            decoded.as_slice(),
            index,
            jump_targets,
        ) {
            optimized.push(fused);
            index += consumed;
            continue;
        }
        if let Some((consumed, fused)) =
            try_matchers(LOCAL_COPY_MATCHERS, decoded.as_slice(), index, jump_targets)
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
            LOAD_MODIFY_STORE_MATCHERS,
            decoded.as_slice(),
            index,
            jump_targets,
        ) {
            optimized.push(fused);
            index += consumed;
            continue;
        }
        if let Some((consumed, fused)) = try_matchers(
            LOAD_STORE_NARROW_COPY_MATCHERS,
            decoded.as_slice(),
            index,
            jump_targets,
        ) {
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
        if let Some((consumed, fused)) = try_matchers(
            LOCAL_IMM_PUSH_MATCHERS,
            decoded.as_slice(),
            index,
            jump_targets,
        ) {
            optimized.push(fused);
            index += consumed;
            continue;
        }
        if let Some((consumed, fused)) = try_matchers(
            LOCAL_LOCAL_PUSH_MATCHERS,
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

fn same_width(lhs: ValueSize, rhs: ValueSize) -> bool {
    matches!(
        (lhs, rhs),
        (ValueSize::Byte4, ValueSize::Byte4)
            | (ValueSize::Byte8, ValueSize::Byte8)
            | (ValueSize::Byte16, ValueSize::Byte16)
    )
}

fn local_get(kind: DecodedKind) -> Option<(ValueSize, u32)> {
    match kind {
        DecodedKind::LocalGet(width, local_addr) => Some((width, local_addr)),
        _ => None,
    }
}

fn local_set_tee(kind: DecodedKind) -> Option<(ValueSize, u32, bool)> {
    match kind {
        DecodedKind::LocalSet(width, local_addr) => Some((width, local_addr, false)),
        DecodedKind::LocalTee(width, local_addr) => Some((width, local_addr, true)),
        _ => None,
    }
}

fn select_width(kind: DecodedKind) -> Option<ValueSize> {
    match kind {
        DecodedKind::Select(width) => Some(width),
        _ => None,
    }
}

fn scalar_matches_const(op: TypedScalarOp, value: TypedConst) -> bool {
    matches!(
        (op, value),
        (TypedScalarOp::I32(_), TypedConst::I32(_))
            | (TypedScalarOp::I64(_), TypedConst::I64(_))
            | (TypedScalarOp::F32(_), TypedConst::F32(_))
            | (TypedScalarOp::F64(_), TypedConst::F64(_))
    )
}

fn compare_matches_const(op: TypedCompareOp, value: TypedConst) -> bool {
    matches!(
        (op, value),
        (TypedCompareOp::I32(_), TypedConst::I32(_))
            | (TypedCompareOp::I64(_), TypedConst::I64(_))
            | (TypedCompareOp::F32(_), TypedConst::F32(_))
            | (TypedCompareOp::F64(_), TypedConst::F64(_))
    )
}

fn is_existing_i32_local_imm_fastpath(op: TypedScalarOp) -> bool {
    matches!(
        op,
        TypedScalarOp::I32(
            vm::I32ScalarKind::Add
                | vm::I32ScalarKind::Sub
                | vm::I32ScalarKind::And
                | vm::I32ScalarKind::Shl
                | vm::I32ScalarKind::ShrU
        )
    )
}

fn is_existing_i32_local_local_fastpath(op: TypedScalarOp) -> bool {
    matches!(op, TypedScalarOp::I32(vm::I32ScalarKind::Add))
}

fn is_integer_scalar(op: TypedScalarOp) -> bool {
    matches!(op, TypedScalarOp::I32(_) | TypedScalarOp::I64(_))
}

fn is_supported_tee_consumer_scalar(op: TypedScalarOp) -> bool {
    matches!(
        op,
        TypedScalarOp::I32(
            vm::I32ScalarKind::Add
                | vm::I32ScalarKind::Sub
                | vm::I32ScalarKind::And
                | vm::I32ScalarKind::Or
                | vm::I32ScalarKind::Xor
                | vm::I32ScalarKind::Shl
                | vm::I32ScalarKind::ShrS
                | vm::I32ScalarKind::ShrU
        ) | TypedScalarOp::I64(
            vm::I64ScalarKind::Add
                | vm::I64ScalarKind::Sub
                | vm::I64ScalarKind::And
                | vm::I64ScalarKind::Or
                | vm::I64ScalarKind::Xor
                | vm::I64ScalarKind::Shl
                | vm::I64ScalarKind::ShrS
                | vm::I64ScalarKind::ShrU
        )
    )
}

fn is_integer_compare(op: TypedCompareOp) -> bool {
    matches!(op, TypedCompareOp::I32(_) | TypedCompareOp::I64(_))
}

fn is_seed_load(op: TypedLoadOp) -> bool {
    matches!(
        op,
        TypedLoadOp::Bits4(
            vm::Load4Kind::I32
                | vm::Load4Kind::I32Load8S
                | vm::Load4Kind::I32Load8U
                | vm::Load4Kind::I32Load16S
                | vm::Load4Kind::I32Load16U
                | vm::Load4Kind::F32
        ) | TypedLoadOp::Bits8(
            vm::Load8Kind::I64
                | vm::Load8Kind::I64Load8S
                | vm::Load8Kind::I64Load8U
                | vm::Load8Kind::I64Load16S
                | vm::Load8Kind::I64Load16U
                | vm::Load8Kind::I64Load32S
                | vm::Load8Kind::I64Load32U
                | vm::Load8Kind::F64
        )
    )
}

fn is_float_compare(op: TypedCompareOp) -> bool {
    matches!(op, TypedCompareOp::F32(_) | TypedCompareOp::F64(_))
}

fn is_float_load_seed_for_compare(seed: ProducerSeed, compare_op: TypedCompareOp) -> bool {
    matches!(
        (seed, compare_op),
        (
            ProducerSeed::LocalAddrLoad {
                op: TypedLoadOp::Bits4(vm::Load4Kind::F32),
                ..
            } | ProducerSeed::LocalImmAddrLoad {
                op: TypedLoadOp::Bits4(vm::Load4Kind::F32),
                ..
            } | ProducerSeed::ConstAddrLoad {
                op: TypedLoadOp::Bits4(vm::Load4Kind::F32),
                ..
            },
            TypedCompareOp::F32(_),
        ) | (
            ProducerSeed::LocalAddrLoad {
                op: TypedLoadOp::Bits8(vm::Load8Kind::F64),
                ..
            } | ProducerSeed::LocalImmAddrLoad {
                op: TypedLoadOp::Bits8(vm::Load8Kind::F64),
                ..
            } | ProducerSeed::ConstAddrLoad {
                op: TypedLoadOp::Bits8(vm::Load8Kind::F64),
                ..
            },
            TypedCompareOp::F64(_),
        )
    )
}

fn match_producer_seed(decoded: &[DecodedInstruction], index: usize) -> Option<ProducerSeedMatch> {
    match_local_imm_addr_load_seed(decoded, index)
        .or_else(|| match_const_addr_load_seed(decoded, index))
        .or_else(|| match_local_addr_load_seed(decoded, index))
        .or_else(|| match_local_imm_scalar_seed(decoded, index))
        .or_else(|| match_local_local_scalar_seed(decoded, index))
        .or_else(|| match_local_seed(decoded, index))
}

fn match_local_seed(decoded: &[DecodedInstruction], index: usize) -> Option<ProducerSeedMatch> {
    let first = decoded.get(index)?;
    let (width, local_addr) = local_get(first.kind)?;
    if !matches!(width, ValueSize::Byte4 | ValueSize::Byte8) {
        return None;
    }
    Some(ProducerSeedMatch {
        seed: ProducerSeed::Local { width, local_addr },
        consumed: 1,
    })
}

fn match_local_imm_scalar_seed(
    decoded: &[DecodedInstruction],
    index: usize,
) -> Option<ProducerSeedMatch> {
    let [first, second, third] = decoded.get(index..index + 3)? else {
        return None;
    };
    let (width, src_local) = local_get(first.kind)?;
    let imm = match second.kind {
        DecodedKind::Const(value) => value,
        _ => return None,
    };
    let op = match third.kind {
        DecodedKind::Scalar(op) => op,
        _ => return None,
    };
    if !matches!(width, ValueSize::Byte4 | ValueSize::Byte8)
        || !same_width(width, imm.width())
        || !same_width(width, op.width())
        || !scalar_matches_const(op, imm)
        || !is_integer_scalar(op)
    {
        return None;
    }
    Some(ProducerSeedMatch {
        seed: ProducerSeed::LocalImmScalar {
            width,
            src_local,
            imm,
            op,
        },
        consumed: 3,
    })
}

fn match_local_local_scalar_seed(
    decoded: &[DecodedInstruction],
    index: usize,
) -> Option<ProducerSeedMatch> {
    let [first, second, third] = decoded.get(index..index + 3)? else {
        return None;
    };
    let (lhs_width, lhs_local_addr) = local_get(first.kind)?;
    let (rhs_width, rhs_local_addr) = local_get(second.kind)?;
    let op = match third.kind {
        DecodedKind::Scalar(op) => op,
        _ => return None,
    };
    if !matches!(lhs_width, ValueSize::Byte4 | ValueSize::Byte8)
        || !same_width(lhs_width, rhs_width)
        || !same_width(lhs_width, op.width())
        || !is_integer_scalar(op)
    {
        return None;
    }
    Some(ProducerSeedMatch {
        seed: ProducerSeed::LocalLocalScalar {
            width: lhs_width,
            lhs_local_addr,
            rhs_local_addr,
            op,
        },
        consumed: 3,
    })
}

fn match_local_addr_load_seed(
    decoded: &[DecodedInstruction],
    index: usize,
) -> Option<ProducerSeedMatch> {
    let [first, second] = decoded.get(index..index + 2)? else {
        return None;
    };
    let (addr_width, local_addr) = local_get(first.kind)?;
    let (op, memarg) = match second.kind {
        DecodedKind::Load(op, memarg) => (op, memarg),
        _ => return None,
    };
    if !same_width(addr_width, ValueSize::Byte4) || !is_seed_load(op) {
        return None;
    }
    Some(ProducerSeedMatch {
        seed: ProducerSeed::LocalAddrLoad {
            width: op.width(),
            local_addr,
            memarg,
            op,
        },
        consumed: 2,
    })
}

fn match_local_imm_addr_load_seed(
    decoded: &[DecodedInstruction],
    index: usize,
) -> Option<ProducerSeedMatch> {
    let [first, second, third, fourth] = decoded.get(index..index + 4)? else {
        return None;
    };
    let (addr_width, local_addr) = local_get(first.kind)?;
    let imm = match second.kind {
        DecodedKind::Const(TypedConst::I32(value)) => value,
        _ => return None,
    };
    if !matches!(
        third.kind,
        DecodedKind::Scalar(TypedScalarOp::I32(vm::I32ScalarKind::Add))
    ) {
        return None;
    }
    let (op, memarg) = match fourth.kind {
        DecodedKind::Load(op, memarg) => (op, memarg),
        _ => return None,
    };
    if !same_width(addr_width, ValueSize::Byte4) || !is_seed_load(op) {
        return None;
    }
    Some(ProducerSeedMatch {
        seed: ProducerSeed::LocalImmAddrLoad {
            width: op.width(),
            local_addr,
            imm,
            memarg,
            op,
        },
        consumed: 4,
    })
}

fn match_const_addr_load_seed(
    decoded: &[DecodedInstruction],
    index: usize,
) -> Option<ProducerSeedMatch> {
    let [first, second] = decoded.get(index..index + 2)? else {
        return None;
    };
    let addr = match first.kind {
        DecodedKind::Const(TypedConst::I32(value)) => value,
        _ => return None,
    };
    let (op, memarg) = match second.kind {
        DecodedKind::Load(op, memarg) => (op, memarg),
        _ => return None,
    };
    if !is_seed_load(op) {
        return None;
    }
    let start = match compute_memory_offset(memarg, addr as u32) {
        VMResult::Success(start) => u32::try_from(start).ok()?,
        _ => return None,
    };
    Some(ProducerSeedMatch {
        seed: ProducerSeed::ConstAddrLoad {
            width: op.width(),
            start,
            op,
        },
        consumed: 2,
    })
}

fn has_nontrivial_seed(seed_match: &ProducerSeedMatch) -> bool {
    seed_match.consumed > 1
}

fn match_producer_imm_and_branch(
    decoded: &[DecodedInstruction],
    index: usize,
    jump_targets: &HashSet<usize>,
) -> Option<MatchOutcome> {
    let seed_match = match_producer_seed(decoded, index)?;
    if !has_nontrivial_seed(&seed_match) {
        return None;
    }
    let const_index = index + seed_match.consumed;
    let scalar_index = const_index + 1;
    let branch_or_eqz = decoded.get(scalar_index + 1)?;
    let rhs_const = match decoded.get(const_index)?.kind {
        DecodedKind::Const(value) => value,
        _ => return None,
    };
    let scalar_op = match decoded.get(scalar_index)?.kind {
        DecodedKind::Scalar(op) => op,
        _ => return None,
    };
    let width = seed_match.seed.width();
    if !same_width(width, rhs_const.width())
        || !same_width(width, scalar_op.width())
        || !scalar_matches_const(scalar_op, rhs_const)
        || !matches!(
            scalar_op,
            TypedScalarOp::I32(vm::I32ScalarKind::And) | TypedScalarOp::I64(vm::I64ScalarKind::And)
        )
    {
        return None;
    }

    let (zero_test, branch_kind, target_old, branch_index, end) = match branch_or_eqz.kind {
        DecodedKind::Eqz(eqz_width) => {
            let branch = decoded.get(scalar_index + 2)?;
            if !same_width(width, eqz_width) {
                return None;
            }
            let (branch_kind, target_old) = match branch.kind {
                DecodedKind::BrIf(target) => (ControlBranchKind::BrIf, target),
                DecodedKind::If(target) => (ControlBranchKind::If, target),
                _ => return None,
            };
            (
                true,
                branch_kind,
                target_old,
                scalar_index + 2,
                branch.old_range.end,
            )
        }
        DecodedKind::BrIf(target) => (
            false,
            ControlBranchKind::BrIf,
            target,
            scalar_index + 1,
            branch_or_eqz.old_range.end,
        ),
        DecodedKind::If(target) => (
            false,
            ControlBranchKind::If,
            target,
            scalar_index + 1,
            branch_or_eqz.old_range.end,
        ),
        _ => return None,
    };
    if sequence_crosses_jump_targets(decoded, jump_targets, index + 1..branch_index + 1) {
        return None;
    }
    Some((
        branch_index - index + 1,
        OptimizedInstruction::ProducerImmAndBranch {
            old_range: decoded[index].old_range.start..end,
            seed: seed_match.seed,
            rhs_const,
            width,
            target_old,
            zero_test,
            branch_kind,
        },
    ))
}

fn match_producer_imm_scalar_set_tee(
    decoded: &[DecodedInstruction],
    index: usize,
    jump_targets: &HashSet<usize>,
) -> Option<MatchOutcome> {
    let seed_match = match_producer_seed(decoded, index)?;
    if !has_nontrivial_seed(&seed_match) {
        return None;
    }
    let const_index = index + seed_match.consumed;
    let scalar_index = const_index + 1;
    let set_tee_index = const_index + 2;
    let rhs_const = match decoded.get(const_index)?.kind {
        DecodedKind::Const(value) => value,
        _ => return None,
    };
    let scalar_op = match decoded.get(scalar_index)?.kind {
        DecodedKind::Scalar(op) => op,
        _ => return None,
    };
    let set_tee = decoded.get(set_tee_index)?;
    if sequence_crosses_jump_targets(decoded, jump_targets, index + 1..set_tee_index + 1) {
        return None;
    }
    let (dst_width, dst_local, dst_tee) = local_set_tee(set_tee.kind)?;
    let width = seed_match.seed.width();
    if !same_width(width, rhs_const.width())
        || !same_width(width, scalar_op.width())
        || !same_width(width, dst_width)
        || !scalar_matches_const(scalar_op, rhs_const)
        || !is_supported_tee_consumer_scalar(scalar_op)
    {
        return None;
    }
    Some((
        set_tee_index - index + 1,
        OptimizedInstruction::ProducerImmScalarSetTee {
            old_range: decoded[index].old_range.start..set_tee.old_range.end,
            seed: seed_match.seed,
            rhs_const,
            dst_local,
            dst_tee,
            op: scalar_op,
        },
    ))
}

fn match_producer_local_compare_select(
    decoded: &[DecodedInstruction],
    index: usize,
    jump_targets: &HashSet<usize>,
) -> Option<MatchOutcome> {
    let seed_match = match_producer_seed(decoded, index)?;
    if !has_nontrivial_seed(&seed_match) {
        return None;
    }
    let rhs_index = index + seed_match.consumed;
    let compare_index = rhs_index + 1;
    let select_index = rhs_index + 2;
    let (rhs_width, rhs_local_addr) = local_get(decoded.get(rhs_index)?.kind)?;
    let compare_op = match decoded.get(compare_index)?.kind {
        DecodedKind::Compare(op) => op,
        _ => return None,
    };
    let select_width = select_width(decoded.get(select_index)?.kind)?;
    let seed = seed_match.seed;
    if sequence_crosses_jump_targets(decoded, jump_targets, index + 1..select_index + 1) {
        return None;
    }
    if !same_width(seed.width(), rhs_width)
        || !same_width(seed.width(), compare_op.width())
        || !matches!(select_width, ValueSize::Byte4 | ValueSize::Byte8)
        || (is_float_compare(compare_op) && !is_float_load_seed_for_compare(seed, compare_op))
    {
        return None;
    }
    Some((
        select_index - index + 1,
        OptimizedInstruction::ProducerCompareSelectLocal {
            old_range: decoded[index].old_range.start..decoded[select_index].old_range.end,
            seed,
            rhs_local_addr,
            select_width,
            op: compare_op,
        },
    ))
}

fn match_producer_const_compare_select(
    decoded: &[DecodedInstruction],
    index: usize,
    jump_targets: &HashSet<usize>,
) -> Option<MatchOutcome> {
    let seed_match = match_producer_seed(decoded, index)?;
    if !has_nontrivial_seed(&seed_match) {
        return None;
    }
    let const_index = index + seed_match.consumed;
    let compare_index = const_index + 1;
    let select_index = const_index + 2;
    let rhs_const = match decoded.get(const_index)?.kind {
        DecodedKind::Const(value) => value,
        _ => return None,
    };
    let compare_op = match decoded.get(compare_index)?.kind {
        DecodedKind::Compare(op) => op,
        _ => return None,
    };
    let select_width = select_width(decoded.get(select_index)?.kind)?;
    let seed = seed_match.seed;
    if sequence_crosses_jump_targets(decoded, jump_targets, index + 1..select_index + 1) {
        return None;
    }
    if !same_width(seed.width(), rhs_const.width())
        || !same_width(seed.width(), compare_op.width())
        || !compare_matches_const(compare_op, rhs_const)
        || !matches!(select_width, ValueSize::Byte4 | ValueSize::Byte8)
        || (is_float_compare(compare_op) && !is_float_load_seed_for_compare(seed, compare_op))
    {
        return None;
    }
    Some((
        select_index - index + 1,
        OptimizedInstruction::ProducerCompareSelectConst {
            old_range: decoded[index].old_range.start..decoded[select_index].old_range.end,
            seed,
            rhs_const,
            select_width,
            op: compare_op,
        },
    ))
}

fn match_producer_tee_eqz_branch(
    decoded: &[DecodedInstruction],
    index: usize,
    jump_targets: &HashSet<usize>,
) -> Option<MatchOutcome> {
    let seed_match = match_producer_seed(decoded, index)?;
    let tee_index = index + seed_match.consumed;
    let eqz_index = tee_index + 1;
    let branch_index = tee_index + 2;
    let tee = decoded.get(tee_index)?;
    let eqz = decoded.get(eqz_index)?;
    let branch = decoded.get(branch_index)?;
    if sequence_crosses_jump_targets(decoded, jump_targets, index + 1..branch_index + 1) {
        return None;
    }
    let (tee_width, tee_local_addr, tee) = local_set_tee(tee.kind)?;
    if !tee || !same_width(seed_match.seed.width(), tee_width) {
        return None;
    }
    let eqz_width = match eqz.kind {
        DecodedKind::Eqz(width) => width,
        _ => return None,
    };
    if !same_width(tee_width, eqz_width) {
        return None;
    }
    let (branch_kind, target_old) = match branch.kind {
        DecodedKind::BrIf(target) => (ControlBranchKind::BrIf, target),
        DecodedKind::If(target) => (ControlBranchKind::If, target),
        _ => return None,
    };
    Some((
        seed_match.consumed + 3,
        OptimizedInstruction::ProducerTeeEqzBranch {
            old_range: decoded[index].old_range.start..branch.old_range.end,
            seed: seed_match.seed,
            tee_local_addr,
            target_old,
            width: tee_width,
            branch_kind,
        },
    ))
}

fn match_producer_tee_imm_compare_branch(
    decoded: &[DecodedInstruction],
    index: usize,
    jump_targets: &HashSet<usize>,
) -> Option<MatchOutcome> {
    let seed_match = match_producer_seed(decoded, index)?;
    let tee_index = index + seed_match.consumed;
    let const_index = tee_index + 1;
    let compare_index = tee_index + 2;
    let branch_index = tee_index + 3;
    let tee = decoded.get(tee_index)?;
    let rhs_const = match decoded.get(const_index)?.kind {
        DecodedKind::Const(value) => value,
        _ => return None,
    };
    let compare_op = match decoded.get(compare_index)?.kind {
        DecodedKind::Compare(op) => op,
        _ => return None,
    };
    let branch = decoded.get(branch_index)?;
    if sequence_crosses_jump_targets(decoded, jump_targets, index + 1..branch_index + 1) {
        return None;
    }
    let (tee_width, tee_local_addr, tee) = local_set_tee(tee.kind)?;
    if !tee
        || !same_width(seed_match.seed.width(), tee_width)
        || !same_width(tee_width, rhs_const.width())
        || !same_width(tee_width, compare_op.width())
        || !is_integer_compare(compare_op)
        || !compare_matches_const(compare_op, rhs_const)
    {
        return None;
    }
    let (branch_kind, target_old) = match branch.kind {
        DecodedKind::BrIf(target) => (ControlBranchKind::BrIf, target),
        DecodedKind::If(target) => (ControlBranchKind::If, target),
        _ => return None,
    };
    Some((
        seed_match.consumed + 4,
        OptimizedInstruction::ProducerTeeImmCompareBranch {
            old_range: decoded[index].old_range.start..branch.old_range.end,
            seed: seed_match.seed,
            tee_local_addr,
            rhs_const,
            target_old,
            op: compare_op,
            branch_kind,
        },
    ))
}

fn match_producer_tee_imm_scalar_set_tee(
    decoded: &[DecodedInstruction],
    index: usize,
    jump_targets: &HashSet<usize>,
) -> Option<MatchOutcome> {
    let seed_match = match_producer_seed(decoded, index)?;
    let tee_index = index + seed_match.consumed;
    let const_index = tee_index + 1;
    let scalar_index = tee_index + 2;
    let set_tee_index = tee_index + 3;
    let tee = decoded.get(tee_index)?;
    let rhs_const = match decoded.get(const_index)?.kind {
        DecodedKind::Const(value) => value,
        _ => return None,
    };
    let scalar_op = match decoded.get(scalar_index)?.kind {
        DecodedKind::Scalar(op) => op,
        _ => return None,
    };
    let set_tee = decoded.get(set_tee_index)?;
    if sequence_crosses_jump_targets(decoded, jump_targets, index + 1..set_tee_index + 1) {
        return None;
    }
    let (tee_width, tee_local_addr, tee) = local_set_tee(tee.kind)?;
    let (dst_width, dst_local, dst_tee) = local_set_tee(set_tee.kind)?;
    if !tee
        || !same_width(seed_match.seed.width(), tee_width)
        || !same_width(tee_width, rhs_const.width())
        || !same_width(tee_width, scalar_op.width())
        || !same_width(tee_width, dst_width)
        || !is_supported_tee_consumer_scalar(scalar_op)
        || !scalar_matches_const(scalar_op, rhs_const)
    {
        return None;
    }
    Some((
        seed_match.consumed + 4,
        OptimizedInstruction::ProducerTeeImmScalarSetTee {
            old_range: decoded[index].old_range.start..decoded[set_tee_index].old_range.end,
            seed: seed_match.seed,
            tee_local_addr,
            rhs_const,
            dst_local,
            dst_tee,
            op: scalar_op,
        },
    ))
}

fn match_producer_tee_const_self_select(
    decoded: &[DecodedInstruction],
    index: usize,
    jump_targets: &HashSet<usize>,
) -> Option<MatchOutcome> {
    let seed_match = match_producer_seed(decoded, index)?;
    let tee_index = index + seed_match.consumed;
    let const_index = tee_index + 1;
    let get_index = tee_index + 2;
    let select_index = tee_index + 3;
    let tee = decoded.get(tee_index)?;
    let rhs_const = match decoded.get(const_index)?.kind {
        DecodedKind::Const(value) => value,
        _ => return None,
    };
    let (tee_width, tee_local_addr, tee) = local_set_tee(tee.kind)?;
    let (self_width, self_local_addr) = local_get(decoded.get(get_index)?.kind)?;
    let select_width = select_width(decoded.get(select_index)?.kind)?;
    if sequence_crosses_jump_targets(decoded, jump_targets, index + 1..select_index + 1) {
        return None;
    }
    if !tee
        || tee_local_addr != self_local_addr
        || !same_width(seed_match.seed.width(), tee_width)
        || !same_width(tee_width, rhs_const.width())
        || !same_width(tee_width, self_width)
        || !same_width(tee_width, select_width)
    {
        return None;
    }
    Some((
        seed_match.consumed + 4,
        OptimizedInstruction::ProducerTeeConstSelfSelect {
            old_range: decoded[index].old_range.start..decoded[select_index].old_range.end,
            seed: seed_match.seed,
            tee_local_addr,
            rhs_const,
            width: tee_width,
        },
    ))
}

fn match_local_local_compare_tee_select(
    decoded: &[DecodedInstruction],
    index: usize,
    jump_targets: &HashSet<usize>,
) -> Option<MatchOutcome> {
    let [first, second, third, fourth, fifth] = decoded.get(index..index + 5)? else {
        return None;
    };
    if sequence_crosses_jump_targets(decoded, jump_targets, index + 1..index + 5) {
        return None;
    }
    let (lhs_width, lhs_local_addr) = local_get(first.kind)?;
    let (rhs_width, rhs_local_addr) = local_get(second.kind)?;
    let compare_op = match third.kind {
        DecodedKind::Compare(op) => op,
        _ => return None,
    };
    let (tee_width, tee_local_addr, tee) = local_set_tee(fourth.kind)?;
    let select_width = select_width(fifth.kind)?;
    if !tee
        || !same_width(lhs_width, rhs_width)
        || !same_width(lhs_width, compare_op.width())
        || !same_width(tee_width, ValueSize::Byte4)
        || !matches!(lhs_width, ValueSize::Byte4 | ValueSize::Byte8)
        || !matches!(select_width, ValueSize::Byte4 | ValueSize::Byte8)
        || !is_integer_compare(compare_op)
    {
        return None;
    }
    Some((
        5,
        OptimizedInstruction::CompareTeeSelectLocal {
            old_range: first.old_range.start..fifth.old_range.end,
            lhs_local_addr,
            rhs_local_addr,
            tee_local_addr,
            select_width,
            op: compare_op,
        },
    ))
}

fn match_local_const_compare_tee_select(
    decoded: &[DecodedInstruction],
    index: usize,
    jump_targets: &HashSet<usize>,
) -> Option<MatchOutcome> {
    let [first, second, third, fourth, fifth] = decoded.get(index..index + 5)? else {
        return None;
    };
    if sequence_crosses_jump_targets(decoded, jump_targets, index + 1..index + 5) {
        return None;
    }
    let (lhs_width, lhs_local_addr) = local_get(first.kind)?;
    let rhs_const = match second.kind {
        DecodedKind::Const(value) => value,
        _ => return None,
    };
    let compare_op = match third.kind {
        DecodedKind::Compare(op) => op,
        _ => return None,
    };
    let (tee_width, tee_local_addr, tee) = local_set_tee(fourth.kind)?;
    let select_width = select_width(fifth.kind)?;
    if !tee
        || !same_width(lhs_width, compare_op.width())
        || !same_width(lhs_width, rhs_const.width())
        || !same_width(tee_width, ValueSize::Byte4)
        || !matches!(lhs_width, ValueSize::Byte4 | ValueSize::Byte8)
        || !matches!(select_width, ValueSize::Byte4 | ValueSize::Byte8)
        || !is_integer_compare(compare_op)
        || !compare_matches_const(compare_op, rhs_const)
    {
        return None;
    }
    Some((
        5,
        OptimizedInstruction::CompareTeeSelectConst {
            old_range: first.old_range.start..fifth.old_range.end,
            lhs_local_addr,
            rhs_const,
            tee_local_addr,
            select_width,
            op: compare_op,
        },
    ))
}

fn match_local_imm_scalar_set_tee(
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
    let (src_width, src_local) = local_get(first.kind)?;
    if !matches!(src_width, ValueSize::Byte4 | ValueSize::Byte8) {
        return None;
    }
    let imm = match second.kind {
        DecodedKind::Const(value) => value,
        _ => return None,
    };
    let op = match third.kind {
        DecodedKind::Scalar(op) => op,
        _ => return None,
    };
    let (dst_width, dst_local, tee) = local_set_tee(fourth.kind)?;

    if !same_width(src_width, imm.width())
        || !same_width(src_width, op.width())
        || !same_width(src_width, dst_width)
        || !scalar_matches_const(op, imm)
    {
        return None;
    }

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

fn match_local_copy(
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
    let (src_width, src_local) = local_get(first.kind)?;
    let (dst_width, dst_local, tee) = local_set_tee(second.kind)?;
    if !matches!(src_width, ValueSize::Byte4 | ValueSize::Byte8)
        || !same_width(src_width, dst_width)
    {
        return None;
    }

    Some((
        2,
        OptimizedInstruction::LocalCopy {
            old_range: first.old_range.start..second.old_range.end,
            src_local,
            dst_local,
            width: src_width,
            tee,
        },
    ))
}

fn match_const_set_tee(
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
    let value = match first.kind {
        DecodedKind::Const(value) => value,
        _ => return None,
    };
    let (dst_width, dst_local, tee) = local_set_tee(second.kind)?;
    if !matches!(dst_width, ValueSize::Byte4 | ValueSize::Byte8)
        || !same_width(value.width(), dst_width)
    {
        return None;
    }

    Some((
        2,
        OptimizedInstruction::ConstSetTee {
            old_range: first.old_range.start..second.old_range.end,
            value,
            dst_local,
            tee,
        },
    ))
}

fn match_local_imm_scalar_push(
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
    let (lhs_width, src_local) = local_get(first.kind)?;
    let imm = match second.kind {
        DecodedKind::Const(value) => value,
        _ => return None,
    };
    let op = match third.kind {
        DecodedKind::Scalar(op) => op,
        _ => return None,
    };

    if !same_width(lhs_width, ValueSize::Byte4)
        || !same_width(lhs_width, imm.width())
        || !same_width(lhs_width, op.width())
        || !matches!(imm, TypedConst::I32(_))
        || !matches!(op, TypedScalarOp::I32(_))
    {
        return None;
    }

    Some((
        3,
        OptimizedInstruction::LocalImmPush {
            old_range: first.old_range.start..third.old_range.end,
            src_local,
            imm,
            op,
        },
    ))
}

fn match_local_local_scalar_set_tee(
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
    let (lhs_width, lhs_local_addr) = local_get(first.kind)?;
    let (rhs_width, rhs_local_addr) = local_get(second.kind)?;
    let op = match third.kind {
        DecodedKind::Scalar(op) => op,
        _ => return None,
    };
    let (dst_width, dst_local, tee) = local_set_tee(fourth.kind)?;

    if !matches!(lhs_width, ValueSize::Byte4 | ValueSize::Byte8)
        || !same_width(lhs_width, rhs_width)
        || !same_width(lhs_width, op.width())
        || !same_width(lhs_width, dst_width)
    {
        return None;
    }

    Some((
        4,
        OptimizedInstruction::LocalLocalSetTee {
            old_range: first.old_range.start..fourth.old_range.end,
            lhs_local_addr,
            rhs_local_addr,
            dst_local,
            tee,
            op,
        },
    ))
}

fn match_local_local_scalar_push(
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
    let (lhs_width, lhs_local_addr) = local_get(first.kind)?;
    let (rhs_width, rhs_local_addr) = local_get(second.kind)?;
    let op = match third.kind {
        DecodedKind::Scalar(op) => op,
        _ => return None,
    };

    if !same_width(lhs_width, ValueSize::Byte4)
        || !same_width(lhs_width, rhs_width)
        || !same_width(lhs_width, op.width())
        || !matches!(op, TypedScalarOp::I32(_))
    {
        return None;
    }

    Some((
        3,
        OptimizedInstruction::LocalLocalPush {
            old_range: first.old_range.start..third.old_range.end,
            lhs_local_addr,
            rhs_local_addr,
            op,
        },
    ))
}

fn match_i32_local_and_imm_branch(
    decoded: &[DecodedInstruction],
    index: usize,
    jump_targets: &HashSet<usize>,
) -> Option<MatchOutcome> {
    let [first, second, third, fourth] = decoded.get(index..index + 4)? else {
        return None;
    };
    let (width, local_addr) = local_get(first.kind)?;
    let imm = match second.kind {
        DecodedKind::Const(TypedConst::I32(value)) => value,
        _ => return None,
    };
    if !same_width(width, ValueSize::Byte4)
        || !matches!(
            third.kind,
            DecodedKind::Scalar(TypedScalarOp::I32(vm::I32ScalarKind::And))
        )
    {
        return None;
    }

    let (zero_test, branch_kind, target_old, end, consumed) = match fourth.kind {
        DecodedKind::Eqz(ValueSize::Byte4) => {
            let fifth = decoded.get(index + 4)?;
            if sequence_crosses_jump_targets(decoded, jump_targets, index + 1..index + 5) {
                return None;
            }
            match fifth.kind {
                DecodedKind::BrIf(target) => (
                    true,
                    ControlBranchKind::BrIf,
                    target,
                    fifth.old_range.end,
                    5,
                ),
                DecodedKind::If(target) => {
                    (true, ControlBranchKind::If, target, fifth.old_range.end, 5)
                }
                _ => return None,
            }
        }
        DecodedKind::BrIf(target) => {
            if sequence_crosses_jump_targets(decoded, jump_targets, index + 1..index + 4) {
                return None;
            }
            (
                false,
                ControlBranchKind::BrIf,
                target,
                fourth.old_range.end,
                4,
            )
        }
        DecodedKind::If(target) => {
            if sequence_crosses_jump_targets(decoded, jump_targets, index + 1..index + 4) {
                return None;
            }
            (
                false,
                ControlBranchKind::If,
                target,
                fourth.old_range.end,
                4,
            )
        }
        _ => return None,
    };

    Some((
        consumed,
        OptimizedInstruction::I32LocalAndImmBranch {
            old_range: first.old_range.start..end,
            local_addr,
            imm,
            target_old,
            zero_test,
            branch_kind,
        },
    ))
}

fn match_i32_local_addr_load8_u_and_imm_eqz_branch(
    decoded: &[DecodedInstruction],
    index: usize,
    jump_targets: &HashSet<usize>,
) -> Option<MatchOutcome> {
    let [first, second, third, fourth, fifth, sixth] = decoded.get(index..index + 6)? else {
        return None;
    };
    if sequence_crosses_jump_targets(decoded, jump_targets, index + 1..index + 6) {
        return None;
    }
    let (addr_width, local_addr) = local_get(first.kind)?;
    let (load_op, memarg) = match second.kind {
        DecodedKind::Load(op, memarg) => (op, memarg),
        _ => return None,
    };
    let imm = match third.kind {
        DecodedKind::Const(TypedConst::I32(value)) => value,
        _ => return None,
    };
    if !same_width(addr_width, ValueSize::Byte4)
        || !matches!(load_op, TypedLoadOp::Bits4(vm::Load4Kind::I32Load8U))
        || !matches!(
            fourth.kind,
            DecodedKind::Scalar(TypedScalarOp::I32(vm::I32ScalarKind::And))
        )
        || !matches!(fifth.kind, DecodedKind::Eqz(ValueSize::Byte4))
    {
        return None;
    }
    let (branch_kind, target_old) = match sixth.kind {
        DecodedKind::BrIf(target) => (ControlBranchKind::BrIf, target),
        DecodedKind::If(target) => (ControlBranchKind::If, target),
        _ => return None,
    };

    Some((
        6,
        OptimizedInstruction::I32LocalAddrLoad8UAndImmEqzBranch {
            old_range: first.old_range.start..sixth.old_range.end,
            local_addr,
            memarg,
            imm,
            target_old,
            branch_kind,
        },
    ))
}

fn match_local_branch(
    decoded: &[DecodedInstruction],
    index: usize,
    jump_targets: &HashSet<usize>,
) -> Option<MatchOutcome> {
    let [first, second] = decoded.get(index..index + 2)? else {
        return None;
    };
    let (width, local_addr) = local_get(first.kind)?;
    if !matches!(width, ValueSize::Byte4 | ValueSize::Byte8) {
        return None;
    }

    let (zero_test, branch_kind, target_old, end, consumed) = match second.kind {
        DecodedKind::Eqz(eqz_width) => {
            let third = decoded.get(index + 2)?;
            if !same_width(width, eqz_width)
                || sequence_crosses_jump_targets(decoded, jump_targets, index + 1..index + 3)
            {
                return None;
            }
            match third.kind {
                DecodedKind::BrIf(target) => (
                    true,
                    ControlBranchKind::BrIf,
                    target,
                    third.old_range.end,
                    3,
                ),
                DecodedKind::If(target) => {
                    (true, ControlBranchKind::If, target, third.old_range.end, 3)
                }
                _ => return None,
            }
        }
        DecodedKind::BrIf(target) => {
            if sequence_crosses_jump_targets(decoded, jump_targets, index + 1..index + 2) {
                return None;
            }
            (
                false,
                ControlBranchKind::BrIf,
                target,
                second.old_range.end,
                2,
            )
        }
        DecodedKind::If(target) => {
            if sequence_crosses_jump_targets(decoded, jump_targets, index + 1..index + 2) {
                return None;
            }
            (
                false,
                ControlBranchKind::If,
                target,
                second.old_range.end,
                2,
            )
        }
        _ => return None,
    };

    Some((
        consumed,
        OptimizedInstruction::LocalBranch {
            old_range: first.old_range.start..end,
            local_addr,
            target_old,
            width,
            zero_test,
            branch_kind,
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
    let (lhs_width, lhs_local_addr) = local_get(first.kind)?;
    let (rhs_width, rhs_local_addr) = local_get(second.kind)?;
    let target_old = match fourth.kind {
        DecodedKind::BrIf(target) => target,
        _ => return None,
    };
    if !same_width(lhs_width, ValueSize::Byte4) || !same_width(rhs_width, ValueSize::Byte4) {
        return None;
    }
    if !matches!(
        third.kind,
        DecodedKind::Compare(TypedCompareOp::I32(vm::IntCompareKind::GeU))
    ) {
        return None;
    }

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

fn match_local_local_compare_set_tee(
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
    let (lhs_width, lhs_local_addr) = local_get(first.kind)?;
    let (rhs_width, rhs_local_addr) = local_get(second.kind)?;
    let op = match third.kind {
        DecodedKind::Compare(op) => op,
        _ => return None,
    };
    let (dst_width, dst_local, tee) = local_set_tee(fourth.kind)?;
    if !same_width(lhs_width, rhs_width)
        || !same_width(lhs_width, op.width())
        || !same_width(dst_width, ValueSize::Byte4)
        || !matches!(lhs_width, ValueSize::Byte4 | ValueSize::Byte8)
    {
        return None;
    }

    Some((
        4,
        OptimizedInstruction::CompareSetTeeLocal {
            old_range: first.old_range.start..fourth.old_range.end,
            lhs_local_addr,
            rhs_local_addr,
            dst_local,
            tee,
            op,
        },
    ))
}

fn match_local_const_compare_set_tee(
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
    let (lhs_width, lhs_local_addr) = local_get(first.kind)?;
    let rhs_const = match second.kind {
        DecodedKind::Const(value) => value,
        _ => return None,
    };
    let op = match third.kind {
        DecodedKind::Compare(op) => op,
        _ => return None,
    };
    let (dst_width, dst_local, tee) = local_set_tee(fourth.kind)?;
    if !same_width(lhs_width, op.width())
        || !same_width(lhs_width, rhs_const.width())
        || !same_width(dst_width, ValueSize::Byte4)
        || !matches!(lhs_width, ValueSize::Byte4 | ValueSize::Byte8)
        || !compare_matches_const(op, rhs_const)
    {
        return None;
    }

    Some((
        4,
        OptimizedInstruction::CompareSetTeeConst {
            old_range: first.old_range.start..fourth.old_range.end,
            lhs_local_addr,
            rhs_const,
            dst_local,
            tee,
            op,
        },
    ))
}

fn match_local_local_compare_br_if(
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
    let (lhs_width, lhs_local_addr) = local_get(first.kind)?;
    let (rhs_width, rhs_local_addr) = local_get(second.kind)?;
    let op = match third.kind {
        DecodedKind::Compare(op) => op,
        _ => return None,
    };
    let target_old = match fourth.kind {
        DecodedKind::BrIf(target) => target,
        _ => return None,
    };
    if !same_width(lhs_width, rhs_width)
        || !same_width(lhs_width, op.width())
        || !matches!(lhs_width, ValueSize::Byte4 | ValueSize::Byte8)
    {
        return None;
    }

    Some((
        4,
        OptimizedInstruction::CompareBrIfLocal {
            old_range: first.old_range.start..fourth.old_range.end,
            lhs_local_addr,
            rhs_local_addr,
            target_old,
            op,
        },
    ))
}

fn match_local_const_compare_br_if(
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
    let (lhs_width, lhs_local_addr) = local_get(first.kind)?;
    let rhs_const = match second.kind {
        DecodedKind::Const(value) => value,
        _ => return None,
    };
    let op = match third.kind {
        DecodedKind::Compare(op) => op,
        _ => return None,
    };
    let target_old = match fourth.kind {
        DecodedKind::BrIf(target) => target,
        _ => return None,
    };
    if !same_width(lhs_width, op.width())
        || !same_width(lhs_width, rhs_const.width())
        || !matches!(lhs_width, ValueSize::Byte4 | ValueSize::Byte8)
        || !compare_matches_const(op, rhs_const)
    {
        return None;
    }

    Some((
        4,
        OptimizedInstruction::CompareBrIfConst {
            old_range: first.old_range.start..fourth.old_range.end,
            lhs_local_addr,
            rhs_const,
            target_old,
            op,
        },
    ))
}

fn match_local_local_compare_select(
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
    let (lhs_width, lhs_local_addr) = local_get(first.kind)?;
    let (rhs_width, rhs_local_addr) = local_get(second.kind)?;
    let op = match third.kind {
        DecodedKind::Compare(op) => op,
        _ => return None,
    };
    let select_width = select_width(fourth.kind)?;
    if !same_width(lhs_width, rhs_width)
        || !same_width(lhs_width, op.width())
        || !matches!(lhs_width, ValueSize::Byte4 | ValueSize::Byte8)
        || !matches!(select_width, ValueSize::Byte4 | ValueSize::Byte8)
    {
        return None;
    }

    Some((
        4,
        OptimizedInstruction::CompareSelectLocal {
            old_range: first.old_range.start..fourth.old_range.end,
            lhs_local_addr,
            rhs_local_addr,
            select_width,
            op,
        },
    ))
}

fn match_local_const_compare_select(
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
    let (lhs_width, lhs_local_addr) = local_get(first.kind)?;
    let rhs_const = match second.kind {
        DecodedKind::Const(value) => value,
        _ => return None,
    };
    let op = match third.kind {
        DecodedKind::Compare(op) => op,
        _ => return None,
    };
    let select_width = select_width(fourth.kind)?;
    if !same_width(lhs_width, op.width())
        || !same_width(lhs_width, rhs_const.width())
        || !matches!(lhs_width, ValueSize::Byte4 | ValueSize::Byte8)
        || !matches!(select_width, ValueSize::Byte4 | ValueSize::Byte8)
        || !compare_matches_const(op, rhs_const)
    {
        return None;
    }

    Some((
        4,
        OptimizedInstruction::CompareSelectConst {
            old_range: first.old_range.start..fourth.old_range.end,
            lhs_local_addr,
            rhs_const,
            select_width,
            op,
        },
    ))
}

fn match_const_load(
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
        DecodedKind::Const(TypedConst::I32(value)) => value,
        _ => return None,
    };
    let (op, memarg) = match second.kind {
        DecodedKind::Load(op, memarg) => (op, memarg),
        _ => return None,
    };
    let start = match compute_memory_offset(memarg, addr as u32) {
        VMResult::Success(start) => u32::try_from(start).ok()?,
        _ => return None,
    };

    Some((
        2,
        OptimizedInstruction::LoadConstLocal {
            old_range: first.old_range.start..second.old_range.end,
            start,
            op,
        },
    ))
}

fn match_const_local_store(
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
        DecodedKind::Const(TypedConst::I32(value)) => value,
        _ => return None,
    };
    let (value_width, value_local_addr) = local_get(second.kind)?;
    let (op, memarg) = match third.kind {
        DecodedKind::Store(op, memarg) => (op, memarg),
        _ => return None,
    };
    if !same_width(value_width, op.value_width()) {
        return None;
    }
    let start = match compute_memory_offset(memarg, addr as u32) {
        VMResult::Success(start) => u32::try_from(start).ok()?,
        _ => return None,
    };

    Some((
        3,
        OptimizedInstruction::StoreConstLocal {
            old_range: first.old_range.start..third.old_range.end,
            start,
            value_local_addr,
            op,
        },
    ))
}

fn match_local_addr_load(
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
    let (addr_width, local_addr) = local_get(first.kind)?;
    let (op, memarg) = match second.kind {
        DecodedKind::Load(op, memarg) => (op, memarg),
        _ => return None,
    };
    if !same_width(addr_width, ValueSize::Byte4) {
        return None;
    }

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

fn match_local_imm_addr_load(
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
    let (addr_width, local_addr) = local_get(first.kind)?;
    let imm = match second.kind {
        DecodedKind::Const(TypedConst::I32(value)) => value,
        _ => return None,
    };
    if !matches!(
        third.kind,
        DecodedKind::Scalar(TypedScalarOp::I32(vm::I32ScalarKind::Add))
    ) {
        return None;
    }
    let (op, memarg) = match fourth.kind {
        DecodedKind::Load(op, memarg) => (op, memarg),
        _ => return None,
    };
    if !same_width(addr_width, ValueSize::Byte4) || !op.uses_dedicated_local_addr() {
        return None;
    }

    Some((
        4,
        OptimizedInstruction::LocalImmAddrLoad {
            old_range: first.old_range.start..fourth.old_range.end,
            local_addr,
            imm,
            memarg,
            op,
        },
    ))
}

fn match_i32_local_local_load_tee_add_imm_store(
    decoded: &[DecodedInstruction],
    index: usize,
    jump_targets: &HashSet<usize>,
) -> Option<MatchOutcome> {
    let [first, second, third, fourth, fifth, sixth, seventh] = decoded.get(index..index + 7)?
    else {
        return None;
    };
    if sequence_crosses_jump_targets(decoded, jump_targets, index + 1..index + 7) {
        return None;
    }
    let (store_addr_width, store_addr_local_addr) = local_get(first.kind)?;
    let (load_addr_width, load_addr_local_addr) = local_get(second.kind)?;
    let (load_op, load_memarg) = match third.kind {
        DecodedKind::Load(op, memarg) => (op, memarg),
        _ => return None,
    };
    let (tee_width, tee_local_addr, tee) = local_set_tee(fourth.kind)?;
    let imm = match fifth.kind {
        DecodedKind::Const(TypedConst::I32(value)) => value,
        _ => return None,
    };
    let (store_op, store_memarg) = match seventh.kind {
        DecodedKind::Store(op, memarg) => (op, memarg),
        _ => return None,
    };
    if !same_width(store_addr_width, ValueSize::Byte4)
        || !same_width(load_addr_width, ValueSize::Byte4)
        || !matches!(load_op, TypedLoadOp::Bits4(vm::Load4Kind::I32))
        || !same_width(tee_width, ValueSize::Byte4)
        || !tee
        || !matches!(
            sixth.kind,
            DecodedKind::Scalar(TypedScalarOp::I32(vm::I32ScalarKind::Add))
        )
        || !matches!(store_op, TypedStoreOp::Bits4(vm::Store4Kind::I32))
    {
        return None;
    }

    Some((
        7,
        OptimizedInstruction::I32LocalLocalLoadTeeAddImmStore {
            old_range: first.old_range.start..seventh.old_range.end,
            store_addr_local_addr,
            load_addr_local_addr,
            tee_local_addr,
            imm,
            load_memarg,
            store_memarg,
        },
    ))
}

fn match_local_local_store(
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
    let (addr_width, addr_local_addr) = local_get(first.kind)?;
    let (value_width, value_local_addr) = local_get(second.kind)?;
    let (op, memarg) = match third.kind {
        DecodedKind::Store(op, memarg) => (op, memarg),
        _ => return None,
    };
    if !same_width(addr_width, ValueSize::Byte4) || !same_width(value_width, op.value_width()) {
        return None;
    }

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

fn match_i32_local_local_load8_u_store8_copy(
    decoded: &[DecodedInstruction],
    index: usize,
    jump_targets: &HashSet<usize>,
) -> Option<MatchOutcome> {
    match_i32_local_local_narrow_copy(decoded, index, jump_targets, NarrowCopyKind::Load8Store8)
}

fn match_i32_local_local_load16_u_store16_copy(
    decoded: &[DecodedInstruction],
    index: usize,
    jump_targets: &HashSet<usize>,
) -> Option<MatchOutcome> {
    match_i32_local_local_narrow_copy(decoded, index, jump_targets, NarrowCopyKind::Load16Store16)
}

fn match_i32_local_local_narrow_copy(
    decoded: &[DecodedInstruction],
    index: usize,
    jump_targets: &HashSet<usize>,
    kind: NarrowCopyKind,
) -> Option<MatchOutcome> {
    let [first, second, third, fourth] = decoded.get(index..index + 4)? else {
        return None;
    };
    if sequence_crosses_jump_targets(decoded, jump_targets, index + 1..index + 4) {
        return None;
    }
    let (dst_width, dst_local_addr) = local_get(first.kind)?;
    let (src_width, src_local_addr) = local_get(second.kind)?;
    let (load_op, load_memarg) = match third.kind {
        DecodedKind::Load(op, memarg) => (op, memarg),
        _ => return None,
    };
    let (store_op, store_memarg) = match fourth.kind {
        DecodedKind::Store(op, memarg) => (op, memarg),
        _ => return None,
    };

    let op_matches = matches!(
        (kind, load_op, store_op),
        (
            NarrowCopyKind::Load8Store8,
            TypedLoadOp::Bits4(vm::Load4Kind::I32Load8U),
            TypedStoreOp::Bits4(vm::Store4Kind::I32Store8),
        ) | (
            NarrowCopyKind::Load16Store16,
            TypedLoadOp::Bits4(vm::Load4Kind::I32Load16U),
            TypedStoreOp::Bits4(vm::Store4Kind::I32Store16),
        )
    );

    if !same_width(dst_width, ValueSize::Byte4)
        || !same_width(src_width, ValueSize::Byte4)
        || !op_matches
    {
        return None;
    }

    Some((
        4,
        OptimizedInstruction::I32LocalLocalNarrowCopy {
            old_range: first.old_range.start..fourth.old_range.end,
            dst_local_addr,
            src_local_addr,
            load_memarg,
            store_memarg,
            kind,
        },
    ))
}

fn match_local_imm_local_store(
    decoded: &[DecodedInstruction],
    index: usize,
    jump_targets: &HashSet<usize>,
) -> Option<MatchOutcome> {
    let [first, second, third, fourth, fifth] = decoded.get(index..index + 5)? else {
        return None;
    };
    if sequence_crosses_jump_targets(decoded, jump_targets, index + 1..index + 5) {
        return None;
    }
    let (addr_width, addr_local_addr) = local_get(first.kind)?;
    let imm = match second.kind {
        DecodedKind::Const(TypedConst::I32(value)) => value,
        _ => return None,
    };
    if !matches!(
        third.kind,
        DecodedKind::Scalar(TypedScalarOp::I32(vm::I32ScalarKind::Add))
    ) {
        return None;
    }
    let (value_width, value_local_addr) = local_get(fourth.kind)?;
    let (op, memarg) = match fifth.kind {
        DecodedKind::Store(op, memarg) => (op, memarg),
        _ => return None,
    };
    if !same_width(addr_width, ValueSize::Byte4)
        || !same_width(value_width, op.value_width())
        || !op.uses_dedicated_local_local()
    {
        return None;
    }

    Some((
        5,
        OptimizedInstruction::LocalImmLocalStore {
            old_range: first.old_range.start..fifth.old_range.end,
            addr_local_addr,
            imm,
            value_local_addr,
            memarg,
            op,
        },
    ))
}

struct LoweredProgram {
    instr: Vec<Instr>,
    instruction_starts: Vec<usize>,
    old_to_new: Vec<u32>,
}

fn lower_program(
    optimized: Vec<OptimizedInstruction>,
    old_flat_len: usize,
    function_index: u32,
) -> LoweredProgram {
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
    let mut instruction_starts = Vec::with_capacity(optimized.len());
    for (instruction_ordinal, instruction) in optimized.into_iter().enumerate() {
        instruction_starts.push(lowered.len());
        lower_instruction(
            instruction,
            &old_to_new,
            &mut lowered,
            function_index,
            instruction_ordinal as u32,
        );
    }
    LoweredProgram {
        instr: lowered,
        instruction_starts,
        old_to_new,
    }
}

fn op_eq(op: Op, expected: Op) -> bool {
    std::ptr::fn_addr_eq(op, expected)
}

fn single_jump_operand_slot(op: Op) -> Option<u8> {
    if op_eq(op, vm::op_br as Op)
        || op_eq(op, vm::op_br_if as Op)
        || op_eq(op, vm::op_br_if_r0 as Op)
        || op_eq(op, vm::op_br_if_r1 as Op)
        || op_eq(op, vm::op_br_if_r2 as Op)
        || op_eq(op, vm::op_br_if_r3 as Op)
        || op_eq(op, vm::op_if as Op)
        || op_eq(op, vm::op_else as Op)
    {
        return Some(1);
    }
    if op_eq(op, vm::op_i32_local_br_if as Op)
        || op_eq(op, vm::op_i32_local_eqz_br_if as Op)
        || op_eq(op, vm::op_i32_local_if as Op)
        || op_eq(op, vm::op_i32_local_eqz_if as Op)
        || op_eq(op, vm::op_i64_local_br_if as Op)
        || op_eq(op, vm::op_i64_local_eqz_br_if as Op)
        || op_eq(op, vm::op_i64_local_if as Op)
        || op_eq(op, vm::op_i64_local_eqz_if as Op)
    {
        return Some(2);
    }
    if op_eq(op, vm::op_i32_local_and_imm_br_if as Op)
        || op_eq(op, vm::op_i32_local_and_imm_eqz_br_if as Op)
        || op_eq(op, vm::op_i32_local_and_imm_if as Op)
        || op_eq(op, vm::op_i32_local_and_imm_eqz_if as Op)
        || op_eq(op, vm::op_i32_local_local_ge_u_br_if as Op)
        || op_eq(op, vm::op_i32_local_local_compare_br_if as Op)
        || op_eq(op, vm::op_i32_local_const_compare_br_if as Op)
        || op_eq(op, vm::op_i64_local_local_compare_br_if as Op)
        || op_eq(op, vm::op_i64_local_const_compare_br_if as Op)
        || op_eq(op, vm::op_f32_local_local_compare_br_if as Op)
        || op_eq(op, vm::op_f32_local_const_compare_br_if as Op)
        || op_eq(op, vm::op_f64_local_local_compare_br_if as Op)
        || op_eq(op, vm::op_f64_local_const_compare_br_if as Op)
    {
        return Some(3);
    }
    if op_eq(op, vm::op_i32_local_addr_load8_u_and_imm_eqz_br_if as Op)
        || op_eq(op, vm::op_i32_local_addr_load8_u_and_imm_eqz_if as Op)
    {
        return Some(4);
    }
    if op_eq(op, vm::op_i32_seed_tee_eqz_br_if as Op)
        || op_eq(op, vm::op_i32_seed_tee_eqz_if as Op)
        || op_eq(op, vm::op_i64_seed_tee_eqz_br_if as Op)
        || op_eq(op, vm::op_i64_seed_tee_eqz_if as Op)
    {
        return Some(7);
    }
    if op_eq(op, vm::op_i32_seed_tee_imm_compare_br_if as Op)
        || op_eq(op, vm::op_i32_seed_tee_imm_compare_if as Op)
        || op_eq(op, vm::op_i64_seed_tee_imm_compare_br_if as Op)
        || op_eq(op, vm::op_i64_seed_tee_imm_compare_if as Op)
    {
        return Some(8);
    }
    if op_eq(op, vm::op_i32_seed_imm_and_br_if as Op)
        || op_eq(op, vm::op_i32_seed_imm_and_eqz_br_if as Op)
        || op_eq(op, vm::op_i32_seed_imm_and_if as Op)
        || op_eq(op, vm::op_i32_seed_imm_and_eqz_if as Op)
        || op_eq(op, vm::op_i64_seed_imm_and_br_if as Op)
        || op_eq(op, vm::op_i64_seed_imm_and_eqz_br_if as Op)
        || op_eq(op, vm::op_i64_seed_imm_and_if as Op)
        || op_eq(op, vm::op_i64_seed_imm_and_eqz_if as Op)
    {
        return Some(7);
    }
    None
}

fn is_br_table(op: Op) -> bool {
    op_eq(op, vm::op_br_table as Op)
}

fn loop_shape_op(op: Op) -> Option<ReturnShape> {
    if op_eq(op, vm::op_loop_empty as Op) {
        Some(ReturnShape::Empty)
    } else if op_eq(op, vm::op_loop4 as Op) {
        Some(ReturnShape::Scalar4)
    } else if op_eq(op, vm::op_loop8 as Op) {
        Some(ReturnShape::Scalar8)
    } else if op_eq(op, vm::op_loop_generic as Op) || op_eq(op, vm::op_loop as Op) {
        Some(ReturnShape::Generic)
    } else {
        None
    }
}

fn block_return_shape_op(op: Op) -> Option<ReturnShape> {
    if op_eq(op, vm::special_block_return_empty as Op) {
        Some(ReturnShape::Empty)
    } else if op_eq(op, vm::special_block_return4 as Op) {
        Some(ReturnShape::Scalar4)
    } else if op_eq(op, vm::special_block_return8 as Op) {
        Some(ReturnShape::Scalar8)
    } else if op_eq(op, vm::special_block_return_generic as Op)
        || op_eq(op, vm::special_block_return as Op)
    {
        Some(ReturnShape::Generic)
    } else {
        None
    }
}

fn collect_control_flow_metadata(
    code: &[Instr],
    instruction_starts: &[usize],
    frame_stack_base: u32,
) -> Arc<[ControlFlowMetadataSite]> {
    let mut metadata = Vec::new();
    for &start in instruction_starts {
        let op = unsafe { code[start].op };
        if let Some(jump_slot) = single_jump_operand_slot(op) {
            let target = unsafe { code[start + jump_slot as usize].operand.jump_addr };
            metadata.push(ControlFlowMetadataSite {
                instruction_ordinal: start as u32,
                kind: ControlFlowMetadataKind::Jump {
                    jump_operand_slots: Arc::from([jump_slot]),
                    target_ordinals: Arc::from([target]),
                },
            });
            continue;
        }
        if is_br_table(op) {
            let table_size = unsafe { code[start + 1].operand.u32 as usize };
            let mut jump_slots = Vec::with_capacity(table_size + 1);
            let mut target_ordinals = Vec::with_capacity(table_size + 1);
            for slot in 0..=table_size {
                let jump_slot = u8::try_from(slot + 2).expect("br_table jump slot exceeds u8");
                jump_slots.push(jump_slot);
                target_ordinals
                    .push(unsafe { code[start + usize::from(jump_slot)].operand.jump_addr });
            }
            metadata.push(ControlFlowMetadataSite {
                instruction_ordinal: start as u32,
                kind: ControlFlowMetadataKind::Jump {
                    jump_operand_slots: Arc::from(jump_slots),
                    target_ordinals: Arc::from(target_ordinals),
                },
            });
            continue;
        }
        if let Some(shape) = loop_shape_op(op) {
            let loop_param = unsafe { code[start + 1].operand.loop_param };
            metadata.push(ControlFlowMetadataSite {
                instruction_ordinal: start as u32,
                kind: ControlFlowMetadataKind::Loop {
                    dst_from_local_top: frame_stack_base + loop_param.stack_top,
                    param_size: loop_param.param_size(),
                    shape,
                },
            });
            continue;
        }
        if let Some(shape) = block_return_shape_op(op) {
            let block_return = unsafe { code[start + 1].operand.block_return };
            metadata.push(ControlFlowMetadataSite {
                instruction_ordinal: start as u32,
                kind: ControlFlowMetadataKind::BlockReturn {
                    dst_from_local_top: frame_stack_base + block_return.stack_top,
                    return_size: block_return.return_size(),
                    shape,
                },
            });
        }
    }
    Arc::from(metadata)
}

fn map_raw_start_to_instruction_ordinal(
    raw_start: usize,
    old_to_new: &[u32],
    instruction_starts: &[usize],
) -> Option<u32> {
    let new_start = *old_to_new.get(raw_start)? as usize;
    let ordinal = instruction_starts.binary_search(&new_start).ok()?;
    Some(ordinal as u32)
}

fn collect_stack_map_metadata(
    source_sites: &[StackMapSourceSite],
    old_to_new: &[u32],
    instruction_starts: &[usize],
) -> Arc<[StackMapSite]> {
    let mut sites = Vec::with_capacity(source_sites.len());
    for site in source_sites {
        let Some(instruction_ordinal) =
            map_raw_start_to_instruction_ordinal(site.raw_start, old_to_new, instruction_starts)
        else {
            continue;
        };
        sites.push(StackMapSite {
            instruction_ordinal,
            kind: site.kind,
            operand_bytes: site.operand_bytes,
            ref_offsets_from_operand_base: site.ref_offsets_from_operand_base.clone(),
        });
    }
    Arc::from(sites)
}

fn collect_unwind_metadata(
    source_sites: &[UnwindSourceSite],
    old_to_new: &[u32],
    instruction_starts: &[usize],
) -> Arc<[UnwindSiteMetadata]> {
    let mut sites = Vec::with_capacity(source_sites.len());
    for site in source_sites {
        let Some(instruction_ordinal) =
            map_raw_start_to_instruction_ordinal(site.raw_start, old_to_new, instruction_starts)
        else {
            continue;
        };
        sites.push(UnwindSiteMetadata {
            instruction_ordinal,
            kind: site.kind,
            result_slot_from_local_top: site.result_slot_from_local_top,
        });
    }
    Arc::from(sites)
}

fn old_range(instruction: &OptimizedInstruction) -> &Range<usize> {
    match instruction {
        OptimizedInstruction::Raw(decoded) => &decoded.old_range,
        OptimizedInstruction::ConstSetTee { old_range, .. }
        | OptimizedInstruction::LocalCopy { old_range, .. }
        | OptimizedInstruction::LocalImmPush { old_range, .. }
        | OptimizedInstruction::LocalLocalPush { old_range, .. }
        | OptimizedInstruction::LocalImmSetTee { old_range, .. }
        | OptimizedInstruction::LocalLocalSetTee { old_range, .. }
        | OptimizedInstruction::LocalBranch { old_range, .. }
        | OptimizedInstruction::I32LocalAndImmBranch { old_range, .. }
        | OptimizedInstruction::ProducerImmAndBranch { old_range, .. }
        | OptimizedInstruction::I32LocalAddrLoad8UAndImmEqzBranch { old_range, .. }
        | OptimizedInstruction::I32LocalLocalGeUBrIf { old_range, .. }
        | OptimizedInstruction::CompareSetTeeLocal { old_range, .. }
        | OptimizedInstruction::CompareSetTeeConst { old_range, .. }
        | OptimizedInstruction::CompareBrIfLocal { old_range, .. }
        | OptimizedInstruction::CompareBrIfConst { old_range, .. }
        | OptimizedInstruction::CompareSelectLocal { old_range, .. }
        | OptimizedInstruction::CompareSelectConst { old_range, .. }
        | OptimizedInstruction::LoadConstLocal { old_range, .. }
        | OptimizedInstruction::StoreConstLocal { old_range, .. }
        | OptimizedInstruction::LocalAddrLoad { old_range, .. }
        | OptimizedInstruction::LocalImmAddrLoad { old_range, .. }
        | OptimizedInstruction::I32LocalLocalLoadTeeAddImmStore { old_range, .. }
        | OptimizedInstruction::LocalLocalStore { old_range, .. }
        | OptimizedInstruction::LocalImmLocalStore { old_range, .. }
        | OptimizedInstruction::I32LocalLocalNarrowCopy { old_range, .. }
        | OptimizedInstruction::ProducerTeeEqzBranch { old_range, .. }
        | OptimizedInstruction::ProducerTeeImmCompareBranch { old_range, .. }
        | OptimizedInstruction::ProducerTeeImmScalarSetTee { old_range, .. }
        | OptimizedInstruction::ProducerImmScalarSetTee { old_range, .. }
        | OptimizedInstruction::ProducerTeeConstSelfSelect { old_range, .. }
        | OptimizedInstruction::ProducerCompareSelectLocal { old_range, .. }
        | OptimizedInstruction::ProducerCompareSelectConst { old_range, .. }
        | OptimizedInstruction::CompareTeeSelectLocal { old_range, .. }
        | OptimizedInstruction::CompareTeeSelectConst { old_range, .. } => old_range,
    }
}

fn output_len(instruction: &OptimizedInstruction) -> usize {
    match instruction {
        OptimizedInstruction::Raw(decoded) => decoded.raw.len(),
        OptimizedInstruction::ConstSetTee { .. } => 3,
        OptimizedInstruction::LocalCopy { .. } => 3,
        OptimizedInstruction::LocalImmPush { .. } | OptimizedInstruction::LocalLocalPush { .. } => {
            4
        }
        OptimizedInstruction::LocalImmSetTee { op, .. } => {
            if is_existing_i32_local_imm_fastpath(*op) {
                4
            } else {
                5
            }
        }
        OptimizedInstruction::LocalLocalSetTee { op, .. } => {
            if is_existing_i32_local_local_fastpath(*op) {
                4
            } else {
                5
            }
        }
        OptimizedInstruction::LocalBranch { .. } => 3,
        OptimizedInstruction::I32LocalAndImmBranch { .. } => 4,
        OptimizedInstruction::ProducerImmAndBranch { .. } => 8,
        OptimizedInstruction::I32LocalAddrLoad8UAndImmEqzBranch { .. } => 5,
        OptimizedInstruction::I32LocalLocalGeUBrIf { .. } => 4,
        OptimizedInstruction::CompareSetTeeLocal { .. }
        | OptimizedInstruction::CompareSetTeeConst { .. }
        | OptimizedInstruction::CompareBrIfLocal { .. }
        | OptimizedInstruction::CompareBrIfConst { .. } => 5,
        OptimizedInstruction::CompareSelectLocal { .. }
        | OptimizedInstruction::CompareSelectConst { .. } => 4,
        OptimizedInstruction::LoadConstLocal { op, .. } => {
            if op.uses_dedicated_const() {
                2
            } else {
                3
            }
        }
        OptimizedInstruction::StoreConstLocal { op, .. } => {
            if op.uses_dedicated_const() {
                3
            } else {
                4
            }
        }
        OptimizedInstruction::LocalAddrLoad { op, .. } => {
            if op.uses_dedicated_local_addr() {
                3
            } else {
                4
            }
        }
        OptimizedInstruction::LocalImmAddrLoad { .. } => 4,
        OptimizedInstruction::I32LocalLocalLoadTeeAddImmStore { .. } => 7,
        OptimizedInstruction::LocalLocalStore { op, .. } => {
            if op.uses_dedicated_local_local() {
                4
            } else {
                5
            }
        }
        OptimizedInstruction::LocalImmLocalStore { .. } => 5,
        OptimizedInstruction::I32LocalLocalNarrowCopy { .. } => 5,
        OptimizedInstruction::ProducerTeeEqzBranch { .. } => 8,
        OptimizedInstruction::ProducerTeeImmCompareBranch { .. } => 10,
        OptimizedInstruction::ProducerTeeImmScalarSetTee { .. } => 10,
        OptimizedInstruction::ProducerImmScalarSetTee { .. } => 9,
        OptimizedInstruction::ProducerTeeConstSelfSelect { .. } => 8,
        OptimizedInstruction::ProducerCompareSelectLocal { .. }
        | OptimizedInstruction::ProducerCompareSelectConst { .. } => 8,
        OptimizedInstruction::CompareTeeSelectLocal { .. }
        | OptimizedInstruction::CompareTeeSelectConst { .. } => 5,
    }
}

fn scalar_kind_operand(op: TypedScalarOp) -> u32 {
    match op {
        TypedScalarOp::I32(kind) => kind as u32,
        TypedScalarOp::I64(kind) => kind as u32,
        TypedScalarOp::F32(kind) => kind as u32,
        TypedScalarOp::F64(kind) => kind as u32,
    }
}

fn compare_kind_operand(op: TypedCompareOp) -> u32 {
    match op {
        TypedCompareOp::I32(kind) => kind as u32,
        TypedCompareOp::I64(kind) => kind as u32,
        TypedCompareOp::F32(kind) => kind as u32,
        TypedCompareOp::F64(kind) => kind as u32,
    }
}

fn load_kind_operand(op: TypedLoadOp) -> u32 {
    match op {
        TypedLoadOp::Bits4(kind) => kind as u32,
        TypedLoadOp::Bits8(kind) => kind as u32,
    }
}

fn store_kind_operand(op: TypedStoreOp) -> u32 {
    match op {
        TypedStoreOp::Bits4(kind) => kind as u32,
        TypedStoreOp::Bits8(kind) => kind as u32,
    }
}

fn producer_seed_kind_operand(seed: ProducerSeed) -> u32 {
    match seed {
        ProducerSeed::Local { .. } => 0,
        ProducerSeed::LocalImmScalar { .. } => 1,
        ProducerSeed::LocalLocalScalar { .. } => 2,
        ProducerSeed::LocalAddrLoad { .. } => 3,
        ProducerSeed::LocalImmAddrLoad { .. } => 4,
        ProducerSeed::ConstAddrLoad { .. } => 5,
    }
}

fn zero_operand() -> Instr {
    Instr {
        operand: Operand { u64: 0 },
    }
}

fn push_const_operand(lowered: &mut Vec<Instr>, value: TypedConst) {
    lowered.push(match value {
        TypedConst::I32(value) => Instr {
            operand: Operand { i32: value },
        },
        TypedConst::I64(value) => Instr {
            operand: Operand { u64: value as u64 },
        },
        TypedConst::F32(bits) => Instr {
            operand: Operand {
                f32: f32::from_bits(bits),
            },
        },
        TypedConst::F64(bits) => Instr {
            operand: Operand {
                f64: f64::from_bits(bits),
            },
        },
    });
}

fn push_producer_seed_operands(lowered: &mut Vec<Instr>, seed: ProducerSeed) {
    lowered.push(Instr {
        operand: Operand {
            u32: producer_seed_kind_operand(seed),
        },
    });
    match seed {
        ProducerSeed::Local { local_addr, .. } => {
            lowered.push(Instr {
                operand: Operand { local_addr },
            });
            lowered.push(zero_operand());
            lowered.push(zero_operand());
            lowered.push(zero_operand());
        }
        ProducerSeed::LocalImmScalar {
            src_local, imm, op, ..
        } => {
            lowered.push(Instr {
                operand: Operand {
                    local_addr: src_local,
                },
            });
            push_const_operand(lowered, imm);
            lowered.push(Instr {
                operand: Operand {
                    u32: scalar_kind_operand(op),
                },
            });
            lowered.push(zero_operand());
        }
        ProducerSeed::LocalLocalScalar {
            lhs_local_addr,
            rhs_local_addr,
            op,
            ..
        } => {
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
                    u32: scalar_kind_operand(op),
                },
            });
            lowered.push(zero_operand());
        }
        ProducerSeed::LocalAddrLoad {
            local_addr,
            memarg,
            op,
            ..
        } => {
            lowered.push(Instr {
                operand: Operand { local_addr },
            });
            lowered.push(Instr {
                operand: Operand { memarg },
            });
            lowered.push(Instr {
                operand: Operand {
                    u32: load_kind_operand(op),
                },
            });
            lowered.push(zero_operand());
        }
        ProducerSeed::LocalImmAddrLoad {
            local_addr,
            imm,
            memarg,
            op,
            ..
        } => {
            lowered.push(Instr {
                operand: Operand { local_addr },
            });
            lowered.push(Instr {
                operand: Operand { i32: imm },
            });
            lowered.push(Instr {
                operand: Operand { memarg },
            });
            lowered.push(Instr {
                operand: Operand {
                    u32: load_kind_operand(op),
                },
            });
        }
        ProducerSeed::ConstAddrLoad { start, op, .. } => {
            lowered.push(Instr {
                operand: Operand { u32: start },
            });
            lowered.push(Instr {
                operand: Operand {
                    u32: load_kind_operand(op),
                },
            });
            lowered.push(zero_operand());
            lowered.push(zero_operand());
        }
    }
}

fn lower_instruction(
    instruction: OptimizedInstruction,
    old_to_new: &[u32],
    lowered: &mut Vec<Instr>,
    function_index: u32,
    instruction_ordinal: u32,
) {
    let start = lowered.len();
    match instruction {
        OptimizedInstruction::Raw(decoded) => {
            lowered.extend(rewrite_raw_jumps(decoded.raw.as_ref(), old_to_new))
        }
        OptimizedInstruction::ConstSetTee {
            value,
            dst_local,
            tee,
            ..
        } => {
            let op = match (value, tee) {
                (TypedConst::I32(_), false) => vm::op_i32_const_set4,
                (TypedConst::I32(_), true) => vm::op_i32_const_tee4,
                (TypedConst::I64(_), false) => vm::op_i64_const_set8,
                (TypedConst::I64(_), true) => vm::op_i64_const_tee8,
                (TypedConst::F32(_), false) => vm::op_f32_const_set4,
                (TypedConst::F32(_), true) => vm::op_f32_const_tee4,
                (TypedConst::F64(_), false) => vm::op_f64_const_set8,
                (TypedConst::F64(_), true) => vm::op_f64_const_tee8,
            };
            lowered.push(Instr { op });
            push_const_operand(lowered, value);
            lowered.push(Instr {
                operand: Operand {
                    local_addr: dst_local,
                },
            });
        }
        OptimizedInstruction::LocalCopy {
            src_local,
            dst_local,
            width,
            tee,
            ..
        } => {
            let op = match (width, tee) {
                (ValueSize::Byte4, false) => vm::op_local_copy4,
                (ValueSize::Byte4, true) => vm::op_local_copy_tee4,
                (ValueSize::Byte8, false) => vm::op_local_copy8,
                (ValueSize::Byte8, true) => vm::op_local_copy_tee8,
                _ => unreachable!("unsupported local copy width"),
            };
            lowered.push(Instr { op });
            lowered.push(Instr {
                operand: Operand {
                    local_addr: src_local,
                },
            });
            lowered.push(Instr {
                operand: Operand {
                    local_addr: dst_local,
                },
            });
        }
        OptimizedInstruction::LocalImmPush {
            src_local, imm, op, ..
        } => {
            lowered.push(Instr {
                op: vm::op_i32_local_scalar_imm_push4,
            });
            lowered.push(Instr {
                operand: Operand {
                    local_addr: src_local,
                },
            });
            push_const_operand(lowered, imm);
            lowered.push(Instr {
                operand: Operand {
                    u32: scalar_kind_operand(op),
                },
            });
        }
        OptimizedInstruction::LocalLocalPush {
            lhs_local_addr,
            rhs_local_addr,
            op,
            ..
        } => {
            lowered.push(Instr {
                op: vm::op_i32_local_local_scalar_push4,
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
                    u32: scalar_kind_operand(op),
                },
            });
        }
        OptimizedInstruction::LocalImmSetTee {
            src_local,
            imm,
            dst_local,
            tee,
            op,
            ..
        } => match op {
            TypedScalarOp::I32(kind)
                if matches!(
                    kind,
                    vm::I32ScalarKind::Add
                        | vm::I32ScalarKind::Sub
                        | vm::I32ScalarKind::And
                        | vm::I32ScalarKind::Shl
                        | vm::I32ScalarKind::ShrU
                ) =>
            {
                let op = match (kind, tee) {
                    (vm::I32ScalarKind::Add, false) => vm::op_i32_local_add_imm_set4,
                    (vm::I32ScalarKind::Add, true) => vm::op_i32_local_add_imm_tee4,
                    (vm::I32ScalarKind::Sub, false) => vm::op_i32_local_sub_imm_set4,
                    (vm::I32ScalarKind::Sub, true) => vm::op_i32_local_sub_imm_tee4,
                    (vm::I32ScalarKind::And, false) => vm::op_i32_local_and_imm_set4,
                    (vm::I32ScalarKind::And, true) => vm::op_i32_local_and_imm_tee4,
                    (vm::I32ScalarKind::Shl, false) => vm::op_i32_local_shl_imm_set4,
                    (vm::I32ScalarKind::Shl, true) => vm::op_i32_local_shl_imm_tee4,
                    (vm::I32ScalarKind::ShrU, false) => vm::op_i32_local_shr_u_imm_set4,
                    (vm::I32ScalarKind::ShrU, true) => vm::op_i32_local_shr_u_imm_tee4,
                    _ => unreachable!(),
                };
                lowered.push(Instr { op });
                lowered.push(Instr {
                    operand: Operand {
                        local_addr: src_local,
                    },
                });
                let TypedConst::I32(imm) = imm else {
                    unreachable!()
                };
                lowered.push(Instr {
                    operand: Operand { i32: imm },
                });
                lowered.push(Instr {
                    operand: Operand {
                        local_addr: dst_local,
                    },
                });
            }
            TypedScalarOp::I32(_) => {
                lowered.push(Instr {
                    op: if tee {
                        vm::op_i32_local_scalar_imm_tee4
                    } else {
                        vm::op_i32_local_scalar_imm_set4
                    },
                });
                lowered.push(Instr {
                    operand: Operand {
                        local_addr: src_local,
                    },
                });
                push_const_operand(lowered, imm);
                lowered.push(Instr {
                    operand: Operand {
                        local_addr: dst_local,
                    },
                });
                lowered.push(Instr {
                    operand: Operand {
                        u32: scalar_kind_operand(op),
                    },
                });
            }
            TypedScalarOp::I64(_) => {
                lowered.push(Instr {
                    op: if tee {
                        vm::op_i64_local_scalar_imm_tee8
                    } else {
                        vm::op_i64_local_scalar_imm_set8
                    },
                });
                lowered.push(Instr {
                    operand: Operand {
                        local_addr: src_local,
                    },
                });
                push_const_operand(lowered, imm);
                lowered.push(Instr {
                    operand: Operand {
                        local_addr: dst_local,
                    },
                });
                lowered.push(Instr {
                    operand: Operand {
                        u32: scalar_kind_operand(op),
                    },
                });
            }
            TypedScalarOp::F32(_) => {
                lowered.push(Instr {
                    op: if tee {
                        vm::op_f32_local_scalar_imm_tee4
                    } else {
                        vm::op_f32_local_scalar_imm_set4
                    },
                });
                lowered.push(Instr {
                    operand: Operand {
                        local_addr: src_local,
                    },
                });
                push_const_operand(lowered, imm);
                lowered.push(Instr {
                    operand: Operand {
                        local_addr: dst_local,
                    },
                });
                lowered.push(Instr {
                    operand: Operand {
                        u32: scalar_kind_operand(op),
                    },
                });
            }
            TypedScalarOp::F64(_) => {
                lowered.push(Instr {
                    op: if tee {
                        vm::op_f64_local_scalar_imm_tee8
                    } else {
                        vm::op_f64_local_scalar_imm_set8
                    },
                });
                lowered.push(Instr {
                    operand: Operand {
                        local_addr: src_local,
                    },
                });
                push_const_operand(lowered, imm);
                lowered.push(Instr {
                    operand: Operand {
                        local_addr: dst_local,
                    },
                });
                lowered.push(Instr {
                    operand: Operand {
                        u32: scalar_kind_operand(op),
                    },
                });
            }
        },
        OptimizedInstruction::LocalLocalSetTee {
            lhs_local_addr,
            rhs_local_addr,
            dst_local,
            tee,
            op,
            ..
        } => match op {
            TypedScalarOp::I32(vm::I32ScalarKind::Add) => {
                lowered.push(Instr {
                    op: if tee {
                        vm::op_i32_local_local_add_tee4
                    } else {
                        vm::op_i32_local_local_add_set4
                    },
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
                        local_addr: dst_local,
                    },
                });
            }
            TypedScalarOp::I32(_) => {
                lowered.push(Instr {
                    op: if tee {
                        vm::op_i32_local_local_scalar_tee4
                    } else {
                        vm::op_i32_local_local_scalar_set4
                    },
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
                        local_addr: dst_local,
                    },
                });
                lowered.push(Instr {
                    operand: Operand {
                        u32: scalar_kind_operand(op),
                    },
                });
            }
            TypedScalarOp::I64(_) => {
                lowered.push(Instr {
                    op: if tee {
                        vm::op_i64_local_local_scalar_tee8
                    } else {
                        vm::op_i64_local_local_scalar_set8
                    },
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
                        local_addr: dst_local,
                    },
                });
                lowered.push(Instr {
                    operand: Operand {
                        u32: scalar_kind_operand(op),
                    },
                });
            }
            TypedScalarOp::F32(_) => {
                lowered.push(Instr {
                    op: if tee {
                        vm::op_f32_local_local_scalar_tee4
                    } else {
                        vm::op_f32_local_local_scalar_set4
                    },
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
                        local_addr: dst_local,
                    },
                });
                lowered.push(Instr {
                    operand: Operand {
                        u32: scalar_kind_operand(op),
                    },
                });
            }
            TypedScalarOp::F64(_) => {
                lowered.push(Instr {
                    op: if tee {
                        vm::op_f64_local_local_scalar_tee8
                    } else {
                        vm::op_f64_local_local_scalar_set8
                    },
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
                        local_addr: dst_local,
                    },
                });
                lowered.push(Instr {
                    operand: Operand {
                        u32: scalar_kind_operand(op),
                    },
                });
            }
        },
        OptimizedInstruction::LocalBranch {
            local_addr,
            target_old,
            width,
            zero_test,
            branch_kind,
            ..
        } => {
            let op = match (width, zero_test, branch_kind) {
                (ValueSize::Byte4, false, ControlBranchKind::BrIf) => vm::op_i32_local_br_if,
                (ValueSize::Byte4, true, ControlBranchKind::BrIf) => vm::op_i32_local_eqz_br_if,
                (ValueSize::Byte8, false, ControlBranchKind::BrIf) => vm::op_i64_local_br_if,
                (ValueSize::Byte8, true, ControlBranchKind::BrIf) => vm::op_i64_local_eqz_br_if,
                (ValueSize::Byte4, false, ControlBranchKind::If) => vm::op_i32_local_if,
                (ValueSize::Byte4, true, ControlBranchKind::If) => vm::op_i32_local_eqz_if,
                (ValueSize::Byte8, false, ControlBranchKind::If) => vm::op_i64_local_if,
                (ValueSize::Byte8, true, ControlBranchKind::If) => vm::op_i64_local_eqz_if,
                (ValueSize::Byte16, _, _) => unreachable!(),
            };
            lowered.push(Instr { op });
            lowered.push(Instr {
                operand: Operand { local_addr },
            });
            lowered.push(Instr {
                operand: Operand {
                    jump_addr: remap_jump_target(target_old, old_to_new),
                },
            });
        }
        OptimizedInstruction::I32LocalAndImmBranch {
            local_addr,
            imm,
            target_old,
            zero_test,
            branch_kind,
            ..
        } => {
            let op = match (zero_test, branch_kind) {
                (false, ControlBranchKind::BrIf) => vm::op_i32_local_and_imm_br_if,
                (true, ControlBranchKind::BrIf) => vm::op_i32_local_and_imm_eqz_br_if,
                (false, ControlBranchKind::If) => vm::op_i32_local_and_imm_if,
                (true, ControlBranchKind::If) => vm::op_i32_local_and_imm_eqz_if,
            };
            lowered.push(Instr { op });
            lowered.push(Instr {
                operand: Operand { local_addr },
            });
            lowered.push(Instr {
                operand: Operand { i32: imm },
            });
            lowered.push(Instr {
                operand: Operand {
                    jump_addr: remap_jump_target(target_old, old_to_new),
                },
            });
        }
        OptimizedInstruction::ProducerImmAndBranch {
            seed,
            rhs_const,
            width,
            target_old,
            zero_test,
            branch_kind,
            ..
        } => {
            let op = match (width, zero_test, branch_kind) {
                (ValueSize::Byte4, false, ControlBranchKind::BrIf) => vm::op_i32_seed_imm_and_br_if,
                (ValueSize::Byte4, true, ControlBranchKind::BrIf) => {
                    vm::op_i32_seed_imm_and_eqz_br_if
                }
                (ValueSize::Byte4, false, ControlBranchKind::If) => vm::op_i32_seed_imm_and_if,
                (ValueSize::Byte4, true, ControlBranchKind::If) => vm::op_i32_seed_imm_and_eqz_if,
                (ValueSize::Byte8, false, ControlBranchKind::BrIf) => vm::op_i64_seed_imm_and_br_if,
                (ValueSize::Byte8, true, ControlBranchKind::BrIf) => {
                    vm::op_i64_seed_imm_and_eqz_br_if
                }
                (ValueSize::Byte8, false, ControlBranchKind::If) => vm::op_i64_seed_imm_and_if,
                (ValueSize::Byte8, true, ControlBranchKind::If) => vm::op_i64_seed_imm_and_eqz_if,
                _ => unreachable!("producer imm-and branch only supports 4/8 byte values"),
            };
            lowered.push(Instr { op });
            push_producer_seed_operands(lowered, seed);
            push_const_operand(lowered, rhs_const);
            lowered.push(Instr {
                operand: Operand {
                    jump_addr: remap_jump_target(target_old, old_to_new),
                },
            });
        }
        OptimizedInstruction::I32LocalAddrLoad8UAndImmEqzBranch {
            local_addr,
            memarg,
            imm,
            target_old,
            branch_kind,
            ..
        } => {
            lowered.push(Instr {
                op: match branch_kind {
                    ControlBranchKind::BrIf => vm::op_i32_local_addr_load8_u_and_imm_eqz_br_if,
                    ControlBranchKind::If => vm::op_i32_local_addr_load8_u_and_imm_eqz_if,
                },
            });
            lowered.push(Instr {
                operand: Operand { local_addr },
            });
            lowered.push(Instr {
                operand: Operand { memarg },
            });
            lowered.push(Instr {
                operand: Operand { i32: imm },
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
        OptimizedInstruction::CompareSetTeeLocal {
            lhs_local_addr,
            rhs_local_addr,
            dst_local,
            tee,
            op: compare_op,
            ..
        } => {
            let handler = match (compare_op, tee) {
                (TypedCompareOp::I32(_), false) => vm::op_i32_local_local_compare_set4,
                (TypedCompareOp::I32(_), true) => vm::op_i32_local_local_compare_tee4,
                (TypedCompareOp::I64(_), false) => vm::op_i64_local_local_compare_set4,
                (TypedCompareOp::I64(_), true) => vm::op_i64_local_local_compare_tee4,
                (TypedCompareOp::F32(_), false) => vm::op_f32_local_local_compare_set4,
                (TypedCompareOp::F32(_), true) => vm::op_f32_local_local_compare_tee4,
                (TypedCompareOp::F64(_), false) => vm::op_f64_local_local_compare_set4,
                (TypedCompareOp::F64(_), true) => vm::op_f64_local_local_compare_tee4,
            };
            lowered.push(Instr { op: handler });
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
            lowered.push(Instr {
                operand: Operand {
                    u32: compare_kind_operand(compare_op),
                },
            });
        }
        OptimizedInstruction::CompareSetTeeConst {
            lhs_local_addr,
            rhs_const,
            dst_local,
            tee,
            op: compare_op,
            ..
        } => {
            let handler = match (compare_op, tee) {
                (TypedCompareOp::I32(_), false) => vm::op_i32_local_const_compare_set4,
                (TypedCompareOp::I32(_), true) => vm::op_i32_local_const_compare_tee4,
                (TypedCompareOp::I64(_), false) => vm::op_i64_local_const_compare_set4,
                (TypedCompareOp::I64(_), true) => vm::op_i64_local_const_compare_tee4,
                (TypedCompareOp::F32(_), false) => vm::op_f32_local_const_compare_set4,
                (TypedCompareOp::F32(_), true) => vm::op_f32_local_const_compare_tee4,
                (TypedCompareOp::F64(_), false) => vm::op_f64_local_const_compare_set4,
                (TypedCompareOp::F64(_), true) => vm::op_f64_local_const_compare_tee4,
            };
            lowered.push(Instr { op: handler });
            lowered.push(Instr {
                operand: Operand {
                    local_addr: lhs_local_addr,
                },
            });
            push_const_operand(lowered, rhs_const);
            lowered.push(Instr {
                operand: Operand {
                    local_addr: dst_local,
                },
            });
            lowered.push(Instr {
                operand: Operand {
                    u32: compare_kind_operand(compare_op),
                },
            });
        }
        OptimizedInstruction::CompareBrIfLocal {
            lhs_local_addr,
            rhs_local_addr,
            target_old,
            op: compare_op,
            ..
        } => {
            let handler = match compare_op {
                TypedCompareOp::I32(_) => vm::op_i32_local_local_compare_br_if,
                TypedCompareOp::I64(_) => vm::op_i64_local_local_compare_br_if,
                TypedCompareOp::F32(_) => vm::op_f32_local_local_compare_br_if,
                TypedCompareOp::F64(_) => vm::op_f64_local_local_compare_br_if,
            };
            lowered.push(Instr { op: handler });
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
            lowered.push(Instr {
                operand: Operand {
                    u32: compare_kind_operand(compare_op),
                },
            });
        }
        OptimizedInstruction::CompareBrIfConst {
            lhs_local_addr,
            rhs_const,
            target_old,
            op: compare_op,
            ..
        } => {
            let handler = match compare_op {
                TypedCompareOp::I32(_) => vm::op_i32_local_const_compare_br_if,
                TypedCompareOp::I64(_) => vm::op_i64_local_const_compare_br_if,
                TypedCompareOp::F32(_) => vm::op_f32_local_const_compare_br_if,
                TypedCompareOp::F64(_) => vm::op_f64_local_const_compare_br_if,
            };
            lowered.push(Instr { op: handler });
            lowered.push(Instr {
                operand: Operand {
                    local_addr: lhs_local_addr,
                },
            });
            push_const_operand(lowered, rhs_const);
            lowered.push(Instr {
                operand: Operand {
                    jump_addr: remap_jump_target(target_old, old_to_new),
                },
            });
            lowered.push(Instr {
                operand: Operand {
                    u32: compare_kind_operand(compare_op),
                },
            });
        }
        OptimizedInstruction::CompareSelectLocal {
            lhs_local_addr,
            rhs_local_addr,
            select_width,
            op: compare_op,
            ..
        } => {
            let handler = match (compare_op, select_width) {
                (TypedCompareOp::I32(_), ValueSize::Byte4) => {
                    vm::op_i32_local_local_compare_select4
                }
                (TypedCompareOp::I32(_), ValueSize::Byte8) => {
                    vm::op_i32_local_local_compare_select8
                }
                (TypedCompareOp::I64(_), ValueSize::Byte4) => {
                    vm::op_i64_local_local_compare_select4
                }
                (TypedCompareOp::I64(_), ValueSize::Byte8) => {
                    vm::op_i64_local_local_compare_select8
                }
                (TypedCompareOp::F32(_), ValueSize::Byte4) => {
                    vm::op_f32_local_local_compare_select4
                }
                (TypedCompareOp::F32(_), ValueSize::Byte8) => {
                    vm::op_f32_local_local_compare_select8
                }
                (TypedCompareOp::F64(_), ValueSize::Byte4) => {
                    vm::op_f64_local_local_compare_select4
                }
                (TypedCompareOp::F64(_), ValueSize::Byte8) => {
                    vm::op_f64_local_local_compare_select8
                }
                (_, ValueSize::Byte16) => unreachable!("select fast path only supports 4/8 byte"),
            };
            lowered.push(Instr { op: handler });
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
                    u32: compare_kind_operand(compare_op),
                },
            });
        }
        OptimizedInstruction::CompareSelectConst {
            lhs_local_addr,
            rhs_const,
            select_width,
            op: compare_op,
            ..
        } => {
            let handler = match (compare_op, select_width) {
                (TypedCompareOp::I32(_), ValueSize::Byte4) => {
                    vm::op_i32_local_const_compare_select4
                }
                (TypedCompareOp::I32(_), ValueSize::Byte8) => {
                    vm::op_i32_local_const_compare_select8
                }
                (TypedCompareOp::I64(_), ValueSize::Byte4) => {
                    vm::op_i64_local_const_compare_select4
                }
                (TypedCompareOp::I64(_), ValueSize::Byte8) => {
                    vm::op_i64_local_const_compare_select8
                }
                (TypedCompareOp::F32(_), ValueSize::Byte4) => {
                    vm::op_f32_local_const_compare_select4
                }
                (TypedCompareOp::F32(_), ValueSize::Byte8) => {
                    vm::op_f32_local_const_compare_select8
                }
                (TypedCompareOp::F64(_), ValueSize::Byte4) => {
                    vm::op_f64_local_const_compare_select4
                }
                (TypedCompareOp::F64(_), ValueSize::Byte8) => {
                    vm::op_f64_local_const_compare_select8
                }
                (_, ValueSize::Byte16) => unreachable!("select fast path only supports 4/8 byte"),
            };
            lowered.push(Instr { op: handler });
            lowered.push(Instr {
                operand: Operand {
                    local_addr: lhs_local_addr,
                },
            });
            push_const_operand(lowered, rhs_const);
            lowered.push(Instr {
                operand: Operand {
                    u32: compare_kind_operand(compare_op),
                },
            });
        }
        OptimizedInstruction::ProducerTeeEqzBranch {
            seed,
            tee_local_addr,
            target_old,
            width,
            branch_kind,
            ..
        } => {
            let op = match (width, branch_kind) {
                (ValueSize::Byte4, ControlBranchKind::BrIf) => vm::op_i32_seed_tee_eqz_br_if,
                (ValueSize::Byte4, ControlBranchKind::If) => vm::op_i32_seed_tee_eqz_if,
                (ValueSize::Byte8, ControlBranchKind::BrIf) => vm::op_i64_seed_tee_eqz_br_if,
                (ValueSize::Byte8, ControlBranchKind::If) => vm::op_i64_seed_tee_eqz_if,
                _ => unreachable!("tee eqz branch only supports 4/8 byte producers"),
            };
            lowered.push(Instr { op });
            push_producer_seed_operands(lowered, seed);
            lowered.push(Instr {
                operand: Operand {
                    local_addr: tee_local_addr,
                },
            });
            lowered.push(Instr {
                operand: Operand {
                    jump_addr: remap_jump_target(target_old, old_to_new),
                },
            });
        }
        OptimizedInstruction::ProducerTeeImmCompareBranch {
            seed,
            tee_local_addr,
            rhs_const,
            target_old,
            op: compare_op,
            branch_kind,
            ..
        } => {
            let handler = match (compare_op, branch_kind) {
                (TypedCompareOp::I32(_), ControlBranchKind::BrIf) => {
                    vm::op_i32_seed_tee_imm_compare_br_if
                }
                (TypedCompareOp::I32(_), ControlBranchKind::If) => {
                    vm::op_i32_seed_tee_imm_compare_if
                }
                (TypedCompareOp::I64(_), ControlBranchKind::BrIf) => {
                    vm::op_i64_seed_tee_imm_compare_br_if
                }
                (TypedCompareOp::I64(_), ControlBranchKind::If) => {
                    vm::op_i64_seed_tee_imm_compare_if
                }
                _ => unreachable!("tee compare branch only supports integer compares"),
            };
            lowered.push(Instr { op: handler });
            push_producer_seed_operands(lowered, seed);
            lowered.push(Instr {
                operand: Operand {
                    local_addr: tee_local_addr,
                },
            });
            push_const_operand(lowered, rhs_const);
            lowered.push(Instr {
                operand: Operand {
                    jump_addr: remap_jump_target(target_old, old_to_new),
                },
            });
            lowered.push(Instr {
                operand: Operand {
                    u32: compare_kind_operand(compare_op),
                },
            });
        }
        OptimizedInstruction::ProducerTeeImmScalarSetTee {
            seed,
            tee_local_addr,
            rhs_const,
            dst_local,
            dst_tee,
            op: scalar_op,
            ..
        } => {
            let handler = match (scalar_op, dst_tee) {
                (TypedScalarOp::I32(_), false) => vm::op_i32_seed_tee_imm_scalar_set4,
                (TypedScalarOp::I32(_), true) => vm::op_i32_seed_tee_imm_scalar_tee4,
                (TypedScalarOp::I64(_), false) => vm::op_i64_seed_tee_imm_scalar_set8,
                (TypedScalarOp::I64(_), true) => vm::op_i64_seed_tee_imm_scalar_tee8,
                _ => unreachable!("tee consumer scalar family only supports integer 4/8 byte"),
            };
            lowered.push(Instr { op: handler });
            push_producer_seed_operands(lowered, seed);
            lowered.push(Instr {
                operand: Operand {
                    local_addr: tee_local_addr,
                },
            });
            push_const_operand(lowered, rhs_const);
            lowered.push(Instr {
                operand: Operand {
                    local_addr: dst_local,
                },
            });
            lowered.push(Instr {
                operand: Operand {
                    u32: scalar_kind_operand(scalar_op),
                },
            });
        }
        OptimizedInstruction::ProducerImmScalarSetTee {
            seed,
            rhs_const,
            dst_local,
            dst_tee,
            op: scalar_op,
            ..
        } => {
            let handler = match (scalar_op, dst_tee) {
                (TypedScalarOp::I32(_), false) => vm::op_i32_seed_imm_scalar_set4,
                (TypedScalarOp::I32(_), true) => vm::op_i32_seed_imm_scalar_tee4,
                (TypedScalarOp::I64(_), false) => vm::op_i64_seed_imm_scalar_set8,
                (TypedScalarOp::I64(_), true) => vm::op_i64_seed_imm_scalar_tee8,
                _ => unreachable!("producer scalar family only supports integer 4/8 byte"),
            };
            lowered.push(Instr { op: handler });
            push_producer_seed_operands(lowered, seed);
            push_const_operand(lowered, rhs_const);
            lowered.push(Instr {
                operand: Operand {
                    local_addr: dst_local,
                },
            });
            lowered.push(Instr {
                operand: Operand {
                    u32: scalar_kind_operand(scalar_op),
                },
            });
        }
        OptimizedInstruction::ProducerTeeConstSelfSelect {
            seed,
            tee_local_addr,
            rhs_const,
            width,
            ..
        } => {
            lowered.push(Instr {
                op: match width {
                    ValueSize::Byte4 => vm::op_i32_seed_tee_const_self_select4,
                    ValueSize::Byte8 => vm::op_i64_seed_tee_const_self_select8,
                    _ => unreachable!("tee const self select only supports 4/8 byte values"),
                },
            });
            push_producer_seed_operands(lowered, seed);
            lowered.push(Instr {
                operand: Operand {
                    local_addr: tee_local_addr,
                },
            });
            push_const_operand(lowered, rhs_const);
        }
        OptimizedInstruction::ProducerCompareSelectLocal {
            seed,
            rhs_local_addr,
            select_width,
            op: compare_op,
            ..
        } => {
            let handler = match (compare_op, select_width) {
                (TypedCompareOp::I32(_), ValueSize::Byte4) => vm::op_i32_seed_local_compare_select4,
                (TypedCompareOp::I32(_), ValueSize::Byte8) => vm::op_i32_seed_local_compare_select8,
                (TypedCompareOp::I64(_), ValueSize::Byte4) => vm::op_i64_seed_local_compare_select4,
                (TypedCompareOp::I64(_), ValueSize::Byte8) => vm::op_i64_seed_local_compare_select8,
                (TypedCompareOp::F32(_), ValueSize::Byte4) => vm::op_f32_seed_local_compare_select4,
                (TypedCompareOp::F32(_), ValueSize::Byte8) => vm::op_f32_seed_local_compare_select8,
                (TypedCompareOp::F64(_), ValueSize::Byte4) => vm::op_f64_seed_local_compare_select4,
                (TypedCompareOp::F64(_), ValueSize::Byte8) => vm::op_f64_seed_local_compare_select8,
                (_, ValueSize::Byte16) => unreachable!("select fast path only supports 4/8 byte"),
            };
            lowered.push(Instr { op: handler });
            push_producer_seed_operands(lowered, seed);
            lowered.push(Instr {
                operand: Operand {
                    local_addr: rhs_local_addr,
                },
            });
            lowered.push(Instr {
                operand: Operand {
                    u32: compare_kind_operand(compare_op),
                },
            });
        }
        OptimizedInstruction::ProducerCompareSelectConst {
            seed,
            rhs_const,
            select_width,
            op: compare_op,
            ..
        } => {
            let handler = match (compare_op, select_width) {
                (TypedCompareOp::I32(_), ValueSize::Byte4) => vm::op_i32_seed_const_compare_select4,
                (TypedCompareOp::I32(_), ValueSize::Byte8) => vm::op_i32_seed_const_compare_select8,
                (TypedCompareOp::I64(_), ValueSize::Byte4) => vm::op_i64_seed_const_compare_select4,
                (TypedCompareOp::I64(_), ValueSize::Byte8) => vm::op_i64_seed_const_compare_select8,
                (TypedCompareOp::F32(_), ValueSize::Byte4) => vm::op_f32_seed_const_compare_select4,
                (TypedCompareOp::F32(_), ValueSize::Byte8) => vm::op_f32_seed_const_compare_select8,
                (TypedCompareOp::F64(_), ValueSize::Byte4) => vm::op_f64_seed_const_compare_select4,
                (TypedCompareOp::F64(_), ValueSize::Byte8) => vm::op_f64_seed_const_compare_select8,
                (_, ValueSize::Byte16) => unreachable!("select fast path only supports 4/8 byte"),
            };
            lowered.push(Instr { op: handler });
            push_producer_seed_operands(lowered, seed);
            push_const_operand(lowered, rhs_const);
            lowered.push(Instr {
                operand: Operand {
                    u32: compare_kind_operand(compare_op),
                },
            });
        }
        OptimizedInstruction::CompareTeeSelectLocal {
            lhs_local_addr,
            rhs_local_addr,
            tee_local_addr,
            select_width,
            op: compare_op,
            ..
        } => {
            let handler = match (compare_op, select_width) {
                (TypedCompareOp::I32(_), ValueSize::Byte4) => {
                    vm::op_i32_local_local_compare_tee_select4
                }
                (TypedCompareOp::I32(_), ValueSize::Byte8) => {
                    vm::op_i32_local_local_compare_tee_select8
                }
                (TypedCompareOp::I64(_), ValueSize::Byte4) => {
                    vm::op_i64_local_local_compare_tee_select4
                }
                (TypedCompareOp::I64(_), ValueSize::Byte8) => {
                    vm::op_i64_local_local_compare_tee_select8
                }
                _ => unreachable!("compare tee select only supports integer compares"),
            };
            lowered.push(Instr { op: handler });
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
                    local_addr: tee_local_addr,
                },
            });
            lowered.push(Instr {
                operand: Operand {
                    u32: compare_kind_operand(compare_op),
                },
            });
        }
        OptimizedInstruction::CompareTeeSelectConst {
            lhs_local_addr,
            rhs_const,
            tee_local_addr,
            select_width,
            op: compare_op,
            ..
        } => {
            let handler = match (compare_op, select_width) {
                (TypedCompareOp::I32(_), ValueSize::Byte4) => {
                    vm::op_i32_local_const_compare_tee_select4
                }
                (TypedCompareOp::I32(_), ValueSize::Byte8) => {
                    vm::op_i32_local_const_compare_tee_select8
                }
                (TypedCompareOp::I64(_), ValueSize::Byte4) => {
                    vm::op_i64_local_const_compare_tee_select4
                }
                (TypedCompareOp::I64(_), ValueSize::Byte8) => {
                    vm::op_i64_local_const_compare_tee_select8
                }
                _ => unreachable!("compare tee select only supports integer compares"),
            };
            lowered.push(Instr { op: handler });
            lowered.push(Instr {
                operand: Operand {
                    local_addr: lhs_local_addr,
                },
            });
            push_const_operand(lowered, rhs_const);
            lowered.push(Instr {
                operand: Operand {
                    local_addr: tee_local_addr,
                },
            });
            lowered.push(Instr {
                operand: Operand {
                    u32: compare_kind_operand(compare_op),
                },
            });
        }
        OptimizedInstruction::LoadConstLocal { start, op, .. } => {
            if op.uses_dedicated_const() {
                lowered.push(Instr {
                    op: vm::op_i32_load_const_local,
                });
                lowered.push(Instr {
                    operand: Operand { u32: start },
                });
            } else {
                lowered.push(Instr {
                    op: match op {
                        TypedLoadOp::Bits4(_) => vm::op_load_const_local4,
                        TypedLoadOp::Bits8(_) => vm::op_load_const_local8,
                    },
                });
                lowered.push(Instr {
                    operand: Operand { u32: start },
                });
                lowered.push(Instr {
                    operand: Operand {
                        u32: load_kind_operand(op),
                    },
                });
            }
        }
        OptimizedInstruction::StoreConstLocal {
            start,
            value_local_addr,
            op,
            ..
        } => {
            if op.uses_dedicated_const() {
                lowered.push(Instr {
                    op: vm::op_i32_local_get4_store_const_local,
                });
                lowered.push(Instr {
                    operand: Operand { u32: start },
                });
                lowered.push(Instr {
                    operand: Operand {
                        local_addr: value_local_addr,
                    },
                });
            } else {
                lowered.push(Instr {
                    op: match op {
                        TypedStoreOp::Bits4(_) => vm::op_local_store_const_local4,
                        TypedStoreOp::Bits8(_) => vm::op_local_store_const_local8,
                    },
                });
                lowered.push(Instr {
                    operand: Operand { u32: start },
                });
                lowered.push(Instr {
                    operand: Operand {
                        local_addr: value_local_addr,
                    },
                });
                lowered.push(Instr {
                    operand: Operand {
                        u32: store_kind_operand(op),
                    },
                });
            }
        }
        OptimizedInstruction::LocalAddrLoad {
            local_addr,
            memarg,
            op,
            ..
        } => {
            if op.uses_dedicated_local_addr() {
                let op = match op {
                    TypedLoadOp::Bits4(vm::Load4Kind::I32) => vm::op_i32_local_addr_load,
                    TypedLoadOp::Bits4(vm::Load4Kind::I32Load8U) => vm::op_i32_local_addr_load8_u,
                    TypedLoadOp::Bits4(vm::Load4Kind::I32Load16S) => vm::op_i32_local_addr_load16_s,
                    TypedLoadOp::Bits4(vm::Load4Kind::I32Load16U) => vm::op_i32_local_addr_load16_u,
                    TypedLoadOp::Bits4(vm::Load4Kind::F32) => vm::op_f32_local_addr_load,
                    _ => unreachable!(),
                };
                lowered.push(Instr { op });
                lowered.push(Instr {
                    operand: Operand { local_addr },
                });
                lowered.push(Instr {
                    operand: Operand { memarg },
                });
            } else {
                lowered.push(Instr {
                    op: match op {
                        TypedLoadOp::Bits4(_) => vm::op_local_addr_load4,
                        TypedLoadOp::Bits8(_) => vm::op_local_addr_load8,
                    },
                });
                lowered.push(Instr {
                    operand: Operand { local_addr },
                });
                lowered.push(Instr {
                    operand: Operand { memarg },
                });
                lowered.push(Instr {
                    operand: Operand {
                        u32: load_kind_operand(op),
                    },
                });
            }
        }
        OptimizedInstruction::LocalImmAddrLoad {
            local_addr,
            imm,
            memarg,
            op,
            ..
        } => {
            let op = match op {
                TypedLoadOp::Bits4(vm::Load4Kind::I32) => vm::op_i32_local_imm_addr_load,
                TypedLoadOp::Bits4(vm::Load4Kind::I32Load8U) => vm::op_i32_local_imm_addr_load8_u,
                TypedLoadOp::Bits4(vm::Load4Kind::I32Load16S) => vm::op_i32_local_imm_addr_load16_s,
                TypedLoadOp::Bits4(vm::Load4Kind::I32Load16U) => vm::op_i32_local_imm_addr_load16_u,
                TypedLoadOp::Bits4(vm::Load4Kind::F32) => vm::op_f32_local_imm_addr_load,
                _ => unreachable!(),
            };
            lowered.push(Instr { op });
            lowered.push(Instr {
                operand: Operand { local_addr },
            });
            lowered.push(Instr {
                operand: Operand { i32: imm },
            });
            lowered.push(Instr {
                operand: Operand { memarg },
            });
        }
        OptimizedInstruction::I32LocalLocalLoadTeeAddImmStore {
            store_addr_local_addr,
            load_addr_local_addr,
            tee_local_addr,
            imm,
            load_memarg,
            store_memarg,
            ..
        } => {
            lowered.push(Instr {
                op: vm::op_i32_local_local_load_tee_add_imm_store,
            });
            lowered.push(Instr {
                operand: Operand {
                    local_addr: store_addr_local_addr,
                },
            });
            lowered.push(Instr {
                operand: Operand {
                    local_addr: load_addr_local_addr,
                },
            });
            lowered.push(Instr {
                operand: Operand {
                    local_addr: tee_local_addr,
                },
            });
            lowered.push(Instr {
                operand: Operand { i32: imm },
            });
            lowered.push(Instr {
                operand: Operand {
                    memarg: load_memarg,
                },
            });
            lowered.push(Instr {
                operand: Operand {
                    memarg: store_memarg,
                },
            });
        }
        OptimizedInstruction::LocalLocalStore {
            addr_local_addr,
            value_local_addr,
            memarg,
            op,
            ..
        } => {
            if op.uses_dedicated_local_local() {
                let op = match op {
                    TypedStoreOp::Bits4(vm::Store4Kind::I32) => vm::op_i32_local_local_store,
                    TypedStoreOp::Bits4(vm::Store4Kind::I32Store8) => vm::op_i32_local_local_store8,
                    TypedStoreOp::Bits4(vm::Store4Kind::I32Store16) => {
                        vm::op_i32_local_local_store16
                    }
                    _ => unreachable!(),
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
            } else {
                lowered.push(Instr {
                    op: match op {
                        TypedStoreOp::Bits4(_) => vm::op_local_local_store4,
                        TypedStoreOp::Bits8(_) => vm::op_local_local_store8,
                    },
                });
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
                lowered.push(Instr {
                    operand: Operand {
                        u32: store_kind_operand(op),
                    },
                });
            }
        }
        OptimizedInstruction::LocalImmLocalStore {
            addr_local_addr,
            imm,
            value_local_addr,
            memarg,
            op,
            ..
        } => {
            let op = match op {
                TypedStoreOp::Bits4(vm::Store4Kind::I32) => vm::op_i32_local_imm_local_store,
                TypedStoreOp::Bits4(vm::Store4Kind::I32Store8) => vm::op_i32_local_imm_local_store8,
                TypedStoreOp::Bits4(vm::Store4Kind::I32Store16) => {
                    vm::op_i32_local_imm_local_store16
                }
                _ => unreachable!(),
            };
            lowered.push(Instr { op });
            lowered.push(Instr {
                operand: Operand {
                    local_addr: addr_local_addr,
                },
            });
            lowered.push(Instr {
                operand: Operand { i32: imm },
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
        OptimizedInstruction::I32LocalLocalNarrowCopy {
            dst_local_addr,
            src_local_addr,
            load_memarg,
            store_memarg,
            kind,
            ..
        } => {
            lowered.push(Instr {
                op: match kind {
                    NarrowCopyKind::Load8Store8 => vm::op_i32_local_local_load8_u_store8,
                    NarrowCopyKind::Load16Store16 => vm::op_i32_local_local_load16_u_store16,
                },
            });
            lowered.push(Instr {
                operand: Operand {
                    local_addr: dst_local_addr,
                },
            });
            lowered.push(Instr {
                operand: Operand {
                    local_addr: src_local_addr,
                },
            });
            lowered.push(Instr {
                operand: Operand {
                    memarg: load_memarg,
                },
            });
            lowered.push(Instr {
                operand: Operand {
                    memarg: store_memarg,
                },
            });
        }
    }
    if start < lowered.len() {
        let op = unsafe { lowered[start].op };
        lowered[start] = Instr {
            op: vm::select_replicated_op(op, function_index, instruction_ordinal),
        };
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

    fn op_is_any(op: crate::common::Op, candidates: &[crate::common::Op]) -> bool {
        candidates
            .iter()
            .copied()
            .any(|candidate| std::ptr::fn_addr_eq(op, candidate))
    }

    fn is_local_get4_family(op: crate::common::Op) -> bool {
        op_is_any(
            op,
            &[
                vm::op_local_get4 as crate::common::Op,
                vm::op_local_get4_r0 as crate::common::Op,
                vm::op_local_get4_r1 as crate::common::Op,
                vm::op_local_get4_r2 as crate::common::Op,
                vm::op_local_get4_r3 as crate::common::Op,
            ],
        )
    }

    fn is_i32_load8_u_local_family(op: crate::common::Op) -> bool {
        op_is_any(
            op,
            &[
                vm::op_i32_load8_u_local as crate::common::Op,
                vm::op_i32_load8_u_local_r0 as crate::common::Op,
                vm::op_i32_load8_u_local_r1 as crate::common::Op,
                vm::op_i32_load8_u_local_r2 as crate::common::Op,
                vm::op_i32_load8_u_local_r3 as crate::common::Op,
            ],
        )
    }

    fn is_f32_load_local_family(op: crate::common::Op) -> bool {
        op_is_any(
            op,
            &[
                vm::op_f32_load_local as crate::common::Op,
                vm::op_f32_load_local_r0 as crate::common::Op,
                vm::op_f32_load_local_r1 as crate::common::Op,
                vm::op_f32_load_local_r2 as crate::common::Op,
                vm::op_f32_load_local_r3 as crate::common::Op,
            ],
        )
    }

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

        assert!(is_local_get4_family(unsafe { optimized[2].op }));
    }

    #[test]
    fn optimizer_does_not_fuse_scalar_when_local_widths_mismatch() {
        let optimized = optimize_core_program(InstructionProgram {
            instr: vec![
                Instr {
                    op: vm::op_local_get8,
                },
                Instr {
                    operand: Operand { local_addr: 0 },
                },
                Instr {
                    op: vm::op_i64_const,
                },
                Instr {
                    operand: Operand { i64: 1 },
                },
                Instr { op: vm::op_i64_add },
                Instr {
                    op: vm::op_local_set4,
                },
                Instr {
                    operand: Operand { local_addr: 0 },
                },
            ],
            instruction_starts: vec![0, 2, 4, 5],
        });

        assert!(std::ptr::fn_addr_eq(
            unsafe { optimized[0].op },
            vm::op_local_get8 as crate::common::Op
        ));
    }

    #[test]
    fn optimizer_does_not_fuse_compare_when_result_local_is_wide() {
        let optimized = optimize_core_program(InstructionProgram {
            instr: vec![
                Instr {
                    op: vm::op_local_get8,
                },
                Instr {
                    operand: Operand { local_addr: 0 },
                },
                Instr {
                    op: vm::op_local_get8,
                },
                Instr {
                    operand: Operand { local_addr: 8 },
                },
                Instr { op: vm::op_i64_eq },
                Instr {
                    op: vm::op_local_set8,
                },
                Instr {
                    operand: Operand { local_addr: 16 },
                },
            ],
            instruction_starts: vec![0, 2, 4, 5],
        });

        assert!(std::ptr::fn_addr_eq(
            unsafe { optimized[0].op },
            vm::op_local_get8 as crate::common::Op
        ));
    }

    #[test]
    fn optimizer_does_not_fuse_local_imm_push_across_jump_target_boundary() {
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
                    op: vm::op_i32_const,
                },
                Instr {
                    operand: Operand { i32: 7 },
                },
                Instr { op: vm::op_i32_add },
            ],
            instruction_starts: vec![0, 2, 4, 6],
        });

        assert!(is_local_get4_family(unsafe { optimized[2].op }));
    }

    #[test]
    fn optimizer_does_not_fuse_compare_select_across_jump_target_boundary() {
        let optimized = optimize_core_program(InstructionProgram {
            instr: vec![
                Instr { op: vm::op_br_if },
                Instr {
                    operand: Operand { jump_addr: 8 },
                },
                Instr {
                    op: vm::op_local_get4,
                },
                Instr {
                    operand: Operand { local_addr: 0 },
                },
                Instr {
                    op: vm::op_local_get4,
                },
                Instr {
                    operand: Operand { local_addr: 4 },
                },
                Instr {
                    op: vm::op_local_get4,
                },
                Instr {
                    operand: Operand { local_addr: 8 },
                },
                Instr {
                    op: vm::op_local_get4,
                },
                Instr {
                    operand: Operand { local_addr: 12 },
                },
                Instr {
                    op: vm::op_i32_lt_u,
                },
                Instr { op: vm::op_select4 },
            ],
            instruction_starts: vec![0, 2, 4, 6, 8, 10, 11],
        });

        assert!(is_local_get4_family(unsafe { optimized[6].op }));
    }

    #[test]
    fn optimizer_does_not_fuse_load_mask_branch_across_jump_target_boundary() {
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
                    op: vm::op_i32_load8_u_local,
                },
                Instr {
                    operand: Operand {
                        memarg: MemArg {
                            align: 1,
                            offset: 0,
                        },
                    },
                },
                Instr {
                    op: vm::op_i32_const,
                },
                Instr {
                    operand: Operand { i32: 32 },
                },
                Instr { op: vm::op_i32_and },
                Instr { op: vm::op_i32_eqz },
                Instr { op: vm::op_if },
                Instr {
                    operand: Operand { jump_addr: 0 },
                },
            ],
            instruction_starts: vec![0, 2, 4, 6, 8, 9, 10],
        });

        assert!(is_local_get4_family(unsafe { optimized[2].op }));
    }

    #[test]
    fn optimizer_replicates_hot_raw_local_get4_handler() {
        let optimized = optimize_core_program_with_function_index(
            InstructionProgram {
                instr: vec![
                    Instr {
                        op: vm::op_local_get4,
                    },
                    Instr {
                        operand: Operand { local_addr: 12 },
                    },
                ],
                instruction_starts: vec![0],
            },
            7,
        );

        let op = unsafe { optimized[0].op };
        assert!(!std::ptr::fn_addr_eq(
            op,
            vm::op_local_get4 as crate::common::Op
        ));
        assert!(op_is_any(
            op,
            &[
                vm::op_local_get4_r0 as crate::common::Op,
                vm::op_local_get4_r1 as crate::common::Op,
                vm::op_local_get4_r2 as crate::common::Op,
                vm::op_local_get4_r3 as crate::common::Op,
            ],
        ));
    }

    #[test]
    fn optimizer_replicates_hot_raw_br_if_handler_without_changing_jump_operand() {
        let optimized = optimize_core_program_with_function_index(
            InstructionProgram {
                instr: vec![
                    Instr { op: vm::op_br_if },
                    Instr {
                        operand: Operand { jump_addr: 0 },
                    },
                ],
                instruction_starts: vec![0],
            },
            11,
        );

        let op = unsafe { optimized[0].op };
        assert!(!std::ptr::fn_addr_eq(op, vm::op_br_if as crate::common::Op));
        assert!(op_is_any(
            op,
            &[
                vm::op_br_if_r0 as crate::common::Op,
                vm::op_br_if_r1 as crate::common::Op,
                vm::op_br_if_r2 as crate::common::Op,
                vm::op_br_if_r3 as crate::common::Op,
            ],
        ));
        assert_eq!(unsafe { optimized[1].operand.jump_addr }, 0);
    }

    #[test]
    fn optimizer_leaves_non_selected_raw_handlers_unreplicated() {
        let const_program = optimize_core_program_with_function_index(
            InstructionProgram {
                instr: vec![
                    Instr {
                        op: vm::op_i32_const,
                    },
                    Instr {
                        operand: Operand { i32: 7 },
                    },
                ],
                instruction_starts: vec![0],
            },
            3,
        );
        let eqz_program = optimize_core_program_with_function_index(
            InstructionProgram {
                instr: vec![Instr { op: vm::op_i32_eqz }],
                instruction_starts: vec![0],
            },
            5,
        );

        assert!(std::ptr::fn_addr_eq(
            unsafe { const_program[0].op },
            vm::op_i32_const as crate::common::Op
        ));
        assert!(std::ptr::fn_addr_eq(
            unsafe { eqz_program[0].op },
            vm::op_i32_eqz as crate::common::Op
        ));

        let tee_program = optimize_core_program_with_function_index(
            InstructionProgram {
                instr: vec![
                    Instr {
                        op: vm::op_local_tee4,
                    },
                    Instr {
                        operand: Operand { local_addr: 0 },
                    },
                ],
                instruction_starts: vec![0],
            },
            7,
        );
        assert!(std::ptr::fn_addr_eq(
            unsafe { tee_program[0].op },
            vm::op_local_tee4 as crate::common::Op
        ));
    }

    #[test]
    fn optimizer_replicates_hot_raw_i32_load8_u_local_handler() {
        let optimized = optimize_core_program_with_function_index(
            InstructionProgram {
                instr: vec![
                    Instr {
                        op: vm::op_i32_load8_u_local,
                    },
                    Instr {
                        operand: Operand {
                            memarg: MemArg {
                                align: 1,
                                offset: 0,
                            },
                        },
                    },
                ],
                instruction_starts: vec![0],
            },
            13,
        );

        assert!(is_i32_load8_u_local_family(unsafe { optimized[0].op }));
        assert!(!std::ptr::fn_addr_eq(
            unsafe { optimized[0].op },
            vm::op_i32_load8_u_local as crate::common::Op
        ));
    }

    #[test]
    fn optimizer_replicates_hot_raw_f32_load_local_handler() {
        let optimized = optimize_core_program_with_function_index(
            InstructionProgram {
                instr: vec![
                    Instr {
                        op: vm::op_f32_load_local,
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
                instruction_starts: vec![0],
            },
            17,
        );

        assert!(is_f32_load_local_family(unsafe { optimized[0].op }));
        assert!(!std::ptr::fn_addr_eq(
            unsafe { optimized[0].op },
            vm::op_f32_load_local as crate::common::Op
        ));
    }

    #[test]
    fn optimizer_fuses_producer_imm_scalar_set_before_shorter_family() {
        let optimized = optimize_core_program(InstructionProgram {
            instr: vec![
                Instr {
                    op: vm::op_local_get4,
                },
                Instr {
                    operand: Operand { local_addr: 0 },
                },
                Instr {
                    op: vm::op_i32_load8_u_local,
                },
                Instr {
                    operand: Operand {
                        memarg: MemArg {
                            align: 1,
                            offset: 0,
                        },
                    },
                },
                Instr {
                    op: vm::op_i32_const,
                },
                Instr {
                    operand: Operand { i32: 31 },
                },
                Instr { op: vm::op_i32_and },
                Instr {
                    op: vm::op_local_set4,
                },
                Instr {
                    operand: Operand { local_addr: 4 },
                },
            ],
            instruction_starts: vec![0, 2, 4, 6, 7],
        });

        assert!(std::ptr::fn_addr_eq(
            unsafe { optimized[0].op },
            vm::op_i32_seed_imm_scalar_set4 as crate::common::Op
        ));
    }

    #[test]
    fn optimizer_fuses_producer_imm_and_branch_before_shorter_family() {
        let optimized = optimize_core_program(InstructionProgram {
            instr: vec![
                Instr {
                    op: vm::op_local_get4,
                },
                Instr {
                    operand: Operand { local_addr: 0 },
                },
                Instr {
                    op: vm::op_i32_load8_u_local,
                },
                Instr {
                    operand: Operand {
                        memarg: MemArg {
                            align: 1,
                            offset: 0,
                        },
                    },
                },
                Instr {
                    op: vm::op_i32_const,
                },
                Instr {
                    operand: Operand { i32: 31 },
                },
                Instr { op: vm::op_i32_and },
                Instr { op: vm::op_i32_eqz },
                Instr { op: vm::op_if },
                Instr {
                    operand: Operand { jump_addr: 0 },
                },
            ],
            instruction_starts: vec![0, 2, 4, 6, 7, 8],
        });

        assert!(std::ptr::fn_addr_eq(
            unsafe { optimized[0].op },
            vm::op_i32_seed_imm_and_eqz_if as crate::common::Op
        ));
    }

    #[test]
    fn optimizer_lowers_float_load_compare_select_superinstruction() {
        let lowered = lower_program(
            vec![OptimizedInstruction::ProducerCompareSelectConst {
                old_range: 0..5,
                seed: ProducerSeed::LocalAddrLoad {
                    width: ValueSize::Byte4,
                    local_addr: 0,
                    memarg: MemArg {
                        align: 4,
                        offset: 0,
                    },
                    op: TypedLoadOp::Bits4(vm::Load4Kind::F32),
                },
                rhs_const: TypedConst::F32(0.0f32.to_bits()),
                select_width: ValueSize::Byte4,
                op: TypedCompareOp::F32(vm::FloatCompareKind::Gt),
            }],
            5,
            0,
        );

        assert!(std::ptr::fn_addr_eq(
            unsafe { lowered.instr[0].op },
            vm::op_f32_seed_const_compare_select4 as crate::common::Op
        ));
    }
}
