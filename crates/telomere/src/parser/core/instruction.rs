use super::base::WasmBaseParser;
use super::instruction_generator::InstructionGenerator;
use super::jump_resolver::JumpResolver;
use super::type_checker::TypeChecker;
use super::validate::*;
use super::values;
use super::Result;
use crate::binary::BinaryReader;
use crate::common::BlockReturn;
use crate::common::ElemInit;
use crate::common::ElemMode;
use crate::common::GlobalType;
use crate::common::LoopParam;
use crate::common::MemArg;
use crate::common::Mut;
use crate::common::RefType;
use crate::common::ResultType;
use crate::parser::core::jump_resolver::JumpResolverDSL;
use crate::parser::core::type_checker::MaybeUnreachable;
use crate::runtime::vm;
use crate::{
    common::{
        BlockType, ConstExpr, DataCountVerifier, Elem, FuncIdx, FuncType, Instr,
        LocalReassignTable, MemType, Op, Operand, ReturnShape, StackMapSafepointKind,
        StackMapSourceSite, TableType, TypeIdx, TypeSection, UnwindSourceSite, ValType, ValueSize,
    },
    WasmParserError,
};
use std::sync::Arc;
use tracing::trace;

macro_rules! simd_instruction {
    ($code: expr,$ctx: expr, $($name: ident),*) => {
        match ($code) {
            $($name::CODE => $name::parse(&mut $ctx)?,)*
            _unknown => Err(WasmParserError::InvalidInstruction([0xFD, 0, 0, 0]))?,
        }
    }
}
fn get_local_addr(
    ty: &ResultType,
    locals: &LocalReassignTable,
    idx: u32,
) -> Result<(ValType, u32)> {
    let mut param_addr = 0u32;
    let mut i = 0u32;
    tracing::trace!("get_local_addr: {locals:?}");
    for t in ty.iter() {
        if idx < i + 1 {
            return Ok((*t, param_addr));
        }
        param_addr = param_addr
            .checked_add(t.stack_size().u32())
            .ok_or(WasmParserError::TooManyLocals)?;
        i += 1;
    }
    let param_len = i;
    for group in &locals.0 {
        if idx < param_len + group.local_end_exclusive {
            let local_index_in_group = idx
                .checked_sub(i)
                .ok_or(WasmParserError::InvalidLocalIndex(idx))?;
            let addr = param_addr
                .checked_add(group.offset_from_local_top)
                .and_then(|base| {
                    local_index_in_group
                        .checked_mul(group.val_type.stack_size().u32())
                        .and_then(|delta| base.checked_add(delta))
                })
                .ok_or(WasmParserError::TooManyLocals)?;
            return Ok((group.val_type, addr));
        }
        i = param_len
            .checked_add(group.local_end_exclusive)
            .ok_or(WasmParserError::TooManyLocals)?;
    }
    Err(WasmParserError::InvalidLocalIndex(idx))
}

fn validate_br_table_types(
    idx: u32,
    type_section: &TypeSection,
    checker: &mut TypeChecker,
) -> Result<u32> {
    let (kind, blocktype, _) = checker.get_block(idx as usize)?;
    let result_len = {
        match kind {
            BlockKind::Block | BlockKind::If => match blocktype {
                BlockType::Void => 0,
                BlockType::ValType(ty) => {
                    checker.check(&[*ty])?;
                    1
                }
                BlockType::TypeIdx(idx) => {
                    let ty = type_section
                        .get(*idx)
                        .ok_or(WasmParserError::InvalidTypeIdx(*idx))?;
                    checker.check(&ty.1 .0)?;
                    ty.1.iter().count() as u32
                }
            },
            BlockKind::Loop => match blocktype {
                BlockType::Void | BlockType::ValType(_) => {
                    // ok
                    0
                }
                BlockType::TypeIdx(idx) => {
                    let ty = type_section
                        .get(*idx)
                        .ok_or(WasmParserError::InvalidTypeIdx(*idx))?;
                    checker.check(&ty.0 .0)?;
                    ty.0.iter().count() as u32
                }
            },
        }
    };
    Ok(result_len)
}

const fn loop_op_for_shape(shape: ReturnShape) -> Op {
    match shape {
        ReturnShape::Empty => vm::op_loop_empty,
        ReturnShape::Scalar4 => vm::op_loop4,
        ReturnShape::Scalar8 => vm::op_loop8,
        ReturnShape::Generic => vm::op_loop_generic,
    }
}

const fn block_return_op_for_shape(shape: ReturnShape) -> Op {
    match shape {
        ReturnShape::Empty => vm::special_block_return_empty,
        ReturnShape::Scalar4 => vm::special_block_return4,
        ReturnShape::Scalar8 => vm::special_block_return8,
        ReturnShape::Generic => vm::special_block_return_generic,
    }
}
fn assert_data_idx(idx: u32, dcv: &mut DataCountVerifier) -> Result<()> {
    match dcv {
        DataCountVerifier::OnePass(count) => {
            if *count <= idx {
                Err(WasmParserError::InvalidDataIdx(idx))?;
            }
        }
        DataCountVerifier::Lazy {
            max_data_idx: Some(max_data_idx),
        } => {
            *max_data_idx = idx.max(*max_data_idx);
        }
        DataCountVerifier::Lazy { max_data_idx } => {
            *max_data_idx = Some(idx);
        }
    }

    Ok(())
}

#[inline(always)]
fn default_memory_is_shared(mems: &[MemType]) -> bool {
    mems.first().map(|mem| mem.shared).unwrap_or(false)
}

#[derive(Debug)]
pub(crate) enum BlockKind {
    Block,
    If,
    Loop,
}
pub struct InstructionParser<'a, R: BinaryReader> {
    reader: &'a mut R,
    types: &'a TypeSection,
    functions: &'a [TypeIdx],
    imported_function_len: u32,
    funcidx: FuncIdx,
    mems: &'a [MemType],
    functype: &'a FuncType,
    locals: &'a LocalReassignTable,
    globals: &'a [GlobalType],
    tables: &'a [TableType],
    elems: &'a [Elem],
}
impl<R: BinaryReader> WasmBaseParser<R> for InstructionParser<'_, R> {
    fn reader(&mut self) -> &mut R {
        self.reader
    }
}
impl<'a, R: BinaryReader> InstructionParser<'a, R> {
    fn record_stack_map_site(
        &self,
        instrs: &InstructionGenerator,
        checker: &TypeChecker,
        stack_map_sites: &mut Vec<StackMapSourceSite>,
        kind: StackMapSafepointKind,
    ) {
        let Some(operand_bytes) = checker.current_stack_byte_size() else {
            return;
        };
        let Some(ref_offsets_from_operand_base) = checker.current_ref_offsets_from_operand_base()
        else {
            return;
        };
        stack_map_sites.push(StackMapSourceSite {
            raw_start: instrs.len(),
            kind,
            operand_bytes,
            ref_offsets_from_operand_base: Arc::from(ref_offsets_from_operand_base),
        });
    }

    fn record_unwind_site(
        &self,
        instrs: &InstructionGenerator,
        unwind_sites: &mut Vec<UnwindSourceSite>,
        kind: StackMapSafepointKind,
        result_slot_from_local_top: Option<u32>,
    ) {
        unwind_sites.push(UnwindSourceSite {
            raw_start: instrs.len(),
            kind,
            result_slot_from_local_top,
        });
    }

    #[inline(always)]
    fn is_local_direct_call_target(&self, idx: u32) -> bool {
        idx >= self.imported_function_len
    }

    fn parse_memory_index_immediate(&mut self, _opcode: u8) -> Result<(usize, u32)> {
        self.parse_u32()
    }

    fn memory_type(&self, memidx: u32) -> Result<MemType> {
        assert_memory(self.mems)?;
        self.mems
            .get(memidx as usize)
            .copied()
            .ok_or(WasmParserError::InvalidMemIdx(memidx))
    }

    #[allow(clippy::too_many_arguments)]
    fn select_memory_op(
        &self,
        memidx: u32,
        local: Op,
        shared: Op,
        indexed_local: Op,
        indexed_shared: Op,
    ) -> Result<Op> {
        let memory = self.memory_type(memidx)?;
        Ok(match (memidx == 0, memory.shared) {
            (true, false) => local,
            (true, true) => shared,
            (false, false) => indexed_local,
            (false, true) => indexed_shared,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn select_memory_copy_op(
        &self,
        dst_memidx: u32,
        src_memidx: u32,
        default_local: Op,
        default_shared: Op,
        indexed_local_local: Op,
        indexed_local_shared: Op,
        indexed_shared_local: Op,
        indexed_shared_shared: Op,
    ) -> Result<Op> {
        let dst_memory = self.memory_type(dst_memidx)?;
        let src_memory = self.memory_type(src_memidx)?;
        if dst_memidx == 0 && src_memidx == 0 {
            return Ok(if default_memory_is_shared(self.mems) {
                default_shared
            } else {
                default_local
            });
        }
        Ok(match (dst_memory.shared, src_memory.shared) {
            (false, false) => indexed_local_local,
            (false, true) => indexed_local_shared,
            (true, false) => indexed_shared_local,
            (true, true) => indexed_shared_shared,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn push_memarg_instruction(
        &self,
        instrs: &mut InstructionGenerator,
        memidx: u32,
        memarg: MemArg,
        local: Op,
        shared: Op,
        indexed_local: Op,
        indexed_shared: Op,
    ) -> Result<()> {
        let op = self.select_memory_op(memidx, local, shared, indexed_local, indexed_shared)?;
        if memidx == 0 {
            instrs.push_with_operand(op, &[Operand { memarg }]);
        } else {
            instrs.push_with_operand(op, &[Operand { memarg }, Operand { u32: memidx }]);
        }
        Ok(())
    }

    fn push_memidx_instruction(
        &self,
        instrs: &mut InstructionGenerator,
        memidx: u32,
        local: Op,
        shared: Op,
        indexed_local: Op,
        indexed_shared: Op,
    ) -> Result<()> {
        let op = self.select_memory_op(memidx, local, shared, indexed_local, indexed_shared)?;
        if memidx == 0 {
            instrs.push(Instr { op });
        } else {
            instrs.push_with_operand(op, &[Operand { u32: memidx }]);
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn push_memidx_u32_instruction(
        &self,
        instrs: &mut InstructionGenerator,
        extra: u32,
        memidx: u32,
        local: Op,
        shared: Op,
        indexed_local: Op,
        indexed_shared: Op,
    ) -> Result<()> {
        let op = self.select_memory_op(memidx, local, shared, indexed_local, indexed_shared)?;
        if memidx == 0 {
            instrs.push_with_operand(op, &[Operand { u32: extra }]);
        } else {
            instrs.push_with_operand(op, &[Operand { u32: extra }, Operand { u32: memidx }]);
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn push_memory_copy_instruction(
        &self,
        instrs: &mut InstructionGenerator,
        dst_memidx: u32,
        src_memidx: u32,
        default_local: Op,
        default_shared: Op,
        indexed_local_local: Op,
        indexed_local_shared: Op,
        indexed_shared_local: Op,
        indexed_shared_shared: Op,
    ) -> Result<()> {
        let op = self.select_memory_copy_op(
            dst_memidx,
            src_memidx,
            default_local,
            default_shared,
            indexed_local_local,
            indexed_local_shared,
            indexed_shared_local,
            indexed_shared_shared,
        )?;
        if dst_memidx == 0 && src_memidx == 0 {
            instrs.push(Instr { op });
        } else {
            instrs.push_with_operand(
                op,
                &[Operand { u32: dst_memidx }, Operand { u32: src_memidx }],
            );
        }
        Ok(())
    }

    fn parse_memarg(&mut self, natural_align: u32) -> Result<(usize, u32, MemArg)> {
        values::parse_memarg(self.reader, natural_align)
    }

    fn parse_atomic_memarg(&mut self, natural_align_log2: u32) -> Result<(usize, u32, MemArg)> {
        values::parse_memarg_exact(self.reader, natural_align_log2)
    }
    #[allow(clippy::too_many_arguments)]
    fn parse_inst(
        &mut self,
        data_count_section: &mut DataCountVerifier,
        instrs: &mut InstructionGenerator,
        checker: &mut TypeChecker,
        jump_resolver: &mut JumpResolver,
        else_addr: &mut Option<u32>,
        stack_map_sites: &mut Vec<StackMapSourceSite>,
        unwind_sites: &mut Vec<UnwindSourceSite>,
    ) -> Result<(usize, bool)> {
        let v = self.reader.read_exact_one()?;

        Ok(match v {
            0x00 => {
                trace!("parse_op_unreachable");
                instrs
                    .push(Instr {
                        op: vm::op_unreachable,
                    })
                    .set_unreachable();

                checker.unreachable();
                (1, false)
            }
            0x01 => (1, false),
            0x02 => {
                let (len, blocktype) = self.parse_block_type()?;
                trace!("parse_op_block: {blocktype:?}");

                if let BlockType::TypeIdx(idx) = blocktype {
                    let ty = self
                        .types
                        .get(idx)
                        .ok_or(WasmParserError::InvalidTypeIdx(idx))?;
                    checker.op(&ty.0 .0, &[])?;
                    checker.enter_block(BlockKind::Block, blocktype);
                    checker.op(&[], &ty.0 .0)?;
                } else {
                    checker.enter_block(BlockKind::Block, blocktype);
                };
                jump_resolver.push(JumpResolverDSL::EnterForwardJumpBlock);
                instrs.enter_block();
                let len2 = self.parse_instrs(
                    data_count_section,
                    instrs,
                    checker,
                    jump_resolver,
                    else_addr,
                    stack_map_sites,
                    unwind_sites,
                )?;
                instrs.leave_block();
                if !instrs.is_unreachable() {
                    let block_base_stack_size = checker.block_base_stack_size()?;
                    let return_size = blocktype
                        .return_size(self.types)
                        .ok_or(WasmParserError::InvalidStackValTypeAny)?;
                    let return_shape = blocktype
                        .return_shape(self.types)
                        .ok_or(WasmParserError::InvalidStackValTypeAny)?;

                    self.record_stack_map_site(
                        instrs,
                        checker,
                        stack_map_sites,
                        StackMapSafepointKind::BlockReturn,
                    );
                    self.record_unwind_site(
                        instrs,
                        unwind_sites,
                        StackMapSafepointKind::BlockReturn,
                        Some(block_base_stack_size),
                    );
                    instrs.push(Instr {
                        op: block_return_op_for_shape(return_shape),
                    });
                    instrs.push(Instr {
                        operand: Operand {
                            block_return: BlockReturn::with_shape(
                                block_base_stack_size,
                                return_size,
                                return_shape,
                            ),
                        },
                    });
                }

                trace!("parse_op_block(2): {checker:?}");
                match blocktype {
                    BlockType::TypeIdx(idx) => {
                        let ty = self
                            .types
                            .get(idx)
                            .ok_or(WasmParserError::InvalidTypeIdx(idx))?;

                        checker.op(&ty.1 .0, &[])?;

                        checker.leave_block()?;
                        checker.op(&[], &ty.1 .0)?;
                    }
                    BlockType::ValType(ty) => {
                        checker.op(&[ty], &[])?;

                        checker.leave_block()?;
                        checker.op(&[], &[ty])?;
                    }
                    BlockType::Void => {
                        checker.leave_block()?;
                    }
                };

                (1 + len + len2, false)
            }
            0x03 => {
                let (len, blocktype) = self.parse_block_type()?;
                trace!("parse_op_loop: {blocktype:?}");

                if let BlockType::TypeIdx(idx) = blocktype {
                    let ty = self
                        .types
                        .get(idx)
                        .ok_or(WasmParserError::InvalidTypeIdx(idx))?;
                    checker.op(&ty.0 .0, &[])?;
                    checker.enter_block(BlockKind::Loop, blocktype);
                    checker.op(&[], &ty.0 .0)?;
                } else {
                    checker.enter_block(BlockKind::Loop, blocktype);
                }
                jump_resolver.push(JumpResolverDSL::EnterBackwardJumpBlock(instrs.len() as u32));

                let param_shape = blocktype
                    .param_shape(self.types)
                    .ok_or(WasmParserError::InvalidStackValTypeAny)?;
                self.record_stack_map_site(
                    instrs,
                    checker,
                    stack_map_sites,
                    StackMapSafepointKind::Loop,
                );
                self.record_unwind_site(
                    instrs,
                    unwind_sites,
                    StackMapSafepointKind::Loop,
                    Some(checker.block_base_stack_size()?),
                );
                instrs.push(Instr {
                    op: loop_op_for_shape(param_shape),
                });
                if !instrs.is_unreachable() {
                    let block_base_stack_size = checker.block_base_stack_size()?;
                    let param_size = blocktype
                        .param_size(self.types)
                        .ok_or(WasmParserError::InvalidStackValTypeAny)?;
                    instrs.push(Instr {
                        operand: Operand {
                            loop_param: LoopParam::with_shape(
                                block_base_stack_size,
                                param_size,
                                param_shape,
                            ),
                        },
                    });
                }

                instrs.enter_block();
                let len2 = self.parse_instrs(
                    data_count_section,
                    instrs,
                    checker,
                    jump_resolver,
                    else_addr,
                    stack_map_sites,
                    unwind_sites,
                )?;
                instrs.leave_block();
                if !instrs.is_unreachable() {
                    let block_base_stack_size = checker.block_base_stack_size()?;
                    let return_size = blocktype
                        .return_size(self.types)
                        .ok_or(WasmParserError::InvalidStackValTypeAny)?;
                    let return_shape = blocktype
                        .return_shape(self.types)
                        .ok_or(WasmParserError::InvalidStackValTypeAny)?;

                    self.record_stack_map_site(
                        instrs,
                        checker,
                        stack_map_sites,
                        StackMapSafepointKind::BlockReturn,
                    );
                    self.record_unwind_site(
                        instrs,
                        unwind_sites,
                        StackMapSafepointKind::BlockReturn,
                        Some(block_base_stack_size),
                    );
                    instrs.push(Instr {
                        op: block_return_op_for_shape(return_shape),
                    });
                    instrs.push(Instr {
                        operand: Operand {
                            block_return: BlockReturn::with_shape(
                                block_base_stack_size,
                                return_size,
                                return_shape,
                            ),
                        },
                    });
                }

                match blocktype {
                    BlockType::Void => {
                        checker.leave_block()?;
                    }
                    BlockType::TypeIdx(idx) => {
                        let ty = self
                            .types
                            .get(idx)
                            .ok_or(WasmParserError::InvalidTypeIdx(idx))?;
                        checker.op(&ty.1 .0, &[])?;
                        checker.leave_block()?;
                        checker.op(&[], &ty.1 .0)?;
                    }
                    BlockType::ValType(ty) => {
                        checker.op(&[ty], &[])?;

                        checker.leave_block()?;
                        checker.op(&[], &[ty])?;
                    }
                };

                (1 + len + len2, false)
            }
            0x04 => {
                trace!("parse_op_if");
                let (len, blocktype) = self.parse_block_type()?;
                let is_unreachable_if_block = instrs.is_unreachable();
                instrs.push(Instr { op: vm::op_if });
                instrs.push(Instr {
                    operand: Operand {
                        jump_addr: 0xFCFCFCFC,
                    },
                });

                checker.op(&[ValType::I32], &[])?;

                if let BlockType::TypeIdx(idx) = blocktype {
                    let ty = self
                        .types
                        .get(idx)
                        .ok_or(WasmParserError::InvalidTypeIdx(idx))?;
                    checker.op(&ty.0 .0, &[])?;
                    checker.enter_block(BlockKind::If, blocktype);
                    checker.op(&[], &ty.0 .0)?;
                } else {
                    checker.enter_block(BlockKind::If, blocktype);
                }
                jump_resolver.push(JumpResolverDSL::EnterForwardJumpBlock);

                let index = instrs.len() - 1;
                let mut else_addr = None;
                instrs.enter_block();
                let len2 = self.parse_instrs(
                    data_count_section,
                    instrs,
                    checker,
                    jump_resolver,
                    &mut else_addr,
                    stack_map_sites,
                    unwind_sites,
                )?;
                if !is_unreachable_if_block {
                    instrs[index].operand = Operand {
                        jump_addr: else_addr.unwrap_or_else(|| (instrs.len() - 1) as u32),
                    };
                }
                match blocktype {
                    BlockType::Void => {
                        if instrs.is_unreachable() {
                            checker.reset_stack()?;
                        }
                        instrs.leave_block();
                        checker.leave_block()?;
                    }
                    BlockType::TypeIdx(idx) => {
                        let ty = self
                            .types
                            .get(idx)
                            .ok_or(WasmParserError::InvalidTypeIdx(idx))?;
                        if else_addr.is_none() {
                            if instrs.is_unreachable() {
                                checker.reset_stack()?;
                            } else {
                                checker.op(&ty.1 .0, &[])?;
                            }
                            checker.leave_block()?;
                            checker.enter_block(BlockKind::If, blocktype);
                            checker.op(&[], &ty.0 .0)?;
                        }
                        if instrs.is_unreachable() {
                            checker.reset_stack()?;
                        } else {
                            checker.op(&ty.1 .0, &[])?;
                        }
                        instrs.leave_block();
                        checker.leave_block()?;
                        checker.op(&[], &ty.1 .0)?;
                    }
                    BlockType::ValType(ty) => {
                        if else_addr.is_none() {
                            if instrs.is_unreachable() {
                                checker.reset_stack()?;
                            } else {
                                checker.op(&[ty], &[])?;
                            }
                            checker.leave_block()?;
                            checker.enter_block(BlockKind::If, blocktype);
                        }
                        if instrs.is_unreachable() {
                            checker.reset_stack()?;
                        } else {
                            checker.op(&[ty], &[])?;
                        }
                        instrs.leave_block();
                        checker.leave_block()?;
                        checker.op(&[], &[ty])?;
                    }
                }

                (1 + len + len2, false)
            }
            0x05 => {
                let inst_unreachable = instrs.is_unreachable();
                trace!("parse_op_else: {inst_unreachable}");
                instrs.leave_block();
                instrs.enter_block();
                if !instrs.is_unreachable() {
                    instrs.push(Instr { op: vm::op_else });
                    jump_resolver.push(JumpResolverDSL::Br(0, instrs.len() as u32));
                    instrs.push(Instr {
                        operand: Operand {
                            jump_addr: 0xFFFB0000,
                        },
                    });
                    *else_addr = Some(instrs.len() as u32);
                    if let (BlockKind::If, blocktype, _block_base_stack_len) =
                        checker.current_block()?
                    {
                        let blocktype = *blocktype;
                        match blocktype {
                            BlockType::Void => {
                                if inst_unreachable {
                                    checker.reset_stack()?;
                                }
                                checker.leave_block()?;
                                checker.enter_block(BlockKind::If, blocktype);
                            }
                            BlockType::TypeIdx(idx) => {
                                let ty = self
                                    .types
                                    .get(idx)
                                    .ok_or(WasmParserError::InvalidTypeIdx(idx))?;
                                if inst_unreachable {
                                    checker.reset_stack()?;
                                } else {
                                    checker.op(&ty.1 .0, &[])?;
                                }
                                checker.leave_block()?;
                                checker.enter_block(BlockKind::If, blocktype);
                                checker.op(&[], &ty.0 .0)?;
                            }
                            BlockType::ValType(ty) => {
                                if inst_unreachable {
                                    checker.reset_stack()?;
                                } else {
                                    checker.op(&[ty], &[])?;
                                }
                                checker.leave_block()?;
                                checker.enter_block(BlockKind::If, blocktype);
                            }
                        }
                    } else {
                        Err(WasmParserError::InvalidStackValTypeAny)?
                    }
                }

                (1, false)
            }
            0x0B => {
                trace!("parse_op_end");
                jump_resolver.push(JumpResolverDSL::LeaveBlock(instrs.len() as u32));
                instrs.force_push(Instr { op: vm::op_end });

                (1, true)
            }

            0x0C => {
                let (len, idx) = self.parse_u32()?;
                trace!("parse_op_br: {idx}");
                if !instrs.is_unreachable() {
                    instrs.push(Instr { op: vm::op_br });
                    jump_resolver.push(JumpResolverDSL::Br(idx, instrs.len() as u32));
                    instrs.push(Instr {
                        operand: Operand {
                            u32: 0xFFFF0000 | idx,
                        },
                    });
                    instrs.set_unreachable();
                }
                let (kind, blocktype, _block_base_stack_len) = checker.get_block(idx as usize)?;

                match kind {
                    BlockKind::Block | BlockKind::If => match blocktype {
                        BlockType::Void => {
                            // ok
                        }
                        BlockType::ValType(ty) => {
                            checker.op(&[*ty], &[])?;
                        }
                        BlockType::TypeIdx(idx) => {
                            let ty = self
                                .types
                                .get(*idx)
                                .ok_or(WasmParserError::InvalidTypeIdx(*idx))?;
                            checker.op(&ty.1 .0, &[])?;
                        }
                    },
                    BlockKind::Loop => match blocktype {
                        BlockType::Void | BlockType::ValType(_) => {
                            // ok
                        }
                        BlockType::TypeIdx(idx) => {
                            let ty = self
                                .types
                                .get(*idx)
                                .ok_or(WasmParserError::InvalidTypeIdx(*idx))?;
                            checker.op(&ty.0 .0, &[])?;
                        }
                    },
                }
                checker.unreachable();

                (1 + len, false)
            }
            0x0D => {
                let (len, idx) = self.parse_u32()?;
                trace!("parse_op_br_if: {}", idx);
                checker.op(&[ValType::I32], &[])?;
                let (kind, blocktype, _base_stack_len) = checker.get_block(idx as usize)?;

                match kind {
                    BlockKind::Block | BlockKind::If => {
                        match blocktype {
                            BlockType::Void => {
                                // ok
                            }
                            BlockType::ValType(ty) => {
                                checker.op(&[*ty], &[*ty])?;
                            }
                            BlockType::TypeIdx(idx) => {
                                let ty = self
                                    .types
                                    .get(*idx)
                                    .ok_or(WasmParserError::InvalidTypeIdx(*idx))?;
                                checker.op_result_type(&ty.1, &ty.1)?;
                            }
                        }
                    }
                    BlockKind::Loop => {
                        // do nothing
                    }
                }
                if !instrs.is_unreachable() {
                    instrs.push(Instr { op: vm::op_br_if });
                    jump_resolver.push(JumpResolverDSL::Br(idx, instrs.len() as u32));
                    instrs.push(Instr {
                        operand: Operand {
                            u32: 0xFFFE0000 | idx,
                        },
                    });
                }
                (1 + len, false)
            }
            0x0E => {
                let (len, idxs) = self.parse_vec(Self::parse_u32)?;
                let (len2, default_idx) = self.parse_u32()?;
                trace!("parse_op_br_table: {idxs:?} {default_idx} {checker:?}");
                checker.op(&[ValType::I32], &[])?;
                trace!("parse_op_br_table: i32 poped");
                let result_len = validate_br_table_types(default_idx, self.types, checker)?;
                for idx in &idxs {
                    if result_len != validate_br_table_types(*idx, self.types, checker)? {
                        Err(WasmParserError::InvalidStackValTypeAny)?;
                    }
                }
                if !instrs.is_unreachable() {
                    instrs.push(Instr {
                        op: vm::op_br_table,
                    });
                    instrs.push(Instr {
                        operand: Operand {
                            u32: idxs.len() as u32,
                        },
                    });
                    for idx in &idxs {
                        jump_resolver.push(JumpResolverDSL::Br(*idx, instrs.len() as u32));
                        instrs.push(Instr {
                            operand: Operand {
                                u32: 0xFFFD0000 | *idx,
                            },
                        });
                    }
                    jump_resolver.push(JumpResolverDSL::Br(default_idx, instrs.len() as u32));
                    instrs.push(Instr {
                        operand: Operand {
                            u32: 0xFFFD0000 | default_idx,
                        },
                    });
                }
                checker.unreachable();
                instrs.set_unreachable();
                (1 + len + len2, false)
            }
            0x0F => {
                trace!("parse_op_return");
                if !instrs.is_unreachable() {
                    self.record_stack_map_site(
                        instrs,
                        checker,
                        stack_map_sites,
                        StackMapSafepointKind::Return,
                    );
                    self.record_unwind_site(
                        instrs,
                        unwind_sites,
                        StackMapSafepointKind::Return,
                        Some(0),
                    );
                    instrs.push(Instr { op: vm::op_return });
                    jump_resolver.push(JumpResolverDSL::Return(instrs.len() as u32));
                    instrs.push(Instr {
                        operand: Operand {
                            jump_addr: 0xFFFC0000,
                        },
                    });
                    instrs.set_unreachable();
                }
                checker.op(&self.functype.1 .0, &[])?;
                checker.unreachable();

                (1, false)
            }
            0x10 => {
                trace!("parse_op_call");
                let (len, idx) = self.parse_u32()?;
                let typeidx = self
                    .functions
                    .get(idx as usize)
                    .ok_or(WasmParserError::InvalidFuncIdx(FuncIdx(idx)))?;
                let ty = self
                    .types
                    .get(*typeidx)
                    .ok_or(WasmParserError::InvalidTypeIdx(TypeIdx(idx)))?;
                self.record_stack_map_site(
                    instrs,
                    checker,
                    stack_map_sites,
                    if self.is_local_direct_call_target(idx) {
                        StackMapSafepointKind::Call
                    } else {
                        StackMapSafepointKind::CallImport
                    },
                );
                checker.op_func_type(ty)?;

                instrs.push(Instr {
                    op: if self.is_local_direct_call_target(idx) {
                        vm::op_call
                    } else {
                        vm::op_call_import
                    },
                });
                instrs.push(Instr {
                    operand: Operand { u32: idx },
                });

                (1 + len, false)
            }
            0x11 => {
                trace!("parse_op_call_indirect");
                let (len, typeidx) = self.parse_u32()?;
                let (len2, tableidx) = self.parse_u32()?;
                if self.tables.len() <= tableidx as usize {
                    Err(WasmParserError::InvalidTableIndex(tableidx))?;
                }
                if self.tables[tableidx as usize].reftype != RefType::FuncRef {
                    Err(WasmParserError::InvalidTableType(tableidx))?;
                }
                self.record_stack_map_site(
                    instrs,
                    checker,
                    stack_map_sites,
                    StackMapSafepointKind::CallIndirect,
                );
                checker.op(&[ValType::I32], &[])?;
                let ty = self
                    .types
                    .get(TypeIdx(typeidx))
                    .ok_or(WasmParserError::InvalidTypeIdx(TypeIdx(typeidx)))?;
                checker.op_func_type(ty)?;
                instrs.push(Instr {
                    op: vm::op_call_indirect,
                });
                instrs.push(Instr {
                    operand: Operand { u32: tableidx },
                });
                instrs.push(Instr {
                    operand: Operand { u32: typeidx },
                });

                (1 + len + len2, false)
            }
            0x12 => {
                trace!("parse_op_return_call");
                let (len, idx) = self.parse_u32()?;
                let typeidx = self
                    .functions
                    .get(idx as usize)
                    .ok_or(WasmParserError::InvalidFuncIdx(FuncIdx(idx)))?;
                let ty = self
                    .types
                    .get(*typeidx)
                    .ok_or(WasmParserError::InvalidTypeIdx(TypeIdx(idx)))?;
                let kind = if self.is_local_direct_call_target(idx) {
                    StackMapSafepointKind::ReturnCall
                } else {
                    StackMapSafepointKind::ReturnCallImport
                };
                self.record_stack_map_site(instrs, checker, stack_map_sites, kind);
                self.record_unwind_site(instrs, unwind_sites, kind, Some(0));
                checker.op_func_type(ty)?;
                checker.op(&self.functype.1 .0, &[])?;
                checker.unreachable();

                instrs.push(Instr {
                    op: if self.is_local_direct_call_target(idx) {
                        vm::op_return_call
                    } else {
                        vm::op_return_call_import
                    },
                });
                instrs.push(Instr {
                    operand: Operand { u32: idx },
                });

                (1 + len, false)
            }
            0x13 => {
                trace!("parse_op_return_call_indirect");
                let (len, typeidx) = self.parse_u32()?;
                let (len2, tableidx) = self.parse_u32()?;
                if self.tables.len() <= tableidx as usize {
                    Err(WasmParserError::InvalidTableIndex(tableidx))?;
                }
                if self.tables[tableidx as usize].reftype != RefType::FuncRef {
                    Err(WasmParserError::InvalidTableType(tableidx))?;
                }
                self.record_stack_map_site(
                    instrs,
                    checker,
                    stack_map_sites,
                    StackMapSafepointKind::ReturnCallIndirect,
                );
                self.record_unwind_site(
                    instrs,
                    unwind_sites,
                    StackMapSafepointKind::ReturnCallIndirect,
                    Some(0),
                );
                checker.op(&[ValType::I32], &[])?;
                let ty = self
                    .types
                    .get(TypeIdx(typeidx))
                    .ok_or(WasmParserError::InvalidTypeIdx(TypeIdx(typeidx)))?;
                checker.op_func_type(ty)?;
                checker.op(&self.functype.1 .0, &[])?;
                checker.unreachable();
                instrs.push(Instr {
                    op: vm::op_return_call_indirect,
                });
                instrs.push(Instr {
                    operand: Operand { u32: tableidx },
                });
                instrs.push(Instr {
                    operand: Operand { u32: typeidx },
                });

                (1 + len + len2, false)
            }
            0x1A => {
                trace!("parse_op_drop");
                let x = checker.pop()?;

                if let MaybeUnreachable::Normal(x) = x {
                    instrs.push(Instr { op: vm::op_drop });
                    instrs.push(Instr {
                        operand: Operand {
                            drop_size: x.stack_size().u32(),
                        },
                    });
                } else {
                    // valid
                    // do nothing
                }

                (1, false)
            }
            0x1B => {
                trace!("parse_op_select");
                checker.op(&[ValType::I32], &[])?;

                let x = checker.pop()?;
                if let MaybeUnreachable::Normal(x) = x {
                    checker.op(&[x], &[x])?;
                    if matches!(x, ValType::ExternRef | ValType::FuncRef) {
                        Err(WasmParserError::InvalidStackValTypeAny)?
                    }
                    match x.stack_size() {
                        ValueSize::Byte4 => {
                            instrs.push(Instr { op: vm::op_select4 });
                        }
                        ValueSize::Byte8 => {
                            instrs.push(Instr { op: vm::op_select8 });
                        }
                        _ => {
                            instrs.push(Instr { op: vm::op_select });
                            instrs.push(Instr {
                                operand: Operand {
                                    select: x.stack_size().u32(),
                                },
                            });
                        }
                    }
                } else {
                    let x = checker.pop()?;
                    if matches!(
                        x,
                        MaybeUnreachable::Normal(ValType::ExternRef | ValType::FuncRef)
                    ) {
                        Err(WasmParserError::InvalidStackValTypeAny)?
                    }
                    if let MaybeUnreachable::Normal(x) = x {
                        match x.stack_size() {
                            ValueSize::Byte4 => {
                                instrs.push(Instr { op: vm::op_select4 });
                            }
                            ValueSize::Byte8 => {
                                instrs.push(Instr { op: vm::op_select8 });
                            }
                            _ => {
                                instrs.push(Instr { op: vm::op_select });
                                instrs.push(Instr {
                                    operand: Operand {
                                        select: x.stack_size().u32(),
                                    },
                                });
                            }
                        }

                        checker.push(x);
                    } else {
                        assert!(instrs.is_unreachable());
                        checker.push_any();
                    }
                }

                (1, false)
            }
            0x1C => {
                let (len, operand) = self.parse_vec(Self::parse_valtype)?;
                trace!("parse_op_select_with_param: {operand:?}");
                if operand.len() != 1 {
                    Err(WasmParserError::InvalidResultArity)?;
                }
                checker.op(&[ValType::I32], &[])?;
                checker.op(&operand, &[])?;
                checker.op(&operand, &operand)?;
                if !instrs.is_unreachable() {
                    let bytes = operand.iter().map(|v| v.stack_size().u32()).sum();
                    match bytes {
                        4 => {
                            instrs.push(Instr { op: vm::op_select4 });
                        }
                        8 => {
                            instrs.push(Instr { op: vm::op_select8 });
                        }
                        _ => {
                            instrs.push(Instr { op: vm::op_select });
                            instrs.push(Instr {
                                operand: Operand { select: bytes },
                            });
                        }
                    }
                }
                (1 + len, false)
            }
            0x20 => {
                let (len, idx) = self.parse_u32()?;
                trace!(
                    "parse_op_local_get: {:?} {:?} {idx}",
                    self.locals,
                    self.functype.0
                );
                let (ty, addr) = get_local_addr(&self.functype.0, self.locals, idx)?;

                match ty.stack_size() {
                    ValueSize::Byte4 => instrs.push(Instr {
                        op: vm::op_local_get4,
                    }),
                    ValueSize::Byte8 => instrs.push(Instr {
                        op: vm::op_local_get8,
                    }),
                    ValueSize::Byte16 => instrs.push(Instr {
                        op: vm::op_local_get16,
                    }),
                };
                instrs.push(Instr {
                    operand: Operand { local_addr: addr },
                });

                checker.op(&[], &[ty])?;

                (1 + len, false)
            }
            0x21 => {
                trace!("parse_op_local_set");
                let (len, idx) = self.parse_u32()?;
                let (ty, addr) = get_local_addr(&self.functype.0, self.locals, idx)?;

                match ty.stack_size() {
                    ValueSize::Byte4 => instrs.push(Instr {
                        op: vm::op_local_set4,
                    }),
                    ValueSize::Byte8 => instrs.push(Instr {
                        op: vm::op_local_set8,
                    }),
                    ValueSize::Byte16 => instrs.push(Instr {
                        op: vm::op_local_set16,
                    }),
                };
                instrs.push(Instr {
                    operand: Operand { local_addr: addr },
                });

                checker.op(&[ty], &[])?;

                (1 + len, false)
            }
            0x22 => {
                trace!("parse_op_local_tee");
                let (len, idx) = self.parse_u32()?;
                let (ty, addr) = get_local_addr(&self.functype.0, self.locals, idx)?;

                match ty.stack_size() {
                    ValueSize::Byte4 => instrs.push(Instr {
                        op: vm::op_local_tee4,
                    }),
                    ValueSize::Byte8 => instrs.push(Instr {
                        op: vm::op_local_tee8,
                    }),
                    ValueSize::Byte16 => instrs.push(Instr {
                        op: vm::op_local_tee16,
                    }),
                };
                instrs.push(Instr {
                    operand: Operand { local_addr: addr },
                });

                checker.op(&[ty], &[ty])?;

                (1 + len, false)
            }
            0x23 => {
                trace!("parse_op_global_get");

                let (len, idx) = self.parse_u32()?;
                let ty = self
                    .globals
                    .get(idx as usize)
                    .ok_or(WasmParserError::UnknownGlobal)?;
                checker.op(&[], &[ty.0])?;

                match ty.0.stack_size() {
                    ValueSize::Byte4 => instrs.push(Instr {
                        op: vm::op_global_get4,
                    }),
                    ValueSize::Byte8 => instrs.push(Instr {
                        op: vm::op_global_get8,
                    }),
                    ValueSize::Byte16 => instrs.push(Instr {
                        op: vm::op_global_get16,
                    }),
                };

                instrs.push(Instr {
                    operand: Operand { u32: idx },
                });

                (1 + len, false)
            }
            0x24 => {
                trace!("parse_op_global_set");
                let (len, idx) = self.parse_u32()?;
                let ty = self
                    .globals
                    .get(idx as usize)
                    .ok_or(WasmParserError::UnknownGlobal)?;
                if ty.1 != Mut::Var {
                    Err(WasmParserError::InvalidGlobalAccess)?
                }
                checker.op(&[ty.0], &[])?;

                match ty.0.stack_size() {
                    ValueSize::Byte4 => instrs.push(Instr {
                        op: vm::op_global_set4,
                    }),
                    ValueSize::Byte8 => instrs.push(Instr {
                        op: vm::op_global_set8,
                    }),
                    ValueSize::Byte16 => instrs.push(Instr {
                        op: vm::op_global_set16,
                    }),
                };

                instrs.push(Instr {
                    operand: Operand { u32: idx },
                });

                (1 + len, false)
            }
            0x25 => {
                trace!("parse_op_table_get");
                let (len, idx) = self.parse_u32()?;
                let ty = self
                    .tables
                    .get(idx as usize)
                    .ok_or(WasmParserError::InvalidTableIndex(idx))?;
                checker.op(&[ValType::I32], &[ty.reftype.into()])?;
                instrs.push(Instr {
                    op: vm::op_table_get,
                });
                instrs.push(Instr {
                    operand: Operand { u32: idx },
                });

                (1 + len, false)
            }
            0x26 => {
                trace!("parse_op_table_set");
                let (len, idx) = self.parse_u32()?;
                let ty = self
                    .tables
                    .get(idx as usize)
                    .ok_or(WasmParserError::InvalidTableIndex(idx))?;
                checker.op(&[ValType::I32, ty.reftype.into()], &[])?;
                instrs.push(Instr {
                    op: vm::op_table_set,
                });
                instrs.push(Instr {
                    operand: Operand { u32: idx },
                });

                (1 + len, false)
            }
            0x28 => {
                trace!("parse_op_i32_load");
                let (len, memidx, memarg) = self.parse_memarg(4)?;
                self.push_memarg_instruction(
                    instrs,
                    memidx,
                    memarg,
                    vm::op_i32_load_local,
                    vm::op_i32_load_shared,
                    vm::op_i32_load_indexed_local,
                    vm::op_i32_load_indexed_shared,
                )?;

                checker.load_op(ValType::I32)?;
                (1 + len, false)
            }
            0x29 => {
                trace!("parse_op_i64_load");
                let (len, memidx, memarg) = self.parse_memarg(8)?;
                self.push_memarg_instruction(
                    instrs,
                    memidx,
                    memarg,
                    vm::op_i64_load_local,
                    vm::op_i64_load_shared,
                    vm::op_i64_load_indexed_local,
                    vm::op_i64_load_indexed_shared,
                )?;

                checker.load_op(ValType::I64)?;
                (1 + len, false)
            }
            0x2A => {
                trace!("parse_op_f32_load");
                let (len, memidx, memarg) = self.parse_memarg(4)?;
                self.push_memarg_instruction(
                    instrs,
                    memidx,
                    memarg,
                    vm::op_f32_load_local,
                    vm::op_f32_load_shared,
                    vm::op_f32_load_indexed_local,
                    vm::op_f32_load_indexed_shared,
                )?;

                checker.load_op(ValType::F32)?;
                (1 + len, false)
            }
            0x2B => {
                trace!("parse_op_f64_load");
                let (len, memidx, memarg) = self.parse_memarg(8)?;
                self.push_memarg_instruction(
                    instrs,
                    memidx,
                    memarg,
                    vm::op_f64_load_local,
                    vm::op_f64_load_shared,
                    vm::op_f64_load_indexed_local,
                    vm::op_f64_load_indexed_shared,
                )?;

                checker.load_op(ValType::F64)?;

                (1 + len, false)
            }
            0x2C => {
                trace!("parse_op_i32_load8_s");
                let (len, memidx, memarg) = self.parse_memarg(1)?;
                self.push_memarg_instruction(
                    instrs,
                    memidx,
                    memarg,
                    vm::op_i32_load8_s_local,
                    vm::op_i32_load8_s_shared,
                    vm::op_i32_load8_s_indexed_local,
                    vm::op_i32_load8_s_indexed_shared,
                )?;

                checker.load_op(ValType::I32)?;

                (1 + len, false)
            }
            0x2D => {
                trace!("parse_op_i32_load8_u");
                let (len, memidx, memarg) = self.parse_memarg(1)?;
                self.push_memarg_instruction(
                    instrs,
                    memidx,
                    memarg,
                    vm::op_i32_load8_u_local,
                    vm::op_i32_load8_u_shared,
                    vm::op_i32_load8_u_indexed_local,
                    vm::op_i32_load8_u_indexed_shared,
                )?;

                checker.load_op(ValType::I32)?;

                (1 + len, false)
            }
            0x2E => {
                trace!("parse_op_i32_load16_s");
                let (len, memidx, memarg) = self.parse_memarg(2)?;
                self.push_memarg_instruction(
                    instrs,
                    memidx,
                    memarg,
                    vm::op_i32_load16_s_local,
                    vm::op_i32_load16_s_shared,
                    vm::op_i32_load16_s_indexed_local,
                    vm::op_i32_load16_s_indexed_shared,
                )?;

                checker.load_op(ValType::I32)?;
                (1 + len, false)
            }
            0x2F => {
                trace!("parse_op_i32_load16_u");
                let (len, memidx, memarg) = self.parse_memarg(2)?;
                self.push_memarg_instruction(
                    instrs,
                    memidx,
                    memarg,
                    vm::op_i32_load16_u_local,
                    vm::op_i32_load16_u_shared,
                    vm::op_i32_load16_u_indexed_local,
                    vm::op_i32_load16_u_indexed_shared,
                )?;

                checker.load_op(ValType::I32)?;
                (1 + len, false)
            }
            0x30 => {
                trace!("parse_op_i64_load8_s");
                let (len, memidx, memarg) = self.parse_memarg(1)?;
                self.push_memarg_instruction(
                    instrs,
                    memidx,
                    memarg,
                    vm::op_i64_load8_s_local,
                    vm::op_i64_load8_s_shared,
                    vm::op_i64_load8_s_indexed_local,
                    vm::op_i64_load8_s_indexed_shared,
                )?;

                checker.load_op(ValType::I64)?;
                (1 + len, false)
            }
            0x31 => {
                trace!("parse_op_i64_load8_u");
                let (len, memidx, memarg) = self.parse_memarg(1)?;
                self.push_memarg_instruction(
                    instrs,
                    memidx,
                    memarg,
                    vm::op_i64_load8_u_local,
                    vm::op_i64_load8_u_shared,
                    vm::op_i64_load8_u_indexed_local,
                    vm::op_i64_load8_u_indexed_shared,
                )?;

                checker.load_op(ValType::I64)?;
                (1 + len, false)
            }
            0x32 => {
                trace!("parse_op_i64_load16_s");
                let (len, memidx, memarg) = self.parse_memarg(2)?;
                self.push_memarg_instruction(
                    instrs,
                    memidx,
                    memarg,
                    vm::op_i64_load16_s_local,
                    vm::op_i64_load16_s_shared,
                    vm::op_i64_load16_s_indexed_local,
                    vm::op_i64_load16_s_indexed_shared,
                )?;

                checker.load_op(ValType::I64)?;
                (1 + len, false)
            }
            0x33 => {
                trace!("parse_op_i64_load16_u");
                let (len, memidx, memarg) = self.parse_memarg(2)?;
                self.push_memarg_instruction(
                    instrs,
                    memidx,
                    memarg,
                    vm::op_i64_load16_u_local,
                    vm::op_i64_load16_u_shared,
                    vm::op_i64_load16_u_indexed_local,
                    vm::op_i64_load16_u_indexed_shared,
                )?;

                checker.load_op(ValType::I64)?;
                (1 + len, false)
            }
            0x34 => {
                trace!("parse_op_i64_load32_s");
                let (len, memidx, memarg) = self.parse_memarg(4)?;
                self.push_memarg_instruction(
                    instrs,
                    memidx,
                    memarg,
                    vm::op_i64_load32_s_local,
                    vm::op_i64_load32_s_shared,
                    vm::op_i64_load32_s_indexed_local,
                    vm::op_i64_load32_s_indexed_shared,
                )?;

                checker.load_op(ValType::I64)?;

                (1 + len, false)
            }
            0x35 => {
                trace!("parse_op_i64_load32_u");
                let (len, memidx, memarg) = self.parse_memarg(4)?;
                self.push_memarg_instruction(
                    instrs,
                    memidx,
                    memarg,
                    vm::op_i64_load32_u_local,
                    vm::op_i64_load32_u_shared,
                    vm::op_i64_load32_u_indexed_local,
                    vm::op_i64_load32_u_indexed_shared,
                )?;

                checker.load_op(ValType::I64)?;

                (1 + len, false)
            }
            0x36 => {
                trace!("parse_op_i32_store");
                let (len, memidx, memarg) = self.parse_memarg(4)?;
                self.push_memarg_instruction(
                    instrs,
                    memidx,
                    memarg,
                    vm::op_i32_store_local,
                    vm::op_i32_store_shared,
                    vm::op_i32_store_indexed_local,
                    vm::op_i32_store_indexed_shared,
                )?;

                checker.store_op(ValType::I32)?;
                (1 + len, false)
            }
            0x37 => {
                trace!("parse_op_i64_store");
                let (len, memidx, memarg) = self.parse_memarg(8)?;
                self.push_memarg_instruction(
                    instrs,
                    memidx,
                    memarg,
                    vm::op_i64_store_local,
                    vm::op_i64_store_shared,
                    vm::op_i64_store_indexed_local,
                    vm::op_i64_store_indexed_shared,
                )?;

                checker.store_op(ValType::I64)?;

                (1 + len, false)
            }
            0x38 => {
                trace!("parse_op_f32_store");
                let (len, memidx, memarg) = self.parse_memarg(4)?;
                self.push_memarg_instruction(
                    instrs,
                    memidx,
                    memarg,
                    vm::op_f32_store_local,
                    vm::op_f32_store_shared,
                    vm::op_f32_store_indexed_local,
                    vm::op_f32_store_indexed_shared,
                )?;

                checker.store_op(ValType::F32)?;
                (1 + len, false)
            }
            0x39 => {
                trace!("parse_op_f64_store");
                let (len, memidx, memarg) = self.parse_memarg(8)?;
                self.push_memarg_instruction(
                    instrs,
                    memidx,
                    memarg,
                    vm::op_f64_store_local,
                    vm::op_f64_store_shared,
                    vm::op_f64_store_indexed_local,
                    vm::op_f64_store_indexed_shared,
                )?;

                checker.store_op(ValType::F64)?;

                (1 + len, false)
            }
            0x3A => {
                trace!("parse_op_i32_store8");
                let (len, memidx, memarg) = self.parse_memarg(1)?;
                self.push_memarg_instruction(
                    instrs,
                    memidx,
                    memarg,
                    vm::op_i32_store8_local,
                    vm::op_i32_store8_shared,
                    vm::op_i32_store8_indexed_local,
                    vm::op_i32_store8_indexed_shared,
                )?;

                checker.store_op(ValType::I32)?;

                (1 + len, false)
            }
            0x3B => {
                trace!("parse_op_i32_store16");
                let (len, memidx, memarg) = self.parse_memarg(2)?;
                self.push_memarg_instruction(
                    instrs,
                    memidx,
                    memarg,
                    vm::op_i32_store16_local,
                    vm::op_i32_store16_shared,
                    vm::op_i32_store16_indexed_local,
                    vm::op_i32_store16_indexed_shared,
                )?;

                checker.store_op(ValType::I32)?;
                (1 + len, false)
            }
            0x3C => {
                trace!("parse_op_i64_store8");
                let (len, memidx, memarg) = self.parse_memarg(1)?;
                self.push_memarg_instruction(
                    instrs,
                    memidx,
                    memarg,
                    vm::op_i64_store8_local,
                    vm::op_i64_store8_shared,
                    vm::op_i64_store8_indexed_local,
                    vm::op_i64_store8_indexed_shared,
                )?;

                checker.store_op(ValType::I64)?;
                (1 + len, false)
            }
            0x3D => {
                trace!("parse_op_i64_store16");
                let (len, memidx, memarg) = self.parse_memarg(2)?;
                self.push_memarg_instruction(
                    instrs,
                    memidx,
                    memarg,
                    vm::op_i64_store16_local,
                    vm::op_i64_store16_shared,
                    vm::op_i64_store16_indexed_local,
                    vm::op_i64_store16_indexed_shared,
                )?;

                checker.store_op(ValType::I64)?;

                (1 + len, false)
            }
            0x3E => {
                trace!("parse_op_i64_store32");
                let (len, memidx, memarg) = self.parse_memarg(4)?;
                self.push_memarg_instruction(
                    instrs,
                    memidx,
                    memarg,
                    vm::op_i64_store32_local,
                    vm::op_i64_store32_shared,
                    vm::op_i64_store32_indexed_local,
                    vm::op_i64_store32_indexed_shared,
                )?;

                checker.store_op(ValType::I64)?;
                (1 + len, false)
            }
            0x3F => {
                trace!("parse_op_mem_size");
                let (len, memidx) = self.parse_memory_index_immediate(0x3F)?;
                self.push_memidx_instruction(
                    instrs,
                    memidx,
                    vm::op_mem_size_local,
                    vm::op_mem_size_shared,
                    vm::op_mem_size_indexed_local,
                    vm::op_mem_size_indexed_shared,
                )?;

                checker.op(&[], &[ValType::I32])?;
                (1 + len, false)
            }
            0x40 => {
                trace!("parse_op_mem_grow");
                let (len, memidx) = self.parse_memory_index_immediate(0x40)?;
                self.push_memidx_instruction(
                    instrs,
                    memidx,
                    vm::op_mem_grow_local,
                    vm::op_mem_grow_shared,
                    vm::op_mem_grow_indexed_local,
                    vm::op_mem_grow_indexed_shared,
                )?;

                checker.op(&[ValType::I32], &[ValType::I32])?;
                (1 + len, false)
            }
            0x41 => {
                trace!("parse_op_i32_const");
                let (len, operand) = self.parse_i32()?;
                instrs.push(Instr {
                    op: vm::op_i32_const,
                });
                instrs.push(Instr {
                    operand: Operand { i32: operand },
                });

                checker.op(&[], &[ValType::I32])?;
                (1 + len, false)
            }
            0x42 => {
                trace!("parse_op_i64_const");
                let (len, operand) = self.parse_i64()?;
                instrs.push(Instr {
                    op: vm::op_i64_const,
                });
                instrs.push(Instr {
                    operand: Operand { i64: operand },
                });

                checker.op(&[], &[ValType::I64])?;
                (1 + len, false)
            }
            0x43 => {
                trace!("parse_op_f32_const");
                let (len, operand) = self.parse_f32()?;
                instrs.push(Instr {
                    op: vm::op_f32_const,
                });
                instrs.push(Instr {
                    operand: Operand { f32: operand },
                });

                checker.op(&[], &[ValType::F32])?;

                (1 + len, false)
            }
            0x44 => {
                trace!("parse_op_f64_const");
                let (len, operand) = self.parse_f64()?;
                instrs.push(Instr {
                    op: vm::op_f64_const,
                });
                instrs.push(Instr {
                    operand: Operand { f64: operand },
                });

                checker.op(&[], &[ValType::F64])?;

                (1 + len, false)
            }
            0x45 => {
                trace!("parse_op_i32_eqz");
                instrs.push(Instr { op: vm::op_i32_eqz });

                checker.op(&[ValType::I32], &[ValType::I32])?;
                (1, false)
            }
            0x46 => {
                trace!("parse_op_i32_eq");
                instrs.push(Instr { op: vm::op_i32_eq });

                checker.cond_op(ValType::I32)?;
                (1, false)
            }
            0x47 => {
                trace!("parse_op_i32_ne");
                instrs.push(Instr { op: vm::op_i32_ne });

                checker.cond_op(ValType::I32)?;
                (1, false)
            }
            0x48 => {
                trace!("parse_op_i32_lt_s");
                instrs.push(Instr {
                    op: vm::op_i32_lt_s,
                });

                checker.cond_op(ValType::I32)?;
                (1, false)
            }
            0x49 => {
                trace!("parse_op_i32_lt_u");
                instrs.push(Instr {
                    op: vm::op_i32_lt_u,
                });

                checker.cond_op(ValType::I32)?;
                (1, false)
            }
            0x4A => {
                trace!("parse_op_i32_gt_s");
                instrs.push(Instr {
                    op: vm::op_i32_gt_s,
                });

                checker.cond_op(ValType::I32)?;
                (1, false)
            }
            0x4B => {
                trace!("parse_op_i32_gt_u");
                instrs.push(Instr {
                    op: vm::op_i32_gt_u,
                });

                checker.cond_op(ValType::I32)?;
                (1, false)
            }
            0x4C => {
                trace!("parse_op_i32_le_s");
                instrs.push(Instr {
                    op: vm::op_i32_le_s,
                });

                checker.cond_op(ValType::I32)?;
                (1, false)
            }
            0x4D => {
                trace!("parse_op_i32_le_u");
                instrs.push(Instr {
                    op: vm::op_i32_le_u,
                });

                checker.cond_op(ValType::I32)?;
                (1, false)
            }
            0x4E => {
                trace!("parse_op_i32_ge_s");
                instrs.push(Instr {
                    op: vm::op_i32_ge_s,
                });

                checker.cond_op(ValType::I32)?;
                (1, false)
            }
            0x4F => {
                trace!("parse_op_i32_ge_u");
                instrs.push(Instr {
                    op: vm::op_i32_ge_u,
                });

                checker.cond_op(ValType::I32)?;
                (1, false)
            }
            0x50 => {
                trace!("parse_op_i64_eqz");
                checker.op(&[ValType::I64], &[ValType::I32])?;

                instrs.push(Instr { op: vm::op_i64_eqz });

                (1, false)
            }
            0x51 => {
                trace!("parse_op_i64_eq");
                instrs.push(Instr { op: vm::op_i64_eq });

                checker.cond_op(ValType::I64)?;
                (1, false)
            }
            0x52 => {
                trace!("parse_op_i64_ne");
                instrs.push(Instr { op: vm::op_i64_ne });

                checker.cond_op(ValType::I64)?;
                (1, false)
            }
            0x53 => {
                trace!("parse_op_i64_lt_s");
                instrs.push(Instr {
                    op: vm::op_i64_lt_s,
                });

                checker.cond_op(ValType::I64)?;
                (1, false)
            }
            0x54 => {
                trace!("parse_op_i64_lt_u");
                instrs.push(Instr {
                    op: vm::op_i64_lt_u,
                });

                checker.cond_op(ValType::I64)?;
                (1, false)
            }
            0x55 => {
                trace!("parse_op_i64_gt_s");
                instrs.push(Instr {
                    op: vm::op_i64_gt_s,
                });

                checker.cond_op(ValType::I64)?;
                (1, false)
            }
            0x56 => {
                trace!("parse_op_i64_gt_u");
                instrs.push(Instr {
                    op: vm::op_i64_gt_u,
                });

                checker.cond_op(ValType::I64)?;
                (1, false)
            }
            0x57 => {
                trace!("parse_op_i64_le_s");
                instrs.push(Instr {
                    op: vm::op_i64_le_s,
                });

                checker.cond_op(ValType::I64)?;
                (1, false)
            }
            0x58 => {
                trace!("parse_op_i64_le_u");
                instrs.push(Instr {
                    op: vm::op_i64_le_u,
                });

                checker.cond_op(ValType::I64)?;
                (1, false)
            }
            0x59 => {
                trace!("parse_op_i64_ge_s");
                instrs.push(Instr {
                    op: vm::op_i64_ge_s,
                });

                checker.cond_op(ValType::I64)?;
                (1, false)
            }
            0x5A => {
                trace!("parse_op_i64_ge_u");
                instrs.push(Instr {
                    op: vm::op_i64_ge_u,
                });

                checker.cond_op(ValType::I64)?;
                (1, false)
            }
            0x5B => {
                trace!("parse_op_f32_eq");
                instrs.push(Instr { op: vm::op_f32_eq });

                checker.cond_op(ValType::F32)?;
                (1, false)
            }
            0x5C => {
                trace!("parse_op_f32_ne");
                instrs.push(Instr { op: vm::op_f32_ne });

                checker.cond_op(ValType::F32)?;
                (1, false)
            }
            0x5D => {
                trace!("parse_op_f32_lt");
                instrs.push(Instr { op: vm::op_f32_lt });

                checker.cond_op(ValType::F32)?;
                (1, false)
            }
            0x5E => {
                trace!("parse_op_f32_gt");
                instrs.push(Instr { op: vm::op_f32_gt });

                checker.cond_op(ValType::F32)?;
                (1, false)
            }
            0x5F => {
                trace!("parse_op_f32_le");
                instrs.push(Instr { op: vm::op_f32_le });

                checker.cond_op(ValType::F32)?;
                (1, false)
            }
            0x60 => {
                trace!("parse_op_f32_ge");
                instrs.push(Instr { op: vm::op_f32_ge });

                checker.cond_op(ValType::F32)?;
                (1, false)
            }
            0x61 => {
                trace!("parse_op_f64_eq");
                instrs.push(Instr { op: vm::op_f64_eq });

                checker.cond_op(ValType::F64)?;
                (1, false)
            }
            0x62 => {
                trace!("parse_op_f64_ne");
                instrs.push(Instr { op: vm::op_f64_ne });

                checker.cond_op(ValType::F64)?;
                (1, false)
            }
            0x63 => {
                trace!("parse_op_f64_lt");
                instrs.push(Instr { op: vm::op_f64_lt });

                checker.cond_op(ValType::F64)?;
                (1, false)
            }
            0x64 => {
                trace!("parse_op_f64_gt");
                instrs.push(Instr { op: vm::op_f64_gt });

                checker.cond_op(ValType::F64)?;
                (1, false)
            }
            0x65 => {
                trace!("parse_op_f64_le");
                instrs.push(Instr { op: vm::op_f64_le });

                checker.cond_op(ValType::F64)?;
                (1, false)
            }
            0x66 => {
                trace!("parse_op_f64_ge");
                instrs.push(Instr { op: vm::op_f64_ge });

                checker.cond_op(ValType::F64)?;
                (1, false)
            }
            0x67 => {
                trace!("parse_op_i32_clz");
                instrs.push(Instr { op: vm::op_i32_clz });

                checker.binary_op(ValType::I32)?;

                (1, false)
            }
            0x68 => {
                trace!("parse_op_i32_ctz");
                instrs.push(Instr { op: vm::op_i32_ctz });

                checker.binary_op(ValType::I32)?;

                (1, false)
            }
            0x69 => {
                trace!("parse_op_i32_popcnt");
                instrs.push(Instr {
                    op: vm::op_i32_popcnt,
                });

                checker.binary_op(ValType::I32)?;
                (1, false)
            }
            0x6A => {
                trace!("parse_op_i32_add");
                instrs.push(Instr { op: vm::op_i32_add });

                checker.unary_op(ValType::I32)?;
                (1, false)
            }
            0x6B => {
                trace!("parse_op_i32_sub");
                instrs.push(Instr { op: vm::op_i32_sub });

                checker.unary_op(ValType::I32)?;
                (1, false)
            }
            0x6C => {
                trace!("parse_op_i32_mul");
                instrs.push(Instr { op: vm::op_i32_mul });
                checker.unary_op(ValType::I32)?;

                (1, false)
            }
            0x6D => {
                trace!("parse_op_i32_div_s");
                instrs.push(Instr {
                    op: vm::op_i32_div_s,
                });

                checker.unary_op(ValType::I32)?;
                (1, false)
            }
            0x6E => {
                trace!("parse_op_i32_div_u");
                instrs.push(Instr {
                    op: vm::op_i32_div_u,
                });

                checker.unary_op(ValType::I32)?;

                (1, false)
            }
            0x6F => {
                trace!("parse_op_i32_rem_s");
                instrs.push(Instr {
                    op: vm::op_i32_rem_s,
                });

                checker.unary_op(ValType::I32)?;

                (1, false)
            }
            0x70 => {
                trace!("parse_op_i32_rem_u");
                instrs.push(Instr {
                    op: vm::op_i32_rem_u,
                });

                checker.unary_op(ValType::I32)?;
                (1, false)
            }
            0x71 => {
                trace!("parse_op_i32_and");
                instrs.push(Instr { op: vm::op_i32_and });

                checker.unary_op(ValType::I32)?;

                (1, false)
            }
            0x72 => {
                trace!("parse_op_i32_or");
                instrs.push(Instr { op: vm::op_i32_or });

                checker.unary_op(ValType::I32)?;
                (1, false)
            }
            0x73 => {
                trace!("parse_op_i32_xor");
                instrs.push(Instr { op: vm::op_i32_xor });

                checker.unary_op(ValType::I32)?;
                (1, false)
            }
            0x74 => {
                trace!("parse_op_i32_shl");
                instrs.push(Instr { op: vm::op_i32_shl });

                checker.unary_op(ValType::I32)?;
                (1, false)
            }
            0x75 => {
                trace!("parse_op_i32_shr_s");
                instrs.push(Instr {
                    op: vm::op_i32_shr_s,
                });

                checker.unary_op(ValType::I32)?;
                (1, false)
            }
            0x76 => {
                trace!("parse_op_i32_shr_u");
                instrs.push(Instr {
                    op: vm::op_i32_shr_u,
                });

                checker.unary_op(ValType::I32)?;
                (1, false)
            }
            0x77 => {
                trace!("parse_op_i32_rotl");
                instrs.push(Instr {
                    op: vm::op_i32_rotl,
                });

                checker.unary_op(ValType::I32)?;
                (1, false)
            }
            0x78 => {
                trace!("parse_op_i32_rotr");
                instrs.push(Instr {
                    op: vm::op_i32_rotr,
                });

                checker.unary_op(ValType::I32)?;
                (1, false)
            }
            0x79 => {
                trace!("parse_op_i64_clz");
                instrs.push(Instr { op: vm::op_i64_clz });

                checker.binary_op(ValType::I64)?;
                (1, false)
            }
            0x7A => {
                trace!("parse_op_i64_ctz");
                instrs.push(Instr { op: vm::op_i64_ctz });

                checker.binary_op(ValType::I64)?;
                (1, false)
            }
            0x7B => {
                trace!("parse_op_i64_popcnt");
                instrs.push(Instr {
                    op: vm::op_i64_popcnt,
                });

                checker.binary_op(ValType::I64)?;
                (1, false)
            }
            0x7C => {
                trace!("parse_op_i64_add");
                instrs.push(Instr { op: vm::op_i64_add });

                checker.unary_op(ValType::I64)?;
                (1, false)
            }
            0x7D => {
                trace!("parse_op_i64_sub");
                instrs.push(Instr { op: vm::op_i64_sub });

                checker.unary_op(ValType::I64)?;

                (1, false)
            }
            0x7E => {
                trace!("parse_op_i64_mul");
                instrs.push(Instr { op: vm::op_i64_mul });

                checker.unary_op(ValType::I64)?;
                (1, false)
            }
            0x7F => {
                trace!("parse_op_i64_div_s");
                instrs.push(Instr {
                    op: vm::op_i64_div_s,
                });

                checker.unary_op(ValType::I64)?;
                (1, false)
            }
            0x80 => {
                trace!("parse_op_i64_div_u");
                instrs.push(Instr {
                    op: vm::op_i64_div_u,
                });

                checker.unary_op(ValType::I64)?;
                (1, false)
            }
            0x81 => {
                trace!("parse_op_i64_rem_s");
                instrs.push(Instr {
                    op: vm::op_i64_rem_s,
                });

                checker.unary_op(ValType::I64)?;
                (1, false)
            }
            0x82 => {
                trace!("parse_op_i64_rem_u");
                instrs.push(Instr {
                    op: vm::op_i64_rem_u,
                });

                checker.unary_op(ValType::I64)?;
                (1, false)
            }
            0x83 => {
                trace!("parse_op_i64_and");
                instrs.push(Instr { op: vm::op_i64_and });

                checker.unary_op(ValType::I64)?;
                (1, false)
            }
            0x84 => {
                trace!("parse_op_i64_or");
                instrs.push(Instr { op: vm::op_i64_or });

                checker.unary_op(ValType::I64)?;
                (1, false)
            }
            0x85 => {
                trace!("parse_op_i64_xor");
                instrs.push(Instr { op: vm::op_i64_xor });

                checker.unary_op(ValType::I64)?;
                (1, false)
            }
            0x86 => {
                trace!("parse_op64_shl");
                instrs.push(Instr { op: vm::op_i64_shl });

                checker.unary_op(ValType::I64)?;
                (1, false)
            }
            0x87 => {
                trace!("parse_op_i64_shr_s");
                instrs.push(Instr {
                    op: vm::op_i64_shr_s,
                });

                checker.unary_op(ValType::I64)?;
                (1, false)
            }
            0x88 => {
                trace!("parse_op_i64_shr_u");
                instrs.push(Instr {
                    op: vm::op_i64_shr_u,
                });

                checker.unary_op(ValType::I64)?;
                (1, false)
            }
            0x89 => {
                trace!("parse_op_i64_rotl");
                instrs.push(Instr {
                    op: vm::op_i64_rotl,
                });

                checker.unary_op(ValType::I64)?;
                (1, false)
            }
            0x8A => {
                trace!("parse_op_i64_rotr");
                instrs.push(Instr {
                    op: vm::op_i64_rotr,
                });

                checker.unary_op(ValType::I64)?;
                (1, false)
            }
            0x8B => {
                trace!("parse_op_f32_abs");
                instrs.push(Instr { op: vm::op_f32_abs });

                checker.binary_op(ValType::F32)?;
                (1, false)
            }
            0x8C => {
                trace!("parse_op_f32_neg");
                instrs.push(Instr { op: vm::op_f32_neg });

                checker.binary_op(ValType::F32)?;

                (1, false)
            }
            0x8D => {
                trace!("parse_op_f32_ceil");
                instrs.push(Instr {
                    op: vm::op_f32_ceil,
                });

                checker.binary_op(ValType::F32)?;
                (1, false)
            }
            0x8E => {
                trace!("parse_op_f32_floor");
                instrs.push(Instr {
                    op: vm::op_f32_floor,
                });

                checker.binary_op(ValType::F32)?;
                (1, false)
            }
            0x8F => {
                trace!("parse_op_f32_trunc");
                instrs.push(Instr {
                    op: vm::op_f32_trunc,
                });

                checker.binary_op(ValType::F32)?;
                (1, false)
            }
            0x90 => {
                trace!("parse_op_f32_nearest");
                instrs.push(Instr {
                    op: vm::op_f32_nearest,
                });

                checker.binary_op(ValType::F32)?;
                (1, false)
            }
            0x91 => {
                trace!("parse_op_f32_sqrt");
                instrs.push(Instr {
                    op: vm::op_f32_sqrt,
                });

                checker.binary_op(ValType::F32)?;
                (1, false)
            }
            0x92 => {
                trace!("parse_op_f32_add");
                instrs.push(Instr { op: vm::op_f32_add });

                checker.unary_op(ValType::F32)?;
                (1, false)
            }
            0x93 => {
                trace!("parse_op_f32_sub");
                instrs.push(Instr { op: vm::op_f32_sub });

                checker.unary_op(ValType::F32)?;
                (1, false)
            }
            0x94 => {
                trace!("parse_op_f32_mul");
                instrs.push(Instr { op: vm::op_f32_mul });

                checker.unary_op(ValType::F32)?;
                (1, false)
            }
            0x95 => {
                trace!("parse_op_f32_div");
                instrs.push(Instr { op: vm::op_f32_div });

                checker.unary_op(ValType::F32)?;
                (1, false)
            }
            0x96 => {
                trace!("parse_op_f32_min");
                instrs.push(Instr { op: vm::op_f32_min });

                checker.unary_op(ValType::F32)?;
                (1, false)
            }
            0x97 => {
                trace!("parse_op_f32_max");
                instrs.push(Instr { op: vm::op_f32_max });

                checker.unary_op(ValType::F32)?;
                (1, false)
            }
            0x98 => {
                trace!("parse_op_f32_copysign");
                instrs.push(Instr {
                    op: vm::op_f32_copysign,
                });

                checker.unary_op(ValType::F32)?;
                (1, false)
            }
            0x99 => {
                trace!("parse_op_f64_abs");
                instrs.push(Instr { op: vm::op_f64_abs });

                checker.binary_op(ValType::F64)?;
                (1, false)
            }
            0x9A => {
                trace!("parse_op_f64_neg");
                instrs.push(Instr { op: vm::op_f64_neg });

                checker.binary_op(ValType::F64)?;
                (1, false)
            }
            0x9B => {
                trace!("parse_op_f64_ceil");
                instrs.push(Instr {
                    op: vm::op_f64_ceil,
                });

                checker.binary_op(ValType::F64)?;
                (1, false)
            }
            0x9C => {
                trace!("parse_op_f64_floor");
                instrs.push(Instr {
                    op: vm::op_f64_floor,
                });

                checker.binary_op(ValType::F64)?;
                (1, false)
            }
            0x9D => {
                trace!("parse_op_f64_trunc");
                instrs.push(Instr {
                    op: vm::op_f64_trunc,
                });

                checker.binary_op(ValType::F64)?;
                (1, false)
            }
            0x9E => {
                trace!("parse_op_f64_nearest");
                instrs.push(Instr {
                    op: vm::op_f64_nearest,
                });

                checker.binary_op(ValType::F64)?;
                (1, false)
            }

            0x9F => {
                trace!("parse_op_f64_sqrt");
                instrs.push(Instr {
                    op: vm::op_f64_sqrt,
                });

                checker.binary_op(ValType::F64)?;
                (1, false)
            }
            0xA0 => {
                trace!("parse_op_f64_add");
                instrs.push(Instr { op: vm::op_f64_add });

                checker.unary_op(ValType::F64)?;
                (1, false)
            }
            0xA1 => {
                trace!("parse_op_f64_sub");
                instrs.push(Instr { op: vm::op_f64_sub });

                checker.unary_op(ValType::F64)?;
                (1, false)
            }
            0xA2 => {
                trace!("parse_op_f64_mul");
                instrs.push(Instr { op: vm::op_f64_mul });

                checker.unary_op(ValType::F64)?;
                (1, false)
            }
            0xA3 => {
                trace!("parse_op_f64_div");
                instrs.push(Instr { op: vm::op_f64_div });
                checker.unary_op(ValType::F64)?;
                (1, false)
            }
            0xA4 => {
                trace!("parse_op_f64_min");
                instrs.push(Instr { op: vm::op_f64_min });

                checker.unary_op(ValType::F64)?;
                (1, false)
            }
            0xA5 => {
                trace!("parse_op_f64_max");
                instrs.push(Instr { op: vm::op_f64_max });

                checker.unary_op(ValType::F64)?;
                (1, false)
            }
            0xA6 => {
                trace!("parse_op_f64_copysign");
                instrs.push(Instr {
                    op: vm::op_f64_copysign,
                });

                checker.unary_op(ValType::F64)?;
                (1, false)
            }
            0xA7 => {
                trace!("parse_op_i32_wrap_i64");
                instrs.push(Instr {
                    op: vm::op_i32_wrap_i64,
                });

                checker.op(&[ValType::I64], &[ValType::I32])?;
                (1, false)
            }
            0xA8 => {
                trace!("parse_op_i32_trunc_f32_s");
                instrs.push(Instr {
                    op: vm::op_i32_trunc_f32_s,
                });

                checker.op(&[ValType::F32], &[ValType::I32])?;
                (1, false)
            }
            0xA9 => {
                trace!("parse_op_i32_trunc_f32_u");
                instrs.push(Instr {
                    op: vm::op_i32_trunc_f32_u,
                });

                checker.op(&[ValType::F32], &[ValType::I32])?;
                (1, false)
            }
            0xAA => {
                trace!("parse_op_i32_trunc_f64_s");
                instrs.push(Instr {
                    op: vm::op_i32_trunc_f64_s,
                });

                checker.op(&[ValType::F64], &[ValType::I32])?;
                (1, false)
            }
            0xAB => {
                trace!("parse_op_i32_trunc_f64_u");
                instrs.push(Instr {
                    op: vm::op_i32_trunc_f64_u,
                });

                checker.op(&[ValType::F64], &[ValType::I32])?;
                (1, false)
            }
            0xAC => {
                trace!("parse_op_i64_extend_i32_s");
                instrs.push(Instr {
                    op: vm::op_i64_extend_i32_s,
                });

                checker.op(&[ValType::I32], &[ValType::I64])?;
                (1, false)
            }
            0xAD => {
                trace!("parse_op_i64_extend_i32_u");
                instrs.push(Instr {
                    op: vm::op_i64_extend_i32_u,
                });

                checker.op(&[ValType::I32], &[ValType::I64])?;

                (1, false)
            }
            0xAE => {
                trace!("parse_op_i64_trunc_f32_s");
                instrs.push(Instr {
                    op: vm::op_i64_trunc_f32_s,
                });

                checker.op(&[ValType::F32], &[ValType::I64])?;
                (1, false)
            }
            0xAF => {
                trace!("parse_op_i64_trunc_f32_u");
                instrs.push(Instr {
                    op: vm::op_i64_trunc_f32_u,
                });

                checker.op(&[ValType::F32], &[ValType::I64])?;
                (1, false)
            }
            0xB0 => {
                trace!("parse_op_i64_trunc_f64_s");
                instrs.push(Instr {
                    op: vm::op_i64_trunc_f64_s,
                });

                checker.op(&[ValType::F64], &[ValType::I64])?;
                (1, false)
            }
            0xB1 => {
                trace!("parse_op_i64_trunc_f64_u");
                instrs.push(Instr {
                    op: vm::op_i64_trunc_f64_u,
                });

                checker.op(&[ValType::F64], &[ValType::I64])?;
                (1, false)
            }
            0xB2 => {
                trace!("parse_op_f32_convert_i32_s");
                instrs.push(Instr {
                    op: vm::op_f32_convert_i32_s,
                });

                checker.op(&[ValType::I32], &[ValType::F32])?;
                (1, false)
            }
            0xB3 => {
                trace!("parse_op_f32_convert_i32_u");
                instrs.push(Instr {
                    op: vm::op_f32_convert_i32_u,
                });

                checker.op(&[ValType::I32], &[ValType::F32])?;
                (1, false)
            }
            0xB4 => {
                trace!("parse_op_f32_convert_i64_s");
                instrs.push(Instr {
                    op: vm::op_f32_convert_i64_s,
                });

                checker.op(&[ValType::I64], &[ValType::F32])?;
                (1, false)
            }
            0xB5 => {
                trace!("parse_op_f32_convert_i64_u");
                instrs.push(Instr {
                    op: vm::op_f32_convert_i64_u,
                });

                checker.op(&[ValType::I64], &[ValType::F32])?;
                (1, false)
            }
            0xB6 => {
                trace!("parse_op_f32_demote_f64");
                instrs.push(Instr {
                    op: vm::op_f32_demote_f64,
                });

                checker.op(&[ValType::F64], &[ValType::F32])?;
                (1, false)
            }
            0xB7 => {
                trace!("parse_op_f64_convert_i32_s");
                instrs.push(Instr {
                    op: vm::op_f64_convert_i32_s,
                });

                checker.op(&[ValType::I32], &[ValType::F64])?;
                (1, false)
            }
            0xB8 => {
                trace!("parse_op_f64_convert_i32_u");
                instrs.push(Instr {
                    op: vm::op_f64_convert_i32_u,
                });

                checker.op(&[ValType::I32], &[ValType::F64])?;
                (1, false)
            }
            0xB9 => {
                trace!("parse_op_f64_convert_i64_s");
                instrs.push(Instr {
                    op: vm::op_f64_convert_i64_s,
                });

                checker.op(&[ValType::I64], &[ValType::F64])?;
                (1, false)
            }
            0xBA => {
                trace!("parse_op_f64_convert_i64_u");
                instrs.push(Instr {
                    op: vm::op_f64_convert_i64_u,
                });

                checker.op(&[ValType::I64], &[ValType::F64])?;
                (1, false)
            }
            0xBB => {
                trace!("parse_op_f64_promote_f32");
                instrs.push(Instr {
                    op: vm::op_f64_promote_f32,
                });

                checker.op(&[ValType::F32], &[ValType::F64])?;
                (1, false)
            }
            0xBC => {
                trace!("parse_op_i32_reinterpret_f32");
                checker.op(&[ValType::F32], &[ValType::I32])?;
                (1, false)
            }
            0xBD => {
                trace!("parse_op_i64_reinterpret_f64");
                checker.op(&[ValType::F64], &[ValType::I64])?;
                (1, false)
            }
            0xBE => {
                trace!("parse_op_f32_reinterpret_i32");
                checker.op(&[ValType::I32], &[ValType::F32])?;
                (1, false)
            }
            0xBF => {
                trace!("parse_op_f64_reinterpret_i64");
                checker.op(&[ValType::I64], &[ValType::F64])?;
                (1, false)
            }
            0xFC => {
                let (len, next) = self.parse_u32()?;
                match next {
                    0 => {
                        trace!("parse_op_i32_trunc_sat_f32_s");
                        instrs.push(Instr {
                            op: vm::op_i32_trunc_sat_f32_s,
                        });

                        checker.op(&[ValType::F32], &[ValType::I32])?;
                        (1 + len, false)
                    }
                    1 => {
                        trace!("parse_op_i32_trunc_sat_f32_u");
                        instrs.push(Instr {
                            op: vm::op_i32_trunc_sat_f32_u,
                        });

                        checker.op(&[ValType::F32], &[ValType::I32])?;
                        (1 + len, false)
                    }
                    2 => {
                        trace!("parse_op_i32_trunc_sat_f64_s");
                        instrs.push(Instr {
                            op: vm::op_i32_trunc_sat_f64_s,
                        });

                        checker.op(&[ValType::F64], &[ValType::I32])?;
                        (1 + len, false)
                    }
                    3 => {
                        trace!("parse_op_i32_trunc_sat_f64_u");
                        instrs.push(Instr {
                            op: vm::op_i32_trunc_sat_f64_u,
                        });

                        checker.op(&[ValType::F64], &[ValType::I32])?;
                        (1 + len, false)
                    }
                    4 => {
                        trace!("parse_op_i64_trunc_sat_f32_s");
                        instrs.push(Instr {
                            op: vm::op_i64_trunc_sat_f32_s,
                        });

                        checker.op(&[ValType::F32], &[ValType::I64])?;
                        (1 + len, false)
                    }
                    5 => {
                        trace!("parse_op_i64_trunc_sat_f32_u");
                        instrs.push(Instr {
                            op: vm::op_i64_trunc_sat_f32_u,
                        });

                        checker.op(&[ValType::F32], &[ValType::I64])?;
                        (1 + len, false)
                    }
                    6 => {
                        trace!("parse_op_i64_trunc_sat_f64_s");
                        instrs.push(Instr {
                            op: vm::op_i64_trunc_sat_f64_s,
                        });

                        checker.op(&[ValType::F64], &[ValType::I64])?;
                        (1 + len, false)
                    }
                    7 => {
                        trace!("parse_op_i64_trunc_sat_f64_u");
                        instrs.push(Instr {
                            op: vm::op_i64_trunc_sat_f64_u,
                        });

                        checker.op(&[ValType::F64], &[ValType::I64])?;
                        (1 + len, false)
                    }
                    8 => {
                        let (len2, idx) = self.parse_u32()?;
                        let (len3, memidx) = self.parse_u32()?;
                        trace!("parse_op_mem_init");
                        self.memory_type(memidx)?;
                        assert_data_idx(idx, data_count_section)?;
                        if !matches!(data_count_section, DataCountVerifier::OnePass(_)) {
                            Err(WasmParserError::InvalidDataSectionCount)?
                        }
                        self.push_memidx_u32_instruction(
                            instrs,
                            idx,
                            memidx,
                            vm::op_mem_init_local,
                            vm::op_mem_init_shared,
                            vm::op_mem_init_indexed_local,
                            vm::op_mem_init_indexed_shared,
                        )?;

                        checker.op(&[ValType::I32, ValType::I32, ValType::I32], &[])?;
                        (1 + len + len2 + len3, false)
                    }
                    9 => {
                        let (len2, idx) = self.parse_u32()?;
                        trace!("parse_op_data_drop");
                        assert_data_idx(idx, data_count_section)?;
                        if !matches!(data_count_section, DataCountVerifier::OnePass(_)) {
                            Err(WasmParserError::InvalidDataSectionCount)?
                        }
                        instrs.push(Instr {
                            op: vm::op_data_drop,
                        });
                        instrs.push(Instr {
                            operand: Operand { u32: idx },
                        });

                        (1 + len + len2, false)
                    }
                    10 => {
                        let (len2, dst_memidx) = self.parse_u32()?;
                        let (len3, src_memidx) = self.parse_u32()?;
                        trace!("parse_op_mem_copy");
                        self.push_memory_copy_instruction(
                            instrs,
                            dst_memidx,
                            src_memidx,
                            vm::op_mem_copy_local,
                            vm::op_mem_copy_shared,
                            vm::op_mem_copy_indexed_local_local,
                            vm::op_mem_copy_indexed_local_shared,
                            vm::op_mem_copy_indexed_shared_local,
                            vm::op_mem_copy_indexed_shared_shared,
                        )?;

                        checker.op(&[ValType::I32, ValType::I32, ValType::I32], &[])?;
                        (1 + len + len2 + len3, false)
                    }
                    11 => {
                        let (len2, memidx) = self.parse_u32()?;
                        trace!("parse_op_mem_fill");
                        self.push_memidx_instruction(
                            instrs,
                            memidx,
                            vm::op_mem_fill_local,
                            vm::op_mem_fill_shared,
                            vm::op_mem_fill_indexed_local,
                            vm::op_mem_fill_indexed_shared,
                        )?;

                        checker.op(&[ValType::I32, ValType::I32, ValType::I32], &[])?;
                        (1 + len + len2, false)
                    }
                    12 => {
                        let (len2, elemidx) = self.parse_u32()?;
                        let (len3, tableidx) = self.parse_u32()?;
                        let elem = self
                            .elems
                            .get(elemidx as usize)
                            .ok_or(WasmParserError::UnknownElement(elemidx))?;

                        validate_active_elem(self.tables, tableidx, elem.kind)?;
                        trace!("parse_op_table_init");
                        instrs.push(Instr {
                            op: vm::op_table_init,
                        });
                        instrs.push(Instr {
                            operand: Operand { u32: elemidx },
                        });
                        instrs.push(Instr {
                            operand: Operand { u32: tableidx },
                        });

                        checker.op(&[ValType::I32, ValType::I32, ValType::I32], &[])?;
                        (1 + len + len2 + len3, false)
                    }
                    13 => {
                        let (len2, elemidx) = self.parse_u32()?;
                        if self.elems.get(elemidx as usize).is_none() {
                            Err(WasmParserError::UnknownElement(elemidx))?;
                        }
                        trace!("parse_op_elem_drop");
                        instrs.push(Instr {
                            op: vm::op_elem_drop,
                        });
                        instrs.push(Instr {
                            operand: Operand { u32: elemidx },
                        });

                        (1 + len + len2, false)
                    }
                    14 => {
                        let (len2, tableidx) = self.parse_u32()?;
                        let (len3, tableidx2) = self.parse_u32()?;
                        trace!("parse_op_table_copy");

                        let tt1 = self
                            .tables
                            .get(tableidx as usize)
                            .ok_or(WasmParserError::InvalidTableIndex(tableidx))?;
                        let tt2 = self
                            .tables
                            .get(tableidx2 as usize)
                            .ok_or(WasmParserError::InvalidTableIndex(tableidx2))?;
                        if tt1.reftype != tt2.reftype {
                            Err(WasmParserError::InvalidStackValTypeAny)?
                        }
                        instrs.push(Instr {
                            op: vm::op_table_copy,
                        });
                        instrs.push(Instr {
                            operand: Operand { u32: tableidx },
                        });
                        instrs.push(Instr {
                            operand: Operand { u32: tableidx2 },
                        });

                        checker.op(&[ValType::I32, ValType::I32, ValType::I32], &[])?;
                        (1 + len + len2 + len3, false)
                    }
                    15 => {
                        let (len2, tableidx) = self.parse_u32()?;
                        trace!("parse_op_table_grow");
                        let tt = self
                            .tables
                            .get(tableidx as usize)
                            .ok_or(WasmParserError::InvalidTableIndex(tableidx))?;
                        checker.op(&[tt.reftype.into(), ValType::I32], &[ValType::I32])?;
                        instrs.push(Instr {
                            op: vm::op_table_grow,
                        });
                        instrs.push(Instr {
                            operand: Operand { u32: tableidx },
                        });

                        (1 + len + len2, false)
                    }
                    16 => {
                        let (len2, tableidx) = self.parse_u32()?;
                        trace!("parse_op_table_size");
                        self.tables
                            .get(tableidx as usize)
                            .ok_or(WasmParserError::InvalidTableIndex(tableidx))?;
                        checker.op(&[], &[ValType::I32])?;
                        instrs.push(Instr {
                            op: vm::op_table_size,
                        });
                        instrs.push(Instr {
                            operand: Operand { u32: tableidx },
                        });

                        (1 + len + len2, false)
                    }
                    17 => {
                        let (len2, tableidx) = self.parse_u32()?;
                        trace!("parse_op_table_fill");
                        let tt = self
                            .tables
                            .get(tableidx as usize)
                            .ok_or(WasmParserError::InvalidTableIndex(tableidx))?;
                        checker.op(&[ValType::I32, tt.reftype.into(), ValType::I32], &[])?;
                        instrs.push(Instr {
                            op: vm::op_table_fill,
                        });
                        instrs.push(Instr {
                            operand: Operand { u32: tableidx },
                        });

                        (1 + len + len2, false)
                    }
                    _ => Err(WasmParserError::InvalidInstruction([
                        0xFC, next as u8, 0x00, 0x00,
                    ]))?,
                }
            }
            0xC0 => {
                trace!("parse_op_i32_extend8_s");
                instrs.push(Instr {
                    op: vm::op_i32_extend8_s,
                });

                checker.binary_op(ValType::I32)?;
                (1, false)
            }
            0xC1 => {
                trace!("parse_op_i32_extend16_s");
                instrs.push(Instr {
                    op: vm::op_i32_extend16_s,
                });

                checker.binary_op(ValType::I32)?;
                (1, false)
            }
            0xC2 => {
                trace!("parse_op_i64_extend8_s");
                instrs.push(Instr {
                    op: vm::op_i64_extend8_s,
                });

                checker.binary_op(ValType::I64)?;
                (1, false)
            }
            0xC3 => {
                trace!("parse_op_i64_extend16_s");
                instrs.push(Instr {
                    op: vm::op_i64_extend16_s,
                });

                checker.binary_op(ValType::I64)?;
                (1, false)
            }
            0xC4 => {
                trace!("parse_op_i64_extend32_s");
                instrs.push(Instr {
                    op: vm::op_i64_extend32_s,
                });

                checker.binary_op(ValType::I64)?;
                (1, false)
            }
            0xD0 => {
                trace!("parse_op_ref_null");
                let (len, t) = self.parse_reftype()?;

                instrs.push(Instr {
                    op: vm::op_ref_null,
                });

                checker.op(&[], &[t.into()])?;
                (1 + len, false)
            }
            0xD1 => {
                trace!("parse_op_ref_is_null");

                instrs.push(Instr {
                    op: vm::op_ref_is_null,
                });

                let v = checker.pop()?;
                if !matches!(
                    v,
                    MaybeUnreachable::Unreachable(_)
                        | MaybeUnreachable::Normal(ValType::ExternRef | ValType::FuncRef)
                ) {
                    Err(WasmParserError::InvalidStackValTypeAny)?;
                }
                checker.op(&[], &[ValType::I32])?;
                (1, false)
            }
            0xD2 => {
                trace!("parse_op_ref_func");

                let (len, idx) = self.parse_u32()?;
                if self.functions.get(idx as usize).is_none() || idx == self.funcidx.0 {
                    let mut found = false;
                    for elem in self.elems {
                        if !matches!(elem.kind, RefType::FuncRef)
                            || !matches!(elem.mode, ElemMode::Declarative)
                        {
                            continue;
                        }
                        match &elem.init {
                            ElemInit::FuncIdx(idxs) => {
                                if idxs.contains(&idx) {
                                    found = true;
                                    break;
                                }
                            }
                            ElemInit::ConstExpr(exprs) => {
                                if exprs.iter().flatten().any(
                                    |expr| matches!(expr, ConstExpr::FuncRef(funcidx) if *funcidx == idx),
                                ) {
                                    found = true;
                                    break;
                                }
                            }
                        }
                    }
                    if !found {
                        Err(WasmParserError::UndeclaredFunctionReference)?
                    }
                }

                instrs.push(Instr {
                    op: vm::op_ref_func,
                });
                instrs.push(Instr {
                    operand: Operand { u32: idx },
                });

                checker.op(&[], &[ValType::FuncRef])?;
                (1 + len, false)
            }
            0xFD => {
                #[cfg(feature = "simd")]
                {
                    let (len, idx) = self.parse_u32()?;
                    use super::simd_instruction::*;
                    let mut ctx = SimdParserContext {
                        mems: self.mems,
                        instrs,
                        reader: self.reader,
                        checker,
                    };
                    let len2 = simd_instruction!(
                        idx,
                        ctx,
                        i8x16_shuffle,
                        v128_load,
                        v128_const,
                        i8x16_splat,
                        i16x8_splat,
                        i32x4_splat,
                        i64x2_splat,
                        f32x4_splat,
                        f64x2_splat,
                        i8x16_swizzle,
                        i8x16_extract_lane_s,
                        i8x16_extract_lane_u,
                        i8x16_replace_lane,
                        i16x8_extract_lane_s,
                        i16x8_extract_lane_u,
                        i16x8_replace_lane,
                        i32x4_extract_lane,
                        i32x4_replace_lane,
                        i64x2_extract_lane,
                        i64x2_replace_lane,
                        f32x4_extract_lane,
                        f32x4_replace_lane,
                        f64x2_extract_lane,
                        f64x2_replace_lane,
                        v128_not,
                        v128_and,
                        v128_andnot,
                        v128_or,
                        v128_xor,
                        i8x16_all_true,
                        v128_bitselect,
                        i8x16_shl,
                        i8x16_shr,
                        u8x16_shr,
                        i8x16_add,
                        i8x16_sub,
                        i8x16_min,
                        u8x16_min,
                        i8x16_max,
                        u8x16_max,
                        f32x4_mul,
                        f32x4_abs,
                        i32x4_abs,
                        f32x4_min,
                        f32x4_div,
                        f32x4_max,
                        f32x4_pmin,
                        f32x4_pmax,
                        i32x4_trunc_sat_f32x4_s,
                        f32x4_convert_i32x4_u,
                        i32x4_add,
                        i64x2_add,
                        v128_store,
                        v128_load8x8_s,
                        v128_load8x8_u,
                        v128_load16x4_s,
                        v128_load16x4_u,
                        v128_load32x2_s,
                        v128_load32x2_u,
                        v128_load8_splat,
                        v128_load16_splat,
                        v128_load32_splat,
                        v128_load64_splat,
                        v128_load8_lane,
                        v128_load16_lane,
                        v128_load32_lane,
                        v128_load64_lane,
                        v128_store8_lane,
                        v128_store16_lane,
                        v128_store32_lane,
                        v128_store64_lane,
                        v128_load32_zero,
                        v128_load64_zero,
                        i16x8_shl,
                        i16x8_shr,
                        u16x8_shr,
                        i32x4_shl,
                        i32x4_shr,
                        u32x4_shr,
                        i64x2_shl,
                        i64x2_shr,
                        u64x2_shr,
                        v128_any_true,
                        i8x16_bitmask,
                        i16x8_all_true,
                        i16x8_bitmask,
                        i32x4_all_true,
                        i32x4_bitmask,
                        i64x2_all_true,
                        i64x2_bitmask,
                        f32x4_convert_i32x4_s,
                        f64x2_convert_low_i32x4_s,
                        f64x2_convert_low_i32x4_u,
                        i8x16_narrow_i16x8_s,
                        i8x16_narrow_i16x8_u,
                        i16x8_narrow_i32x4_s,
                        i16x8_narrow_i32x4_u,
                        f64x2_promote_low_f32x4,
                        f32x4_demote_f64x2_zero,
                        i32x4_sub,
                        i32x4_mul,
                        i16x8_extend_low_i8x16_s,
                        i16x8_extend_high_i8x16_s,
                        i16x8_extend_low_i8x16_u,
                        i16x8_extend_high_i8x16_u,
                        i32x4_extend_low_i16x8_s,
                        i32x4_extend_high_i16x8_s,
                        i32x4_extend_low_i16x8_u,
                        i32x4_extend_high_i16x8_u,
                        f32x4_add,
                        f32x4_sub,
                        f32x4_neg,
                        f32x4_sqrt,
                        f32x4_eq,
                        f32x4_ne,
                        f32x4_lt,
                        f32x4_gt,
                        f32x4_le,
                        f32x4_ge,
                        f32x4_ceil,
                        f32x4_floor,
                        f32x4_trunc,
                        f32x4_nearest,
                        f64x2_add,
                        f64x2_sub,
                        f64x2_mul,
                        f64x2_div,
                        f64x2_neg,
                        f64x2_abs,
                        f64x2_sqrt,
                        f64x2_min,
                        f64x2_max,
                        f64x2_pmin,
                        f64x2_pmax,
                        f64x2_ceil,
                        f64x2_floor,
                        f64x2_trunc,
                        f64x2_nearest,
                        f64x2_eq,
                        f64x2_ne,
                        f64x2_lt,
                        f64x2_gt,
                        f64x2_le,
                        f64x2_ge,
                        i8x16_add_sat,
                        u8x16_add_sat,
                        i8x16_sub_sat,
                        u8x16_sub_sat,
                        i8x16_neg,
                        u8x16_avgr,
                        i8x16_abs,
                        u8x16_popcnt,
                        i8x16_eq,
                        i8x16_ne,
                        i8x16_lt,
                        u8x16_lt,
                        i8x16_gt,
                        u8x16_gt,
                        i8x16_le,
                        u8x16_le,
                        i8x16_ge,
                        u8x16_ge,
                        i16x8_add_sat,
                        u16x8_add_sat,
                        i16x8_sub_sat,
                        u16x8_sub_sat,
                        i16x8_add,
                        i16x8_sub,
                        i16x8_mul,
                        i16x8_neg,
                        u16x8_avgr,
                        i16x8_min,
                        u16x8_min,
                        i16x8_max,
                        u16x8_max,
                        i16x8_abs,
                        i16x8_eq,
                        i16x8_ne,
                        i16x8_lt,
                        u16x8_lt,
                        i16x8_gt,
                        u16x8_gt,
                        i16x8_le,
                        u16x8_le,
                        i16x8_ge,
                        u16x8_ge,
                        i16x8_extadd_pairwise_i8x16,
                        u16x8_extadd_pairwise_i8x16,
                        i16x8_q15mulr_sat_s,
                        u16x8_extmul_low,
                        u16x8_extmul_high,
                        i16x8_extmul_low,
                        i16x8_extmul_high,
                        i32x4_neg,
                        i32x4_min,
                        u32x4_min,
                        i32x4_max,
                        u32x4_max,
                        i32x4_eq,
                        i32x4_ne,
                        i32x4_lt,
                        u32x4_lt,
                        i32x4_gt,
                        u32x4_gt,
                        i32x4_le,
                        u32x4_le,
                        i32x4_ge,
                        u32x4_ge,
                        i32x4_dot_i16x8,
                        i32x4_extadd_pairwise_i16x8,
                        u32x4_extadd_pairwise_i16x8,
                        i32x4_extmul_low,
                        i32x4_extmul_high,
                        u32x4_extmul_low,
                        u32x4_extmul_high,
                        i64x2_abs,
                        i64x2_neg,
                        i64x2_extend_low_i32x4_s,
                        i64x2_extend_high_i32x4_s,
                        i64x2_extend_low_i32x4_u,
                        i64x2_extend_high_i32x4_u,
                        i32x4_trunc_sat_f32x4_u,
                        i32x4_trunc_sat_f64x2_s,
                        i32x4_trunc_sat_f64x2_u,
                        i64x2_sub,
                        i64x2_mul,
                        i64x2_eq,
                        i64x2_ne,
                        i64x2_lt,
                        i64x2_gt,
                        i64x2_le,
                        i64x2_ge,
                        i64x2_extmul_low_i32x4_s,
                        i64x2_extmul_high_i32x4_s,
                        i64x2_extmul_low_i32x4_u,
                        i64x2_extmul_high_i32x4_u
                    );
                    (1 + len + len2, false)
                }
                #[cfg(not(feature = "simd"))]
                {
                    return Err(WasmParserError::unsupported_feature(
                        super::ProposalFeature::Simd,
                        [0xFD, 0, 0, 0],
                    ));
                }
            }
            0xFE => {
                #[cfg(not(feature = "threads"))]
                {
                    return Err(WasmParserError::unsupported_feature(
                        super::ProposalFeature::Threads,
                        [0xFE, 0, 0, 0],
                    ));
                }
                #[cfg(feature = "threads")]
                {
                    let (len, next) = self.parse_u32()?;
                    macro_rules! atomic_load {
                        ($align:expr, $local:path, $shared:path, $indexed_local:path, $indexed_shared:path, $ty:expr) => {{
                            let (len2, memidx, memarg) = self.parse_atomic_memarg($align)?;
                            self.push_memarg_instruction(
                                instrs,
                                memidx,
                                memarg,
                                $local,
                                $shared,
                                $indexed_local,
                                $indexed_shared,
                            )?;
                            checker.load_op($ty)?;
                            (1 + len + len2, false)
                        }};
                    }
                    macro_rules! atomic_store {
                        ($align:expr, $local:path, $shared:path, $indexed_local:path, $indexed_shared:path, $ty:expr) => {{
                            let (len2, memidx, memarg) = self.parse_atomic_memarg($align)?;
                            self.push_memarg_instruction(
                                instrs,
                                memidx,
                                memarg,
                                $local,
                                $shared,
                                $indexed_local,
                                $indexed_shared,
                            )?;
                            checker.store_op($ty)?;
                            (1 + len + len2, false)
                        }};
                    }
                    macro_rules! atomic_rmw {
                        ($align:expr, $local:path, $shared:path, $indexed_local:path, $indexed_shared:path, $value_ty:expr, $result_ty:expr) => {{
                            let (len2, memidx, memarg) = self.parse_atomic_memarg($align)?;
                            self.push_memarg_instruction(
                                instrs,
                                memidx,
                                memarg,
                                $local,
                                $shared,
                                $indexed_local,
                                $indexed_shared,
                            )?;
                            checker.op(&[ValType::I32, $value_ty], &[$result_ty])?;
                            (1 + len + len2, false)
                        }};
                    }
                    macro_rules! atomic_cmpxchg {
                        ($align:expr, $local:path, $shared:path, $indexed_local:path, $indexed_shared:path, $value_ty:expr, $result_ty:expr) => {{
                            let (len2, memidx, memarg) = self.parse_atomic_memarg($align)?;
                            self.push_memarg_instruction(
                                instrs,
                                memidx,
                                memarg,
                                $local,
                                $shared,
                                $indexed_local,
                                $indexed_shared,
                            )?;
                            checker.op(&[ValType::I32, $value_ty, $value_ty], &[$result_ty])?;
                            (1 + len + len2, false)
                        }};
                    }
                    match next {
                        0x00 => {
                            let (len2, memidx, memarg) = self.parse_atomic_memarg(2)?;
                            self.push_memarg_instruction(
                                instrs,
                                memidx,
                                memarg,
                                vm::op_memory_atomic_notify_unshared,
                                vm::op_memory_atomic_notify_shared,
                                vm::op_memory_atomic_notify_indexed_unshared,
                                vm::op_memory_atomic_notify_indexed_shared,
                            )?;
                            checker.op(&[ValType::I32, ValType::I32], &[ValType::I32])?;
                            (1 + len + len2, false)
                        }
                        0x01 => {
                            let (len2, memidx, memarg) = self.parse_atomic_memarg(2)?;
                            self.record_stack_map_site(
                                instrs,
                                checker,
                                stack_map_sites,
                                StackMapSafepointKind::MemoryWait,
                            );
                            self.record_unwind_site(
                                instrs,
                                unwind_sites,
                                StackMapSafepointKind::MemoryWait,
                                None,
                            );
                            self.push_memarg_instruction(
                                instrs,
                                memidx,
                                memarg,
                                vm::op_memory_atomic_wait32_unshared,
                                vm::op_memory_atomic_wait32_shared,
                                vm::op_memory_atomic_wait32_indexed_unshared,
                                vm::op_memory_atomic_wait32_indexed_shared,
                            )?;
                            checker
                                .op(&[ValType::I32, ValType::I32, ValType::I64], &[ValType::I32])?;
                            (1 + len + len2, false)
                        }
                        0x02 => {
                            let (len2, memidx, memarg) = self.parse_atomic_memarg(3)?;
                            self.record_stack_map_site(
                                instrs,
                                checker,
                                stack_map_sites,
                                StackMapSafepointKind::MemoryWait,
                            );
                            self.record_unwind_site(
                                instrs,
                                unwind_sites,
                                StackMapSafepointKind::MemoryWait,
                                None,
                            );
                            self.push_memarg_instruction(
                                instrs,
                                memidx,
                                memarg,
                                vm::op_memory_atomic_wait64_unshared,
                                vm::op_memory_atomic_wait64_shared,
                                vm::op_memory_atomic_wait64_indexed_unshared,
                                vm::op_memory_atomic_wait64_indexed_shared,
                            )?;
                            checker
                                .op(&[ValType::I32, ValType::I64, ValType::I64], &[ValType::I32])?;
                            (1 + len + len2, false)
                        }
                        0x03 => {
                            let reserved = self.reader.read_exact_one()?;
                            if reserved != 0 {
                                Err(WasmParserError::InvalidInstruction([
                                    0xFE, 0x03, reserved, 0x00,
                                ]))?;
                            }
                            assert_memory(self.mems)?;
                            instrs.push(Instr {
                                op: if default_memory_is_shared(self.mems) {
                                    vm::op_atomic_fence_shared
                                } else {
                                    vm::op_atomic_fence_local
                                },
                            });
                            instrs.push(Instr {
                                operand: Operand { u32: 0 },
                            });
                            checker.op(&[], &[])?;
                            (2 + len, false)
                        }
                        0x10 => atomic_load!(
                            2,
                            vm::op_i32_atomic_load_local,
                            vm::op_i32_atomic_load_shared,
                            vm::op_i32_atomic_load_indexed_local,
                            vm::op_i32_atomic_load_indexed_shared,
                            ValType::I32
                        ),
                        0x11 => atomic_load!(
                            3,
                            vm::op_i64_atomic_load_local,
                            vm::op_i64_atomic_load_shared,
                            vm::op_i64_atomic_load_indexed_local,
                            vm::op_i64_atomic_load_indexed_shared,
                            ValType::I64
                        ),
                        0x12 => atomic_load!(
                            0,
                            vm::op_i32_atomic_load8_u_local,
                            vm::op_i32_atomic_load8_u_shared,
                            vm::op_i32_atomic_load8_u_indexed_local,
                            vm::op_i32_atomic_load8_u_indexed_shared,
                            ValType::I32
                        ),
                        0x13 => atomic_load!(
                            1,
                            vm::op_i32_atomic_load16_u_local,
                            vm::op_i32_atomic_load16_u_shared,
                            vm::op_i32_atomic_load16_u_indexed_local,
                            vm::op_i32_atomic_load16_u_indexed_shared,
                            ValType::I32
                        ),
                        0x14 => atomic_load!(
                            0,
                            vm::op_i64_atomic_load8_u_local,
                            vm::op_i64_atomic_load8_u_shared,
                            vm::op_i64_atomic_load8_u_indexed_local,
                            vm::op_i64_atomic_load8_u_indexed_shared,
                            ValType::I64
                        ),
                        0x15 => atomic_load!(
                            1,
                            vm::op_i64_atomic_load16_u_local,
                            vm::op_i64_atomic_load16_u_shared,
                            vm::op_i64_atomic_load16_u_indexed_local,
                            vm::op_i64_atomic_load16_u_indexed_shared,
                            ValType::I64
                        ),
                        0x16 => atomic_load!(
                            2,
                            vm::op_i64_atomic_load32_u_local,
                            vm::op_i64_atomic_load32_u_shared,
                            vm::op_i64_atomic_load32_u_indexed_local,
                            vm::op_i64_atomic_load32_u_indexed_shared,
                            ValType::I64
                        ),
                        0x17 => atomic_store!(
                            2,
                            vm::op_i32_atomic_store_local,
                            vm::op_i32_atomic_store_shared,
                            vm::op_i32_atomic_store_indexed_local,
                            vm::op_i32_atomic_store_indexed_shared,
                            ValType::I32
                        ),
                        0x18 => atomic_store!(
                            3,
                            vm::op_i64_atomic_store_local,
                            vm::op_i64_atomic_store_shared,
                            vm::op_i64_atomic_store_indexed_local,
                            vm::op_i64_atomic_store_indexed_shared,
                            ValType::I64
                        ),
                        0x19 => atomic_store!(
                            0,
                            vm::op_i32_atomic_store8_local,
                            vm::op_i32_atomic_store8_shared,
                            vm::op_i32_atomic_store8_indexed_local,
                            vm::op_i32_atomic_store8_indexed_shared,
                            ValType::I32
                        ),
                        0x1A => atomic_store!(
                            1,
                            vm::op_i32_atomic_store16_local,
                            vm::op_i32_atomic_store16_shared,
                            vm::op_i32_atomic_store16_indexed_local,
                            vm::op_i32_atomic_store16_indexed_shared,
                            ValType::I32
                        ),
                        0x1B => atomic_store!(
                            0,
                            vm::op_i64_atomic_store8_local,
                            vm::op_i64_atomic_store8_shared,
                            vm::op_i64_atomic_store8_indexed_local,
                            vm::op_i64_atomic_store8_indexed_shared,
                            ValType::I64
                        ),
                        0x1C => atomic_store!(
                            1,
                            vm::op_i64_atomic_store16_local,
                            vm::op_i64_atomic_store16_shared,
                            vm::op_i64_atomic_store16_indexed_local,
                            vm::op_i64_atomic_store16_indexed_shared,
                            ValType::I64
                        ),
                        0x1D => atomic_store!(
                            2,
                            vm::op_i64_atomic_store32_local,
                            vm::op_i64_atomic_store32_shared,
                            vm::op_i64_atomic_store32_indexed_local,
                            vm::op_i64_atomic_store32_indexed_shared,
                            ValType::I64
                        ),
                        0x1E => atomic_rmw!(
                            2,
                            vm::op_i32_atomic_rmw_add,
                            vm::op_i32_atomic_rmw_add_shared,
                            vm::op_i32_atomic_rmw_add_indexed_local,
                            vm::op_i32_atomic_rmw_add_indexed_shared,
                            ValType::I32,
                            ValType::I32
                        ),
                        0x1F => atomic_rmw!(
                            3,
                            vm::op_i64_atomic_rmw_add,
                            vm::op_i64_atomic_rmw_add_shared,
                            vm::op_i64_atomic_rmw_add_indexed_local,
                            vm::op_i64_atomic_rmw_add_indexed_shared,
                            ValType::I64,
                            ValType::I64
                        ),
                        0x20 => {
                            atomic_rmw!(
                                0,
                                vm::op_i32_atomic_rmw8_add_u,
                                vm::op_i32_atomic_rmw8_add_u_shared,
                                vm::op_i32_atomic_rmw8_add_u_indexed_local,
                                vm::op_i32_atomic_rmw8_add_u_indexed_shared,
                                ValType::I32,
                                ValType::I32
                            )
                        }
                        0x21 => {
                            atomic_rmw!(
                                1,
                                vm::op_i32_atomic_rmw16_add_u,
                                vm::op_i32_atomic_rmw16_add_u_shared,
                                vm::op_i32_atomic_rmw16_add_u_indexed_local,
                                vm::op_i32_atomic_rmw16_add_u_indexed_shared,
                                ValType::I32,
                                ValType::I32
                            )
                        }
                        0x22 => {
                            atomic_rmw!(
                                0,
                                vm::op_i64_atomic_rmw8_add_u,
                                vm::op_i64_atomic_rmw8_add_u_shared,
                                vm::op_i64_atomic_rmw8_add_u_indexed_local,
                                vm::op_i64_atomic_rmw8_add_u_indexed_shared,
                                ValType::I64,
                                ValType::I64
                            )
                        }
                        0x23 => {
                            atomic_rmw!(
                                1,
                                vm::op_i64_atomic_rmw16_add_u,
                                vm::op_i64_atomic_rmw16_add_u_shared,
                                vm::op_i64_atomic_rmw16_add_u_indexed_local,
                                vm::op_i64_atomic_rmw16_add_u_indexed_shared,
                                ValType::I64,
                                ValType::I64
                            )
                        }
                        0x24 => {
                            atomic_rmw!(
                                2,
                                vm::op_i64_atomic_rmw32_add_u,
                                vm::op_i64_atomic_rmw32_add_u_shared,
                                vm::op_i64_atomic_rmw32_add_u_indexed_local,
                                vm::op_i64_atomic_rmw32_add_u_indexed_shared,
                                ValType::I64,
                                ValType::I64
                            )
                        }
                        0x25 => atomic_rmw!(
                            2,
                            vm::op_i32_atomic_rmw_sub,
                            vm::op_i32_atomic_rmw_sub_shared,
                            vm::op_i32_atomic_rmw_sub_indexed_local,
                            vm::op_i32_atomic_rmw_sub_indexed_shared,
                            ValType::I32,
                            ValType::I32
                        ),
                        0x26 => atomic_rmw!(
                            3,
                            vm::op_i64_atomic_rmw_sub,
                            vm::op_i64_atomic_rmw_sub_shared,
                            vm::op_i64_atomic_rmw_sub_indexed_local,
                            vm::op_i64_atomic_rmw_sub_indexed_shared,
                            ValType::I64,
                            ValType::I64
                        ),
                        0x27 => {
                            atomic_rmw!(
                                0,
                                vm::op_i32_atomic_rmw8_sub_u,
                                vm::op_i32_atomic_rmw8_sub_u_shared,
                                vm::op_i32_atomic_rmw8_sub_u_indexed_local,
                                vm::op_i32_atomic_rmw8_sub_u_indexed_shared,
                                ValType::I32,
                                ValType::I32
                            )
                        }
                        0x28 => {
                            atomic_rmw!(
                                1,
                                vm::op_i32_atomic_rmw16_sub_u,
                                vm::op_i32_atomic_rmw16_sub_u_shared,
                                vm::op_i32_atomic_rmw16_sub_u_indexed_local,
                                vm::op_i32_atomic_rmw16_sub_u_indexed_shared,
                                ValType::I32,
                                ValType::I32
                            )
                        }
                        0x29 => {
                            atomic_rmw!(
                                0,
                                vm::op_i64_atomic_rmw8_sub_u,
                                vm::op_i64_atomic_rmw8_sub_u_shared,
                                vm::op_i64_atomic_rmw8_sub_u_indexed_local,
                                vm::op_i64_atomic_rmw8_sub_u_indexed_shared,
                                ValType::I64,
                                ValType::I64
                            )
                        }
                        0x2A => {
                            atomic_rmw!(
                                1,
                                vm::op_i64_atomic_rmw16_sub_u,
                                vm::op_i64_atomic_rmw16_sub_u_shared,
                                vm::op_i64_atomic_rmw16_sub_u_indexed_local,
                                vm::op_i64_atomic_rmw16_sub_u_indexed_shared,
                                ValType::I64,
                                ValType::I64
                            )
                        }
                        0x2B => {
                            atomic_rmw!(
                                2,
                                vm::op_i64_atomic_rmw32_sub_u,
                                vm::op_i64_atomic_rmw32_sub_u_shared,
                                vm::op_i64_atomic_rmw32_sub_u_indexed_local,
                                vm::op_i64_atomic_rmw32_sub_u_indexed_shared,
                                ValType::I64,
                                ValType::I64
                            )
                        }
                        0x2C => atomic_rmw!(
                            2,
                            vm::op_i32_atomic_rmw_and,
                            vm::op_i32_atomic_rmw_and_shared,
                            vm::op_i32_atomic_rmw_and_indexed_local,
                            vm::op_i32_atomic_rmw_and_indexed_shared,
                            ValType::I32,
                            ValType::I32
                        ),
                        0x2D => atomic_rmw!(
                            3,
                            vm::op_i64_atomic_rmw_and,
                            vm::op_i64_atomic_rmw_and_shared,
                            vm::op_i64_atomic_rmw_and_indexed_local,
                            vm::op_i64_atomic_rmw_and_indexed_shared,
                            ValType::I64,
                            ValType::I64
                        ),
                        0x2E => {
                            atomic_rmw!(
                                0,
                                vm::op_i32_atomic_rmw8_and_u,
                                vm::op_i32_atomic_rmw8_and_u_shared,
                                vm::op_i32_atomic_rmw8_and_u_indexed_local,
                                vm::op_i32_atomic_rmw8_and_u_indexed_shared,
                                ValType::I32,
                                ValType::I32
                            )
                        }
                        0x2F => {
                            atomic_rmw!(
                                1,
                                vm::op_i32_atomic_rmw16_and_u,
                                vm::op_i32_atomic_rmw16_and_u_shared,
                                vm::op_i32_atomic_rmw16_and_u_indexed_local,
                                vm::op_i32_atomic_rmw16_and_u_indexed_shared,
                                ValType::I32,
                                ValType::I32
                            )
                        }
                        0x30 => {
                            atomic_rmw!(
                                0,
                                vm::op_i64_atomic_rmw8_and_u,
                                vm::op_i64_atomic_rmw8_and_u_shared,
                                vm::op_i64_atomic_rmw8_and_u_indexed_local,
                                vm::op_i64_atomic_rmw8_and_u_indexed_shared,
                                ValType::I64,
                                ValType::I64
                            )
                        }
                        0x31 => {
                            atomic_rmw!(
                                1,
                                vm::op_i64_atomic_rmw16_and_u,
                                vm::op_i64_atomic_rmw16_and_u_shared,
                                vm::op_i64_atomic_rmw16_and_u_indexed_local,
                                vm::op_i64_atomic_rmw16_and_u_indexed_shared,
                                ValType::I64,
                                ValType::I64
                            )
                        }
                        0x32 => {
                            atomic_rmw!(
                                2,
                                vm::op_i64_atomic_rmw32_and_u,
                                vm::op_i64_atomic_rmw32_and_u_shared,
                                vm::op_i64_atomic_rmw32_and_u_indexed_local,
                                vm::op_i64_atomic_rmw32_and_u_indexed_shared,
                                ValType::I64,
                                ValType::I64
                            )
                        }
                        0x33 => atomic_rmw!(
                            2,
                            vm::op_i32_atomic_rmw_or,
                            vm::op_i32_atomic_rmw_or_shared,
                            vm::op_i32_atomic_rmw_or_indexed_local,
                            vm::op_i32_atomic_rmw_or_indexed_shared,
                            ValType::I32,
                            ValType::I32
                        ),
                        0x34 => atomic_rmw!(
                            3,
                            vm::op_i64_atomic_rmw_or,
                            vm::op_i64_atomic_rmw_or_shared,
                            vm::op_i64_atomic_rmw_or_indexed_local,
                            vm::op_i64_atomic_rmw_or_indexed_shared,
                            ValType::I64,
                            ValType::I64
                        ),
                        0x35 => atomic_rmw!(
                            0,
                            vm::op_i32_atomic_rmw8_or_u,
                            vm::op_i32_atomic_rmw8_or_u_shared,
                            vm::op_i32_atomic_rmw8_or_u_indexed_local,
                            vm::op_i32_atomic_rmw8_or_u_indexed_shared,
                            ValType::I32,
                            ValType::I32
                        ),
                        0x36 => {
                            atomic_rmw!(
                                1,
                                vm::op_i32_atomic_rmw16_or_u,
                                vm::op_i32_atomic_rmw16_or_u_shared,
                                vm::op_i32_atomic_rmw16_or_u_indexed_local,
                                vm::op_i32_atomic_rmw16_or_u_indexed_shared,
                                ValType::I32,
                                ValType::I32
                            )
                        }
                        0x37 => atomic_rmw!(
                            0,
                            vm::op_i64_atomic_rmw8_or_u,
                            vm::op_i64_atomic_rmw8_or_u_shared,
                            vm::op_i64_atomic_rmw8_or_u_indexed_local,
                            vm::op_i64_atomic_rmw8_or_u_indexed_shared,
                            ValType::I64,
                            ValType::I64
                        ),
                        0x38 => {
                            atomic_rmw!(
                                1,
                                vm::op_i64_atomic_rmw16_or_u,
                                vm::op_i64_atomic_rmw16_or_u_shared,
                                vm::op_i64_atomic_rmw16_or_u_indexed_local,
                                vm::op_i64_atomic_rmw16_or_u_indexed_shared,
                                ValType::I64,
                                ValType::I64
                            )
                        }
                        0x39 => {
                            atomic_rmw!(
                                2,
                                vm::op_i64_atomic_rmw32_or_u,
                                vm::op_i64_atomic_rmw32_or_u_shared,
                                vm::op_i64_atomic_rmw32_or_u_indexed_local,
                                vm::op_i64_atomic_rmw32_or_u_indexed_shared,
                                ValType::I64,
                                ValType::I64
                            )
                        }
                        0x3A => atomic_rmw!(
                            2,
                            vm::op_i32_atomic_rmw_xor,
                            vm::op_i32_atomic_rmw_xor_shared,
                            vm::op_i32_atomic_rmw_xor_indexed_local,
                            vm::op_i32_atomic_rmw_xor_indexed_shared,
                            ValType::I32,
                            ValType::I32
                        ),
                        0x3B => atomic_rmw!(
                            3,
                            vm::op_i64_atomic_rmw_xor,
                            vm::op_i64_atomic_rmw_xor_shared,
                            vm::op_i64_atomic_rmw_xor_indexed_local,
                            vm::op_i64_atomic_rmw_xor_indexed_shared,
                            ValType::I64,
                            ValType::I64
                        ),
                        0x3C => {
                            atomic_rmw!(
                                0,
                                vm::op_i32_atomic_rmw8_xor_u,
                                vm::op_i32_atomic_rmw8_xor_u_shared,
                                vm::op_i32_atomic_rmw8_xor_u_indexed_local,
                                vm::op_i32_atomic_rmw8_xor_u_indexed_shared,
                                ValType::I32,
                                ValType::I32
                            )
                        }
                        0x3D => {
                            atomic_rmw!(
                                1,
                                vm::op_i32_atomic_rmw16_xor_u,
                                vm::op_i32_atomic_rmw16_xor_u_shared,
                                vm::op_i32_atomic_rmw16_xor_u_indexed_local,
                                vm::op_i32_atomic_rmw16_xor_u_indexed_shared,
                                ValType::I32,
                                ValType::I32
                            )
                        }
                        0x3E => {
                            atomic_rmw!(
                                0,
                                vm::op_i64_atomic_rmw8_xor_u,
                                vm::op_i64_atomic_rmw8_xor_u_shared,
                                vm::op_i64_atomic_rmw8_xor_u_indexed_local,
                                vm::op_i64_atomic_rmw8_xor_u_indexed_shared,
                                ValType::I64,
                                ValType::I64
                            )
                        }
                        0x3F => {
                            atomic_rmw!(
                                1,
                                vm::op_i64_atomic_rmw16_xor_u,
                                vm::op_i64_atomic_rmw16_xor_u_shared,
                                vm::op_i64_atomic_rmw16_xor_u_indexed_local,
                                vm::op_i64_atomic_rmw16_xor_u_indexed_shared,
                                ValType::I64,
                                ValType::I64
                            )
                        }
                        0x40 => {
                            atomic_rmw!(
                                2,
                                vm::op_i64_atomic_rmw32_xor_u,
                                vm::op_i64_atomic_rmw32_xor_u_shared,
                                vm::op_i64_atomic_rmw32_xor_u_indexed_local,
                                vm::op_i64_atomic_rmw32_xor_u_indexed_shared,
                                ValType::I64,
                                ValType::I64
                            )
                        }
                        0x41 => atomic_rmw!(
                            2,
                            vm::op_i32_atomic_rmw_xchg,
                            vm::op_i32_atomic_rmw_xchg_shared,
                            vm::op_i32_atomic_rmw_xchg_indexed_local,
                            vm::op_i32_atomic_rmw_xchg_indexed_shared,
                            ValType::I32,
                            ValType::I32
                        ),
                        0x42 => atomic_rmw!(
                            3,
                            vm::op_i64_atomic_rmw_xchg,
                            vm::op_i64_atomic_rmw_xchg_shared,
                            vm::op_i64_atomic_rmw_xchg_indexed_local,
                            vm::op_i64_atomic_rmw_xchg_indexed_shared,
                            ValType::I64,
                            ValType::I64
                        ),
                        0x43 => {
                            atomic_rmw!(
                                0,
                                vm::op_i32_atomic_rmw8_xchg_u,
                                vm::op_i32_atomic_rmw8_xchg_u_shared,
                                vm::op_i32_atomic_rmw8_xchg_u_indexed_local,
                                vm::op_i32_atomic_rmw8_xchg_u_indexed_shared,
                                ValType::I32,
                                ValType::I32
                            )
                        }
                        0x44 => atomic_rmw!(
                            1,
                            vm::op_i32_atomic_rmw16_xchg_u,
                            vm::op_i32_atomic_rmw16_xchg_u_shared,
                            vm::op_i32_atomic_rmw16_xchg_u_indexed_local,
                            vm::op_i32_atomic_rmw16_xchg_u_indexed_shared,
                            ValType::I32,
                            ValType::I32
                        ),
                        0x45 => {
                            atomic_rmw!(
                                0,
                                vm::op_i64_atomic_rmw8_xchg_u,
                                vm::op_i64_atomic_rmw8_xchg_u_shared,
                                vm::op_i64_atomic_rmw8_xchg_u_indexed_local,
                                vm::op_i64_atomic_rmw8_xchg_u_indexed_shared,
                                ValType::I64,
                                ValType::I64
                            )
                        }
                        0x46 => atomic_rmw!(
                            1,
                            vm::op_i64_atomic_rmw16_xchg_u,
                            vm::op_i64_atomic_rmw16_xchg_u_shared,
                            vm::op_i64_atomic_rmw16_xchg_u_indexed_local,
                            vm::op_i64_atomic_rmw16_xchg_u_indexed_shared,
                            ValType::I64,
                            ValType::I64
                        ),
                        0x47 => atomic_rmw!(
                            2,
                            vm::op_i64_atomic_rmw32_xchg_u,
                            vm::op_i64_atomic_rmw32_xchg_u_shared,
                            vm::op_i64_atomic_rmw32_xchg_u_indexed_local,
                            vm::op_i64_atomic_rmw32_xchg_u_indexed_shared,
                            ValType::I64,
                            ValType::I64
                        ),
                        0x48 => atomic_cmpxchg!(
                            2,
                            vm::op_i32_atomic_rmw_cmpxchg,
                            vm::op_i32_atomic_rmw_cmpxchg_shared,
                            vm::op_i32_atomic_rmw_cmpxchg_indexed_local,
                            vm::op_i32_atomic_rmw_cmpxchg_indexed_shared,
                            ValType::I32,
                            ValType::I32
                        ),
                        0x49 => atomic_cmpxchg!(
                            3,
                            vm::op_i64_atomic_rmw_cmpxchg,
                            vm::op_i64_atomic_rmw_cmpxchg_shared,
                            vm::op_i64_atomic_rmw_cmpxchg_indexed_local,
                            vm::op_i64_atomic_rmw_cmpxchg_indexed_shared,
                            ValType::I64,
                            ValType::I64
                        ),
                        0x4A => atomic_cmpxchg!(
                            0,
                            vm::op_i32_atomic_rmw8_cmpxchg_u,
                            vm::op_i32_atomic_rmw8_cmpxchg_u_shared,
                            vm::op_i32_atomic_rmw8_cmpxchg_u_indexed_local,
                            vm::op_i32_atomic_rmw8_cmpxchg_u_indexed_shared,
                            ValType::I32,
                            ValType::I32
                        ),
                        0x4B => atomic_cmpxchg!(
                            1,
                            vm::op_i32_atomic_rmw16_cmpxchg_u,
                            vm::op_i32_atomic_rmw16_cmpxchg_u_shared,
                            vm::op_i32_atomic_rmw16_cmpxchg_u_indexed_local,
                            vm::op_i32_atomic_rmw16_cmpxchg_u_indexed_shared,
                            ValType::I32,
                            ValType::I32
                        ),
                        0x4C => atomic_cmpxchg!(
                            0,
                            vm::op_i64_atomic_rmw8_cmpxchg_u,
                            vm::op_i64_atomic_rmw8_cmpxchg_u_shared,
                            vm::op_i64_atomic_rmw8_cmpxchg_u_indexed_local,
                            vm::op_i64_atomic_rmw8_cmpxchg_u_indexed_shared,
                            ValType::I64,
                            ValType::I64
                        ),
                        0x4D => atomic_cmpxchg!(
                            1,
                            vm::op_i64_atomic_rmw16_cmpxchg_u,
                            vm::op_i64_atomic_rmw16_cmpxchg_u_shared,
                            vm::op_i64_atomic_rmw16_cmpxchg_u_indexed_local,
                            vm::op_i64_atomic_rmw16_cmpxchg_u_indexed_shared,
                            ValType::I64,
                            ValType::I64
                        ),
                        0x4E => atomic_cmpxchg!(
                            2,
                            vm::op_i64_atomic_rmw32_cmpxchg_u,
                            vm::op_i64_atomic_rmw32_cmpxchg_u_shared,
                            vm::op_i64_atomic_rmw32_cmpxchg_u_indexed_local,
                            vm::op_i64_atomic_rmw32_cmpxchg_u_indexed_shared,
                            ValType::I64,
                            ValType::I64
                        ),
                        _ => Err(WasmParserError::InvalidInstruction([
                            0xFE, next as u8, 0x00, 0x00,
                        ]))?,
                    }
                }
            }
            unknown => Err(WasmParserError::invalid_instruction1(unknown))?,
        })
    }
    #[allow(clippy::too_many_arguments)]
    pub fn parse_instrs(
        &mut self,
        data_count_section: &mut DataCountVerifier,
        instrs: &mut InstructionGenerator,
        checker: &mut TypeChecker,
        jump_resolver: &mut JumpResolver,
        else_addr: &mut Option<u32>,
        stack_map_sites: &mut Vec<StackMapSourceSite>,
        unwind_sites: &mut Vec<UnwindSourceSite>,
    ) -> Result<usize> {
        let mut read_bytes = 0;
        loop {
            let (len, end) = self.parse_inst(
                data_count_section,
                instrs,
                checker,
                jump_resolver,
                else_addr,
                stack_map_sites,
                unwind_sites,
            )?;
            instrs.seal_emitted_instruction();
            trace!("{checker:?}");
            read_bytes += len;
            if end {
                return Ok(read_bytes);
            }
        }
    }
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        reader: &'a mut R,
        types: &'a TypeSection,
        functions: &'a [TypeIdx],
        imported_function_len: u32,
        funcidx: FuncIdx,
        mems: &'a [MemType],
        functype: &'a FuncType,
        locals: &'a LocalReassignTable,
        globals: &'a [GlobalType],
        tables: &'a [TableType],
        elems: &'a [Elem],
    ) -> Self {
        Self {
            reader,
            elems,
            types,
            functions,
            imported_function_len,
            funcidx,
            mems,
            functype,
            locals,
            globals,
            tables,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        common::{ControlFlowMetadataKind, FunctionBody, Operand},
        IoReadBinaryReader, WasmParser,
    };

    fn func_in_module(wat: &str, func_index: usize) -> crate::common::Func {
        let bytes = wat::parse_str(wat).expect("wat must parse");
        let mut reader = IoReadBinaryReader::from(bytes.as_slice());
        let mut parser = WasmParser::new(&mut reader);
        let module = parser.parse_module().expect("module must parse");
        let FunctionBody::Wasm(func) = &module.codes.0[func_index] else {
            panic!("expected wasm function body");
        };
        func.clone()
    }

    fn op_at_in_func(wat: &str, func_index: usize, index: usize) -> crate::common::Op {
        let bytes = wat::parse_str(wat).expect("wat must parse");
        let mut reader = IoReadBinaryReader::from(bytes.as_slice());
        let mut parser = WasmParser::new(&mut reader);
        let module = parser.parse_module().expect("module must parse");
        let FunctionBody::Wasm(func) = &module.codes.0[func_index] else {
            panic!("expected wasm function body");
        };
        unsafe { func.expr[index].op }
    }

    fn op_at(wat: &str, index: usize) -> crate::common::Op {
        op_at_in_func(wat, 0, index)
    }

    fn ops_in_func(wat: &str, func_index: usize) -> Vec<crate::common::Op> {
        let bytes = wat::parse_str(wat).expect("wat must parse");
        let mut reader = IoReadBinaryReader::from(bytes.as_slice());
        let mut parser = WasmParser::new(&mut reader);
        let module = parser.parse_module().expect("module must parse");
        let FunctionBody::Wasm(func) = &module.codes.0[func_index] else {
            panic!("expected wasm function body");
        };
        func.expr.iter().map(|instr| unsafe { instr.op }).collect()
    }

    fn contains_op(ops: &[crate::common::Op], expected: crate::common::Op) -> bool {
        ops.iter()
            .copied()
            .any(|op| std::ptr::fn_addr_eq(op, expected))
    }

    fn operand_at(wat: &str, index: usize) -> Operand {
        let func = func_in_module(wat, 0);
        unsafe { func.expr[index].operand }
    }

    #[test]
    fn parser_specializes_default_memory_load_handler() {
        let ops = ops_in_func(
            r#"(module (memory 1) (func (export "f") (param i32) (result i32) local.get 0 i32.const 0 i32.add i32.load))"#,
            0,
        );
        assert!(contains_op(
            &ops,
            vm::op_i32_local_imm_addr_load as crate::common::Op
        ));
    }

    #[cfg(feature = "threads")]
    #[test]
    fn parser_specializes_shared_default_memory_load_handler() {
        let shared = op_at(
            r#"(module (memory 1 2 shared) (func (export "f") (param i32) (result i32) local.get 0 i32.load))"#,
            2,
        );
        assert!(std::ptr::fn_addr_eq(
            shared,
            vm::op_i32_load_shared as crate::common::Op
        ));
    }

    #[test]
    fn parser_specializes_indexed_local_memory_load_handler() {
        let local = op_at(
            r#"
            (module
              (memory 1)
              (memory $m 1)
              (func (export "f") (param i32) (result i32)
                local.get 0
                i32.load $m))
            "#,
            2,
        );
        assert!(std::ptr::fn_addr_eq(
            local,
            vm::op_i32_load_indexed_local as crate::common::Op
        ));
    }

    #[test]
    fn parser_collects_control_flow_metadata_for_raw_br_table_and_if_else() {
        let func = func_in_module(
            r#"
            (module
              (func (export "f") (param i32 i32) (result i32)
                block (result i32)
                  local.get 0
                  if (result i32)
                    local.get 1
                    i32.const 1
                    br_table 0 0
                  else
                    i32.const 7
                  end
                end))
            "#,
            0,
        );

        assert!(!func.control_flow_metadata.is_empty());
        let mut saw_if = false;
        let mut saw_else = false;
        let mut saw_br_table = false;
        for site in func.control_flow_metadata.iter() {
            assert!((site.instruction_ordinal as usize) < func.expr.len());
            let op = unsafe { func.expr[site.instruction_ordinal as usize].op };
            match &site.kind {
                ControlFlowMetadataKind::Jump {
                    jump_operand_slots,
                    target_ordinals,
                } if std::ptr::fn_addr_eq(op, vm::op_if as crate::common::Op) => {
                    saw_if = true;
                    assert_eq!(&**jump_operand_slots, &[1]);
                    assert_eq!(target_ordinals.len(), 1);
                }
                ControlFlowMetadataKind::Jump {
                    jump_operand_slots,
                    target_ordinals,
                } if std::ptr::fn_addr_eq(op, vm::op_else as crate::common::Op) => {
                    saw_else = true;
                    assert_eq!(&**jump_operand_slots, &[1]);
                    assert_eq!(target_ordinals.len(), 1);
                }
                ControlFlowMetadataKind::Jump {
                    jump_operand_slots,
                    target_ordinals,
                } if std::ptr::fn_addr_eq(op, vm::op_br_table as crate::common::Op) => {
                    saw_br_table = true;
                    assert_eq!(jump_operand_slots.len(), 2);
                    assert_eq!(target_ordinals.len(), 2);
                }
                _ => {}
            }
        }
        assert!(saw_if, "if site must be recorded");
        assert!(saw_else, "else site must be recorded");
        assert!(saw_br_table, "br_table site must be recorded");
    }

    #[test]
    fn parser_collects_control_flow_metadata_for_specialized_branch_superinstructions() {
        let func = func_in_module(
            r#"
            (module
              (func (export "f") (param i32) (result i32)
                block
                  local.get 0
                  i32.eqz
                  br_if 0
                end
                i32.const 9))
            "#,
            0,
        );

        let site = func
            .control_flow_metadata
            .iter()
            .find(|site| {
                let op = unsafe { func.expr[site.instruction_ordinal as usize].op };
                std::ptr::fn_addr_eq(op, vm::op_i32_local_eqz_br_if as crate::common::Op)
            })
            .expect("specialized br_if site must be recorded");

        match &site.kind {
            ControlFlowMetadataKind::Jump {
                jump_operand_slots,
                target_ordinals,
            } => {
                assert_eq!(&**jump_operand_slots, &[2]);
                assert_eq!(target_ordinals.len(), 1);
                assert!((target_ordinals[0] as usize) < func.expr.len());
            }
            other => panic!("expected jump metadata, got {other:?}"),
        }
    }

    #[test]
    fn parser_collects_stack_map_and_unwind_metadata_for_safepoints() {
        let func = func_in_module(
            r#"
            (module
              (func $callee (param externref) (result externref)
                local.get 0)
              (func (export "f") (param externref) (result externref)
                block (result externref)
                  local.get 0
                end
                call $callee))
            "#,
            1,
        );

        let layout = func.frame_layout.as_ref();
        let stack_sites = layout.cold.stack_map_sites.as_ref();
        let unwind_sites = layout.cold.unwind_sites.as_ref();
        assert!(!stack_sites.is_empty());
        assert!(!unwind_sites.is_empty());

        let call_site = stack_sites
            .iter()
            .find(|site| site.kind == StackMapSafepointKind::Call)
            .expect("call safepoint must exist");
        assert_eq!(call_site.operand_bytes, 4);
        assert_eq!(call_site.ref_offsets_from_operand_base.as_ref(), &[0]);

        let block_return_site = stack_sites
            .iter()
            .find(|site| site.kind == StackMapSafepointKind::BlockReturn)
            .expect("block return safepoint must exist");
        assert_eq!(block_return_site.operand_bytes, 4);
        assert_eq!(
            block_return_site.ref_offsets_from_operand_base.as_ref(),
            &[0]
        );

        let function_return_site = stack_sites
            .iter()
            .find(|site| site.kind == StackMapSafepointKind::FunctionReturn)
            .expect("function return safepoint must exist");
        assert_eq!(function_return_site.operand_bytes, 4);
        assert_eq!(
            function_return_site.ref_offsets_from_operand_base.as_ref(),
            &[0]
        );

        assert!(unwind_sites
            .iter()
            .any(|site| site.kind == StackMapSafepointKind::BlockReturn));
        assert!(unwind_sites
            .iter()
            .any(|site| site.kind == StackMapSafepointKind::FunctionReturn));
        assert!(stack_sites
            .iter()
            .all(|site| (site.instruction_ordinal as usize) < func.expr.len()));
        assert!(unwind_sites
            .iter()
            .all(|site| (site.instruction_ordinal as usize) < func.expr.len()));
    }

    #[cfg(feature = "threads")]
    #[test]
    fn parser_collects_memory_wait_safepoint_metadata() {
        let func = func_in_module(
            r#"
            (module
              (memory 1 1 shared)
              (func (export "wait32") (param i32 i32 i64) (result i32)
                local.get 0
                local.get 1
                local.get 2
                memory.atomic.wait32))
            "#,
            0,
        );

        let layout = func.frame_layout.as_ref();
        let stack_sites = layout.cold.stack_map_sites.as_ref();
        let unwind_sites = layout.cold.unwind_sites.as_ref();

        let wait_stack_site = stack_sites
            .iter()
            .find(|site| site.kind == StackMapSafepointKind::MemoryWait)
            .expect("memory.wait stack-map safepoint must exist");
        assert_eq!(wait_stack_site.operand_bytes, 16);
        assert_eq!(
            wait_stack_site.ref_offsets_from_operand_base.as_ref(),
            &[] as &[u32]
        );

        let wait_unwind_site = unwind_sites
            .iter()
            .find(|site| site.kind == StackMapSafepointKind::MemoryWait)
            .expect("memory.wait unwind safepoint must exist");
        assert_eq!(wait_unwind_site.result_slot_from_local_top, None);
    }

    #[test]
    fn parser_builds_frame_layout_metadata_for_mixed_locals_and_refs() {
        let func = func_in_module(
            r#"
            (module
              (func (export "f") (param i32 i64)
                (local externref)
                (local i32)
                (local funcref)
                (local externref)
                nop))
            "#,
            0,
        );

        let layout = func.frame_layout.as_ref();
        let slots = layout.cold.local_slots.as_ref();
        let ref_runs = layout.cold.local_ref_runs.as_ref();

        assert_eq!(slots.len(), 6);
        assert_eq!(
            slots
                .iter()
                .map(|slot| slot.wasm_local_index)
                .collect::<Vec<_>>(),
            vec![0, 1, 2, 3, 4, 5]
        );
        assert_eq!(
            slots.iter().map(|slot| slot.val_type).collect::<Vec<_>>(),
            vec![
                ValType::I32,
                ValType::I64,
                ValType::ExternRef,
                ValType::I32,
                ValType::FuncRef,
                ValType::ExternRef,
            ]
        );
        assert!(slots
            .iter()
            .all(|slot| slot.offset_from_local_top < layout.fixed_frame_bytes));
        assert_eq!(
            ref_runs,
            &[crate::common::RefSlotRun {
                start_from_local_top: 16,
                len_bytes: 12,
            },]
        );
    }

    #[test]
    fn parser_specializes_local_direct_call_handler() {
        let op = op_at_in_func(
            r#"
            (module
              (func $callee)
              (func (export "f")
                call $callee))
            "#,
            1,
            0,
        );
        assert!(std::ptr::fn_addr_eq(op, vm::op_call as crate::common::Op));
    }

    #[test]
    fn parser_keeps_import_direct_call_generic() {
        let op = op_at(
            r#"
            (module
              (import "host" "callee" (func $callee))
              (func (export "f")
                call $callee))
            "#,
            0,
        );
        assert!(std::ptr::fn_addr_eq(
            op,
            vm::op_call_import as crate::common::Op
        ));
    }

    #[test]
    fn parser_specializes_local_return_call_handler() {
        let op = op_at_in_func(
            r#"
            (module
              (func $callee)
              (func (export "f")
                return_call $callee))
            "#,
            1,
            0,
        );
        assert!(std::ptr::fn_addr_eq(
            op,
            vm::op_return_call as crate::common::Op
        ));
    }

    #[test]
    fn parser_keeps_import_return_call_generic() {
        let op = op_at(
            r#"
            (module
              (import "host" "callee" (func $callee))
              (func (export "f")
                return_call $callee))
            "#,
            0,
        );
        assert!(std::ptr::fn_addr_eq(
            op,
            vm::op_return_call_import as crate::common::Op
        ));
    }

    #[test]
    fn parser_specializes_i32_local_add_imm_set4_superinstruction() {
        let op = op_at(
            r#"
            (module
              (func (export "f") (param i32)
                local.get 0
                i32.const 7
                i32.add
                local.set 0))
            "#,
            0,
        );
        assert!(std::ptr::fn_addr_eq(
            op,
            vm::op_i32_local_add_imm_set4 as crate::common::Op
        ));
        assert_eq!(
            unsafe {
                operand_at(r#"(module (func (export "f") (param i32) local.get 0 i32.const 7 i32.add local.set 0))"#, 2).i32
            },
            7
        );
    }

    #[test]
    fn parser_specializes_i32_local_sub_imm_tee4_superinstruction() {
        let op = op_at(
            r#"
            (module
              (func (export "f") (param i32) (result i32)
                local.get 0
                i32.const 7
                i32.sub
                local.tee 0))
            "#,
            0,
        );
        assert!(std::ptr::fn_addr_eq(
            op,
            vm::op_i32_local_sub_imm_tee4 as crate::common::Op
        ));
    }

    #[test]
    fn parser_specializes_i32_local_eqz_br_if_superinstruction() {
        let op = op_at(
            r#"
            (module
              (func (export "f") (param i32)
                block
                  local.get 0
                  i32.eqz
                  br_if 0
                end))
            "#,
            0,
        );
        assert!(std::ptr::fn_addr_eq(
            op,
            vm::op_i32_local_eqz_br_if as crate::common::Op
        ));
    }

    #[test]
    fn parser_specializes_i32_local_br_if_superinstruction() {
        let op = op_at(
            r#"
            (module
              (func (export "f") (param i32)
                block
                  local.get 0
                  br_if 0
                end))
            "#,
            0,
        );
        assert!(std::ptr::fn_addr_eq(
            op,
            vm::op_i32_local_br_if as crate::common::Op
        ));
    }

    #[test]
    fn parser_specializes_i32_local_eqz_if_superinstruction() {
        let op = op_at(
            r#"
            (module
              (func (export "f") (param i32)
                local.get 0
                i32.eqz
                if
                  nop
                end))
            "#,
            0,
        );
        assert!(std::ptr::fn_addr_eq(
            op,
            vm::op_i32_local_eqz_if as crate::common::Op
        ));
    }

    #[test]
    fn parser_specializes_i32_local_and_eqz_br_if_superinstruction() {
        let op = op_at(
            r#"
            (module
              (func (export "f") (param i32)
                block
                  local.get 0
                  i32.const 2
                  i32.and
                  i32.eqz
                  br_if 0
                end))
            "#,
            0,
        );
        assert!(std::ptr::fn_addr_eq(
            op,
            vm::op_i32_local_and_imm_eqz_br_if as crate::common::Op
        ));
    }

    #[test]
    fn parser_specializes_i32_local_and_if_superinstruction() {
        let op = op_at(
            r#"
            (module
              (func (export "f") (param i32) (result i32)
                local.get 0
                i32.const 1
                i32.and
                if (result i32)
                  i32.const 1
                else
                  i32.const 0
                end))
            "#,
            0,
        );
        assert!(
            std::ptr::fn_addr_eq(op, vm::op_i32_local_and_imm_if as crate::common::Op)
                || std::ptr::fn_addr_eq(op, vm::op_i32_local_scalar_imm_push4 as crate::common::Op)
        );
    }

    #[test]
    fn parser_specializes_i32_local_local_ge_u_br_if_superinstruction() {
        let op = op_at(
            r#"
            (module
              (func (export "f") (param i32 i32)
                block
                  local.get 0
                  local.get 1
                  i32.ge_u
                  br_if 0
                end))
            "#,
            0,
        );
        assert!(std::ptr::fn_addr_eq(
            op,
            vm::op_i32_local_local_ge_u_br_if as crate::common::Op
        ));
    }

    #[test]
    fn parser_specializes_i32_load_const_local_superinstruction() {
        let op = op_at(
            r#"
            (module
              (memory 1)
              (func (export "f") (result i32)
                i32.const 8
                i32.load))
            "#,
            0,
        );
        assert!(std::ptr::fn_addr_eq(
            op,
            vm::op_i32_load_const_local as crate::common::Op
        ));
        assert_eq!(
            unsafe {
                operand_at(
                    r#"(module (memory 1) (func (export "f") (result i32) i32.const 8 i32.load))"#,
                    1,
                )
                .u32
            },
            8
        );
    }

    #[test]
    fn parser_specializes_i32_local_get4_store_const_local_superinstruction() {
        let op = op_at(
            r#"
            (module
              (memory 1)
              (func (export "f") (param i32)
                i32.const 8
                local.get 0
                i32.store))
            "#,
            0,
        );
        assert!(std::ptr::fn_addr_eq(
            op,
            vm::op_i32_local_get4_store_const_local as crate::common::Op
        ));
        assert_eq!(
            unsafe {
                operand_at(
                    r#"(module (memory 1) (func (export "f") (param i32) i32.const 8 local.get 0 i32.store))"#,
                    1,
                )
                .u32
            },
            8
        );
    }

    #[test]
    fn parser_specializes_i32_local_local_add_set4_superinstruction() {
        let op = op_at(
            r#"
            (module
              (func (export "f") (param i32 i32)
                local.get 0
                local.get 1
                i32.add
                local.set 0))
            "#,
            0,
        );
        assert!(std::ptr::fn_addr_eq(
            op,
            vm::op_i32_local_local_add_set4 as crate::common::Op
        ));
    }

    #[test]
    fn parser_specializes_local_copy4_superinstruction() {
        let op = op_at(
            r#"
            (module
              (func (export "f") (param i32) (result i32)
                (local i32)
                local.get 0
                local.set 1
                local.get 1))
            "#,
            0,
        );
        assert!(std::ptr::fn_addr_eq(
            op,
            vm::op_local_copy4 as crate::common::Op
        ));
    }

    #[test]
    fn parser_specializes_local_copy_tee4_superinstruction() {
        let op = op_at(
            r#"
            (module
              (func (export "f") (param i32) (result i32)
                (local i32)
                local.get 0
                local.tee 1))
            "#,
            0,
        );
        assert!(std::ptr::fn_addr_eq(
            op,
            vm::op_local_copy_tee4 as crate::common::Op
        ));
    }

    #[test]
    fn parser_specializes_i32_const_set4_superinstruction() {
        let op = op_at(
            r#"
            (module
              (func (export "f") (result i32)
                (local i32)
                i32.const 7
                local.set 0
                local.get 0))
            "#,
            0,
        );
        assert!(std::ptr::fn_addr_eq(
            op,
            vm::op_i32_const_set4 as crate::common::Op
        ));
    }

    #[test]
    fn parser_specializes_i32_local_local_add_tee4_superinstruction() {
        let op = op_at(
            r#"
            (module
              (func (export "f") (param i32 i32) (result i32)
                local.get 0
                local.get 1
                i32.add
                local.tee 0))
            "#,
            0,
        );
        assert!(std::ptr::fn_addr_eq(
            op,
            vm::op_i32_local_local_add_tee4 as crate::common::Op
        ));
    }

    #[test]
    fn parser_specializes_i32_local_and_imm_set4_superinstruction() {
        let op = op_at(
            r#"
            (module
              (func (export "f") (param i32)
                local.get 0
                i32.const 255
                i32.and
                local.set 0))
            "#,
            0,
        );
        assert!(std::ptr::fn_addr_eq(
            op,
            vm::op_i32_local_and_imm_set4 as crate::common::Op
        ));
    }

    #[test]
    fn parser_specializes_i32_local_shl_imm_tee4_superinstruction() {
        let op = op_at(
            r#"
            (module
              (func (export "f") (param i32) (result i32)
                local.get 0
                i32.const 3
                i32.shl
                local.tee 0))
            "#,
            0,
        );
        assert!(std::ptr::fn_addr_eq(
            op,
            vm::op_i32_local_shl_imm_tee4 as crate::common::Op
        ));
    }

    #[test]
    fn parser_specializes_i32_local_shr_u_imm_set4_superinstruction() {
        let op = op_at(
            r#"
            (module
              (func (export "f") (param i32)
                local.get 0
                i32.const 5
                i32.shr_u
                local.set 0))
            "#,
            0,
        );
        assert!(std::ptr::fn_addr_eq(
            op,
            vm::op_i32_local_shr_u_imm_set4 as crate::common::Op
        ));
    }

    #[test]
    fn parser_specializes_i32_local_add_imm_push4_superinstruction() {
        let op = op_at(
            r#"
            (module
              (func (export "f") (param i32) (result i32)
                local.get 0
                i32.const 7
                i32.add
                i32.const 3
                i32.xor))
            "#,
            0,
        );
        assert!(std::ptr::fn_addr_eq(
            op,
            vm::op_i32_local_scalar_imm_push4 as crate::common::Op
        ));
    }

    #[test]
    fn parser_specializes_i32_local_local_sub_push4_superinstruction() {
        let op = op_at(
            r#"
            (module
              (func (export "f") (param i32 i32) (result i32)
                local.get 0
                local.get 1
                i32.sub
                i32.const 1
                i32.add))
            "#,
            0,
        );
        assert!(std::ptr::fn_addr_eq(
            op,
            vm::op_i32_local_local_scalar_push4 as crate::common::Op
        ));
    }

    #[test]
    fn parser_specializes_i32_local_addr_load_superinstruction() {
        let op = op_at(
            r#"
            (module
              (memory 1)
              (func (export "f") (param i32) (result i32)
                local.get 0
                i32.load))
            "#,
            0,
        );
        assert!(std::ptr::fn_addr_eq(
            op,
            vm::op_i32_local_addr_load as crate::common::Op
        ));
    }

    #[test]
    fn parser_specializes_i32_local_imm_addr_load_superinstruction() {
        let op = op_at(
            r#"
            (module
              (memory 1)
              (func (export "f") (param i32) (result i32)
                local.get 0
                i32.const 4
                i32.add
                i32.load))
            "#,
            0,
        );
        assert!(std::ptr::fn_addr_eq(
            op,
            vm::op_i32_local_imm_addr_load as crate::common::Op
        ));
    }

    #[test]
    fn parser_specializes_i32_local_addr_load8_u_superinstruction() {
        let op = op_at(
            r#"
            (module
              (memory 1)
              (func (export "f") (param i32) (result i32)
                local.get 0
                i32.load8_u))
            "#,
            0,
        );
        assert!(std::ptr::fn_addr_eq(
            op,
            vm::op_i32_local_addr_load8_u as crate::common::Op
        ));
    }

    #[test]
    fn parser_specializes_i32_local_addr_load8_u_and_imm_eqz_if_superinstruction() {
        let op = op_at(
            r#"
            (module
              (memory 1)
              (func (export "f") (param i32)
                local.get 0
                i32.load8_u
                i32.const 32
                i32.and
                i32.eqz
                if
                  nop
                end))
            "#,
            0,
        );
        assert!(std::ptr::fn_addr_eq(
            op,
            vm::op_i32_local_addr_load8_u_and_imm_eqz_if as crate::common::Op
        ));
    }

    #[test]
    fn parser_specializes_i32_local_addr_load8_u_and_imm_eqz_br_if_superinstruction() {
        let ops = ops_in_func(
            r#"
            (module
              (memory 1)
              (func (export "f") (param i32) (result i32)
                block
                  local.get 0
                  i32.load8_u
                  i32.const 32
                  i32.and
                  i32.eqz
                  br_if 0
                  i32.const 7
                  return
                end
                i32.const 9))
            "#,
            0,
        );
        assert!(contains_op(
            &ops,
            vm::op_i32_local_addr_load8_u_and_imm_eqz_br_if as crate::common::Op
        ));
    }

    #[test]
    fn parser_specializes_producer_tee_eqz_if_superinstruction() {
        let ops = ops_in_func(
            r#"
            (module
              (func (export "f") (param i32)
                (local i32)
                block
                  local.get 0
                  i32.const 255
                  i32.and
                  local.tee 1
                  i32.eqz
                  if
                    nop
                end
                end))
            "#,
            0,
        );
        assert!(contains_op(
            &ops,
            vm::op_i32_seed_tee_eqz_if as crate::common::Op
        ));
    }

    #[test]
    fn parser_specializes_producer_tee_eqz_br_if_superinstruction() {
        let ops = ops_in_func(
            r#"
            (module
              (func (export "f") (param i32) (result i32)
                (local i32)
                block $skip
                  local.get 0
                  i32.const 255
                  i32.and
                  local.tee 1
                  i32.eqz
                  br_if $skip
                  local.get 1
                  return
                end
                i32.const 0))
            "#,
            0,
        );
        assert!(contains_op(
            &ops,
            vm::op_i32_seed_tee_eqz_br_if as crate::common::Op
        ));
    }

    #[test]
    fn parser_specializes_producer_tee_imm_compare_br_if_superinstruction() {
        let ops = ops_in_func(
            r#"
            (module
              (func (export "f") (param i32) (result i32)
                (local i32)
                block $skip
                  local.get 0
                  i32.const 255
                  i32.and
                  local.tee 1
                  i32.const 32
                  i32.gt_u
                  br_if $skip
                  local.get 1
                  return
                end
                i32.const 0))
            "#,
            0,
        );
        assert!(contains_op(
            &ops,
            vm::op_i32_seed_tee_imm_compare_br_if as crate::common::Op
        ));
    }

    #[test]
    fn parser_specializes_producer_tee_imm_scalar_set4_superinstruction() {
        let op = op_at(
            r#"
            (module
              (func (export "f") (param i32) (result i32)
                (local i32)
                local.get 0
                local.tee 1
                i32.const 3
                i32.shl
                local.set 1
                local.get 1))
            "#,
            0,
        );
        assert!(std::ptr::fn_addr_eq(
            op,
            vm::op_i32_seed_tee_imm_scalar_set4 as crate::common::Op
        ));
    }

    #[test]
    fn parser_specializes_producer_tee_imm_scalar_tee4_superinstruction() {
        let op = op_at(
            r#"
            (module
              (func (export "f") (param i32) (result i32)
                (local i32)
                local.get 0
                local.tee 1
                i32.const 3
                i32.shl
                local.tee 1))
            "#,
            0,
        );
        assert!(std::ptr::fn_addr_eq(
            op,
            vm::op_i32_seed_tee_imm_scalar_tee4 as crate::common::Op
        ));
    }

    #[test]
    fn parser_specializes_producer_tee_const_self_select4_superinstruction() {
        let op = op_at(
            r#"
            (module
              (func (export "f") (param i32) (result i32)
                (local i32)
                local.get 0
                local.tee 1
                i32.const 7
                local.get 1
                select))
            "#,
            0,
        );
        assert!(std::ptr::fn_addr_eq(
            op,
            vm::op_i32_seed_tee_const_self_select4 as crate::common::Op
        ));
    }

    #[test]
    fn parser_specializes_i32_local_local_compare_tee_select4_superinstruction() {
        let op = op_at(
            r#"
            (module
              (func (export "f") (param i32 i32 i32 i32) (result i32) (local i32)
                local.get 0
                local.get 1
                local.get 2
                local.get 3
                i32.lt_u
                local.tee 4
                select))
            "#,
            4,
        );
        assert!(std::ptr::fn_addr_eq(
            op,
            vm::op_i32_local_local_compare_tee_select4 as crate::common::Op
        ));
    }

    #[test]
    fn parser_specializes_i32_local_const_compare_tee_select4_superinstruction() {
        let op = op_at(
            r#"
            (module
              (func (export "f") (param i32 i32 i32) (result i32) (local i32)
                local.get 0
                local.get 1
                local.get 2
                i32.const 7
                i32.gt_u
                local.tee 3
                select))
            "#,
            4,
        );
        assert!(std::ptr::fn_addr_eq(
            op,
            vm::op_i32_local_const_compare_tee_select4 as crate::common::Op
        ));
    }

    #[test]
    fn parser_specializes_i32_local_addr_load16_s_superinstruction() {
        let op = op_at(
            r#"
            (module
              (memory 1)
              (func (export "f") (param i32) (result i32)
                local.get 0
                i32.load16_s))
            "#,
            0,
        );
        assert!(std::ptr::fn_addr_eq(
            op,
            vm::op_i32_local_addr_load16_s as crate::common::Op
        ));
    }

    #[test]
    fn parser_specializes_i32_local_addr_load16_u_superinstruction() {
        let op = op_at(
            r#"
            (module
              (memory 1)
              (func (export "f") (param i32) (result i32)
                local.get 0
                i32.load16_u))
            "#,
            0,
        );
        assert!(std::ptr::fn_addr_eq(
            op,
            vm::op_i32_local_addr_load16_u as crate::common::Op
        ));
    }

    #[test]
    fn parser_specializes_f32_local_addr_load_superinstruction() {
        let op = op_at(
            r#"
            (module
              (memory 1)
              (func (export "f") (param i32) (result f32)
                local.get 0
                f32.load))
            "#,
            0,
        );
        assert!(std::ptr::fn_addr_eq(
            op,
            vm::op_f32_local_addr_load as crate::common::Op
        ));
    }

    #[test]
    fn parser_specializes_i32_local_local_store_superinstruction() {
        let op = op_at(
            r#"
            (module
              (memory 1)
              (func (export "f") (param i32 i32)
                local.get 0
                local.get 1
                i32.store))
            "#,
            0,
        );
        assert!(std::ptr::fn_addr_eq(
            op,
            vm::op_i32_local_local_store as crate::common::Op
        ));
    }

    #[test]
    fn parser_specializes_i32_local_imm_local_store_superinstruction() {
        let op = op_at(
            r#"
            (module
              (memory 1)
              (func (export "f") (param i32 i32)
                local.get 0
                i32.const 4
                i32.add
                local.get 1
                i32.store))
            "#,
            0,
        );
        assert!(std::ptr::fn_addr_eq(
            op,
            vm::op_i32_local_imm_local_store as crate::common::Op
        ));
    }

    #[test]
    fn parser_specializes_i32_local_local_load_tee_add_imm_store_superinstruction() {
        let op = op_at(
            r#"
            (module
              (memory 1)
              (func (export "f") (param i32) (local i32)
                local.get 0
                local.get 0
                i32.load
                local.tee 1
                i32.const 4
                i32.add
                i32.store))
            "#,
            0,
        );
        assert!(std::ptr::fn_addr_eq(
            op,
            vm::op_i32_local_local_load_tee_add_imm_store as crate::common::Op
        ));
    }

    #[test]
    fn parser_specializes_i32_local_local_store8_superinstruction() {
        let op = op_at(
            r#"
            (module
              (memory 1)
              (func (export "f") (param i32 i32)
                local.get 0
                local.get 1
                i32.store8))
            "#,
            0,
        );
        assert!(std::ptr::fn_addr_eq(
            op,
            vm::op_i32_local_local_store8 as crate::common::Op
        ));
    }

    #[test]
    fn parser_specializes_i32_local_local_load8_u_store8_superinstruction() {
        let op = op_at(
            r#"
            (module
              (memory 1)
              (func (export "f") (param i32 i32)
                local.get 0
                local.get 1
                i32.load8_u
                i32.store8))
            "#,
            0,
        );
        assert!(std::ptr::fn_addr_eq(
            op,
            vm::op_i32_local_local_load8_u_store8 as crate::common::Op
        ));
    }

    #[test]
    fn parser_specializes_i32_local_local_store16_superinstruction() {
        let op = op_at(
            r#"
            (module
              (memory 1)
              (func (export "f") (param i32 i32)
                local.get 0
                local.get 1
                i32.store16))
            "#,
            0,
        );
        assert!(std::ptr::fn_addr_eq(
            op,
            vm::op_i32_local_local_store16 as crate::common::Op
        ));
    }

    #[test]
    fn parser_specializes_i32_local_local_load16_u_store16_superinstruction() {
        let op = op_at(
            r#"
            (module
              (memory 1)
              (func (export "f") (param i32 i32)
                local.get 0
                local.get 1
                i32.load16_u
                i32.store16))
            "#,
            0,
        );
        assert!(std::ptr::fn_addr_eq(
            op,
            vm::op_i32_local_local_load16_u_store16 as crate::common::Op
        ));
    }

    #[test]
    fn parser_does_not_apply_second_wave_superinstructions_to_i64_locals() {
        let ops = ops_in_func(
            r#"
            (module
              (func (export "f") (param i64 i64)
                local.get 0
                local.get 1
                i64.add
                local.set 0))
            "#,
            0,
        );
        for op in ops {
            assert!(!std::ptr::fn_addr_eq(
                op,
                vm::op_i32_local_local_add_set4 as crate::common::Op
            ));
        }
    }

    #[test]
    fn parser_specializes_i64_local_mul_imm_set8_superinstruction() {
        let op = op_at(
            r#"
            (module
              (func (export "f") (param i64)
                local.get 0
                i64.const 3
                i64.mul
                local.set 0))
            "#,
            0,
        );
        assert!(std::ptr::fn_addr_eq(
            op,
            vm::op_i64_local_scalar_imm_set8 as crate::common::Op
        ));
    }

    #[test]
    fn parser_specializes_f64_local_div_imm_tee8_superinstruction() {
        let op = op_at(
            r#"
            (module
              (func (export "f") (param f64) (result f64)
                local.get 0
                f64.const 2
                f64.div
                local.tee 0))
            "#,
            0,
        );
        assert!(std::ptr::fn_addr_eq(
            op,
            vm::op_f64_local_scalar_imm_tee8 as crate::common::Op
        ));
    }

    #[test]
    fn parser_specializes_i64_local_local_xor_set8_superinstruction() {
        let op = op_at(
            r#"
            (module
              (func (export "f") (param i64 i64)
                local.get 0
                local.get 1
                i64.xor
                local.set 0))
            "#,
            0,
        );
        assert!(std::ptr::fn_addr_eq(
            op,
            vm::op_i64_local_local_scalar_set8 as crate::common::Op
        ));
    }

    #[test]
    fn parser_specializes_function_return_internal_ops_by_result_shape() {
        let ops_scalar4 = ops_in_func(
            r#"
            (module
              (func (export "f") (result i32)
                i32.const 1))
            "#,
            0,
        );
        let ops_generic = ops_in_func(
            r#"
            (module
              (func (export "f") (result i32 i32)
                i32.const 1
                i32.const 2))
            "#,
            0,
        );

        assert!(contains_op(
            &ops_scalar4,
            vm::special_function_return4 as crate::common::Op
        ));
        assert!(!contains_op(
            &ops_scalar4,
            vm::special_function_return_generic as crate::common::Op
        ));
        assert!(contains_op(
            &ops_generic,
            vm::special_function_return_generic as crate::common::Op
        ));
    }

    #[test]
    fn parser_specializes_loop_internal_ops_by_param_shape() {
        let ops = ops_in_func(
            r#"
            (module
              (func (export "f") (param i32) (result i32)
                local.get 0
                loop (param i32) (result i32)
                end))
            "#,
            0,
        );

        assert!(contains_op(&ops, vm::op_loop4 as crate::common::Op));
        assert!(!contains_op(&ops, vm::op_loop as crate::common::Op));
    }

    #[test]
    fn parser_specializes_block_return_internal_ops_by_result_shape() {
        let ops = ops_in_func(
            r#"
            (module
              (func (export "f") (result i32)
                block (result i32)
                  i32.const 1
                end))
            "#,
            0,
        );

        assert!(contains_op(
            &ops,
            vm::special_block_return4 as crate::common::Op
        ));
        assert!(!contains_op(
            &ops,
            vm::special_block_return as crate::common::Op
        ));
    }

    #[test]
    fn parser_specializes_i64_local_eqz_br_if_superinstruction() {
        let op = op_at(
            r#"
            (module
              (func (export "f") (param i64)
                block
                  local.get 0
                  i64.eqz
                  br_if 0
                end))
            "#,
            0,
        );
        assert!(std::ptr::fn_addr_eq(
            op,
            vm::op_i64_local_eqz_br_if as crate::common::Op
        ));
    }

    #[test]
    fn parser_specializes_i64_compare_set4_superinstruction() {
        let op = op_at(
            r#"
            (module
              (func (export "f") (param i64 i64) (local i32)
                local.get 0
                local.get 1
                i64.lt_s
                local.set 2))
            "#,
            0,
        );
        assert!(std::ptr::fn_addr_eq(
            op,
            vm::op_i64_local_local_compare_set4 as crate::common::Op
        ));
    }

    #[test]
    fn parser_specializes_f64_const_compare_br_if_superinstruction() {
        let op = op_at(
            r#"
            (module
              (func (export "f") (param f64)
                block
                  local.get 0
                  f64.const 0
                  f64.lt
                  br_if 0
                end))
            "#,
            0,
        );
        assert!(std::ptr::fn_addr_eq(
            op,
            vm::op_f64_local_const_compare_br_if as crate::common::Op
        ));
    }

    #[test]
    fn parser_specializes_select4_handler() {
        let op = op_at(
            r#"
            (module
              (func (export "f") (param i32 i32 i32) (result i32)
                local.get 0
                local.get 1
                local.get 2
                select))
            "#,
            6,
        );
        assert!(std::ptr::fn_addr_eq(
            op,
            vm::op_select4 as crate::common::Op
        ));
    }

    #[test]
    fn parser_specializes_select8_handler() {
        let op = op_at(
            r#"
            (module
              (func (export "f") (param i64 i64 i32) (result i64)
                local.get 0
                local.get 1
                local.get 2
                select))
            "#,
            6,
        );
        assert!(std::ptr::fn_addr_eq(
            op,
            vm::op_select8 as crate::common::Op
        ));
    }

    #[test]
    fn parser_specializes_i32_local_local_compare_select4_superinstruction() {
        let op = op_at(
            r#"
            (module
              (func (export "f") (param i32 i32 i32 i32) (result i32)
                local.get 0
                local.get 1
                local.get 2
                local.get 3
                i32.lt_u
                select))
            "#,
            4,
        );
        assert!(std::ptr::fn_addr_eq(
            op,
            vm::op_i32_local_local_compare_select4 as crate::common::Op
        ));
    }

    #[test]
    fn parser_specializes_i64_local_const_compare_select8_superinstruction() {
        let op = op_at(
            r#"
            (module
              (func (export "f") (param i64 i64 i64) (result i64)
                local.get 0
                local.get 1
                local.get 2
                i64.const 7
                i64.gt_s
                select))
            "#,
            4,
        );
        assert!(std::ptr::fn_addr_eq(
            op,
            vm::op_i64_local_const_compare_select8 as crate::common::Op
        ));
    }

    #[test]
    fn parser_specializes_i32_seed_tee_eqz_br_if_superinstruction() {
        let ops = ops_in_func(
            r#"
            (module
              (memory 1)
              (func (export "f") (param i32) (result i32) (local i32)
                block $exit
                  local.get 0
                  i32.load8_u
                  local.tee 1
                  i32.eqz
                  br_if $exit
                  i32.const 0
                  return
                end
                local.get 1))
            "#,
            0,
        );
        assert!(contains_op(
            &ops,
            vm::op_i32_seed_tee_eqz_br_if as crate::common::Op
        ));
    }

    #[test]
    fn parser_specializes_i32_seed_tee_imm_compare_br_if_superinstruction() {
        let ops = ops_in_func(
            r#"
            (module
              (memory 1)
              (func (export "f") (param i32) (result i32) (local i32)
                block $exit
                  local.get 0
                  i32.load8_u
                  local.tee 1
                  i32.const 31
                  i32.gt_u
                  br_if $exit
                  i32.const 0
                  return
                end
                local.get 1))
            "#,
            0,
        );
        assert!(contains_op(
            &ops,
            vm::op_i32_seed_tee_imm_compare_br_if as crate::common::Op
        ));
    }

    #[test]
    fn parser_specializes_i32_seed_tee_imm_scalar_tee4_superinstruction() {
        let op = op_at(
            r#"
            (module
              (func (export "f") (param i32) (result i32)
                local.get 0
                i32.const 1
                i32.add
                local.tee 0
                i32.const 255
                i32.and
                local.tee 0))
            "#,
            0,
        );
        assert!(std::ptr::fn_addr_eq(
            op,
            vm::op_i32_seed_tee_imm_scalar_tee4 as crate::common::Op
        ));
    }

    #[test]
    fn parser_specializes_i64_seed_tee_imm_scalar_set8_superinstruction() {
        let op = op_at(
            r#"
            (module
              (func (export "f") (param i64) (result i64)
                local.get 0
                local.tee 0
                i64.const 3
                i64.shr_u
                local.set 0
                local.get 0))
            "#,
            0,
        );
        assert!(std::ptr::fn_addr_eq(
            op,
            vm::op_i64_seed_tee_imm_scalar_set8 as crate::common::Op
        ));
    }

    #[test]
    fn parser_specializes_i32_seed_tee_const_self_select4_superinstruction() {
        let op = op_at(
            r#"
            (module
              (func (export "f") (param i32) (result i32) (local i32)
                local.get 0
                local.tee 1
                i32.const 7
                local.get 1
                select))
            "#,
            0,
        );
        assert!(std::ptr::fn_addr_eq(
            op,
            vm::op_i32_seed_tee_const_self_select4 as crate::common::Op
        ));
    }

    #[test]
    fn parser_specializes_i64_load_const_local_superinstruction() {
        let op = op_at(
            r#"
            (module
              (memory 1)
              (func (export "f") (result i64)
                i32.const 8
                i64.load))
            "#,
            0,
        );
        assert!(std::ptr::fn_addr_eq(
            op,
            vm::op_load_const_local8 as crate::common::Op
        ));
    }

    #[test]
    fn parser_specializes_i64_local_addr_load_superinstruction() {
        let op = op_at(
            r#"
            (module
              (memory 1)
              (func (export "f") (param i32) (result i64)
                local.get 0
                i64.load))
            "#,
            0,
        );
        assert!(std::ptr::fn_addr_eq(
            op,
            vm::op_local_addr_load8 as crate::common::Op
        ));
    }

    #[test]
    fn parser_specializes_i64_local_local_store_superinstruction() {
        let op = op_at(
            r#"
            (module
              (memory 1)
              (func (export "f") (param i32 i64)
                local.get 0
                local.get 1
                i64.store))
            "#,
            0,
        );
        assert!(std::ptr::fn_addr_eq(
            op,
            vm::op_local_local_store8 as crate::common::Op
        ));
    }

    #[test]
    fn parser_keeps_shared_i64_load_out_of_local_addr_superinstruction() {
        let op = op_at(
            r#"
            (module
              (memory 1 1 shared)
              (func (export "f") (param i32) (result i64)
                local.get 0
                i64.load))
            "#,
            2,
        );
        assert!(std::ptr::fn_addr_eq(
            op,
            vm::op_i64_load_shared as crate::common::Op
        ));
    }

    #[test]
    fn parser_keeps_indexed_i64_store_out_of_local_local_superinstruction() {
        let op = op_at(
            r#"
            (module
              (memory 1)
              (memory $dst 1)
              (func (export "f") (param i32 i64)
                local.get 0
                local.get 1
                i64.store $dst))
            "#,
            4,
        );
        assert!(std::ptr::fn_addr_eq(
            op,
            vm::op_i64_store_indexed_local as crate::common::Op
        ));
    }

    #[test]
    fn parser_decodes_indexed_memarg_memidx_and_offset() {
        let memarg = unsafe {
            operand_at(
                r#"
                (module
                  (memory 1)
                  (memory $dst 1)
                  (func (export "f") (param i32) (result i32)
                    local.get 0
                    i32.load $dst offset=8))
                "#,
                3,
            )
            .memarg
        };
        let memidx = unsafe {
            operand_at(
                r#"
                (module
                  (memory 1)
                  (memory $dst 1)
                  (func (export "f") (param i32) (result i32)
                    local.get 0
                    i32.load $dst offset=8))
                "#,
                4,
            )
            .u32
        };
        assert_eq!(memarg.offset, 8);
        assert_eq!(memidx, 1);
    }

    #[test]
    fn parser_specializes_indexed_local_memory_store_handler() {
        let local = op_at(
            r#"
            (module
              (memory 1)
              (memory $m 1)
              (func (export "f") (param i32)
                i32.const 8
                local.get 0
                i32.store $m))
            "#,
            4,
        );
        assert!(std::ptr::fn_addr_eq(
            local,
            vm::op_i32_store_indexed_local as crate::common::Op
        ));
    }

    #[cfg(feature = "threads")]
    #[test]
    fn parser_specializes_indexed_shared_memory_load_handler() {
        let shared = op_at(
            r#"
            (module
              (memory 1)
              (memory $m 1 2 shared)
              (func (export "f") (param i32) (result i32)
                local.get 0
                i32.load $m))
            "#,
            2,
        );
        assert!(std::ptr::fn_addr_eq(
            shared,
            vm::op_i32_load_indexed_shared as crate::common::Op
        ));
    }

    #[test]
    fn parser_specializes_bulk_memory_handler() {
        let local = op_at(
            r#"
            (module
              (memory 1)
              (data "abcd")
              (func (export "f") (param i32 i32 i32)
                local.get 0
                local.get 1
                local.get 2
                memory.copy))
            "#,
            6,
        );
        assert!(std::ptr::fn_addr_eq(
            local,
            vm::op_mem_copy_local as crate::common::Op
        ));
    }

    #[cfg(feature = "threads")]
    #[test]
    fn parser_specializes_shared_bulk_memory_handler() {
        let shared = op_at(
            r#"
            (module
              (memory 1 2 shared)
              (data "abcd")
              (func (export "f") (param i32 i32 i32)
                local.get 0
                local.get 1
                local.get 2
                memory.copy))
            "#,
            6,
        );
        assert!(std::ptr::fn_addr_eq(
            shared,
            vm::op_mem_copy_shared as crate::common::Op
        ));
    }

    #[test]
    fn parser_specializes_indexed_local_bulk_memory_handler() {
        let local_local = op_at(
            r#"
            (module
              (memory $dst 1)
              (memory $src 1)
              (data "abcd")
              (func (export "f") (param i32 i32 i32)
                local.get 0
                local.get 1
                local.get 2
                memory.copy $dst $src))
            "#,
            6,
        );
        assert!(std::ptr::fn_addr_eq(
            local_local,
            vm::op_mem_copy_indexed_local_local as crate::common::Op
        ));
    }

    #[cfg(feature = "threads")]
    #[test]
    fn parser_specializes_indexed_mixed_bulk_memory_handler() {
        let local_shared = op_at(
            r#"
            (module
              (memory $dst 1)
              (memory $src 1 2 shared)
              (data "abcd")
              (func (export "f") (param i32 i32 i32)
                local.get 0
                local.get 1
                local.get 2
                memory.copy $dst $src))
            "#,
            6,
        );
        let shared_local = op_at(
            r#"
            (module
              (memory $src 1)
              (memory $dst 1 2 shared)
              (data "abcd")
              (func (export "f") (param i32 i32 i32)
                local.get 0
                local.get 1
                local.get 2
                memory.copy $dst $src))
            "#,
            6,
        );
        assert!(std::ptr::fn_addr_eq(
            local_shared,
            vm::op_mem_copy_indexed_local_shared as crate::common::Op
        ));
        assert!(std::ptr::fn_addr_eq(
            shared_local,
            vm::op_mem_copy_indexed_shared_local as crate::common::Op
        ));
    }

    #[cfg(feature = "simd")]
    #[test]
    fn parser_specializes_simd_memory_handler() {
        let local = op_at(
            r#"
            (module
              (memory 1)
              (func (export "f") (param i32) (result v128)
                local.get 0
                v128.load))
            "#,
            2,
        );
        assert!(std::ptr::fn_addr_eq(
            local,
            vm::simd::op_v128_load as crate::common::Op
        ));
    }

    #[cfg(all(feature = "simd", feature = "threads"))]
    #[test]
    fn parser_specializes_shared_simd_memory_handler() {
        let shared = op_at(
            r#"
            (module
              (memory 1 2 shared)
              (func (export "f") (param i32) (result v128)
                local.get 0
                v128.load))
            "#,
            2,
        );
        assert!(std::ptr::fn_addr_eq(
            shared,
            vm::simd::op_v128_load_shared as crate::common::Op
        ));
    }

    #[cfg(feature = "simd")]
    #[test]
    fn parser_specializes_indexed_local_simd_memory_handler() {
        let local = op_at(
            r#"
            (module
              (memory 1)
              (memory $m 1)
              (func (export "f") (param i32) (result v128)
                local.get 0
                v128.load $m))
            "#,
            2,
        );
        assert!(std::ptr::fn_addr_eq(
            local,
            vm::simd::op_v128_load_indexed_local as crate::common::Op
        ));
    }

    #[cfg(all(feature = "simd", feature = "threads"))]
    #[test]
    fn parser_specializes_indexed_shared_simd_memory_handler() {
        let shared = op_at(
            r#"
            (module
              (memory 1)
              (memory $m 1 2 shared)
              (func (export "f") (param i32) (result v128)
                local.get 0
                v128.load $m))
            "#,
            2,
        );
        assert!(std::ptr::fn_addr_eq(
            shared,
            vm::simd::op_v128_load_indexed_shared as crate::common::Op
        ));
    }

    #[cfg(feature = "threads")]
    #[test]
    fn parser_specializes_atomic_wait_handler() {
        let unshared = op_at(
            r#"
            (module
              (memory 1)
              (func (export "f") (param i32 i32 i64) (result i32)
                local.get 0
                local.get 1
                local.get 2
                memory.atomic.wait32))
            "#,
            6,
        );
        let shared = op_at(
            r#"
            (module
              (memory 1 2 shared)
              (func (export "f") (param i32 i32 i64) (result i32)
                local.get 0
                local.get 1
                local.get 2
                memory.atomic.wait32))
            "#,
            6,
        );
        assert!(std::ptr::fn_addr_eq(
            unshared,
            vm::op_memory_atomic_wait32_unshared as crate::common::Op
        ));
        assert!(std::ptr::fn_addr_eq(
            shared,
            vm::op_memory_atomic_wait32_shared as crate::common::Op
        ));
    }

    #[cfg(feature = "threads")]
    #[test]
    fn parser_specializes_indexed_unshared_atomic_wait_handler() {
        let unshared = op_at(
            r#"
            (module
              (memory 1)
              (memory $m 1)
              (func (export "f") (param i32 i32 i64) (result i32)
                local.get 0
                local.get 1
                local.get 2
                memory.atomic.wait32 $m))
            "#,
            6,
        );
        assert!(std::ptr::fn_addr_eq(
            unshared,
            vm::op_memory_atomic_wait32_indexed_unshared as crate::common::Op
        ));
    }

    #[cfg(feature = "threads")]
    #[test]
    fn parser_specializes_indexed_shared_atomic_wait_handler() {
        let shared = op_at(
            r#"
            (module
              (memory 1)
              (memory $m 1 2 shared)
              (func (export "f") (param i32 i32 i64) (result i32)
                local.get 0
                local.get 1
                local.get 2
                memory.atomic.wait32 $m))
            "#,
            6,
        );
        assert!(std::ptr::fn_addr_eq(
            shared,
            vm::op_memory_atomic_wait32_indexed_shared as crate::common::Op
        ));
    }

    #[cfg(feature = "threads")]
    #[test]
    fn parser_specializes_atomic_notify_handler() {
        let unshared = op_at(
            r#"
            (module
              (memory 1)
              (func (export "f") (param i32 i32) (result i32)
                local.get 0
                local.get 1
                memory.atomic.notify))
            "#,
            4,
        );
        let shared = op_at(
            r#"
            (module
              (memory 1 2 shared)
              (func (export "f") (param i32 i32) (result i32)
                local.get 0
                local.get 1
                memory.atomic.notify))
            "#,
            4,
        );
        assert!(std::ptr::fn_addr_eq(
            unshared,
            vm::op_memory_atomic_notify_unshared as crate::common::Op
        ));
        assert!(std::ptr::fn_addr_eq(
            shared,
            vm::op_memory_atomic_notify_shared as crate::common::Op
        ));
    }

    #[cfg(feature = "threads")]
    #[test]
    fn parser_specializes_indexed_unshared_atomic_notify_handler() {
        let unshared = op_at(
            r#"
            (module
              (memory 1)
              (memory $m 1)
              (func (export "f") (param i32 i32) (result i32)
                local.get 0
                local.get 1
                memory.atomic.notify $m))
            "#,
            4,
        );
        assert!(std::ptr::fn_addr_eq(
            unshared,
            vm::op_memory_atomic_notify_indexed_unshared as crate::common::Op
        ));
    }

    #[cfg(feature = "threads")]
    #[test]
    fn parser_specializes_indexed_shared_atomic_notify_handler() {
        let shared = op_at(
            r#"
            (module
              (memory 1)
              (memory $m 1 2 shared)
              (func (export "f") (param i32 i32) (result i32)
                local.get 0
                local.get 1
                memory.atomic.notify $m))
            "#,
            4,
        );
        assert!(std::ptr::fn_addr_eq(
            shared,
            vm::op_memory_atomic_notify_indexed_shared as crate::common::Op
        ));
    }
}
