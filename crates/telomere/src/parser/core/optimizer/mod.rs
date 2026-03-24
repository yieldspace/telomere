use std::{collections::HashSet, ops::Range, sync::Arc};

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
                    op: TypedLoadOp::Bits4(Load4Kind::F32),
                },
                rhs_const: TypedConst::F32(0.0f32.to_bits()),
                select_width: ValueSize::Byte4,
                op: TypedCompareOp::F32(FloatCompareKind::Gt),
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
