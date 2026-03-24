use super::*;
use smallvec::SmallVec;

pub(super) struct LoweredProgram {
    pub(super) instr: Vec<Instr>,
    pub(super) instruction_starts: Vec<usize>,
    pub(super) old_to_new: Vec<u32>,
}

pub(super) fn lower_program(
    optimized: Vec<OptimizedInstruction>,
    old_flat_len: usize,
    function_index: u32,
    source_instrs: &[Instr],
) -> LoweredProgram {
    let mut old_to_new = vec![0u32; old_flat_len];
    let mut new_len = 0usize;
    for instruction in &optimized {
        let span = old_span(*instruction);
        let lowered_start = u32::try_from(new_len).expect("optimized program grew too large");
        for entry in old_to_new.iter_mut().take(span.end()).skip(span.start()) {
            *entry = lowered_start;
        }
        new_len += output_len(*instruction);
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
            source_instrs,
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

pub(super) fn loop_shape_op(op: Op) -> Option<ReturnShape> {
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

pub(super) fn block_return_shape_op(op: Op) -> Option<ReturnShape> {
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

fn old_span(instruction: OptimizedInstruction) -> InstructionSpan {
    match instruction {
        OptimizedInstruction::Raw(span)
        | OptimizedInstruction::ConstSetTee { span, .. }
        | OptimizedInstruction::LocalCopy { span, .. }
        | OptimizedInstruction::LocalImmPush { span, .. }
        | OptimizedInstruction::LocalLocalPush { span, .. }
        | OptimizedInstruction::LocalImmSetTee { span, .. }
        | OptimizedInstruction::LocalLocalSetTee { span, .. }
        | OptimizedInstruction::LocalBranch { span, .. }
        | OptimizedInstruction::I32LocalAndImmBranch { span, .. }
        | OptimizedInstruction::ProducerImmAndBranch { span, .. }
        | OptimizedInstruction::I32LocalAddrLoad8UAndImmEqzBranch { span, .. }
        | OptimizedInstruction::I32LocalLocalGeUBrIf { span, .. }
        | OptimizedInstruction::CompareSetTeeLocal { span, .. }
        | OptimizedInstruction::CompareSetTeeConst { span, .. }
        | OptimizedInstruction::CompareBrIfLocal { span, .. }
        | OptimizedInstruction::CompareBrIfConst { span, .. }
        | OptimizedInstruction::CompareSelectLocal { span, .. }
        | OptimizedInstruction::CompareSelectConst { span, .. }
        | OptimizedInstruction::LoadConstLocal { span, .. }
        | OptimizedInstruction::StoreConstLocal { span, .. }
        | OptimizedInstruction::LocalAddrLoad { span, .. }
        | OptimizedInstruction::LocalImmAddrLoad { span, .. }
        | OptimizedInstruction::I32LocalLocalLoadTeeAddImmStore { span, .. }
        | OptimizedInstruction::LocalLocalStore { span, .. }
        | OptimizedInstruction::LocalImmLocalStore { span, .. }
        | OptimizedInstruction::I32LocalLocalNarrowCopy { span, .. }
        | OptimizedInstruction::ProducerTeeEqzBranch { span, .. }
        | OptimizedInstruction::ProducerTeeImmCompareBranch { span, .. }
        | OptimizedInstruction::ProducerTeeImmScalarSetTee { span, .. }
        | OptimizedInstruction::ProducerImmScalarSetTee { span, .. }
        | OptimizedInstruction::ProducerCompareBranchLocal { span, .. }
        | OptimizedInstruction::ProducerCompareBranchConst { span, .. }
        | OptimizedInstruction::ProducerTeeConstSelfSelect { span, .. }
        | OptimizedInstruction::ProducerCompareSelectLocal { span, .. }
        | OptimizedInstruction::ProducerCompareSelectConst { span, .. }
        | OptimizedInstruction::CompareTeeSelectLocal { span, .. }
        | OptimizedInstruction::CompareTeeSelectConst { span, .. } => span,
    }
}

fn output_len(instruction: OptimizedInstruction) -> usize {
    match instruction {
        OptimizedInstruction::Raw(span) => span.len(),
        OptimizedInstruction::ConstSetTee { .. } => 3,
        OptimizedInstruction::LocalCopy { .. } => 3,
        OptimizedInstruction::LocalImmPush { .. } | OptimizedInstruction::LocalLocalPush { .. } => {
            4
        }
        OptimizedInstruction::LocalImmSetTee { op, .. } => {
            if is_existing_i32_local_imm_fastpath(op) {
                4
            } else {
                5
            }
        }
        OptimizedInstruction::LocalLocalSetTee { op, .. } => {
            if is_existing_i32_local_local_fastpath(op) {
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
        OptimizedInstruction::ProducerCompareBranchLocal { .. }
        | OptimizedInstruction::ProducerCompareBranchConst { .. } => 9,
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
    source_instrs: &[Instr],
) {
    let start = lowered.len();
    match instruction {
        OptimizedInstruction::Raw(span) => {
            rewrite_raw_jumps_into(
                &source_instrs[span.start()..span.end()],
                old_to_new,
                lowered,
            );
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
                    I32ScalarKind::Add
                        | I32ScalarKind::Sub
                        | I32ScalarKind::And
                        | I32ScalarKind::Shl
                        | I32ScalarKind::ShrU
                ) =>
            {
                let op = match (kind, tee) {
                    (I32ScalarKind::Add, false) => vm::op_i32_local_add_imm_set4,
                    (I32ScalarKind::Add, true) => vm::op_i32_local_add_imm_tee4,
                    (I32ScalarKind::Sub, false) => vm::op_i32_local_sub_imm_set4,
                    (I32ScalarKind::Sub, true) => vm::op_i32_local_sub_imm_tee4,
                    (I32ScalarKind::And, false) => vm::op_i32_local_and_imm_set4,
                    (I32ScalarKind::And, true) => vm::op_i32_local_and_imm_tee4,
                    (I32ScalarKind::Shl, false) => vm::op_i32_local_shl_imm_set4,
                    (I32ScalarKind::Shl, true) => vm::op_i32_local_shl_imm_tee4,
                    (I32ScalarKind::ShrU, false) => vm::op_i32_local_shr_u_imm_set4,
                    (I32ScalarKind::ShrU, true) => vm::op_i32_local_shr_u_imm_tee4,
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
            TypedScalarOp::I32(I32ScalarKind::Add) => {
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
        OptimizedInstruction::ProducerCompareBranchLocal {
            seed,
            rhs_local_addr,
            target_old,
            op: compare_op,
            branch_kind,
            ..
        } => {
            let handler = match (compare_op, branch_kind) {
                (TypedCompareOp::I32(_), ControlBranchKind::BrIf) => {
                    vm::op_i32_seed_local_compare_br_if
                }
                (TypedCompareOp::I32(_), ControlBranchKind::If) => vm::op_i32_seed_local_compare_if,
                (TypedCompareOp::I64(_), ControlBranchKind::BrIf) => {
                    vm::op_i64_seed_local_compare_br_if
                }
                (TypedCompareOp::I64(_), ControlBranchKind::If) => vm::op_i64_seed_local_compare_if,
                (TypedCompareOp::F32(_), ControlBranchKind::BrIf) => {
                    vm::op_f32_seed_local_compare_br_if
                }
                (TypedCompareOp::F32(_), ControlBranchKind::If) => vm::op_f32_seed_local_compare_if,
                (TypedCompareOp::F64(_), ControlBranchKind::BrIf) => {
                    vm::op_f64_seed_local_compare_br_if
                }
                (TypedCompareOp::F64(_), ControlBranchKind::If) => vm::op_f64_seed_local_compare_if,
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
                    jump_addr: remap_jump_target(target_old, old_to_new),
                },
            });
            lowered.push(Instr {
                operand: Operand {
                    u32: compare_kind_operand(compare_op),
                },
            });
        }
        OptimizedInstruction::ProducerCompareBranchConst {
            seed,
            rhs_const,
            target_old,
            op: compare_op,
            branch_kind,
            ..
        } => {
            let handler = match (compare_op, branch_kind) {
                (TypedCompareOp::I32(_), ControlBranchKind::BrIf) => {
                    vm::op_i32_seed_const_compare_br_if
                }
                (TypedCompareOp::I32(_), ControlBranchKind::If) => vm::op_i32_seed_const_compare_if,
                (TypedCompareOp::I64(_), ControlBranchKind::BrIf) => {
                    vm::op_i64_seed_const_compare_br_if
                }
                (TypedCompareOp::I64(_), ControlBranchKind::If) => vm::op_i64_seed_const_compare_if,
                (TypedCompareOp::F32(_), ControlBranchKind::BrIf) => {
                    vm::op_f32_seed_const_compare_br_if
                }
                (TypedCompareOp::F32(_), ControlBranchKind::If) => vm::op_f32_seed_const_compare_if,
                (TypedCompareOp::F64(_), ControlBranchKind::BrIf) => {
                    vm::op_f64_seed_const_compare_br_if
                }
                (TypedCompareOp::F64(_), ControlBranchKind::If) => vm::op_f64_seed_const_compare_if,
            };
            lowered.push(Instr { op: handler });
            push_producer_seed_operands(lowered, seed);
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
                    TypedLoadOp::Bits4(Load4Kind::I32) => vm::op_i32_local_addr_load,
                    TypedLoadOp::Bits4(Load4Kind::I32Load8U) => vm::op_i32_local_addr_load8_u,
                    TypedLoadOp::Bits4(Load4Kind::I32Load16S) => vm::op_i32_local_addr_load16_s,
                    TypedLoadOp::Bits4(Load4Kind::I32Load16U) => vm::op_i32_local_addr_load16_u,
                    TypedLoadOp::Bits4(Load4Kind::F32) => vm::op_f32_local_addr_load,
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
                TypedLoadOp::Bits4(Load4Kind::I32) => vm::op_i32_local_imm_addr_load,
                TypedLoadOp::Bits4(Load4Kind::I32Load8U) => vm::op_i32_local_imm_addr_load8_u,
                TypedLoadOp::Bits4(Load4Kind::I32Load16S) => vm::op_i32_local_imm_addr_load16_s,
                TypedLoadOp::Bits4(Load4Kind::I32Load16U) => vm::op_i32_local_imm_addr_load16_u,
                TypedLoadOp::Bits4(Load4Kind::F32) => vm::op_f32_local_imm_addr_load,
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
                    TypedStoreOp::Bits4(Store4Kind::I32) => vm::op_i32_local_local_store,
                    TypedStoreOp::Bits4(Store4Kind::I32Store8) => vm::op_i32_local_local_store8,
                    TypedStoreOp::Bits4(Store4Kind::I32Store16) => vm::op_i32_local_local_store16,
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
                TypedStoreOp::Bits4(Store4Kind::I32) => vm::op_i32_local_imm_local_store,
                TypedStoreOp::Bits4(Store4Kind::I32Store8) => vm::op_i32_local_imm_local_store8,
                TypedStoreOp::Bits4(Store4Kind::I32Store16) => vm::op_i32_local_imm_local_store16,
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

fn rewrite_raw_jumps_into(raw: &[Instr], old_to_new: &[u32], lowered: &mut Vec<Instr>) {
    let mut rewritten: SmallVec<[Instr; 8]> = SmallVec::from_slice(raw);
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
        lowered.extend(rewritten);
        return;
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
    lowered.extend(rewritten);
}

fn remap_jump_target(target_old: u32, old_to_new: &[u32]) -> u32 {
    old_to_new[target_old as usize]
}
