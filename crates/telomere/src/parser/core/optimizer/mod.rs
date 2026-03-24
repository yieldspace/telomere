use std::{ops::Range, sync::Arc};

use crate::{
    common::{
        structured_jump_rewrite, ControlFlowMetadataKind, ControlFlowMetadataSite,
        FloatCompareKind, FloatScalarKind, I32ScalarKind, I64ScalarKind, Instr, IntCompareKind,
        Load4Kind, Load8Kind, MemArg, Op, Operand, ReturnShape, StackMapSite, StackMapSourceSite,
        Store4Kind, Store8Kind, StructuredJumpRewriteKind, UnwindSiteMetadata, UnwindSourceSite,
        ValueSize,
    },
    parser::core::instruction_generator::InstructionProgram,
    runtime::vm::{self, compute_memory_offset},
    VMResult,
};

mod decode;
mod families;
mod lowering;
mod metadata;
mod producer_seed;

use self::{decode::*, families::*, lowering::*, metadata::*, producer_seed::*};

pub(crate) struct OptimizedCoreProgram {
    pub(crate) instr: Vec<Instr>,
    pub(crate) control_flow_metadata: Arc<[ControlFlowMetadataSite]>,
    pub(crate) stack_map_sites: Arc<[StackMapSite]>,
    pub(crate) unwind_sites: Arc<[UnwindSiteMetadata]>,
    pub(crate) instruction_ordinal_by_raw_start: Arc<[u32]>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct InstructionSpan {
    start_raw: u32,
    len_raw: u32,
}

impl InstructionSpan {
    pub(super) fn new(start_raw: usize, end_raw: usize) -> Self {
        let start_raw = u32::try_from(start_raw).expect("instruction span start overflowed u32");
        let end_raw = u32::try_from(end_raw).expect("instruction span end overflowed u32");
        Self::from_bounds(start_raw, end_raw)
    }

    pub(super) fn from_bounds(start_raw: u32, end_raw: u32) -> Self {
        Self {
            start_raw,
            len_raw: end_raw - start_raw,
        }
    }

    pub(super) fn from_old_range(range: &std::ops::Range<usize>) -> Self {
        Self::new(range.start, range.end)
    }

    pub(super) fn start(self) -> usize {
        self.start_raw as usize
    }

    pub(super) fn end(self) -> usize {
        (self.start_raw + self.len_raw) as usize
    }

    pub(super) fn len(self) -> usize {
        self.len_raw as usize
    }
}

pub(super) struct JumpTargetBitmap {
    bits: Box<[u64]>,
}

impl JumpTargetBitmap {
    pub(super) fn with_raw_len(raw_len: usize) -> Self {
        let word_len = raw_len.div_ceil(u64::BITS as usize);
        Self {
            bits: vec![0u64; word_len].into_boxed_slice(),
        }
    }

    pub(super) fn mark(&mut self, raw_index: usize) {
        let word_index = raw_index / u64::BITS as usize;
        let bit_index = raw_index % u64::BITS as usize;
        if let Some(word) = self.bits.get_mut(word_index) {
            *word |= 1u64 << bit_index;
        }
    }

    pub(super) fn contains_raw(&self, raw_index: u32) -> bool {
        let raw_index = raw_index as usize;
        let word_index = raw_index / u64::BITS as usize;
        let bit_index = raw_index % u64::BITS as usize;
        self.bits
            .get(word_index)
            .map(|word| (word & (1u64 << bit_index)) != 0)
            .unwrap_or(false)
    }
}

#[derive(Clone, Copy)]
enum OptimizedInstruction {
    Raw(InstructionSpan),
    ConstSetTee {
        span: InstructionSpan,
        value: TypedConst,
        dst_local: u32,
        tee: bool,
    },
    LocalCopy {
        span: InstructionSpan,
        src_local: u32,
        dst_local: u32,
        width: ValueSize,
        tee: bool,
    },
    LocalImmPush {
        span: InstructionSpan,
        src_local: u32,
        imm: TypedConst,
        op: TypedScalarOp,
    },
    LocalLocalPush {
        span: InstructionSpan,
        lhs_local_addr: u32,
        rhs_local_addr: u32,
        op: TypedScalarOp,
    },
    LocalImmSetTee {
        span: InstructionSpan,
        src_local: u32,
        imm: TypedConst,
        dst_local: u32,
        tee: bool,
        op: TypedScalarOp,
    },
    LocalLocalSetTee {
        span: InstructionSpan,
        lhs_local_addr: u32,
        rhs_local_addr: u32,
        dst_local: u32,
        tee: bool,
        op: TypedScalarOp,
    },
    LocalBranch {
        span: InstructionSpan,
        local_addr: u32,
        target_old: u32,
        width: ValueSize,
        zero_test: bool,
        branch_kind: ControlBranchKind,
    },
    I32LocalAndImmBranch {
        span: InstructionSpan,
        local_addr: u32,
        imm: i32,
        target_old: u32,
        zero_test: bool,
        branch_kind: ControlBranchKind,
    },
    ProducerImmAndBranch {
        span: InstructionSpan,
        seed: ProducerSeed,
        rhs_const: TypedConst,
        width: ValueSize,
        target_old: u32,
        zero_test: bool,
        branch_kind: ControlBranchKind,
    },
    I32LocalAddrLoad8UAndImmEqzBranch {
        span: InstructionSpan,
        local_addr: u32,
        memarg: MemArg,
        imm: i32,
        target_old: u32,
        branch_kind: ControlBranchKind,
    },
    I32LocalLocalGeUBrIf {
        span: InstructionSpan,
        lhs_local_addr: u32,
        rhs_local_addr: u32,
        target_old: u32,
    },
    CompareSetTeeLocal {
        span: InstructionSpan,
        lhs_local_addr: u32,
        rhs_local_addr: u32,
        dst_local: u32,
        tee: bool,
        op: TypedCompareOp,
    },
    CompareSetTeeConst {
        span: InstructionSpan,
        lhs_local_addr: u32,
        rhs_const: TypedConst,
        dst_local: u32,
        tee: bool,
        op: TypedCompareOp,
    },
    CompareBrIfLocal {
        span: InstructionSpan,
        lhs_local_addr: u32,
        rhs_local_addr: u32,
        target_old: u32,
        op: TypedCompareOp,
    },
    CompareBrIfConst {
        span: InstructionSpan,
        lhs_local_addr: u32,
        rhs_const: TypedConst,
        target_old: u32,
        op: TypedCompareOp,
    },
    CompareSelectLocal {
        span: InstructionSpan,
        lhs_local_addr: u32,
        rhs_local_addr: u32,
        select_width: ValueSize,
        op: TypedCompareOp,
    },
    CompareSelectConst {
        span: InstructionSpan,
        lhs_local_addr: u32,
        rhs_const: TypedConst,
        select_width: ValueSize,
        op: TypedCompareOp,
    },
    LoadConstLocal {
        span: InstructionSpan,
        start: u32,
        op: TypedLoadOp,
    },
    StoreConstLocal {
        span: InstructionSpan,
        start: u32,
        value_local_addr: u32,
        op: TypedStoreOp,
    },
    LocalAddrLoad {
        span: InstructionSpan,
        local_addr: u32,
        memarg: MemArg,
        op: TypedLoadOp,
    },
    LocalImmAddrLoad {
        span: InstructionSpan,
        local_addr: u32,
        imm: i32,
        memarg: MemArg,
        op: TypedLoadOp,
    },
    I32LocalLocalLoadTeeAddImmStore {
        span: InstructionSpan,
        store_addr_local_addr: u32,
        load_addr_local_addr: u32,
        tee_local_addr: u32,
        imm: i32,
        load_memarg: MemArg,
        store_memarg: MemArg,
    },
    LocalLocalStore {
        span: InstructionSpan,
        addr_local_addr: u32,
        value_local_addr: u32,
        memarg: MemArg,
        op: TypedStoreOp,
    },
    LocalImmLocalStore {
        span: InstructionSpan,
        addr_local_addr: u32,
        imm: i32,
        value_local_addr: u32,
        memarg: MemArg,
        op: TypedStoreOp,
    },
    I32LocalLocalNarrowCopy {
        span: InstructionSpan,
        dst_local_addr: u32,
        src_local_addr: u32,
        load_memarg: MemArg,
        store_memarg: MemArg,
        kind: NarrowCopyKind,
    },
    ProducerTeeEqzBranch {
        span: InstructionSpan,
        seed: ProducerSeed,
        tee_local_addr: u32,
        target_old: u32,
        width: ValueSize,
        branch_kind: ControlBranchKind,
    },
    ProducerTeeImmCompareBranch {
        span: InstructionSpan,
        seed: ProducerSeed,
        tee_local_addr: u32,
        rhs_const: TypedConst,
        target_old: u32,
        op: TypedCompareOp,
        branch_kind: ControlBranchKind,
    },
    ProducerTeeImmScalarSetTee {
        span: InstructionSpan,
        seed: ProducerSeed,
        tee_local_addr: u32,
        rhs_const: TypedConst,
        dst_local: u32,
        dst_tee: bool,
        op: TypedScalarOp,
    },
    ProducerImmScalarSetTee {
        span: InstructionSpan,
        seed: ProducerSeed,
        rhs_const: TypedConst,
        dst_local: u32,
        dst_tee: bool,
        op: TypedScalarOp,
    },
    ProducerCompareBranchLocal {
        span: InstructionSpan,
        seed: ProducerSeed,
        rhs_local_addr: u32,
        target_old: u32,
        op: TypedCompareOp,
        branch_kind: ControlBranchKind,
    },
    ProducerCompareBranchConst {
        span: InstructionSpan,
        seed: ProducerSeed,
        rhs_const: TypedConst,
        target_old: u32,
        op: TypedCompareOp,
        branch_kind: ControlBranchKind,
    },
    ProducerTeeConstSelfSelect {
        span: InstructionSpan,
        seed: ProducerSeed,
        tee_local_addr: u32,
        rhs_const: TypedConst,
        width: ValueSize,
    },
    ProducerCompareSelectLocal {
        span: InstructionSpan,
        seed: ProducerSeed,
        rhs_local_addr: u32,
        select_width: ValueSize,
        op: TypedCompareOp,
    },
    ProducerCompareSelectConst {
        span: InstructionSpan,
        seed: ProducerSeed,
        rhs_const: TypedConst,
        select_width: ValueSize,
        op: TypedCompareOp,
    },
    CompareTeeSelectLocal {
        span: InstructionSpan,
        lhs_local_addr: u32,
        rhs_local_addr: u32,
        tee_local_addr: u32,
        select_width: ValueSize,
        op: TypedCompareOp,
    },
    CompareTeeSelectConst {
        span: InstructionSpan,
        lhs_local_addr: u32,
        rhs_const: TypedConst,
        tee_local_addr: u32,
        select_width: ValueSize,
        op: TypedCompareOp,
    },
}

impl OptimizedInstruction {
    fn raw(span: InstructionSpan) -> Self {
        Self::Raw(span)
    }
}

type MatchOutcome = (usize, OptimizedInstruction);
type Matcher = fn(&[DecodedInstruction], usize, &JumpTargetBitmap) -> Option<MatchOutcome>;

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
const PRODUCER_COMPARE_BRANCH_MATCHERS: &[Matcher] = &[
    match_producer_local_compare_branch,
    match_producer_const_compare_branch,
];
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
    let jump_targets = collect_jump_targets(&decoded, program.instr.len(), &program.instr);
    let optimized = fuse_superinstructions(decoded, &jump_targets);
    let lowered = lower_program(
        optimized,
        program.instr.len(),
        function_index,
        &program.instr,
    );
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
    }

    #[test]
    fn optimizer_replicates_hot_raw_local_set4_handler() {
        let optimized = optimize_core_program_with_function_index(
            InstructionProgram {
                instr: vec![
                    Instr {
                        op: vm::op_local_set4,
                    },
                    Instr {
                        operand: Operand { local_addr: 8 },
                    },
                ],
                instruction_starts: vec![0],
            },
            19,
        );

        let op = unsafe { optimized[0].op };
        assert!(!std::ptr::fn_addr_eq(
            op,
            vm::op_local_set4 as crate::common::Op
        ));
        assert!(op_is_any(
            op,
            &[
                vm::op_local_set4_r0 as crate::common::Op,
                vm::op_local_set4_r1 as crate::common::Op,
                vm::op_local_set4_r2 as crate::common::Op,
                vm::op_local_set4_r3 as crate::common::Op,
            ],
        ));
    }

    #[test]
    fn optimizer_replicates_hot_raw_local_tee4_handler() {
        let optimized = optimize_core_program_with_function_index(
            InstructionProgram {
                instr: vec![
                    Instr {
                        op: vm::op_local_tee4,
                    },
                    Instr {
                        operand: Operand { local_addr: 12 },
                    },
                ],
                instruction_starts: vec![0],
            },
            23,
        );

        let op = unsafe { optimized[0].op };
        assert!(!std::ptr::fn_addr_eq(
            op,
            vm::op_local_tee4 as crate::common::Op
        ));
        assert!(op_is_any(
            op,
            &[
                vm::op_local_tee4_r0 as crate::common::Op,
                vm::op_local_tee4_r1 as crate::common::Op,
                vm::op_local_tee4_r2 as crate::common::Op,
                vm::op_local_tee4_r3 as crate::common::Op,
            ],
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
                span: InstructionSpan::new(0, 5),
                seed: ProducerSeed::LocalAddrLoad {
                    width: ValueSize::Byte4,
                    local_addr: 0,
                    memarg: MemArg {
                        align: 4,
                        offset: 0,
                    },
                    op: TypedLoadOp::Bits4(Load4Kind::F32),
                },
                rhs_const: TypedConst::F32(0.0f32.to_bits()),
                select_width: ValueSize::Byte4,
                op: TypedCompareOp::F32(FloatCompareKind::Gt),
            }],
            5,
            0,
            &[],
        );

        assert!(std::ptr::fn_addr_eq(
            unsafe { lowered.instr[0].op },
            vm::op_f32_seed_const_compare_select4 as crate::common::Op
        ));
    }

    #[test]
    fn optimizer_ir_stays_compact_after_span_backed_raw_refactor() {
        use std::mem::{needs_drop, size_of};

        type OldDecodedOwned = (Range<usize>, DecodedKind, Box<[Instr]>);
        type OldHeavyPayload = (
            Range<usize>,
            ProducerSeed,
            TypedConst,
            TypedCompareOp,
            ValueSize,
            u32,
            bool,
            ControlBranchKind,
        );

        assert!(size_of::<InstructionSpan>() < size_of::<Range<usize>>());
        assert!(size_of::<DecodedInstruction>() < size_of::<OldDecodedOwned>());
        assert!(size_of::<OptimizedInstruction>() <= size_of::<OldHeavyPayload>());
        assert!(!needs_drop::<DecodedInstruction>());
        assert!(!needs_drop::<OptimizedInstruction>());
    }
}
